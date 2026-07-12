use sea_orm::ConnectionTrait;
use serde::{Deserialize, Serialize};
use zhuangsheng_core::{
    canonical,
    graph::AppliedGraphDefinition,
    schema::{self, JsonSchemaSpec, SchemaCompilationDraft},
};

use crate::{StorageError, StorageResult};

use super::helpers::{load_object_bytes, load_object_json};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StoredSchemaBundle {
    pub schema_version: u32,
    pub compilations: Vec<StoredSchemaCompilation>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StoredSchemaCompilation {
    pub canonical_document_hash: String,
    pub schema_hash: String,
    pub compiler_id: String,
    pub compiler_version: String,
    pub payload_format_version: u32,
    pub canonical_schema_object_id: String,
    pub compiled_payload_object_id: String,
    pub compiled_payload_hash: String,
}

pub(super) async fn verify_schema_bundle<C: ConnectionTrait>(
    connection: &C,
    bundle_id: &str,
    definition: &AppliedGraphDefinition,
) -> StorageResult<()> {
    let bundle: StoredSchemaBundle = load_object_json(connection, bundle_id).await?;
    if bundle.schema_version != 1 || bundle.compilations.len() != definition.schema_semantics.len()
    {
        return Err(integrity("schema bundle envelope mismatch"));
    }
    for expected in &definition.schema_semantics {
        let stored = bundle
            .compilations
            .iter()
            .find(|item| item.schema_hash == expected.schema_hash)
            .ok_or_else(|| integrity("schema compilation missing"))?;
        if stored.canonical_document_hash != expected.canonical_document_hash
            || stored.compiler_id != expected.compiler_id
            || stored.compiler_version != expected.compiler_version
            || stored.payload_format_version != expected.payload_format_version
            || stored.compiled_payload_hash != expected.compiled_payload_hash
        {
            return Err(integrity("schema compilation tuple mismatch"));
        }
        verify_compilation(connection, stored).await?;
    }
    Ok(())
}

pub(crate) async fn verify_compilation_bundle<C: ConnectionTrait>(
    connection: &C,
    bundle_id: &str,
    expected: &[SchemaCompilationDraft],
) -> StorageResult<()> {
    let bundle: StoredSchemaBundle = load_object_json(connection, bundle_id).await?;
    if bundle.schema_version != 1 || bundle.compilations.len() != expected.len() {
        return Err(integrity("schema bundle envelope mismatch"));
    }
    for draft in expected {
        let stored = bundle
            .compilations
            .iter()
            .find(|item| item.schema_hash == draft.schema_hash)
            .ok_or_else(|| integrity("schema compilation missing"))?;
        if stored.canonical_document_hash != draft.canonical_document_hash
            || stored.compiler_id != draft.compiler_id
            || stored.compiler_version != draft.compiler_version
            || stored.payload_format_version != draft.payload_format_version
            || stored.compiled_payload_hash != draft.compiled_payload_hash
        {
            return Err(integrity("schema compilation tuple mismatch"));
        }
        verify_compilation(connection, stored).await?;
    }
    Ok(())
}

async fn verify_compilation<C: ConnectionTrait>(
    connection: &C,
    stored: &StoredSchemaCompilation,
) -> StorageResult<()> {
    let source = load_object_bytes(connection, &stored.canonical_schema_object_id).await?;
    let payload = load_object_bytes(connection, &stored.compiled_payload_object_id).await?;
    if canonical::hash_bytes(&payload) != stored.compiled_payload_hash {
        return Err(integrity("compiled schema payload hash mismatch"));
    }
    let spec: JsonSchemaSpec = serde_json::from_slice(&source)
        .map_err(|_| integrity("canonical schema source cannot be decoded"))?;
    let compiled = schema::compile(&spec)?;
    if compiled.canonical_source.as_bytes() != source
        || compiled.compiled_payload.as_bytes() != payload
        || compiled.canonical_document_hash != stored.canonical_document_hash
        || compiled.schema_hash != stored.schema_hash
        || compiled.compiler_id != stored.compiler_id
        || compiled.compiler_version != stored.compiler_version
        || compiled.payload_format_version != stored.payload_format_version
        || compiled.compiled_payload_hash != stored.compiled_payload_hash
    {
        return Err(integrity("stored schema compilation cannot be reproduced"));
    }
    Ok(())
}

fn integrity(message: &str) -> StorageError {
    StorageError::Integrity(message.into())
}
