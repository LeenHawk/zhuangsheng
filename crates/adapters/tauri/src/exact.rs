use serde::Serialize;
use serde_json::Value;

use crate::{CommandResult, TauriAdapter, TauriCommandError};

mod config;
mod context;
mod graph;
mod memory;
mod runtime;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExactEnvelope {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<TauriCommandError>,
}

impl TauriAdapter {
    pub async fn invoke_exact_json(&self, operation: &str, payload_json: &str) -> Vec<u8> {
        let result = match zhuangsheng_core::canonical::parse(payload_json) {
            Ok(payload) => self.dispatch_exact(operation, &payload).await,
            Err(_) => Err(error(
                "invalid_json",
                "invalid or out-of-bounds JSON payload",
            )),
        };
        let envelope = match result {
            Ok(value) => ExactEnvelope {
                ok: true,
                value: Some(value),
                error: None,
            },
            Err(error) => ExactEnvelope {
                ok: false,
                value: None,
                error: Some(error),
            },
        };
        serde_json::to_vec(&envelope).unwrap_or_else(|_| {
            br#"{"ok":false,"error":{"code":"internal_error","message":"response serialization failed","retryable":false}}"#.to_vec()
        })
    }

    async fn dispatch_exact(&self, operation: &str, payload: &Value) -> CommandResult<Value> {
        if let Some(result) = runtime::dispatch(self, operation, payload).await {
            return result;
        }
        if let Some(result) = graph::dispatch(self, operation, payload).await {
            return result;
        }
        if let Some(result) = config::dispatch(self, operation, payload).await {
            return result;
        }
        if let Some(result) = context::dispatch(self, operation, payload).await {
            return result;
        }
        if let Some(result) = memory::dispatch(self, operation, payload).await {
            return result;
        }
        if operation == "list_tool_descriptors" {
            return encode(self.list_tool_descriptors().await);
        }
        Err(error(
            "unknown_operation",
            "unsupported exact JSON operation",
        ))
    }
}

pub(super) fn argument<T: serde::de::DeserializeOwned>(
    payload: &Value,
    key: &str,
) -> CommandResult<T> {
    let value = payload
        .get(key)
        .cloned()
        .ok_or_else(|| error("invalid_argument", &format!("missing argument: {key}")))?;
    serde_json::from_value(value)
        .map_err(|_| error("invalid_argument", &format!("invalid argument: {key}")))
}

pub(super) fn encode<T: Serialize>(result: CommandResult<T>) -> CommandResult<Value> {
    result.and_then(|value| {
        serde_json::to_value(value)
            .map_err(|_| error("internal_error", "response serialization failed"))
    })
}

fn error(code: &str, message: &str) -> TauriCommandError {
    TauriCommandError {
        code: code.into(),
        message: message.into(),
        retryable: false,
        details: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ResolveEffectUnknownInput;

    #[test]
    fn exact_ipc_dto_preserves_domain_number_lexemes() {
        let payload = zhuangsheng_core::canonical::parse(
            r#"{"input":{"effectId":"effect_1","expectedEffectAttemptId":"attempt_1","expectedRunControlEpoch":1,"kind":"abort_run","decision":{"unsafeInteger":9007199254740993,"decimal":1.2345678901234567890123456789,"exponent":12345678901234567890e-17},"resultObjectId":null,"evidenceObjectId":null,"idempotencyKey":"resolution_1"}}"#,
        ).unwrap();

        let input = argument::<ResolveEffectUnknownInput>(&payload, "input").unwrap();
        let serialized =
            serde_json::to_string(&encode::<Value>(Ok(input.decision)).unwrap()).unwrap();

        assert!(serialized.contains("9007199254740993"));
        assert!(serialized.contains("1.2345678901234567890123456789"));
        assert!(serialized.contains("12345678901234567890e-17"));
        assert!(!serialized.contains("9007199254740992"));
    }
}
