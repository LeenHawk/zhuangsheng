use std::collections::BTreeMap;

use sea_orm::ConnectionTrait;
use serde_json::{Value, json};
use zhuangsheng_core::{
    canonical,
    graph::{DraftNodeKind, GraphNode, InputSelector, MemoryReadConsistency, RouterReadSource},
    selector,
};

use crate::{
    StorageError, StorageResult,
    context::query::load_context,
    graph::helpers::{load_object_json, new_id, put_inline_object, sql},
};

use super::{events::add_object_ref, long_term_read};

pub(super) struct ResolvedSelection {
    pub aggregate_kind: &'static str,
    pub aggregate_id: String,
    pub lineage_key: String,
    pub commit_id: String,
    pub selection_ordinal: Option<i64>,
    pub content_hash: Option<String>,
}

pub(super) struct ResolvedBinding {
    pub envelope: Value,
    pub selections: Vec<ResolvedSelection>,
    pub scope_snapshot_token: Option<String>,
    pub truncated: bool,
}

pub(super) async fn resolve_router_reads<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    attempt_id: &str,
    node: &GraphNode,
    now: i64,
) -> StorageResult<()> {
    let DraftNodeKind::Router {
        memory: Some(memory),
        ..
    } = &node.kind
    else {
        return Ok(());
    };
    if memory.reads.is_empty() {
        return Ok(());
    }
    let run = connection
        .query_one_raw(sql(
            "SELECT context_id, branch_id FROM graph_runs WHERE id = ?",
            vec![run_id.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::Integrity("Router run binding missing".into()))?;
    let context_id: String = run.try_get("", "context_id")?;
    let branch_id: String = run.try_get("", "branch_id")?;
    let context = load_context(connection, &context_id, &branch_id).await?;
    for read in &memory.reads {
        let resolved = match &read.source {
            RouterReadSource::WorkingContext { path, .. } => {
                let selected = selector::select(
                    &InputSelector::JsonPointer {
                        pointer: path.clone(),
                    },
                    &context.value,
                    1,
                );
                let (envelope, content_hash) = match selected {
                    Ok(value) => {
                        let hash = canonical::hash(&value)?;
                        (
                            json!({
                                "kind":"working_context",
                                "found":true,
                                "commitId":context.head_commit_id,
                                "value":value,
                            }),
                            Some(hash),
                        )
                    }
                    Err(_) if !read.required => (
                        json!({
                            "kind":"working_context",
                            "found":false,
                            "commitId":context.head_commit_id,
                        }),
                        None,
                    ),
                    Err(_) => {
                        return Err(StorageError::InputContract(format!(
                            "required Router memory binding '{}' did not resolve",
                            read.id
                        )));
                    }
                };
                ResolvedBinding {
                    envelope,
                    selections: vec![ResolvedSelection {
                        aggregate_kind: "working_context",
                        aggregate_id: context_id.clone(),
                        lineage_key: branch_id.clone(),
                        commit_id: context.head_commit_id.clone(),
                        selection_ordinal: None,
                        content_hash,
                    }],
                    scope_snapshot_token: None,
                    truncated: false,
                }
            }
            RouterReadSource::LongTermMemory { .. } => {
                long_term_read::resolve(connection, read).await?
            }
        };
        let envelope_bytes = canonical::to_vec(&resolved.envelope)?;
        if envelope_bytes.len() as u64 > read.max_bytes {
            return Err(StorageError::InputContract(format!(
                "Router memory binding '{}' exceeds maxBytes",
                read.id
            )));
        }
        let digest = canonical::hash_bytes(&envelope_bytes);
        let mut result = json!({
            "bindingId":read.id,
            "envelope":resolved.envelope,
            "envelopeDigest":digest,
        });
        if let Some(token) = &resolved.scope_snapshot_token {
            result
                .as_object_mut()
                .expect("bound result object")
                .insert("scopeSnapshotToken".into(), token.clone().into());
        }
        let object_id = put_inline_object(connection, &canonical::to_vec(&result)?, now).await?;
        for selection in resolved.selections {
            connection.execute_raw(sql(
                "INSERT INTO node_read_set (id, node_attempt_id, aggregate_kind, aggregate_id, lineage_key, commit_id, binding_id, selection_ordinal, selected_content_hash, consistency) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                vec![new_id("readset").into(), attempt_id.into(), selection.aggregate_kind.into(), selection.aggregate_id.into(), selection.lineage_key.into(), selection.commit_id.into(), read.id.clone().into(), selection.selection_ordinal.into(), selection.content_hash.into(), consistency(read.consistency).into()],
            )).await?;
        }
        connection.execute_raw(sql(
            "INSERT INTO node_bound_read_results (node_attempt_id, binding_id, envelope_object_id, result_digest, scope_snapshot_token, truncated) VALUES (?, ?, ?, ?, ?, ?)",
            vec![attempt_id.into(), read.id.clone().into(), object_id.clone().into(), digest.into(), resolved.scope_snapshot_token.into(), i64::from(resolved.truncated).into()],
        )).await?;
        add_object_ref(
            connection,
            &object_id,
            "node_attempt",
            attempt_id,
            &format!("bound_read:{}", read.id),
            now,
        )
        .await?;
    }
    Ok(())
}

pub(super) async fn load_router_memory<C: ConnectionTrait>(
    connection: &C,
    attempt_id: &str,
    node: &GraphNode,
) -> StorageResult<BTreeMap<String, Value>> {
    let DraftNodeKind::Router {
        memory: Some(memory),
        ..
    } = &node.kind
    else {
        return Ok(BTreeMap::new());
    };
    let mut values = BTreeMap::new();
    for read in &memory.reads {
        let row = connection.query_one_raw(sql(
            "SELECT envelope_object_id, result_digest FROM node_bound_read_results WHERE node_attempt_id = ? AND binding_id = ?",
            vec![attempt_id.into(), read.id.clone().into()],
        )).await?.ok_or_else(|| StorageError::Integrity("bound Router read result missing".into()))?;
        let object_id: String = row.try_get("", "envelope_object_id")?;
        let expected: String = row.try_get("", "result_digest")?;
        let result: Value = load_object_json(connection, &object_id).await?;
        let envelope = result
            .get("envelope")
            .cloned()
            .ok_or_else(|| StorageError::Integrity("bound read envelope missing".into()))?;
        if result.get("bindingId").and_then(Value::as_str) != Some(&read.id)
            || canonical::hash(&envelope)? != expected
        {
            return Err(StorageError::Integrity(
                "bound Router read result digest mismatch".into(),
            ));
        }
        values.insert(read.alias.clone(), envelope);
    }
    Ok(values)
}

fn consistency(value: MemoryReadConsistency) -> &'static str {
    match value {
        MemoryReadConsistency::Snapshot => "snapshot",
        MemoryReadConsistency::ValidateOnCommit => "validate_on_commit",
    }
}

pub(crate) async fn copy_attempt_reads<C: ConnectionTrait>(
    connection: &C,
    source_attempt_id: &str,
    target_attempt_id: &str,
    now: i64,
) -> StorageResult<()> {
    let entries = connection.query_all_raw(sql(
        "SELECT aggregate_kind, aggregate_id, lineage_key, commit_id, binding_id, selection_ordinal, selected_content_hash, consistency FROM node_read_set WHERE node_attempt_id = ? ORDER BY binding_id, selection_ordinal",
        vec![source_attempt_id.into()],
    )).await?;
    for entry in entries {
        let aggregate_kind: String = entry.try_get("", "aggregate_kind")?;
        let aggregate_id: String = entry.try_get("", "aggregate_id")?;
        let lineage_key: String = entry.try_get("", "lineage_key")?;
        let commit_id: String = entry.try_get("", "commit_id")?;
        let binding_id: String = entry.try_get("", "binding_id")?;
        let ordinal: Option<i64> = entry.try_get("", "selection_ordinal")?;
        let content_hash: Option<String> = entry.try_get("", "selected_content_hash")?;
        let consistency: String = entry.try_get("", "consistency")?;
        connection.execute_raw(sql(
            "INSERT INTO node_read_set (id, node_attempt_id, aggregate_kind, aggregate_id, lineage_key, commit_id, binding_id, selection_ordinal, selected_content_hash, consistency) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            vec![new_id("readset").into(), target_attempt_id.into(), aggregate_kind.into(), aggregate_id.into(), lineage_key.into(), commit_id.into(), binding_id.into(), ordinal.into(), content_hash.into(), consistency.into()],
        )).await?;
    }
    let results = connection.query_all_raw(sql(
        "SELECT binding_id, envelope_object_id, result_digest, scope_snapshot_token, truncated FROM node_bound_read_results WHERE node_attempt_id = ? ORDER BY binding_id",
        vec![source_attempt_id.into()],
    )).await?;
    for result in results {
        let binding_id: String = result.try_get("", "binding_id")?;
        let object_id: String = result.try_get("", "envelope_object_id")?;
        let digest: String = result.try_get("", "result_digest")?;
        let token: Option<String> = result.try_get("", "scope_snapshot_token")?;
        let truncated: i64 = result.try_get("", "truncated")?;
        connection.execute_raw(sql(
            "INSERT INTO node_bound_read_results (node_attempt_id, binding_id, envelope_object_id, result_digest, scope_snapshot_token, truncated) VALUES (?, ?, ?, ?, ?, ?)",
            vec![target_attempt_id.into(), binding_id.clone().into(), object_id.clone().into(), digest.into(), token.into(), truncated.into()],
        )).await?;
        add_object_ref(
            connection,
            &object_id,
            "node_attempt",
            target_attempt_id,
            &format!("bound_read:{binding_id}"),
            now,
        )
        .await?;
    }
    Ok(())
}
