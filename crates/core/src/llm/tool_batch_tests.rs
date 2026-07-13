use std::collections::BTreeMap;

use serde_json::json;

use crate::{
    graph::{
        ArtifactGrant, EffectClassification, LlmNodeExecutionSnapshot, LlmNodeLimits,
        ToolApprovalPolicy, ToolEffectSpec, ToolGrant,
    },
    llm::{
        ChannelCredential, ChannelTransportPolicy, ContentGenerationKind, InitialToolBatchInput,
        InitialToolBatchPlan, LlmChannelRevision, LlmChannelRevisionSpec, LlmLoopCheckpoint,
        LlmOperationExecutionPin, Operation, OperationKey, ResolvedRequestTool,
        ResolvedToolDescriptor, ToolCallCheckpointStatus, ToolDescriptor, ToolLimits,
        ToolRegistryEntrySnapshot, ToolRegistrySnapshot,
        context::{ContextAssemblyMode, ContextAssemblySpec, ContextConfigSnapshot},
        ir::{LlmTurnItemIr, ToolCallIr},
        plan_initial_tool_batch,
    },
    schema::{DIALECT_2020_12, JsonSchemaLimits, JsonSchemaSpec},
};

#[test]
fn planner_rejects_an_unexposed_tool_name() {
    let execution = execution(4);
    let tools = vec![request_tool(false)];
    let items = vec![tool_call("missing", 0)];
    let error = match plan(&execution, &tools, &items, checkpoint(0)) {
        Err(error) => error,
        Ok(_) => panic!("unknown tool must fail"),
    };
    assert_eq!(error.code, "tool_binding_unknown");
}

#[test]
fn planner_enforces_the_activation_tool_call_limit() {
    let execution = execution(1);
    let tools = vec![request_tool(false)];
    let items = vec![tool_call("echo_alias", 0)];
    let error = match plan(&execution, &tools, &items, checkpoint(1)) {
        Err(error) => error,
        Ok(_) => panic!("tool limit must fail"),
    };
    assert_eq!(error.code, "tool_call_limit_exceeded");
}

#[test]
fn planner_builds_one_ordered_approval_batch() {
    let execution = execution(4);
    let tools = vec![request_tool(true)];
    let items = vec![tool_call("echo_alias", 0), tool_call("echo_alias", 1)];
    let InitialToolBatchPlan::Approval(command) =
        plan(&execution, &tools, &items, checkpoint(0)).unwrap()
    else {
        panic!("expected approval batch")
    };
    assert_eq!(command.wait_id, "wait_tool_approval_model-call");
    assert_eq!(command.calls.len(), 2);
    assert_eq!(command.calls[0].call_index, 0);
    assert_eq!(command.calls[1].call_index, 1);
    assert_eq!(
        command.calls[0].provider_call_id.as_deref(),
        Some("provider-0")
    );
    assert_eq!(command.checkpoint.tool_calls_used, 2);
    assert_eq!(command.checkpoint.wait_ids, vec![command.wait_id.clone()]);
    assert!(command.checkpoint.checksum_is_valid());
    assert!(command.checkpoint.current_batch.iter().all(|call| {
        call.status == ToolCallCheckpointStatus::AwaitingApproval
            && call.wait_id.as_deref() == Some(command.wait_id.as_str())
    }));
}

#[test]
fn planner_leaves_a_preapproved_batch_ready_for_execution() {
    let execution = execution(4);
    let tools = vec![request_tool(false)];
    let items = vec![tool_call("echo_alias", 0)];
    let InitialToolBatchPlan::Executable(batch) =
        plan(&execution, &tools, &items, checkpoint(0)).unwrap()
    else {
        panic!("expected executable batch")
    };
    assert_eq!(batch.calls.len(), 1);
    assert_eq!(batch.checkpoint.tool_calls_used, 0);
    assert_eq!(
        batch.checkpoint.current_batch[0].status,
        ToolCallCheckpointStatus::Requested
    );
    assert!(batch.checkpoint.checksum_is_valid());
}

fn plan(
    execution: &LlmNodeExecutionSnapshot,
    tools: &[ResolvedRequestTool],
    items: &[LlmTurnItemIr],
    checkpoint: LlmLoopCheckpoint,
) -> Result<InitialToolBatchPlan, super::ToolBatchPlanError> {
    plan_initial_tool_batch(InitialToolBatchInput {
        execution,
        request_tools: tools,
        response_items: items,
        model_call_id: "model-call",
        node_instance_id: "node-instance",
        originating_attempt_id: "attempt",
        checkpoint,
        now_ms: 1_000,
    })
}

fn tool_call(name: &str, index: u64) -> LlmTurnItemIr {
    LlmTurnItemIr::AssistantToolCall {
        id: format!("item-{index}"),
        call: ToolCallIr {
            id: format!("call-{index}"),
            provider_call_id: Some(format!("provider-{index}")),
            name: name.into(),
            arguments: json!({"text":format!("value-{index}")}),
        },
    }
}

fn checkpoint(tool_calls_used: u64) -> LlmLoopCheckpoint {
    LlmLoopCheckpoint {
        schema_version: 1,
        node_instance_id: "node-instance".into(),
        last_updated_by_attempt_id: "attempt".into(),
        graph_revision_id: "graph-revision".into(),
        registry_snapshot: registry_snapshot(),
        context_snapshot_ref: "context-snapshot".into(),
        read_set_digest: "sha256:read-set".into(),
        model_call_no: 1,
        transcript_ref: "transcript".into(),
        continuation_ref: None,
        active_model_effect: None,
        active_count_effect: None,
        current_batch: Vec::new(),
        model_calls_used: 1,
        count_calls_used: 0,
        tool_calls_used,
        effect_watermark: "model-call".into(),
        wait_ids: Vec::new(),
        checksum: String::new(),
    }
}

