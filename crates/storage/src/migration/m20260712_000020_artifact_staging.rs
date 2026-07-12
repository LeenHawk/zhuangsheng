use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260712_000020_artifact_staging"
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
            .execute_unprepared("DROP TABLE artifact_staging")
            .await?;
        Ok(())
    }
}

const UP: &[&str] = &[
    r#"CREATE TABLE artifact_staging (
        id TEXT PRIMARY KEY NOT NULL,
        context_id TEXT REFERENCES contexts(id),
        node_attempt_id TEXT REFERENCES node_attempts(id),
        tool_call_id TEXT REFERENCES tool_calls(id),
        temp_storage_key TEXT,
        expected_media_type TEXT,
        validated_media_type TEXT,
        byte_size INTEGER CHECK (byte_size IS NULL OR byte_size > 0),
        content_hash TEXT,
        metadata_draft_object_id TEXT NOT NULL REFERENCES content_objects(id),
        metadata_draft_digest TEXT NOT NULL,
        validated_content_object_id TEXT REFERENCES content_objects(id),
        status TEXT NOT NULL CHECK (status IN (
            'uploading','staged','validated','quarantined','deleting','deleted','committed'
        )),
        lifecycle_generation INTEGER NOT NULL DEFAULT 0 CHECK (lifecycle_generation >= 0),
        delete_fence TEXT,
        lease_until INTEGER,
        expires_at INTEGER NOT NULL,
        quarantined_at INTEGER,
        commit_request_digest TEXT,
        committed_artifact_id TEXT UNIQUE,
        commit_result_object_id TEXT REFERENCES content_objects(id),
        committed_at INTEGER,
        deleted_at INTEGER,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        CHECK ((status = 'uploading' AND byte_size IS NULL AND content_hash IS NULL
                AND validated_media_type IS NULL AND validated_content_object_id IS NULL
                AND lease_until IS NOT NULL AND quarantined_at IS NULL
                AND delete_fence IS NULL AND deleted_at IS NULL)
            OR (status = 'staged' AND byte_size IS NOT NULL AND content_hash IS NOT NULL
                AND validated_media_type IS NULL AND validated_content_object_id IS NULL
                AND lease_until IS NULL AND quarantined_at IS NULL
                AND delete_fence IS NULL AND deleted_at IS NULL)
            OR (status = 'validated' AND byte_size IS NOT NULL AND content_hash IS NOT NULL
                AND validated_media_type IS NOT NULL AND validated_content_object_id IS NOT NULL
                AND lease_until IS NULL AND quarantined_at IS NULL
                AND delete_fence IS NULL AND deleted_at IS NULL)
            OR (status = 'quarantined' AND quarantined_at IS NOT NULL
                AND lease_until IS NULL AND delete_fence IS NULL AND deleted_at IS NULL)
            OR (status = 'deleting' AND quarantined_at IS NOT NULL
                AND lease_until IS NULL AND delete_fence IS NOT NULL AND deleted_at IS NULL)
            OR (status = 'deleted' AND quarantined_at IS NOT NULL
                AND lease_until IS NULL AND delete_fence IS NOT NULL AND deleted_at IS NOT NULL)
            OR (status = 'committed' AND byte_size IS NOT NULL AND content_hash IS NOT NULL
                AND validated_media_type IS NOT NULL AND validated_content_object_id IS NOT NULL
                AND committed_artifact_id IS NOT NULL AND commit_result_object_id IS NOT NULL
                AND committed_at IS NOT NULL AND lease_until IS NULL
                AND quarantined_at IS NULL AND delete_fence IS NULL AND deleted_at IS NULL)),
        CHECK ((commit_request_digest IS NULL AND committed_artifact_id IS NULL
                AND commit_result_object_id IS NULL AND committed_at IS NULL)
            OR status = 'committed')
    )"#,
    "CREATE INDEX artifact_staging_status_expiry ON artifact_staging(status, expires_at)",
    "CREATE INDEX artifact_staging_context_created ON artifact_staging(context_id, created_at)",
];
