use sea_orm::ConnectionTrait;
use zhuangsheng_core::llm::{
    LlmLogicalCallStatus, LlmLoopCheckpoint, PendingLlmOutputRepair, ir::LlmTurnItemIr,
};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{load_object_json, sql},
};

pub(super) struct OutputRepairResume {
    pub used: u64,
    pub pending: Option<PendingLlmOutputRepair>,
}

pub(super) async fn load_output_repair_resume<C: ConnectionTrait>(
    connection: &C,
    checkpoint: &LlmLoopCheckpoint,
    transcript: &[LlmTurnItemIr],
) -> StorageResult<OutputRepairResume> {
    let count: i64 = connection
        .query_one_raw(sql(
            "SELECT COUNT(*) AS count FROM llm_output_repairs WHERE node_instance_id = ?",
            vec![checkpoint.node_instance_id.clone().into()],
        ))
        .await?
        .expect("count query returns a row")
        .try_get("", "count")?;
    let used = u64::try_from(count)
        .map_err(|_| StorageError::Integrity("invalid output repair count".into()))?;
    let Some(active) = checkpoint.active_model_effect.as_ref() else {
        return Ok(OutputRepairResume {
            used,
            pending: None,
        });
    };
    if active.status != LlmLogicalCallStatus::Completed || !checkpoint.current_batch.is_empty() {
        return Ok(OutputRepairResume {
            used,
            pending: None,
        });
    }
    let row = connection
        .query_one_raw(sql(
            "SELECT id, repair_no, source_model_call_id, extracted_bytes_digest, error_code, instruction_object_id FROM llm_output_repairs WHERE source_model_call_id = ?",
            vec![active.model_call_id.clone().into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(OutputRepairResume {
            used,
            pending: None,
        });
    };
    let repair_id: String = row.try_get("", "id")?;
    let instruction: LlmTurnItemIr = load_object_json(
        connection,
        &row.try_get::<String>("", "instruction_object_id")?,
    )
    .await?;
    if checkpoint.effect_watermark != format!("outputrepair:{repair_id}")
        || transcript.last() != Some(&instruction)
    {
        return Err(StorageError::Integrity(
            "pending output repair does not match the checkpoint".into(),
        ));
    }
    Ok(OutputRepairResume {
        used,
        pending: Some(PendingLlmOutputRepair {
            repair_id,
            repair_no: u64::try_from(row.try_get::<i64>("", "repair_no")?)
                .map_err(|_| StorageError::Integrity("invalid output repair number".into()))?,
            source_model_call_id: row.try_get("", "source_model_call_id")?,
            extracted_bytes_digest: row.try_get("", "extracted_bytes_digest")?,
            error_code: row.try_get("", "error_code")?,
        }),
    })
}
