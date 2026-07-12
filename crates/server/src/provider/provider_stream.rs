use reqwest::header::CONTENT_TYPE;

use super::{
    HttpProviderClient, MAX_RESPONSE_BYTES, ProviderHttpError, ProviderHttpStreamResponse,
    endpoint_url, error, method, provider_request_id, request_headers, retryable_status,
    transport_error,
};
use crate::provider::provider_sse::SseDataDecoder;
use zhuangsheng_core::{
    application::secret::SecretValue,
    llm::{LlmChannelRevision, adapter::WireGenerationRequest},
};

const MAX_STREAM_BYTES: usize = 64 * 1024 * 1024;

impl HttpProviderClient {
    pub async fn send_stream(
        &self,
        channel: &LlmChannelRevision,
        wire: &WireGenerationRequest,
        credential: Option<&SecretValue>,
    ) -> Result<ProviderHttpStreamResponse, ProviderHttpError> {
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
        if !status.is_success() {
            let body = read_bounded_body(&mut response, MAX_RESPONSE_BYTES).await?;
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
        let is_sse = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.to_ascii_lowercase().starts_with("text/event-stream"));
        if !is_sse {
            return Err(ProviderHttpError {
                code: "provider_stream_content_type_invalid",
                safe_message: "provider stream did not use text/event-stream".into(),
                retryable: false,
                outcome_unknown: false,
                status: Some(status.as_u16()),
                provider_request_id,
                response_body: None,
            });
        }
        let request_id = provider_request_id.clone();
        let frames = Box::pin(async_stream::try_stream! {
            let mut decoder = SseDataDecoder::default();
            let mut total = 0usize;
            while let Some(chunk) = response.chunk().await.map_err(|error| {
                stream_transport_error(error, request_id.clone())
            })? {
                total = total.saturating_add(chunk.len());
                if total > MAX_STREAM_BYTES {
                    Err(stream_error(
                        "provider_stream_too_large",
                        "provider stream exceeds 64 MiB",
                        request_id.clone(),
                    ))?;
                }
                for frame in decoder.push(&chunk).map_err(|_| {
                    stream_error(
                        "provider_stream_framing_invalid",
                        "provider returned an invalid SSE stream",
                        request_id.clone(),
                    )
                })? {
                    if frame != b"[DONE]" {
                        yield frame;
                    }
                }
            }
            for frame in decoder.finish().map_err(|_| {
                stream_error(
                    "provider_stream_framing_invalid",
                    "provider returned an invalid SSE stream",
                    request_id.clone(),
                )
            })? {
                if frame != b"[DONE]" {
                    yield frame;
                }
            }
        });
        Ok(ProviderHttpStreamResponse {
            status: status.as_u16(),
            provider_request_id,
            frames,
        })
    }
}

async fn read_bounded_body(
    response: &mut reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, ProviderHttpError> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
        if body.len().saturating_add(chunk.len()) > limit {
            return Err(error(
                "provider_response_too_large",
                "provider response exceeds the configured limit",
            ));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn stream_transport_error(
    error: reqwest::Error,
    provider_request_id: Option<String>,
) -> ProviderHttpError {
    let mut error = transport_error(error);
    error.provider_request_id = provider_request_id;
    error.outcome_unknown = true;
    error
}

fn stream_error(
    code: &'static str,
    message: &'static str,
    provider_request_id: Option<String>,
) -> ProviderHttpError {
    ProviderHttpError {
        code,
        safe_message: message.into(),
        retryable: false,
        outcome_unknown: true,
        status: None,
        provider_request_id,
        response_body: None,
    }
}
