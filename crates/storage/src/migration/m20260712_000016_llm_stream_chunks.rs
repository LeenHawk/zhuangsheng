use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260712_000016_llm_stream_chunks"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                r#"CREATE TABLE llm_stream_chunks (
                    effect_attempt_id TEXT NOT NULL REFERENCES effect_attempts(id),
                    chunk_no INTEGER NOT NULL CHECK (chunk_no > 0),
                    model_call_id TEXT NOT NULL REFERENCES model_calls(id),
                    run_id TEXT NOT NULL REFERENCES graph_runs(id),
                    durable_seq INTEGER NOT NULL CHECK (durable_seq > 0),
                    payload_digest TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    PRIMARY KEY(effect_attempt_id, chunk_no),
                    UNIQUE(run_id, durable_seq)
                )"#,
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE llm_stream_chunks")
            .await?;
        Ok(())
    }
}
