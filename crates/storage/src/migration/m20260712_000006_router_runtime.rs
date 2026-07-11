use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260712_000006_router_runtime"
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
    r#"CREATE TABLE router_controls (
        run_id TEXT NOT NULL REFERENCES graph_runs(id),
        node_id TEXT NOT NULL,
        visits INTEGER NOT NULL CHECK (visits > 0),
        first_visited_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        PRIMARY KEY(run_id, node_id)
    )"#,
    r#"CREATE TABLE router_activation_controls (
        node_instance_id TEXT PRIMARY KEY NOT NULL REFERENCES node_instances(id),
        run_id TEXT NOT NULL REFERENCES graph_runs(id),
        node_id TEXT NOT NULL,
        visits INTEGER NOT NULL CHECK (visits > 0),
        first_visited_at INTEGER NOT NULL,
        decision_at INTEGER NOT NULL,
        elapsed_ms INTEGER NOT NULL CHECK (elapsed_ms >= 0),
        limit_reasons_json TEXT NOT NULL CHECK (json_valid(limit_reasons_json)),
        UNIQUE(run_id, node_id, visits)
    )"#,
    r#"CREATE TABLE router_decisions (
        node_instance_id TEXT PRIMARY KEY NOT NULL REFERENCES node_instances(id),
        attempt_id TEXT NOT NULL REFERENCES node_attempts(id),
        outcome TEXT NOT NULL CHECK (outcome IN ('decision','error')),
        decision_object_id TEXT NOT NULL REFERENCES content_objects(id),
        created_at INTEGER NOT NULL
    )"#,
    "CREATE INDEX router_controls_run ON router_controls(run_id)",
    "CREATE INDEX router_decisions_attempt ON router_decisions(attempt_id)",
];

const DOWN: &[&str] = &[
    "DROP TABLE router_decisions",
    "DROP TABLE router_activation_controls",
    "DROP TABLE router_controls",
];
