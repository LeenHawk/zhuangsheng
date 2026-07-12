use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use serde_json::json;
use tokio::sync::Notify;
use zhuangsheng_core::{
    application::tool::{
        PublishToolCommand, ToolCallOutput, ToolExecutionContext, ToolExecutionError, ToolExecutor,
        ToolOutputPart,
    },
    graph::{ApprovalRequiredAction, ToolFailureAction, ToolFailurePolicy},
    runtime::{RunContextCommand, RunStatus, StartRunCommand},
    scheduler::{Scheduler, SchedulerStore},
};
use zhuangsheng_storage::SqliteStore;

use crate::{llm_executor::LocalLlmExecutor, tool_executor::ToolExecutorRegistry};

use super::{
    create_llm_graph, now_ms,
    tool_registry::{EchoLoopProvider, descriptor, grant},
};

struct RecoverableEchoExecutor {
    calls: Arc<AtomicUsize>,
    first_started: Arc<Notify>,
    release_first: Arc<Notify>,
}

#[async_trait]
impl ToolExecutor for RecoverableEchoExecutor {
    async fn execute(
        &self,
        context: ToolExecutionContext,
    ) -> Result<ToolCallOutput, ToolExecutionError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            self.first_started.notify_one();
            self.release_first.notified().await;
        }
        let text = context.invocation.arguments["text"]
            .as_str()
            .unwrap()
            .to_owned();
        Ok(ToolCallOutput {
            parts: vec![ToolOutputPart::LlmResult {
                content: vec![zhuangsheng_core::llm::ir::LlmContentPartIr::Text { text }],
            }],
        })
    }
}

#[tokio::test]
async fn expired_started_tool_effect_retries_without_replaying_the_model_call() {
    const IMPLEMENTATION: &str = "sha256:recoverable-echo-v1";
    let store = Arc::new(SqliteStore::connect("sqlite::memory:").await.unwrap());
    store
        .publish_tool(PublishToolCommand {
            descriptor: descriptor(),
            implementation_digest: IMPLEMENTATION.into(),
            executor_key: "test.recoverable-echo".into(),
            enabled: true,
            idempotency_key: "server-publish-recoverable-echo".into(),
        })
        .await
        .unwrap();
    let mut recoverable_grant = grant();
    recoverable_grant.failure_policy = Some(ToolFailurePolicy {
        invalid_call: ToolFailureAction::ModelVisibleError,
        denied: ToolFailureAction::ModelVisibleError,
        approval_required: ApprovalRequiredAction::Wait,
        execution_error: ToolFailureAction::ModelVisibleError,
        max_attempts: 2,
        retry_backoff_ms: vec![0],
    });
    let revision_id = create_llm_graph(&store, false, None, Some(recoverable_grant)).await;
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision_id,
            input: json!({"message":"recover echo"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "server-tool-recovery-run".into(),
        })
        .await
        .unwrap();
    let provider_calls = Arc::new(AtomicUsize::new(0));
    let tool_calls = Arc::new(AtomicUsize::new(0));
    let first_started = Arc::new(Notify::new());
    let release_first = Arc::new(Notify::new());
    let mut tools = ToolExecutorRegistry::with_builtins();
    tools.register(
        "test.recoverable-echo",
        IMPLEMENTATION,
        Arc::new(RecoverableEchoExecutor {
            calls: tool_calls.clone(),
            first_started: first_started.clone(),
            release_first: release_first.clone(),
        }),
    );
    let executor = Arc::new(LocalLlmExecutor::with_provider_and_tools(
        store.clone(),
        Arc::new(EchoLoopProvider {
            calls: provider_calls.clone(),
        }),
        tools,
    ));
    let now = now_ms();
    let first_scheduler =
        Scheduler::new(store.clone(), "tool-recovery-worker").with_llm_executor(executor.clone());
    let first_run = tokio::spawn(async move { first_scheduler.run_until_idle(now, 128).await });
    tokio::time::timeout(std::time::Duration::from_secs(2), first_started.notified())
        .await
        .unwrap();
    assert!(
        SchedulerStore::recover_expired_leases(store.as_ref(), now + 31_000)
            .await
            .unwrap()
            >= 1
    );
    release_first.notify_waiters();
    assert!(first_run.await.unwrap().is_err());
    Scheduler::new(store.clone(), "tool-recovery-worker")
        .with_llm_executor(executor)
        .run_until_idle(now + 31_001, 128)
        .await
        .unwrap();
    let status = store.get_run(&run.id).await.unwrap().status;
    let events = store.list_run_events(&run.id, 0, 500).await.unwrap();
    assert_eq!(status, RunStatus::Completed, "events: {events:?}");
    assert_eq!(provider_calls.load(Ordering::SeqCst), 2);
    assert_eq!(tool_calls.load(Ordering::SeqCst), 2);
}
