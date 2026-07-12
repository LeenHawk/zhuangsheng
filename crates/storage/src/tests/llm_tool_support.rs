use std::sync::Arc;

use zhuangsheng_core::{
    application::{
        channel::{CreateChannelCommand, PublishChannelRevisionCommand},
        graph::{ApplyGraphCommand, CreateGraphCommand, UpdateGraphDraftCommand},
        preset::{CreateContextPresetCommand, PublishContextPresetVersionCommand},
    },
    graph::{ArtifactGrant, DraftNodeKind, ToolApprovalPolicy, ToolGrant},
    llm::context::{ContextAssemblyMode, ContextAssemblySpec},
    runtime::{RunContextCommand, StartRunCommand},
    scheduler::{ClaimedAttempt, Scheduler, SchedulerWork},
};

use crate::{
    SqliteStore,
    tests::llm_graph::{channel_spec, llm_draft},
};

pub(super) fn echo_grant() -> ToolGrant {
    ToolGrant {
        binding_id: "echo-binding".into(),
        tool_id: "echo-tool".into(),
        version: "1".into(),
        exposed_name: Some("echo".into()),
        scopes: vec![],
        artifact: ArtifactGrant {
            read_scopes: vec![],
            write_scopes: vec![],
            allowed_media_types: vec![],
            max_objects: 1,
            max_bytes: 1024,
        },
        constraints: Default::default(),
        approval: Some(ToolApprovalPolicy::DescriptorDefault),
        failure_policy: None,
    }
}

pub(super) async fn prepare_running_tool_attempt(store: &SqliteStore) -> ClaimedAttempt {
    let channel = store
        .create_channel(CreateChannelCommand {
            name: "Tool LLM".into(),
            idempotency_key: "tool-ledger-channel".into(),
        })
        .await
        .unwrap();
    let mut spec = channel_spec();
    spec.model_catalogs[0].models[0].capabilities.tool_calling = Some(true);
    store
        .publish_channel_revision(PublishChannelRevisionCommand {
            channel_id: channel.id.clone(),
            expected_head_revision_id: None,
            spec,
            idempotency_key: "tool-ledger-channel-revision".into(),
        })
        .await
        .unwrap();
    let preset = store
        .create_context_preset(CreateContextPresetCommand {
            name: "Tool RP".into(),
            idempotency_key: "tool-ledger-preset".into(),
        })
        .await
        .unwrap();
    store
        .publish_context_preset_version(PublishContextPresetVersionCommand {
            preset_id: preset.id.clone(),
            expected_head_version_id: None,
            spec: ContextAssemblySpec {
                id: None,
                name: None,
                mode: ContextAssemblyMode::Chat,
                items: vec![],
                budget: None,
                post_process: vec![],
                preview: None,
            },
            idempotency_key: "tool-ledger-preset-version".into(),
        })
        .await
        .unwrap();
    let graph = store
        .create_graph(CreateGraphCommand {
            name: "Tool Graph".into(),
            idempotency_key: "tool-ledger-graph".into(),
        })
        .await
        .unwrap();
    let current = store.get_graph_draft(&graph.graph.id).await.unwrap();
    let mut document = llm_draft(&graph.graph.id, &channel.id, &preset.id);
    let config = document
        .nodes
        .iter_mut()
        .find_map(|node| match &mut node.kind {
            DraftNodeKind::Llm { config } => Some(config),
            _ => None,
        })
        .unwrap();
    config.tools.push(echo_grant());
    let updated = store
        .update_graph_draft(UpdateGraphDraftCommand {
            graph_id: graph.graph.id.clone(),
            expected_revision_token: current.revision_token,
            document,
            idempotency_key: "tool-ledger-draft".into(),
        })
        .await
        .unwrap();
    let revision = store
        .apply_graph(ApplyGraphCommand {
            graph_id: graph.graph.id,
            expected_revision_token: updated.revision_token,
            operation_taxonomy_version: 1,
            adapter_decoder_version: 1,
            idempotency_key: "tool-ledger-apply".into(),
        })
        .await
        .unwrap();
    store
        .start_run(StartRunCommand {
            graph_revision_id: revision.id,
            input: serde_json::json!({"message":"use echo"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "tool-ledger-run".into(),
        })
        .await
        .unwrap();
    let now = super::llm_ledger::now_ms();
    Scheduler::new(Arc::new(store.clone()), "tool-ledger-worker")
        .run_one(now)
        .await
        .unwrap();
    for _ in 0..16 {
        let work = store
            .claim_next_work("tool-ledger-worker", now + 1, now + 30_000)
            .await
            .unwrap()
            .unwrap();
        match work {
            SchedulerWork::Attempt(attempt) => {
                if attempt.node.id == "generate" {
                    store.mark_attempt_running(&attempt, now + 1).await.unwrap();
                    return *attempt;
                }
                store.mark_attempt_running(&attempt, now + 1).await.unwrap();
                panic!("unexpected attempt before tool LLM")
            }
            SchedulerWork::Activate {
                wakeup_id,
                run_id,
                node_id,
            } => store
                .activate_if_ready(&wakeup_id, &run_id, &node_id, now + 1)
                .await
                .unwrap(),
            SchedulerWork::Settle { wakeup_id, run_id } => store
                .settle_run(&wakeup_id, &run_id, now + 1)
                .await
                .unwrap(),
            SchedulerWork::Noop => {}
        }
    }
    panic!("tool LLM attempt was not scheduled")
}
