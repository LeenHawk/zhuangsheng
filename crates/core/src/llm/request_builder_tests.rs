use std::collections::{BTreeMap, BTreeSet};

use serde_json::json;

use crate::{
    canonical,
    graph::{
        ArtifactGrant, EffectClassification, GenerationOptionsIr, HostedToolBinding,
        LlmNodeExecutionSnapshot, LlmNodeLimits, LlmOutputSpec, LlmRequestOptions,
        ToolApprovalPolicy, ToolEffectSpec, ToolGrant, ToolScopeGrant, ToolScopeKind,
    },
    llm::{
        ChannelCredential, ChannelModel, ChannelModelCatalog, ChannelTransportPolicy,
        ContentGenerationKind, LlmChannelRevision, LlmChannelRevisionSpec,
        LlmOperationExecutionPin, ModelCapabilities, ModelCatalogPolicy, Operation, OperationKey,
        ResolvedToolDescriptor, ToolDescriptor, ToolLimits, ToolRegistryEntrySnapshot,
        ToolRegistrySnapshot, ToolScopeRequirement,
        context::{
            AssembledMessageIr, ContextAssemblyMode, ContextAssemblyOutput,
            ContextAssemblySnapshot, ContextAssemblySnapshotConfig, ContextAssemblySpec,
            ContextBudgetReport, ContextConfigSnapshot, ContextCountSource,
        },
        ir::{
            ContextProvenanceIr, ContextSensitivity, ContextTrust, LlmTurnItemIr, MessageRole,
            ProvenanceRole, ResponseFormatIr,
        },
    },
    schema::{DIALECT_2020_12, JsonSchemaLimits, JsonSchemaSpec},
};

use super::{LlmRequestBuildInput, build_llm_request};

#[test]
fn builder_binds_context_model_tools_and_output_contract() {
    let fixture = fixture_with_tool(false);
    let approved = BTreeSet::new();
    let first = build(&fixture, &approved).unwrap();
    let second = build(&fixture, &approved).unwrap();
    assert_eq!(first.request_digest, second.request_digest);
    assert_eq!(first.request.model, "roleplay-model");
    assert_eq!(first.request.tools[0].name, "echo_alias");
    assert!(matches!(
        first.request.response_format,
        Some(ResponseFormatIr::Json {
            strict: true,
            schema: Some(_)
        })
    ));
    assert_eq!(first.request.transcript.len(), 1);
    assert_eq!(first.resolved_tools.len(), 1);
    assert!(!first.resolved_tools[0].requires_approval);
    assert_eq!(
        first.request.metadata.get("contextAssemblyDigest"),
        Some(&crate::llm::ir::MetadataValue::String(
            "sha256:assembly".into()
        ))
    );
}

#[test]
fn descriptor_pin_and_required_scope_are_authoritative() {
    let mut fixture = fixture_with_tool(false);
    fixture.registry.entries[0].descriptor_digest = "sha256:wrong".into();
    assert_eq!(
        build(&fixture, &BTreeSet::new()).unwrap_err().code,
        "tool_descriptor_pin_mismatch"
    );

    let mut fixture = fixture_with_tool(false);
    fixture.descriptors[0]
        .descriptor
        .required_scopes
        .push(ToolScopeRequirement {
            kind: ToolScopeKind::MemoryRead,
            scope: "memory:character".into(),
        });
    repin(&mut fixture);
    assert_eq!(
        build(&fixture, &BTreeSet::new()).unwrap_err().code,
        "tool_required_scope_missing"
    );
    fixture.execution.tools[0].scopes.push(ToolScopeGrant {
        kind: ToolScopeKind::MemoryRead,
        scope: "memory:character".into(),
        paths: Vec::new(),
        origins: Vec::new(),
    });
    build(&fixture, &BTreeSet::new()).unwrap();
}

#[test]
fn descriptor_approval_cannot_be_disabled_by_the_grant() {
    let fixture = fixture_with_tool(true);
    let output = build(&fixture, &BTreeSet::new()).unwrap();
    assert!(output.resolved_tools[0].requires_approval);
}

#[test]
fn hosted_tool_requires_explicit_preapproval_and_safe_scalar_config() {
    let mut fixture = fixture();
    fixture.execution.hosted_tools.push(HostedToolBinding {
        binding_id: "web-search".into(),
        operation_key: operation(),
        hosted_kind: "web_search".into(),
        model_facing_config: BTreeMap::from([("mode".into(), json!("fast"))]),
        resource_scopes: vec!["https://example.test".into()],
        effect: ToolEffectSpec {
            classification: EffectClassification::Pure,
            operation_key: "hosted.web_search".into(),
            requires_approval: true,
        },
        max_uses_per_model_call: 1,
    });
    assert_eq!(
        build(&fixture, &BTreeSet::new()).unwrap_err().code,
        "hosted_tool_approval_required"
    );
    let approved = BTreeSet::from(["web-search".into()]);
    assert_eq!(
        build(&fixture, &approved)
            .unwrap()
            .request
            .hosted_tools
            .len(),
        1
    );

    fixture.execution.hosted_tools[0]
        .model_facing_config
        .insert("api_token".into(), json!("nope"));
    assert_eq!(
        build(&fixture, &approved).unwrap_err().code,
        "hosted_tool_config_unsafe"
    );
}

