use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{DomainError, DomainResult, ValidationIssue, canonical};

pub const DIALECT_2020_12: &str = "https://json-schema.org/draft/2020-12/schema";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonSchemaLimits {
    pub max_schema_bytes: u64,
    pub max_schema_nodes: u64,
    pub max_schema_depth: u64,
    pub max_local_refs: u64,
    pub max_ref_depth: u64,
    pub max_regex_bytes: u64,
    pub max_instance_bytes: u64,
    pub max_instance_depth: u64,
    pub max_collection_items: u64,
    pub max_string_bytes: u64,
    pub max_number_digits: u64,
    pub max_number_exponent_magnitude: u64,
    pub max_validation_errors: u64,
    pub validation_fuel: u64,
}

impl Default for JsonSchemaLimits {
    fn default() -> Self {
        Self {
            max_schema_bytes: 256 * 1024,
            max_schema_nodes: 4096,
            max_schema_depth: 128,
            max_local_refs: 1024,
            max_ref_depth: 64,
            max_regex_bytes: 4096,
            max_instance_bytes: 16 * 1024 * 1024,
            max_instance_depth: 128,
            max_collection_items: 100_000,
            max_string_bytes: 8 * 1024 * 1024,
            max_number_digits: 128,
            max_number_exponent_magnitude: 1024,
            max_validation_errors: 32,
            validation_fuel: 1_000_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonSchemaSpec {
    pub schema_version: u32,
    pub dialect: String,
    pub validation_profile_version: u32,
    pub format_policy_version: u32,
    pub document: Value,
    pub limits: JsonSchemaLimits,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaCompilationDraft {
    pub canonical_document_hash: String,
    pub schema_hash: String,
    pub canonical_source: String,
    pub compiled_payload: String,
    pub compiled_payload_hash: String,
    pub compiler_id: String,
    pub compiler_version: String,
    pub payload_format_version: u32,
}

pub fn compile(spec: &JsonSchemaSpec) -> DomainResult<SchemaCompilationDraft> {
    let mut issues = Vec::new();
    if spec.schema_version != 1 {
        issues.push(issue("unsupported_schema_version", "/schemaVersion"));
    }
    if spec.dialect != DIALECT_2020_12 {
        issues.push(issue("unsupported_schema_dialect", "/dialect"));
    }
    if spec.validation_profile_version != 1 || spec.format_policy_version != 1 {
        issues.push(issue("unsupported_schema_profile", "/"));
    }
    let limit_json = serde_json::to_value(&spec.limits)
        .map_err(|error| DomainError::Serialization(error.to_string()))?;
    if limit_json
        .as_object()
        .is_some_and(|values| values.values().any(|value| value.as_u64() == Some(0)))
    {
        issues.push(issue("schema_limit_not_positive", "/limits"));
    }
    let mut refs = BTreeSet::new();
    let mut nodes = 0_u64;
    walk(
        &spec.document,
        "",
        0,
        spec,
        &mut refs,
        &mut nodes,
        &mut issues,
    );
    let source = canonical::to_string(spec)?;
    if source.len() as u64 > spec.limits.max_schema_bytes {
        issues.push(issue("schema_bytes_exceeded", "/document"));
    }
    if !issues.is_empty() {
        return Err(DomainError::SchemaValidation(issues));
    }
    let document_identity = json!({
        "schemaVersion": spec.schema_version,
        "dialect": spec.dialect,
        "validationProfileVersion": spec.validation_profile_version,
        "formatPolicyVersion": spec.format_policy_version,
        "document": spec.document,
    });
    let canonical_document_hash = canonical::hash(&document_identity)?;
    let schema_hash = canonical::hash(spec)?;
    let payload = canonical::to_string(&json!({
        "payloadFormatVersion": 1,
        "schemaHash": schema_hash,
        "localRefs": refs,
        "schemaNodes": nodes,
    }))?;
    Ok(SchemaCompilationDraft {
        canonical_document_hash,
        schema_hash,
        canonical_source: source,
        compiled_payload_hash: canonical::hash_bytes(payload.as_bytes()),
        compiled_payload: payload,
        compiler_id: "zhuangsheng-json-schema".into(),
        compiler_version: env!("CARGO_PKG_VERSION").into(),
        payload_format_version: 1,
    })
}

fn walk(
    value: &Value,
    path: &str,
    depth: u64,
    spec: &JsonSchemaSpec,
    refs: &mut BTreeSet<String>,
    nodes: &mut u64,
    issues: &mut Vec<ValidationIssue>,
) {
    *nodes += 1;
    if *nodes > spec.limits.max_schema_nodes || depth > spec.limits.max_schema_depth {
        issues.push(issue("schema_structure_limit_exceeded", path));
        return;
    }
    let Some(object) = value.as_object() else {
        if !value.is_boolean() {
            issues.push(issue("schema_must_be_object_or_boolean", path));
        }
        return;
    };
    for (key, child) in object {
        let child_path = format!("{path}/{}", escape(key));
        if !ALLOWED.contains(&key.as_str()) {
            issues.push(issue("unsupported_schema_keyword", &child_path));
            continue;
        }
        match key.as_str() {
            "$ref" => check_ref(child, &child_path, spec, refs, issues),
            "$defs" | "properties" | "patternProperties" | "dependentSchemas" => {
                if let Some(map) = child.as_object() {
                    for (name, schema) in map {
                        walk(
                            schema,
                            &format!("{child_path}/{}", escape(name)),
                            depth + 1,
                            spec,
                            refs,
                            nodes,
                            issues,
                        );
                    }
                }
            }
            "allOf" | "anyOf" | "oneOf" | "prefixItems" => {
                if let Some(items) = child.as_array() {
                    for (index, schema) in items.iter().enumerate() {
                        walk(
                            schema,
                            &format!("{child_path}/{index}"),
                            depth + 1,
                            spec,
                            refs,
                            nodes,
                            issues,
                        );
                    }
                }
            }
            "items" | "contains" | "not" | "if" | "then" | "else" | "propertyNames" => {
                walk(child, &child_path, depth + 1, spec, refs, nodes, issues);
            }
            "pattern"
                if child
                    .as_str()
                    .is_some_and(|value| value.len() as u64 > spec.limits.max_regex_bytes) =>
            {
                issues.push(issue("schema_regex_bytes_exceeded", &child_path));
            }
            _ => {}
        }
    }
}

fn check_ref(
    value: &Value,
    path: &str,
    spec: &JsonSchemaSpec,
    refs: &mut BTreeSet<String>,
    issues: &mut Vec<ValidationIssue>,
) {
    let Some(reference) = value.as_str() else {
        issues.push(issue("schema_ref_not_string", path));
        return;
    };
    if reference != "#" && !reference.starts_with("#/$defs/") {
        issues.push(issue("schema_remote_ref_forbidden", path));
    }
    refs.insert(reference.into());
    if refs.len() as u64 > spec.limits.max_local_refs {
        issues.push(issue("schema_local_refs_exceeded", path));
    }
}

fn issue(code: &str, path: impl Into<String>) -> ValidationIssue {
    ValidationIssue::error(code, path, code.replace('_', " "))
}

fn escape(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

const ALLOWED: &[&str] = &[
    "$schema",
    "$defs",
    "$ref",
    "type",
    "enum",
    "const",
    "multipleOf",
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "minLength",
    "maxLength",
    "pattern",
    "format",
    "properties",
    "required",
    "additionalProperties",
    "patternProperties",
    "propertyNames",
    "minProperties",
    "maxProperties",
    "prefixItems",
    "items",
    "contains",
    "minContains",
    "maxContains",
    "minItems",
    "maxItems",
    "uniqueItems",
    "allOf",
    "anyOf",
    "oneOf",
    "not",
    "if",
    "then",
    "else",
    "dependentRequired",
    "dependentSchemas",
    "title",
    "description",
    "default",
    "examples",
    "deprecated",
    "readOnly",
    "writeOnly",
    "$comment",
];

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn spec(document: Value) -> JsonSchemaSpec {
        JsonSchemaSpec {
            schema_version: 1,
            dialect: DIALECT_2020_12.into(),
            validation_profile_version: 1,
            format_policy_version: 1,
            document,
            limits: JsonSchemaLimits::default(),
        }
    }

    #[test]
    fn compiles_local_closed_schema() {
        let result = compile(&spec(json!({"type":"object","properties":{"name":{"type":"string"}},"additionalProperties":false}))).unwrap();
        assert!(result.schema_hash.starts_with("sha256:"));
    }

    #[test]
    fn rejects_unknown_keyword_and_remote_ref() {
        let error = compile(&spec(
            json!({"typo":"string","$ref":"https://example.com/x"}),
        ))
        .unwrap_err();
        assert!(matches!(error, DomainError::SchemaValidation(issues) if issues.len() == 2));
    }
}
