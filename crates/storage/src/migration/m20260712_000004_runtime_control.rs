use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260712_000004_runtime_control"
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
    r#"CREATE TABLE run_commands (
        id TEXT PRIMARY KEY NOT NULL,
        run_id TEXT NOT NULL REFERENCES graph_runs(id),
        command_kind TEXT NOT NULL CHECK (command_kind IN ('interrupt','resume','cancel')),
        idempotency_key TEXT NOT NULL,
        request_digest TEXT NOT NULL,
        expected_control_epoch INTEGER NOT NULL CHECK (expected_control_epoch >= 0),
        payload_object_id TEXT REFERENCES content_objects(id),
        status TEXT NOT NULL CHECK (status IN ('pending','completed')),
        result_object_id TEXT REFERENCES content_objects(id),
        created_at INTEGER NOT NULL,
        applied_at INTEGER,
        UNIQUE(run_id, idempotency_key)
    )"#,
    "CREATE INDEX run_commands_history ON run_commands(run_id, created_at, id)",
];

const DOWN: &[&str] = &["DROP TABLE run_commands"];
