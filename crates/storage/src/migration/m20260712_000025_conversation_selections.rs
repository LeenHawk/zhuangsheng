use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260712_000025_conversation_selections"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute_unprepared(CREATE).await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE conversation_selections")
            .await?;
        Ok(())
    }
}

const CREATE: &str = r#"CREATE TABLE conversation_selections (
    turn_id TEXT PRIMARY KEY REFERENCES conversation_turns(id),
    selected_run_id TEXT NOT NULL,
    selection_scope TEXT NOT NULL,
    selection_key TEXT NOT NULL,
    selected_at INTEGER NOT NULL,
    UNIQUE(selection_scope, selection_key),
    FOREIGN KEY(turn_id, selected_run_id) REFERENCES turn_candidates(turn_id, run_id)
)"#;
