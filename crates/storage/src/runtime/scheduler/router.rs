use std::collections::BTreeMap;

use sea_orm::ConnectionTrait;
use serde_json::{Value, json};
use zhuangsheng_core::{
    canonical,
    graph::{DraftNodeKind, GraphNode},
    router::{RouterControlSnapshot, RouterDecision, RouterDecisionError, RouterLimitReason},
};

use crate::{
    StorageError, StorageResult,
    graph::helpers::{put_inline_object, sql},
};

use super::{
    emit::StoredValue,
    events::{Event, add_object_ref, append_event},
};

pub(super) async fn create_control_snapshot<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    node: &GraphNode,
    instance_id: &str,
    now: i64,
) -> StorageResult<Option<RouterControlSnapshot>> {
    let DraftNodeKind::Router { limits, .. } = &node.kind else {
        return Ok(None);
    };
    let existing = connection
        .query_one(sql(
            "SELECT visits, first_visited_at FROM router_controls WHERE run_id = ? AND node_id = ?",
            vec![run_id.into(), node.id.clone().into()],
        ))
        .await?;
    let (visits, first_visited_at) = if let Some(row) = existing {
        let old: i64 = row.try_get("", "visits")?;
        let first: i64 = row.try_get("", "first_visited_at")?;
        let visits = old
            .checked_add(1)
            .ok_or_else(|| StorageError::Integrity("Router visits overflow".into()))?;
        let updated = connection.execute(sql(
            "UPDATE router_controls SET visits = ?, updated_at = ? WHERE run_id = ? AND node_id = ? AND visits = ?",
            vec![visits.into(), now.into(), run_id.into(), node.id.clone().into(), old.into()],
        )).await?;
        if updated.rows_affected() != 1 {
            return Err(StorageError::Conflict("router_control_visit"));
        }
        (visits, first)
    } else {
        connection.execute(sql(
            "INSERT INTO router_controls (run_id, node_id, visits, first_visited_at, updated_at) VALUES (?, ?, 1, ?, ?)",
            vec![run_id.into(), node.id.clone().into(), now.into(), now.into()],
        )).await?;
        (1, now)
    };
    let elapsed = u64::try_from(now.saturating_sub(first_visited_at).max(0))
        .map_err(|_| StorageError::Integrity("invalid Router elapsed time".into()))?;
    let visits_u64 = u64::try_from(visits)
        .map_err(|_| StorageError::Integrity("invalid Router visit count".into()))?;
    let mut reasons = Vec::new();
    if let Some(limits) = limits {
        if limits
            .max_visits_per_run
            .is_some_and(|maximum| visits_u64 > maximum)
        {
            reasons.push(RouterLimitReason::MaxVisits);
        }
        if limits
            .timeout_ms_per_run
            .is_some_and(|timeout| elapsed >= timeout)
        {
            reasons.push(RouterLimitReason::Timeout);
        }
    }
    let snapshot = RouterControlSnapshot {
        visits: visits_u64,
        first_visited_at,
        decision_at: now,
        elapsed_ms: elapsed,
        limit_reasons: reasons,
    };
    connection.execute(sql(
        "INSERT INTO router_activation_controls (node_instance_id, run_id, node_id, visits, first_visited_at, decision_at, elapsed_ms, limit_reasons_json) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        vec![instance_id.into(), run_id.into(), node.id.clone().into(), visits.into(), first_visited_at.into(), now.into(), (elapsed as i64).into(), canonical::to_string(&snapshot.limit_reasons)?.into()],
    )).await?;
    Ok(Some(snapshot))
}

