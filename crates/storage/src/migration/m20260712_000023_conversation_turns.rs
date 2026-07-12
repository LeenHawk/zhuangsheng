use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260712_000023_conversation_turns"
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
        for table in [
            "conversation_run_bindings",
            "turn_candidates",
            "conversation_turns",
            "conversation_messages",
        ] {
            manager
                .get_connection()
                .execute_unprepared(&format!("DROP TABLE {table}"))
                .await?;
        }
        Ok(())
    }
}

const UP: &[&str] = &[
    r#"CREATE TABLE conversation_messages (
        id TEXT PRIMARY KEY NOT NULL,
        conversation_id TEXT NOT NULL REFERENCES conversations(id),
        turn_id TEXT NOT NULL,
        branch_id TEXT NOT NULL REFERENCES context_branches(id),
        commit_id TEXT NOT NULL UNIQUE REFERENCES version_commits(id),
        parent_message_id TEXT REFERENCES conversation_messages(id),
        role TEXT NOT NULL CHECK (role IN ('user','assistant')),
        source_kind TEXT NOT NULL CHECK (source_kind IN ('user_input','run_output','saved_partial')),
        content_object_id TEXT NOT NULL REFERENCES content_objects(id),
        origin_run_id TEXT REFERENCES graph_runs(id),
        created_at INTEGER NOT NULL,
        FOREIGN KEY(turn_id) REFERENCES conversation_turns(id) DEFERRABLE INITIALLY DEFERRED,
        CHECK ((role = 'user' AND source_kind = 'user_input' AND origin_run_id IS NULL)
            OR (role = 'assistant' AND source_kind IN ('run_output','saved_partial')
                AND origin_run_id IS NOT NULL AND parent_message_id IS NOT NULL))
    )"#,
    r#"CREATE TABLE conversation_turns (
        id TEXT PRIMARY KEY NOT NULL,
        conversation_id TEXT NOT NULL REFERENCES conversations(id),
        user_message_id TEXT NOT NULL UNIQUE REFERENCES conversation_messages(id) DEFERRABLE INITIALLY DEFERRED,
        user_commit_id TEXT NOT NULL UNIQUE REFERENCES version_commits(id),
        request_scope TEXT NOT NULL,
        request_key TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        UNIQUE(request_scope, request_key)
    )"#,
    r#"CREATE TABLE turn_candidates (
        turn_id TEXT NOT NULL REFERENCES conversation_turns(id),
        run_id TEXT NOT NULL UNIQUE REFERENCES graph_runs(id),
        branch_id TEXT NOT NULL REFERENCES context_branches(id),
        base_commit_id TEXT NOT NULL REFERENCES version_commits(id),
        reply_output_key TEXT NOT NULL,
        creation_scope TEXT NOT NULL,
        creation_key TEXT NOT NULL,
        assistant_message_id TEXT REFERENCES conversation_messages(id),
        candidate_commit_id TEXT REFERENCES version_commits(id),
        projection_error_object_id TEXT REFERENCES content_objects(id),
        status TEXT NOT NULL CHECK (status IN ('running','ready','failed','cancelled','projection_conflicted','projection_failed','projection_abandoned')),
        created_at INTEGER NOT NULL,
        PRIMARY KEY(turn_id, run_id),
        UNIQUE(creation_scope, creation_key)
    )"#,
    r#"CREATE TABLE conversation_run_bindings (
        run_id TEXT PRIMARY KEY REFERENCES graph_runs(id),
        conversation_id TEXT NOT NULL REFERENCES conversations(id),
        turn_id TEXT NOT NULL REFERENCES conversation_turns(id),
        reply_output_key TEXT NOT NULL
    )"#,
    "CREATE INDEX conversation_messages_branch ON conversation_messages(conversation_id, branch_id, created_at)",
    "CREATE INDEX turn_candidates_run ON turn_candidates(run_id)",
];
