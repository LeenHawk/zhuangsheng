use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use serde_json::json;
use zhuangsheng_core::{
    application::{secret::SecretValue, tool::PublishToolCommand},
    graph::{ArtifactGrant, EffectClassification, ToolApprovalPolicy, ToolEffectSpec, ToolGrant},
    llm::{LlmChannelRevision, ToolDescriptor, ToolLimits, adapter::WireGenerationRequest},
    runtime::WaitKind,
    runtime::{
        RunContextCommand, RunStatus, StartRunCommand, SubmitWaitResponseCommand,
        ToolApprovalDecision, ToolApprovalDecisionKind, WaitResponsePayload,
    },
    scheduler::Scheduler,
    schema::{DIALECT_2020_12, JsonSchemaLimits, JsonSchemaSpec},
};
use zhuangsheng_storage::SqliteStore;

use crate::{
    llm_executor::LocalLlmExecutor,
    provider::{ProviderHttpError, ProviderHttpResponse, ProviderTransport},
    tool_executor::BUILTIN_ECHO_IMPLEMENTATION_DIGEST,
};

use super::{create_llm_graph, now_ms, provider_response};

struct ToolAwareProvider {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ProviderTransport for ToolAwareProvider {
    async fn send(
        &self,
        _channel: &LlmChannelRevision,
        wire: &WireGenerationRequest,
        credential: Option<&SecretValue>,
    ) -> Result<ProviderHttpResponse, ProviderHttpError> {
        assert!(credential.is_none());
        let body: serde_json::Value = serde_json::from_slice(wire.body()).unwrap();
        assert_eq!(body["tools"][0]["name"], "echo_alias");
        assert_eq!(body["tools"][0]["parameters"]["required"][0], "text");
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(provider_response("工具已就绪。"))
    }
}

#[tokio::test]
async fn executor_builds_model_request_from_persisted_registry_snapshot() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    store
        .publish_tool(PublishToolCommand {
            descriptor: descriptor(),
            implementation_digest: "sha256:server-echo-implementation".into(),
            executor_key: "builtin.echo".into(),
            enabled: true,
            idempotency_key: "server-publish-echo".into(),
        })
        .await
        .unwrap();
    let revision_id = create_llm_graph(&store, false, None, Some(grant())).await;
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision_id,
            input: json!({"message":"hello"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "server-tool-registry-run".into(),
        })
        .await
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let executor = Arc::new(LocalLlmExecutor::with_provider(
        store.clone(),
        Arc::new(ToolAwareProvider {
            calls: calls.clone(),
        }),
    ));
    Scheduler::new(store.clone(), "tool-registry-worker")
        .with_llm_executor(executor)
        .run_until_idle(now_ms(), 128)
        .await
        .unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Completed
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

struct ApprovalToolProvider;

#[async_trait]
impl ProviderTransport for ApprovalToolProvider {
    async fn send(
        &self,
        _channel: &LlmChannelRevision,
        _wire: &WireGenerationRequest,
        _credential: Option<&SecretValue>,
    ) -> Result<ProviderHttpResponse, ProviderHttpError> {
        Ok(ProviderHttpResponse {
            status: 200,
            provider_request_id: Some("approval-tool-request".into()),
            body: serde_json::to_vec(&json!({
                "id":"response-tool-1",
                "created_at":1,
                "object":"response",
                "output":[{
                    "type":"function_call",
                    "id":"function-call-1",
                    "call_id":"provider-call-1",
                    "name":"echo_alias",
                    "arguments":"{\"text\":\"hello\"}",
                    "status":"completed"
                }],
                "status":"completed",
                "usage":{
                    "input_tokens":10,
                    "output_tokens":5,
                    "total_tokens":15,
                    "output_tokens_details":{"reasoning_tokens":0}
                }
            }))
            .unwrap(),
        })
    }
}

#[tokio::test]
async fn provider_tool_call_opens_one_durable_approval_batch() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    store
        .publish_tool(PublishToolCommand {
            descriptor: descriptor(),
            implementation_digest: "sha256:server-echo-implementation".into(),
            executor_key: "builtin.echo".into(),
            enabled: true,
            idempotency_key: "server-publish-approval-echo".into(),
        })
        .await
        .unwrap();
    let mut approved_grant = grant();
    approved_grant.approval = Some(ToolApprovalPolicy::Always);
    let revision_id = create_llm_graph(&store, false, None, Some(approved_grant)).await;
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision_id,
            input: json!({"message":"use echo"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "server-tool-approval-run".into(),
        })
        .await
        .unwrap();
    let executor = Arc::new(LocalLlmExecutor::with_provider(
        store.clone(),
        Arc::new(ApprovalToolProvider),
    ));
    Scheduler::new(store.clone(), "tool-approval-worker")
        .with_llm_executor(executor)
        .run_until_idle(now_ms(), 128)
        .await
        .unwrap();
    let status = store.get_run(&run.id).await.unwrap().status;
    let events = store.list_run_events(&run.id, 0, 500).await.unwrap();
    assert_eq!(status, RunStatus::Waiting, "events: {events:?}");
    let waits = store.list_open_waits(&run.id).await.unwrap();
    assert_eq!(waits.len(), 1);
    assert_eq!(waits[0].kind, WaitKind::Approval);
    assert_eq!(waits[0].blockers.len(), 1);
    assert_eq!(
        waits[0].request["calls"][0]["toolCallId"],
        waits[0].blockers[0].id
    );
    assert!(waits[0].request["calls"][0].get("arguments").is_none());
}

