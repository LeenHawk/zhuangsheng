use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260712_000001_graph"
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
    r#"CREATE TABLE content_objects (
        id TEXT PRIMARY KEY NOT NULL,
        content_hash TEXT NOT NULL UNIQUE,
        byte_size INTEGER NOT NULL CHECK (byte_size >= 0),
        storage_kind TEXT NOT NULL CHECK (storage_kind = 'inline'),
        lifecycle TEXT NOT NULL CHECK (lifecycle IN ('live','deleting','deleted')),
        lifecycle_generation INTEGER NOT NULL DEFAULT 0,
        delete_fence TEXT,
        inline_bytes BLOB,
        storage_key TEXT,
        created_at INTEGER NOT NULL,
        deleted_at INTEGER,
        CHECK ((lifecycle = 'live' AND inline_bytes IS NOT NULL AND storage_key IS NULL)
            OR lifecycle IN ('deleting','deleted'))
    )"#,
    r#"CREATE TABLE graphs (
        id TEXT PRIMARY KEY NOT NULL,
        name TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    )"#,
    r#"CREATE TABLE graph_drafts (
        graph_id TEXT PRIMARY KEY NOT NULL REFERENCES graphs(id) ON DELETE CASCADE,
        document_json TEXT NOT NULL CHECK (json_valid(document_json)),
        revision_token TEXT NOT NULL,
        updated_at INTEGER NOT NULL
    )"#,
    r#"CREATE TABLE graph_revisions (
        id TEXT PRIMARY KEY NOT NULL,
        graph_id TEXT NOT NULL REFERENCES graphs(id),
        revision_no INTEGER NOT NULL CHECK (revision_no > 0),
        operation_taxonomy_version INTEGER NOT NULL CHECK (operation_taxonomy_version > 0),
        adapter_decoder_version INTEGER NOT NULL CHECK (adapter_decoder_version > 0),
        definition_json TEXT NOT NULL CHECK (json_valid(definition_json)),
        schema_bundle_object_id TEXT NOT NULL REFERENCES content_objects(id),
        content_hash TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        UNIQUE(graph_id, revision_no),
        UNIQUE(graph_id, content_hash)
    )"#,
    r#"CREATE TABLE content_object_refs (
        object_id TEXT NOT NULL REFERENCES content_objects(id),
        owner_kind TEXT NOT NULL,
        owner_id TEXT NOT NULL,
        role TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        PRIMARY KEY(object_id, owner_kind, owner_id, role)
    )"#,
    r#"CREATE TABLE application_command_receipts (
        scope TEXT NOT NULL,
        idempotency_key TEXT NOT NULL,
        request_digest TEXT NOT NULL,
        command_kind TEXT NOT NULL,
        resource_kind TEXT,
        resource_id TEXT,
        status TEXT NOT NULL CHECK (status IN ('pending','completed','expired')),
        result_object_id TEXT REFERENCES content_objects(id),
        result_expires_at INTEGER,
        created_at INTEGER NOT NULL,
        completed_at INTEGER,
        expired_at INTEGER,
        PRIMARY KEY(scope, idempotency_key)
    )"#,
    "CREATE INDEX graph_revisions_graph_revision ON graph_revisions(graph_id, revision_no DESC)",
];

const DOWN: &[&str] = &[
    "DROP TABLE application_command_receipts",
    "DROP TABLE content_object_refs",
    "DROP TABLE graph_revisions",
    "DROP TABLE graph_drafts",
    "DROP TABLE graphs",
    "DROP TABLE content_objects",
];
