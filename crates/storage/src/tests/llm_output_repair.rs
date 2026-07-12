use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    canonical,
    llm::{
        EffectAttemptFence, FinishModelCallCommand, LlmLogicalCallStatus,
        LoadLlmResumeStateCommand, ModelCallEffectOutcome, PrepareLlmOutputRepairCommand,
        StartModelCallCommand,
        ir::{LlmContentPartIr, LlmTurnItemIr, MessageRole},
    },
};

use crate::{
    graph::helpers::{put_inline_object, sql},
    tests::{
        llm_ledger::{checkpoint, now_ms, prepare_command, prepare_running_llm_attempt},
        store,
    },
};

#[tokio::test]
async fn output_repair_persists_instruction_and_resumes_idempotently() {
    let store = store().await;
    let claimed = prepare_running_llm_attempt(&store).await;
    let snapshot = claimed.execution_snapshot.clone().unwrap();
    let now = now_ms();
    let snapshot_object_id = store
        .db
        .query_one_raw(sql(
            "SELECT execution_snapshot_object_id FROM node_instances WHERE id = ?",
            vec![claimed.node_instance_id.clone().into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "execution_snapshot_object_id")
        .unwrap();
    let transcript_ref = put_inline_object(
        &store.db,
        &canonical::to_vec(&json!({"schemaVersion":1,"items":[]})).unwrap(),
        now,
    )
    .await
    .unwrap();
    store
        .prepare_model_call(
            prepare_command(
                &claimed,
                &snapshot,
                checkpoint(
                    &claimed,
                    &snapshot_object_id,
                    &transcript_ref,
                    LlmLogicalCallStatus::Prepared,
                    None,
                ),
            ),
            now,
        )
        .await
        .unwrap();
    let fence = EffectAttemptFence {
        invoking_node_attempt_id: claimed.attempt_id.clone(),
        worker_id: claimed.worker_id.clone(),
        lease_fence: claimed.lease_fence,
        run_control_epoch: claimed.run_control_epoch,
    };
    store
        .start_model_call(
            StartModelCallCommand {
                effect_attempt_id: "effect-attempt-1".into(),
                fence: fence.clone(),
                provider_request_id: Some("provider-request-1".into()),
                checkpoint: checkpoint(
                    &claimed,
                    &snapshot_object_id,
                    &transcript_ref,
                    LlmLogicalCallStatus::Running,
                    None,
                ),
            },
            now + 1,
        )
        .await
        .unwrap();
    let assistant = LlmTurnItemIr::Message {
        id: "model-call-1:message:0".into(),
        role: MessageRole::Assistant,
        content: vec![LlmContentPartIr::Text {
            text: "not json".into(),
        }],
        provenance: None,
        placeholder: false,
    };
    let completed = store
        .finish_model_call(
            FinishModelCallCommand {
                effect_attempt_id: "effect-attempt-1".into(),
                fence: fence.clone(),
                outcome: ModelCallEffectOutcome::Completed {
                    response_bytes: canonical::to_vec(&json!({"text":"not json"})).unwrap(),
                    usage: None,
                },
                checkpoint: checkpoint(
                    &claimed,
                    &snapshot_object_id,
                    &transcript_ref,
                    LlmLogicalCallStatus::Completed,
                    None,
                ),
                transcript: Some(vec![assistant]),
            },
            now + 2,
        )
        .await
        .unwrap();
    let first = store
        .prepare_llm_output_repair(repair_command(&claimed, &fence, completed.clone()), now + 3)
        .await
        .unwrap();
    assert!(!first.replayed);
    assert_eq!(first.repair_no, 1);
    assert_eq!(first.transcript.last(), Some(&instruction()));
    let replay = store
        .prepare_llm_output_repair(repair_command(&claimed, &fence, completed), now + 4)
        .await
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.checkpoint.checksum, first.checkpoint.checksum);
    let state = store
        .load_llm_resume_state(LoadLlmResumeStateCommand {
            node_instance_id: claimed.node_instance_id,
            fence,
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.output_repairs_used, 1);
    assert_eq!(state.pending_output_repair.unwrap().repair_id, "repair-1");
    let events = store
        .db
        .query_one_raw(sql(
            "SELECT COUNT(*) AS count FROM run_events WHERE event_type = 'llm.output.repair_prepared'",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i64>("", "count")
        .unwrap();
    assert_eq!(events, 1);
}

fn repair_command(
    claimed: &zhuangsheng_core::scheduler::ClaimedAttempt,
    fence: &EffectAttemptFence,
    checkpoint: zhuangsheng_core::llm::LlmLoopCheckpoint,
) -> PrepareLlmOutputRepairCommand {
    PrepareLlmOutputRepairCommand {
        repair_id: "repair-1".into(),
        node_instance_id: claimed.node_instance_id.clone(),
        source_model_call_id: "model-call-1".into(),
        extracted_bytes_digest: canonical::hash_bytes(b"not json"),
        error_code: "llm_json_parse_failed".into(),
        instruction: instruction(),
        fence: fence.clone(),
        checkpoint,
    }
}

fn instruction() -> LlmTurnItemIr {
    LlmTurnItemIr::Message {
        id: "repair-1:instruction".into(),
        role: MessageRole::User,
        content: vec![LlmContentPartIr::Text {
            text: "Return valid JSON.".into(),
        }],
        provenance: None,
        placeholder: false,
    }
}
