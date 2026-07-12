use std::{pin::Pin, time::Duration};

use async_trait::async_trait;
use futures_core::Stream;
use reqwest::{
    Client, StatusCode, Url,
    header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue},
};
use thiserror::Error;
use zeroize::Zeroizing;
use zhuangsheng_core::{
    application::secret::SecretValue,
    llm::{LlmChannelRevision, Provider, adapter::WireGenerationRequest},
};

const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

mod provider_sse;
mod provider_stream;

pub struct HttpProviderClient {
    client: Client,
}

#[async_trait]
pub trait ProviderTransport: Send + Sync {
    async fn send(
        &self,
        channel: &LlmChannelRevision,
        wire: &WireGenerationRequest,
        credential: Option<&SecretValue>,
    ) -> Result<ProviderHttpResponse, ProviderHttpError>;

    async fn send_stream(
        &self,
        _channel: &LlmChannelRevision,
        _wire: &WireGenerationRequest,
        _credential: Option<&SecretValue>,
    ) -> Result<ProviderHttpStreamResponse, ProviderHttpError> {
        Err(error(
            "provider_streaming_unsupported",
            "provider transport does not support streaming",
        ))
    }
}

pub type ProviderFrameStream =
    Pin<Box<dyn Stream<Item = Result<Vec<u8>, ProviderHttpError>> + Send>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderHttpResponse {
    pub status: u16,
    pub provider_request_id: Option<String>,
    pub body: Vec<u8>,
}

pub struct ProviderHttpStreamResponse {
    pub status: u16,
    pub provider_request_id: Option<String>,
    pub frames: ProviderFrameStream,
}

#[derive(Debug, Error)]
#[error("{code}: {safe_message}")]
pub struct ProviderHttpError {
    pub code: &'static str,
    pub safe_message: String,
    pub retryable: bool,
    pub outcome_unknown: bool,
    pub status: Option<u16>,
    pub provider_request_id: Option<String>,
    pub response_body: Option<Vec<u8>>,
}

impl HttpProviderClient {
    pub fn new() -> Result<Self, ProviderHttpError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(120))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("zhuangsheng/0.1")
            .build()
            .map_err(|_| {
                error(
                    "provider_client_init_failed",
                    "provider client initialization failed",
                )
            })?;
        Ok(Self { client })
    }

    pub async fn send(
        &self,
        channel: &LlmChannelRevision,
        wire: &WireGenerationRequest,
        credential: Option<&SecretValue>,
    ) -> Result<ProviderHttpResponse, ProviderHttpError> {
        if wire.operation.channel_revision_id != channel.id {
            return Err(error(
                "provider_channel_pin_mismatch",
                "provider request does not match the pinned channel revision",
            ));
        }
        let url = endpoint_url(channel, wire)?;
        let headers = request_headers(channel, wire, credential)?;
        let mut response = self
            .client
            .request(method(wire.method)?, url)
            .headers(headers)
            .body(wire.body().to_vec())
            .send()
            .await
            .map_err(transport_error)?;
        let status = response.status();
        let provider_request_id = provider_request_id(response.headers());
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err(ProviderHttpError {
                code: "provider_response_too_large",
                safe_message: "provider response exceeds the configured limit".into(),
                retryable: false,
                outcome_unknown: false,
                status: Some(status.as_u16()),
                provider_request_id,
                response_body: None,
            });
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
            if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                return Err(ProviderHttpError {
                    code: "provider_response_too_large",
                    safe_message: "provider response exceeds the configured limit".into(),
                    retryable: false,
                    outcome_unknown: false,
                    status: Some(status.as_u16()),
                    provider_request_id,
                    response_body: None,
                });
            }
            body.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            return Err(ProviderHttpError {
                code: "provider_http_error",
                safe_message: format!("provider returned HTTP {}", status.as_u16()),
                retryable: retryable_status(status),
                outcome_unknown: false,
                status: Some(status.as_u16()),
                provider_request_id,
                response_body: Some(body),
            });
        }
        Ok(ProviderHttpResponse {
            status: status.as_u16(),
            provider_request_id,
            body,
        })
    }
}

