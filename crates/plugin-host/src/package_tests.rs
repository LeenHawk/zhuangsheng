use std::fs;

use serde_json::json;

use crate::package::inspect_package;

fn write_package(root: &std::path::Path, entrypoint: &str) {
    let manifest = json!({
        "apiVersion": 1,
        "id": "example.story-renderer",
        "name": "Story Renderer",
        "version": "1.0.0",
        "description": null,
        "minimumHostVersion": null,
        "entrypoints": { "uiWorker": entrypoint },
        "permissions": ["ui_message_read_display", "ui_message_decorate"],
        "renderers": [{
            "id": "story-message",
            "slot": "conversation_message_body",
            "priority": 10,
            "roles": []
        }],
        "dependencies": [],
        "settingsSchema": null
    });
    fs::create_dir_all(root.join("dist")).unwrap();
    fs::write(root.join("manifest.json"), manifest.to_string()).unwrap();
}

#[test]
fn package_inspection_hashes_manifest_and_complete_tree() {
    let directory = tempfile::tempdir().unwrap();
    write_package(directory.path(), "dist/plugin.js");
    fs::write(
        directory.path().join("dist/plugin.js"),
        "export function render() { return []; }",
    )
    .unwrap();
    let first = inspect_package(directory.path()).unwrap();
    assert_eq!(first.manifest.id, "example.story-renderer");
    assert!(first.tree_hash.starts_with("sha256:"));

    fs::write(
        directory.path().join("dist/plugin.js"),
        "export function render() { return [{type:'text',text:'ok'}]; }",
    )
    .unwrap();
    let second = inspect_package(directory.path()).unwrap();
    assert_eq!(first.manifest_hash, second.manifest_hash);
    assert_ne!(first.tree_hash, second.tree_hash);
}

#[test]
fn package_rejects_parent_entrypoint_and_oversized_files() {
    let unsafe_directory = tempfile::tempdir().unwrap();
    write_package(unsafe_directory.path(), "../plugin.js");
    assert!(inspect_package(unsafe_directory.path()).is_err());

    let large_directory = tempfile::tempdir().unwrap();
    write_package(large_directory.path(), "dist/plugin.js");
    fs::write(
        large_directory.path().join("dist/plugin.js"),
        vec![
            b'x';
            (zhuangsheng_core::application::plugin::MAX_PLUGIN_ENTRYPOINT_BYTES + 1) as usize
        ],
    )
    .unwrap();
    assert!(inspect_package(large_directory.path()).is_err());
}

#[cfg(unix)]
#[test]
fn package_rejects_symlinks() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    write_package(directory.path(), "dist/plugin.js");
    fs::write(
        directory.path().join("dist/plugin.js"),
        "export const render = () => [];",
    )
    .unwrap();
    symlink(
        directory.path().join("dist/plugin.js"),
        directory.path().join("dist/alias.js"),
    )
    .unwrap();
    assert!(inspect_package(directory.path()).is_err());
}
