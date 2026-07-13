use serde_json::Value;
use thiserror::Error;

use crate::{
    canonical,
    graph::{LlmFinalText, LlmOutputSpec},
    schema,
};

use super::ir::{LlmContentPartIr, LlmTurnItemIr, MessageRole};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{code}: {message}")]
pub struct LlmOutputError {
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmOutputRepairMaterial {
    pub extracted_bytes_digest: String,
    pub error_code: String,
    pub instruction: String,
}

impl LlmOutputError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

pub fn finalize_llm_output(
    output: Option<&LlmOutputSpec>,
    last_model_items: &[LlmTurnItemIr],
    all_model_items: &[LlmTurnItemIr],
) -> Result<Value, LlmOutputError> {
    match output {
        Some(LlmOutputSpec::Json { schema, .. }) => {
            let bytes = extract_json_bytes(last_model_items)?;
            let value = parse_exact_json(&bytes)?;
            schema::validate(schema, &value).map_err(|error| {
                LlmOutputError::new("llm_output_schema_invalid", error.to_string())
            })?;
            Ok(value)
        }
        Some(LlmOutputSpec::Text {
            final_text,
            allow_empty,
        }) => {
            let items = if final_text.unwrap_or(LlmFinalText::LastAssistantTurn)
                == LlmFinalText::AllAssistantText
            {
                all_model_items
            } else {
                last_model_items
            };
            text_output(items, *allow_empty)
        }
        None => text_output(last_model_items, false),
    }
}

pub fn build_llm_output_repair_material(
    error: &LlmOutputError,
    last_model_items: &[LlmTurnItemIr],
) -> LlmOutputRepairMaterial {
    let mut extracted = String::new();
    for item in last_model_items {
        if let LlmTurnItemIr::Message {
            role: MessageRole::Assistant,
            content,
            ..
        } = item
        {
            for part in content {
                if let LlmContentPartIr::Text { text } = part {
                    extracted.push_str(text);
                }
            }
        }
    }
    LlmOutputRepairMaterial {
        extracted_bytes_digest: canonical::hash_bytes(extracted.as_bytes()),
        error_code: error.code.into(),
        instruction: format!(
            "The previous assistant response failed the required JSON output contract ({code}). Return exactly one JSON value matching the configured schema. Do not use Markdown fences, commentary, or any text outside the JSON value.",
            code = error.code,
        ),
    }
}

fn text_output(items: &[LlmTurnItemIr], allow_empty: bool) -> Result<Value, LlmOutputError> {
    let mut output = String::new();
    for item in items {
        if let LlmTurnItemIr::Message {
            role: MessageRole::Assistant,
            content,
            ..
        } = item
        {
            for part in content {
                if let LlmContentPartIr::Text { text } = part {
                    output.push_str(text);
                }
            }
        }
    }
    if output.is_empty() && !allow_empty {
        return Err(LlmOutputError::new(
            "llm_empty_output",
            "LLM response contained no assistant text",
        ));
    }
    Ok(Value::String(output))
}

fn extract_json_bytes(items: &[LlmTurnItemIr]) -> Result<String, LlmOutputError> {
    let mut output = String::new();
    let mut text_parts = 0usize;
    for item in items {
        let LlmTurnItemIr::Message {
            role: MessageRole::Assistant,
            content,
            ..
        } = item
        else {
            continue;
        };
        for part in content {
            match part {
                LlmContentPartIr::Text { text } => {
                    text_parts += 1;
                    output.push_str(text);
                }
                LlmContentPartIr::Image { .. } | LlmContentPartIr::File { .. } => {
                    return Err(LlmOutputError::new(
                        "llm_json_output_non_text",
                        "JSON output messages cannot contain image or file parts",
                    ));
                }
            }
        }
    }
    if text_parts == 0 {
        return Err(LlmOutputError::new(
            "llm_json_output_missing",
            "JSON output contains no assistant text",
        ));
    }
    Ok(output)
}

fn parse_exact_json(input: &str) -> Result<Value, LlmOutputError> {
    canonical::parse(input).map_err(|error| {
        let message = error.to_string();
        LlmOutputError::new(
            if message.contains("duplicate object key") {
                "llm_json_duplicate_key"
            } else {
                "llm_json_parse_failed"
            },
            message,
        )
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::{
        graph::LlmOutputSpec,
        llm::ir::{LlmContentPartIr, LlmTurnItemIr, MessageRole},
        schema::{DIALECT_2020_12, JsonSchemaLimits, JsonSchemaSpec},
    };

    use super::*;

    #[test]
    fn text_modes_use_exact_concatenation_and_allow_empty() {
        let first = message("a", "one");
        let last = message("b", "two");
        let spec = LlmOutputSpec::Text {
            final_text: Some(LlmFinalText::AllAssistantText),
            allow_empty: false,
        };
        let all = [first, last];
        assert_eq!(
            finalize_llm_output(Some(&spec), std::slice::from_ref(&all[1]), &all).unwrap(),
            json!("onetwo")
        );
        assert!(
            finalize_llm_output(
                Some(&LlmOutputSpec::Text {
                    final_text: None,
                    allow_empty: true
                }),
                &[],
                &[],
            )
            .is_ok()
        );
    }

    #[test]
    fn json_rejects_duplicate_keys_and_validates_schema() {
        let duplicate = message("json", r#"{"a":1,"a":2}"#);
        let spec = LlmOutputSpec::Json {
            schema: object_schema(),
            strict: true,
        };
        assert_eq!(
            finalize_llm_output(Some(&spec), &[duplicate], &[])
                .unwrap_err()
                .code,
            "llm_json_duplicate_key"
        );
        let valid = message("json", r#"{"a":1}"#);
        assert_eq!(
            finalize_llm_output(Some(&spec), &[valid], &[]).unwrap(),
            json!({"a":1})
        );
    }

    #[test]
    fn repair_material_records_a_digest_without_echoing_invalid_output() {
        let item = message("json", "private invalid output");
        let error = LlmOutputError::new("llm_json_parse_failed", "parse details");
        let material = build_llm_output_repair_material(&error, &[item]);
        assert_eq!(
            material.extracted_bytes_digest,
            canonical::hash_bytes(b"private invalid output")
        );
        assert_eq!(material.error_code, "llm_json_parse_failed");
        assert!(!material.instruction.contains("private invalid output"));
        assert!(!material.instruction.contains("parse details"));
    }

    fn message(id: &str, text: &str) -> LlmTurnItemIr {
        LlmTurnItemIr::Message {
            id: id.into(),
            role: MessageRole::Assistant,
            content: vec![LlmContentPartIr::Text { text: text.into() }],
            provenance: None,
            placeholder: false,
        }
    }

    fn object_schema() -> JsonSchemaSpec {
        JsonSchemaSpec {
            schema_version: 1,
            dialect: DIALECT_2020_12.into(),
            validation_profile_version: 1,
            format_policy_version: 1,
            document: json!({"type":"object","required":["a"],"additionalProperties":false,"properties":{"a":{"type":"integer"}}}),
            limits: JsonSchemaLimits::default(),
        }
    }
}
