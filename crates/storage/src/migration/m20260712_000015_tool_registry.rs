use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260712_000015_tool_registry"
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
    r#"CREATE TABLE tool_registry_entries (
        tool_id TEXT NOT NULL,
        tool_version TEXT NOT NULL,
        descriptor_json TEXT NOT NULL CHECK (json_valid(descriptor_json)),
        schema_bundle_object_id TEXT NOT NULL REFERENCES content_objects(id),
        descriptor_digest TEXT NOT NULL,
        implementation_digest TEXT NOT NULL,
        executor_key TEXT NOT NULL,
        enabled INTEGER NOT NULL CHECK (enabled IN (0,1)),
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        PRIMARY KEY(tool_id, tool_version)
    )"#,
    "CREATE INDEX tool_registry_enabled ON tool_registry_entries(enabled, tool_id, tool_version)",
];

const DOWN: &[&str] = &[
    "DROP INDEX tool_registry_enabled",
    "DROP TABLE tool_registry_entries",
];
