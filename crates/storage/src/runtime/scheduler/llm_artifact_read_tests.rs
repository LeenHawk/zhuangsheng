use std::collections::BTreeMap;

use serde_json::json;
use zhuangsheng_core::{
    application::{
        artifact::{
            CommitArtifactStagingCommand, CompleteArtifactStagingCommand,
            CreateArtifactStagingCommand,
        },
        conversation::CreateConversationCommand,
    },
    artifact::{ArtifactClassification, ArtifactMetadataDraft, ArtifactRetention},
    graph::{
        InputSelector, MemoryReadConsistency, PreExecutionValueSelector, PreExecutionValueSource,
        StaticMemoryRead, StaticMemoryReadSource,
    },
    llm::{
        context::ResolvedContextValue,
        ir::{ContextSensitivity, LlmContentPartIr},
    },
};

use crate::StorageError;

use super::{llm_artifact_binding::artifact_binding, llm_artifact_read::resolve};

#[tokio::test]
async fn artifact_read_requires_exact_ref_and_context_owner() {
    let store = crate::SqliteStore::connect("sqlite::memory:")
        .await
        .unwrap();
    let conversation = store
        .create_conversation_at(
            CreateConversationCommand {
                title: None,
                default_run: None,
                idempotency_key: "artifact-read-context".into(),
            },
            1_700_000_000_000,
        )
        .await
        .unwrap();
    let staging = store
        .create_artifact_staging_at(
            CreateArtifactStagingCommand {
                context_id: Some(conversation.context_id.clone()),
                node_attempt_id: None,
                tool_call_id: None,
                metadata_draft: ArtifactMetadataDraft {
                    name: Some("lore.txt".into()),
                    classification: ArtifactClassification::Private,
                    retention: ArtifactRetention::Context,
                },
                declared_media_type: Some("text/plain".into()),
            },
            1_700_000_000_001,
        )
        .await
        .unwrap();
    store
        .complete_artifact_staging_at(
            CompleteArtifactStagingCommand {
                staging_id: staging.staging_id.clone(),
                expected_lifecycle_generation: 0,
                bytes: b"moonlit archive".to_vec(),
            },
            1_700_000_000_002,
        )
        .await
        .unwrap();
    let artifact = store
        .commit_artifact_staging_at(
            CommitArtifactStagingCommand {
                staging_id: staging.staging_id,
                expected_lifecycle_generation: 2,
                idempotency_key: "artifact-read-commit".into(),
            },
            1_700_000_000_003,
        )
        .await
        .unwrap();
    let selector = InputSelector::JsonPointer {
        pointer: "/artifact".into(),
    };
    let read = StaticMemoryRead {
        id: "document".into(),
        alias: "document".into(),
        source: StaticMemoryReadSource::Artifact {
            scope: "run-context".into(),
            artifact_ref_from: PreExecutionValueSelector {
                source: PreExecutionValueSource::Input,
                source_name: "default".into(),
                selector: selector.clone(),
            },
        },
        required: true,
        consistency: MemoryReadConsistency::Snapshot,
        limit: None,
        max_bytes: 1024,
    };
    let inputs = BTreeMap::from([(
        "default".into(),
        json!({"artifact":artifact.metadata.content}),
    )]);
    let resolved = resolve(
        &store.db,
        &read,
        "default",
        &selector,
        &inputs,
        &conversation.context_id,
    )
    .await
    .unwrap();
    assert_eq!(resolved.envelope["text"], "moonlit archive");
    assert_eq!(resolved.selections[0].aggregate_kind, "artifact_metadata");
    let binding = artifact_binding(&read, "run-context", resolved.envelope).unwrap();
    assert_eq!(binding.binding_id, "document");
    assert_eq!(binding.values.len(), 2);
    let ResolvedContextValue::Data {
        content,
        provenance,
        tags,
        ..
    } = &binding.values[1]
    else {
        panic!("expected artifact text value")
    };
    assert_eq!(
        content,
        &[LlmContentPartIr::Text {
            text: "moonlit archive".into()
        }]
    );
    assert_eq!(provenance.sensitivity, ContextSensitivity::Private);
    assert_eq!(tags, &["artifact_view:text"]);
    assert!(matches!(
        resolve(
            &store.db,
            &read,
            "default",
            &selector,
            &inputs,
            "another-context",
        )
        .await,
        Err(StorageError::InputContract(_))
    ));
}
