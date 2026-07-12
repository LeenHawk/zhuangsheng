use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260712_000018_join_by_key"
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
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE coordination_buffer_items")
            .await?;
        Ok(())
    }
}

const UP: &[&str] = &[
    r#"CREATE TABLE coordination_buffer_items (
        id TEXT PRIMARY KEY NOT NULL,
        run_id TEXT NOT NULL REFERENCES graph_runs(id),
        node_id TEXT NOT NULL,
        input_port TEXT NOT NULL,
        queue_value_id TEXT NOT NULL UNIQUE REFERENCES edge_queue_values(id),
        enqueue_seq INTEGER NOT NULL CHECK (enqueue_seq > 0),
        key_json TEXT NOT NULL CHECK (json_valid(key_json)),
        key_canonical BLOB NOT NULL,
        status TEXT NOT NULL CHECK (status IN ('indexed','consumed','stranded','cancelled')),
        consumed_by_instance_id TEXT REFERENCES node_instances(id),
        created_at INTEGER NOT NULL,
        terminal_at INTEGER,
        CHECK ((status = 'indexed') = (terminal_at IS NULL)),
        CHECK ((status = 'consumed') = (consumed_by_instance_id IS NOT NULL))
    )"#,
    "CREATE INDEX coordination_ready ON coordination_buffer_items(run_id, node_id, status, key_canonical, input_port, enqueue_seq)",
    "CREATE INDEX coordination_queue ON coordination_buffer_items(queue_value_id, status)",
];
