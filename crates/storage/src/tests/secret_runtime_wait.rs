use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    application::secret::{
        InitializeSecretStoreCommand, LockSecretStoreCommand, PutSecretCommand,
        ResolveRuntimeSecretCommand, RuntimeSecretResolution, SecretKind, SecretValue,
        UnlockSecretStoreCommand,
    },
    llm::{SecretRef, SecretScheme},
    runtime::{SubmitWaitResponseCommand, WaitResponsePayload},
};

use crate::{
    graph::helpers::sql,
    runtime::compute_llm_read_set_digest,
    tests::{llm_ledger::prepare_running_llm_attempt, store},
};

const PASSWORD: &str = "runtime secret wait password";
const API_KEY: &str = "sk-runtime-wait-never-persist-plaintext";

#[tokio::test]
async fn locked_runtime_secret_waits_before_effect_and_unlock_resumes_same_snapshot() {
    let store = store().await;
    let claimed = prepare_running_llm_attempt(&store).await;
    let read_set_digest = claimed
        .context_snapshot
        .as_ref()
        .unwrap()
        .read_set_digest
        .clone();
    let original_snapshot_ref = snapshot_ref(&store, &claimed.node_instance_id).await;
    let session = store
        .initialize_secret_store(InitializeSecretStoreCommand {
            master_password: secret(PASSWORD),
            idempotency_key: "runtime-wait-init".into(),
        })
        .await
        .unwrap();
    store
        .put_secret_record(PutSecretCommand {
            secret_id: "runtime-api-key".into(),
            name: None,
            kind: SecretKind::ApiKey,
            value: secret(API_KEY),
            session_id: session.session_id.clone(),
            idempotency_key: "runtime-wait-put".into(),
        })
        .await
        .unwrap();
    store
        .lock_secret_store(LockSecretStoreCommand {
            expected_session_id: Some(session.session_id),
            idempotency_key: "runtime-wait-lock".into(),
        })
        .await
        .unwrap();

    let resolution = store
        .resolve_runtime_secret_value(&secret_ref(), command(&claimed, &read_set_digest), now_ms())
        .await
        .unwrap();
    let RuntimeSecretResolution::Waiting { wait_id } = resolution else {
        panic!("locked runtime secret should open a wait")
    };
    assert_waiting_without_effect(&store, &wait_id).await;
    assert_no_plaintext(&store).await;
    let public_delivery = store
        .submit_wait_response(
            SubmitWaitResponseCommand {
                wait_id: wait_id.clone(),
                delivery_id: "forbidden-public-unlock".into(),
                actor_kind: "human".into(),
                actor_id: Some("user-1".into()),
                payload: WaitResponsePayload::ToolApproval {
                    decisions: Vec::new(),
                },
            },
            now_ms(),
        )
        .await;
    assert!(matches!(
        public_delivery,
        Err(crate::StorageError::Conflict("wait_response_kind"))
    ));

    let unlocked = store
        .unlock_secret_store(UnlockSecretStoreCommand {
            master_password: secret(PASSWORD),
            idempotency_key: "runtime-wait-unlock".into(),
        })
        .await
        .unwrap();
    let row = store.db.query_one_raw(sql(
        "SELECT w.status AS wait_status, w.accepted_delivery_id, a.status AS source_status, ni.status AS instance_status, r.status AS run_status, c.open_waits FROM node_waits w JOIN node_attempts a ON a.id = w.node_attempt_id JOIN node_instances ni ON ni.id = w.node_instance_id JOIN graph_runs r ON r.id = w.run_id JOIN run_execution_counters c ON c.run_id = w.run_id WHERE w.id = ?",
        vec![wait_id.clone().into()],
    )).await.unwrap().unwrap();
    assert_eq!(
        row.try_get::<String>("", "wait_status").unwrap(),
        "resolved"
    );
    assert_eq!(
        row.try_get::<String>("", "source_status").unwrap(),
        "waiting"
    );
    assert_eq!(
        row.try_get::<String>("", "instance_status").unwrap(),
        "ready"
    );
    assert_eq!(row.try_get::<String>("", "run_status").unwrap(), "running");
    assert_eq!(row.try_get::<i64>("", "open_waits").unwrap(), 0);
    assert_eq!(
        row.try_get::<String>("", "accepted_delivery_id").unwrap(),
        format!("unlock:{}", unlocked.session_id)
    );
    let resume = store.db.query_one_raw(sql(
        "SELECT id, status, invocation_kind FROM node_attempts WHERE node_instance_id = ? AND invocation_kind = 'resume'",
        vec![claimed.node_instance_id.clone().into()],
    )).await.unwrap().unwrap();
    let resume_id: String = resume.try_get("", "id").unwrap();
    assert_eq!(resume.try_get::<String>("", "status").unwrap(), "queued");
    assert_eq!(
        compute_llm_read_set_digest(&store.db, &resume_id)
            .await
            .unwrap(),
        read_set_digest
    );
    assert_eq!(
        snapshot_ref(&store, &claimed.node_instance_id).await,
        original_snapshot_ref
    );
    assert_no_plaintext(&store).await;
}

