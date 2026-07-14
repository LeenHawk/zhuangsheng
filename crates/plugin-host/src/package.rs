use std::{
    fs,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};
use zhuangsheng_core::{
    application::{ApplicationError, plugin::*},
    canonical,
};

use crate::source::invalid;

pub(super) struct InspectedPackage {
    pub manifest: PluginManifest,
    pub manifest_hash: String,
    pub tree_hash: String,
}

pub(super) fn inspect_package(root: &Path) -> Result<InspectedPackage, ApplicationError> {
    let files = collect_files(root)?;
    let manifest_path = root.join("manifest.json");
    let manifest_bytes = fs::read(&manifest_path).map_err(|_| {
        invalid(
            "plugin_manifest_missing",
            "plugin repository must contain manifest.json",
        )
    })?;
    if manifest_bytes.len() > 256 * 1024 {
        return Err(invalid(
            "plugin_manifest_size",
            "plugin manifest is too large",
        ));
    }
    let text = std::str::from_utf8(&manifest_bytes)
        .map_err(|_| invalid("plugin_manifest_json", "plugin manifest must be UTF-8 JSON"))?;
    let value = canonical::parse(text)
        .map_err(|_| invalid("plugin_manifest_json", "plugin manifest JSON is invalid"))?;
    let manifest: PluginManifest = serde_json::from_value(value).map_err(|_| {
        invalid(
            "plugin_manifest_shape",
            "plugin manifest does not match API version 1",
        )
    })?;
    let manifest = normalize_plugin_manifest(manifest)?;
    let entrypoint = root.join(&manifest.entrypoints.ui_worker);
    let metadata = fs::metadata(&entrypoint).map_err(|_| {
        invalid(
            "plugin_entrypoint_missing",
            "plugin UI worker entrypoint is missing",
        )
    })?;
    if !metadata.is_file() || metadata.len() > MAX_PLUGIN_ENTRYPOINT_BYTES {
        return Err(invalid(
            "plugin_entrypoint_size",
            "plugin UI worker is too large",
        ));
    }
    let tree_hash = hash_files(root, &files)?;
    Ok(InspectedPackage {
        manifest_hash: canonical::hash(&manifest).map_err(|_| ApplicationError::Internal)?,
        manifest,
        tree_hash,
    })
}

fn collect_files(root: &Path) -> Result<Vec<PathBuf>, ApplicationError> {
    let mut pending = vec![root.to_owned()];
    let mut files = Vec::new();
    let mut total = 0u64;
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).map_err(|_| ApplicationError::Internal)? {
            let entry = entry.map_err(|_| ApplicationError::Internal)?;
            let file_type = entry.file_type().map_err(|_| ApplicationError::Internal)?;
            if file_type.is_symlink() {
                return Err(invalid(
                    "plugin_symlink",
                    "plugin packages cannot contain symlinks",
                ));
            }
            if file_type.is_dir() {
                pending.push(entry.path());
                continue;
            }
            if !file_type.is_file() {
                return Err(invalid(
                    "plugin_file",
                    "plugin contains an unsupported file type",
                ));
            }
            let size = entry
                .metadata()
                .map_err(|_| ApplicationError::Internal)?
                .len();
            if size > MAX_PLUGIN_FILE_BYTES {
                return Err(invalid("plugin_file_size", "plugin file is too large"));
            }
            total = total.checked_add(size).ok_or(ApplicationError::Internal)?;
            files.push(entry.path());
            if files.len() > MAX_PLUGIN_FILES || total > MAX_PLUGIN_BYTES {
                return Err(invalid(
                    "plugin_package_size",
                    "plugin package exceeds safety limits",
                ));
            }
        }
    }
    files.sort();
    Ok(files)
}

fn hash_files(root: &Path, files: &[PathBuf]) -> Result<String, ApplicationError> {
    let mut hash = Sha256::new();
    for path in files {
        let relative = path
            .strip_prefix(root)
            .map_err(|_| ApplicationError::Internal)?;
        let relative = relative
            .to_str()
            .ok_or_else(|| invalid("plugin_path_encoding", "plugin paths must be UTF-8"))?
            .replace('\\', "/");
        let bytes = fs::read(path).map_err(|_| ApplicationError::Internal)?;
        hash.update((relative.len() as u64).to_be_bytes());
        hash.update(relative.as_bytes());
        hash.update((bytes.len() as u64).to_be_bytes());
        hash.update(&bytes);
    }
    Ok(format!("sha256:{}", hex(&hash.finalize())))
}

fn hex(bytes: &[u8]) -> String {
    const VALUES: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(VALUES[(byte >> 4) as usize] as char);
        output.push(VALUES[(byte & 15) as usize] as char);
    }
    output
}
