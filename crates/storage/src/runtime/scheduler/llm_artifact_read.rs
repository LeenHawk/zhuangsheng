use std::collections::BTreeMap;

use sea_orm::ConnectionTrait;
use serde_json::{Value, json};
use zhuangsheng_core::{
    artifact::ArtifactRef,
    canonical,
    graph::{InputSelector, StaticMemoryRead},
    selector,
};

use crate::{StorageError, StorageResult, artifact::read::load_artifact_binding};

use super::read_set::{ResolvedBinding, ResolvedSelection};

pub(super) async fn resolve<C: ConnectionTrait>(
    connection: &C,
    read: &StaticMemoryRead,
    source_name: &str,
    input_selector: &InputSelector,
    inputs: &BTreeMap<String, Value>,
    context_id: &str,
) -> StorageResult<ResolvedBinding> {
    let selected = inputs
        .get(source_name)
        .ok_or_else(|| StorageError::Integrity("artifact input port is missing".into()))
        .and_then(|value| {
            selector::select(input_selector, value, 1).map_err(StorageError::InputContract)
        });
    let selected = match selected {
        Ok(value) => value,
        Err(StorageError::InputContract(_)) if !read.required => {
            return missing(read, inputs.get(source_name));
        }
        Err(error) => return Err(error),
    };
    let reference: ArtifactRef = serde_json::from_value(selected)
        .map_err(|_| StorageError::InputContract("artifact input is not an ArtifactRef".into()))?;
    reference
        .validate()
        .map_err(|message| StorageError::InputContract(message.into()))?;
    let (artifact, bytes, owner_context_id) =
        match load_artifact_binding(connection, &reference.artifact_id).await {
            Ok(value) => value,
            Err(StorageError::NotFound { .. }) if !read.required => {
                return missing(read, inputs.get(source_name));
            }
            Err(StorageError::NotFound { .. }) => {
                return Err(StorageError::InputContract(
                    "required artifact did not resolve".into(),
                ));
            }
            Err(error) => return Err(error),
        };
    if artifact.metadata.content != reference || owner_context_id.as_deref() != Some(context_id) {
        return Err(StorageError::InputContract(
            "artifact ref is not authorized for the run context".into(),
        ));
    }
    if bytes.len() as u64 > read.max_bytes {
        return Err(StorageError::InputContract(
            "artifact content exceeds the static read limit".into(),
        ));
    }
    let text = artifact
        .metadata
        .content
        .media_type
        .starts_with("text/")
        .then(|| String::from_utf8(bytes))
        .transpose()
        .map_err(|_| StorageError::Integrity("text artifact is not UTF-8".into()))?;
    let metadata = serde_json::to_value(&artifact.metadata)
        .map_err(|error| StorageError::Integrity(error.to_string()))?;
    let envelope = json!({
        "kind":"artifact",
        "found":true,
        "version":artifact.metadata_head_commit_id,
        "artifactRef":reference,
        "metadata":metadata,
        "text":text,
    });
    Ok(ResolvedBinding {
        envelope,
        selections: vec![ResolvedSelection {
            aggregate_kind: "artifact_metadata",
            aggregate_id: reference.artifact_id,
            lineage_key: "global".into(),
            commit_id: artifact.metadata_head_commit_id,
            selection_ordinal: None,
            content_hash: Some(reference.content_hash),
        }],
        scope_snapshot_token: None,
        truncated: false,
    })
}

fn missing(read: &StaticMemoryRead, source: Option<&Value>) -> StorageResult<ResolvedBinding> {
    let version = canonical::hash(&json!({
        "bindingId":read.id,
        "source":source,
    }))?;
    Ok(ResolvedBinding {
        envelope: json!({"kind":"artifact","found":false,"version":version}),
        selections: vec![],
        scope_snapshot_token: None,
        truncated: false,
    })
}
