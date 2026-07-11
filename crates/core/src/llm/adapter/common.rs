use std::collections::HashSet;

use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::{
    canonical,
    llm::{
        LlmOperationExecutionPin,
        ir::{LlmContentPartIr, LlmRequestIr, OpaqueContinuationRef},
    },
};

use super::{
    AdapterExecutionOptions, AdapterResources, ResolvedArtifactMaterial, ShapeAdapterError,
    ShapeAdapterKey, WireGenerationRequest, resolve_shape_adapter,
};

pub(super) fn prepare(
    pin: &LlmOperationExecutionPin,
    request: &LlmRequestIr,
    options: AdapterExecutionOptions,
    expected: ShapeAdapterKey,
) -> Result<gproxy_protocol::RequestTarget, ShapeAdapterError> {
    crate::llm::ir::validate_request_ir(request)
        .map_err(|error| ShapeAdapterError::new(error.code, error.message))?;
    let descriptor = resolve_shape_adapter(pin)?;
    if descriptor.key != expected || request.model != pin.model_id {
        return Err(ShapeAdapterError::new(
            "adapter_execution_mismatch",
            "request model or adapter does not match execution pin",
        ));
    }
    if options.max_output_tokens == 0 || options.max_output_tokens > 256_000 {
        return Err(ShapeAdapterError::new(
            "invalid_adapter_output_limit",
            "adapter max output tokens are outside the supported bound",
        ));
    }
    Ok(gproxy_protocol::request_target(
        pin.operation_key,
        &pin.model_id,
        options.stream,
    ))
}

pub(super) fn finish_request<T>(
    adapter_key: ShapeAdapterKey,
    pin: &LlmOperationExecutionPin,
    target: gproxy_protocol::RequestTarget,
    value: Value,
) -> Result<WireGenerationRequest, ShapeAdapterError>
where
    T: DeserializeOwned + Serialize,
{
    let typed: T = serde_json::from_value(value).map_err(|_| {
        ShapeAdapterError::new(
            "wire_request_encoding_failed",
            "generated request does not match gproxy-protocol wire type",
        )
    })?;
    let body = canonical::to_vec(&typed).map_err(|_| {
        ShapeAdapterError::new(
            "wire_request_encoding_failed",
            "generated request cannot be canonically serialized",
        )
    })?;
    Ok(WireGenerationRequest::from_parts(
        adapter_key,
        pin.clone(),
        target.method,
        target.path,
        target.query,
        body,
    ))
}

pub(super) fn openai_content(
    parts: &[LlmContentPartIr],
    resources: &AdapterResources,
) -> Result<Value, ShapeAdapterError> {
    let mut result = Vec::with_capacity(parts.len());
    for part in parts {
        result.push(match part {
            LlmContentPartIr::Text { text } => json!({"type":"text","text":text}),
            LlmContentPartIr::Image { artifact_ref } => {
                let data = material_data_url(artifact_ref, resources)?;
                json!({"type":"image_url","image_url":{"url":data}})
            }
            LlmContentPartIr::File { artifact_ref } => {
                let data = material_data_url(artifact_ref, resources)?;
                json!({"type":"file","file":{"file_data":data,"filename":artifact_ref.artifact_id}})
            }
        });
    }
    Ok(Value::Array(result))
}

pub(super) fn text_only(parts: &[LlmContentPartIr]) -> Result<String, ShapeAdapterError> {
    let mut output = String::new();
    for part in parts {
        let LlmContentPartIr::Text { text } = part else {
            return Err(ShapeAdapterError::new(
                "unsupported_wire_content",
                "this wire position accepts text only",
            ));
        };
        output.push_str(text);
    }
    Ok(output)
}

pub(super) fn opaque_value(
    reference: &OpaqueContinuationRef,
    resources: &AdapterResources,
) -> Result<Value, ShapeAdapterError> {
    let key = format!(
        "{}:{}",
        reference.entry_ref.object_id, reference.entry_ref.entry_key
    );
    let bytes = resources.opaque_entries.get(&key).ok_or_else(|| {
        ShapeAdapterError::new(
            "opaque_continuation_unresolved",
            "opaque continuation entry is not available",
        )
    })?;
    serde_json::from_slice(bytes).map_err(|_| {
        ShapeAdapterError::new(
            "opaque_continuation_invalid",
            "opaque continuation entry is not valid adapter JSON",
        )
    })
}

pub(super) fn apply_extension_fields(
    body: &mut Map<String, Value>,
    options: &crate::graph::ProviderExtraIr,
    allowed: &[&str],
) -> Result<(), ShapeAdapterError> {
    if !options.extra_headers.is_empty() {
        return Err(ShapeAdapterError::new(
            "unsupported_extension_header",
            "this adapter version has no extension header allowlist",
        ));
    }
    let allowed: HashSet<_> = allowed.iter().copied().collect();
    for (key, value) in options.options.iter().chain(&options.extra_body) {
        if !allowed.contains(key.as_str()) || body.contains_key(key) {
            return Err(ShapeAdapterError::new(
                "unsupported_provider_extension",
                "provider extension key is unsupported or conflicts with a standard field",
            ));
        }
        body.insert(key.clone(), value.clone());
    }
    Ok(())
}

pub(super) fn parse_typed_terminal<T>(bytes: &[u8]) -> Result<Value, ShapeAdapterError>
where
    T: DeserializeOwned + Serialize,
{
    if bytes.is_empty() || bytes.len() > 16 * 1024 * 1024 {
        return Err(ShapeAdapterError::new(
            "wire_terminal_size_limit",
            "provider terminal is empty or exceeds 16 MiB",
        ));
    }
    let typed: T = serde_json::from_slice(bytes).map_err(|_| {
        ShapeAdapterError::new(
            "wire_terminal_decode_failed",
            "provider terminal does not match gproxy-protocol wire type",
        )
    })?;
    serde_json::to_value(typed).map_err(|_| {
        ShapeAdapterError::new(
            "wire_terminal_decode_failed",
            "provider terminal could not be normalized",
        )
    })
}

fn material_data_url(
    reference: &crate::artifact::ArtifactRef,
    resources: &AdapterResources,
) -> Result<String, ShapeAdapterError> {
    let material = resources
        .materials
        .get(&reference.artifact_id)
        .ok_or_else(|| {
            ShapeAdapterError::new(
                "artifact_material_unresolved",
                "artifact bytes are unavailable to the shape adapter",
            )
        })?;
    validate_material(reference, material)?;
    Ok(format!(
        "data:{};base64,{}",
        reference.media_type,
        STANDARD.encode(&material.bytes)
    ))
}

fn validate_material(
    reference: &crate::artifact::ArtifactRef,
    material: &ResolvedArtifactMaterial,
) -> Result<(), ShapeAdapterError> {
    if &material.artifact_ref != reference || material.bytes.len() as u64 != reference.byte_size {
        return Err(ShapeAdapterError::new(
            "artifact_material_mismatch",
            "resolved artifact metadata does not match its immutable reference",
        ));
    }
    let hash = format!("sha256:{}", hex::encode(Sha256::digest(&material.bytes)));
    if hash != reference.content_hash {
        return Err(ShapeAdapterError::new(
            "artifact_material_hash_mismatch",
            "resolved artifact bytes failed content hash validation",
        ));
    }
    Ok(())
}
