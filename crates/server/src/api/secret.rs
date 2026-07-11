use std::fmt;

use axum::{
    Json, Router,
    extract::{Path, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
    routing::{get, post, put},
};
use serde::{Deserialize, Deserializer};
use zeroize::Zeroizing;
use zhuangsheng_core::application::secret::{
    ChangeMasterPasswordCommand, InitializeSecretStoreCommand, LockSecretStoreCommand,
    LockSecretStoreResult, PutSecretCommand, SecretKind, SecretMetadataView,
    SecretStoreSessionView, SecretStoreStatusView, SecretValue, UnlockSecretStoreCommand,
};

use super::{
    AppState,
    error::ApiResult,
    graph::{json_body, required_header},
};

struct SensitiveBytes(Zeroizing<Vec<u8>>);

impl<'de> Deserialize<'de> for SensitiveBytes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = SensitiveBytes;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a secret string")
            }

            fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Self::Value, E> {
                Ok(SensitiveBytes(Zeroizing::new(value.into_bytes())))
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(SensitiveBytes(Zeroizing::new(value.as_bytes().to_vec())))
            }
        }
        deserializer.deserialize_string(Visitor)
    }
}

impl SensitiveBytes {
    fn into_secret(self) -> SecretValue {
        SecretValue::from_zeroizing(self.0)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PasswordBody {
    master_password: SensitiveBytes,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LockBody {
    expected_session_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PutSecretBody {
    name: Option<String>,
    kind: SecretKind,
    value: SensitiveBytes,
    session_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChangePasswordBody {
    current_password: SensitiveBytes,
    new_password: SensitiveBytes,
    session_id: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/secret-store/status", get(status))
        .route("/v1/secret-store/initialize", post(initialize))
        .route("/v1/secret-store/unlock", post(unlock))
        .route("/v1/secret-store/lock", post(lock))
        .route("/v1/secret-store/change-password", post(change_password))
        .route("/v1/secrets", get(list_secrets))
        .route("/v1/secrets/{secret_id}", put(put_secret))
}

async fn status(State(state): State<AppState>) -> ApiResult<Json<SecretStoreStatusView>> {
    Ok(Json(state.secret_service.status().await?))
}

async fn initialize(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<PasswordBody>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<SecretStoreSessionView>)> {
    let body = json_body(body)?;
    let result = state
        .secret_service
        .initialize(InitializeSecretStoreCommand {
            master_password: body.master_password.into_secret(),
            idempotency_key: required_header(&headers, "idempotency-key")?,
        })
        .await?;
    Ok((StatusCode::CREATED, Json(result)))
}

async fn unlock(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<PasswordBody>, JsonRejection>,
) -> ApiResult<Json<SecretStoreSessionView>> {
    let body = json_body(body)?;
    Ok(Json(
        state
            .secret_service
            .unlock(UnlockSecretStoreCommand {
                master_password: body.master_password.into_secret(),
                idempotency_key: required_header(&headers, "idempotency-key")?,
            })
            .await?,
    ))
}

async fn lock(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<LockBody>, JsonRejection>,
) -> ApiResult<Json<LockSecretStoreResult>> {
    let body = json_body(body)?;
    Ok(Json(
        state
            .secret_service
            .lock(LockSecretStoreCommand {
                expected_session_id: body.expected_session_id,
                idempotency_key: required_header(&headers, "idempotency-key")?,
            })
            .await?,
    ))
}

async fn list_secrets(State(state): State<AppState>) -> ApiResult<Json<Vec<SecretMetadataView>>> {
    Ok(Json(state.secret_service.list_secrets().await?))
}

async fn put_secret(
    State(state): State<AppState>,
    Path(secret_id): Path<String>,
    headers: HeaderMap,
    body: Result<Json<PutSecretBody>, JsonRejection>,
) -> ApiResult<Json<SecretMetadataView>> {
    let body = json_body(body)?;
    Ok(Json(
        state
            .secret_service
            .put_secret(PutSecretCommand {
                secret_id,
                name: body.name,
                kind: body.kind,
                value: body.value.into_secret(),
                session_id: body.session_id,
                idempotency_key: required_header(&headers, "idempotency-key")?,
            })
            .await?,
    ))
}

async fn change_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<ChangePasswordBody>, JsonRejection>,
) -> ApiResult<Json<SecretStoreSessionView>> {
    let body = json_body(body)?;
    Ok(Json(
        state
            .secret_service
            .change_master_password(ChangeMasterPasswordCommand {
                current_password: body.current_password.into_secret(),
                new_password: body.new_password.into_secret(),
                session_id: body.session_id,
                idempotency_key: required_header(&headers, "idempotency-key")?,
            })
            .await?,
    ))
}
