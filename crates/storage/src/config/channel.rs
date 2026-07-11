use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::json;
use zhuangsheng_core::{
    application::channel::{ChannelView, CreateChannelCommand, PublishChannelRevisionCommand},
    canonical,
    llm::{
        ChannelCredential, LlmChannelRevision, normalize_channel_revision, revision_content_hash,
    },
};

use crate::{SqliteStore, StorageError, StorageResult, graph::helpers::*};

use super::rows::{canonical_json, channel_from_row, load_channel_revision};

impl SqliteStore {
    pub async fn create_channel(
        &self,
        command: CreateChannelCommand,
    ) -> StorageResult<ChannelView> {
        let name = valid_name(&command.name)?;
        require_key(&command.idempotency_key)?;
        let scope = "workspace:local:channels:create";
        let digest = canonical::hash(&json!({"command":"create_channel","name":name}))?;
        let transaction = self.db.begin().await?;
        if let Some(receipt) = find_receipt(&transaction, scope, &command.idempotency_key).await? {
            if receipt.digest != digest {
                return Err(StorageError::IdempotencyConflict);
            }
            let result =
                load_object_json(&transaction, &required_result(receipt.result_object_id)?).await?;
            transaction.commit().await?;
            return Ok(result);
        }
        let id = new_id("channel");
        let now = now_ms();
        insert_pending_receipt(
            &transaction,
            scope,
            &command.idempotency_key,
            &digest,
            &id,
            now,
        )
        .await?;
        transaction.execute(sql(
            "INSERT INTO llm_channels (id, name, head_revision_id, created_at, updated_at) VALUES (?, ?, NULL, ?, ?)",
            vec![id.clone().into(), name.clone().into(), now.into(), now.into()],
        )).await?;
        let view = ChannelView {
            id,
            name,
            head_revision_id: None,
            created_at: now,
            updated_at: now,
        };
        finish_receipt(&transaction, scope, &command.idempotency_key, &view, now).await?;
        transaction.commit().await?;
        Ok(view)
    }

    pub async fn list_channels(&self) -> StorageResult<Vec<ChannelView>> {
        self.db.query_all(sql(
            "SELECT id, name, head_revision_id, created_at, updated_at FROM llm_channels ORDER BY created_at, id",
            vec![],
        )).await?.iter().map(channel_from_row).collect()
    }

    pub async fn get_channel(&self, channel_id: &str) -> StorageResult<ChannelView> {
        let row = self.db.query_one(sql(
            "SELECT id, name, head_revision_id, created_at, updated_at FROM llm_channels WHERE id = ?",
            vec![channel_id.into()],
        )).await?.ok_or_else(|| StorageError::NotFound { kind: "channel", id: channel_id.into() })?;
        channel_from_row(&row)
    }

