use jsonschema::{PatternOptions, Validator};
use serde_json::Value;

use crate::{DomainError, DomainResult, ValidationIssue, canonical};

use super::JsonSchemaSpec;

pub(super) fn build_validator(schema: &Value) -> Result<Validator, String> {
    jsonschema::draft202012::options()
        .should_validate_formats(true)
        .should_ignore_unknown_formats(false)
        .with_pattern_options(
            PatternOptions::regex()
                .size_limit(1024 * 1024)
                .dfa_size_limit(1024 * 1024),
        )
        .build(schema)
        .map_err(|error| error.to_string())
}

pub fn validate(spec: &JsonSchemaSpec, instance: &Value) -> DomainResult<()> {
    super::compile(spec)?;
    validate_instance_limits(spec, instance, 0)?;
    let bytes = canonical::to_vec(instance)?;
    if bytes.len() as u64 > spec.limits.max_instance_bytes {
        return Err(limit("instance bytes exceeded"));
    }
    let validator = build_validator(&spec.document).map_err(|message| {
        DomainError::SchemaValidation(vec![ValidationIssue::error(
            "schema_compile_failed",
            "/",
            message,
        )])
    })?;
    let issues: Vec<_> = validator
        .iter_errors(instance)
        .take(spec.limits.max_validation_errors as usize)
        .map(|error| {
            ValidationIssue::error(
                "schema_instance_invalid",
                error.instance_path().as_str(),
                error.to_string(),
            )
        })
        .collect();
    if issues.is_empty() {
        Ok(())
    } else {
        Err(DomainError::SchemaValidation(issues))
    }
}

fn validate_instance_limits(spec: &JsonSchemaSpec, value: &Value, depth: u64) -> DomainResult<()> {
    if depth > spec.limits.max_instance_depth {
        return Err(limit("instance depth exceeded"));
    }
    match value {
        Value::String(value) if value.len() as u64 > spec.limits.max_string_bytes => {
            Err(limit("string bytes exceeded"))
        }
        Value::Number(number) => validate_number_limits(spec, &number.to_string()),
        Value::Array(values) => {
            if values.len() as u64 > spec.limits.max_collection_items {
                return Err(limit("collection items exceeded"));
            }
            for value in values {
                validate_instance_limits(spec, value, depth + 1)?;
            }
            Ok(())
        }
        Value::Object(values) => {
            if values.len() as u64 > spec.limits.max_collection_items {
                return Err(limit("collection items exceeded"));
            }
            for value in values.values() {
                validate_instance_limits(spec, value, depth + 1)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn validate_number_limits(spec: &JsonSchemaSpec, raw: &str) -> DomainResult<()> {
    canonical::validate_number(
        raw,
        spec.limits.max_number_digits as usize,
        spec.limits.max_number_exponent_magnitude as i64,
    )
    .map_err(|_| limit("number limits exceeded"))
}

fn limit(message: &str) -> DomainError {
    DomainError::SchemaValidation(vec![ValidationIssue::error(
        "schema_validation_limit_exceeded",
        "/",
        message,
    )])
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::schema::{DIALECT_2020_12, JsonSchemaLimits};

    #[test]
    fn validates_instances_without_coercion() {
        let spec = JsonSchemaSpec {
            schema_version: 1,
            dialect: DIALECT_2020_12.into(),
            validation_profile_version: 1,
            format_policy_version: 1,
            document: json!({"type":"object","properties":{"count":{"type":"integer"}},"required":["count"],"additionalProperties":false}),
            limits: JsonSchemaLimits::default(),
        };
        assert!(validate(&spec, &json!({"count": 2})).is_ok());
        assert!(validate(&spec, &json!({"count": "2"})).is_err());
    }

    #[test]
    fn validates_exact_decimal_semantics_and_tighter_normalized_limits() {
        let mut spec = JsonSchemaSpec {
            schema_version: 1,
            dialect: DIALECT_2020_12.into(),
            validation_profile_version: 1,
            format_policy_version: 1,
            document: json!({"type":"number","minimum":9007199254740993_u64,"multipleOf":0.1}),
            limits: JsonSchemaLimits::default(),
        };
        let exact = canonical::parse("9007199254740993.1").unwrap();
        assert!(validate(&spec, &exact).is_ok());

        spec.limits.max_number_exponent_magnitude = 2;
        let normalized_overflow = canonical::parse("10e2").unwrap();
        assert!(matches!(
            validate(&spec, &normalized_overflow),
            Err(DomainError::SchemaValidation(_))
        ));
    }
}