#[test]
fn strict_alternation_placeholder_remains_explicit_and_empty() {
    let mut fixture = fixture();
    fixture.context.messages[0].role = MessageRole::Assistant;
    fixture.context.messages[0].content.clear();
    fixture.context.messages[0].placeholder = true;
    fixture.context.provenance[0].final_role = ProvenanceRole::Assistant;
    let output = build(&fixture, &BTreeSet::new()).unwrap();
    let LlmTurnItemIr::Message {
        placeholder,
        content,
        ..
    } = &output.request.transcript[0]
    else {
        panic!("expected message")
    };
    assert!(*placeholder);
    assert!(content.is_empty());
}

#[test]
fn request_builder_rejects_context_mismatch_and_impossible_tool_choice() {
    let mut mismatched = fixture();
    mismatched.context.snapshot.config = ContextAssemblySnapshotConfig::GraphInline {
        graph_revision_id: "other".into(),
        node_id: "generate".into(),
        content_hash: "sha256:context".into(),
    };
    assert_eq!(
        build(&mismatched, &BTreeSet::new()).unwrap_err().code,
        "context_execution_snapshot_mismatch"
    );

    let mut fixture = fixture();
    fixture.execution.request.as_mut().unwrap().tool_choice =
        Some(crate::graph::ToolChoiceIr::Required);
    assert_eq!(
        build(&fixture, &BTreeSet::new()).unwrap_err().code,
        "required_tool_unavailable"
    );
}

struct Fixture {
    execution: LlmNodeExecutionSnapshot,
    context: ContextAssemblyOutput,
    registry: ToolRegistrySnapshot,
    descriptors: Vec<ResolvedToolDescriptor>,
}

fn build(
    fixture: &Fixture,
    approved: &BTreeSet<String>,
) -> Result<super::LlmRequestBuildOutput, super::LlmRequestBuildError> {
    build_llm_request(LlmRequestBuildInput {
        execution: &fixture.execution,
        context: &fixture.context,
        registry_snapshot: &fixture.registry,
        tool_descriptors: &fixture.descriptors,
        transcript_tail: &[],
        continuation: None,
        approved_hosted_bindings: approved,
        model_call_no: 1,
    })
}

fn fixture_with_tool(requires_approval: bool) -> Fixture {
    let mut fixture = fixture();
    fixture.execution.tools.push(ToolGrant {
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
            max_bytes: 1024,
        },
        constraints: BTreeMap::new(),
        approval: Some(ToolApprovalPolicy::DescriptorDefault),
        failure_policy: None,
    });
    let descriptor = ToolDescriptor {
        tool_id: "echo-tool".into(),
        version: "1".into(),
        name: "echo".into(),
        description: Some("Echo a value".into()),
        input_schema: object_schema(),
        binding_config_schema: None,
        effect: ToolEffectSpec {
            classification: EffectClassification::Pure,
            operation_key: "tool.echo".into(),
            requires_approval,
        },
        supports_parallel: true,
        required_scopes: Vec::new(),
        limits: ToolLimits {
            timeout_ms: 1_000,
            max_input_bytes: 1024,
            max_llm_result_bytes: 1024,
            max_artifact_bytes: 1024,
        },
    };
    let compilation = crate::schema::compile(&descriptor.input_schema).unwrap();
    let resolved = ResolvedToolDescriptor {
        descriptor_digest: descriptor.digest().unwrap(),
        descriptor,
        schema_compilation_digests: vec![compilation.compiled_payload_hash],
        implementation_digest: "sha256:implementation".into(),
    };
    fixture.descriptors.push(resolved);
    repin(&mut fixture);
    fixture
}

fn repin(fixture: &mut Fixture) {
    let resolved = &mut fixture.descriptors[0];
    resolved.descriptor_digest = resolved.descriptor.digest().unwrap();
    fixture.registry.entries = vec![ToolRegistryEntrySnapshot {
        tool_id: resolved.descriptor.tool_id.clone(),
        version: resolved.descriptor.version.clone(),
        descriptor_digest: resolved.descriptor_digest.clone(),
        schema_compilation_digests: resolved.schema_compilation_digests.clone(),
        implementation_digest: resolved.implementation_digest.clone(),
    }];
}

