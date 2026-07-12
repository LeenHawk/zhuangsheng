use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    llm::ToolCallCheckpointStatus,
    runtime::{RunControlCommand, ToolApprovalDecisionKind, WaitKind},
};

use crate::{
    SqliteStore, StorageError,
    graph::helpers::sql,
    tests::{
        llm_tool_approval_support::{
            approval_command, load_checkpoint, prepare_model_tool_batch, response_command,
        },
        store,
    },
};

#[tokio::test]
async fn approval_batch_waits_without_effects_and_approve_prepares_all_siblings() {
    let store = store().await;
    let setup = prepare_model_tool_batch(&store).await;
    let opened = store
        .prepare_tool_approval_batch(approval_command(&setup), setup.now + 3)
        .await
        .unwrap();
    assert!(!opened.replayed);
    let replay = store
        .prepare_tool_approval_batch(approval_command(&setup), setup.now + 4)
        .await
        .unwrap();
    assert!(replay.replayed);
    assert_open_projection(&store).await;

    let stale = store
        .submit_wait_response(
            response_command("stale-digest", ToolApprovalDecisionKind::Approve),
            setup.now + 5,
        )
        .await
        .unwrap_err();
    assert!(matches!(stale, StorageError::InvalidArgument(_)));
    assert_open_projection(&store).await;

    let command = response_command(&setup.call_digest, ToolApprovalDecisionKind::Approve);
    let resolved = store
        .submit_wait_response(command.clone(), setup.now + 6)
        .await
        .unwrap();
    assert_eq!(
        resolved.prepared_tool_call_ids,
        vec!["tool-call-1", "tool-call-2"]
    );
    assert!(resolved.denied_tool_call_ids.is_empty());
    let replay = store
        .submit_wait_response(command, setup.now + 7)
        .await
        .unwrap();
    assert!(replay.replayed);

    let rows = store
        .db
        .query_all(sql(
            "SELECT tc.id, tc.status, e.id AS effect_id, ea.id AS attempt_id, ea.invoking_node_attempt_id FROM tool_calls tc JOIN effects e ON e.tool_call_id = tc.id JOIN effect_attempts ea ON ea.effect_id = e.id ORDER BY tc.call_index",
            vec![],
        ))
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .all(|row| row.try_get::<String>("", "status").unwrap() == "prepared")
    );
    let resume_attempt: String = rows[0].try_get("", "invoking_node_attempt_id").unwrap();
    assert_eq!(
        rows[1]
            .try_get::<String>("", "invoking_node_attempt_id")
            .unwrap(),
        resume_attempt
    );
    let wait = store
        .db
        .query_one(sql(
            "SELECT w.status, w.accepted_delivery_id, wb.status AS blocker_status, ni.status AS instance_status, c.open_waits FROM node_waits w JOIN wait_blockers wb ON wb.wait_id = w.id JOIN node_instances ni ON ni.id = w.node_instance_id JOIN run_execution_counters c ON c.run_id = w.run_id WHERE w.id = 'approval-wait-1'",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(wait.try_get::<String>("", "status").unwrap(), "resolved");
    assert_eq!(
        wait.try_get::<String>("", "blocker_status").unwrap(),
        "satisfied"
    );
    assert_eq!(
        wait.try_get::<String>("", "instance_status").unwrap(),
        "ready"
    );
    assert_eq!(wait.try_get::<i64>("", "open_waits").unwrap(), 0);
    let checkpoint = load_checkpoint(&store, &setup.claimed.node_instance_id).await;
    assert!(
        checkpoint
            .current_batch
            .iter()
            .all(|call| call.status == ToolCallCheckpointStatus::Prepared
                && call.effect_id.is_some()
                && call.wait_id.is_none())
    );
}

#[tokio::test]
async fn rejected_call_is_denied_while_unblocked_sibling_is_prepared() {
    let store = store().await;
    let setup = prepare_model_tool_batch(&store).await;
    store
        .prepare_tool_approval_batch(approval_command(&setup), setup.now + 3)
        .await
        .unwrap();
    let resolved = store
        .submit_wait_response(
            response_command(&setup.call_digest, ToolApprovalDecisionKind::Reject),
            setup.now + 4,
        )
        .await
        .unwrap();
    assert_eq!(resolved.prepared_tool_call_ids, vec!["tool-call-2"]);
    assert_eq!(resolved.denied_tool_call_ids, vec!["tool-call-1"]);
    let rows = store
        .db
        .query_all(sql(
            "SELECT id, status, error_object_id FROM tool_calls ORDER BY call_index",
            vec![],
        ))
        .await
        .unwrap();
    assert_eq!(rows[0].try_get::<String>("", "status").unwrap(), "denied");
    assert!(
        rows[0]
            .try_get::<Option<String>>("", "error_object_id")
            .unwrap()
            .is_some()
    );
    assert_eq!(rows[1].try_get::<String>("", "status").unwrap(), "prepared");
    let effects: i64 = store
        .db
        .query_one(sql(
            "SELECT COUNT(*) AS count FROM effects WHERE tool_call_id IS NOT NULL",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "count")
        .unwrap();
    assert_eq!(effects, 1);
}

#[tokio::test]
async fn terminal_run_aborts_approval_blocker_without_fabricating_effect() {
    let store = store().await;
    let setup = prepare_model_tool_batch(&store).await;
    store
        .prepare_tool_approval_batch(approval_command(&setup), setup.now + 3)
        .await
        .unwrap();
    let run_id: String = store
        .db
        .query_one(sql(
            "SELECT run_id FROM node_instances WHERE id = ?",
            vec![setup.claimed.node_instance_id.clone().into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "run_id")
        .unwrap();
    store
        .request_cancel(RunControlCommand {
            run_id,
            expected_epoch: setup.claimed.run_control_epoch,
            idempotency_key: "cancel-approval-wait".into(),
            reason: Some("test".into()),
        })
        .await
        .unwrap();
    let statuses: Vec<String> = store
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
    assert_eq!(
        statuses,
        vec!["cancelled_before_start", "cancelled_before_start"]
    );
    let blocker = store
        .db
        .query_one(sql(
            "SELECT status, decision_object_id FROM wait_blockers WHERE wait_id = 'approval-wait-1'",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(blocker.try_get::<String>("", "status").unwrap(), "aborted");
    assert!(
        blocker
            .try_get::<Option<String>>("", "decision_object_id")
            .unwrap()
            .is_some()
    );
    let tool_effects: i64 = store
        .db
        .query_one(sql(
            "SELECT COUNT(*) AS count FROM effects WHERE tool_call_id IS NOT NULL",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "count")
        .unwrap();
    assert_eq!(tool_effects, 0);
}

#[tokio::test]
async fn identical_digest_blockers_require_distinct_complete_decisions() {
    let store = store().await;
    let setup = prepare_model_tool_batch(&store).await;
    let mut batch = approval_command(&setup);
    batch.calls[1].binding_id = "echo-binding".into();
    batch.calls[1].call_digest = setup.call_digest.clone();
    batch.calls[1].risk_summary = "Second independent echo".into();
    batch.checkpoint.current_batch[1].call_digest = setup.call_digest.clone();
    batch.checkpoint.current_batch[1].status = ToolCallCheckpointStatus::AwaitingApproval;
    batch.checkpoint = batch.checkpoint.seal().unwrap();
    store
        .prepare_tool_approval_batch(batch, setup.now + 3)
        .await
        .unwrap();

    let mut response = response_command(&setup.call_digest, ToolApprovalDecisionKind::Approve);
    let incomplete = store
        .submit_wait_response(response.clone(), setup.now + 4)
        .await
        .unwrap_err();
    assert!(matches!(incomplete, StorageError::InvalidArgument(_)));
    let open: i64 = store
        .db
        .query_one(sql(
            "SELECT COUNT(*) AS count FROM wait_blockers WHERE wait_id = 'approval-wait-1' AND status = 'open'",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "count")
        .unwrap();
    assert_eq!(open, 2);

    let zhuangsheng_core::runtime::WaitResponsePayload::ToolApproval { decisions } =
        &mut response.payload;
    decisions.push(zhuangsheng_core::runtime::ToolApprovalDecision {
        tool_call_id: "tool-call-2".into(),
        call_digest: setup.call_digest.clone(),
        decision: ToolApprovalDecisionKind::Approve,
        reason: Some("reviewed independently".into()),
    });
    let resolved = store
        .submit_wait_response(response, setup.now + 5)
        .await
        .unwrap();
    assert_eq!(resolved.prepared_tool_call_ids.len(), 2);
    let satisfied: i64 = store
        .db
        .query_one(sql(
            "SELECT COUNT(*) AS count FROM wait_blockers WHERE wait_id = 'approval-wait-1' AND status = 'satisfied'",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "count")
        .unwrap();
    assert_eq!(satisfied, 2);
}

async fn assert_open_projection(store: &SqliteStore) {
    let rows = store
        .db
        .query_all(sql(
            "SELECT status FROM tool_calls ORDER BY call_index",
            vec![],
        ))
        .await
        .unwrap();
    assert_eq!(
        rows[0].try_get::<String>("", "status").unwrap(),
        "awaiting_approval"
    );
    assert_eq!(
        rows[1].try_get::<String>("", "status").unwrap(),
        "validated"
    );
    let effects: i64 = store
        .db
        .query_one(sql(
            "SELECT COUNT(*) AS count FROM effects WHERE tool_call_id IS NOT NULL",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "count")
        .unwrap();
    assert_eq!(effects, 0);
    let wait = store
        .db
        .query_one(sql(
            "SELECT w.status, wb.blocker_id, wb.status AS blocker_status FROM node_waits w JOIN wait_blockers wb ON wb.wait_id = w.id WHERE w.id = 'approval-wait-1'",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(wait.try_get::<String>("", "status").unwrap(), "open");
    assert_eq!(
        wait.try_get::<String>("", "blocker_id").unwrap(),
        "tool-call-1"
    );
    assert_eq!(
        wait.try_get::<String>("", "blocker_status").unwrap(),
        "open"
    );
    let run_id: String = store
        .db
        .query_one(sql(
            "SELECT run_id FROM node_waits WHERE id = 'approval-wait-1'",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "run_id")
        .unwrap();
    let views = store.list_open_waits(&run_id).await.unwrap();
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].kind, WaitKind::Approval);
    assert_eq!(views[0].blockers[0].id, "tool-call-1");
    assert_eq!(views[0].request["calls"][0]["toolCallId"], "tool-call-1");
    assert!(
        views[0].request["calls"][0]["callDigest"]
            .as_str()
            .is_some_and(|digest| !digest.is_empty())
    );
}