pub(super) async fn load_control_snapshot<C: ConnectionTrait>(
    connection: &C,
    node: &GraphNode,
    instance_id: &str,
) -> StorageResult<Option<RouterControlSnapshot>> {
    if !matches!(&node.kind, DraftNodeKind::Router { .. }) {
        return Ok(None);
    }
    let row = connection.query_one(sql(
        "SELECT visits, first_visited_at, decision_at, elapsed_ms, limit_reasons_json FROM router_activation_controls WHERE node_instance_id = ?",
        vec![instance_id.into()],
    )).await?.ok_or_else(|| StorageError::Integrity("Router control snapshot missing".into()))?;
    let reasons_json: String = row.try_get("", "limit_reasons_json")?;
    Ok(Some(RouterControlSnapshot {
        visits: to_u64(row.try_get("", "visits")?, "visits")?,
        first_visited_at: row.try_get("", "first_visited_at")?,
        decision_at: row.try_get("", "decision_at")?,
        elapsed_ms: to_u64(row.try_get("", "elapsed_ms")?, "elapsed")?,
        limit_reasons: serde_json::from_str(&reasons_json)
            .map_err(|error| StorageError::Integrity(error.to_string()))?,
    }))
}

pub(super) async fn persist_decision<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    instance_id: &str,
    attempt_id: &str,
    decision: &RouterDecision,
    outputs: &BTreeMap<String, StoredValue>,
    now: i64,
) -> StorageResult<()> {
    let output_refs: BTreeMap<_, _> = decision
        .selected_ports
        .iter()
        .map(|port| {
            outputs
                .get(port)
                .map(|value| (port.clone(), value.id.clone()))
                .ok_or_else(|| StorageError::Integrity("Router output ref missing".into()))
        })
        .collect::<Result<_, _>>()?;
    let payload_ref = decision
        .selected_ports
        .first()
        .and_then(|port| outputs.get(port))
        .ok_or_else(|| StorageError::Integrity("Router payload ref missing".into()))?
        .id
        .clone();
    let record = json!({
        "schemaVersion": 1,
        "dslVersion": decision.dsl_version,
        "matchedRuleIds": decision.matched_rule_ids,
        "evaluatedRuleIds": decision.evaluated_rule_ids,
        "selectedPorts": decision.selected_ports,
        "reason": decision.reason,
        "control": decision.control,
        "readSetRefs": [],
        "payloadRef": payload_ref,
        "outputRefs": output_refs,
    });
    persist_outcome(
        connection,
        run_id,
        instance_id,
        attempt_id,
        "decision",
        "router.decision",
        &record,
        now,
    )
    .await
}

pub(super) async fn persist_error<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    instance_id: &str,
    attempt_id: &str,
    error: &RouterDecisionError,
    now: i64,
) -> StorageResult<()> {
    persist_outcome(
        connection,
        run_id,
        instance_id,
        attempt_id,
        "error",
        "router.decision_error",
        &json!({
            "schemaVersion": 1,
            "code": error.code,
            "safeMessage": error.safe_message,
            "ruleId": error.rule_id,
            "evaluatedRuleIds": error.evaluated_rule_ids,
        }),
        now,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn persist_outcome<C: ConnectionTrait>(
    connection: &C,
    run_id: &str,
    instance_id: &str,
    attempt_id: &str,
    outcome: &str,
    event_type: &str,
    record: &Value,
    now: i64,
) -> StorageResult<()> {
    let object_id = put_inline_object(connection, &canonical::to_vec(record)?, now).await?;
    connection.execute(sql(
        "INSERT INTO router_decisions (node_instance_id, attempt_id, outcome, decision_object_id, created_at) VALUES (?, ?, ?, ?, ?)",
        vec![instance_id.into(), attempt_id.into(), outcome.into(), object_id.clone().into(), now.into()],
    )).await?;
    add_object_ref(
        connection,
        &object_id,
        "node_instance",
        instance_id,
        "router_decision",
        now,
    )
    .await?;
    append_event(
        connection,
        Event {
            run_id,
            event_type,
            importance: "critical",
            node_instance_id: Some(instance_id),
            attempt_id: Some(attempt_id),
            payload: json!({"schemaVersion":1,"decisionRef":object_id}),
            now,
        },
    )
    .await?;
    Ok(())
}

fn to_u64(value: i64, field: &str) -> StorageResult<u64> {
    u64::try_from(value)
        .map_err(|_| StorageError::Integrity(format!("invalid Router {field} snapshot")))
}
