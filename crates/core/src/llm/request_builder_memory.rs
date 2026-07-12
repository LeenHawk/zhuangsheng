use std::collections::BTreeSet;

use serde_json::json;

use crate::{
    graph::{LlmNodeExecutionSnapshot, MemoryToolCapability, MemoryToolGrant},
    schema::{DIALECT_2020_12, JsonSchemaLimits, JsonSchemaSpec},
};

use super::{
    MEMORY_PROPOSAL_TOOL_NAME, MEMORY_SEARCH_TOOL_NAME, ir::ToolDescriptorIr,
    request_builder::LlmRequestBuildError,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedMemoryTool {
    pub exposed_name: &'static str,
    pub grant: MemoryToolGrant,
}

pub(super) fn resolve_memory_tools<'a>(
    execution: &LlmNodeExecutionSnapshot,
    occupied_names: impl Iterator<Item = &'a str>,
) -> Result<(Vec<ToolDescriptorIr>, Vec<ResolvedMemoryTool>), LlmRequestBuildError> {
    let mut names: BTreeSet<String> = occupied_names.map(str::to_owned).collect();
    let grants = execution
        .memory
        .as_ref()
        .map(|memory| memory.tools.as_slice())
        .unwrap_or_default();
    let mut tools = Vec::with_capacity(grants.len());
    let mut resolved = Vec::with_capacity(grants.len());
    for grant in grants {
        let (name, description, document) = match grant.capability {
            MemoryToolCapability::SearchMemory => (
                MEMORY_SEARCH_TOOL_NAME,
                "Search an explicitly granted long-term-memory scope.",
                search_schema(),
            ),
            MemoryToolCapability::ProposeMemoryChange => (
                MEMORY_PROPOSAL_TOOL_NAME,
                "Propose a reviewed change to an explicitly granted long-term-memory scope.",
                proposal_schema(),
            ),
        };
        if !names.insert(name.into()) {
            return Err(LlmRequestBuildError::new(
                "memory_tool_name_conflict",
                format!("memory capability tool name '{name}' is already exposed"),
            ));
        }
        tools.push(ToolDescriptorIr {
            name: name.into(),
            description: Some(description.into()),
            input_schema: schema(document),
        });
        resolved.push(ResolvedMemoryTool {
            exposed_name: name,
            grant: grant.clone(),
        });
    }
    Ok((tools, resolved))
}

fn schema(document: serde_json::Value) -> JsonSchemaSpec {
    JsonSchemaSpec {
        schema_version: 1,
        dialect: DIALECT_2020_12.into(),
        validation_profile_version: 1,
        format_policy_version: 1,
        document,
        limits: JsonSchemaLimits::default(),
    }
}

fn search_schema() -> serde_json::Value {
    json!({
        "type":"object",
        "properties":{
            "scopeId":{"type":"string","minLength":1,"maxLength":256},
            "text":{"type":["string","null"],"minLength":1,"maxLength":4096},
            "tags":{"type":"array","items":{"type":"string","minLength":1,"maxLength":256},"maxItems":64},
            "status":{"type":["string","null"],"enum":["active","obsolete",null]},
            "limit":{"type":"integer","minimum":1,"maximum":100}
        },
        "required":["scopeId","text","tags","status","limit"],
        "additionalProperties":false
    })
}

fn proposal_schema() -> serde_json::Value {
    json!({
        "$defs":{
            "content":{
                "type":"object",
                "properties":{
                    "schemaVersion":{"const":1},
                    "text":{"type":"string","minLength":1,"maxLength":65536},
                    "tags":{"type":"array","items":{"type":"string","minLength":1,"maxLength":256},"maxItems":64},
                    "attributes":{"type":"object"}
                },
                "required":["schemaVersion","text","tags","attributes"],
                "additionalProperties":false
            },
            "change":{
                "oneOf":[
                    {"type":"object","properties":{"type":{"const":"create"},"content":{"$ref":"#/$defs/content"}},"required":["type","content"],"additionalProperties":false},
                    {"type":"object","properties":{"type":{"const":"replace_content"},"content":{"$ref":"#/$defs/content"}},"required":["type","content"],"additionalProperties":false},
                    {"type":"object","properties":{"type":{"const":"mark_obsolete"}},"required":["type"],"additionalProperties":false},
                    {"type":"object","properties":{"type":{"const":"delete_tombstone"}},"required":["type"],"additionalProperties":false}
                ]
            }
        },
        "type":"object",
        "properties":{
            "scopeId":{"type":"string","minLength":1,"maxLength":256},
            "memoryId":{"type":["string","null"],"minLength":1,"maxLength":256},
            "expectedHeadCommitId":{"type":["string","null"],"minLength":1,"maxLength":256},
            "change":{"$ref":"#/$defs/change"},
            "reason":{"type":"string","minLength":1,"maxLength":4096},
            "evidenceRefs":{"type":"array","items":{"type":"string","minLength":1,"maxLength":1024},"maxItems":64}
        },
        "required":["scopeId","memoryId","expectedHeadCommitId","change","reason","evidenceRefs"],
        "additionalProperties":false
    })
}
