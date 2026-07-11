use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260712_000005_runtime_timers"
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
    r#"CREATE TABLE runtime_timers (
        id TEXT PRIMARY KEY NOT NULL,
        run_id TEXT NOT NULL REFERENCES graph_runs(id),
        node_instance_id TEXT REFERENCES node_instances(id),
        node_attempt_id TEXT REFERENCES node_attempts(id),
        kind TEXT NOT NULL CHECK (kind IN ('run_deadline','attempt_deadline','retry')),
        due_at INTEGER NOT NULL,
        dedupe_key TEXT NOT NULL UNIQUE,
        status TEXT NOT NULL CHECK (status IN ('pending','ready','fired','cancelled')),
        payload_object_id TEXT REFERENCES content_objects(id),
        created_at INTEGER NOT NULL,
        fired_at INTEGER
    )"#,
    "CREATE INDEX runtime_timers_due ON runtime_timers(status, due_at)",
    "CREATE INDEX runtime_timers_run ON runtime_timers(run_id, status)",
];

const DOWN: &[&str] = &["DROP TABLE runtime_timers"];
