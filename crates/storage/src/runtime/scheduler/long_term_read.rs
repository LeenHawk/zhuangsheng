use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    application::memory::MemorySearchCommand,
    canonical,
    graph::{MemoryRecordStatus, RouterReadBinding, RouterReadSource},
    memory::LongTermMemoryStatus,
};

use crate::{StorageError, StorageResult, graph::helpers::sql, memory::search_in};

use super::read_set::{ResolvedBinding, ResolvedSelection};

pub(super) async fn resolve<C: ConnectionTrait>(
    connection: &C,
    read: &RouterReadBinding,
) -> StorageResult<ResolvedBinding> {
    let RouterReadSource::LongTermMemory { scope, query } = &read.source else {
        return Err(StorageError::Integrity(
            "long-term read resolver received another source".into(),
        ));
    };
    let mut search = MemorySearchCommand {
        scope_id: scope.clone(),
        text: query.as_ref().map(|query| query.text.clone()),
        tags: query
            .as_ref()
            .map(|query| query.tags.clone())
            .unwrap_or_default(),
        status: query
            .as_ref()
            .and_then(|query| query.status)
            .map(|status| match status {
                MemoryRecordStatus::Active => LongTermMemoryStatus::Active,
                MemoryRecordStatus::Obsolete => LongTermMemoryStatus::Obsolete,
            }),
        limit: read.limit.unwrap_or(20),
    };
    let result = search_in(connection, &mut search).await?;
    if result.records.is_empty() && read.required {
        return Err(StorageError::InputContract(format!(
            "required Router memory binding '{}' returned no records",
            read.id
        )));
    }
    let mut records = Vec::new();
    let mut selections = Vec::new();
    for (ordinal, record) in result.records.into_iter().enumerate() {
        let commit_id = record
            .head_commit_id
            .ok_or_else(|| StorageError::Integrity("memory search record has no head".into()))?;
        let content_ref = record
            .content_ref
            .ok_or_else(|| StorageError::Integrity("memory search record has no content".into()))?;
        let content = record
            .content
            .ok_or_else(|| StorageError::Integrity("memory search content missing".into()))?;
        let row = connection
            .query_one(sql(
                "SELECT content_hash FROM content_objects WHERE id = ? AND lifecycle = 'live'",
                vec![content_ref.into()],
            ))
            .await?
            .ok_or_else(|| StorageError::Integrity("memory content object missing".into()))?;
        let content_hash: String = row.try_get("", "content_hash")?;
        let evidence = evidence_refs(connection, &commit_id).await?;
        records.push(json!({
            "memoryId":record.id.clone(),
            "commitId":commit_id.clone(),
            "contentHash":content_hash.clone(),
            "summary":content.text,
            "evidenceRefs":evidence,
        }));
        selections.push(ResolvedSelection {
            aggregate_kind: "long_term_memory",
            aggregate_id: record.id,
            lineage_key: "global".into(),
            commit_id,
            selection_ordinal: Some(ordinal as i64),
            content_hash: Some(content_hash),
        });
    }
    let mut truncated = result.truncated;
    loop {
        let envelope = json!({
            "kind":"long_term_memory",
            "records":records,
            "truncated":truncated,
        });
        if canonical::to_vec(&envelope)?.len() as u64 <= read.max_bytes {
            return Ok(ResolvedBinding {
                envelope,
                selections,
                scope_snapshot_token: Some(result.scope_snapshot_token),
                truncated,
            });
        }
        if records.pop().is_none() {
            return Err(StorageError::InputContract(format!(
                "Router memory binding '{}' exceeds maxBytes",
                read.id
            )));
        }
        selections.pop();
        truncated = true;
    }
}

async fn evidence_refs<C: ConnectionTrait>(
    connection: &C,
    commit_id: &str,
) -> StorageResult<Vec<String>> {
    let row = connection
        .query_one(sql(
            "SELECT evidence_refs_json FROM memory_change_proposals WHERE applied_commit_id = ?",
            vec![commit_id.into()],
        ))
        .await?;
    match row {
        Some(row) => {
            let json: String = row.try_get("", "evidence_refs_json")?;
            serde_json::from_str(&json).map_err(|error| StorageError::Integrity(error.to_string()))
        }
        None => Ok(Vec::new()),
    }
}
