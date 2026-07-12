use serde_json::json;
use zhuangsheng_core::{
    graph::{DraftNodeKind, GraphDraft, LlmOutputSpec},
    schema::{DIALECT_2020_12, JsonSchemaLimits, JsonSchemaSpec},
};

use super::LlmGraphFixture;

pub(super) fn graph_draft(
    graph_id: &str,
    channel_id: &str,
    preset_id: &str,
    fixture: LlmGraphFixture,
) -> GraphDraft {
    let LlmGraphFixture {
        json_output,
        tool,
        hosted,
        streaming,
        memory,
        ..
    } = fixture;
    let mut draft: GraphDraft = serde_json::from_value(json!({
        "graphId":graph_id,
        "nodes":[
            {"id":"input","kind":"input","runInputSelector":{"type":"whole_value"}},
            {
                "id":"generate",
                "kind":"llm",
                "model":{
                    "channelId":channel_id,
                    "modelId":"roleplay-model",
                    "operationKey":{"operation":"generate_content","kind":"open_ai_responses"}
                },
                "context":{"type":"preset","presetId":preset_id}
            },
            {"id":"output","kind":"output","outputKey":"reply"}
        ],
        "edges":[
            {"from":{"nodeId":"input","output":"default"},"to":{"nodeId":"generate","input":"default"}},
            {"from":{"nodeId":"generate","output":"default"},"to":{"nodeId":"output","input":"default"}}
        ],
        "outputContract":[{"key":"reply","collection":"single","required":true}]
    }))
    .unwrap();
    let config = draft
        .nodes
        .iter_mut()
        .find_map(|node| match &mut node.kind {
            DraftNodeKind::Llm { config } => Some(config),
            _ => None,
        })
        .unwrap();
    if json_output {
        config.output = Some(LlmOutputSpec::Json {
            schema: JsonSchemaSpec {
                schema_version: 1,
                dialect: DIALECT_2020_12.into(),
                validation_profile_version: 1,
                format_policy_version: 1,
                document: json!({
                    "type":"object",
                    "required":["reply"],
                    "additionalProperties":false,
                    "properties":{"reply":{"type":"string"}}
                }),
                limits: JsonSchemaLimits::default(),
            },
            strict: true,
        });
    }
    if let Some(tool) = tool {
        config.tools.push(tool);
    }
    if let Some(hosted) = hosted {
        config.hosted_tools.push(hosted);
    }
    if let Some(streaming) = streaming {
        config.streaming = Some(streaming);
    }
    if let Some(memory) = memory {
        config.memory = Some(memory);
    }
    draft
}
