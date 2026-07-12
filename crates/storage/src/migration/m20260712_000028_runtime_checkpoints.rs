use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260712_000028_runtime_checkpoints"
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
    r#"CREATE TABLE runtime_checkpoints (
        id TEXT PRIMARY KEY NOT NULL,
        run_id TEXT NOT NULL REFERENCES graph_runs(id),
        context_branch_id TEXT NOT NULL,
        through_seq INTEGER NOT NULL CHECK (through_seq > 0),
        graph_revision_id TEXT NOT NULL REFERENCES graph_revisions(id),
        head_commit_id TEXT NOT NULL REFERENCES version_commits(id),
        snapshot_object_id TEXT NOT NULL REFERENCES content_objects(id),
        effect_watermark TEXT REFERENCES effect_attempts(id),
        schema_version INTEGER NOT NULL CHECK (schema_version = 1),
        checksum TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        UNIQUE(run_id, through_seq),
        FOREIGN KEY(context_branch_id) REFERENCES context_branches(id)
    )"#,
    "CREATE INDEX runtime_checkpoints_latest ON runtime_checkpoints(run_id, through_seq DESC)",
];

const DOWN: &[&str] = &[
    "DROP INDEX runtime_checkpoints_latest",
    "DROP TABLE runtime_checkpoints",
];
