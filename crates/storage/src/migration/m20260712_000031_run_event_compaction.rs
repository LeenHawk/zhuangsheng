use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260712_000031_run_event_compaction"
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
    r#"CREATE TABLE run_event_compactions (
        run_id TEXT NOT NULL REFERENCES graph_runs(id),
        seq INTEGER NOT NULL CHECK (seq > 0),
        event_id TEXT NOT NULL UNIQUE,
        event_type TEXT NOT NULL,
        schema_version INTEGER NOT NULL CHECK (schema_version > 0),
        importance TEXT NOT NULL CHECK (importance IN ('debug','info')),
        payload_hash TEXT NOT NULL,
        checkpoint_id TEXT NOT NULL REFERENCES runtime_checkpoints(id),
        compacted_at INTEGER NOT NULL,
        PRIMARY KEY(run_id, seq)
    )"#,
    "CREATE INDEX run_event_compactions_checkpoint ON run_event_compactions(checkpoint_id, seq)",
];

const DOWN: &[&str] = &[
    "DROP INDEX run_event_compactions_checkpoint",
    "DROP TABLE run_event_compactions",
];