#[async_trait]
impl ProviderTransport for HttpProviderClient {
    async fn send(
        &self,
        channel: &LlmChannelRevision,
        wire: &WireGenerationRequest,
        credential: Option<&SecretValue>,
    ) -> Result<ProviderHttpResponse, ProviderHttpError> {
        HttpProviderClient::send(self, channel, wire, credential).await
    }

    async fn send_stream(
        &self,
        channel: &LlmChannelRevision,
        wire: &WireGenerationRequest,
        credential: Option<&SecretValue>,
    ) -> Result<ProviderHttpStreamResponse, ProviderHttpError> {
        HttpProviderClient::send_stream(self, channel, wire, credential).await
    }
}

fn endpoint_url(
    channel: &LlmChannelRevision,
    wire: &WireGenerationRequest,
) -> Result<Url, ProviderHttpError> {
    let mut base = Url::parse(&channel.spec.base_url).map_err(|_| {
        error(
            "provider_base_url_invalid",
            "pinned provider base URL is invalid",
        )
    })?;
    if base.query().is_some()
        || base.fragment().is_some()
        || !wire.relative_path.starts_with('/')
        || wire.relative_path.contains("..")
        || wire.relative_path.contains(['?', '#'])
    {
        return Err(error(
            "provider_request_target_invalid",
            "provider request target is not a safe relative path",
        ));
    }
    let base_path = base.path().trim_end_matches('/');
    let path = if base_path.is_empty() || wire.relative_path.starts_with(&format!("{base_path}/")) {
        wire.relative_path.clone()
    } else {
        format!("{base_path}{}", wire.relative_path)
    };
    base.set_path(&path);
    base.set_query(wire.query.as_deref());
    Ok(base)
}

fn request_headers(
    channel: &LlmChannelRevision,
    wire: &WireGenerationRequest,
    credential: Option<&SecretValue>,
) -> Result<HeaderMap, ProviderHttpError> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static(wire.content_type));
    let Some(secret) = credential else {
        if channel.spec.transport_policy.allow_unauthenticated {
            return Ok(headers);
        }
        return Err(error(
            "provider_credential_missing",
            "provider credential is unavailable",
        ));
    };
    match wire.operation.operation_key.provider_family() {
        Provider::OpenAi => {
            let value = secret.with_bytes(|bytes| {
                let mut bearer = Zeroizing::new(Vec::with_capacity(7 + bytes.len()));
                bearer.extend_from_slice(b"Bearer ");
                bearer.extend_from_slice(bytes);
                HeaderValue::from_bytes(&bearer)
            });
            headers.insert(AUTHORIZATION, valid_header(value)?);
        }
        Provider::Claude => {
            headers.insert(
                HeaderName::from_static("x-api-key"),
                valid_header(secret.with_bytes(HeaderValue::from_bytes))?,
            );
            headers.insert(
                HeaderName::from_static("anthropic-version"),
                HeaderValue::from_static("2023-06-01"),
            );
        }
        Provider::Gemini => {
            headers.insert(
                HeaderName::from_static("x-goog-api-key"),
                valid_header(secret.with_bytes(HeaderValue::from_bytes))?,
            );
        }
    }
    Ok(headers)
}

fn valid_header(
    value: Result<HeaderValue, reqwest::header::InvalidHeaderValue>,
) -> Result<HeaderValue, ProviderHttpError> {
    value.map_err(|_| {
        error(
            "provider_credential_invalid",
            "provider credential is invalid",
        )
    })
}

fn method(value: gproxy_protocol::HttpMethod) -> Result<reqwest::Method, ProviderHttpError> {
    match value {
        gproxy_protocol::HttpMethod::Get => Ok(reqwest::Method::GET),
        gproxy_protocol::HttpMethod::Post => Ok(reqwest::Method::POST),
        gproxy_protocol::HttpMethod::Put => Ok(reqwest::Method::PUT),
        gproxy_protocol::HttpMethod::Delete => Ok(reqwest::Method::DELETE),
        gproxy_protocol::HttpMethod::Patch => Ok(reqwest::Method::PATCH),
    }
}

