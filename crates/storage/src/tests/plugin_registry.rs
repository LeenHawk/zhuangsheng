use zhuangsheng_core::{
    application::plugin::{
        ActivatePluginCandidateCommand, PluginCandidateView, PluginEntrypoints, PluginManifest,
        PluginPermission, PluginRendererDeclaration, PluginRendererSlot, PluginUpdatePolicy,
        RegisterPluginCandidateCommand, RollbackPluginCommand,
    },
    canonical,
};

use crate::{StorageError, tests::store};

fn candidate(
    id: &str,
    version_id: &str,
    version: &str,
    commit_digit: char,
    current_version_id: Option<&str>,
) -> PluginCandidateView {
    let permissions = vec![
        PluginPermission::UiMessageReadDisplay,
        PluginPermission::UiMessageDecorate,
    ];
    let manifest = PluginManifest {
        api_version: 1,
        id: "example.story-renderer".into(),
        name: "Story Renderer".into(),
        version: version.into(),
        description: None,
        minimum_host_version: None,
        entrypoints: PluginEntrypoints {
            ui_worker: "dist/plugin.js".into(),
        },
        permissions: permissions.clone(),
        renderers: vec![PluginRendererDeclaration {
            id: "story-message".into(),
            slot: PluginRendererSlot::ConversationMessageBody,
            priority: 10,
            roles: vec![],
        }],
        dependencies: vec![],
        settings_schema: None,
    };
    PluginCandidateView {
        id: id.into(),
        planned_version_id: version_id.into(),
        source_url: "https://example.test/story-renderer.git".into(),
        source_ref: Some("main".into()),
        credential_secret_id: None,
        credential_username: None,
        resolved_commit: std::iter::repeat_n(commit_digit, 40).collect(),
        tree_hash: format!("sha256:tree-{version}"),
        manifest_hash: canonical::hash(&manifest).unwrap(),
        manifest,
        current_version_id: current_version_id.map(str::to_owned),
        added_permissions: permissions,
        created_at: 1,
    }
}

fn activate(
    candidate_id: &str,
    expected: Option<&str>,
    key: &str,
) -> ActivatePluginCandidateCommand {
    ActivatePluginCandidateCommand {
        candidate_id: candidate_id.into(),
        expected_active_version_id: expected.map(str::to_owned),
        approved_permissions: vec![
            PluginPermission::UiMessageReadDisplay,
            PluginPermission::UiMessageDecorate,
        ],
        update_policy: PluginUpdatePolicy::Notify,
        idempotency_key: key.into(),
    }
}

#[tokio::test]
async fn plugin_activation_requires_exact_permissions() {
    let store = store().await;
    let candidate = candidate("candidate-1", "version-1", "1.0.0", '1', None);
    store
        .register_plugin_candidate(RegisterPluginCandidateCommand {
            candidate: candidate.clone(),
        })
        .await
        .unwrap();
    assert_eq!(
        store
            .register_plugin_candidate(RegisterPluginCandidateCommand { candidate })
            .await
            .unwrap()
            .id,
        "candidate-1"
    );

    let mut command = activate("candidate-1", None, "activate-without-permission");
    command.approved_permissions.pop();
    assert!(matches!(
        store.activate_plugin_candidate(command).await,
        Err(StorageError::InvalidArgument(_))
    ));
}

#[tokio::test]
async fn plugin_update_uses_cas_keeps_history_and_rolls_back() {
    let store = store().await;
    let first = candidate("candidate-1", "version-1", "1.0.0", '1', None);
    store
        .register_plugin_candidate(RegisterPluginCandidateCommand { candidate: first })
        .await
        .unwrap();
    let installed = store
        .activate_plugin_candidate(activate("candidate-1", None, "activate-1"))
        .await
        .unwrap();
    assert_eq!(installed.active_version.id, "version-1");

    let second = candidate("candidate-2", "version-2", "1.1.0", '2', Some("version-1"));
    store
        .register_plugin_candidate(RegisterPluginCandidateCommand { candidate: second })
        .await
        .unwrap();
    assert!(matches!(
        store
            .activate_plugin_candidate(activate("candidate-2", None, "activate-stale"))
            .await,
        Err(StorageError::Conflict("plugin_active_version"))
    ));

    let updated = store
        .activate_plugin_candidate(activate("candidate-2", Some("version-1"), "activate-2"))
        .await
        .unwrap();
    assert_eq!(updated.active_version.id, "version-2");
    assert_eq!(updated.previous_versions[0].id, "version-1");

    let rolled_back = store
        .rollback_plugin_installation(RollbackPluginCommand {
            plugin_id: "example.story-renderer".into(),
            target_version_id: "version-1".into(),
            expected_active_version_id: "version-2".into(),
            idempotency_key: "rollback-1".into(),
        })
        .await
        .unwrap();
    assert_eq!(rolled_back.active_version.id, "version-1");
    assert_eq!(rolled_back.previous_versions[0].id, "version-2");
}