    pub async fn publish_channel_revision(
        &self,
        command: PublishChannelRevisionCommand,
    ) -> StorageResult<LlmChannelRevision> {
        require_key(&command.idempotency_key)?;
        let spec = normalize_channel_revision(command.spec).map_err(StorageError::LlmConfig)?;
        let content_hash = revision_content_hash(&spec).map_err(StorageError::LlmConfig)?;
        let scope = format!("workspace:local:channels:{}:publish", command.channel_id);
        let digest = canonical::hash(&json!({
            "command":"publish_channel_revision",
            "channelId":command.channel_id,
            "expectedHeadRevisionId":command.expected_head_revision_id,
            "contentHash":content_hash,
        }))?;
        let transaction = self.db.begin().await?;
        if let Some(receipt) = find_receipt(&transaction, &scope, &command.idempotency_key).await? {
            if receipt.digest != digest {
                return Err(StorageError::IdempotencyConflict);
            }
            let result =
                load_object_json(&transaction, &required_result(receipt.result_object_id)?).await?;
            transaction.commit().await?;
            return Ok(result);
        }
        let row = transaction
            .query_one(sql(
                "SELECT head_revision_id FROM llm_channels WHERE id = ?",
                vec![command.channel_id.clone().into()],
            ))
            .await?
            .ok_or_else(|| StorageError::NotFound {
                kind: "channel",
                id: command.channel_id.clone(),
            })?;
        let head: Option<String> = row.try_get("", "head_revision_id")?;
        if head != command.expected_head_revision_id {
            return Err(StorageError::Conflict("channel_head_conflict"));
        }
        let revision_no = next_revision_no(&transaction, &command.channel_id).await?;
        let id = new_id("channelrev");
        let now = now_ms();
        insert_pending_receipt(
            &transaction,
            &scope,
            &command.idempotency_key,
            &digest,
            &id,
            now,
        )
        .await?;
        insert_revision(
            &transaction,
            &id,
            &command.channel_id,
            revision_no,
            &spec,
            &content_hash,
            now,
        )
        .await?;
        transaction
            .execute(sql(
                "UPDATE llm_channels SET head_revision_id = ?, updated_at = ? WHERE id = ?",
                vec![
                    id.clone().into(),
                    now.into(),
                    command.channel_id.clone().into(),
                ],
            ))
            .await?;
        let revision = LlmChannelRevision {
            id,
            channel_id: command.channel_id,
            revision_no,
            spec,
            content_hash,
            created_at: now,
        };
        finish_receipt(
            &transaction,
            &scope,
            &command.idempotency_key,
            &revision,
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(revision)
    }

    pub async fn get_channel_revision(
        &self,
        revision_id: &str,
    ) -> StorageResult<LlmChannelRevision> {
        load_channel_revision(&self.db, revision_id).await
    }

    pub async fn get_channel_head_revision(
        &self,
        channel_id: &str,
    ) -> StorageResult<LlmChannelRevision> {
        let channel = self.get_channel(channel_id).await?;
        let revision_id = channel
            .head_revision_id
            .ok_or_else(|| StorageError::Conflict("channel_has_no_revision"))?;
        self.get_channel_revision(&revision_id).await
    }
}

async fn next_revision_no<C: ConnectionTrait>(
    connection: &C,
    channel_id: &str,
) -> StorageResult<u64> {
    let row = connection.query_one(sql(
        "SELECT COALESCE(MAX(revision_no), 0) + 1 AS next_no FROM llm_channel_revisions WHERE channel_id = ?",
        vec![channel_id.into()],
    )).await?.ok_or_else(|| StorageError::Integrity("missing channel revision sequence".into()))?;
    let value: i64 = row.try_get("", "next_no")?;
    u64::try_from(value)
        .map_err(|_| StorageError::Integrity("invalid channel revision number".into()))
}

async fn insert_revision<C: ConnectionTrait>(
    connection: &C,
    id: &str,
    channel_id: &str,
    revision_no: u64,
    spec: &zhuangsheng_core::llm::LlmChannelRevisionSpec,
    content_hash: &str,
    now: i64,
) -> StorageResult<()> {
    let (credential_kind, api_key_ref) = match &spec.credential {
        ChannelCredential::Secret { api_key_ref } => ("secret", Some(canonical_json(api_key_ref)?)),
        ChannelCredential::None => ("none", None),
    };
    connection.execute(sql(
        "INSERT INTO llm_channel_revisions (id, channel_id, revision_no, operation_taxonomy_version, adapter_decoder_version, base_url, transport_policy_json, credential_kind, api_key_ref, operation_keys_json, model_catalogs_json, capabilities_json, content_hash, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        vec![
            id.into(), channel_id.into(), (revision_no as i64).into(),
            (spec.operation_taxonomy_version as i64).into(), (spec.adapter_decoder_version as i64).into(),
            spec.base_url.clone().into(), canonical_json(&spec.transport_policy)?.into(), credential_kind.into(), api_key_ref.into(),
            canonical_json(&spec.operation_keys)?.into(), canonical_json(&spec.model_catalogs)?.into(), canonical_json(&spec.capabilities)?.into(),
            content_hash.into(), now.into(),
        ],
    )).await?;
    Ok(())
}

pub(super) fn valid_name(raw: &str) -> StorageResult<String> {
    let name = raw.trim();
    if name.is_empty() || name.len() > 200 {
        return Err(StorageError::InvalidArgument(
            "name must contain 1..=200 bytes".into(),
        ));
    }
    Ok(name.into())
}

pub(super) fn require_key(key: &str) -> StorageResult<()> {
    if key.trim().is_empty() || key.len() > 256 {
        return Err(StorageError::InvalidArgument(
            "invalid idempotency key".into(),
        ));
    }
    Ok(())
}

pub(super) fn required_result(value: Option<String>) -> StorageResult<String> {
    value.ok_or_else(|| StorageError::Integrity("completed receipt has no result object".into()))
}

pub(super) async fn insert_pending_receipt<C: ConnectionTrait>(
    connection: &C,
    scope: &str,
    key: &str,
    digest: &str,
    resource_id: &str,
    now: i64,
) -> StorageResult<()> {
    connection.execute(sql(
        "INSERT INTO application_command_receipts (scope, idempotency_key, request_digest, command_kind, resource_kind, resource_id, status, created_at) VALUES (?, ?, ?, 'publish_config', 'config', ?, 'pending', ?)",
        vec![scope.into(), key.into(), digest.into(), resource_id.into(), now.into()],
    )).await?;
    Ok(())
}

pub(super) async fn finish_receipt<C: ConnectionTrait, T: serde::Serialize>(
    connection: &C,
    scope: &str,
    key: &str,
    result: &T,
    now: i64,
) -> StorageResult<()> {
    let object_id = put_inline_object(connection, &canonical::to_vec(result)?, now).await?;
    connection.execute(sql(
        "UPDATE application_command_receipts SET status = 'completed', result_object_id = ?, completed_at = ? WHERE scope = ? AND idempotency_key = ?",
        vec![object_id.into(), now.into(), scope.into(), key.into()],
    )).await?;
    Ok(())
}
