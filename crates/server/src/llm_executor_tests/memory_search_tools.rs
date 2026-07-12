use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use async_trait::async_trait;
use serde_json::json;
use zhuangsheng_core::{
    application::{
        memory::{
            ApplyMemoryProposalCommand, DecideMemoryProposalCommand, MemoryProposalDecision,
            ProposeMemoryChangeCommand,
        },
        secret::SecretValue,
    },
    graph::{LlmMemoryBinding, MemoryToolCapability, MemoryToolGrant, NodeMemoryBinding},
    llm::{LlmChannelRevision, adapter::WireGenerationRequest},
    memory::{LongTermMemoryContentV1, MemoryProposalChangeInput, MemoryProposalStatus},
    runtime::{RunContextCommand, RunStatus, StartRunCommand},
    scheduler::Scheduler,
    state::{ActorKind, ActorRef},
};
use zhuangsheng_storage::SqliteStore;

use crate::{
    llm_executor::LocalLlmExecutor,
    provider::{ProviderHttpError, ProviderHttpResponse, ProviderTransport},
};

use super::{create_memory_llm_graph, now_ms, provider_response};

struct MemorySearchProvider {
    calls: AtomicUsize,
}

#[async_trait]
impl ProviderTransport for MemorySearchProvider {
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
                .any(|tool| tool["name"] == "search_memory")
        );
        if call == 0 {
            return Ok(ProviderHttpResponse{status:200,provider_request_id:Some("memory-search-model".into()),body:serde_json::to_vec(&json!({
            "id":"memory-search-response-1","created_at":1,"object":"response","status":"completed",
            "output":[{"type":"function_call","id":"memory-search-function-1","call_id":"memory-search-provider-call-1","name":"search_memory","arguments":"{\"scopeId\":\"roleplay\",\"text\":\"northern gate\",\"tags\":[\"story\"],\"status\":null,\"limit\":5}","status":"completed"}],
            "usage":{"input_tokens":10,"output_tokens":5,"total_tokens":15,"output_tokens_details":{"reasoning_tokens":0}}
        })).unwrap()});
        }
        let output = body["input"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["type"] == "function_call_output")
            .unwrap();
        assert!(
            output["output"]
                .as_str()
                .unwrap()
                .contains("Dragons guard the northern gate")
        );
        Ok(provider_response("检索完成。"))
    }
}

#[tokio::test]
async fn search_memory_is_exposed_executed_and_replayed_into_the_next_model_call() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    seed_memory(&store).await;
    let revision = create_memory_llm_graph(
        &store,
        LlmMemoryBinding {
            node: NodeMemoryBinding::default(),
            tools: vec![MemoryToolGrant {
                capability: MemoryToolCapability::SearchMemory,
                scopes: vec!["roleplay".into()],
                max_results: Some(5),
                max_proposal_bytes: None,
            }],
        },
    )
    .await;
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision,
            input: json!({"message":"search lore"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "memory-search-e2e".into(),
        })
        .await
        .unwrap();
    let provider = Arc::new(MemorySearchProvider {
        calls: AtomicUsize::new(0),
    });
    Scheduler::new(store.clone(), "memory-search-worker")
        .with_llm_executor(Arc::new(LocalLlmExecutor::with_provider(
            store.clone(),
            provider.clone(),
        )))
        .run_until_idle(now_ms(), 128)
        .await
        .unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Completed
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    assert!(
        store
            .list_run_events(&run.id, 0, 500)
            .await
            .unwrap()
            .iter()
            .any(|event| event.event_type == "llm.tool.memory_search_completed")
    );
}

async fn seed_memory(store: &SqliteStore) {
    let proposal = store
        .propose_memory_change(ProposeMemoryChangeCommand {
            scope_id: "roleplay".into(),
            memory_id: None,
            expected_head_commit_id: None,
            change: MemoryProposalChangeInput::Create {
                content: LongTermMemoryContentV1 {
                    schema_version: 1,
                    text: "Dragons guard the northern gate".into(),
                    tags: vec!["story".into()],
                    attributes: BTreeMap::new(),
                },
            },
            reason: "seed".into(),
            evidence_refs: vec!["message:seed".into()],
            requested_by: ActorRef {
                kind: ActorKind::User,
                id: Some("tester".into()),
            },
            idempotency_key: "seed-memory-search".into(),
            schema_version: 1,
            policy_version: 1,
            origin_run_id: None,
            origin_node_instance_id: None,
        })
        .await
        .unwrap();
    store
        .decide_memory_proposal(DecideMemoryProposalCommand {
            proposal_id: proposal.id.clone(),
            expected_status: MemoryProposalStatus::AwaitingReview,
            decision: MemoryProposalDecision::Approve,
            actor: ActorRef {
                kind: ActorKind::User,
                id: Some("reviewer".into()),
            },
            idempotency_key: "approve-seed-memory-search".into(),
        })
        .await
        .unwrap();
    store
        .apply_memory_proposal(ApplyMemoryProposalCommand {
            proposal_id: proposal.id,
            expected_status: MemoryProposalStatus::Approved,
            idempotency_key: "apply-seed-memory-search".into(),
        })
        .await
        .unwrap();
}
