use sea_orm::{ConnectionTrait, TransactionTrait};
use zhuangsheng_core::{
    canonical,
    graph::{AppliedGraph, AppliedGraphDefinition, apply_graph_with_dependencies},
};

use crate::{SqliteStore, StorageError, StorageResult};

use super::{
    ApplyGraphCommand, GraphRevisionView,
    draft::load_draft,
    helpers::*,
    llm_dependencies::load_llm_dependencies,
    schema_bundle::{StoredSchemaBundle, StoredSchemaCompilation, verify_schema_bundle},
};

impl SqliteStore {
    pub async fn apply_graph(
        &self,
        command: ApplyGraphCommand,
    ) -> StorageResult<GraphRevisionView> {
        if command.idempotency_key.is_empty() {
            return Err(StorageError::InvalidArgument(
                "missing idempotency key".into(),
            ));
        }
        let scope = format!("workspace:local:graphs:{}:apply", command.graph_id);
        let digest = canonical::hash(&serde_json::json!({
            "command": "apply_graph",
            "graphId": command.graph_id,
            "expectedRevisionToken": command.expected_revision_token,
            "operationTaxonomyVersion": command.operation_taxonomy_version,
            "adapterDecoderVersion": command.adapter_decoder_version,
        }))?;
        let transaction = self.db.begin().await?;
        if let Some(receipt) = find_receipt(&transaction, &scope, &command.idempotency_key).await? {
            if receipt.digest != digest {
                return Err(StorageError::IdempotencyConflict);
            }
            let object = receipt.result_object_id.ok_or_else(|| {
                StorageError::Integrity("apply receipt has no result object".into())
            })?;
            let result = load_object_json(&transaction, &object).await?;
            transaction.commit().await?;
            return Ok(result);
        }
        let draft = load_draft(&transaction, &command.graph_id).await?;
        if draft.revision_token != command.expected_revision_token {
            return Err(StorageError::Conflict("graph_draft_revision"));
        }
        let dependencies = load_llm_dependencies(&transaction, &draft.document).await?;
        let applied = apply_graph_with_dependencies(
            draft.document,
            command.operation_taxonomy_version,
            command.adapter_decoder_version,
            &dependencies,
        )?;
        let now = now_ms();
        transaction.execute_raw(sql(
            "INSERT INTO application_command_receipts (scope, idempotency_key, request_digest, command_kind, resource_kind, status, created_at) VALUES (?, ?, ?, 'apply_graph', 'graph_revision', 'pending', ?)",
            vec![scope.clone().into(), command.idempotency_key.clone().into(), digest.into(), now.into()],
        )).await?;
        let revision_id = if let Some((id, _)) =
            find_by_hash(&transaction, &command.graph_id, &applied.content_hash).await?
        {
            id
        } else {
            persist_revision(&transaction, &command.graph_id, &applied, now)
                .await?
                .0
        };
        let mut result = load_revision(&transaction, &revision_id).await?;
        result.warnings = applied.warnings;
        let object_id = put_inline_object(&transaction, &canonical::to_vec(&result)?, now).await?;
        transaction.execute_raw(sql(
            "UPDATE application_command_receipts SET resource_id = ?, status = 'completed', result_object_id = ?, completed_at = ? WHERE scope = ? AND idempotency_key = ?",
            vec![revision_id.into(), object_id.into(), now.into(), scope.into(), command.idempotency_key.into()],
        )).await?;
        transaction.commit().await?;
        Ok(result)
    }

    pub async fn get_graph_revision(&self, id: &str) -> StorageResult<GraphRevisionView> {
        load_revision(&self.db, id).await
    }

    pub async fn get_graph_revision_for_graph(
        &self,
        graph_id: &str,
        id: &str,
    ) -> StorageResult<GraphRevisionView> {
        let revision = load_revision(&self.db, id).await?;
        if revision.graph_id != graph_id {
            return Err(StorageError::NotFound {
                kind: "graph_revision",
                id: id.into(),
            });
        }
        Ok(revision)
    }
}

