use std::time::{SystemTime, UNIX_EPOCH};

use tokio::fs;
use ulid::Ulid;
use zeroize::Zeroizing;
use zhuangsheng_core::{
    application::{ApplicationError, plugin::*},
    llm::{SecretRef, SecretScheme},
};

use crate::{
    git::{GitCheckout, checkout},
    manager::GitPluginManager,
    package::inspect_package,
    source::{NormalizedSource, candidate_root, invalid, normalize_source},
};

pub(super) enum StageResult {
    Candidate(PluginCandidateView),
    UpToDate,
}

pub(super) async fn stage_source(
    manager: &GitPluginManager,
    command: InspectGitPluginCommand,
) -> Result<StageResult, ApplicationError> {
    let source = normalize_source(command)?;
    let candidate_id = format!("plugincand_{}", Ulid::new());
    let planned_version_id = format!("pluginver_{}", Ulid::new());
    let candidate_dir = candidate_root(&manager.root, &candidate_id);
    let work = candidate_dir.join("work");
    let credential = credential(manager, &source).await?;
    let git_home = manager.root.join("git-home");
    let checkout_result = checkout(GitCheckout {
        source_url: &source.git_url,
        source_ref: source.source_ref.as_deref(),
        destination: &work,
        isolated_home: &git_home,
        credential,
    })
    .await;
    let commit = match checkout_result {
        Ok(value) => value,
        Err(error) => {
            let _ = fs::remove_dir_all(&candidate_dir).await;
            return Err(error);
        }
    };
    fs::remove_dir_all(work.join(".git"))
        .await
        .map_err(|_| ApplicationError::Internal)?;
    let inspect_root = work.clone();
    let inspected = match tokio::task::spawn_blocking(move || inspect_package(&inspect_root)).await
    {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            let _ = fs::remove_dir_all(&candidate_dir).await;
            return Err(error);
        }
        Err(_) => {
            let _ = fs::remove_dir_all(&candidate_dir).await;
            return Err(ApplicationError::Internal);
        }
    };
    let current = match manager
        .registry
        .get_plugin_installation(&inspected.manifest.id)
        .await
    {
        Ok(value) => Some(value),
        Err(ApplicationError::NotFound { .. }) => None,
        Err(error) => {
            let _ = fs::remove_dir_all(&candidate_dir).await;
            return Err(error);
        }
    };
    if current.as_ref().is_some_and(|value| {
        value.active_version.resolved_commit == commit
            || value
                .previous_versions
                .iter()
                .any(|version| version.resolved_commit == commit)
    }) {
        let _ = fs::remove_dir_all(candidate_dir).await;
        return Ok(StageResult::UpToDate);
    }
    let current_permissions = current
        .as_ref()
        .map(|value| value.active_version.manifest.permissions.as_slice())
        .unwrap_or_default();
    let added_permissions = inspected
        .manifest
        .permissions
        .iter()
        .copied()
        .filter(|permission| !current_permissions.contains(permission))
        .collect();
    let candidate = PluginCandidateView {
        id: candidate_id.clone(),
        planned_version_id,
        source_url: source.url,
        source_ref: source.source_ref,
        credential_secret_id: source.credential_secret_id,
        credential_username: source.credential_username,
        resolved_commit: commit,
        tree_hash: inspected.tree_hash,
        manifest_hash: inspected.manifest_hash,
        manifest: inspected.manifest,
        current_version_id: current.map(|value| value.active_version.id),
        added_permissions,
        created_at: now_ms(),
    };
    match manager
        .registry
        .register_plugin_candidate(RegisterPluginCandidateCommand { candidate })
        .await
    {
        Ok(value) => {
            if value.id != candidate_id {
                let _ = fs::remove_dir_all(candidate_dir).await;
            }
            Ok(StageResult::Candidate(value))
        }
        Err(error) => {
            let _ = fs::remove_dir_all(candidate_dir).await;
            Err(error)
        }
    }
}

async fn credential(
    manager: &GitPluginManager,
    source: &NormalizedSource,
) -> Result<Option<Zeroizing<String>>, ApplicationError> {
    let Some(id) = source.credential_secret_id.as_ref() else {
        return Ok(None);
    };
    let value = manager
        .secrets
        .resolve_secret(&SecretRef {
            scheme: SecretScheme::Secret,
            id: id.clone(),
        })
        .await?;
    value
        .with_bytes(|bytes| std::str::from_utf8(bytes).map(|text| Zeroizing::new(text.to_owned())))
        .map(Some)
        .map_err(|_| invalid("plugin_git_credential", "Git credential must be UTF-8"))
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |value| {
            i64::try_from(value.as_millis()).unwrap_or(i64::MAX)
        })
}
