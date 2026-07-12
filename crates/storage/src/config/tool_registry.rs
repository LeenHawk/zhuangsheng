use sea_orm::{ConnectionTrait, TransactionTrait};
use zhuangsheng_core::{
    application::tool::{
        PublishToolCommand, RegisteredToolView, SetToolEnabledCommand, ToolDescriptorView,
    },
    canonical,
    llm::{ResolvedToolDescriptor, compile_tool_descriptor, validate_resolved_tool_descriptor},
};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::{
        helpers::*,
        schema_bundle::{StoredSchemaBundle, StoredSchemaCompilation},
    },
};

use super::{
    channel::{finish_receipt, insert_pending_receipt, require_key, required_result},
    tool_registry_rows::load_registered_tool,
};

impl SqliteStore {
    pub async fn publish_tool(
        &self,
        command: PublishToolCommand,
    ) -> StorageResult<RegisteredToolView> {
        require_key(&command.idempotency_key)?;
        let compilations = compile_tool_descriptor(&command.descriptor)
            .map_err(|error| StorageError::InvalidArgument(error.to_string()))?;
        let descriptor_digest = command.descriptor.digest()?;
        let resolved = ResolvedToolDescriptor {
            descriptor: command.descriptor.clone(),
            descriptor_digest: descriptor_digest.clone(),
            schema_compilation_digests: compilations
                .iter()
                .map(|item| item.compiled_payload_hash.clone())
                .collect(),
            implementation_digest: command.implementation_digest.clone(),
            executor_key: command.executor_key.clone(),
        };
        validate_resolved_tool_descriptor(&resolved)
            .map_err(|error| StorageError::InvalidArgument(error.to_string()))?;
        let scope = format!(
            "workspace:local:tools:{}:{}:publish",
            command.descriptor.tool_id, command.descriptor.version
        );
        let digest = canonical::hash(&serde_json::json!({
            "command":"publish_tool",
            "resolved":resolved,
            "enabled":command.enabled,
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
        if transaction.query_one(sql(
            "SELECT 1 AS present FROM tool_registry_entries WHERE tool_id = ? AND tool_version = ?",
            vec![command.descriptor.tool_id.clone().into(), command.descriptor.version.clone().into()],
        )).await?.is_some() {
            return Err(StorageError::Conflict("tool_version_exists"));
        }
        let now = now_ms();
        let owner_id = format!(
            "{}:{}",
            command.descriptor.tool_id, command.descriptor.version
        );
        insert_pending_receipt(
            &transaction,
            &scope,
            &command.idempotency_key,
            &digest,
            &owner_id,
            now,
        )
        .await?;
        let bundle_id = persist_schema_bundle(&transaction, &owner_id, &compilations, now).await?;
        transaction.execute(sql(
            "INSERT INTO tool_registry_entries (tool_id, tool_version, descriptor_json, schema_bundle_object_id, descriptor_digest, implementation_digest, executor_key, enabled, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            vec![
                command.descriptor.tool_id.clone().into(), command.descriptor.version.clone().into(),
                canonical::to_string(&command.descriptor)?.into(), bundle_id.into(), descriptor_digest.into(),
                command.implementation_digest.into(), command.executor_key.into(), i64::from(command.enabled).into(),
                now.into(), now.into(),
            ],
        )).await?;
        let view = RegisteredToolView {
            resolved,
            enabled: command.enabled,
            created_at: now,
            updated_at: now,
        };
        finish_receipt(&transaction, &scope, &command.idempotency_key, &view, now).await?;
        transaction.commit().await?;
        Ok(view)
    }

    pub async fn set_tool_enabled(
        &self,
        command: SetToolEnabledCommand,
    ) -> StorageResult<RegisteredToolView> {
        require_key(&command.idempotency_key)?;
        let scope = format!(
            "workspace:local:tools:{}:{}:enabled",
            command.tool_id, command.version
        );
        let digest = canonical::hash(&command)?;
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
        let current =
            load_registered_tool(&transaction, &command.tool_id, &command.version).await?;
        let now = now_ms();
        insert_pending_receipt(
            &transaction,
            &scope,
            &command.idempotency_key,
            &digest,
            &format!("{}:{}", command.tool_id, command.version),
            now,
        )
        .await?;
        transaction.execute(sql(
            "UPDATE tool_registry_entries SET enabled = ?, updated_at = ? WHERE tool_id = ? AND tool_version = ?",
            vec![i64::from(command.enabled).into(), now.into(), command.tool_id.clone().into(), command.version.clone().into()],
        )).await?;
        let view = RegisteredToolView {
            enabled: command.enabled,
            updated_at: now,
            ..current
        };
        finish_receipt(&transaction, &scope, &command.idempotency_key, &view, now).await?;
        transaction.commit().await?;
        Ok(view)
    }

    pub async fn get_registered_tool(
        &self,
        tool_id: &str,
        version: &str,
    ) -> StorageResult<RegisteredToolView> {
        load_registered_tool(&self.db, tool_id, version).await
    }

    pub async fn list_tool_descriptors(&self) -> StorageResult<Vec<ToolDescriptorView>> {
        let rows = self.db.query_all(sql(
            "SELECT tool_id, tool_version FROM tool_registry_entries WHERE enabled = 1 ORDER BY tool_id, tool_version",
            vec![],
        )).await?;
        let mut views = Vec::with_capacity(rows.len());
        for row in rows {
            let registered = load_registered_tool(
                &self.db,
                &row.try_get::<String>("", "tool_id")?,
                &row.try_get::<String>("", "tool_version")?,
            )
            .await?;
            let descriptor = registered.resolved.descriptor;
            views.push(ToolDescriptorView {
                tool_id: descriptor.tool_id,
                version: descriptor.version,
                name: descriptor.name,
                description: descriptor.description,
                input_schema: descriptor.input_schema,
            });
        }
        Ok(views)
    }
}

async fn persist_schema_bundle<C: ConnectionTrait>(
    connection: &C,
    owner_id: &str,
    compilations: &[zhuangsheng_core::schema::SchemaCompilationDraft],
    now: i64,
) -> StorageResult<String> {
    let mut stored = Vec::with_capacity(compilations.len());
    for (index, schema) in compilations.iter().enumerate() {
        let source = put_inline_object(connection, schema.canonical_source.as_bytes(), now).await?;
        let payload =
            put_inline_object(connection, schema.compiled_payload.as_bytes(), now).await?;
        for (object_id, role) in [
            (&source, format!("canonical_schema:{index}")),
            (&payload, format!("compiled_schema:{index}")),
        ] {
            connection.execute(sql(
                "INSERT OR IGNORE INTO content_object_refs (object_id, owner_kind, owner_id, role, created_at) VALUES (?, 'tool_registry_entry', ?, ?, ?)",
                vec![object_id.clone().into(), owner_id.into(), role.into(), now.into()],
            )).await?;
        }
        stored.push(StoredSchemaCompilation {
            canonical_document_hash: schema.canonical_document_hash.clone(),
            schema_hash: schema.schema_hash.clone(),
            compiler_id: schema.compiler_id.clone(),
            compiler_version: schema.compiler_version.clone(),
            payload_format_version: schema.payload_format_version,
            canonical_schema_object_id: source,
            compiled_payload_object_id: payload,
            compiled_payload_hash: schema.compiled_payload_hash.clone(),
        });
    }
    let bundle = canonical::to_vec(&StoredSchemaBundle {
        schema_version: 1,
        compilations: stored,
    })?;
    let bundle_id = put_inline_object(connection, &bundle, now).await?;
    connection.execute(sql(
        "INSERT OR IGNORE INTO content_object_refs (object_id, owner_kind, owner_id, role, created_at) VALUES (?, 'tool_registry_entry', ?, 'schema_bundle', ?)",
        vec![bundle_id.clone().into(), owner_id.into(), now.into()],
    )).await?;
    Ok(bundle_id)
}
