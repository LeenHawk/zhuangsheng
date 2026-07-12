use std::sync::Arc;

use async_trait::async_trait;
use zhuangsheng_core::{
    application::{
        ApplicationError,
        channel::{
            ChannelModelDiscoveryService, ChannelModelDiscoveryView, ChannelService,
            DiscoverChannelModelsCommand, DiscoveredChannelModel,
        },
        secret::SecretResolver,
    },
    llm::{
        ChannelCredential, LlmChannelRevision, LlmOperationExecutionPin, Operation, OperationKey,
        Provider,
        adapter::{ShapeAdapterKey, WireGenerationRequest},
    },
};

use crate::provider::ProviderTransport;

pub struct RemoteModelDiscoveryService {
    channels: Arc<dyn ChannelService>,
    secrets: Arc<dyn SecretResolver>,
    provider: Arc<dyn ProviderTransport>,
}

impl RemoteModelDiscoveryService {
    pub fn new(
        channels: Arc<dyn ChannelService>,
        secrets: Arc<dyn SecretResolver>,
        provider: Arc<dyn ProviderTransport>,
    ) -> Self {
        Self {
            channels,
            secrets,
            provider,
        }
    }
}

#[async_trait]
impl ChannelModelDiscoveryService for RemoteModelDiscoveryService {
    async fn discover_models(
        &self,
        command: DiscoverChannelModelsCommand,
    ) -> Result<ChannelModelDiscoveryView, ApplicationError> {
        let revision = match command.revision_id {
            Some(id) => self.channels.get_channel_revision(&id).await?,
            None => {
                self.channels
                    .get_channel_head_revision(&command.channel_id)
                    .await?
            }
        };
        if revision.channel_id != command.channel_id {
            return Err(invalid("model_discovery_channel_mismatch"));
        }
        let operation = select_operation(&revision, command.operation_key)?;
        let credential = match &revision.spec.credential {
            ChannelCredential::Secret { api_key_ref } => {
                Some(self.secrets.resolve_secret(api_key_ref).await?)
            }
            ChannelCredential::None => None,
        };
        let wire = discovery_wire(&revision, operation);
        let response = self
            .provider
            .send(&revision, &wire, credential.as_ref())
            .await
            .map_err(|_| ApplicationError::Unavailable)?;
        let mut models = decode_models(operation.provider_family(), &response.body)?;
        models.sort_by(|left, right| left.id.cmp(&right.id));
        models.dedup_by(|left, right| left.id == right.id);
        if models.len() > 1000 {
            return Err(invalid("model_discovery_limit"));
        }
        Ok(ChannelModelDiscoveryView {
            channel_id: command.channel_id,
            channel_revision_id: revision.id,
            operation_key: operation,
            models,
        })
    }
}

fn select_operation(
    revision: &LlmChannelRevision,
    requested: Option<OperationKey>,
) -> Result<OperationKey, ApplicationError> {
    let operations: Vec<_> = revision
        .spec
        .operation_keys
        .iter()
        .copied()
        .filter(|key| key.operation == Operation::ListModels)
        .collect();
    match requested {
        Some(key) if operations.contains(&key) => Ok(key),
        None if operations.len() == 1 => Ok(operations[0]),
        _ => Err(invalid("model_discovery_operation_invalid")),
    }
}

fn discovery_wire(revision: &LlmChannelRevision, operation: OperationKey) -> WireGenerationRequest {
    let target = gproxy_protocol::request_target(operation, "", false);
    WireGenerationRequest::from_parts(
        match operation.provider_family() {
            Provider::OpenAi => ShapeAdapterKey::OpenAiResponsesV1,
            Provider::Claude => ShapeAdapterKey::ClaudeMessagesV1,
            Provider::Gemini => ShapeAdapterKey::GeminiGenerateContentV1,
        },
        LlmOperationExecutionPin {
            channel_revision_id: revision.id.clone(),
            model_id: String::new(),
            operation_key: operation,
            operation_taxonomy_version: revision.spec.operation_taxonomy_version,
            adapter_decoder_version: revision.spec.adapter_decoder_version,
        },
        target.method,
        target.path,
        target.query,
        Vec::new(),
    )
}

fn decode_models(
    provider: Provider,
    bytes: &[u8],
) -> Result<Vec<DiscoveredChannelModel>, ApplicationError> {
    match provider {
        Provider::OpenAi => {
            let value: gproxy_protocol::openai::ModelListResponse = decode(bytes)?;
            value
                .data
                .into_iter()
                .map(|model| {
                    Ok(DiscoveredChannelModel {
                        id: model_id(&model.id)?,
                        name: None,
                        context_window: None,
                        max_output_tokens: None,
                    })
                })
                .collect()
        }
        Provider::Claude => {
            let value: gproxy_protocol::claude::ListModelsResponse = decode(bytes)?;
            value
                .data
                .into_iter()
                .map(|model| {
                    Ok(DiscoveredChannelModel {
                        id: model_id(&model.id)?,
                        name: Some(model.display_name),
                        context_window: Some(model.max_input_tokens),
                        max_output_tokens: Some(model.max_tokens),
                    })
                })
                .collect()
        }
        Provider::Gemini => {
            let value: gproxy_protocol::gemini::ListModelsResponse = decode(bytes)?;
            value
                .models
                .into_iter()
                .map(|model| {
                    let id = model
                        .name
                        .ok_or_else(|| invalid("model_discovery_response_invalid"))?;
                    Ok(DiscoveredChannelModel {
                        id: id.strip_prefix("models/").unwrap_or(&id).into(),
                        name: model.display_name,
                        context_window: model.input_token_limit.and_then(|v| u64::try_from(v).ok()),
                        max_output_tokens: model
                            .output_token_limit
                            .and_then(|v| u64::try_from(v).ok()),
                    })
                })
                .collect()
        }
    }
}

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, ApplicationError> {
    serde_json::from_slice(bytes).map_err(|_| invalid("model_discovery_response_invalid"))
}

fn model_id<T: serde::Serialize>(value: &T) -> Result<String, ApplicationError> {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| invalid("model_discovery_response_invalid"))
}

fn invalid(code: &'static str) -> ApplicationError {
    ApplicationError::InvalidArgument {
        code,
        message: "model discovery request or response is invalid".into(),
    }
}