pub(super) struct EchoLoopProvider {
    pub(super) calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ProviderTransport for EchoLoopProvider {
    async fn send(
        &self,
        _channel: &LlmChannelRevision,
        wire: &WireGenerationRequest,
        _credential: Option<&SecretValue>,
    ) -> Result<ProviderHttpResponse, ProviderHttpError> {
        let call_no = self.calls.fetch_add(1, Ordering::SeqCst);
        if call_no == 0 {
            return Ok(tool_call_response());
        }
        let body: serde_json::Value = serde_json::from_slice(wire.body()).unwrap();
        let output = body["input"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["type"] == "function_call_output")
            .expect("second request must contain the durable tool result");
        assert_eq!(output["call_id"], "provider-call-1");
        assert_eq!(output["output"], "hello");
        Ok(provider_response("Echo 完成。"))
    }
}

#[tokio::test]
async fn custom_tool_result_is_persisted_before_the_next_model_call() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    store
        .publish_tool(PublishToolCommand {
            descriptor: descriptor(),
            implementation_digest: BUILTIN_ECHO_IMPLEMENTATION_DIGEST.into(),
            executor_key: "builtin.echo".into(),
            enabled: true,
            idempotency_key: "server-publish-executable-echo".into(),
        })
        .await
        .unwrap();
    let revision_id = create_llm_graph(&store, false, None, Some(grant())).await;
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision_id,
            input: json!({"message":"echo hello"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "server-tool-loop-run".into(),
        })
        .await
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let executor = Arc::new(LocalLlmExecutor::with_provider(
        store.clone(),
        Arc::new(EchoLoopProvider {
            calls: calls.clone(),
        }),
    ));
    Scheduler::new(store.clone(), "tool-loop-worker")
        .with_llm_executor(executor)
        .run_until_idle(now_ms(), 128)
        .await
        .unwrap();
    let status = store.get_run(&run.id).await.unwrap().status;
    let events = store.list_run_events(&run.id, 0, 500).await.unwrap();
    assert_eq!(status, RunStatus::Completed, "events: {events:?}");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(
        events
            .iter()
            .any(|event| event.event_type == "tool.call.completed")
    );
}

#[tokio::test]
async fn approved_tool_batch_resumes_without_repeating_the_first_model_call() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    store
        .publish_tool(PublishToolCommand {
            descriptor: descriptor(),
            implementation_digest: BUILTIN_ECHO_IMPLEMENTATION_DIGEST.into(),
            executor_key: "builtin.echo".into(),
            enabled: true,
            idempotency_key: "server-publish-resumable-echo".into(),
        })
        .await
        .unwrap();
    let mut approved_grant = grant();
    approved_grant.approval = Some(ToolApprovalPolicy::Always);
    let revision_id = create_llm_graph(&store, false, None, Some(approved_grant)).await;
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision_id,
            input: json!({"message":"approve echo"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "server-tool-resume-run".into(),
        })
        .await
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let executor = Arc::new(LocalLlmExecutor::with_provider(
        store.clone(),
        Arc::new(EchoLoopProvider {
            calls: calls.clone(),
        }),
    ));
    Scheduler::new(store.clone(), "tool-resume-worker")
        .with_llm_executor(executor.clone())
        .run_until_idle(now_ms(), 128)
        .await
        .unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Waiting
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let wait = store.list_open_waits(&run.id).await.unwrap().remove(0);
    let tool_call_id = wait.request["calls"][0]["toolCallId"]
        .as_str()
        .unwrap()
        .to_owned();
    let call_digest = wait.request["calls"][0]["callDigest"]
        .as_str()
        .unwrap()
        .to_owned();
    store
        .submit_wait_response(
            SubmitWaitResponseCommand {
                wait_id: wait.id,
                delivery_id: "server-tool-resume-approval".into(),
                actor_kind: "human".into(),
                actor_id: Some("test-user".into()),
                payload: WaitResponsePayload::ToolApproval {
                    decisions: vec![ToolApprovalDecision {
                        tool_call_id,
                        call_digest,
                        decision: ToolApprovalDecisionKind::Approve,
                        reason: Some("approved in test".into()),
                    }],
                },
            },
            now_ms() + 1,
        )
        .await
        .unwrap();
    Scheduler::new(store.clone(), "tool-resume-worker")
        .with_llm_executor(executor)
        .run_until_idle(now_ms() + 2, 128)
        .await
        .unwrap();
    let status = store.get_run(&run.id).await.unwrap().status;
    let events = store.list_run_events(&run.id, 0, 500).await.unwrap();
    assert_eq!(status, RunStatus::Completed, "events: {events:?}");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

pub(super) fn tool_call_response() -> ProviderHttpResponse {
    ProviderHttpResponse {
        status: 200,
        provider_request_id: Some("tool-loop-request".into()),
        body: serde_json::to_vec(&json!({
            "id":"response-tool-loop",
            "created_at":1,
            "object":"response",
            "output":[{
                "type":"function_call",
                "id":"function-call-1",
                "call_id":"provider-call-1",
                "name":"echo_alias",
                "arguments":"{\"text\":\"hello\"}",
                "status":"completed"
            }],
            "status":"completed",
            "usage":{
                "input_tokens":10,
                "output_tokens":5,
                "total_tokens":15,
                "output_tokens_details":{"reasoning_tokens":0}
            }
        }))
        .unwrap(),
    }
}

pub(super) fn descriptor() -> ToolDescriptor {
    ToolDescriptor {
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
            max_input_bytes: 1024,
            max_llm_result_bytes: 1024,
            max_artifact_bytes: 1024,
        },
    }
}

pub(super) fn grant() -> ToolGrant {
    ToolGrant {
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
        constraints: Default::default(),
        approval: Some(ToolApprovalPolicy::DescriptorDefault),
        failure_policy: None,
    }
}
