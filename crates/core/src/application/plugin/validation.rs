use std::collections::BTreeSet;

use super::{PluginManifest, PluginPermission};
use crate::application::ApplicationError;

pub const PLUGIN_API_VERSION: u32 = 1;
pub const MAX_PLUGIN_FILES: usize = 2_000;
pub const MAX_PLUGIN_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_PLUGIN_FILE_BYTES: u64 = 2 * 1024 * 1024;
pub const MAX_PLUGIN_ENTRYPOINT_BYTES: u64 = 1024 * 1024;

pub fn normalize_plugin_manifest(
    mut manifest: PluginManifest,
) -> Result<PluginManifest, ApplicationError> {
    if manifest.api_version != PLUGIN_API_VERSION {
        return invalid("plugin_api_version", "plugin API version is unsupported");
    }
    manifest.id = manifest.id.trim().to_ascii_lowercase();
    manifest.name = manifest.name.trim().to_owned();
    manifest.version = manifest.version.trim().to_owned();
    if !valid_id(&manifest.id) || manifest.name.is_empty() || manifest.name.len() > 200 {
        return invalid("plugin_manifest", "plugin identity is invalid");
    }
    if manifest.version.is_empty() || manifest.version.len() > 64 {
        return invalid("plugin_manifest", "plugin version is invalid");
    }
    validate_asset_path(&manifest.entrypoints.ui_worker)?;
    if !manifest.entrypoints.ui_worker.ends_with(".js")
        && !manifest.entrypoints.ui_worker.ends_with(".mjs")
    {
        return invalid(
            "plugin_entrypoint",
            "UI worker must be a bundled JavaScript module",
        );
    }
    manifest.permissions.sort();
    manifest.permissions.dedup();
    let mut renderer_ids = BTreeSet::new();
    for renderer in &mut manifest.renderers {
        renderer.id = renderer.id.trim().to_owned();
        if !valid_id(&renderer.id)
            || !renderer_ids.insert(renderer.id.clone())
            || !(-1_000..=1_000).contains(&renderer.priority)
        {
            return invalid("plugin_renderer", "renderer declaration is invalid");
        }
        renderer.roles.sort();
        renderer.roles.dedup();
    }
    if !manifest.renderers.is_empty()
        && (!manifest
            .permissions
            .contains(&PluginPermission::UiMessageReadDisplay)
            || !manifest
                .permissions
                .contains(&PluginPermission::UiMessageDecorate))
    {
        return invalid(
            "plugin_permission",
            "message renderers require read-display and decorate permissions",
        );
    }
    manifest.dependencies = manifest
        .dependencies
        .into_iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .collect();
    manifest.dependencies.sort();
    manifest.dependencies.dedup();
    if manifest
        .dependencies
        .iter()
        .any(|value| !valid_id(value) || value == &manifest.id)
    {
        return invalid("plugin_dependency", "plugin dependency is invalid");
    }
    Ok(manifest)
}

pub fn validate_asset_path(value: &str) -> Result<(), ApplicationError> {
    let path = std::path::Path::new(value);
    if value.is_empty()
        || value.len() > 240
        || path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return invalid("plugin_asset_path", "plugin asset path is unsafe");
    }
    Ok(())
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || (index > 0 && matches!(byte, b'.' | b'-' | b'_'))
        })
}

fn invalid<T>(code: &'static str, message: &'static str) -> Result<T, ApplicationError> {
    Err(ApplicationError::InvalidArgument {
        code,
        message: message.into(),
    })
}
