use sea_orm::{ConnectionTrait, QueryResult};
use zhuangsheng_core::{
    application::{channel::ChannelView, preset::ContextPresetView},
    canonical,
    compatibility::supports_operation_versions,
    llm::{
        ChannelCredential, LlmChannelRevision, LlmChannelRevisionSpec,
        context::{CONTEXT_SEMANTIC_POLICY_VERSION, ContextPresetVersion},
        revision_content_hash,
    },
};

use crate::{StorageError, StorageResult, graph::helpers::sql};

pub(super) fn channel_from_row(row: &QueryResult) -> StorageResult<ChannelView> {
    Ok(ChannelView {
        id: row.try_get("", "id")?,
        name: row.try_get("", "name")?,
        head_revision_id: row.try_get("", "head_revision_id")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

pub(super) fn preset_from_row(row: &QueryResult) -> StorageResult<ContextPresetView> {
    Ok(ContextPresetView {
        id: row.try_get("", "id")?,
        name: row.try_get("", "name")?,
        head_version_id: row.try_get("", "head_version_id")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

pub(crate) async fn load_channel_revision<C: ConnectionTrait>(
    connection: &C,
    revision_id: &str,
) -> StorageResult<LlmChannelRevision> {
    let row = connection
        .query_one_raw(sql(
            "SELECT * FROM llm_channel_revisions WHERE id = ?",
            vec![revision_id.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::NotFound {
            kind: "channel_revision",
            id: revision_id.into(),
        })?;
    channel_revision_from_row(&row)
}

pub(crate) async fn load_channel_head<C: ConnectionTrait>(
    connection: &C,
    channel_id: &str,
) -> StorageResult<LlmChannelRevision> {
    let row = connection
        .query_one_raw(sql(
            "SELECT head_revision_id FROM llm_channels WHERE id = ?",
            vec![channel_id.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::NotFound {
            kind: "channel",
            id: channel_id.into(),
        })?;
    let id: Option<String> = row.try_get("", "head_revision_id")?;
    load_channel_revision(
        connection,
        &id.ok_or(StorageError::Conflict("channel_has_no_revision"))?,
    )
    .await
}

pub(crate) async fn load_preset_version<C: ConnectionTrait>(
    connection: &C,
    version_id: &str,
) -> StorageResult<ContextPresetVersion> {
    let row = connection
        .query_one_raw(sql(
            "SELECT * FROM context_preset_versions WHERE id = ?",
            vec![version_id.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::NotFound {
            kind: "context_preset_version",
            id: version_id.into(),
        })?;
    preset_version_from_row(&row)
}

pub(crate) async fn load_preset_head<C: ConnectionTrait>(
    connection: &C,
    preset_id: &str,
) -> StorageResult<ContextPresetVersion> {
    let row = connection
        .query_one_raw(sql(
            "SELECT head_version_id FROM context_presets WHERE id = ?",
            vec![preset_id.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::NotFound {
            kind: "context_preset",
            id: preset_id.into(),
        })?;
    let id: Option<String> = row.try_get("", "head_version_id")?;
    load_preset_version(
        connection,
        &id.ok_or(StorageError::Conflict("context_preset_has_no_version"))?,
    )
    .await
}

pub(crate) fn preset_version_from_row(row: &QueryResult) -> StorageResult<ContextPresetVersion> {
    let semantic: i64 = row.try_get("", "semantic_policy_version")?;
    if semantic != CONTEXT_SEMANTIC_POLICY_VERSION as i64 {
        return Err(StorageError::Integrity(
            "unsupported context semantic policy".into(),
        ));
    }
    let spec_json: String = row.try_get("", "spec_json")?;
    let spec = serde_json::from_str(&spec_json)
        .map_err(|error| StorageError::Integrity(error.to_string()))?;
    let stored_hash: String = row.try_get("", "content_hash")?;
    let expected_hash = canonical::hash(&serde_json::json!({
        "semanticPolicyVersion": semantic,
        "spec": spec
    }))?;
    if stored_hash != expected_hash {
        return Err(StorageError::Integrity(
            "context preset content hash mismatch".into(),
        ));
    }
    Ok(ContextPresetVersion {
        id: row.try_get("", "id")?,
        preset_id: row.try_get("", "preset_id")?,
        version_no: positive_u64(row, "version_no")?,
        semantic_policy_version: semantic as u32,
        spec,
        content_hash: stored_hash,
        created_at: row.try_get("", "created_at")?,
    })
}

pub(super) fn channel_revision_from_row(row: &QueryResult) -> StorageResult<LlmChannelRevision> {
    let taxonomy: i64 = row.try_get("", "operation_taxonomy_version")?;
    let decoder: i64 = row.try_get("", "adapter_decoder_version")?;
    if taxonomy <= 0
        || decoder <= 0
        || !supports_operation_versions(taxonomy as u32, decoder as u32)
    {
        return Err(StorageError::Integrity(
            "channel revision uses an unsupported operation version pair".into(),
        ));
    }
    let credential_kind: String = row.try_get("", "credential_kind")?;
    let api_key_ref: Option<String> = row.try_get("", "api_key_ref")?;
    let credential = match (credential_kind.as_str(), api_key_ref) {
        ("secret", Some(value)) => ChannelCredential::Secret {
            api_key_ref: decode(&value, "channel api key ref")?,
        },
        ("none", None) => ChannelCredential::None,
        _ => {
            return Err(StorageError::Integrity(
                "invalid channel credential row".into(),
            ));
        }
    };
    let spec = LlmChannelRevisionSpec {
        operation_taxonomy_version: taxonomy as u32,
        adapter_decoder_version: decoder as u32,
        base_url: row.try_get("", "base_url")?,
        transport_policy: decode_column(row, "transport_policy_json")?,
        credential,
        operation_keys: decode_column(row, "operation_keys_json")?,
        model_catalogs: decode_column(row, "model_catalogs_json")?,
        capabilities: decode_column(row, "capabilities_json")?,
    };
    let stored_hash: String = row.try_get("", "content_hash")?;
    if revision_content_hash(&spec).map_err(|error| StorageError::Integrity(error.to_string()))?
        != stored_hash
    {
        return Err(StorageError::Integrity(
            "channel revision content hash mismatch".into(),
        ));
    }
    Ok(LlmChannelRevision {
        id: row.try_get("", "id")?,
        channel_id: row.try_get("", "channel_id")?,
        revision_no: positive_u64(row, "revision_no")?,
        spec,
        content_hash: stored_hash,
        created_at: row.try_get("", "created_at")?,
    })
}

pub(super) fn decode_column<T: serde::de::DeserializeOwned>(
    row: &QueryResult,
    column: &str,
) -> StorageResult<T> {
    let value: String = row.try_get("", column)?;
    decode(&value, column)
}

pub(super) fn decode<T: serde::de::DeserializeOwned>(value: &str, field: &str) -> StorageResult<T> {
    serde_json::from_str(value)
        .map_err(|error| StorageError::Integrity(format!("invalid {field}: {error}")))
}

pub(super) fn positive_u64(row: &QueryResult, column: &str) -> StorageResult<u64> {
    let value: i64 = row.try_get("", column)?;
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| StorageError::Integrity(format!("invalid positive {column}")))
}

pub(super) fn canonical_json<T: serde::Serialize>(value: &T) -> StorageResult<String> {
    canonical::to_string(value).map_err(Into::into)
}
