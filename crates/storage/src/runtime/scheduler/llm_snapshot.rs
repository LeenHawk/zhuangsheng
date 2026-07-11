use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    application::graph::GraphRevisionView,
    canonical,
    graph::{DraftNodeKind, GraphNode, LlmNodeExecutionSnapshot, llm_model_requirements},
    llm::{
        LlmOperationExecutionPin,
        context::{CONTEXT_SEMANTIC_POLICY_VERSION, ContextAssemblyConfig, ContextConfigSnapshot},
        revision_content_hash, validate_generation_model,
    },
};

use crate::{
    StorageError, StorageResult,
    config::rows::{load_channel_head, load_preset_head},
    graph::helpers::{load_object_json, put_inline_object, sql},
};

pub(super) async fn ensure_llm_snapshot<C: ConnectionTrait>(
    connection: &C,
    instance_id: &str,
    revision: &GraphRevisionView,
    node: &GraphNode,
    now: i64,
) -> StorageResult<Option<LlmNodeExecutionSnapshot>> {
    let DraftNodeKind::Llm { config } = &node.kind else {
        return Ok(None);
    };
    let row = connection
        .query_one(sql(
            "SELECT execution_snapshot_object_id FROM node_instances WHERE id = ?",
            vec![instance_id.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::Integrity("node instance disappeared".into()))?;
    let existing: Option<String> = row.try_get("", "execution_snapshot_object_id")?;
    if let Some(object_id) = existing {
        let snapshot = load_object_json(connection, &object_id).await?;
        verify_snapshot(&snapshot, revision, node)?;
        return Ok(Some(snapshot));
    }
    let channel = load_channel_head(connection, &config.model.channel_id).await?;
    if channel.spec.operation_taxonomy_version != revision.operation_taxonomy_version
        || channel.spec.adapter_decoder_version != revision.adapter_decoder_version
    {
        return Err(StorageError::InvalidArgument(
            "operation_version_mismatch: graph and channel versions differ".into(),
        ));
    }
    validate_generation_model(
        &channel.spec,
        &config.model,
        &llm_model_requirements(config),
        &config.capability_overrides,
    )
    .map_err(StorageError::LlmConfig)?;
    let (context, preset_version_id) = match &config.context {
        ContextAssemblyConfig::Preset { preset_id } => {
            let version = load_preset_head(connection, preset_id).await?;
            (
                ContextConfigSnapshot::Preset {
                    preset_id: preset_id.clone(),
                    version_id: version.id.clone(),
                    version: version.version_no,
                    content_hash: version.content_hash,
                    semantic_policy_version: version.semantic_policy_version,
                    spec: version.spec,
                },
                Some(version.id),
            )
        }
        ContextAssemblyConfig::Inline { spec } => {
            let content_hash = context_hash(spec)?;
            (
                ContextConfigSnapshot::GraphInline {
                    graph_revision_id: revision.id.clone(),
                    node_id: node.id.clone(),
                    content_hash,
                    semantic_policy_version: CONTEXT_SEMANTIC_POLICY_VERSION,
                    spec: spec.clone(),
                },
                None,
            )
        }
    };
    let limits = config
        .limits
        .clone()
        .ok_or_else(|| StorageError::Integrity("applied LLM limits are missing".into()))?;
    let snapshot = LlmNodeExecutionSnapshot {
        schema_version: 1,
        graph_revision_id: revision.id.clone(),
        graph_content_hash: revision.content_hash.clone(),
        node_id: node.id.clone(),
        operation: LlmOperationExecutionPin {
            channel_revision_id: channel.id.clone(),
            model_id: config.model.model_id.clone(),
            operation_key: config.model.operation_key,
            operation_taxonomy_version: revision.operation_taxonomy_version,
            adapter_decoder_version: revision.adapter_decoder_version,
        },
        channel,
        context,
        capability_overrides: config.capability_overrides.clone(),
        limits,
    };
    verify_snapshot(&snapshot, revision, node)?;
    let object_id = put_inline_object(connection, &canonical::to_vec(&snapshot)?, now).await?;
    let updated = connection
        .execute(sql(
            "UPDATE node_instances SET execution_snapshot_object_id = ?, operation_taxonomy_version = ?, adapter_decoder_version = ?, preset_version_id = ?, updated_at = ? WHERE id = ? AND execution_snapshot_object_id IS NULL",
            vec![
                object_id.clone().into(),
                (revision.operation_taxonomy_version as i64).into(),
                (revision.adapter_decoder_version as i64).into(),
                preset_version_id.into(),
                now.into(),
                instance_id.into(),
            ],
        ))
        .await?;
    if updated.rows_affected() != 1 {
        return Err(StorageError::Conflict("llm_execution_snapshot_race"));
    }
    connection
        .execute(sql(
            "INSERT OR IGNORE INTO content_object_refs (object_id, owner_kind, owner_id, role, created_at) VALUES (?, 'node_instance', ?, 'llm_execution_snapshot', ?)",
            vec![object_id.into(), instance_id.into(), now.into()],
        ))
        .await?;
    Ok(Some(snapshot))
}

fn verify_snapshot(
    snapshot: &LlmNodeExecutionSnapshot,
    revision: &GraphRevisionView,
    node: &GraphNode,
) -> StorageResult<()> {
    let DraftNodeKind::Llm { config } = &node.kind else {
        return Err(StorageError::Integrity(
            "LLM snapshot attached to a non-LLM node".into(),
        ));
    };
    if snapshot.schema_version != 1
        || snapshot.graph_revision_id != revision.id
        || snapshot.graph_content_hash != revision.content_hash
        || snapshot.node_id != node.id
        || snapshot.channel.channel_id != config.model.channel_id
        || snapshot.operation.channel_revision_id != snapshot.channel.id
        || snapshot.operation.model_id != config.model.model_id
        || snapshot.operation.operation_key != config.model.operation_key
        || snapshot.operation.operation_taxonomy_version != revision.operation_taxonomy_version
        || snapshot.operation.adapter_decoder_version != revision.adapter_decoder_version
        || snapshot.capability_overrides != config.capability_overrides
        || snapshot.limits != config.limits.clone().unwrap_or_default()
    {
        return Err(StorageError::Integrity(
            "LLM execution snapshot does not match its graph node".into(),
        ));
    }
    let hash = revision_content_hash(&snapshot.channel.spec)
        .map_err(|error| StorageError::Integrity(error.to_string()))?;
    if hash != snapshot.channel.content_hash {
        return Err(StorageError::Integrity(
            "pinned channel revision failed content hash validation".into(),
        ));
    }
    match (&config.context, &snapshot.context) {
        (
            ContextAssemblyConfig::Preset { preset_id },
            ContextConfigSnapshot::Preset {
                preset_id: pinned,
                semantic_policy_version,
                content_hash,
                spec,
                ..
            },
        ) if preset_id == pinned
            && *semantic_policy_version == CONTEXT_SEMANTIC_POLICY_VERSION
            && *content_hash == context_hash(spec)? => {}
        (
            ContextAssemblyConfig::Inline { spec },
            ContextConfigSnapshot::GraphInline {
                graph_revision_id,
                node_id,
                semantic_policy_version,
                content_hash,
                spec: pinned,
            },
        ) if graph_revision_id == &revision.id
            && node_id == &node.id
            && *semantic_policy_version == CONTEXT_SEMANTIC_POLICY_VERSION
            && spec == pinned
            && *content_hash == context_hash(pinned)? => {}
        _ => {
            return Err(StorageError::Integrity(
                "pinned context snapshot failed validation".into(),
            ));
        }
    }
    Ok(())
}

fn context_hash(
    spec: &zhuangsheng_core::llm::context::ContextAssemblySpec,
) -> StorageResult<String> {
    canonical::hash(&json!({
        "semanticPolicyVersion": CONTEXT_SEMANTIC_POLICY_VERSION,
        "spec": spec,
    }))
    .map_err(Into::into)
}
