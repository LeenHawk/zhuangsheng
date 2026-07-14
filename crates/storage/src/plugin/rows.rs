use sea_orm::{ConnectionTrait, QueryResult};
use zhuangsheng_core::{
    application::plugin::{
        PluginCandidateView, PluginInstallationView, PluginManifest, PluginPermission,
        PluginUpdatePolicy, PluginVersionView, normalize_plugin_manifest,
    },
    canonical,
};

use crate::{StorageError, StorageResult, graph::helpers::sql};

pub(super) async fn load_candidate<C: ConnectionTrait>(
    db: &C,
    id: &str,
) -> StorageResult<PluginCandidateView> {
    let row = db
        .query_one_raw(sql(
            "SELECT * FROM plugin_candidates WHERE id = ?",
            vec![id.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::NotFound {
            kind: "plugin_candidate",
            id: id.into(),
        })?;
    candidate_from_row(&row)
}

pub(super) async fn load_installation<C: ConnectionTrait>(
    db: &C,
    plugin_id: &str,
) -> StorageResult<PluginInstallationView> {
    let row = db
        .query_one_raw(sql(
            "SELECT * FROM plugin_installations WHERE plugin_id = ?",
            vec![plugin_id.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::NotFound {
            kind: "plugin",
            id: plugin_id.into(),
        })?;
    installation_from_row(db, &row).await
}

pub(super) async fn list_installations<C: ConnectionTrait>(
    db: &C,
) -> StorageResult<Vec<PluginInstallationView>> {
    let rows = db
        .query_all_raw(sql(
            "SELECT * FROM plugin_installations ORDER BY plugin_id",
            vec![],
        ))
        .await?;
    let mut result = Vec::with_capacity(rows.len());
    for row in rows {
        result.push(installation_from_row(db, &row).await?);
    }
    Ok(result)
}

pub(super) async fn load_version<C: ConnectionTrait>(
    db: &C,
    version_id: &str,
) -> StorageResult<PluginVersionView> {
    let row = db
        .query_one_raw(sql(
            "SELECT * FROM plugin_versions WHERE id = ?",
            vec![version_id.into()],
        ))
        .await?
        .ok_or_else(|| StorageError::NotFound {
            kind: "plugin_version",
            id: version_id.into(),
        })?;
    version_from_row(&row)
}

fn candidate_from_row(row: &QueryResult) -> StorageResult<PluginCandidateView> {
    let manifest = manifest(row, "manifest_json", "manifest_hash")?;
    let plugin_id: String = row.try_get("", "plugin_id")?;
    if manifest.id != plugin_id {
        return Err(StorageError::Integrity(
            "plugin candidate identity mismatch".into(),
        ));
    }
    let mut added_permissions: Vec<PluginPermission> = serde_json::from_str(
        &row.try_get::<String>("", "added_permissions_json")?,
    )
    .map_err(|_| StorageError::Integrity("plugin candidate permissions are invalid".into()))?;
    added_permissions.sort();
    added_permissions.dedup();
    Ok(PluginCandidateView {
        id: row.try_get("", "id")?,
        planned_version_id: row.try_get("", "planned_version_id")?,
        source_url: row.try_get("", "source_url")?,
        source_ref: row.try_get("", "source_ref")?,
        credential_secret_id: row.try_get("", "credential_secret_id")?,
        credential_username: row.try_get("", "credential_username")?,
        resolved_commit: row.try_get("", "resolved_commit")?,
        tree_hash: row.try_get("", "tree_hash")?,
        manifest_hash: canonical::hash(&manifest)?,
        manifest,
        current_version_id: row.try_get("", "current_version_id")?,
        added_permissions,
        created_at: row.try_get("", "created_at")?,
    })
}

async fn installation_from_row<C: ConnectionTrait>(
    db: &C,
    row: &QueryResult,
) -> StorageResult<PluginInstallationView> {
    let plugin_id: String = row.try_get("", "plugin_id")?;
    let active_id: String = row.try_get("", "active_version_id")?;
    let active_version = load_version(db, &active_id).await?;
    if active_version.plugin_id != plugin_id {
        return Err(StorageError::Integrity(
            "active plugin version mismatch".into(),
        ));
    }
    let rows = db.query_all_raw(sql(
        "SELECT * FROM plugin_versions WHERE plugin_id = ? AND id <> ? ORDER BY installed_at DESC",
        vec![plugin_id.clone().into(), active_id.into()],
    )).await?;
    let previous_versions = rows
        .iter()
        .map(version_from_row)
        .collect::<StorageResult<_>>()?;
    Ok(PluginInstallationView {
        plugin_id,
        source_url: row.try_get("", "source_url")?,
        source_ref: row.try_get("", "source_ref")?,
        credential_secret_id: row.try_get("", "credential_secret_id")?,
        credential_username: row.try_get("", "credential_username")?,
        update_policy: policy(&row.try_get::<String>("", "update_policy")?)?,
        enabled: row.try_get::<i64>("", "enabled")? != 0,
        active_version,
        previous_versions,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

fn version_from_row(row: &QueryResult) -> StorageResult<PluginVersionView> {
    let manifest = manifest(row, "manifest_json", "manifest_hash")?;
    let plugin_id: String = row.try_get("", "plugin_id")?;
    if manifest.id != plugin_id {
        return Err(StorageError::Integrity(
            "plugin version identity mismatch".into(),
        ));
    }
    Ok(PluginVersionView {
        id: row.try_get("", "id")?,
        plugin_id,
        version: row.try_get("", "version")?,
        resolved_commit: row.try_get("", "resolved_commit")?,
        tree_hash: row.try_get("", "tree_hash")?,
        manifest_hash: canonical::hash(&manifest)?,
        manifest,
        installed_at: row.try_get("", "installed_at")?,
    })
}

fn manifest(row: &QueryResult, json_key: &str, hash_key: &str) -> StorageResult<PluginManifest> {
    let encoded: String = row.try_get("", json_key)?;
    let decoded: PluginManifest = serde_json::from_str(&encoded)
        .map_err(|_| StorageError::Integrity("plugin manifest JSON is invalid".into()))?;
    let normalized = normalize_plugin_manifest(decoded)
        .map_err(|_| StorageError::Integrity("plugin manifest is invalid".into()))?;
    if canonical::to_string(&normalized)? != encoded
        || canonical::hash(&normalized)? != row.try_get::<String>("", hash_key)?
    {
        return Err(StorageError::Integrity(
            "plugin manifest hash mismatch".into(),
        ));
    }
    Ok(normalized)
}

pub(super) fn policy_name(value: PluginUpdatePolicy) -> &'static str {
    match value {
        PluginUpdatePolicy::Manual => "manual",
        PluginUpdatePolicy::Notify => "notify",
        PluginUpdatePolicy::Automatic => "automatic",
    }
}

fn policy(value: &str) -> StorageResult<PluginUpdatePolicy> {
    match value {
        "manual" => Ok(PluginUpdatePolicy::Manual),
        "notify" => Ok(PluginUpdatePolicy::Notify),
        "automatic" => Ok(PluginUpdatePolicy::Automatic),
        _ => Err(StorageError::Integrity(
            "plugin update policy is invalid".into(),
        )),
    }
}