async fn persist_revision<C: ConnectionTrait>(
    connection: &C,
    graph_id: &str,
    applied: &AppliedGraph,
    now: i64,
) -> StorageResult<(String, u64)> {
    let revision_id = new_id("graphrev");
    let revision_no = next_revision(connection, graph_id).await?;
    let mut stored = Vec::new();
    let mut referenced = Vec::new();
    for schema in &applied.schemas {
        let source = put_inline_object(connection, schema.canonical_source.as_bytes(), now).await?;
        let payload =
            put_inline_object(connection, schema.compiled_payload.as_bytes(), now).await?;
        referenced.push((source.clone(), "canonical_schema"));
        referenced.push((payload.clone(), "compiled_schema"));
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
    referenced.push((bundle_id.clone(), "schema_bundle"));
    connection.execute_raw(sql(
        "INSERT INTO graph_revisions (id, graph_id, revision_no, operation_taxonomy_version, adapter_decoder_version, definition_json, schema_bundle_object_id, content_hash, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        vec![revision_id.clone().into(), graph_id.into(), (revision_no as i64).into(), (applied.definition.operation_taxonomy_version as i64).into(), (applied.definition.adapter_decoder_version as i64).into(), canonical::to_string(&applied.definition)?.into(), bundle_id.into(), applied.content_hash.clone().into(), now.into()],
    )).await?;
    for (object, role) in referenced {
        connection.execute_raw(sql(
            "INSERT OR IGNORE INTO content_object_refs (object_id, owner_kind, owner_id, role, created_at) VALUES (?, 'graph_revision', ?, ?, ?)",
            vec![object.into(), revision_id.clone().into(), role.into(), now.into()],
        )).await?;
    }
    Ok((revision_id, revision_no))
}

async fn next_revision<C: ConnectionTrait>(connection: &C, graph_id: &str) -> StorageResult<u64> {
    let row = connection.query_one_raw(sql(
        "SELECT COALESCE(MAX(revision_no), 0) + 1 AS next_revision FROM graph_revisions WHERE graph_id = ?",
        vec![graph_id.into()],
    )).await?.ok_or_else(|| StorageError::Integrity("revision counter query failed".into()))?;
    let value: i64 = row.try_get("", "next_revision")?;
    Ok(value as u64)
}

async fn find_by_hash<C: ConnectionTrait>(
    connection: &C,
    graph_id: &str,
    hash: &str,
) -> StorageResult<Option<(String, u64)>> {
    connection
        .query_one_raw(sql(
            "SELECT id, revision_no FROM graph_revisions WHERE graph_id = ? AND content_hash = ?",
            vec![graph_id.into(), hash.into()],
        ))
        .await?
        .map(|row| {
            let number: i64 = row.try_get("", "revision_no")?;
            Ok((row.try_get("", "id")?, number as u64))
        })
        .transpose()
}

pub(crate) async fn load_revision<C: ConnectionTrait>(
    connection: &C,
    id: &str,
) -> StorageResult<GraphRevisionView> {
    let row = connection.query_one_raw(sql(
        "SELECT id, graph_id, revision_no, operation_taxonomy_version, adapter_decoder_version, definition_json, schema_bundle_object_id, content_hash, created_at FROM graph_revisions WHERE id = ?",
        vec![id.into()],
    )).await?.ok_or_else(|| StorageError::NotFound { kind: "graph_revision", id: id.into() })?;
    let taxonomy: i64 = row.try_get("", "operation_taxonomy_version")?;
    let decoder: i64 = row.try_get("", "adapter_decoder_version")?;
    if taxonomy <= 0
        || decoder <= 0
        || !zhuangsheng_core::compatibility::supports_operation_versions(
            taxonomy as u32,
            decoder as u32,
        )
    {
        return Err(StorageError::Integrity(
            "graph revision uses an unsupported operation version pair".into(),
        ));
    }
    let definition_json: String = row.try_get("", "definition_json")?;
    let definition: AppliedGraphDefinition = serde_json::from_str(&definition_json)
        .map_err(|error| StorageError::Integrity(error.to_string()))?;
    if definition.operation_taxonomy_version != taxonomy as u32
        || definition.adapter_decoder_version != decoder as u32
    {
        return Err(StorageError::Integrity(
            "graph revision envelope and definition versions disagree".into(),
        ));
    }
    if canonical::hash(&definition)? != row.try_get::<String>("", "content_hash")? {
        return Err(StorageError::Integrity(
            "graph revision hash mismatch".into(),
        ));
    }
    let schema_bundle_id: String = row.try_get("", "schema_bundle_object_id")?;
    verify_schema_bundle(connection, &schema_bundle_id, &definition).await?;
    let revision_no: i64 = row.try_get("", "revision_no")?;
    Ok(GraphRevisionView {
        id: row.try_get("", "id")?,
        graph_id: row.try_get("", "graph_id")?,
        revision_no: revision_no as u64,
        operation_taxonomy_version: taxonomy as u32,
        adapter_decoder_version: decoder as u32,
        definition,
        content_hash: row.try_get("", "content_hash")?,
        created_at: row.try_get("", "created_at")?,
        warnings: vec![],
    })
}
