use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    application::secret::{
        InitializeSecretStoreCommand, LockSecretStoreCommand, SecretValue, UnlockSecretStoreCommand,
    },
    llm::adapter::{SensitiveEntryDraft, ShapeAdapterKey},
};

use crate::{
    StorageError,
    graph::helpers::sql,
    secret::SecretStoreError,
    tests::{
        llm_initial_model_call::command,
        llm_ledger::{now_ms, prepare_running_llm_attempt},
    },
};

const PASSWORD: &str = "opaque-bundle-password";
const MARKER: &str = "opaque-secret-marker-never-plaintext";

#[tokio::test]
async fn opaque_bundle_is_encrypted_pinned_idempotent_and_restart_safe() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let url = format!("sqlite://{}?mode=rwc", file.path().display());
    let store = crate::SqliteStore::connect(&url).await.unwrap();
    let session = store
        .initialize_secret_store(InitializeSecretStoreCommand {
            master_password: secret(PASSWORD),
            idempotency_key: "opaque-initialize".into(),
        })
        .await
        .unwrap();
    let claimed = prepare_running_llm_attempt(&store).await;
    let snapshot = claimed.execution_snapshot.clone().unwrap();
    let read_set_digest = claimed
        .context_snapshot
        .as_ref()
        .unwrap()
        .read_set_digest
        .clone();
    let now = now_ms();
    store
        .prepare_initial_model_call(command(&claimed, &snapshot, &read_set_digest), now)
        .await
        .unwrap();
    let operation = snapshot.operation;
    let bytes = zhuangsheng_core::canonical::to_vec(&serde_json::json!({
        "type":"reasoning",
        "encrypted_content":MARKER
    }))
    .unwrap();
    let entries = vec![draft(bytes.clone())];

    let stored = store
        .store_llm_opaque_bundle(
            "effect-attempt-initial",
            "model-call-initial",
            &operation,
            &entries,
            now + 1,
        )
        .await
        .unwrap();
    let reference = stored.entries["responses_item_0"].clone();
    let references = vec![reference.clone()];
    assert_eq!(
        store
            .load_llm_opaque_entries(&references, &operation, now + 2)
            .await
            .unwrap()[&format!("{}:responses_item_0", reference.entry_ref.object_id)],
        bytes
    );
    let replay = store
        .store_llm_opaque_bundle(
            "effect-attempt-initial",
            "model-call-initial",
            &operation,
            &entries,
            now + 3,
        )
        .await
        .unwrap();
    assert_eq!(replay.entries, stored.entries);
    let conflict = store
        .store_llm_opaque_bundle(
            "effect-attempt-initial",
            "model-call-initial",
            &operation,
            &[draft(b"different opaque value".to_vec())],
            now + 4,
        )
        .await;
    assert!(matches!(
        conflict,
        Err(StorageError::Conflict("opaque_bundle_replay"))
    ));

    let row = store
        .db
        .query_one_raw(sql(
            "SELECT ciphertext FROM internal_sensitive_objects",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap();
    let ciphertext: Vec<u8> = row.try_get("", "ciphertext").unwrap();
    assert!(!contains(&ciphertext, MARKER.as_bytes()));
    let schema = store
        .db
        .query_one_raw(sql(
            "SELECT sql FROM sqlite_master WHERE name = 'internal_sensitive_objects'",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "sql")
        .unwrap();
    assert!(!schema.contains("plaintext_hash"));

    store
        .lock_secret_store(LockSecretStoreCommand {
            expected_session_id: Some(session.session_id),
            idempotency_key: "opaque-lock".into(),
        })
        .await
        .unwrap();
    assert!(matches!(
        store
            .load_llm_opaque_entries(&references, &operation, now + 5)
            .await,
        Err(StorageError::SecretStore(SecretStoreError::Locked))
    ));
    drop(store);

    let restarted = crate::SqliteStore::connect(&url).await.unwrap();
    assert!(restarted.secret_store_status().await.unwrap().locked);
    restarted
        .unlock_secret_store(UnlockSecretStoreCommand {
            master_password: secret(PASSWORD),
            idempotency_key: "opaque-unlock-after-restart".into(),
        })
        .await
        .unwrap();
    assert_eq!(
        restarted
            .load_llm_opaque_entries(&references, &operation, now + 6)
            .await
            .unwrap()[&format!("{}:responses_item_0", reference.entry_ref.object_id)],
        bytes
    );

    let mut wrong_pin = reference.clone();
    wrong_pin.adapter_decoder_version += 1;
    assert!(matches!(
        restarted
            .load_llm_opaque_entries(&[wrong_pin], &operation, now + 7)
            .await,
        Err(StorageError::InvalidArgument(_))
    ));
    let mut wrong_digest = reference.clone();
    wrong_digest.digest = "sha256:tampered".into();
    assert!(matches!(
        restarted
            .load_llm_opaque_entries(&[wrong_digest], &operation, now + 8)
            .await,
        Err(StorageError::SecretStore(SecretStoreError::CorruptStore))
    ));
    restarted
        .db
        .execute_raw(sql(
            "UPDATE internal_sensitive_objects SET ciphertext = X'00010203'",
            vec![],
        ))
        .await
        .unwrap();
    assert!(matches!(
        restarted
            .load_llm_opaque_entries(&references, &operation, now + 9)
            .await,
        Err(StorageError::SecretStore(SecretStoreError::CorruptStore))
    ));
}

fn draft(opaque_bytes: Vec<u8>) -> SensitiveEntryDraft {
    SensitiveEntryDraft {
        entry_key: "responses_item_0".into(),
        adapter_key: ShapeAdapterKey::OpenAiResponsesV1,
        semantic_slot: "reasoning".into(),
        opaque_bytes,
    }
}

fn secret(value: &str) -> SecretValue {
    SecretValue::from_utf8(value.into())
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}