fn provider_request_id(headers: &HeaderMap) -> Option<String> {
    ["x-request-id", "request-id", "x-goog-request-id"]
        .iter()
        .find_map(|name| {
            headers
                .get(*name)
                .and_then(|value| value.to_str().ok())
                .filter(|value| !value.is_empty() && value.len() <= 256)
                .map(str::to_owned)
        })
}

fn retryable_status(status: StatusCode) -> bool {
    status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
}

fn transport_error(error: reqwest::Error) -> ProviderHttpError {
    ProviderHttpError {
        code: if error.is_timeout() {
            "provider_timeout"
        } else if error.is_connect() {
            "provider_connect_failed"
        } else {
            "provider_transport_failed"
        },
        safe_message: if error.is_timeout() {
            "provider request timed out"
        } else if error.is_connect() {
            "provider connection failed"
        } else {
            "provider transport failed"
        }
        .into(),
        retryable: true,
        outcome_unknown: !error.is_connect(),
        status: error.status().map(|status| status.as_u16()),
        provider_request_id: None,
        response_body: None,
    }
}

fn error(code: &'static str, message: &'static str) -> ProviderHttpError {
    ProviderHttpError {
        code,
        safe_message: message.into(),
        retryable: false,
        outcome_unknown: false,
        status: None,
        provider_request_id: None,
        response_body: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zhuangsheng_core::llm::{ContentGenerationKind, Operation, OperationKey};

    #[test]
    fn endpoint_preserves_base_prefix_without_duplicate_version() {
        let (channel, wire) = fixture();
        assert_eq!(
            endpoint_url(&channel, &wire).unwrap().as_str(),
            "https://api.example.test/v1/responses"
        );
    }

    #[test]
    fn openai_auth_is_bearer_and_credential_is_required() {
        let (channel, wire) = fixture();
        let secret = SecretValue::from_utf8("test-key".into());
        let headers = request_headers(&channel, &wire, Some(&secret)).unwrap();
        assert_eq!(headers.get(AUTHORIZATION).unwrap(), "Bearer test-key");
        assert_eq!(
            request_headers(&channel, &wire, None).unwrap_err().code,
            "provider_credential_missing"
        );
    }

    fn fixture() -> (LlmChannelRevision, WireGenerationRequest) {
        let operation_key = OperationKey::content_generation(
            Operation::GenerateContent,
            ContentGenerationKind::OpenAiResponses,
        );
        let pin = zhuangsheng_core::llm::LlmOperationExecutionPin {
            channel_revision_id: "channel-revision".into(),
            model_id: "model".into(),
            operation_key,
            operation_taxonomy_version: 1,
            adapter_decoder_version: 1,
        };
        let channel = LlmChannelRevision {
            id: "channel-revision".into(),
            channel_id: "channel".into(),
            revision_no: 1,
            spec: zhuangsheng_core::llm::LlmChannelRevisionSpec {
                operation_taxonomy_version: 1,
                adapter_decoder_version: 1,
                base_url: "https://api.example.test/v1".into(),
                transport_policy: zhuangsheng_core::llm::ChannelTransportPolicy {
                    allow_loopback_http: false,
                    allow_unauthenticated: false,
                },
                credential: zhuangsheng_core::llm::ChannelCredential::None,
                operation_keys: vec![operation_key],
                model_catalogs: Vec::new(),
                capabilities: Vec::new(),
            },
            content_hash: "sha256:test".into(),
            created_at: 0,
        };
        let wire = WireGenerationRequest::from_parts(
            zhuangsheng_core::llm::adapter::ShapeAdapterKey::OpenAiResponsesV1,
            pin,
            gproxy_protocol::HttpMethod::Post,
            "/v1/responses".into(),
            None,
            b"{}".to_vec(),
        );
        (channel, wire)
    }
}
