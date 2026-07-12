use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260712_000021_artifacts"
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
            .execute_unprepared("DROP TABLE artifacts")
            .await?;
        Ok(())
    }
}

const UP: &[&str] = &[
    r#"CREATE TABLE artifacts (
        id TEXT PRIMARY KEY NOT NULL,
        context_id TEXT REFERENCES contexts(id),
        source_staging_id TEXT NOT NULL UNIQUE REFERENCES artifact_staging(id),
        content_object_id TEXT NOT NULL REFERENCES content_objects(id),
        metadata_head_commit_id TEXT NOT NULL REFERENCES version_commits(id),
        media_type TEXT NOT NULL,
        name TEXT,
        classification TEXT NOT NULL CHECK (classification IN ('public','private','sensitive')),
        retention_kind TEXT NOT NULL CHECK (retention_kind IN (
            'ephemeral','run','context','pinned','audit_until'
        )),
        retention_until INTEGER,
        status TEXT NOT NULL CHECK (status IN ('active','deleted')),
        origin_run_id TEXT REFERENCES graph_runs(id),
        origin_node_instance_id TEXT REFERENCES node_instances(id),
        origin_tool_call_id TEXT REFERENCES tool_calls(id),
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        CHECK ((retention_kind IN ('ephemeral','audit_until') AND retention_until IS NOT NULL)
            OR (retention_kind IN ('run','context','pinned') AND retention_until IS NULL)),
        CHECK (origin_node_instance_id IS NULL OR origin_run_id IS NOT NULL),
        CHECK (origin_tool_call_id IS NULL OR origin_node_instance_id IS NOT NULL)
    )"#,
    "CREATE INDEX artifacts_context_created ON artifacts(context_id, created_at)",
    "CREATE INDEX artifacts_status_retention ON artifacts(status, retention_kind, retention_until)",
    "CREATE INDEX artifacts_content ON artifacts(content_object_id)",
];
