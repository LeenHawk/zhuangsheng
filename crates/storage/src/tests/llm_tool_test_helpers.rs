use zhuangsheng_core::{
    graph::ToolGrant,
    llm::{
        TOOL_CALL_POLICY_VERSION, ToolCallCheckpoint, ToolCallCheckpointStatus,
        ToolCallDigestMaterial, ToolRegistryEntrySnapshot, ToolRegistrySnapshot,
    },
};

pub(super) const DESCRIPTOR_DIGEST: &str = "sha256:echo-descriptor-v1";
pub(super) const SCHEMA_DIGEST: &str = "sha256:echo-input-schema-v1";
pub(super) const IMPLEMENTATION_DIGEST: &str = "sha256:echo-implementation-v1";

pub(super) fn registry() -> ToolRegistrySnapshot {
    ToolRegistrySnapshot {
        revision: "tool-registry-v1".into(),
        entries: vec![ToolRegistryEntrySnapshot {
            tool_id: "echo-tool".into(),
            version: "1".into(),
            descriptor_digest: DESCRIPTOR_DIGEST.into(),
            schema_compilation_digests: vec![SCHEMA_DIGEST.into()],
            implementation_digest: IMPLEMENTATION_DIGEST.into(),
        }],
    }
}

pub(super) fn digest(grant: &ToolGrant, arguments: serde_json::Value) -> String {
    ToolCallDigestMaterial {
        binding_id: "echo-binding".into(),
        tool_id: "echo-tool".into(),
        tool_version: "1".into(),
        arguments,
        grant: grant.clone(),
        descriptor_digest: DESCRIPTOR_DIGEST.into(),
        schema_compilation_digests: vec![SCHEMA_DIGEST.into()],
        implementation_digest: IMPLEMENTATION_DIGEST.into(),
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
