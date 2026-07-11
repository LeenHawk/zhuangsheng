use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260712_000011_secret_store"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for statement in UP {
            manager
                .get_connection()
                .execute_unprepared(statement)
                .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for statement in DOWN {
            manager
                .get_connection()
                .execute_unprepared(statement)
                .await?;
        }
        Ok(())
    }
}

const UP: &[&str] = &[
    r#"CREATE TABLE secret_store_headers (
        singleton INTEGER PRIMARY KEY NOT NULL CHECK (singleton = 1),
        store_id TEXT NOT NULL UNIQUE,
        format_version INTEGER NOT NULL CHECK (format_version > 0),
        header_json TEXT NOT NULL CHECK (json_valid(header_json)),
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    )"#,
    r#"CREATE TABLE secret_records (
        id TEXT PRIMARY KEY NOT NULL,
        store_id TEXT NOT NULL REFERENCES secret_store_headers(store_id),
        name TEXT,
        kind TEXT NOT NULL CHECK (kind IN ('api_key','token')),
        key_version INTEGER NOT NULL CHECK (key_version > 0),
        algorithm TEXT NOT NULL CHECK (algorithm = 'xchacha20-poly1305'),
        nonce TEXT NOT NULL,
        ciphertext BLOB NOT NULL,
        status TEXT NOT NULL CHECK (status IN ('active','deleted')),
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        deleted_at INTEGER,
        CHECK ((status = 'active' AND deleted_at IS NULL)
            OR (status = 'deleted' AND deleted_at IS NOT NULL))
    )"#,
    r#"CREATE TABLE secret_command_receipts (
        scope TEXT NOT NULL,
        idempotency_key TEXT NOT NULL,
        command_kind TEXT NOT NULL,
        receipt_key_version INTEGER NOT NULL CHECK (receipt_key_version = 1),
        request_hmac BLOB NOT NULL,
        status TEXT NOT NULL CHECK (status IN ('completed','expired')),
        result_object_id TEXT REFERENCES content_objects(id),
        unlock_session_id TEXT,
        unlock_process_generation TEXT,
        result_expires_at INTEGER,
        created_at INTEGER NOT NULL,
        completed_at INTEGER,
        expired_at INTEGER,
        PRIMARY KEY(scope, idempotency_key),
        CHECK ((command_kind IN ('initialize_secret_store','unlock_secret_store')
                AND unlock_session_id IS NOT NULL AND unlock_process_generation IS NOT NULL)
            OR (command_kind NOT IN ('initialize_secret_store','unlock_secret_store')
                AND unlock_session_id IS NULL AND unlock_process_generation IS NULL))
    )"#,
    r#"CREATE TABLE secret_store_audit (
        id TEXT PRIMARY KEY NOT NULL,
        store_id TEXT,
        action TEXT NOT NULL,
        secret_id TEXT,
        result TEXT NOT NULL,
        created_at INTEGER NOT NULL
    )"#,
    "CREATE INDEX secret_records_store_status ON secret_records(store_id, status, created_at, id)",
    "CREATE INDEX secret_audit_created ON secret_store_audit(created_at, id)",
];

const DOWN: &[&str] = &[
    "DROP TABLE secret_store_audit",
    "DROP TABLE secret_command_receipts",
    "DROP TABLE secret_records",
    "DROP TABLE secret_store_headers",
];
