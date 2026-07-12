use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    application::memory::MemorySearchCommand,
    canonical,
    graph::{
        MemoryQuery, MemoryRecordStatus, RouterReadBinding, RouterReadSource, StaticMemoryRead,
        StaticMemoryReadSource,
    },
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
    resolve_parts(
        connection,
        &read.id,
        read.required,
        read.limit,
        read.max_bytes,
        scope,
        query.as_ref(),
    )
    .await
}

pub(super) async fn resolve_static<C: ConnectionTrait>(
    connection: &C,
    read: &StaticMemoryRead,
) -> StorageResult<ResolvedBinding> {
    let StaticMemoryReadSource::LongTermMemory { scope, query } = &read.source else {
        return Err(StorageError::Integrity(
            "long-term read resolver received another static source".into(),
        ));
    };
    resolve_parts(
        connection,
        &read.id,
        read.required,
        read.limit,
        read.max_bytes,
        scope,
        query.as_ref(),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn resolve_parts<C: ConnectionTrait>(
    connection: &C,
    binding_id: &str,
    required: bool,
    limit: Option<u32>,
    max_bytes: u64,
    scope: &str,
    query: Option<&MemoryQuery>,
) -> StorageResult<ResolvedBinding> {
    let mut search = MemorySearchCommand {
        scope_id: scope.to_owned(),
        text: query.map(|query| query.text.clone()),
        tags: query.map(|query| query.tags.clone()).unwrap_or_default(),
        status: query
            .and_then(|query| query.status)
            .map(|status| match status {
                MemoryRecordStatus::Active => LongTermMemoryStatus::Active,
                MemoryRecordStatus::Obsolete => LongTermMemoryStatus::Obsolete,
            }),
        limit: limit.unwrap_or(20),
    };
    let result = search_in(connection, &mut search).await?;
    if result.records.is_empty() && required {
        return Err(StorageError::InputContract(format!(
            "required memory binding '{binding_id}' returned no records",
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
            .query_one_raw(sql(
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
            "tags":content.tags,
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
        if canonical::to_vec(&envelope)?.len() as u64 <= max_bytes {
            return Ok(ResolvedBinding {
                envelope,
                selections,
                scope_snapshot_token: Some(result.scope_snapshot_token),
                truncated,
            });
        }
        if records.pop().is_none() {
            return Err(StorageError::InputContract(format!(
                "memory binding '{binding_id}' exceeds maxBytes",
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
        .query_one_raw(sql(
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
