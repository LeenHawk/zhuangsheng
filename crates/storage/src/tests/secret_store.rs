use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    application::secret::{
        ChangeMasterPasswordCommand, InitializeSecretStoreCommand, LockSecretStoreCommand,
        PutSecretCommand, SecretKind, SecretResolver, SecretValue, UnlockSecretStoreCommand,
    },
    llm::{SecretRef, SecretScheme},
};

use crate::{StorageError, graph::helpers::sql, secret::SecretStoreError};

const PASSWORD: &str = "correct horse battery staple";
const NEW_PASSWORD: &str = "new correct horse battery staple";
const API_KEY: &str = "sk-test-never-store-in-plaintext-42";

#[tokio::test]
async fn encrypted_secret_lifecycle_is_session_bound_and_restart_locked() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let path = file.path().to_owned();
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let store = crate::SqliteStore::connect(&url).await.unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert!(!store.secret_store_status().await.unwrap().initialized);

    let initialized = store
        .initialize_secret_store(InitializeSecretStoreCommand {
            master_password: secret(PASSWORD),
            idempotency_key: "initialize-1".into(),
        })
        .await
        .unwrap();
    let replay = store
        .initialize_secret_store(InitializeSecretStoreCommand {
            master_password: secret(PASSWORD),
            idempotency_key: "initialize-1".into(),
        })
        .await
        .unwrap();
    assert_eq!(initialized, replay);
    assert!(!store.secret_store_status().await.unwrap().locked);

    let metadata = store
        .put_secret_record(PutSecretCommand {
            secret_id: "primary-api-key".into(),
            name: Some("Primary API Key".into()),
            kind: SecretKind::ApiKey,
            value: secret(API_KEY),
            session_id: initialized.session_id.clone(),
            idempotency_key: "put-secret-1".into(),
        })
        .await
        .unwrap();
    assert_eq!(metadata.secret_ref.id, "primary-api-key");
    assert_eq!(store.list_secret_metadata().await.unwrap(), vec![metadata]);
    assert_resolves(&store).await;
    assert_files_do_not_contain(&path, PASSWORD.as_bytes());
    assert_files_do_not_contain(&path, API_KEY.as_bytes());

    store
        .lock_secret_store(LockSecretStoreCommand {
            expected_session_id: Some(initialized.session_id),
            idempotency_key: "lock-1".into(),
        })
        .await
        .unwrap();
    let locked = SecretResolver::resolve_secret(&store, &secret_ref()).await;
    assert!(matches!(
        locked,
        Err(zhuangsheng_core::application::ApplicationError::Conflict(
            "secret_store_locked"
        ))
    ));

    let wrong = store
        .unlock_secret_store(UnlockSecretStoreCommand {
            master_password: secret("this password is incorrect"),
            idempotency_key: "unlock-wrong".into(),
        })
        .await;
    assert!(matches!(
        wrong,
        Err(StorageError::SecretStore(SecretStoreError::UnlockFailed))
    ));
    let failed_receipts = store.db.query_one(sql(
        "SELECT COUNT(*) AS count FROM secret_command_receipts WHERE idempotency_key = 'unlock-wrong'",
        vec![],
    )).await.unwrap().unwrap();
    assert_eq!(failed_receipts.try_get::<i64>("", "count").unwrap(), 0);

    let unlocked = store
        .unlock_secret_store(UnlockSecretStoreCommand {
            master_password: secret(PASSWORD),
            idempotency_key: "unlock-1".into(),
        })
        .await
        .unwrap();
    assert_resolves(&store).await;
    store
        .lock_secret_store(LockSecretStoreCommand {
            expected_session_id: Some(unlocked.session_id),
            idempotency_key: "lock-2".into(),
        })
        .await
        .unwrap();
    let expired = store
        .unlock_secret_store(UnlockSecretStoreCommand {
            master_password: secret(PASSWORD),
            idempotency_key: "unlock-1".into(),
        })
        .await;
    assert!(matches!(
        expired,
        Err(StorageError::SecretStore(
            SecretStoreError::IdempotencyKeyExpired
        ))
    ));
    drop(store);

    let restarted = crate::SqliteStore::connect(&url).await.unwrap();
    assert!(restarted.secret_store_status().await.unwrap().locked);
    let session = restarted
        .unlock_secret_store(UnlockSecretStoreCommand {
            master_password: secret(PASSWORD),
            idempotency_key: "unlock-after-restart".into(),
        })
        .await
        .unwrap();
    assert_resolves(&restarted).await;
    restarted
        .change_secret_store_password(ChangeMasterPasswordCommand {
            current_password: secret(PASSWORD),
            new_password: secret(NEW_PASSWORD),
            session_id: session.session_id.clone(),
            idempotency_key: "change-password-1".into(),
        })
        .await
        .unwrap();
    restarted
        .lock_secret_store(LockSecretStoreCommand {
            expected_session_id: Some(session.session_id),
            idempotency_key: "lock-after-password-change".into(),
        })
        .await
        .unwrap();
    assert!(
        restarted
            .unlock_secret_store(UnlockSecretStoreCommand {
                master_password: secret(PASSWORD),
                idempotency_key: "unlock-old-password".into(),
            })
            .await
            .is_err()
    );
    restarted
        .unlock_secret_store(UnlockSecretStoreCommand {
            master_password: secret(NEW_PASSWORD),
            idempotency_key: "unlock-new-password".into(),
        })
        .await
        .unwrap();
    assert_resolves(&restarted).await;
}

#[tokio::test]
async fn ciphertext_tampering_fails_closed() {
    let store = crate::tests::store().await;
    let session = store
        .initialize_secret_store(InitializeSecretStoreCommand {
            master_password: secret(PASSWORD),
            idempotency_key: "tamper-init".into(),
        })
        .await
        .unwrap();
    store
        .put_secret_record(PutSecretCommand {
            secret_id: "primary-api-key".into(),
            name: None,
            kind: SecretKind::ApiKey,
            value: secret(API_KEY),
            session_id: session.session_id,
            idempotency_key: "tamper-put".into(),
        })
        .await
        .unwrap();
    store
        .db
        .execute(sql(
            "UPDATE secret_records SET ciphertext = X'00010203' WHERE id = 'primary-api-key'",
            vec![],
        ))
        .await
        .unwrap();
    let result = SecretResolver::resolve_secret(&store, &secret_ref()).await;
    assert!(matches!(
        result,
        Err(zhuangsheng_core::application::ApplicationError::Internal)
    ));
}

async fn assert_resolves(store: &crate::SqliteStore) {
    let value = SecretResolver::resolve_secret(store, &secret_ref())
        .await
        .unwrap();
    value.with_bytes(|bytes| assert_eq!(bytes, API_KEY.as_bytes()));
}

fn secret_ref() -> SecretRef {
    SecretRef {
        scheme: SecretScheme::Secret,
        id: "primary-api-key".into(),
    }
}

fn secret(value: &str) -> SecretValue {
    SecretValue::from_utf8(value.into())
}

fn assert_files_do_not_contain(path: &std::path::Path, needle: &[u8]) {
    for candidate in [
        path.to_owned(),
        std::path::PathBuf::from(format!("{}-wal", path.display())),
        std::path::PathBuf::from(format!("{}-shm", path.display())),
    ] {
        if let Ok(bytes) = std::fs::read(candidate) {
            assert!(!bytes.windows(needle.len()).any(|window| window == needle));
        }
    }
}
