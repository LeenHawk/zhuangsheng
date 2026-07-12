use serde_json::Value;

use crate::{
    canonical,
    llm::ir::{HostedToolPhase, LlmTurnItemIr},
};

use super::{
    OpaqueAttachmentDraft, OpaqueAttachmentTarget, SensitiveEntryDraft, ShapeAdapterError,
    ShapeAdapterKey,
};

pub(super) fn push_opaque_reasoning(
    model_call_id: &str,
    index: usize,
    item: &Value,
    items: &mut Vec<LlmTurnItemIr>,
    sensitive_entries: &mut Vec<SensitiveEntryDraft>,
    attachments: &mut Vec<OpaqueAttachmentDraft>,
) -> Result<(), ShapeAdapterError> {
    let id = format!("{model_call_id}:reasoning:{index}");
    let summary = item
        .get("summary")
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<String>()
        })
        .filter(|value| !value.is_empty());
    items.push(LlmTurnItemIr::Reasoning {
        id: id.clone(),
        summary,
        opaque_item_ref: None,
    });
    push_sidecar(index, "reasoning", item, id, sensitive_entries, attachments)
}

pub(super) fn push_opaque_hosted(
    model_call_id: &str,
    index: usize,
    kind: &str,
    item: &Value,
    items: &mut Vec<LlmTurnItemIr>,
    sensitive_entries: &mut Vec<SensitiveEntryDraft>,
    attachments: &mut Vec<OpaqueAttachmentDraft>,
) -> Result<(), ShapeAdapterError> {
    let id = format!("{model_call_id}:hosted:{index}");
    let phase = match item.get("status").and_then(Value::as_str) {
        Some("completed") => HostedToolPhase::Completed,
        Some("failed") => HostedToolPhase::Failed,
        Some("in_progress" | "searching") => HostedToolPhase::Running,
        _ => HostedToolPhase::Requested,
    };
    items.push(LlmTurnItemIr::HostedTool {
        id: id.clone(),
        binding_id: kind.into(),
        kind: kind.into(),
        phase,
        display_content: Vec::new(),
        opaque_item_ref: None,
    });
    if kind == "web_search_call" {
        return Ok(());
    }
    push_sidecar(index, kind, item, id, sensitive_entries, attachments)
}

fn push_sidecar(
    index: usize,
    slot: &str,
    item: &Value,
    item_id: String,
    sensitive_entries: &mut Vec<SensitiveEntryDraft>,
    attachments: &mut Vec<OpaqueAttachmentDraft>,
) -> Result<(), ShapeAdapterError> {
    let entry_key = format!("responses_item_{index}");
    sensitive_entries.push(SensitiveEntryDraft {
        entry_key: entry_key.clone(),
        adapter_key: ShapeAdapterKey::OpenAiResponsesV1,
        semantic_slot: slot.into(),
        opaque_bytes: canonical::to_vec(item).map_err(|_| {
            ShapeAdapterError::new(
                "opaque_item_encode_failed",
                "responses sidecar could not be encoded",
            )
        })?,
    });
    attachments.push(OpaqueAttachmentDraft {
        entry_key,
        target: OpaqueAttachmentTarget::Item { item_id },
    });
    Ok(())
}
