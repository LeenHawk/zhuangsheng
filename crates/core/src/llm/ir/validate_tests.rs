use super::*;
use crate::graph::ToolChoiceIr;

#[test]
fn transcript_requires_ordered_tool_call_result_pairing() {
    let mut request = base_request();
    request.transcript.push(LlmTurnItemIr::ToolResult {
        id: "result-1".into(),
        tool_call_id: "call-1".into(),
        tool_name: "lookup".into(),
        outcome: ToolResultOutcome::Success,
        content: vec![LlmContentPartIr::Text { text: "ok".into() }],
    });
    assert_eq!(
        validate_request_ir(&request).unwrap_err().code,
        "orphan_tool_result"
    );
}

#[test]
fn metadata_and_named_tool_choice_are_closed() {
    let mut request = base_request();
    request.tool_choice = Some(ToolChoiceIr::Named {
        name: "missing".into(),
    });
    assert_eq!(
        validate_request_ir(&request).unwrap_err().code,
        "unknown_tool_choice"
    );
    request.tool_choice = None;
    request.metadata.insert(
        "api_token".into(),
        MetadataValue::String("not allowed".into()),
    );
    assert_eq!(
        validate_request_ir(&request).unwrap_err().code,
        "invalid_metadata"
    );
}

fn base_request() -> LlmRequestIr {
    LlmRequestIr {
        model: "model-1".into(),
        instructions: vec![],
        transcript: vec![LlmTurnItemIr::Message {
            id: "message-1".into(),
            role: MessageRole::User,
            content: vec![LlmContentPartIr::Text { text: "hi".into() }],
            provenance: None,
        }],
        tools: vec![],
        hosted_tools: vec![],
        tool_choice: None,
        response_format: Some(ResponseFormatIr::Text),
        generation: None,
        extensions: None,
        metadata: Default::default(),
        continuation: None,
    }
}
