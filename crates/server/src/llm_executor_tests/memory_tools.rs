use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use serde_json::json;
use zhuangsheng_core::{
    application::secret::SecretValue,
    graph::{LlmMemoryBinding, MemoryToolCapability, MemoryToolGrant, NodeMemoryBinding},
    llm::{LlmChannelRevision, adapter::WireGenerationRequest},
    runtime::{
        MemoryProposalDecision, RunContextCommand, RunStatus, StartRunCommand,
        SubmitWaitResponseCommand, ToolApprovalDecisionKind, WaitResponsePayload,
    },
    scheduler::Scheduler,
};
use zhuangsheng_storage::SqliteStore;

use crate::{
    llm_executor::LocalLlmExecutor,
    provider::{ProviderHttpError, ProviderHttpResponse, ProviderTransport},
};

use super::{create_memory_llm_graph, now_ms, provider_response};

struct MemoryProposalProvider {
    calls: AtomicUsize,
}

#[async_trait]
impl ProviderTransport for MemoryProposalProvider {
    async fn send(
        &self,
        _channel: &LlmChannelRevision,
        wire: &WireGenerationRequest,
        _credential: Option<&SecretValue>,
    ) -> Result<ProviderHttpResponse, ProviderHttpError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        let body: serde_json::Value = serde_json::from_slice(wire.body()).unwrap();
        assert!(
            body["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["name"] == "propose_memory_change")
        );
        if call == 0 {
            return Ok(ProviderHttpResponse{status:200,provider_request_id:Some("memory-proposal-model".into()),body:serde_json::to_vec(&json!({
            "id":"memory-response-1","created_at":1,"object":"response","status":"completed",
            "output":[{"type":"function_call","id":"memory-function-1","call_id":"memory-provider-call-1","name":"propose_memory_change","arguments":serde_json::to_string(&json!({"scopeId":"roleplay","memoryId":null,"expectedHeadCommitId":null,"change":{"type":"create","content":{"schemaVersion":1,"text":"The northern gate is guarded.","tags":["story"],"attributes":{}}},"reason":"The current scene established this fact.","evidenceRefs":["message:current"]})).unwrap(),"status":"completed"}],
            "usage":{"input_tokens":10,"output_tokens":5,"total_tokens":15,"output_tokens_details":{"reasoning_tokens":0}}
        })).unwrap()});
        }
        let outputs = body["input"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["type"] == "function_call_output")
            .collect::<Vec<_>>();
        assert_eq!(outputs.len(), 1);
        assert!(
            outputs[0]["output"]
                .as_str()
                .unwrap()
                .contains("Memory proposal")
        );
        Ok(provider_response("记忆提案已审核。"))
    }
}

#[tokio::test]
async fn proposed_memory_change_waits_for_review_then_resumes_the_same_llm_loop() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let revision = create_memory_llm_graph(
        &store,
        LlmMemoryBinding {
            node: NodeMemoryBinding::default(),
            tools: vec![MemoryToolGrant {
                capability: MemoryToolCapability::ProposeMemoryChange,
                scopes: vec!["roleplay".into()],
                max_results: None,
                max_proposal_bytes: Some(256 * 1024),
            }],
        },
    )
    .await;
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision,
            input: json!({"message":"remember this"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "memory-proposal-e2e".into(),
        })
        .await
        .unwrap();
    let provider = Arc::new(MemoryProposalProvider {
        calls: AtomicUsize::new(0),
    });
    let executor = Arc::new(LocalLlmExecutor::with_provider(
        store.clone(),
        provider.clone(),
    ));
    Scheduler::new(store.clone(), "memory-tool-worker")
        .with_llm_executor(executor.clone())
        .run_until_idle(now_ms(), 128)
        .await
        .unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Waiting
    );
    let waits = store.list_open_waits(&run.id).await.unwrap();
    assert_eq!(waits.len(), 1);
    let proposal_id = waits[0].blockers[0].id.clone();
    assert_eq!(
        waits[0].request["proposals"][0]["proposal"]["reason"],
        "The current scene established this fact."
    );
    store
        .submit_wait_response(
            SubmitWaitResponseCommand {
                wait_id: waits[0].id.clone(),
                delivery_id: "review-memory-e2e".into(),
                actor_kind: "human".into(),
                actor_id: Some("reviewer".into()),
                payload: WaitResponsePayload::MemoryProposal {
                    decisions: vec![MemoryProposalDecision {
                        proposal_id,
                        decision: ToolApprovalDecisionKind::Approve,
                    }],
                },
            },
            now_ms(),
        )
        .await
        .unwrap();
    Scheduler::new(store.clone(), "memory-tool-resume-worker")
        .with_llm_executor(executor)
        .run_until_idle(now_ms(), 128)
        .await
        .unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Completed
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
}
