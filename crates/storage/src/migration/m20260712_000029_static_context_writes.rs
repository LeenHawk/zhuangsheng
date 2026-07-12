use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260712_000029_static_context_writes"
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
            .execute_unprepared("DROP TABLE node_static_write_bases")
            .await?;
        Ok(())
    }
}

const UP: &str = r#"CREATE TABLE node_static_write_bases (
    node_instance_id TEXT PRIMARY KEY NOT NULL REFERENCES node_instances(id),
    context_id TEXT NOT NULL,
    branch_id TEXT NOT NULL,
    base_commit_id TEXT NOT NULL REFERENCES version_commits(id),
    created_at INTEGER NOT NULL,
    FOREIGN KEY(context_id, branch_id) REFERENCES context_branches(context_id, id)
)"#;