fn fixture() -> Fixture {
    let spec = ContextAssemblySpec {
        id: None,
        name: None,
        mode: ContextAssemblyMode::Chat,
        items: Vec::new(),
        budget: None,
        post_process: Vec::new(),
        preview: None,
    };
    let config = ContextConfigSnapshot::GraphInline {
        graph_revision_id: "graph-revision".into(),
        node_id: "generate".into(),
        content_hash: "sha256:context".into(),
        semantic_policy_version: 1,
        spec,
    };
    let channel_spec = channel_spec();
    let channel_hash = canonical::hash(&channel_spec).unwrap();
    Fixture {
        execution: LlmNodeExecutionSnapshot {
            schema_version: 1,
            graph_revision_id: "graph-revision".into(),
            graph_content_hash: "sha256:graph".into(),
            node_id: "generate".into(),
            operation: LlmOperationExecutionPin {
                channel_revision_id: "channel-revision".into(),
                model_id: "roleplay-model".into(),
                operation_key: operation(),
                operation_taxonomy_version: 1,
                adapter_decoder_version: 1,
            },
            channel: LlmChannelRevision {
                id: "channel-revision".into(),
                channel_id: "channel".into(),
                revision_no: 1,
                spec: channel_spec,
                content_hash: channel_hash,
                created_at: 1,
            },
            context: config,
            capability_overrides: Vec::new(),
            memory: None,
            tools: Vec::new(),
            hosted_tools: Vec::new(),
            request: Some(LlmRequestOptions {
                generation: Some(GenerationOptionsIr {
                    temperature: Some(0.7),
                    top_p: None,
                    max_output_tokens: Some(512),
                    stop: Vec::new(),
                    seed: Some(7),
                }),
                extensions: None,
                tool_choice: None,
            }),
            output: Some(LlmOutputSpec::Json {
                schema: object_schema(),
                strict: true,
            }),
            streaming: None,
            limits: LlmNodeLimits {
                max_model_calls: Some(8),
                max_count_calls: Some(2),
                max_tool_calls: Some(32),
                max_output_repairs: Some(1),
                max_concurrent_tools: Some(4),
                max_input_tokens: Some(16_384),
                max_output_tokens: Some(2_048),
            },
        },
        context: context_output(),
        registry: ToolRegistrySnapshot {
            revision: "registry-v1".into(),
            entries: Vec::new(),
        },
        descriptors: Vec::new(),
    }
}

fn context_output() -> ContextAssemblyOutput {
    let provenance = ContextProvenanceIr {
        id: "provenance:user".into(),
        item_id: "user".into(),
        source_type: "input".into(),
        source_id: "/message".into(),
        trust: ContextTrust::UserInput,
        sensitivity: ContextSensitivity::Private,
        final_role: ProvenanceRole::User,
        transformations: Vec::new(),
    };
    ContextAssemblyOutput {
        instructions: Vec::new(),
        messages: vec![AssembledMessageIr {
            id: "message:user".into(),
            role: MessageRole::User,
            content: vec![crate::llm::ir::LlmContentPartIr::Text {
                text: "hello".into(),
            }],
            provenance_id: provenance.id.clone(),
            placeholder: false,
        }],
        provenance: vec![provenance],
        budget_report: ContextBudgetReport {
            available_input_tokens: 16_384,
            fixed_request_tokens: 0,
            assembled_tokens: 5,
            count_source: ContextCountSource::Local,
            items: Vec::new(),
        },
        snapshot: ContextAssemblySnapshot {
            config: ContextAssemblySnapshotConfig::GraphInline {
                graph_revision_id: "graph-revision".into(),
                node_id: "generate".into(),
                content_hash: "sha256:context".into(),
            },
            read_set_ref: "readset".into(),
            read_set_digest: "sha256:readset".into(),
            resolved_bindings_digest: "sha256:bindings".into(),
            assembly_digest: "sha256:assembly".into(),
        },
    }
}

fn channel_spec() -> LlmChannelRevisionSpec {
    LlmChannelRevisionSpec {
        operation_taxonomy_version: 1,
        adapter_decoder_version: 1,
        base_url: "https://example.test/v1".into(),
        transport_policy: ChannelTransportPolicy {
            allow_loopback_http: false,
            allow_unauthenticated: true,
        },
        credential: ChannelCredential::None,
        operation_keys: vec![operation()],
        model_catalogs: vec![ChannelModelCatalog {
            operation_key: operation(),
            policy: ModelCatalogPolicy::Allowlist,
            models: vec![ChannelModel {
                id: "roleplay-model".into(),
                name: None,
                context_window: Some(16_384),
                max_output_tokens: Some(2_048),
                capabilities: ModelCapabilities {
                    streaming: Some(true),
                    tool_calling: Some(true),
                    structured_output: Some(true),
                    vision_input: Some(false),
                },
            }],
        }],
        capabilities: Vec::new(),
    }
}

fn object_schema() -> JsonSchemaSpec {
    JsonSchemaSpec {
        schema_version: 1,
        dialect: DIALECT_2020_12.into(),
        validation_profile_version: 1,
        format_policy_version: 1,
        document: json!({"type":"object","additionalProperties":false}),
        limits: JsonSchemaLimits::default(),
    }
}

fn operation() -> OperationKey {
    OperationKey::content_generation(
        Operation::GenerateContent,
        ContentGenerationKind::OpenAiResponses,
    )
}