#[tokio::test]
async fn initializing_store_resolves_preexisting_runtime_unlock_wait() {
    let store = store().await;
    let claimed = prepare_running_llm_attempt(&store).await;
    let digest = claimed
        .context_snapshot
        .as_ref()
        .unwrap()
        .read_set_digest
        .clone();
    let resolution = store
        .resolve_runtime_secret_value(&secret_ref(), command(&claimed, &digest), now_ms())
        .await
        .unwrap();
    let RuntimeSecretResolution::Waiting { wait_id } = resolution else {
        panic!("uninitialized store should open an unlock wait")
    };
    store
        .initialize_secret_store(InitializeSecretStoreCommand {
            master_password: secret(PASSWORD),
            idempotency_key: "runtime-wait-first-init".into(),
        })
        .await
        .unwrap();
    let row = store
        .db
        .query_one_raw(sql(
            "SELECT status, accepted_delivery_id FROM node_waits WHERE id = ?",
            vec![wait_id.into()],
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.try_get::<String>("", "status").unwrap(), "resolved");
    assert!(
        row.try_get::<String>("", "accepted_delivery_id")
            .unwrap()
            .starts_with("unlock:secretsession_")
    );
}

async fn assert_waiting_without_effect(store: &crate::SqliteStore, wait_id: &str) {
    let row = store.db.query_one_raw(sql(
        "SELECT w.kind, a.status AS attempt_status, ni.status AS instance_status, r.status AS run_status, c.open_waits FROM node_waits w JOIN node_attempts a ON a.id = w.node_attempt_id JOIN node_instances ni ON ni.id = w.node_instance_id JOIN graph_runs r ON r.id = w.run_id JOIN run_execution_counters c ON c.run_id = w.run_id WHERE w.id = ?",
        vec![wait_id.into()],
    )).await.unwrap().unwrap();
    assert_eq!(
        row.try_get::<String>("", "kind").unwrap(),
        "secret_store_unlocked"
    );
    assert_eq!(
        row.try_get::<String>("", "attempt_status").unwrap(),
        "waiting"
    );
    assert_eq!(
        row.try_get::<String>("", "instance_status").unwrap(),
        "waiting"
    );
    assert_eq!(row.try_get::<String>("", "run_status").unwrap(), "waiting");
    assert_eq!(row.try_get::<i64>("", "open_waits").unwrap(), 1);
    for table in [
        "model_calls",
        "effects",
        "effect_attempts",
        "llm_loop_checkpoints",
    ] {
        let query = format!("SELECT COUNT(*) AS count FROM {table}");
        let count: i64 = store
            .db
            .query_one_raw(sql(&query, vec![]))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "count")
            .unwrap();
        assert_eq!(count, 0, "{table} must remain empty before unlock");
    }
}

async fn assert_no_plaintext(store: &crate::SqliteStore) {
    let objects = store
        .db
        .query_all_raw(sql(
            "SELECT inline_bytes FROM content_objects WHERE inline_bytes IS NOT NULL",
            vec![],
        ))
        .await
        .unwrap();
    let events = store
        .db
        .query_all_raw(sql(
            "SELECT payload_json FROM run_events WHERE payload_json IS NOT NULL",
            vec![],
        ))
        .await
        .unwrap();
    for needle in [PASSWORD.as_bytes(), API_KEY.as_bytes()] {
        assert!(objects.iter().all(|row| {
            let bytes: Vec<u8> = row.try_get("", "inline_bytes").unwrap();
            !bytes.windows(needle.len()).any(|window| window == needle)
        }));
        assert!(events.iter().all(|row| {
            let payload: String = row.try_get("", "payload_json").unwrap();
            !payload
                .as_bytes()
                .windows(needle.len())
                .any(|window| window == needle)
        }));
    }
}

async fn snapshot_ref(store: &crate::SqliteStore, node_instance_id: &str) -> String {
    store
        .db
        .query_one_raw(sql(
            "SELECT execution_snapshot_object_id FROM node_instances WHERE id = ?",
            vec![node_instance_id.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "execution_snapshot_object_id")
        .unwrap()
}

fn command(
    claimed: &zhuangsheng_core::scheduler::ClaimedAttempt,
    read_set_digest: &str,
) -> ResolveRuntimeSecretCommand {
    ResolveRuntimeSecretCommand {
        run_id: claimed.run_id.clone(),
        node_instance_id: claimed.node_instance_id.clone(),
        attempt_id: claimed.attempt_id.clone(),
        wakeup_id: claimed.wakeup_id.clone(),
        worker_id: claimed.worker_id.clone(),
        lease_fence: claimed.lease_fence,
        run_control_epoch: claimed.run_control_epoch,
        channel_id: claimed
            .execution_snapshot
            .as_ref()
            .unwrap()
            .channel
            .channel_id
            .clone(),
        read_set_digest: read_set_digest.into(),
    }
}

fn secret_ref() -> SecretRef {
    SecretRef {
        scheme: SecretScheme::Secret,
        id: "runtime-api-key".into(),
    }
}

fn secret(value: &str) -> SecretValue {
    SecretValue::from_utf8(value.into())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}
