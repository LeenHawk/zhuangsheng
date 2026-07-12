use serde_json::{Value, json};

use crate::{
    graph::GenerationOptionsIr,
    llm::{
        ContentGenerationKind, LlmOperationExecutionPin, Operation, OperationKey,
        ir::{
            LlmContentPartIr, LlmRequestIr, LlmTurnItemIr, MessageRole, ResponseFormatIr,
            ToolResultOutcome,
        },
    },
};

use super::*;

#[test]
fn responses_encoder_applies_hard_output_limit() {
    let pin = pin(ContentGenerationKind::OpenAiResponses);
    let mut request = base_request();
    request.generation = Some(GenerationOptionsIr {
        temperature: Some(0.5),
        top_p: None,
        max_output_tokens: Some(999),
        stop: Vec::new(),
        seed: None,
    });
    let wire = encode_openai_responses_request(
        &pin,
        &request,
        &AdapterResources::default(),
        AdapterExecutionOptions {
            stream: false,
            max_output_tokens: 100,
        },
    )
    .unwrap();
    let value: Value = serde_json::from_slice(wire.body()).unwrap();
    assert_eq!(value["max_output_tokens"], 100);
    assert_eq!(value["input"][0]["role"], "user");
}

#[test]
fn responses_terminal_roundtrips_provider_tool_call_id() {
    let pin = pin(ContentGenerationKind::OpenAiResponses);
    let terminal = json!({
        "id":"response-1",
        "created_at":1,
        "object":"response",
        "output":[
            {
                "type":"message",
                "id":"message-1",
                "role":"assistant",
                "status":"completed",
                "content":[{"type":"output_text","text":"checking","annotations":[]}]
            },
            {
                "type":"function_call",
                "id":"function-1",
                "call_id":"provider-call-1",
                "name":"lookup",
                "arguments":"{\"query\":\"moon\"}",
                "status":"completed"
            }
        ],
        "status":"completed",
        "usage":{
            "input_tokens":10,
            "output_tokens":5,
            "total_tokens":15,
            "output_tokens_details":{"reasoning_tokens":1}
        }
    });
    let mut decoded = decode_openai_responses_terminal(
        &pin,
        "model-call-1",
        &serde_json::to_vec(&terminal).unwrap(),
    )
    .unwrap();
    let call_id = match &decoded.response.items[1] {
        LlmTurnItemIr::AssistantToolCall { call, .. } => call.id.clone(),
        _ => panic!("expected tool call"),
    };
    decoded.response.items.push(LlmTurnItemIr::ToolResult {
        id: "tool-result-1".into(),
        tool_call_id: call_id,
        tool_name: "lookup".into(),
        outcome: ToolResultOutcome::Success,
        content: vec![LlmContentPartIr::Text {
            text: "found".into(),
        }],
    });
    let mut request = base_request();
    request.transcript = decoded.response.items;
    let wire =
        encode_openai_responses_request(&pin, &request, &AdapterResources::default(), options())
            .unwrap();
    let value: Value = serde_json::from_slice(wire.body()).unwrap();
    let input = value["input"].as_array().unwrap();
    assert_eq!(input[1]["call_id"], "provider-call-1");
    assert_eq!(input[2]["call_id"], "provider-call-1");
}

#[test]
fn chat_terminal_roundtrips_provider_tool_call_id() {
    let pin = pin(ContentGenerationKind::OpenAiChatCompletions);
    let terminal = json!({
        "id":"chat-1",
        "model":"model-1",
        "choices":[{
            "index":0,
            "finish_reason":"tool_calls",
            "message":{
                "role":"assistant",
                "content":"checking",
                "tool_calls":[{
                    "type":"function",
                    "id":"provider-call-1",
                    "function":{"name":"lookup","arguments":"{\"query\":\"moon\"}"}
                }]
            }
        }]
    });
    let mut decoded = decode_openai_chat_terminal(
        &pin,
        "model-call-1",
        &serde_json::to_vec(&terminal).unwrap(),
    )
    .unwrap();
    let call_id = match &decoded.response.items[1] {
        LlmTurnItemIr::AssistantToolCall { call, .. } => call.id.clone(),
        _ => panic!("expected tool call"),
    };
    decoded.response.items.push(LlmTurnItemIr::ToolResult {
        id: "tool-result-1".into(),
        tool_call_id: call_id,
        tool_name: "lookup".into(),
        outcome: ToolResultOutcome::Success,
        content: vec![LlmContentPartIr::Text {
            text: "found".into(),
        }],
    });
    let mut request = base_request();
    request.transcript = decoded.response.items;
    let wire = encode_openai_chat_request(&pin, &request, &AdapterResources::default(), options())
        .unwrap();
    let value: Value = serde_json::from_slice(wire.body()).unwrap();
    let messages = value["messages"].as_array().unwrap();
    assert_eq!(messages[1]["tool_calls"][0]["id"], "provider-call-1");
    assert_eq!(messages[2]["tool_call_id"], "provider-call-1");
}

