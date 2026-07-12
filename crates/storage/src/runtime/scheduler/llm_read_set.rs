use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    graph::{
        DraftNodeKind, GraphNode, InputSelector, MemoryReadConsistency, StaticMemoryReadSource,
    },
    selector,
};

use crate::{
    StorageError, StorageResult,
    context::query::load_context,
    graph::helpers::{new_id, put_inline_object, sql},
};

use super::{
    events::add_object_ref,
    long_term_read,
    read_set::{ResolvedBinding, ResolvedSelection},
};

pub(super) async fn resolve_llm_reads<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    attempt_id: &str,
    node: &GraphNode,
    now: i64,
) -> StorageResult<()> {
    let DraftNodeKind::Llm { config } = &node.kind else {
        return Ok(());
    };
    let Some(memory) = &config.memory else {
        return Ok(());
    };
    if memory.node.reads.is_empty() {
        return Ok(());
    }
    let run = connection
        .query_one(sql(
            "SELECT context_id, branch_id FROM graph_runs WHERE id = ?",
            vec![run_id.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::Integrity("LLM run binding missing".into()))?;
    let context_id: String = run.try_get("", "context_id")?;
    let branch_id: String = run.try_get("", "branch_id")?;
    let context = load_context(connection, &context_id, &branch_id).await?;
    for read in &memory.node.reads {
        let resolved = match &read.source {
            StaticMemoryReadSource::WorkingContext { path, .. } => {
                resolve_working(read, path, &context_id, &branch_id, &context)?
            }
            StaticMemoryReadSource::LongTermMemory { .. } => {
                long_term_read::resolve_static(connection, read).await?
            }
            StaticMemoryReadSource::Artifact { .. } => {
                return Err(StorageError::InputContract(format!(
                    "artifact context binding '{}' is not available in phase one storage",
                    read.id
                )));
            }
        };
        persist_binding(connection, attempt_id, read, resolved, now).await?;
    }
    Ok(())
}

fn resolve_working(
    read: &zhuangsheng_core::graph::StaticMemoryRead,
    path: &str,
    context_id: &str,
    branch_id: &str,
    context: &zhuangsheng_core::application::context::WorkingContextView,
) -> StorageResult<ResolvedBinding> {
    let selected = selector::select(
        &InputSelector::JsonPointer {
            pointer: path.into(),
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
                "required LLM memory binding '{}' did not resolve",
                read.id
            )));
        }
    };
    Ok(ResolvedBinding {
        envelope,
        selections: vec![ResolvedSelection {
            aggregate_kind: "working_context",
            aggregate_id: context_id.into(),
            lineage_key: branch_id.into(),
            commit_id: context.head_commit_id.clone(),
            selection_ordinal: None,
            content_hash,
        }],
        scope_snapshot_token: None,
        truncated: false,
    })
}

async fn persist_binding<C: ConnectionTrait>(
    connection: &C,
    attempt_id: &str,
    read: &zhuangsheng_core::graph::StaticMemoryRead,
    resolved: ResolvedBinding,
    now: i64,
) -> StorageResult<()> {
    let envelope_bytes = canonical::to_vec(&resolved.envelope)?;
    if envelope_bytes.len() as u64 > read.max_bytes {
        return Err(StorageError::InputContract(format!(
            "LLM memory binding '{}' exceeds maxBytes",
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
        connection.execute(sql(
            "INSERT INTO node_read_set (id, node_attempt_id, aggregate_kind, aggregate_id, lineage_key, commit_id, binding_id, selection_ordinal, selected_content_hash, consistency) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            vec![new_id("readset").into(), attempt_id.into(), selection.aggregate_kind.into(), selection.aggregate_id.into(), selection.lineage_key.into(), selection.commit_id.into(), read.id.clone().into(), selection.selection_ordinal.into(), selection.content_hash.into(), consistency(read.consistency).into()],
        )).await?;
    }
    connection.execute(sql(
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
    Ok(())
}

fn consistency(value: MemoryReadConsistency) -> &'static str {
    match value {
        MemoryReadConsistency::Snapshot => "snapshot",
        MemoryReadConsistency::ValidateOnCommit => "validate_on_commit",
    }
}
