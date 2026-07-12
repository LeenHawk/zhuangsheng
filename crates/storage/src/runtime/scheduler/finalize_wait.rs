use sea_orm::ConnectionTrait;
use serde_json::{Value, json};
use zhuangsheng_core::{
    canonical,
    runtime::WaitKind,
    scheduler::{ExternalWaitRequest, FinalizeAttemptCommand, WaitTimeoutPolicy},
    schema,
};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{new_id, put_inline_object, sql},
};

use super::{
    attempt_finish::{fail_attempt, settle_interrupt_after_attempt},
    attempt_state::AttemptState,
    events::{Event, add_object_ref, append_event, enqueue_wakeup, finish_wakeup},
};

pub(super) async fn finalize<C: ConnectionTrait>(
    connection: &C,
    state: &AttemptState,
    command: &FinalizeAttemptCommand,
    wait: &ExternalWaitRequest,
    continuation: &Value,
    now: i64,
) -> StorageResult<()> {
    let compiled = match validate(wait, continuation, state.run_deadline, now) {
        Ok(compiled) => compiled,
        Err(error) => {
            fail_attempt(
                connection,
                state,
                command,
                "executor_wait_contract_invalid",
                &error.to_string(),
                now,
            )
            .await?;
            return Ok(());
        }
    };
    let request_ref =
        put_inline_object(connection, &canonical::to_vec(&wait.request)?, now).await?;
    let continuation_ref =
        put_inline_object(connection, &canonical::to_vec(continuation)?, now).await?;
    let (schema_ref, compilation_ref) = if let Some((spec, compilation)) = compiled {
        (
            Some(put_inline_object(connection, &canonical::to_vec(spec)?, now).await?),
            Some(put_inline_object(connection, &canonical::to_vec(&compilation)?, now).await?),
        )
    } else {
        (None, None)
    };
    let wait_id = new_id("wait");
    settle_attempt(connection, state, command, &continuation_ref, now).await?;
    connection.execute_raw(sql(
        "INSERT INTO node_waits (id,run_id,node_instance_id,node_attempt_id,kind,correlation_key,request_object_id,continuation_object_id,response_schema_object_id,response_schema_compilation_object_id,deadline_at,on_timeout,status,created_at) VALUES (?,?,?,?,?,?,?,?,?,?,?,?, 'open',?)",
        vec![
            wait_id.clone().into(), state.run_id.clone().into(),
            state.node_instance_id.clone().into(), command.attempt_id.clone().into(),
            kind_name(wait.kind).into(), wait.correlation_key.clone().into(),
            request_ref.clone().into(), continuation_ref.clone().into(), schema_ref.clone().into(),
            compilation_ref.clone().into(), wait.deadline_at.into(), timeout_name(wait.on_timeout).into(),
            now.into(),
        ],
    )).await?;
    connection
        .execute_raw(sql(
            "UPDATE run_execution_counters SET open_waits=open_waits+1 WHERE run_id=?",
            vec![state.run_id.clone().into()],
        ))
        .await?;
    add_refs(
        connection,
        &wait_id,
        &request_ref,
        &continuation_ref,
        schema_ref.as_deref(),
        compilation_ref.as_deref(),
        now,
    )
    .await?;
    let seq = append_event(connection, Event {
        run_id: &state.run_id,
        event_type: "node.waiting",
        importance: "critical",
        node_instance_id: Some(&state.node_instance_id),
        attempt_id: Some(&command.attempt_id),
        payload: json!({"schemaVersion":1,"nodeId":state.node_id,"waitId":wait_id,"kind":kind_name(wait.kind),"deadlineAt":wait.deadline_at}),
        now,
    }).await?;
    finish_wakeup(connection, &command.wakeup_id).await?;
    enqueue_wakeup(
        connection,
        &state.run_id,
        None,
        "settle_run",
        seq,
        &format!("settle-wait:{wait_id}"),
        now,
    )
    .await?;
    settle_interrupt_after_attempt(connection, &state.run_id, now).await?;
    Ok(())
}