#[test]
fn claude_terminal_roundtrips_tool_use_id_and_preserves_thinking_sidecar() {
    let pin = pin(ContentGenerationKind::ClaudeMessages);
    let terminal = json!({
        "id":"message-1",
        "type":"message",
        "role":"assistant",
        "model":"model-1",
        "content":[
            {"type":"thinking","thinking":"private","signature":"signed-1"},
            {"type":"text","text":"checking"},
            {
                "type":"tool_use",
                "id":"provider-tool-1",
                "name":"lookup",
                "input":{"query":"moon"}
            }
        ],
        "stop_reason":"tool_use",
        "stop_sequence":null,
        "usage":{"input_tokens":10,"output_tokens":5}
    });
    let mut decoded = decode_claude_terminal(
        &pin,
        "model-call-1",
        &serde_json::to_vec(&terminal).unwrap(),
    )
    .unwrap();
    assert_eq!(decoded.sensitive_entries.len(), 1);
    assert_eq!(decoded.opaque_attachments.len(), 1);
    let call_id = match &decoded.response.items[2] {
        LlmTurnItemIr::AssistantToolCall { call, .. } => call.id.clone(),
        _ => panic!("expected tool call"),
    };
    decoded.response.items.remove(0);
    decoded.response.items.push(LlmTurnItemIr::ToolResult {
        id: "tool-result-1".into(),
        tool_call_id: call_id,
        tool_name: "lookup".into(),
        outcome: ToolResultOutcome::Success,
        content: vec![LlmContentPartIr::Text {
            text: "found".into(),
        }],
    });
    let mut request = base_request();
    request.transcript = decoded.response.items;
    let wire =
        encode_claude_request(&pin, &request, &AdapterResources::default(), options()).unwrap();
    let value: Value = serde_json::from_slice(wire.body()).unwrap();
    assert_eq!(value["messages"][0]["content"][1]["id"], "provider-tool-1");
    assert_eq!(
        value["messages"][1]["content"][0]["tool_use_id"],
        "provider-tool-1"
    );
}

#[test]
fn gemini_terminal_roundtrips_function_id_and_thought_signature() {
    let pin = pin(ContentGenerationKind::GeminiGenerateContent);
    let terminal = json!({
        "candidates":[{
            "index":0,
            "finishReason":"STOP",
            "content":{
                "role":"model",
                "parts":[
                    {"text":"private","thought":true,"thoughtSignature":"signed-1"},
                    {"text":"checking"},
                    {"functionCall":{"id":"provider-call-1","name":"lookup","args":{"query":"moon"}}}
                ]
            }
        }],
        "usageMetadata":{
            "promptTokenCount":10,
            "candidatesTokenCount":5,
            "thoughtsTokenCount":2,
            "totalTokenCount":15
        }
    });
    let mut decoded = decode_gemini_terminal(
        &pin,
        "model-call-1",
        &serde_json::to_vec(&terminal).unwrap(),
    )
    .unwrap();
    assert_eq!(decoded.sensitive_entries.len(), 1);
    assert_eq!(decoded.opaque_attachments.len(), 1);
    let call_id = match &decoded.response.items[2] {
        LlmTurnItemIr::AssistantToolCall { call, .. } => call.id.clone(),
        _ => panic!("expected tool call"),
    };
    decoded.response.items.remove(0);
    decoded.response.items.push(LlmTurnItemIr::ToolResult {
        id: "tool-result-1".into(),
        tool_call_id: call_id,
        tool_name: "lookup".into(),
        outcome: ToolResultOutcome::Success,
        content: vec![LlmContentPartIr::Text {
            text: "found".into(),
        }],
    });
    let mut request = base_request();
    request.transcript = decoded.response.items;
    let wire =
        encode_gemini_request(&pin, &request, &AdapterResources::default(), options()).unwrap();
    let value: Value = serde_json::from_slice(wire.body()).unwrap();
    assert_eq!(
        value["contents"][0]["parts"][1]["functionCall"]["id"],
        "provider-call-1"
    );
    assert_eq!(
        value["contents"][1]["parts"][0]["functionResponse"]["id"],
        "provider-call-1"
    );
}

fn pin(kind: ContentGenerationKind) -> LlmOperationExecutionPin {
    LlmOperationExecutionPin {
        channel_revision_id: "revision-1".into(),
        model_id: "model-1".into(),
        operation_key: OperationKey::content_generation(Operation::GenerateContent, kind),
        operation_taxonomy_version: 1,
        adapter_decoder_version: 1,
    }
}

fn options() -> AdapterExecutionOptions {
    AdapterExecutionOptions {
        stream: false,
        max_output_tokens: 128,
    }
}

fn base_request() -> LlmRequestIr {
    LlmRequestIr {
        model: "model-1".into(),
        instructions: Vec::new(),
        transcript: vec![LlmTurnItemIr::Message {
            id: "user-message-1".into(),
            role: MessageRole::User,
            content: vec![LlmContentPartIr::Text { text: "hi".into() }],
            provenance: None,
            placeholder: false,
        }],
        tools: Vec::new(),
        hosted_tools: Vec::new(),
        tool_choice: None,
        response_format: Some(ResponseFormatIr::Text),
        generation: None,
        extensions: None,
        metadata: Default::default(),
        continuation: None,
    }
}
