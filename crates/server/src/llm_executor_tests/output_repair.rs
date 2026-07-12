use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use serde_json::json;
use zhuangsheng_core::{
    application::{secret::SecretValue, tool::PublishToolCommand},
    llm::{LlmChannelRevision, adapter::WireGenerationRequest},
    runtime::{RunContextCommand, RunOutputValueView, RunStatus, StartRunCommand},
    scheduler::{Scheduler, SchedulerStore},
};
use zhuangsheng_storage::SqliteStore;

use crate::{
    llm_executor::{LocalLlmExecutor, RepairPreparedPause},
    provider::{ProviderHttpError, ProviderHttpResponse, ProviderTransport},
    tool_executor::BUILTIN_ECHO_IMPLEMENTATION_DIGEST,
};

use super::{
    create_llm_graph, now_ms, provider_response,
    tool_registry::{descriptor, grant, tool_call_response},
};

struct RepairProvider {
    calls: Arc<AtomicUsize>,
    repair_succeeds: bool,
}

struct ToolThenRepairProvider {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ProviderTransport for ToolThenRepairProvider {
    async fn send(
        &self,
        _channel: &LlmChannelRevision,
        wire: &WireGenerationRequest,
        _credential: Option<&SecretValue>,
    ) -> Result<ProviderHttpResponse, ProviderHttpError> {
        match self.calls.fetch_add(1, Ordering::SeqCst) {
            0 => Ok(tool_call_response()),
            1 => {
                let body: serde_json::Value = serde_json::from_slice(wire.body()).unwrap();
                assert!(body["input"].to_string().contains("function_call_output"));
                Ok(provider_response("not json"))
            }
            2 => {
                let body: serde_json::Value = serde_json::from_slice(wire.body()).unwrap();
                let encoded = body["input"].to_string();
                assert!(encoded.contains("function_call_output"));
                assert!(encoded.contains("llm_json_parse_failed"));
                Ok(provider_response(r#"{"reply":"tool preserved"}"#))
            }
            call => panic!("unexpected provider call {call}"),
        }
    }
}

#[async_trait]
impl ProviderTransport for RepairProvider {
    async fn send(
        &self,
        _channel: &LlmChannelRevision,
        wire: &WireGenerationRequest,
        _credential: Option<&SecretValue>,
    ) -> Result<ProviderHttpResponse, ProviderHttpError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if call == 0 {
            return Ok(provider_response("not json"));
        }
        let request: serde_json::Value = serde_json::from_slice(wire.body()).unwrap();
        let encoded = request["input"].to_string();
        assert!(encoded.contains("not json"));
        assert!(encoded.contains("llm_json_parse_failed"));
        assert!(encoded.contains("Return exactly one JSON value"));
        Ok(provider_response(if self.repair_succeeds {
            r#"{"reply":"fixed"}"#
        } else {
            "still not json"
        }))
    }
}

#[tokio::test]
async fn invalid_json_is_repaired_on_the_same_durable_transcript() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let revision_id = create_llm_graph(&store, true, None, None).await;
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision_id,
            input: json!({"message":"json please"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "json-repair-run".into(),
        })
        .await
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let executor = Arc::new(LocalLlmExecutor::with_provider(
        store.clone(),
        Arc::new(RepairProvider {
            calls: calls.clone(),
            repair_succeeds: true,
        }),
    ));
    Scheduler::new(store.clone(), "json-repair-worker")
        .with_llm_executor(executor)
        .run_until_idle(now_ms(), 128)
        .await
        .unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Completed
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let outputs = store.get_run_outputs(&run.id).await.unwrap();
    assert!(matches!(
        &outputs["reply"].values[0],
        RunOutputValueView::InlineJson { value, .. }
            if value == &json!({"reply":"fixed"})
    ));
    let events = store.list_run_events(&run.id, 0, 500).await.unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "llm.output.repair_prepared")
            .count(),
        1
    );
}

