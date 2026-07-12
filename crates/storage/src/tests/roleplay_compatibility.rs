use zhuangsheng_core::{
    application::{
        channel::{CreateChannelCommand, PublishChannelRevisionCommand},
        graph::{ApplyGraphCommand, CreateGraphCommand, UpdateGraphDraftCommand},
        preset::{CreateContextPresetCommand, PublishContextPresetVersionCommand},
    },
    conversation::{
        RolePlayCompatibilityView, assistant_reply_payload_v1_schema,
        conversation_run_input_v1_schema,
    },
    graph::GraphDraft,
    llm::context::{
        ContextAssemblyMode, ContextAssemblySpec, ContextItem, ContextPosition, ContextRole,
        ContextSource,
    },
};

use super::{
    llm_graph::{channel_spec, llm_draft},
    store,
};

#[tokio::test]
async fn roleplay_options_project_compatible_and_locked_graphs() {
    let store = store().await;
    let channel = store
        .create_channel(CreateChannelCommand {
            name: "Role Play LLM".into(),
            idempotency_key: "roleplay-channel".into(),
        })
        .await
        .unwrap();
    store
        .publish_channel_revision(PublishChannelRevisionCommand {
            channel_id: channel.id.clone(),
            expected_head_revision_id: None,
            spec: channel_spec(),
            idempotency_key: "roleplay-channel-revision".into(),
        })
        .await
        .unwrap();
    let preset = store
        .create_context_preset(CreateContextPresetCommand {
            name: "Role Play Context".into(),
            idempotency_key: "roleplay-preset".into(),
        })
        .await
        .unwrap();
    let preset_v1 = store
        .publish_context_preset_version(PublishContextPresetVersionCommand {
            preset_id: preset.id.clone(),
            expected_head_version_id: None,
            spec: context_spec("character:main", true),
            idempotency_key: "roleplay-preset-v1".into(),
        })
        .await
        .unwrap();

    let compatible = apply_graph(
        &store,
        "Role Play Graph",
        "roleplay-compatible",
        |graph_id| {
            let mut draft = llm_draft(graph_id, &channel.id, &preset.id);
            draft.run_input_schema = Some(conversation_run_input_v1_schema());
            draft.output_contract[0].schema = Some(assistant_reply_payload_v1_schema());
            draft
        },
    )
    .await;
    let expert = apply_graph(
        &store,
        "Custom Graph",
        "roleplay-expert",
        conversation_only_draft,
    )
    .await;

    let options = store.list_roleplay_graph_options().await.unwrap();
    let option = options
        .iter()
        .find(|option| option.revision_id == compatible)
        .unwrap();
    assert_eq!(option.reply_output_keys, ["reply"]);
    assert_eq!(option.primary_llm_node_id.as_deref(), Some("generate"));
    let RolePlayCompatibilityView::Editable {
        profile_version,
        editable_fields,
    } = &option.compatibility
    else {
        panic!("expected editable graph")
    };
    assert_eq!(*profile_version, 1);
    assert!(
        editable_fields
            .iter()
            .any(|field| field == "context.character")
    );
    assert!(matches!(
        options
            .iter()
            .find(|option| option.revision_id == expert)
            .unwrap()
            .compatibility,
        RolePlayCompatibilityView::ExpertOnly { .. }
    ));

    let mut custom_input = context_spec("custom-directive", false);
    custom_input.items[0].source = ContextSource::Input {
        path: "/other".into(),
    };
    store
        .publish_context_preset_version(PublishContextPresetVersionCommand {
            preset_id: preset.id,
            expected_head_version_id: Some(preset_v1.id),
            spec: custom_input,
            idempotency_key: "roleplay-preset-v2".into(),
        })
        .await
        .unwrap();
    let compatibility = store.get_roleplay_compatibility(&compatible).await.unwrap();
    let RolePlayCompatibilityView::Partial { locked_reasons, .. } = compatibility else {
        panic!("expected partially editable graph")
    };
    assert_eq!(locked_reasons, ["unknown_context_items"]);
}

async fn apply_graph(
    store: &crate::SqliteStore,
    name: &str,
    key: &str,
    draft: impl FnOnce(&str) -> GraphDraft,
) -> String {
    let graph = store
        .create_graph(CreateGraphCommand {
            name: name.into(),
            idempotency_key: format!("{key}-create"),
        })
        .await
        .unwrap();
    let current = store.get_graph_draft(&graph.graph.id).await.unwrap();
    let updated = store
        .update_graph_draft(UpdateGraphDraftCommand {
            graph_id: graph.graph.id.clone(),
            expected_revision_token: current.revision_token,
            document: draft(&graph.graph.id),
            idempotency_key: format!("{key}-draft"),
        })
        .await
        .unwrap();
    store
        .apply_graph(ApplyGraphCommand {
            graph_id: graph.graph.id,
            expected_revision_token: updated.revision_token,
            operation_taxonomy_version: 1,
            adapter_decoder_version: 1,
            idempotency_key: format!("{key}-apply"),
        })
        .await
        .unwrap()
        .id
}

fn conversation_only_draft(graph_id: &str) -> GraphDraft {
    serde_json::from_value(serde_json::json!({
        "graphId": graph_id,
        "nodes": [
            {"id":"input","kind":"input","runInputSelector":{"type":"whole_value"}},
            {"id":"output","kind":"output","outputKey":"reply"}
        ],
        "edges": [{
            "from":{"nodeId":"input","output":"default"},
            "to":{"nodeId":"output","input":"default"}
        }],
        "runInputSchema": conversation_run_input_v1_schema(),
        "outputContract": [{
            "key":"reply",
            "schema":assistant_reply_payload_v1_schema(),
            "collection":"single",
            "required":true
        }]
    }))
    .unwrap()
}

fn context_spec(id: &str, enabled: bool) -> ContextAssemblySpec {
    ContextAssemblySpec {
        id: None,
        name: None,
        mode: ContextAssemblyMode::Chat,
        items: vec![ContextItem {
            id: id.into(),
            name: None,
            enabled,
            requested_role: ContextRole::System,
            source: ContextSource::Literal {
                text: "Role play context".into(),
            },
            position: ContextPosition::Start,
            order: 0,
            priority: 0,
            insertion_depth: 0,
            budget: Default::default(),
            overflow: None,
        }],
        budget: None,
        post_process: vec![],
        preview: None,
    }
}