fn request_tool(requires_approval: bool) -> ResolvedRequestTool {
    let descriptor = ToolDescriptor {
        tool_id: "echo-tool".into(),
        version: "1".into(),
        name: "echo".into(),
        description: Some("Echo text".into()),
        input_schema: JsonSchemaSpec {
            schema_version: 1,
            dialect: DIALECT_2020_12.into(),
            validation_profile_version: 1,
            format_policy_version: 1,
            document: json!({
                "type":"object",
                "required":["text"],
                "additionalProperties":false,
                "properties":{"text":{"type":"string"}}
            }),
            limits: JsonSchemaLimits::default(),
        },
        binding_config_schema: None,
        effect: ToolEffectSpec {
            classification: EffectClassification::Pure,
            operation_key: "tool.echo".into(),
            requires_approval: false,
        },
        supports_parallel: true,
        required_scopes: Vec::new(),
        limits: ToolLimits {
            timeout_ms: 1_000,
            max_input_bytes: 1_024,
            max_llm_result_bytes: 1_024,
            max_artifact_bytes: 1_024,
        },
    };
    let compiled = crate::schema::compile(&descriptor.input_schema).unwrap();
    let resolved = ResolvedToolDescriptor {
        descriptor_digest: descriptor.digest().unwrap(),
        descriptor,
        schema_compilation_digests: vec![compiled.compiled_payload_hash],
        implementation_digest: "sha256:echo-implementation".into(),
        executor_key: "builtin.echo".into(),
    };
    ResolvedRequestTool {
        binding_id: "echo-binding".into(),
        exposed_name: "echo_alias".into(),
        grant: ToolGrant {
            binding_id: "echo-binding".into(),
            tool_id: "echo-tool".into(),
            version: "1".into(),
            exposed_name: Some("echo_alias".into()),
            scopes: Vec::new(),
            artifact: ArtifactGrant {
                read_scopes: Vec::new(),
                write_scopes: Vec::new(),
                allowed_media_types: Vec::new(),
                max_objects: 1,
                max_bytes: 1_024,
            },
            constraints: BTreeMap::new(),
            approval: Some(if requires_approval {
                ToolApprovalPolicy::Always
            } else {
                ToolApprovalPolicy::DescriptorDefault
            }),
            failure_policy: None,
        },
        descriptor: resolved,
        requires_approval,
    }
}

fn execution(max_tool_calls: u64) -> LlmNodeExecutionSnapshot {
    let operation_key = OperationKey::content_generation(
        Operation::GenerateContent,
        ContentGenerationKind::OpenAiResponses,
    );
    let context_spec = ContextAssemblySpec {
        id: None,
        name: None,
        mode: ContextAssemblyMode::Chat,
        items: Vec::new(),
        budget: None,
        post_process: Vec::new(),
        text_transforms: Vec::new(),
        text_transform_macros: Default::default(),
        preview: None,
    };
    LlmNodeExecutionSnapshot {
        schema_version: 1,
        graph_revision_id: "graph-revision".into(),
        graph_content_hash: "sha256:graph".into(),
        node_id: "generate".into(),
        operation: LlmOperationExecutionPin {
            channel_revision_id: "channel-revision".into(),
            model_id: "model".into(),
            operation_key,
            operation_taxonomy_version: 1,
            adapter_decoder_version: 1,
        },
        channel: LlmChannelRevision {
            id: "channel-revision".into(),
            channel_id: "channel".into(),
            revision_no: 1,
            spec: LlmChannelRevisionSpec {
                operation_taxonomy_version: 1,
                adapter_decoder_version: 1,
                base_url: "https://example.test/v1".into(),
                transport_policy: ChannelTransportPolicy {
                    allow_loopback_http: false,
                    allow_unauthenticated: true,
                },
                credential: ChannelCredential::None,
                operation_keys: vec![operation_key],
                model_catalogs: Vec::new(),
                capabilities: Vec::new(),
            },
            content_hash: "sha256:channel".into(),
            created_at: 1,
        },
        context: ContextConfigSnapshot::GraphInline {
            graph_revision_id: "graph-revision".into(),
            node_id: "generate".into(),
            content_hash: "sha256:context".into(),
            semantic_policy_version: 1,
            spec: context_spec,
        },
        capability_overrides: Vec::new(),
        memory: None,
        tools: Vec::new(),
        tool_registry: registry_snapshot(),
        tool_descriptors: Vec::new(),
        hosted_tools: Vec::new(),
        request: None,
        output: None,
        streaming: None,
        limits: LlmNodeLimits {
            max_model_calls: Some(4),
            max_count_calls: Some(1),
            max_tool_calls: Some(max_tool_calls),
            max_output_repairs: Some(1),
            max_concurrent_tools: Some(2),
            max_input_tokens: Some(4_096),
            max_output_tokens: Some(1_024),
        },
    }
}

fn registry_snapshot() -> ToolRegistrySnapshot {
    ToolRegistrySnapshot {
        revision: "registry-v1".into(),
        entries: vec![ToolRegistryEntrySnapshot {
            tool_id: "echo-tool".into(),
            version: "1".into(),
            descriptor_digest: "sha256:descriptor".into(),
            schema_compilation_digests: vec!["sha256:schema".into()],
            implementation_digest: "sha256:echo-implementation".into(),
        }],
    }
}
