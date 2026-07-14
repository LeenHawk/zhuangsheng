use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260714_000032_plugins"
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
    r#"CREATE TABLE plugin_candidates (
        id TEXT PRIMARY KEY,
        planned_version_id TEXT NOT NULL UNIQUE,
        plugin_id TEXT NOT NULL,
        source_url TEXT NOT NULL,
        source_ref TEXT,
        credential_secret_id TEXT,
        credential_username TEXT,
        resolved_commit TEXT NOT NULL,
        tree_hash TEXT NOT NULL,
        manifest_hash TEXT NOT NULL,
        manifest_json TEXT NOT NULL,
        current_version_id TEXT,
        added_permissions_json TEXT NOT NULL,
        status TEXT NOT NULL CHECK(status IN ('staged','activated')),
        activated_version_id TEXT,
        created_at INTEGER NOT NULL
    )"#,
    r#"CREATE TABLE plugin_installations (
        plugin_id TEXT PRIMARY KEY,
        source_url TEXT NOT NULL,
        source_ref TEXT,
        credential_secret_id TEXT,
        credential_username TEXT,
        update_policy TEXT NOT NULL CHECK(update_policy IN ('manual','notify','automatic')),
        enabled INTEGER NOT NULL CHECK(enabled IN (0,1)),
        active_version_id TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    )"#,
    r#"CREATE TABLE plugin_versions (
        id TEXT PRIMARY KEY,
        plugin_id TEXT NOT NULL REFERENCES plugin_installations(plugin_id) ON DELETE CASCADE,
        version TEXT NOT NULL,
        resolved_commit TEXT NOT NULL,
        tree_hash TEXT NOT NULL,
        manifest_hash TEXT NOT NULL,
        manifest_json TEXT NOT NULL,
        installed_at INTEGER NOT NULL,
        UNIQUE(plugin_id, resolved_commit)
    )"#,
    "CREATE INDEX plugin_versions_history ON plugin_versions(plugin_id, installed_at DESC)",
    "CREATE INDEX plugin_candidates_plugin ON plugin_candidates(plugin_id, created_at DESC)",
];

const DOWN: &[&str] = &[
    "DROP INDEX plugin_candidates_plugin",
    "DROP INDEX plugin_versions_history",
    "DROP TABLE plugin_versions",
    "DROP TABLE plugin_installations",
    "DROP TABLE plugin_candidates",
];
