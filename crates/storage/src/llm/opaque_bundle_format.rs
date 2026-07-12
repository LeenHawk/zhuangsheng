use std::collections::{BTreeMap, BTreeSet};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use zhuangsheng_core::llm::{
    LlmOperationExecutionPin, OperationKey,
    adapter::{SensitiveEntryDraft, ShapeAdapterKey},
    ir::{InternalSensitiveEntryRef, OpaqueContinuationRef},
};

use crate::{StorageError, StorageResult, secret::SecretStoreError};

const MAX_ENTRIES: usize = 64;
pub(super) const MAX_ENTRY_BYTES: usize = 256 * 1024;
pub(super) const MAX_BUNDLE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct StoredOpaqueBundleRefs {
    pub entries: BTreeMap<String, OpaqueContinuationRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct OpaqueBundle {
    pub schema_version: u32,
    pub effect_attempt_id: String,
    pub model_call_id: String,
    pub adapter_key: String,
    pub operation_key: OperationKey,
    pub operation_taxonomy_version: u32,
    pub adapter_decoder_version: u32,
    pub entries: BTreeMap<String, OpaqueEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct OpaqueEntry {
    adapter_key: String,
    operation_key: OperationKey,
    semantic_slot: String,
    pub bytes_base64: String,
}

pub(super) fn build_bundle(
    effect_attempt_id: &str,
    model_call_id: &str,
    operation: &LlmOperationExecutionPin,
    adapter: ShapeAdapterKey,
    drafts: &[SensitiveEntryDraft],
) -> StorageResult<OpaqueBundle> {
    if drafts.is_empty()
        || drafts.len() > MAX_ENTRIES
        || effect_attempt_id.is_empty()
        || model_call_id.is_empty()
    {
        return Err(StorageError::InvalidArgument(
            "invalid opaque bundle entry count".into(),
        ));
    }
    let mut keys = BTreeSet::new();
    let mut total = 0_usize;
    let mut entries = BTreeMap::new();
    for draft in drafts {
        total = total.saturating_add(draft.opaque_bytes.len());
        if draft.adapter_key != adapter
            || draft.entry_key.is_empty()
            || draft.entry_key.len() > 128
            || draft.semantic_slot.is_empty()
            || draft.semantic_slot.len() > 128
            || draft.opaque_bytes.is_empty()
            || draft.opaque_bytes.len() > MAX_ENTRY_BYTES
            || total > MAX_BUNDLE_BYTES
            || !keys.insert(&draft.entry_key)
        {
            return Err(StorageError::InvalidArgument(
                "invalid opaque bundle entry".into(),
            ));
        }
        entries.insert(
            draft.entry_key.clone(),
            OpaqueEntry {
                adapter_key: adapter.as_str().into(),
                operation_key: operation.operation_key,
                semantic_slot: draft.semantic_slot.clone(),
                bytes_base64: URL_SAFE_NO_PAD.encode(&draft.opaque_bytes),
            },
        );
    }
    Ok(OpaqueBundle {
        schema_version: 1,
        effect_attempt_id: effect_attempt_id.into(),
        model_call_id: model_call_id.into(),
        adapter_key: adapter.as_str().into(),
        operation_key: operation.operation_key,
        operation_taxonomy_version: operation.operation_taxonomy_version,
        adapter_decoder_version: operation.adapter_decoder_version,
        entries,
    })
}

pub(super) fn bundle_refs(
    object_id: &str,
    digest: &str,
    bundle: &OpaqueBundle,
) -> StoredOpaqueBundleRefs {
    StoredOpaqueBundleRefs {
        entries: bundle
            .entries
            .keys()
            .map(|key| {
                (
                    key.clone(),
                    OpaqueContinuationRef {
                        adapter_key: bundle.adapter_key.clone(),
                        operation_key: bundle.operation_key,
                        operation_taxonomy_version: bundle.operation_taxonomy_version,
                        adapter_decoder_version: bundle.adapter_decoder_version,
                        model_call_id: bundle.model_call_id.clone(),
                        entry_ref: InternalSensitiveEntryRef {
                            object_id: object_id.into(),
                            entry_key: key.clone(),
                        },
                        digest: digest.into(),
                        expires_at: None,
                    },
                )
            })
            .collect(),
    }
}

pub(super) fn validate_reference(
    reference: &OpaqueContinuationRef,
    operation: &LlmOperationExecutionPin,
    adapter: &str,
    now: i64,
) -> StorageResult<()> {
    if reference.adapter_key != adapter
        || reference.operation_key != operation.operation_key
        || reference.operation_taxonomy_version != operation.operation_taxonomy_version
        || reference.adapter_decoder_version != operation.adapter_decoder_version
        || reference.expires_at.is_some_and(|expires| expires <= now)
    {
        return Err(StorageError::InvalidArgument(
            "opaque continuation pin mismatch".into(),
        ));
    }
    Ok(())
}

pub(super) fn validate_bundle(
    bundle: &OpaqueBundle,
    effect_attempt_id: &str,
    operation: &LlmOperationExecutionPin,
    adapter: &str,
) -> StorageResult<()> {
    if bundle.schema_version != 1
        || bundle.effect_attempt_id != effect_attempt_id
        || bundle.adapter_key != adapter
        || bundle.operation_key != operation.operation_key
        || bundle.operation_taxonomy_version != operation.operation_taxonomy_version
        || bundle.adapter_decoder_version != operation.adapter_decoder_version
        || bundle.entries.is_empty()
        || bundle.entries.len() > MAX_ENTRIES
        || bundle.entries.values().any(|entry| {
            entry.adapter_key != adapter
                || entry.operation_key != operation.operation_key
                || entry.semantic_slot.is_empty()
        })
    {
        return Err(SecretStoreError::CorruptStore.into());
    }
    Ok(())
}
