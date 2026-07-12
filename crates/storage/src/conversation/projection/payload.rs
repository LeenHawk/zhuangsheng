use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    conversation::{AssistantReplyPayloadV1, assistant_reply_payload_v1_schema},
    schema,
};

use crate::{
    StorageError,
    graph::helpers::{load_object_json, sql},
};

pub(crate) enum ReplyPayloadError {
    Invalid(&'static str),
    Storage(StorageError),
}

pub(crate) async fn load_reply_payload<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    output_key: &str,
) -> Result<AssistantReplyPayloadV1, ReplyPayloadError> {
    let rows = connection.query_all_raw(sql(
        "SELECT value_object_id FROM run_output_values WHERE run_id = ? AND output_key = ? ORDER BY output_seq",
        vec![run_id.into(), output_key.into()],
    )).await.map_err(StorageError::from).map_err(ReplyPayloadError::Storage)?;
    if rows.len() != 1 {
        return Err(ReplyPayloadError::Invalid(
            "reply output cardinality is invalid",
        ));
    }
    let value_id: String = rows[0]
        .try_get("", "value_object_id")
        .map_err(StorageError::from)
        .map_err(ReplyPayloadError::Storage)?;
    let value: serde_json::Value = match load_object_json(connection, &value_id).await {
        Ok(value) => value,
        Err(StorageError::Integrity(_)) => {
            return Err(ReplyPayloadError::Invalid("reply output is corrupt"));
        }
        Err(error) => return Err(ReplyPayloadError::Storage(error)),
    };
    if schema::validate(&assistant_reply_payload_v1_schema(), &value).is_err() {
        return Err(ReplyPayloadError::Invalid("reply output schema is invalid"));
    }
    let payload: AssistantReplyPayloadV1 = serde_json::from_value(value)
        .map_err(|_| ReplyPayloadError::Invalid("reply output cannot be decoded"))?;
    payload
        .validate()
        .map_err(|_| ReplyPayloadError::Invalid("reply content is invalid"))?;
    Ok(payload)
}
