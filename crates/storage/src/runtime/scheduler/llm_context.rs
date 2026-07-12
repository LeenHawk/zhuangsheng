use std::collections::BTreeMap;

use sea_orm::ConnectionTrait;
use serde_json::{Value, json};
use zhuangsheng_core::{
    canonical,
    graph::{DraftNodeKind, GraphNode, StaticMemoryRead, StaticMemoryReadSource},
    llm::{
        context::{ContextProvenance, ContextRole, ResolvedContextBinding, ResolvedContextValue},
        ir::{ContextSensitivity, ContextTrust, LlmContentPartIr},
    },
    scheduler::ClaimedContextSnapshot,
};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{load_object_json, sql},
};

pub(super) async fn load_llm_context_snapshot<C: ConnectionTrait>(
    connection: &C,
    attempt_id: &str,
    node_instance_id: &str,
    node: &GraphNode,
) -> StorageResult<Option<ClaimedContextSnapshot>> {
    let DraftNodeKind::Llm { config } = &node.kind else {
        return Ok(None);
    };
    let reads = config
        .memory
        .as_ref()
        .map(|memory| memory.node.reads.as_slice())
        .unwrap_or_default();
    let mut bindings = BTreeMap::new();
    for read in reads {
        let binding = load_binding(connection, attempt_id, read).await?;
        bindings.insert(read.id.clone(), binding);
    }
    let read_set_digest = compute_llm_read_set_digest(connection, attempt_id).await?;
    Ok(Some(ClaimedContextSnapshot {
        bindings,
        read_set_ref: format!("node-instance:{node_instance_id}:read-set:v1"),
        read_set_digest,
    }))
}

async fn load_binding<C: ConnectionTrait>(
    connection: &C,
    attempt_id: &str,
    read: &StaticMemoryRead,
) -> StorageResult<ResolvedContextBinding> {
    let row = connection
        .query_one_raw(sql(
            "SELECT envelope_object_id, result_digest, scope_snapshot_token FROM node_bound_read_results WHERE node_attempt_id = ? AND binding_id = ?",
            vec![attempt_id.into(), read.id.clone().into()],
        ))
        .await?
        .ok_or_else(|| StorageError::Integrity("bound LLM read result missing".into()))?;
    let object_id: String = row.try_get("", "envelope_object_id")?;
    let expected: String = row.try_get("", "result_digest")?;
    let snapshot_token: Option<String> = row.try_get("", "scope_snapshot_token")?;
    let result: Value = load_object_json(connection, &object_id).await?;
    let envelope = result
        .get("envelope")
        .cloned()
        .ok_or_else(|| StorageError::Integrity("bound LLM read envelope missing".into()))?;
    if result.get("bindingId").and_then(Value::as_str) != Some(&read.id)
        || canonical::hash(&envelope)? != expected
    {
        return Err(StorageError::Integrity(
            "bound LLM read result digest mismatch".into(),
        ));
    }
    match &read.source {
        StaticMemoryReadSource::WorkingContext { scope, .. } => {
            working_binding(read, scope, envelope)
        }
        StaticMemoryReadSource::ConversationHistory { scope } => {
            conversation_history_binding(read, scope, envelope)
        }
        StaticMemoryReadSource::LongTermMemory { scope, .. } => {
            long_term_binding(read, scope, snapshot_token, envelope)
        }
        StaticMemoryReadSource::Artifact { .. } => Err(StorageError::InputContract(format!(
            "artifact context binding '{}' is not available in phase one storage",
            read.id
        ))),
    }
}

fn conversation_history_binding(
    read: &StaticMemoryRead,
    scope: &str,
    envelope: Value,
) -> StorageResult<ResolvedContextBinding> {
    if envelope.get("kind").and_then(Value::as_str) != Some("conversation_history") {
        return Err(StorageError::Integrity(
            "conversation history binding kind mismatch".into(),
        ));
    }
    let version = required_string(&envelope, "commitId")?;
    let values: Vec<ResolvedContextValue> =
        serde_json::from_value(envelope.get("values").cloned().ok_or_else(|| {
            StorageError::Integrity("conversation history values missing".into())
        })?)
        .map_err(|error| StorageError::Integrity(error.to_string()))?;
    if values
        .iter()
        .any(|value| !matches!(value, ResolvedContextValue::HistoryMessage { .. }))
    {
        return Err(StorageError::Integrity(
            "conversation history contains a non-history value".into(),
        ));
    }
    Ok(ResolvedContextBinding {
        binding_id: read.id.clone(),
        scope: scope.into(),
        version,
        values,
        template_value: None,
        template_provenance: None,
    })
}

fn working_binding(
    read: &StaticMemoryRead,
    scope: &str,
    envelope: Value,
) -> StorageResult<ResolvedContextBinding> {
    let version = required_string(&envelope, "commitId")?;
    let found = envelope
        .get("found")
        .and_then(Value::as_bool)
        .ok_or_else(|| StorageError::Integrity("working binding found flag missing".into()))?;
    let template_value = found.then(|| envelope.get("value").cloned()).flatten();
    let values = if let Some(value) = template_value.as_ref() {
        vec![data_value(
            format!("working:{}", read.id),
            value_content(value)?,
            "working_context",
            &read.id,
            Vec::new(),
        )?]
    } else {
        Vec::new()
    };
    Ok(ResolvedContextBinding {
        binding_id: read.id.clone(),
        scope: scope.into(),
        version,
        values,
        template_value,
        template_provenance: Some(provenance("working_context", &read.id)),
    })
}

