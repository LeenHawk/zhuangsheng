use sea_orm::TransactionTrait;
use serde_json::json;
use zhuangsheng_core::{
    application::graph::{
        ApplyGraphCommand, CreateGraphCommand, CreateRolePlayTemplateCommand, GraphRevisionView,
        UpdateGraphDraftCommand,
    },
    conversation::{assistant_reply_payload_v1_schema, conversation_run_input_v1_schema},
    graph::{DraftNodeKind, GraphDraft, LlmRequestOptions},
    llm::{OperationKey, context::ContextAssemblyConfig, is_supported_generation_key},
};

use crate::{SqliteStore, StorageError, StorageResult};

impl SqliteStore {
    pub async fn create_roleplay_template(
        &self,
        command: CreateRolePlayTemplateCommand,
    ) -> StorageResult<GraphRevisionView> {
        if command.idempotency_key.is_empty()
            || command.channel_id.is_empty()
            || command.preset_id.is_empty()
        {
            return Err(StorageError::InvalidArgument(
                "role play template resources are required".into(),
            ));
        }
        let graph = self
            .create_graph(CreateGraphCommand {
                name: command.name.clone(),
                idempotency_key: format!("{}:create", command.idempotency_key),
            })
            .await?;
        if let Some(revision) = self
            .replay_roleplay_apply(&graph.graph.id, &command)
            .await?
        {
            return Ok(revision);
        }
        let channel = self.get_channel_head_revision(&command.channel_id).await?;
        self.get_context_preset_head(&command.preset_id).await?;
        let operation_key = channel
            .spec
            .operation_keys
            .iter()
            .copied()
            .filter(|key| is_supported_generation_key(*key))
            .collect::<Vec<_>>();
        if operation_key.len() != 1 {
            return Err(StorageError::InvalidArgument(
                "channel must expose exactly one supported generation operation".into(),
            ));
        }
        let operation_key = operation_key[0];
        let catalog = channel
            .spec
            .model_catalogs
            .iter()
            .find(|catalog| catalog.operation_key == operation_key)
            .ok_or_else(|| {
                StorageError::InvalidArgument("channel model catalog is missing".into())
            })?;
        if catalog.models.len() != 1 {
            return Err(StorageError::InvalidArgument(
                "channel must expose exactly one model for user-mode mapping".into(),
            ));
        }
        let document = roleplay_draft(
            &graph.graph.id,
            &graph.graph.name,
            &command.channel_id,
            &catalog.models[0].id,
            operation_key,
            &command.preset_id,
            command.generation.clone(),
            command.extensions.clone(),
        )?;
        let updated = self
            .update_graph_draft(UpdateGraphDraftCommand {
                graph_id: graph.graph.id.clone(),
                // The create receipt always returns the original empty-draft token. Reusing it
                // lets the draft receipt replay after a crash between draft update and apply.
                expected_revision_token: graph.draft_revision_token,
                document,
                idempotency_key: format!("{}:draft", command.idempotency_key),
            })
            .await?;
        self.apply_graph(ApplyGraphCommand {
            graph_id: graph.graph.id,
            expected_revision_token: updated.revision_token,
            operation_taxonomy_version: 1,
            adapter_decoder_version: 1,
            idempotency_key: format!("{}:apply", command.idempotency_key),
        })
        .await
    }

    async fn replay_roleplay_apply(
        &self,
        graph_id: &str,
        command: &CreateRolePlayTemplateCommand,
    ) -> StorageResult<Option<GraphRevisionView>> {
        let scope = format!("workspace:local:graphs:{graph_id}:apply");
        let key = format!("{}:apply", command.idempotency_key);
        let transaction = self.db.begin().await?;
        let Some(receipt) = super::helpers::find_receipt(&transaction, &scope, &key).await? else {
            transaction.commit().await?;
            return Ok(None);
        };
        let object_id = receipt.result_object_id.ok_or_else(|| {
            StorageError::Integrity("role play apply receipt has no result object".into())
        })?;
        let revision: GraphRevisionView =
            super::helpers::load_object_json(&transaction, &object_id).await?;
        if !roleplay_revision_matches(&revision, graph_id, command) {
            return Err(StorageError::IdempotencyConflict);
        }
        transaction.commit().await?;
        Ok(Some(revision))
    }
}

fn roleplay_revision_matches(
    revision: &GraphRevisionView,
    graph_id: &str,
    command: &CreateRolePlayTemplateCommand,
) -> bool {
    revision.graph_id == graph_id
        && revision.definition.nodes.iter().any(|node| {
            let DraftNodeKind::Llm { config } = &node.kind else {
                return false;
            };
            config.model.channel_id == command.channel_id
                && matches!(
                    &config.context,
                    ContextAssemblyConfig::Preset { preset_id }
                        if preset_id == &command.preset_id
                )
                && config
                    .request
                    .as_ref()
                    .and_then(|request| request.generation.as_ref())
                    == command.generation.as_ref()
                && config
                    .request
                    .as_ref()
                    .and_then(|request| request.extensions.as_ref())
                    == command.extensions.as_ref()
        })
}

fn roleplay_draft(
    graph_id: &str,
    name: &str,
    channel_id: &str,
    model_id: &str,
    operation_key: OperationKey,
    preset_id: &str,
    generation: Option<zhuangsheng_core::graph::GenerationOptionsIr>,
    extensions: Option<zhuangsheng_core::graph::ProviderExtensionsIr>,
) -> StorageResult<GraphDraft> {
    let reply_schema = assistant_reply_payload_v1_schema();
    let mut draft: GraphDraft = serde_json::from_value(json!({
        "graphId": graph_id,
        "name": name,
        "nodes": [
            {"id":"input","name":"Conversation input","kind":"input","runInputSelector":{"type":"whole_value"}},
            {"id":"reply","name":"Role response","kind":"llm","model":{"channelId":channel_id,"modelId":model_id,"operationKey":operation_key},"context":{"type":"preset","presetId":preset_id},"memory":{"reads":[{"id":"history","as":"history","source":{"kind":"conversation_history","scope":"run-context"},"required":true,"consistency":"snapshot","limit":null,"maxBytes":16777216}],"workingWrites":[],"tools":[]},"output":{"mode":"json","schema":reply_schema,"strict":true},"streaming":{"enabled":false,"audience":"user","persistChunks":false}},
            {"id":"output","name":"Assistant reply","kind":"output","outputKey":"reply"}
        ],
        "edges": [
            {"from":{"nodeId":"input","output":"default"},"to":{"nodeId":"reply","input":"default"}},
            {"from":{"nodeId":"reply","output":"default"},"to":{"nodeId":"output","input":"default"}}
        ],
        "runInputSchema": conversation_run_input_v1_schema(),
        "outputContract": [{"key":"reply","schema":reply_schema,"collection":"single","required":true}]
    }))
    .map_err(|error| StorageError::Integrity(error.to_string()))?;
    if generation.is_some() || extensions.is_some() {
        let config = draft
            .nodes
            .iter_mut()
            .find_map(|node| match &mut node.kind {
                DraftNodeKind::Llm { config } if node.id == "reply" => Some(config),
                _ => None,
            });
        let config = config.ok_or_else(|| {
            StorageError::Integrity("role play template lost its LLM node".into())
        })?;
        config.request = Some(LlmRequestOptions {
            generation,
            extensions,
            tool_choice: None,
        });
    }
    Ok(draft)
}
