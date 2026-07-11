use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260712_000007_working_context"
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
    r#"CREATE TABLE domain_event_counters (
        aggregate_kind TEXT NOT NULL,
        aggregate_id TEXT NOT NULL,
        lineage_key TEXT NOT NULL,
        next_seq INTEGER NOT NULL CHECK (next_seq > 0),
        PRIMARY KEY(aggregate_kind, aggregate_id, lineage_key)
    )"#,
    r#"CREATE TABLE domain_events (
        id TEXT PRIMARY KEY NOT NULL,
        aggregate_kind TEXT NOT NULL,
        aggregate_id TEXT NOT NULL,
        lineage_key TEXT NOT NULL,
        seq INTEGER NOT NULL CHECK (seq > 0),
        event_type TEXT NOT NULL,
        schema_version INTEGER NOT NULL CHECK (schema_version > 0),
        payload_json TEXT CHECK (payload_json IS NULL OR json_valid(payload_json)),
        payload_object_id TEXT REFERENCES content_objects(id),
        created_at INTEGER NOT NULL,
        UNIQUE(aggregate_kind, aggregate_id, lineage_key, seq),
        CHECK ((payload_json IS NULL) <> (payload_object_id IS NULL))
    )"#,
    r#"CREATE TABLE node_output_commits (
        node_instance_id TEXT NOT NULL REFERENCES node_instances(id),
        commit_id TEXT NOT NULL REFERENCES version_commits(id),
        output_order INTEGER NOT NULL CHECK (output_order > 0),
        PRIMARY KEY(node_instance_id, output_order),
        UNIQUE(node_instance_id, commit_id)
    )"#,
    r#"CREATE TABLE version_snapshots (
        commit_id TEXT PRIMARY KEY NOT NULL REFERENCES version_commits(id),
        snapshot_object_id TEXT NOT NULL REFERENCES content_objects(id),
        schema_version INTEGER NOT NULL CHECK (schema_version > 0),
        checksum TEXT NOT NULL,
        retention_until INTEGER,
        pinned INTEGER NOT NULL CHECK (pinned IN (0,1)),
        created_at INTEGER NOT NULL
    )"#,
    "CREATE INDEX domain_events_sequence ON domain_events(aggregate_kind, aggregate_id, lineage_key, seq)",
];

const DOWN: &[&str] = &[
    "DROP TABLE version_snapshots",
    "DROP TABLE node_output_commits",
    "DROP TABLE domain_events",
    "DROP TABLE domain_event_counters",
];
