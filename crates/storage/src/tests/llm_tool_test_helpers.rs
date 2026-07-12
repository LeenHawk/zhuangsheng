use zhuangsheng_core::{
    graph::{EffectClassification, ToolEffectSpec, ToolGrant},
    llm::{
        ResolvedToolDescriptor, TOOL_CALL_POLICY_VERSION, ToolCallCheckpoint,
        ToolCallCheckpointStatus, ToolCallDigestMaterial, ToolDescriptor, ToolLimits,
        ToolRegistrySnapshot, build_tool_registry_snapshot,
    },
    schema::{DIALECT_2020_12, JsonSchemaLimits, JsonSchemaSpec},
};

pub(super) const IMPLEMENTATION_DIGEST: &str = "sha256:echo-implementation-v1";
pub(super) const EXECUTOR_KEY: &str = "test.echo";

pub(super) fn descriptor() -> ToolDescriptor {
    descriptor_with_classification(EffectClassification::Pure)
}

pub(super) fn descriptor_with_classification(
    classification: EffectClassification,
) -> ToolDescriptor {
    ToolDescriptor {
        tool_id: "echo-tool".into(),
        version: "1".into(),
        name: "echo".into(),
        description: Some("Echo a value".into()),
        input_schema: JsonSchemaSpec {
            schema_version: 1,
            dialect: DIALECT_2020_12.into(),
            validation_profile_version: 1,
            format_policy_version: 1,
            document: serde_json::json!({
                "type":"object",
                "required":["text"],
                "additionalProperties":false,
                "properties":{"text":{"type":"string"}}
            }),
            limits: JsonSchemaLimits::default(),
        },
        binding_config_schema: None,
        effect: ToolEffectSpec {
            classification,
            operation_key: "tool.echo".into(),
            requires_approval: false,
        },
        supports_parallel: true,
        required_scopes: Vec::new(),
        limits: ToolLimits {
            timeout_ms: 1_000,
            max_input_bytes: 1024,
            max_llm_result_bytes: 1024,
            max_artifact_bytes: 1024,
        },
    }
}

pub(super) fn resolved() -> ResolvedToolDescriptor {
    resolved_with_classification(EffectClassification::Pure)
}

pub(super) fn resolved_with_classification(
    classification: EffectClassification,
) -> ResolvedToolDescriptor {
    let descriptor = descriptor_with_classification(classification);
    let compilations = zhuangsheng_core::llm::compile_tool_descriptor(&descriptor).unwrap();
    ResolvedToolDescriptor {
        descriptor_digest: descriptor.digest().unwrap(),
        descriptor,
        schema_compilation_digests: compilations
            .into_iter()
            .map(|item| item.compiled_payload_hash)
            .collect(),
        implementation_digest: IMPLEMENTATION_DIGEST.into(),
        executor_key: EXECUTOR_KEY.into(),
    }
}

pub(super) fn registry() -> ToolRegistrySnapshot {
    build_tool_registry_snapshot(&[resolved()]).unwrap()
}

pub(super) fn digest(grant: &ToolGrant, arguments: serde_json::Value) -> String {
    let resolved = resolved();
    digest_with_resolved(grant, arguments, &resolved)
}

pub(super) fn digest_with_resolved(
    grant: &ToolGrant,
    arguments: serde_json::Value,
    resolved: &ResolvedToolDescriptor,
) -> String {
    ToolCallDigestMaterial {
        binding_id: grant.binding_id.clone(),
        tool_id: "echo-tool".into(),
        tool_version: "1".into(),
        arguments,
        grant: grant.clone(),
        descriptor_digest: resolved.descriptor_digest.clone(),
        schema_compilation_digests: resolved.schema_compilation_digests.clone(),
        implementation_digest: resolved.implementation_digest.clone(),
        policy_version: TOOL_CALL_POLICY_VERSION,
    }
    .digest()
    .unwrap()
}

pub(super) fn tool_checkpoint_call(
    tool_call_id: &str,
    effect_id: &str,
    call_index: u64,
    call_digest: &str,
    status: ToolCallCheckpointStatus,
    output_ref: Option<String>,
) -> ToolCallCheckpoint {
    ToolCallCheckpoint {
        tool_call_id: tool_call_id.into(),
        call_index,
        call_digest: call_digest.into(),
        status,
        effect_id: Some(effect_id.into()),
        output_ref,
        wait_id: None,
    }
}
