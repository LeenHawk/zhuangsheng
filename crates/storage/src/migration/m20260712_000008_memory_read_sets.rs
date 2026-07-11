use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260712_000008_memory_read_sets"
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
    r#"CREATE TABLE node_read_set (
        id TEXT PRIMARY KEY NOT NULL,
        node_attempt_id TEXT NOT NULL REFERENCES node_attempts(id),
        aggregate_kind TEXT NOT NULL CHECK (aggregate_kind IN ('working_context','long_term_memory','artifact_metadata')),
        aggregate_id TEXT NOT NULL,
        lineage_key TEXT NOT NULL,
        commit_id TEXT NOT NULL REFERENCES version_commits(id),
        binding_id TEXT NOT NULL,
        selection_ordinal INTEGER,
        selected_content_hash TEXT,
        consistency TEXT NOT NULL CHECK (consistency IN ('snapshot','validate_on_commit')),
        CHECK (selection_ordinal IS NULL OR selection_ordinal >= 0)
    )"#,
    r#"CREATE TABLE node_bound_read_results (
        node_attempt_id TEXT NOT NULL REFERENCES node_attempts(id),
        binding_id TEXT NOT NULL,
        envelope_object_id TEXT NOT NULL REFERENCES content_objects(id),
        result_digest TEXT NOT NULL,
        scope_snapshot_token TEXT,
        truncated INTEGER NOT NULL CHECK (truncated IN (0,1)),
        PRIMARY KEY(node_attempt_id, binding_id)
    )"#,
    "CREATE UNIQUE INDEX node_read_set_scalar ON node_read_set(node_attempt_id, binding_id, aggregate_kind, aggregate_id, lineage_key) WHERE selection_ordinal IS NULL",
    "CREATE UNIQUE INDEX node_read_set_ordered ON node_read_set(node_attempt_id, binding_id, selection_ordinal) WHERE selection_ordinal IS NOT NULL",
    "CREATE INDEX node_read_set_commit ON node_read_set(commit_id)",
];

const DOWN: &[&str] = &[
    "DROP TABLE node_bound_read_results",
    "DROP TABLE node_read_set",
];
