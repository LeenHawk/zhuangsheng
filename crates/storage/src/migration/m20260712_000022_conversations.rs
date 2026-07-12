use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260712_000022_conversations"
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
            .execute_unprepared("DROP TABLE conversations")
            .await?;
        Ok(())
    }
}

const UP: &[&str] = &[
    r#"CREATE TABLE conversations (
        id TEXT PRIMARY KEY NOT NULL,
        context_id TEXT NOT NULL UNIQUE REFERENCES contexts(id),
        active_branch_id TEXT NOT NULL,
        active_head_commit_id TEXT NOT NULL REFERENCES version_commits(id),
        default_graph_revision_id TEXT REFERENCES graph_revisions(id),
        default_reply_output_key TEXT,
        default_input_shape TEXT CHECK (
            default_input_shape IS NULL OR default_input_shape = 'conversation_message_v1'
        ),
        run_profile_revision_no INTEGER CHECK (
            run_profile_revision_no IS NULL OR run_profile_revision_no > 0
        ),
        title TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        FOREIGN KEY(context_id, active_branch_id) REFERENCES context_branches(context_id, id),
        CHECK ((default_graph_revision_id IS NULL
                AND default_reply_output_key IS NULL
                AND default_input_shape IS NULL
                AND run_profile_revision_no IS NULL)
            OR (default_graph_revision_id IS NOT NULL
                AND default_reply_output_key IS NOT NULL
                AND default_input_shape IS NOT NULL
                AND run_profile_revision_no IS NOT NULL))
    )"#,
    "CREATE INDEX conversations_updated ON conversations(updated_at DESC, id)",
];
