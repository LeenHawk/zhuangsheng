use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    application::{
        channel::{CreateChannelCommand, PublishChannelRevisionCommand},
        preset::{CreateContextPresetCommand, PublishContextPresetVersionCommand},
    },
    llm::{
        ChannelCredential, ChannelModel, ChannelModelCatalog, ChannelTransportPolicy,
        ContentGenerationKind, LlmChannelRevisionSpec, ModelCapabilities, ModelCatalogPolicy,
        Operation, OperationKey,
        context::{
            ContextAssemblyMode, ContextAssemblySpec, ContextBudgetPolicy, ContextBudgetStrategy,
            ContextItem, ContextPosition, ContextRole, ContextSource, TokenBudgetHint,
        },
    },
};

use crate::{StorageError, graph::helpers::sql, tests::store};

fn operation() -> OperationKey {
    OperationKey::content_generation(
        Operation::GenerateContent,
        ContentGenerationKind::OpenAiResponses,
    )
}

fn channel_spec() -> LlmChannelRevisionSpec {
    LlmChannelRevisionSpec {
        operation_taxonomy_version: 1,
        adapter_decoder_version: 1,
        base_url: "https://llm.example.test/v1/".into(),
        transport_policy: ChannelTransportPolicy {
            allow_loopback_http: false,
            allow_unauthenticated: true,
        },
        credential: ChannelCredential::None,
        operation_keys: vec![operation()],
        model_catalogs: vec![ChannelModelCatalog {
            operation_key: operation(),
            policy: ModelCatalogPolicy::Allowlist,
            models: vec![ChannelModel {
                id: "model-1".into(),
                name: Some("Model One".into()),
                context_window: Some(128_000),
                max_output_tokens: Some(8_192),
                capabilities: ModelCapabilities {
                    streaming: Some(true),
                    ..Default::default()
                },
            }],
        }],
        capabilities: vec![],
    }
}

fn preset_spec() -> ContextAssemblySpec {
    ContextAssemblySpec {
        id: Some("roleplay-v1".into()),
        name: Some("Role Play".into()),
        mode: ContextAssemblyMode::Chat,
        items: vec![ContextItem {
            id: "character".into(),
            name: Some("Character".into()),
            enabled: true,
            requested_role: ContextRole::System,
            source: ContextSource::Literal {
                text: "You are Alice.".into(),
            },
            position: ContextPosition::Start,
            order: 0,
            priority: 100,
            insertion_depth: 0,
            budget: TokenBudgetHint {
                max_tokens: None,
                required: true,
            },
            overflow: None,
        }],
        budget: Some(ContextBudgetPolicy {
            max_input_tokens: Some(16_000),
            strategy: Some(ContextBudgetStrategy::Strict),
        }),
        post_process: vec![],
        preview: None,
    }
}

#[tokio::test]
async fn channel_create_publish_and_reader_are_versioned_and_idempotent() {
    let store = store().await;
    let created = store
        .create_channel(CreateChannelCommand {
            name: "Primary".into(),
            idempotency_key: "channel-create".into(),
        })
        .await
        .unwrap();
    assert!(created.head_revision_id.is_none());
    assert_eq!(
        store
            .create_channel(CreateChannelCommand {
                name: "Primary".into(),
                idempotency_key: "channel-create".into(),
            })
            .await
            .unwrap()
            .id,
        created.id
    );
    let published = store
        .publish_channel_revision(PublishChannelRevisionCommand {
            channel_id: created.id.clone(),
            expected_head_revision_id: None,
            spec: channel_spec(),
            idempotency_key: "channel-publish".into(),
        })
        .await
        .unwrap();
    assert_eq!(published.revision_no, 1);
    assert_eq!(published.spec.base_url, "https://llm.example.test/v1");
    assert_eq!(
        store
            .get_channel_head_revision(&created.id)
            .await
            .unwrap()
            .content_hash,
        published.content_hash
    );
    let stale = store
        .publish_channel_revision(PublishChannelRevisionCommand {
            channel_id: created.id,
            expected_head_revision_id: None,
            spec: channel_spec(),
            idempotency_key: "channel-stale".into(),
        })
        .await;
    assert!(matches!(
        stale,
        Err(StorageError::Conflict("channel_head_conflict"))
    ));

    store
        .db
        .execute_raw(sql(
            "UPDATE llm_channel_revisions SET operation_taxonomy_version = 999 WHERE id = ?",
            vec![published.id.clone().into()],
        ))
        .await
        .unwrap();
    assert!(matches!(
        store.get_channel_revision(&published.id).await,
        Err(StorageError::Integrity(_))
    ));
}

#[tokio::test]
async fn context_preset_publish_materializes_canonical_defaults() {
    let store = store().await;
    let preset = store
        .create_context_preset(CreateContextPresetCommand {
            name: "Role Play".into(),
            idempotency_key: "preset-create".into(),
        })
        .await
        .unwrap();
    let version = store
        .publish_context_preset_version(PublishContextPresetVersionCommand {
            preset_id: preset.id.clone(),
            expected_head_version_id: None,
            spec: preset_spec(),
            idempotency_key: "preset-publish".into(),
        })
        .await
        .unwrap();
    assert_eq!(version.version_no, 1);
    assert!(version.spec.preview.is_some());
    assert_eq!(
        store
            .get_context_preset_head(&preset.id)
            .await
            .unwrap()
            .content_hash,
        version.content_hash
    );
    let replay = store
        .publish_context_preset_version(PublishContextPresetVersionCommand {
            preset_id: preset.id,
            expected_head_version_id: None,
            spec: preset_spec(),
            idempotency_key: "preset-publish".into(),
        })
        .await
        .unwrap();
    assert_eq!(replay.id, version.id);
    assert_eq!(
        serde_json::to_value(replay).unwrap()["spec"]["mode"],
        json!("chat")
    );
}
