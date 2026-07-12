use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260712_000030_memory_proposal_tools"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute_unprepared(UP).await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE memory_proposal_tool_calls")
            .await?;
        Ok(())
    }
}

const UP: &str = r#"CREATE TABLE memory_proposal_tool_calls (
    proposal_id TEXT PRIMARY KEY NOT NULL REFERENCES memory_change_proposals(id),
    tool_call_id TEXT UNIQUE NOT NULL REFERENCES tool_calls(id),
    created_at INTEGER NOT NULL
)"#;
