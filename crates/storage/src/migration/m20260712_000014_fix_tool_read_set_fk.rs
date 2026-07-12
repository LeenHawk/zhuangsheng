use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260712_000014_fix_tool_read_set_fk"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        rebuild(manager, "version_commits").await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        rebuild(manager, "memory_commits").await
    }
}

async fn rebuild(manager: &SchemaManager<'_>, commit_table: &str) -> Result<(), DbErr> {
    let connection = manager.get_connection();
    connection
        .execute_unprepared("ALTER TABLE tool_call_read_set RENAME TO tool_call_read_set_legacy")
        .await?;
    connection
        .execute_unprepared(&format!(
            r#"CREATE TABLE tool_call_read_set (
                tool_call_id TEXT NOT NULL REFERENCES tool_calls(id),
                memory_id TEXT NOT NULL REFERENCES memory_records(id),
                commit_id TEXT NOT NULL REFERENCES {commit_table}(id),
                selection_ordinal INTEGER NOT NULL CHECK (selection_ordinal >= 0),
                selected_content_hash TEXT NOT NULL,
                PRIMARY KEY(tool_call_id, selection_ordinal),
                UNIQUE(tool_call_id, memory_id)
            )"#
        ))
        .await?;
    connection
        .execute_unprepared(
            r#"INSERT INTO tool_call_read_set (
                tool_call_id, memory_id, commit_id, selection_ordinal, selected_content_hash
            )
            SELECT tool_call_id, memory_id, commit_id, selection_ordinal, selected_content_hash
            FROM tool_call_read_set_legacy"#,
        )
        .await?;
    connection
        .execute_unprepared("DROP TABLE tool_call_read_set_legacy")
        .await?;
    Ok(())
}
