use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260712_000013_durable_waits"
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
    r#"CREATE TABLE node_waits (
        id TEXT PRIMARY KEY NOT NULL,
        run_id TEXT NOT NULL REFERENCES graph_runs(id),
        node_instance_id TEXT NOT NULL REFERENCES node_instances(id),
        node_attempt_id TEXT NOT NULL REFERENCES node_attempts(id),
        kind TEXT NOT NULL CHECK (kind IN (
            'human_response','approval','webhook','timer','external_job',
            'effect_resolution','secret_store_unlocked'
        )),
        correlation_key TEXT,
        request_object_id TEXT NOT NULL REFERENCES content_objects(id),
        continuation_object_id TEXT NOT NULL REFERENCES content_objects(id),
        response_schema_object_id TEXT REFERENCES content_objects(id),
        response_schema_compilation_object_id TEXT REFERENCES content_objects(id),
        deadline_at INTEGER,
        on_timeout TEXT NOT NULL CHECK (on_timeout IN ('fail','resume_with_timeout')),
        status TEXT NOT NULL CHECK (status IN ('open','resolved','expired','cancelled')),
        response_object_id TEXT REFERENCES content_objects(id),
        accepted_delivery_id TEXT,
        created_at INTEGER NOT NULL,
        resolved_at INTEGER,
        CHECK (
            (response_schema_object_id IS NULL) =
            (response_schema_compilation_object_id IS NULL)
        ),
        CHECK (
            (status = 'open' AND response_object_id IS NULL AND resolved_at IS NULL)
            OR (status <> 'open' AND resolved_at IS NOT NULL)
        )
    )"#,
    r#"CREATE TABLE wait_blockers (
        wait_id TEXT NOT NULL REFERENCES node_waits(id),
        blocker_kind TEXT NOT NULL CHECK (blocker_kind IN ('tool_call','memory_proposal','effect')),
        blocker_id TEXT NOT NULL,
        blocker_order INTEGER NOT NULL CHECK (blocker_order >= 0),
        status TEXT NOT NULL CHECK (status IN ('open','satisfied','rejected','aborted')),
        decision_object_id TEXT REFERENCES content_objects(id),
        PRIMARY KEY(wait_id, blocker_kind, blocker_id),
        UNIQUE(wait_id, blocker_order),
        CHECK (
            (status = 'open' AND decision_object_id IS NULL)
            OR (status <> 'open' AND decision_object_id IS NOT NULL)
        )
    )"#,
    r#"CREATE TABLE wait_deliveries (
        wait_id TEXT NOT NULL REFERENCES node_waits(id),
        delivery_id TEXT NOT NULL,
        payload_digest TEXT NOT NULL,
        result_object_id TEXT NOT NULL REFERENCES content_objects(id),
        created_at INTEGER NOT NULL,
        PRIMARY KEY(wait_id, delivery_id)
    )"#,
    "CREATE UNIQUE INDEX node_waits_instance_open ON node_waits(node_instance_id) WHERE status = 'open'",
    "CREATE UNIQUE INDEX node_waits_correlation_open ON node_waits(kind, correlation_key) WHERE status = 'open' AND correlation_key IS NOT NULL",
    "CREATE INDEX node_waits_status_deadline ON node_waits(status, deadline_at)",
    "CREATE INDEX wait_blockers_lookup ON wait_blockers(blocker_kind, blocker_id, status)",
];

const DOWN: &[&str] = &[
    "DROP TABLE wait_deliveries",
    "DROP TABLE wait_blockers",
    "DROP TABLE node_waits",
];