fn long_term_binding(
    read: &StaticMemoryRead,
    scope: &str,
    snapshot_token: Option<String>,
    envelope: Value,
) -> StorageResult<ResolvedContextBinding> {
    let version = snapshot_token
        .filter(|value| !value.is_empty())
        .ok_or_else(|| StorageError::Integrity("memory snapshot token missing".into()))?;
    let records = envelope
        .get("records")
        .and_then(Value::as_array)
        .ok_or_else(|| StorageError::Integrity("memory binding records missing".into()))?;
    let mut values = Vec::with_capacity(records.len());
    for record in records {
        let id = required_string(record, "memoryId")?;
        let text = required_string(record, "summary")?;
        let tags = record
            .get("tags")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .map(|value| {
                        value.as_str().map(str::to_owned).ok_or_else(|| {
                            StorageError::Integrity("memory binding tag is not a string".into())
                        })
                    })
                    .collect::<StorageResult<Vec<_>>>()
            })
            .transpose()?
            .unwrap_or_default();
        values.push(data_value(
            id.clone(),
            vec![LlmContentPartIr::Text { text }],
            "long_term_memory",
            &id,
            tags,
        )?);
    }
    Ok(ResolvedContextBinding {
        binding_id: read.id.clone(),
        scope: scope.into(),
        version,
        values,
        template_value: Some(envelope),
        template_provenance: Some(provenance("long_term_memory", &read.id)),
    })
}

fn data_value(
    id: String,
    content: Vec<LlmContentPartIr>,
    source_type: &str,
    source_id: &str,
    tags: Vec<String>,
) -> StorageResult<ResolvedContextValue> {
    Ok(ResolvedContextValue::Data {
        id,
        content_hash: canonical::hash(&content)?,
        content,
        provenance: provenance(source_type, source_id),
        allowed_roles: vec![ContextRole::Context],
        relevance_score_micros: None,
        tags,
    })
}

fn value_content(value: &Value) -> StorageResult<Vec<LlmContentPartIr>> {
    let text = match value {
        Value::String(value) => value.clone(),
        _ => canonical::to_string(value)?,
    };
    Ok(vec![LlmContentPartIr::Text { text }])
}

fn provenance(source_type: &str, source_id: &str) -> ContextProvenance {
    ContextProvenance {
        source_type: source_type.into(),
        source_id: source_id.into(),
        trust: ContextTrust::ExternalUntrusted,
        sensitivity: ContextSensitivity::Private,
    }
}

fn required_string(value: &Value, key: &str) -> StorageResult<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| StorageError::Integrity(format!("bound LLM read field missing: {key}")))
}

pub(crate) async fn compute_llm_read_set_digest<C: ConnectionTrait>(
    connection: &C,
    attempt_id: &str,
) -> StorageResult<String> {
    let selections = connection.query_all_raw(sql(
        "SELECT aggregate_kind, aggregate_id, lineage_key, commit_id, binding_id, selection_ordinal, selected_content_hash, consistency FROM node_read_set WHERE node_attempt_id = ? ORDER BY binding_id, selection_ordinal, id",
        vec![attempt_id.into()],
    )).await?;
    let mut entries = Vec::with_capacity(selections.len());
    for row in selections {
        entries.push(json!({
            "aggregateKind":row.try_get::<String>("", "aggregate_kind")?,
            "aggregateId":row.try_get::<String>("", "aggregate_id")?,
            "lineageKey":row.try_get::<String>("", "lineage_key")?,
            "commitId":row.try_get::<String>("", "commit_id")?,
            "bindingId":row.try_get::<String>("", "binding_id")?,
            "selectionOrdinal":row.try_get::<Option<i64>>("", "selection_ordinal")?,
            "selectedContentHash":row.try_get::<Option<String>>("", "selected_content_hash")?,
            "consistency":row.try_get::<String>("", "consistency")?,
        }));
    }
    let results = connection.query_all_raw(sql(
        "SELECT binding_id, result_digest, scope_snapshot_token, truncated FROM node_bound_read_results WHERE node_attempt_id = ? ORDER BY binding_id",
        vec![attempt_id.into()],
    )).await?;
    let mut bound = Vec::with_capacity(results.len());
    for row in results {
        bound.push(json!({
            "bindingId":row.try_get::<String>("", "binding_id")?,
            "resultDigest":row.try_get::<String>("", "result_digest")?,
            "scopeSnapshotToken":row.try_get::<Option<String>>("", "scope_snapshot_token")?,
            "truncated":row.try_get::<i64>("", "truncated")? != 0,
        }));
    }
    canonical::hash(&json!({
        "schemaVersion":1,
        "selections":entries,
        "boundResults":bound,
    }))
    .map_err(Into::into)
}
