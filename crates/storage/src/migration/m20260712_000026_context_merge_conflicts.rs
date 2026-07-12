use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260712_000026_context_merge_conflicts"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute_unprepared(CREATE).await?;
        manager.get_connection().execute_unprepared(INDEX).await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE context_merge_conflicts")
            .await?;
        Ok(())
    }
}

const CREATE: &str = r#"CREATE TABLE context_merge_conflicts (
    id TEXT PRIMARY KEY NOT NULL,
    context_id TEXT NOT NULL REFERENCES contexts(id),
    source_branch_id TEXT NOT NULL REFERENCES context_branches(id),
    target_branch_id TEXT NOT NULL REFERENCES context_branches(id),
    base_commit_id TEXT NOT NULL REFERENCES version_commits(id),
    source_head_commit_id TEXT NOT NULL REFERENCES version_commits(id),
    target_head_commit_id TEXT NOT NULL REFERENCES version_commits(id),
    path TEXT NOT NULL,
    base_value_object_id TEXT NOT NULL REFERENCES content_objects(id),
    source_value_object_id TEXT NOT NULL REFERENCES content_objects(id),
    target_value_object_id TEXT NOT NULL REFERENCES content_objects(id),
    status TEXT NOT NULL CHECK (status IN ('open','resolved')),
    resolution_object_id TEXT REFERENCES content_objects(id),
    created_at INTEGER NOT NULL,
    resolved_at INTEGER,
    UNIQUE(context_id, source_branch_id, target_branch_id, base_commit_id,
           source_head_commit_id, target_head_commit_id, path),
    CHECK ((status = 'resolved') = (resolution_object_id IS NOT NULL AND resolved_at IS NOT NULL))
)"#;

const INDEX: &str = "CREATE INDEX context_merge_conflicts_target_status ON context_merge_conflicts(target_branch_id, status)";
