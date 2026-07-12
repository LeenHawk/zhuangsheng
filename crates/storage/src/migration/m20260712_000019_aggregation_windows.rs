use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260712_000019_aggregation_windows"
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
    r#"CREATE TABLE aggregation_windows (
        id TEXT PRIMARY KEY NOT NULL,
        run_id TEXT NOT NULL REFERENCES graph_runs(id),
        node_id TEXT NOT NULL,
        node_instance_id TEXT NOT NULL UNIQUE REFERENCES node_instances(id),
        open_attempt_id TEXT NOT NULL UNIQUE REFERENCES node_attempts(id),
        status TEXT NOT NULL CHECK (status IN ('open','completed','cancelled')),
        count_limit INTEGER NOT NULL CHECK (count_limit > 0),
        item_count INTEGER NOT NULL CHECK (item_count > 0),
        opened_at INTEGER NOT NULL,
        deadline_at INTEGER NOT NULL,
        close_reason TEXT CHECK (close_reason IN ('count','timeout')),
        closed_at INTEGER,
        CHECK ((status = 'completed') = (close_reason IS NOT NULL)),
        CHECK ((status = 'open') = (closed_at IS NULL))
    )"#,
    r#"CREATE TABLE aggregation_window_items (
        window_id TEXT NOT NULL REFERENCES aggregation_windows(id),
        item_index INTEGER NOT NULL CHECK (item_index >= 0),
        queue_value_id TEXT NOT NULL UNIQUE REFERENCES edge_queue_values(id),
        enqueue_seq INTEGER NOT NULL CHECK (enqueue_seq > 0),
        selected_value_object_id TEXT NOT NULL REFERENCES content_objects(id),
        created_at INTEGER NOT NULL,
        PRIMARY KEY(window_id, item_index)
    )"#,
    "CREATE UNIQUE INDEX aggregation_one_open ON aggregation_windows(run_id, node_id) WHERE status = 'open'",
    "CREATE INDEX aggregation_due ON aggregation_windows(status, deadline_at)",
    "CREATE INDEX aggregation_items_order ON aggregation_window_items(window_id, item_index)",
];

const DOWN: &[&str] = &[
    "DROP TABLE aggregation_window_items",
    "DROP TABLE aggregation_windows",
];