#[tokio::test]
async fn invalid_json_fails_after_the_repair_budget_is_exhausted() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let revision_id = create_llm_graph(&store, true, None, None).await;
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision_id,
            input: json!({"message":"bad json"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "json-repair-exhausted-run".into(),
        })
        .await
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let executor = Arc::new(LocalLlmExecutor::with_provider(
        store.clone(),
        Arc::new(RepairProvider {
            calls: calls.clone(),
            repair_succeeds: false,
        }),
    ));
    Scheduler::new(store.clone(), "json-repair-exhausted-worker")
        .with_llm_executor(executor)
        .run_until_idle(now_ms(), 128)
        .await
        .unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Failed
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        store
            .list_run_events(&run.id, 0, 500)
            .await
            .unwrap()
            .iter()
            .filter(|event| event.event_type == "llm.output.repair_prepared")
            .count(),
        1
    );
}

#[tokio::test]
async fn prepared_repair_resumes_after_the_original_worker_loses_its_lease() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    let revision_id = create_llm_graph(&store, true, None, None).await;
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision_id,
            input: json!({"message":"recover repair"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "json-repair-recovery-run".into(),
        })
        .await
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let pause = Arc::new(RepairPreparedPause::new());
    let executor = Arc::new(
        LocalLlmExecutor::with_provider(
            store.clone(),
            Arc::new(RepairProvider {
                calls: calls.clone(),
                repair_succeeds: true,
            }),
        )
        .with_repair_pause(pause.clone()),
    );
    let now = now_ms();
    let old =
        Scheduler::new(store.clone(), "repair-old-worker").with_llm_executor(executor.clone());
    let old_run = tokio::spawn(async move { old.run_until_idle(now, 128).await });
    tokio::time::timeout(std::time::Duration::from_secs(2), pause.started.notified())
        .await
        .unwrap();
    assert!(
        SchedulerStore::recover_expired_leases(store.as_ref(), now + 31_000)
            .await
            .unwrap()
            >= 1
    );
    pause.release.notify_waiters();
    assert!(old_run.await.unwrap().is_err());
    Scheduler::new(store.clone(), "repair-new-worker")
        .with_llm_executor(executor)
        .run_until_idle(now + 31_001, 128)
        .await
        .unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Completed
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let outputs = store.get_run_outputs(&run.id).await.unwrap();
    assert!(matches!(
        &outputs["reply"].values[0],
        RunOutputValueView::InlineJson { value, .. }
            if value == &json!({"reply":"fixed"})
    ));
}

#[tokio::test]
async fn json_repair_reuses_completed_tool_results_without_dispatching_tools_again() {
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    store
        .publish_tool(PublishToolCommand {
            descriptor: descriptor(),
            implementation_digest: BUILTIN_ECHO_IMPLEMENTATION_DIGEST.into(),
            executor_key: "builtin.echo".into(),
            enabled: true,
            idempotency_key: "publish-json-repair-echo".into(),
        })
        .await
        .unwrap();
    let revision_id = create_llm_graph(&store, true, None, Some(grant())).await;
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision_id,
            input: json!({"message":"tool then json"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "json-repair-tool-run".into(),
        })
        .await
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let executor = Arc::new(LocalLlmExecutor::with_provider(
        store.clone(),
        Arc::new(ToolThenRepairProvider {
            calls: calls.clone(),
        }),
    ));
    Scheduler::new(store.clone(), "json-repair-tool-worker")
        .with_llm_executor(executor)
        .run_until_idle(now_ms(), 128)
        .await
        .unwrap();
    assert_eq!(
        store.get_run(&run.id).await.unwrap().status,
        RunStatus::Completed
    );
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    let events = store.list_run_events(&run.id, 0, 500).await.unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "llm.tool.completed")
            .count(),
        1
    );
    let outputs = store.get_run_outputs(&run.id).await.unwrap();
    assert!(matches!(
        &outputs["reply"].values[0],
        RunOutputValueView::InlineJson { value, .. }
            if value == &json!({"reply":"tool preserved"})
    ));
}