fn validate<'a>(
    wait: &'a ExternalWaitRequest,
    continuation: &Value,
    run_deadline: i64,
    now: i64,
) -> StorageResult<Option<(&'a schema::JsonSchemaSpec, schema::SchemaCompilationDraft)>> {
    if !matches!(
        wait.kind,
        WaitKind::HumanResponse | WaitKind::Webhook | WaitKind::Timer | WaitKind::ExternalJob
    ) || wait
        .correlation_key
        .as_ref()
        .is_some_and(|key| key.is_empty() || key.len() > 256)
        || wait
            .deadline_at
            .is_some_and(|deadline| deadline <= now || deadline > run_deadline)
        || continuation.is_null()
    {
        return Err(StorageError::InvalidArgument(
            "executor wait request is invalid".into(),
        ));
    }
    let Some(spec) = &wait.response_schema else {
        return Ok(None);
    };
    let compilation = schema::compile(spec)?;
    if wait.on_timeout == WaitTimeoutPolicy::ResumeWithTimeout {
        schema::validate(spec, &json!({"timedOut":true}))?;
    }
    Ok(Some((spec, compilation)))
}

async fn settle_attempt<C: ConnectionTrait>(
    connection: &C,
    state: &AttemptState,
    command: &FinalizeAttemptCommand,
    continuation_ref: &str,
    now: i64,
) -> StorageResult<()> {
    let attempt = connection.execute_raw(sql(
        "UPDATE node_attempts SET status='waiting',result_idempotency_key=?,continuation_object_id=?,worker_id=NULL,lease_until=NULL,finished_at=? WHERE id=? AND status='running' AND worker_id=? AND lease_fence=? AND run_control_epoch=?",
        vec![command.result_idempotency_key.clone().into(), continuation_ref.into(), now.into(), command.attempt_id.clone().into(), command.worker_id.clone().into(), (command.lease_fence as i64).into(), (command.run_control_epoch as i64).into()],
    )).await?;
    if attempt.rows_affected() != 1 {
        return Err(StorageError::Conflict("attempt_fence"));
    }
    connection.execute_raw(sql("UPDATE runtime_timers SET status='cancelled' WHERE node_attempt_id=? AND kind='attempt_deadline' AND status='pending'", vec![command.attempt_id.clone().into()])).await?;
    if connection.execute_raw(sql("UPDATE node_instances SET status='waiting',updated_at=? WHERE id=? AND status='running'", vec![now.into(), state.node_instance_id.clone().into()])).await?.rows_affected() != 1 {
        return Err(StorageError::Conflict("node_instance_status"));
    }
    Ok(())
}

async fn add_refs<C: ConnectionTrait>(
    connection: &C,
    wait_id: &str,
    request: &str,
    continuation: &str,
    schema: Option<&str>,
    compilation: Option<&str>,
    now: i64,
) -> StorageResult<()> {
    for (object, role) in [(request, "request"), (continuation, "continuation")] {
        add_object_ref(connection, object, "node_wait", wait_id, role, now).await?;
    }
    if let Some(object) = schema {
        add_object_ref(
            connection,
            object,
            "node_wait",
            wait_id,
            "response_schema",
            now,
        )
        .await?;
    }
    if let Some(object) = compilation {
        add_object_ref(
            connection,
            object,
            "node_wait",
            wait_id,
            "response_schema_compilation",
            now,
        )
        .await?;
    }
    Ok(())
}

fn kind_name(kind: WaitKind) -> &'static str {
    match kind {
        WaitKind::HumanResponse => "human_response",
        WaitKind::Approval => "approval",
        WaitKind::Webhook => "webhook",
        WaitKind::Timer => "timer",
        WaitKind::ExternalJob => "external_job",
        WaitKind::EffectResolution => "effect_resolution",
        WaitKind::SecretStoreUnlocked => "secret_store_unlocked",
    }
}

fn timeout_name(policy: WaitTimeoutPolicy) -> &'static str {
    match policy {
        WaitTimeoutPolicy::Fail => "fail",
        WaitTimeoutPolicy::ResumeWithTimeout => "resume_with_timeout",
    }
}
