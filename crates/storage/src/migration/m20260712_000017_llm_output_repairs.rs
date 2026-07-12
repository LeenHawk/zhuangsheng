use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260712_000017_llm_output_repairs"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                r#"CREATE TABLE llm_output_repairs (
                    id TEXT PRIMARY KEY NOT NULL,
                    node_instance_id TEXT NOT NULL REFERENCES node_instances(id),
                    repair_no INTEGER NOT NULL CHECK (repair_no > 0),
                    source_model_call_id TEXT NOT NULL UNIQUE REFERENCES model_calls(id),
                    originating_attempt_id TEXT NOT NULL REFERENCES node_attempts(id),
                    extracted_bytes_digest TEXT NOT NULL,
                    error_code TEXT NOT NULL,
                    error_object_id TEXT NOT NULL REFERENCES content_objects(id),
                    instruction_object_id TEXT NOT NULL REFERENCES content_objects(id),
                    request_digest TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    UNIQUE(node_instance_id, repair_no)
                )"#,
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE llm_output_repairs")
            .await?;
        Ok(())
    }
}
