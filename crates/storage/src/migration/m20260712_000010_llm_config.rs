use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260712_000010_llm_config"
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
    r#"CREATE TABLE llm_channels (
        id TEXT PRIMARY KEY NOT NULL,
        name TEXT NOT NULL,
        head_revision_id TEXT REFERENCES llm_channel_revisions(id),
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    )"#,
    r#"CREATE TABLE llm_channel_revisions (
        id TEXT PRIMARY KEY NOT NULL,
        channel_id TEXT NOT NULL REFERENCES llm_channels(id),
        revision_no INTEGER NOT NULL CHECK (revision_no > 0),
        operation_taxonomy_version INTEGER NOT NULL CHECK (operation_taxonomy_version > 0),
        adapter_decoder_version INTEGER NOT NULL CHECK (adapter_decoder_version > 0),
        base_url TEXT NOT NULL,
        transport_policy_json TEXT NOT NULL CHECK (json_valid(transport_policy_json)),
        credential_kind TEXT NOT NULL CHECK (credential_kind IN ('secret','none')),
        api_key_ref TEXT CHECK (api_key_ref IS NULL OR json_valid(api_key_ref)),
        operation_keys_json TEXT NOT NULL CHECK (json_valid(operation_keys_json)),
        model_catalogs_json TEXT NOT NULL CHECK (json_valid(model_catalogs_json)),
        capabilities_json TEXT NOT NULL CHECK (json_valid(capabilities_json)),
        content_hash TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        UNIQUE(channel_id, revision_no),
        UNIQUE(channel_id, content_hash),
        CHECK ((credential_kind = 'secret' AND api_key_ref IS NOT NULL)
            OR (credential_kind = 'none' AND api_key_ref IS NULL))
    )"#,
    "CREATE INDEX llm_channel_revisions_channel ON llm_channel_revisions(channel_id, revision_no DESC)",
    r#"CREATE TABLE context_presets (
        id TEXT PRIMARY KEY NOT NULL,
        name TEXT NOT NULL,
        head_version_id TEXT REFERENCES context_preset_versions(id),
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    )"#,
    r#"CREATE TABLE context_preset_versions (
        id TEXT PRIMARY KEY NOT NULL,
        preset_id TEXT NOT NULL REFERENCES context_presets(id),
        version_no INTEGER NOT NULL CHECK (version_no > 0),
        semantic_policy_version INTEGER NOT NULL CHECK (semantic_policy_version > 0),
        spec_json TEXT NOT NULL CHECK (json_valid(spec_json)),
        content_hash TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        UNIQUE(preset_id, version_no),
        UNIQUE(preset_id, content_hash)
    )"#,
    "CREATE INDEX context_preset_versions_preset ON context_preset_versions(preset_id, version_no DESC)",
];

const DOWN: &[&str] = &[
    "UPDATE context_presets SET head_version_id = NULL",
    "DELETE FROM context_preset_versions",
    "DROP TABLE context_preset_versions",
    "DROP TABLE context_presets",
    "UPDATE llm_channels SET head_revision_id = NULL",
    "DELETE FROM llm_channel_revisions",
    "DROP TABLE llm_channel_revisions",
    "DROP TABLE llm_channels",
];
