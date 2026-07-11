use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260712_000009_long_term_memory"
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
    "ALTER TABLE version_commits ADD COLUMN source_proposal_id TEXT",
    r#"CREATE TABLE memory_scopes (
        id TEXT PRIMARY KEY NOT NULL,
        revision_no INTEGER NOT NULL DEFAULT 0 CHECK (revision_no >= 0),
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    )"#,
    r#"CREATE TABLE memory_records (
        id TEXT PRIMARY KEY NOT NULL,
        scope_id TEXT NOT NULL REFERENCES memory_scopes(id),
        status TEXT NOT NULL CHECK (status IN ('reserved','active','obsolete','deleted','discarded')),
        head_commit_id TEXT REFERENCES version_commits(id),
        current_content_object_id TEXT REFERENCES content_objects(id),
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        CHECK ((status = 'reserved' AND head_commit_id IS NULL)
            OR (status IN ('active','obsolete','deleted') AND head_commit_id IS NOT NULL)
            OR status = 'discarded'),
        CHECK (status NOT IN ('active','obsolete') OR current_content_object_id IS NOT NULL),
        CHECK (status != 'deleted' OR current_content_object_id IS NULL)
    )"#,
    r#"CREATE TABLE memory_change_proposals (
        id TEXT PRIMARY KEY NOT NULL,
        scope_id TEXT NOT NULL REFERENCES memory_scopes(id),
        memory_id TEXT NOT NULL REFERENCES memory_records(id),
        expected_head_commit_id TEXT,
        change_type TEXT NOT NULL CHECK (change_type IN ('create','replace_content','mark_obsolete','delete_tombstone')),
        content_object_id TEXT REFERENCES content_objects(id),
        reason TEXT NOT NULL,
        evidence_refs_json TEXT NOT NULL CHECK (json_valid(evidence_refs_json)),
        requested_by_kind TEXT NOT NULL CHECK (requested_by_kind IN ('user','system','node','tool','application')),
        requested_by_id TEXT,
        schema_version INTEGER NOT NULL CHECK (schema_version > 0),
        policy_version INTEGER NOT NULL CHECK (policy_version > 0),
        origin_run_id TEXT REFERENCES graph_runs(id),
        origin_node_instance_id TEXT REFERENCES node_instances(id),
        applied_commit_id TEXT REFERENCES version_commits(id),
        status TEXT NOT NULL CHECK (status IN ('proposed','awaiting_confirmation','awaiting_review','approved','rejected','applied','conflicted')),
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        CHECK ((change_type IN ('create','replace_content')) = (content_object_id IS NOT NULL)),
        CHECK ((change_type = 'create') = (expected_head_commit_id IS NULL))
    )"#,
    r#"CREATE TABLE memory_proposal_transitions (
        id TEXT PRIMARY KEY NOT NULL,
        proposal_id TEXT NOT NULL REFERENCES memory_change_proposals(id),
        transition_no INTEGER NOT NULL CHECK (transition_no > 0),
        from_status TEXT,
        to_status TEXT NOT NULL,
        actor_kind TEXT NOT NULL,
        actor_id TEXT,
        command_idempotency_key TEXT,
        created_at INTEGER NOT NULL,
        UNIQUE(proposal_id, transition_no),
        UNIQUE(proposal_id, command_idempotency_key)
    )"#,
    "CREATE INDEX memory_records_scope_status ON memory_records(scope_id, status, id)",
    "CREATE INDEX memory_proposals_status ON memory_change_proposals(scope_id, status, created_at)",
    "CREATE VIRTUAL TABLE memory_search USING fts5(memory_id UNINDEXED, scope_id UNINDEXED, text, tags, tokenize='unicode61')",
];

const DOWN: &[&str] = &[
    "DROP TABLE memory_search",
    "DROP TABLE memory_proposal_transitions",
    "DROP TABLE memory_change_proposals",
    "DROP TABLE memory_records",
    "DROP TABLE memory_scopes",
];
