use serde_json::json;

use crate::{
    graph::{MemoryToolCapability, MemoryToolGrant},
    llm::{
        LlmLoopCheckpoint, MemoryToolBatchInput, ResolvedMemoryTool, ToolRegistrySnapshot,
        ir::{LlmTurnItemIr, ToolCallIr},
        plan_memory_tool_batch,
    },
};

#[test]
fn search_arguments_are_closed_and_bounded_by_the_grant() {
    let tool = resolved(MemoryToolCapability::SearchMemory, 8_192);
    let unknown = call(
        "search_memory",
        json!({"scopeId":"story","text":null,"tags":[],"status":null,"limit":1,"extra":true}),
    );
    assert_error(&tool, &[unknown], "memory_search_arguments_invalid");

    let denied = call(
        "search_memory",
        json!({"scopeId":"private","text":null,"tags":[],"status":null,"limit":1}),
    );
    assert_error(&tool, &[denied], "memory_search_grant_denied");
}

#[test]
fn proposal_arguments_are_closed_and_bounded_by_the_grant() {
    let tool = resolved(MemoryToolCapability::ProposeMemoryChange, 1);
    let unknown = call(
        "propose_memory_change",
        proposal(json!("story"), Some(("unknown", json!(true)))),
    );
    assert_error(&tool, &[unknown], "memory_proposal_arguments_invalid");

    let oversized = call("propose_memory_change", proposal(json!("story"), None));
    assert_error(&tool, &[oversized], "memory_proposal_arguments_invalid");
}

#[test]
fn memory_and_custom_calls_are_rejected_as_one_ambiguous_batch() {
    let tool = resolved(MemoryToolCapability::SearchMemory, 8_192);
    let calls = [
        call(
            "search_memory",
            json!({"scopeId":"story","text":null,"tags":[],"status":null,"limit":1}),
        ),
        call("custom_tool", json!({})),
    ];
    assert_error(&tool, &calls, "mixed_memory_tool_batch");
}

fn assert_error(tool: &ResolvedMemoryTool, items: &[LlmTurnItemIr], code: &str) {
    let Err(error) = plan(tool, items) else {
        panic!("memory tool batch must fail")
    };
    assert_eq!(error.code, code);
}

fn plan(
    tool: &ResolvedMemoryTool,
    items: &[LlmTurnItemIr],
) -> Result<Option<super::MemoryToolBatchPlan>, super::MemoryToolBatchError> {
    plan_memory_tool_batch(MemoryToolBatchInput {
        tools: std::slice::from_ref(tool),
        response_items: items,
        model_call_id: "model-call",
        node_instance_id: "node-instance",
        originating_attempt_id: "attempt",
        checkpoint: checkpoint(),
        max_tool_calls: 8,
    })
}

fn resolved(capability: MemoryToolCapability, byte_limit: u64) -> ResolvedMemoryTool {
    ResolvedMemoryTool {
        exposed_name: match capability {
            MemoryToolCapability::SearchMemory => "search_memory",
            MemoryToolCapability::ProposeMemoryChange => "propose_memory_change",
        },
        grant: MemoryToolGrant {
            capability,
            scopes: vec!["story".into()],
            max_results: Some(5),
            max_proposal_bytes: Some(byte_limit),
        },
    }
}

fn call(name: &str, arguments: serde_json::Value) -> LlmTurnItemIr {
    LlmTurnItemIr::AssistantToolCall {
        id: format!("item-{name}"),
        call: ToolCallIr {
            id: format!("call-{name}"),
            provider_call_id: None,
            name: name.into(),
            arguments,
        },
    }
}

fn proposal(
    scope: serde_json::Value,
    extra: Option<(&str, serde_json::Value)>,
) -> serde_json::Value {
    let mut value = json!({
        "scopeId":scope,
        "memoryId":null,
        "expectedHeadCommitId":null,
        "change":{"type":"create","content":{"schemaVersion":1,"text":"fact","tags":[],"attributes":{}}},
        "reason":"durable evidence",
        "evidenceRefs":["message:1"]
    });
    if let Some((key, extra)) = extra {
        value.as_object_mut().unwrap().insert(key.into(), extra);
    }
    value
}

fn checkpoint() -> LlmLoopCheckpoint {
    LlmLoopCheckpoint {
        schema_version: 1,
        node_instance_id: "node-instance".into(),
        last_updated_by_attempt_id: "attempt".into(),
        graph_revision_id: "graph-revision".into(),
        registry_snapshot: ToolRegistrySnapshot {
            revision: "registry-revision".into(),
            entries: vec![],
        },
        context_snapshot_ref: "context".into(),
        read_set_digest: "sha256:read-set".into(),
        model_call_no: 1,
        transcript_ref: "transcript".into(),
        continuation_ref: None,
        active_model_effect: None,
        active_count_effect: None,
        current_batch: vec![],
        model_calls_used: 1,
        count_calls_used: 0,
        tool_calls_used: 0,
        effect_watermark: "model-call".into(),
        wait_ids: vec![],
        checksum: String::new(),
    }
}
