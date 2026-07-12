use sea_orm::{ConnectionTrait, TransactionTrait};
use serde_json::{Map, Value, json};
use zhuangsheng_core::canonical;

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::{
        apply::load_revision,
        helpers::{put_inline_object, sql},
    },
};

use super::{
    events::{Event, add_object_ref, append_event, fail_run, finish_wakeup},
    join_by_key_buffer,
};

impl SqliteStore {
    pub(crate) async fn settle_run(
        &self,
        wakeup_id: &str,
        run_id: &str,
        now: i64,
    ) -> StorageResult<()> {
        let transaction = self.db.begin().await?;
        let run = transaction.query_one_raw(sql(
            "SELECT graph_revision_id, context_id, branch_id, deadline_at FROM graph_runs WHERE id = ? AND status = 'running'",
            vec![run_id.into()],
        )).await?;
        let Some(run) = run else {
            finish_wakeup(&transaction, wakeup_id).await?;
            transaction.commit().await?;
            return Ok(());
        };
        let deadline: i64 = run.try_get("", "deadline_at")?;
        if now >= deadline {
            fail_run(
                &transaction,
                run_id,
                "run_deadline_exceeded",
                "run deadline exceeded",
                now,
            )
            .await?;
            transaction.commit().await?;
            return Ok(());
        }
        if has_active_work(&transaction, run_id, wakeup_id).await? {
            finish_wakeup(&transaction, wakeup_id).await?;
            transaction.commit().await?;
            return Ok(());
        }
        let revision_id: String = run.try_get("", "graph_revision_id")?;
        let revision = load_revision(&transaction, &revision_id).await?;
        let missing = required_output_missing(&transaction, run_id, &revision.definition).await?;
        if let Some(key) = missing {
            fail_run(
                &transaction,
                run_id,
                "required_output_missing",
                &format!("required output is missing: {key}"),
                now,
            )
            .await?;
            transaction.commit().await?;
            return Ok(());
        }
        join_by_key_buffer::strand(&transaction, run_id, now).await?;
        append_stranded_events(&transaction, run_id, now).await?;
        let outputs = build_outputs(&transaction, run_id, &revision.definition).await?;
        let outputs_id =
            put_inline_object(&transaction, &canonical::to_vec(&outputs)?, now).await?;
        let context_id: String = run.try_get("", "context_id")?;
        let branch_id: String = run.try_get("", "branch_id")?;
        let head = transaction
            .query_one_raw(sql(
                "SELECT head_commit_id FROM context_branches WHERE context_id = ? AND id = ?",
                vec![context_id.into(), branch_id.into()],
            ))
            .await?
            .ok_or_else(|| StorageError::Integrity("run context branch missing".into()))?;
        let head_id: String = head.try_get("", "head_commit_id")?;
        let completed = transaction.execute_raw(sql(
            "UPDATE graph_runs SET status = 'completed', output_commit_id = ?, run_outputs_object_id = ?, finished_at = ?, updated_at = ? WHERE id = ? AND status = 'running'",
            vec![head_id.clone().into(), outputs_id.clone().into(), now.into(), now.into(), run_id.into()],
        )).await?;
        if completed.rows_affected() != 1 {
            return Err(StorageError::Conflict("run_status"));
        }
        transaction.execute_raw(sql(
            "UPDATE runtime_timers SET status = 'cancelled' WHERE run_id = ? AND status IN ('pending','ready')",
            vec![run_id.into()],
        )).await?;
        finish_wakeup(&transaction, wakeup_id).await?;
        add_object_ref(
            &transaction,
            &outputs_id,
            "graph_run",
            run_id,
            "run_outputs",
            now,
        )
        .await?;
        append_event(&transaction, Event {
            run_id,
            event_type: "run.completed",
            importance: "critical",
            node_instance_id: None,
            attempt_id: None,
            payload: json!({"schemaVersion":1,"outputCommitId":head_id,"outputsRef":outputs_id}),
            now,
        }).await?;
        transaction.commit().await?;
        Ok(())
    }
}

async fn has_active_work<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    current_wakeup: &str,
) -> StorageResult<bool> {
    if connection.query_one_raw(sql(
        "SELECT 1 AS present FROM node_instances WHERE run_id = ? AND status IN ('ready','running','waiting') LIMIT 1",
        vec![run_id.into()],
    )).await?.is_some() {
        return Ok(true);
    }
    Ok(connection.query_one_raw(sql(
        "SELECT 1 AS present FROM scheduler_wakeups WHERE run_id = ? AND id <> ? AND status IN ('pending','claimed') LIMIT 1",
        vec![run_id.into(), current_wakeup.into()],
    )).await?.is_some())
}

async fn required_output_missing<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    definition: &zhuangsheng_core::graph::AppliedGraphDefinition,
) -> StorageResult<Option<String>> {
    for contract in definition
        .output_contract
        .iter()
        .filter(|item| item.required)
    {
        let row = connection.query_one_raw(sql(
            "SELECT 1 AS present FROM run_output_values WHERE run_id = ? AND output_key = ? LIMIT 1",
            vec![run_id.into(), contract.key.clone().into()],
        )).await?;
        if row.is_none() {
            return Ok(Some(contract.key.clone()));
        }
    }
    Ok(None)
}

async fn build_outputs<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    definition: &zhuangsheng_core::graph::AppliedGraphDefinition,
) -> StorageResult<Value> {
    let mut outputs = Map::new();
    for contract in &definition.output_contract {
        let rows = connection.query_all_raw(sql(
            "SELECT o.output_seq, o.value_object_id, c.content_hash, c.byte_size FROM run_output_values o JOIN content_objects c ON c.id = o.value_object_id WHERE o.run_id = ? AND o.output_key = ? ORDER BY o.output_seq",
            vec![run_id.into(), contract.key.clone().into()],
        )).await?;
        let values: Vec<_> = rows
            .iter()
            .map(|row| -> StorageResult<Value> {
                Ok(json!({
                    "valueRef": row.try_get::<String>("", "value_object_id")?,
                    "contentHash": row.try_get::<String>("", "content_hash")?,
                    "sizeBytes": row.try_get::<i64>("", "byte_size")?,
                    "outputSeq": row.try_get::<i64>("", "output_seq")?
                }))
            })
            .collect::<StorageResult<_>>()?;
        outputs.insert(
            contract.key.clone(),
            json!({
                "collection": contract.collection,
                "values": values
            }),
        );
    }
    Ok(json!({"schemaVersion":1,"outputs":outputs}))
}

async fn append_stranded_events<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    now: i64,
) -> StorageResult<()> {
    let rows = connection.query_all_raw(sql(
        "SELECT id, edge_id, enqueue_seq FROM edge_queue_values WHERE run_id = ? AND consumed_at IS NULL ORDER BY enqueue_seq",
        vec![run_id.into()],
    )).await?;
    for row in rows {
        append_event(
            connection,
            Event {
                run_id,
                event_type: "edge.value.stranded",
                importance: "critical",
                node_instance_id: None,
                attempt_id: None,
                payload: json!({
                    "schemaVersion":1,
                    "queueValueId":row.try_get::<String>("", "id")?,
                    "edgeId":row.try_get::<String>("", "edge_id")?,
                    "enqueueSeq":row.try_get::<i64>("", "enqueue_seq")?
                }),
                now,
            },
        )
        .await?;
    }
    Ok(())
}
