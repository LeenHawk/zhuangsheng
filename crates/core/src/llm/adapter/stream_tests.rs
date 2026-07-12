use serde_json::json;

use crate::llm::{
    ContentGenerationKind, LlmOperationExecutionPin, Operation, OperationKey, ir::StreamFinalizer,
};

use super::*;

#[test]
fn responses_stream_normalizes_sequence_and_terminal_identity() {
    let mut decoder =
        OpenAiResponsesStreamDecoder::new(pin(ContentGenerationKind::OpenAiResponses), "call-1")
            .unwrap();
    let fixtures = [
        json!({
            "type":"response.created","sequence_number":0,
            "response":{"id":"response-1","created_at":1,"object":"response","output":[],"status":"in_progress"}
        }),
        json!({
            "type":"response.output_item.added","sequence_number":1,"output_index":0,
            "item":{"type":"message","id":"provider-message-1","role":"assistant","status":"in_progress","content":[]}
        }),
        json!({
            "type":"response.output_text.delta","sequence_number":2,"content_index":0,
            "delta":"hello","item_id":"provider-message-1","output_index":0
        }),
        json!({
            "type":"response.completed","sequence_number":3,
            "response":{
                "id":"response-1","created_at":1,"object":"response","status":"completed",
                "output":[{
                    "type":"message","id":"provider-message-1","role":"assistant","status":"completed",
                    "content":[{"type":"output_text","text":"hello","annotations":[]}]
                }]
            }
        }),
    ];
    let mut finalizer = StreamFinalizer::default();
    for fixture in fixtures {
        push_events(
            &mut finalizer,
            decoder
                .push(&serde_json::to_vec(&fixture).unwrap())
                .unwrap(),
        );
    }
    finalizer.finish().unwrap();
}

#[test]
fn responses_stream_emits_hosted_tool_lifecycle_before_terminal() {
    let mut decoder =
        OpenAiResponsesStreamDecoder::new(pin(ContentGenerationKind::OpenAiResponses), "call-1")
            .unwrap();
    let fixtures = [
        json!({
            "type":"response.created","sequence_number":0,
            "response":{"id":"response-1","created_at":1,"object":"response","output":[],"status":"in_progress"}
        }),
        json!({
            "type":"response.output_item.added","sequence_number":1,"output_index":0,
            "item":{"type":"web_search_call","id":"search-1","status":"in_progress","action":{"type":"search","query":"lore"}}
        }),
        json!({
            "type":"response.output_item.done","sequence_number":2,"output_index":0,
            "item":{"type":"web_search_call","id":"search-1","status":"completed","action":{"type":"search","query":"lore"}}
        }),
        json!({
            "type":"response.output_item.added","sequence_number":3,"output_index":1,
            "item":{"type":"message","id":"message-1","role":"assistant","status":"in_progress","content":[]}
        }),
        json!({
            "type":"response.output_text.delta","sequence_number":4,"content_index":0,
            "delta":"found","item_id":"message-1","output_index":1
        }),
        json!({
            "type":"response.completed","sequence_number":5,
            "response":{
                "id":"response-1","created_at":1,"object":"response","status":"completed",
                "output":[
                    {"type":"web_search_call","id":"search-1","status":"completed","action":{"type":"search","query":"lore"}},
                    {"type":"message","id":"message-1","role":"assistant","status":"completed","content":[{"type":"output_text","text":"found","annotations":[]}]}
                ]
            }
        }),
    ];
    let mut finalizer = StreamFinalizer::default();
    let mut hosted_events = 0;
    for fixture in fixtures {
        let batch = decoder
            .push(&serde_json::to_vec(&fixture).unwrap())
            .unwrap();
        hosted_events += batch
            .events
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    crate::llm::ir::LlmStreamEventIr::HostedToolEvent { .. }
                )
            })
            .count();
        push_events(&mut finalizer, batch);
    }
    assert_eq!(hosted_events, 1);
    finalizer.finish().unwrap();
}

#[test]
fn chat_stream_aggregates_tool_arguments_before_terminal() {
    let mut decoder =
        OpenAiChatStreamDecoder::new(pin(ContentGenerationKind::OpenAiChatCompletions), "call-1")
            .unwrap();
    let fixtures = [
        json!({
            "id":"chat-stream-1","created":1,"model":"model-1","object":"chat.completion.chunk",
            "choices":[{"index":0,"finish_reason":null,"delta":{
                "role":"assistant","content":"checking","tool_calls":[{
                    "index":0,"id":"provider-call-1","type":"function",
                    "function":{"name":"lookup","arguments":"{\"query\":"}
                }]
            }}]
        }),
        json!({
            "id":"chat-stream-1","created":1,"model":"model-1","object":"chat.completion.chunk",
            "choices":[{"index":0,"finish_reason":"tool_calls","delta":{
                "tool_calls":[{"index":0,"function":{"arguments":"\"moon\"}"}}]
            }}]
        }),
        json!({
            "id":"chat-stream-1","created":1,"model":"model-1","object":"chat.completion.chunk",
            "choices":[],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}
        }),
    ];
    let mut finalizer = StreamFinalizer::default();
    for fixture in fixtures {
        push_events(
            &mut finalizer,
            decoder
                .push(&serde_json::to_vec(&fixture).unwrap())
                .unwrap(),
        );
    }
    push_events(&mut finalizer, decoder.finish().unwrap());
    finalizer.finish().unwrap();
}

#[test]
fn gemini_stream_aggregates_text_into_terminal_candidate() {
    let mut decoder =
        GeminiStreamDecoder::new(pin(ContentGenerationKind::GeminiGenerateContent), "call-1")
            .unwrap();
    let fixtures = [
        json!({"candidates":[{"index":0,"content":{"role":"model","parts":[{"text":"hel"}]}}]}),
        json!({
            "candidates":[{"index":0,"finishReason":"STOP","content":{"role":"model","parts":[{"text":"lo"}]}}],
            "usageMetadata":{"promptTokenCount":4,"candidatesTokenCount":1,"totalTokenCount":5}
        }),
    ];
    let mut finalizer = StreamFinalizer::default();
    for fixture in fixtures {
        push_events(
            &mut finalizer,
            decoder
                .push(&serde_json::to_vec(&fixture).unwrap())
                .unwrap(),
        );
    }
    decoder.finish().unwrap();
    finalizer.finish().unwrap();
}

#[test]
fn claude_stream_requires_block_lifecycle_and_builds_terminal() {
    let mut decoder =
        ClaudeStreamDecoder::new(pin(ContentGenerationKind::ClaudeMessages), "call-1").unwrap();
    let fixtures = [
        json!({
            "type":"message_start","message":{
                "id":"claude-message-1","type":"message","role":"assistant","content":[],
                "model":"model-1","stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":4}
            }
        }),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hello"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":1}}),
        json!({"type":"message_stop"}),
    ];
    let mut finalizer = StreamFinalizer::default();
    for fixture in fixtures {
        push_events(
            &mut finalizer,
            decoder
                .push(&serde_json::to_vec(&fixture).unwrap())
                .unwrap(),
        );
    }
    decoder.finish().unwrap();
    finalizer.finish().unwrap();
}

fn push_events(finalizer: &mut StreamFinalizer, batch: DecodedStreamBatch) {
    for event in batch.events {
        finalizer.push(event).unwrap();
    }
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
