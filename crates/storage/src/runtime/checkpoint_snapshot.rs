use sea_orm::ConnectionTrait;
use serde_json::{Value, json};

use crate::{StorageError, StorageResult, graph::helpers::sql};

pub(super) struct SnapshotIdentity<'a> {
    pub run_id: &'a str,
    pub graph_revision_id: &'a str,
    pub context_id: &'a str,
    pub branch_id: &'a str,
    pub head_commit_id: &'a str,
    pub status: &'a str,
    pub control_epoch: u64,
    pub through_seq: u64,
}

pub(super) async fn build_snapshot<C: ConnectionTrait>(
    connection: &C,
    identity: SnapshotIdentity<'_>,
) -> StorageResult<Value> {
    let run_id = identity.run_id;
    Ok(json!({
        "schemaVersion": 1,
        "throughSeq": identity.through_seq,
        "run": {
            "id": run_id,
            "graphRevisionId": identity.graph_revision_id,
            "contextId": identity.context_id,
            "branchId": identity.branch_id,
            "headCommitId": identity.head_commit_id,
            "status": identity.status,
            "controlEpoch": identity.control_epoch,
        },
        "counters": object(connection, "SELECT json_object('nextEnqueueSeq',next_enqueue_seq,'nextOutputSeq',next_output_seq,'totalActivations',total_activations,'totalAttempts',total_attempts,'totalQueueValues',total_queue_values,'pendingQueueValues',pending_queue_values,'openWaits',open_waits,'coordinatorBufferedValues',coordinator_buffered_values) AS data FROM run_execution_counters WHERE run_id = ?", run_id).await?,
        "nodeInstances": array(connection, "SELECT json_object('id',id,'nodeId',node_id,'activationSeq',activation_seq,'status',status,'updatedAt',updated_at) AS item FROM node_instances WHERE run_id = ? ORDER BY node_id,activation_seq,id", run_id).await?,
        "nodeAttempts": array(connection, "SELECT json_object('id',a.id,'nodeInstanceId',a.node_instance_id,'attemptNo',a.attempt_no,'status',a.status,'runControlEpoch',a.run_control_epoch,'leaseFence',a.lease_fence,'workerId',a.worker_id,'leaseUntil',a.lease_until) AS item FROM node_attempts a JOIN node_instances ni ON ni.id=a.node_instance_id WHERE ni.run_id = ? ORDER BY a.node_instance_id,a.attempt_no,a.id", run_id).await?,
        "edgeQueue": array(connection, "SELECT json_object('id',id,'edgeId',edge_id,'enqueueSeq',enqueue_seq,'producerInstanceId',producer_instance_id,'consumedByInstanceId',consumed_by_instance_id,'consumedAt',consumed_at) AS item FROM edge_queue_values WHERE run_id = ? ORDER BY enqueue_seq,id", run_id).await?,
        "waits": array(connection, "SELECT json_object('id',id,'nodeInstanceId',node_instance_id,'nodeAttemptId',node_attempt_id,'kind',kind,'status',status,'acceptedDeliveryId',accepted_delivery_id) AS item FROM node_waits WHERE run_id = ? ORDER BY created_at,id", run_id).await?,
        "waitBlockers": array(connection, "SELECT json_object('waitId',wb.wait_id,'kind',wb.blocker_kind,'id',wb.blocker_id,'order',wb.blocker_order,'status',wb.status) AS item FROM wait_blockers wb JOIN node_waits w ON w.id=wb.wait_id WHERE w.run_id = ? ORDER BY wb.wait_id,wb.blocker_order", run_id).await?,
        "timers": array(connection, "SELECT json_object('id',id,'kind',kind,'nodeAttemptId',node_attempt_id,'dueAt',due_at,'status',status) AS item FROM runtime_timers WHERE run_id = ? ORDER BY due_at,id", run_id).await?,
        "wakeups": array(connection, "SELECT json_object('id',id,'nodeId',node_id,'kind',kind,'causedBySeq',caused_by_seq,'status',status,'availableAt',available_at) AS item FROM scheduler_wakeups WHERE run_id = ? ORDER BY created_at,id", run_id).await?,
        "coordination": array(connection, "SELECT json_object('id',id,'nodeId',node_id,'inputPort',input_port,'queueValueId',queue_value_id,'enqueueSeq',enqueue_seq,'status',status) AS item FROM coordination_buffer_items WHERE run_id = ? ORDER BY node_id,enqueue_seq,id", run_id).await?,
        "aggregationWindows": array(connection, "SELECT json_object('id',id,'nodeId',node_id,'nodeInstanceId',node_instance_id,'status',status,'itemCount',item_count,'deadlineAt',deadline_at,'closeReason',close_reason) AS item FROM aggregation_windows WHERE run_id = ? ORDER BY opened_at,id", run_id).await?,
        "effects": array(connection, "SELECT json_object('id',e.id,'nodeInstanceId',e.node_instance_id,'classification',e.classification,'status',e.status,'modelCallId',e.model_call_id,'countCallId',e.count_call_id,'toolCallId',e.tool_call_id) AS item FROM effects e JOIN node_instances ni ON ni.id=e.node_instance_id WHERE ni.run_id = ? ORDER BY e.created_at,e.id", run_id).await?,
        "effectAttempts": array(connection, "SELECT json_object('id',ea.id,'effectId',ea.effect_id,'attemptNo',ea.attempt_no,'status',ea.status,'invokingNodeAttemptId',ea.invoking_node_attempt_id) AS item FROM effect_attempts ea JOIN effects e ON e.id=ea.effect_id JOIN node_instances ni ON ni.id=e.node_instance_id WHERE ni.run_id = ? ORDER BY e.created_at,ea.attempt_no,ea.id", run_id).await?,
    }))
}

async fn object<C: ConnectionTrait>(
    connection: &C,
    query: &str,
    run_id: &str,
) -> StorageResult<Value> {
    let row = connection
        .query_one_raw(sql(query, vec![run_id.into()]))
        .await?
        .ok_or_else(|| StorageError::Integrity("runtime checkpoint counters missing".into()))?;
    parse(row.try_get::<String>("", "data")?)
}

async fn array<C: ConnectionTrait>(
    connection: &C,
    query: &str,
    run_id: &str,
) -> StorageResult<Value> {
    let rows = connection
        .query_all_raw(sql(query, vec![run_id.into()]))
        .await?;
    let values = rows
        .into_iter()
        .map(|row| parse(row.try_get::<String>("", "item")?))
        .collect::<StorageResult<Vec<_>>>()?;
    Ok(Value::Array(values))
}

fn parse(value: String) -> StorageResult<Value> {
    serde_json::from_str(&value).map_err(|error| StorageError::Integrity(error.to_string()))
}
