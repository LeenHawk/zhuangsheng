use sea_orm::ConnectionTrait;
use zhuangsheng_core::llm::{EffectAttemptFence, StartToolCallCommand, ToolCallCheckpointStatus};
use zhuangsheng_core::runtime::ToolApprovalDecisionKind;

use crate::{
    SqliteStore, StorageError,
    graph::helpers::sql,
    tests::{
        llm_tool_approval_support::{
            approval_command, load_checkpoint, prepare_model_tool_batch,
            prepare_non_idempotent_model_tool_batch, response_command,
        },
        store,
    },
};

#[tokio::test]
async fn expired_parallel_started_tools_reconcile_as_one_checkpoint_batch() {
    let store = store().await;
    let setup = prepare_model_tool_batch(&store).await;
    store
        .prepare_tool_approval_batch(approval_command(&setup), setup.now + 3)
        .await
        .unwrap();
    store
        .submit_wait_response(
            response_command(&setup.call_digest, ToolApprovalDecisionKind::Approve),
            setup.now + 4,
        )
        .await
        .unwrap();
    let fence = activate_resume(&store, &setup.claimed.node_instance_id, setup.now).await;

    let mut first = load_checkpoint(&store, &setup.claimed.node_instance_id).await;
    first.current_batch[0].status = ToolCallCheckpointStatus::Running;
    first.effect_watermark = "tool-effect-attempt-1".into();
    first = first.seal().unwrap();
    store
        .start_tool_call(
            StartToolCallCommand {
                effect_attempt_id: "tool-effect-attempt-1".into(),
                fence: fence.clone(),
                provider_request_id: Some("parallel-request-1".into()),
                checkpoint: first,
            },
            setup.now + 4,
        )
        .await
        .unwrap();
    let mut second = load_checkpoint(&store, &setup.claimed.node_instance_id).await;
    second.current_batch[1].status = ToolCallCheckpointStatus::Running;
    second.effect_watermark = "tool-effect-attempt-2".into();
    second = second.seal().unwrap();
    store
        .start_tool_call(
            StartToolCallCommand {
                effect_attempt_id: "tool-effect-attempt-2".into(),
                fence,
                provider_request_id: Some("parallel-request-2".into()),
                checkpoint: second,
            },
            setup.now + 4,
        )
        .await
        .unwrap();

    assert_eq!(
        store.recover_expired_leases(setup.now + 6).await.unwrap(),
        1
    );
    let tool_statuses: Vec<String> = store
        .db
        .query_all(sql(
            "SELECT status FROM tool_calls ORDER BY call_index",
            vec![],
        ))
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.try_get("", "status").unwrap())
        .collect();
    assert_eq!(tool_statuses, vec!["retry_ready", "retry_ready"]);
    let attempt_statuses: Vec<String> = store
        .db
        .query_all(sql(
            "SELECT status FROM effect_attempts WHERE effect_id IN ('tool-effect-1','tool-effect-2') ORDER BY effect_id",
            vec![],
        ))
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.try_get("", "status").unwrap())
        .collect();
    assert_eq!(attempt_statuses, vec!["outcome_unknown", "outcome_unknown"]);
    let checkpoint = load_checkpoint(&store, &setup.claimed.node_instance_id).await;
    assert!(
        checkpoint
            .current_batch
            .iter()
            .all(|call| call.status == ToolCallCheckpointStatus::RetryReady)
    );
    let reconcile: i64 = store
        .db
        .query_one(sql(
            "SELECT COUNT(*) AS count FROM node_attempts WHERE node_instance_id = ? AND invocation_kind = 'reconcile' AND status = 'queued'",
            vec![setup.claimed.node_instance_id.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "count")
        .unwrap();
    assert_eq!(reconcile, 1);
}

#[tokio::test]
async fn non_idempotent_tool_blocks_later_siblings_until_terminal() {
    let store = store().await;
    let setup = prepare_non_idempotent_model_tool_batch(&store).await;
    let batch = approval_command(&setup);
    store
        .prepare_tool_approval_batch(batch, setup.now + 3)
        .await
        .unwrap();
    store
        .submit_wait_response(
            response_command(&setup.call_digest, ToolApprovalDecisionKind::Approve),
            setup.now + 4,
        )
        .await
        .unwrap();
    let fence = activate_resume(&store, &setup.claimed.node_instance_id, setup.now).await;

    let mut later = load_checkpoint(&store, &setup.claimed.node_instance_id).await;
    later.current_batch[1].status = ToolCallCheckpointStatus::Running;
    later.effect_watermark = "tool-effect-attempt-2".into();
    later = later.seal().unwrap();
    let blocked = store
        .start_tool_call(
            StartToolCallCommand {
                effect_attempt_id: "tool-effect-attempt-2".into(),
                fence: fence.clone(),
                provider_request_id: Some("later-too-early".into()),
                checkpoint: later,
            },
            setup.now + 4,
        )
        .await
        .unwrap_err();
    assert!(matches!(
        blocked,
        StorageError::Conflict("tool_non_idempotent_serial")
    ));

    let mut first = load_checkpoint(&store, &setup.claimed.node_instance_id).await;
    first.current_batch[0].status = ToolCallCheckpointStatus::Running;
    first.effect_watermark = "tool-effect-attempt-1".into();
    first = first.seal().unwrap();
    store
        .start_tool_call(
            StartToolCallCommand {
                effect_attempt_id: "tool-effect-attempt-1".into(),
                fence: fence.clone(),
                provider_request_id: Some("non-idempotent-first".into()),
                checkpoint: first,
            },
            setup.now + 4,
        )
        .await
        .unwrap();
    let mut later = load_checkpoint(&store, &setup.claimed.node_instance_id).await;
    later.current_batch[1].status = ToolCallCheckpointStatus::Running;
    later.effect_watermark = "tool-effect-attempt-2".into();
    later = later.seal().unwrap();
    let blocked = store
        .start_tool_call(
            StartToolCallCommand {
                effect_attempt_id: "tool-effect-attempt-2".into(),
                fence,
                provider_request_id: Some("later-while-unknown".into()),
                checkpoint: later,
            },
            setup.now + 4,
        )
        .await
        .unwrap_err();
    assert!(matches!(
        blocked,
        StorageError::Conflict("tool_non_idempotent_serial")
    ));
}

async fn activate_resume(
    store: &SqliteStore,
    node_instance_id: &str,
    now: i64,
) -> EffectAttemptFence {
    let resume = store
        .db
        .query_one(sql(
            "SELECT id, run_control_epoch FROM node_attempts WHERE node_instance_id = ? AND invocation_kind = 'resume'",
            vec![node_instance_id.into()],
        ))
        .await
        .unwrap()
        .unwrap();
    let resume_attempt_id: String = resume.try_get("", "id").unwrap();
    let run_control_epoch: i64 = resume.try_get("", "run_control_epoch").unwrap();
    store
        .db
        .execute(sql(
            "UPDATE node_attempts SET status = 'running', worker_id = 'parallel-worker', lease_fence = 1, lease_until = ?, started_at = ? WHERE id = ? AND status = 'queued'",
            vec![(now + 5).into(), (now + 4).into(), resume_attempt_id.clone().into()],
        ))
        .await
        .unwrap();
    store
        .db
        .execute(sql(
            "UPDATE node_instances SET status = 'running' WHERE id = ? AND status = 'ready'",
            vec![node_instance_id.into()],
        ))
        .await
        .unwrap();
    EffectAttemptFence {
        invoking_node_attempt_id: resume_attempt_id,
        worker_id: "parallel-worker".into(),
        lease_fence: 1,
        run_control_epoch: u64::try_from(run_control_epoch).unwrap(),
    }
}
