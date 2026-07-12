use serde_json::Value;
use zhuangsheng_core::{
    canonical,
    graph::StaticMemoryRead,
    llm::{
        context::{ContextProvenance, ContextRole, ResolvedContextBinding, ResolvedContextValue},
        ir::{ContextSensitivity, ContextTrust, LlmContentPartIr},
    },
};

use crate::{StorageError, StorageResult};

pub(super) fn artifact_binding(
    read: &StaticMemoryRead,
    scope: &str,
    envelope: Value,
) -> StorageResult<ResolvedContextBinding> {
    if envelope.get("kind").and_then(Value::as_str) != Some("artifact") {
        return Err(StorageError::Integrity(
            "artifact binding kind mismatch".into(),
        ));
    }
    let version = required_string(&envelope, "version")?;
    let found = envelope
        .get("found")
        .and_then(Value::as_bool)
        .ok_or_else(|| StorageError::Integrity("artifact binding found flag missing".into()))?;
    if !found {
        return Ok(ResolvedContextBinding {
            binding_id: read.alias.clone(),
            scope: scope.into(),
            version,
            values: vec![],
            template_value: None,
            template_provenance: Some(provenance(&read.id, ContextSensitivity::Private)),
        });
    }
    let reference = required_value(&envelope, "artifactRef")?;
    let artifact_id = required_string(&reference, "artifactId")?;
    let metadata = required_value(&envelope, "metadata")?;
    let artifact_provenance = provenance(&artifact_id, classification(&metadata)?);
    let mut values = vec![artifact_value(
        format!("{artifact_id}:metadata"),
        value_content(&metadata)?,
        artifact_provenance.clone(),
        "artifact_view:metadata",
    )?];
    if let Some(text) = envelope.get("text").and_then(Value::as_str) {
        values.push(artifact_value(
            format!("{artifact_id}:text"),
            vec![LlmContentPartIr::Text { text: text.into() }],
            artifact_provenance.clone(),
            "artifact_view:text",
        )?);
    }
    Ok(ResolvedContextBinding {
        binding_id: read.alias.clone(),
        scope: scope.into(),
        version,
        values,
        template_value: Some(reference),
        template_provenance: Some(artifact_provenance),
    })
}

fn artifact_value(
    id: String,
    content: Vec<LlmContentPartIr>,
    provenance: ContextProvenance,
    view_tag: &str,
) -> StorageResult<ResolvedContextValue> {
    Ok(ResolvedContextValue::Data {
        id,
        content_hash: canonical::hash(&content)?,
        content,
        provenance,
        allowed_roles: vec![ContextRole::Context],
        relevance_score_micros: None,
        tags: vec![view_tag.into()],
    })
}

fn classification(metadata: &Value) -> StorageResult<ContextSensitivity> {
    match metadata.get("classification").and_then(Value::as_str) {
        Some("public") => Ok(ContextSensitivity::Public),
        Some("private") => Ok(ContextSensitivity::Private),
        Some("sensitive") => Ok(ContextSensitivity::Sensitive),
        _ => Err(StorageError::Integrity(
            "artifact classification is invalid".into(),
        )),
    }
}

fn provenance(source_id: &str, sensitivity: ContextSensitivity) -> ContextProvenance {
    ContextProvenance {
        source_type: "artifact".into(),
        source_id: source_id.into(),
        trust: ContextTrust::ExternalUntrusted,
        sensitivity,
    }
}

fn value_content(value: &Value) -> StorageResult<Vec<LlmContentPartIr>> {
    Ok(vec![LlmContentPartIr::Text {
        text: canonical::to_string(value)?,
    }])
}

fn required_value(value: &Value, key: &str) -> StorageResult<Value> {
    value
        .get(key)
        .cloned()
        .ok_or_else(|| StorageError::Integrity(format!("artifact binding field missing: {key}")))
}

fn required_string(value: &Value, key: &str) -> StorageResult<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| StorageError::Integrity(format!("artifact binding field missing: {key}")))
}
