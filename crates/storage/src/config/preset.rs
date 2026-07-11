use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::json;
use zhuangsheng_core::{
    application::preset::{
        ContextPresetView, CreateContextPresetCommand, PublishContextPresetVersionCommand,
    },
    canonical,
    llm::context::{ContextNormalizationPolicy, ContextPresetVersion, normalize_context_spec},
};

use crate::{SqliteStore, StorageError, StorageResult, graph::helpers::*};

use super::{
    channel::{finish_receipt, insert_pending_receipt, require_key, required_result, valid_name},
    rows::{canonical_json, load_preset_version, preset_from_row},
};

impl SqliteStore {
    pub async fn create_context_preset(
        &self,
        command: CreateContextPresetCommand,
    ) -> StorageResult<ContextPresetView> {
        let name = valid_name(&command.name)?;
        require_key(&command.idempotency_key)?;
        let scope = "workspace:local:context-presets:create";
        let digest = canonical::hash(&json!({"command":"create_context_preset","name":name}))?;
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
        let id = new_id("preset");
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
            "INSERT INTO context_presets (id, name, head_version_id, created_at, updated_at) VALUES (?, ?, NULL, ?, ?)",
            vec![id.clone().into(), name.clone().into(), now.into(), now.into()],
        )).await?;
        let view = ContextPresetView {
            id,
            name,
            head_version_id: None,
            created_at: now,
            updated_at: now,
        };
        finish_receipt(&transaction, scope, &command.idempotency_key, &view, now).await?;
        transaction.commit().await?;
        Ok(view)
    }

    pub async fn list_context_presets(&self) -> StorageResult<Vec<ContextPresetView>> {
        self.db.query_all(sql(
            "SELECT id, name, head_version_id, created_at, updated_at FROM context_presets ORDER BY created_at, id",
            vec![],
        )).await?.iter().map(preset_from_row).collect()
    }

    pub async fn get_context_preset(&self, preset_id: &str) -> StorageResult<ContextPresetView> {
        let row = self.db.query_one(sql(
            "SELECT id, name, head_version_id, created_at, updated_at FROM context_presets WHERE id = ?",
            vec![preset_id.into()],
        )).await?.ok_or_else(|| StorageError::NotFound { kind: "context_preset", id: preset_id.into() })?;
        preset_from_row(&row)
    }

    pub async fn publish_context_preset_version(
        &self,
        command: PublishContextPresetVersionCommand,
    ) -> StorageResult<ContextPresetVersion> {
        require_key(&command.idempotency_key)?;
        let policy = ContextNormalizationPolicy::default();
        let spec =
            normalize_context_spec(command.spec, &policy).map_err(StorageError::LlmConfig)?;
        let content_hash = canonical::hash(&json!({
            "semanticPolicyVersion": policy.semantic_policy_version,
            "spec": spec,
        }))?;
        let scope = format!(
            "workspace:local:context-presets:{}:publish",
            command.preset_id
        );
        let digest = canonical::hash(&json!({
            "command":"publish_context_preset_version",
            "presetId":command.preset_id,
            "expectedHeadVersionId":command.expected_head_version_id,
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
                "SELECT head_version_id FROM context_presets WHERE id = ?",
                vec![command.preset_id.clone().into()],
            ))
            .await?
            .ok_or_else(|| StorageError::NotFound {
                kind: "context_preset",
                id: command.preset_id.clone(),
            })?;
        let head: Option<String> = row.try_get("", "head_version_id")?;
        if head != command.expected_head_version_id {
            return Err(StorageError::Conflict("context_preset_head_conflict"));
        }
        let version_no = next_version_no(&transaction, &command.preset_id).await?;
        let id = new_id("presetver");
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
        transaction.execute(sql(
            "INSERT INTO context_preset_versions (id, preset_id, version_no, semantic_policy_version, spec_json, content_hash, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
            vec![id.clone().into(), command.preset_id.clone().into(), (version_no as i64).into(), (policy.semantic_policy_version as i64).into(), canonical_json(&spec)?.into(), content_hash.clone().into(), now.into()],
        )).await?;
        transaction
            .execute(sql(
                "UPDATE context_presets SET head_version_id = ?, updated_at = ? WHERE id = ?",
                vec![
                    id.clone().into(),
                    now.into(),
                    command.preset_id.clone().into(),
                ],
            ))
            .await?;
        let version = ContextPresetVersion {
            id,
            preset_id: command.preset_id,
            version_no,
            semantic_policy_version: policy.semantic_policy_version,
            spec,
            content_hash,
            created_at: now,
        };
        finish_receipt(
            &transaction,
            &scope,
            &command.idempotency_key,
            &version,
            now,
        )
        .await?;
        transaction.commit().await?;
        Ok(version)
    }

    pub async fn get_context_preset_version(
        &self,
        version_id: &str,
    ) -> StorageResult<ContextPresetVersion> {
        load_preset_version(&self.db, version_id).await
    }

    pub async fn get_context_preset_head(
        &self,
        preset_id: &str,
    ) -> StorageResult<ContextPresetVersion> {
        let preset = self.get_context_preset(preset_id).await?;
        let version_id = preset
            .head_version_id
            .ok_or_else(|| StorageError::Conflict("context_preset_has_no_version"))?;
        self.get_context_preset_version(&version_id).await
    }
}

async fn next_version_no<C: ConnectionTrait>(
    connection: &C,
    preset_id: &str,
) -> StorageResult<u64> {
    let row = connection.query_one(sql(
        "SELECT COALESCE(MAX(version_no), 0) + 1 AS next_no FROM context_preset_versions WHERE preset_id = ?",
        vec![preset_id.into()],
    )).await?.ok_or_else(|| StorageError::Integrity("missing preset version sequence".into()))?;
    let value: i64 = row.try_get("", "next_no")?;
    u64::try_from(value)
        .map_err(|_| StorageError::Integrity("invalid preset version number".into()))
}
