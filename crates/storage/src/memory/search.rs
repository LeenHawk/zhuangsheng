use sea_orm::{ConnectionTrait, TransactionTrait};
use zhuangsheng_core::application::memory::{MemorySearchCommand, MemorySearchView};
use zhuangsheng_core::memory::LongTermMemoryStatus;

use crate::{SqliteStore, StorageError, StorageResult, graph::helpers::sql};

use super::query::load_record;

const MAX_CANDIDATES: i64 = 1000;

impl SqliteStore {
    pub async fn search_memory(
        &self,
        mut command: MemorySearchCommand,
    ) -> StorageResult<MemorySearchView> {
        let transaction = self.db.begin().await?;
        let result = search_in(&transaction, &mut command).await?;
        transaction.commit().await?;
        Ok(result)
    }
}

pub(crate) async fn search_in<C: ConnectionTrait>(
    connection: &C,
    command: &mut MemorySearchCommand,
) -> StorageResult<MemorySearchView> {
    validate(command)?;
    let scope = connection
        .query_one(sql(
            "SELECT revision_no FROM memory_scopes WHERE id = ?",
            vec![command.scope_id.clone().into()],
        ))
        .await?
        .ok_or_else(|| StorageError::NotFound {
            kind: "memory_scope",
            id: command.scope_id.clone(),
        })?;
    let revision: i64 = scope.try_get("", "revision_no")?;
    let candidate_ids = candidates(connection, command).await?;
    let mut records = Vec::new();
    let requested_status = command.status.unwrap_or(LongTermMemoryStatus::Active);
    for memory_id in candidate_ids {
        let record = load_record(connection, &memory_id).await?;
        let status = match record.status {
            LongTermMemoryStatus::Active => LongTermMemoryStatus::Active,
            LongTermMemoryStatus::Obsolete => LongTermMemoryStatus::Obsolete,
            _ => continue,
        };
        if status != requested_status {
            continue;
        }
        let Some(content) = &record.content else {
            continue;
        };
        if command
            .tags
            .iter()
            .all(|required| content.tags.binary_search(required).is_ok())
        {
            records.push(record);
            if records.len() > command.limit as usize {
                break;
            }
        }
    }
    let truncated = records.len() > command.limit as usize;
    records.truncate(command.limit as usize);
    Ok(MemorySearchView {
        records,
        truncated,
        scope_snapshot_token: format!("memory-scope:{}:revision:{revision}", command.scope_id),
    })
}

async fn candidates<C: ConnectionTrait>(
    connection: &C,
    command: &MemorySearchCommand,
) -> StorageResult<Vec<String>> {
    let rows = if let Some(text) = command.text.as_deref() {
        connection.query_all(sql(
            "SELECT f.memory_id FROM memory_search f JOIN memory_records r ON r.id = f.memory_id WHERE memory_search MATCH ? AND f.scope_id = ? AND r.status IN ('active','obsolete') ORDER BY bm25(memory_search), f.memory_id LIMIT ?",
            vec![fts_query(text).into(), command.scope_id.clone().into(), MAX_CANDIDATES.into()],
        )).await?
    } else {
        connection.query_all(sql(
            "SELECT id AS memory_id FROM memory_records WHERE scope_id = ? AND status IN ('active','obsolete') ORDER BY id LIMIT ?",
            vec![command.scope_id.clone().into(), MAX_CANDIDATES.into()],
        )).await?
    };
    rows.iter()
        .map(|row| row.try_get("", "memory_id").map_err(Into::into))
        .collect()
}

fn validate(command: &mut MemorySearchCommand) -> StorageResult<()> {
    if command.scope_id.is_empty()
        || command.limit == 0
        || command.limit > 100
        || command
            .text
            .as_ref()
            .is_some_and(|text| text.trim().is_empty() || text.len() > 4096)
        || !matches!(
            command.status,
            None | Some(LongTermMemoryStatus::Active | LongTermMemoryStatus::Obsolete)
        )
        || command.tags.len() > 64
        || command
            .tags
            .iter()
            .any(|tag| tag.is_empty() || tag.len() > 256)
    {
        return Err(StorageError::InvalidArgument(
            "memory search parameters are invalid".into(),
        ));
    }
    command.tags.sort();
    command.tags.dedup();
    Ok(())
}

fn fts_query(text: &str) -> String {
    text.split_whitespace()
        .map(|token| format!("\"{}\"", token.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND ")
}
