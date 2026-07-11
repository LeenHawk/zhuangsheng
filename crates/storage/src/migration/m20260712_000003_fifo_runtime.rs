use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260712_000003_fifo_runtime"
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
    r#"CREATE TABLE edge_queue_values (
        id TEXT PRIMARY KEY NOT NULL,
        run_id TEXT NOT NULL REFERENCES graph_runs(id),
        edge_id TEXT NOT NULL,
        enqueue_seq INTEGER NOT NULL CHECK (enqueue_seq > 0),
        producer_instance_id TEXT NOT NULL REFERENCES node_instances(id),
        producer_emission_index INTEGER NOT NULL CHECK (producer_emission_index >= 0),
        value_object_id TEXT NOT NULL REFERENCES content_objects(id),
        consumed_by_instance_id TEXT REFERENCES node_instances(id),
        consumed_at INTEGER,
        created_at INTEGER NOT NULL,
        CHECK ((consumed_by_instance_id IS NULL) = (consumed_at IS NULL)),
        UNIQUE(run_id, enqueue_seq),
        UNIQUE(run_id, edge_id, producer_instance_id, producer_emission_index)
    )"#,
    r#"CREATE TABLE run_output_values (
        id TEXT PRIMARY KEY NOT NULL,
        run_id TEXT NOT NULL REFERENCES graph_runs(id),
        output_key TEXT NOT NULL,
        collection_mode TEXT NOT NULL CHECK (collection_mode IN ('single','append')),
        output_seq INTEGER NOT NULL CHECK (output_seq > 0),
        node_instance_id TEXT NOT NULL REFERENCES node_instances(id),
        value_object_id TEXT NOT NULL REFERENCES content_objects(id),
        created_at INTEGER NOT NULL,
        UNIQUE(run_id, output_key, output_seq)
    )"#,
    "CREATE UNIQUE INDEX run_output_single ON run_output_values(run_id, output_key) WHERE collection_mode = 'single'",
    "CREATE INDEX edge_queue_pending ON edge_queue_values(run_id, edge_id, enqueue_seq) WHERE consumed_at IS NULL",
    "CREATE INDEX run_output_order ON run_output_values(run_id, output_key, output_seq)",
];

const DOWN: &[&str] = &[
    "DROP TABLE run_output_values",
    "DROP TABLE edge_queue_values",
];
