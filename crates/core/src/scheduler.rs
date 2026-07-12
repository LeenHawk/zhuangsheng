use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{
    application::ApplicationError,
    graph::{DraftNodeKind, GraphNode, LlmNodeExecutionSnapshot, RetryPolicy},
    llm::context::ResolvedContextBinding,
    router::{RouterControlSnapshot, RouterDecision, RouterDecisionError, evaluate_router},
    runtime::WaitKind,
    schema::JsonSchemaSpec,
};

#[derive(Debug, Clone)]
pub enum SchedulerWork {
    Noop,
    Attempt(Box<ClaimedAttempt>),
    Activate {
        wakeup_id: String,
        run_id: String,
        node_id: String,
    },
    Settle {
        wakeup_id: String,
        run_id: String,
    },
}

#[derive(Debug, Clone)]
pub struct ClaimedAttempt {
    pub wakeup_id: String,
    pub run_id: String,
    pub node_instance_id: String,
    pub attempt_id: String,
    pub worker_id: String,
    pub lease_fence: u64,
    pub run_control_epoch: u64,
    pub node: GraphNode,
    pub inputs: BTreeMap<String, Value>,
    pub memory: BTreeMap<String, Value>,
    pub router_control: Option<RouterControlSnapshot>,
    pub execution_snapshot: Option<LlmNodeExecutionSnapshot>,
    pub context_snapshot: Option<ClaimedContextSnapshot>,
    pub coordination: Option<ClaimedCoordinationSnapshot>,
    pub wait_resume: Option<ClaimedWaitResume>,
}

#[derive(Debug, Clone)]
pub struct ClaimedWaitResume {
    pub wait_id: String,
    pub kind: WaitKind,
    pub continuation_ref: String,
    pub continuation: Value,
    pub response_ref: String,
    pub response: Value,
}

#[derive(Debug, Clone)]
pub struct ClaimedCoordinationSnapshot {
    pub join_key: Value,
}

#[derive(Debug, Clone)]
pub struct ClaimedContextSnapshot {
    pub bindings: BTreeMap<String, ResolvedContextBinding>,
    pub read_set_ref: String,
    pub read_set_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BuiltinResult {
    Completed {
        outputs: BTreeMap<String, Value>,
    },
    Expanded {
        output: String,
        values: Vec<Value>,
    },
    Failed {
        code: String,
        safe_message: String,
    },
    RouterDecision {
        decision: RouterDecision,
    },
    RouterFailed {
        error: RouterDecisionError,
    },
    Waiting {
        wait: Box<ExternalWaitRequest>,
        continuation: Value,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitTimeoutPolicy {
    Fail,
    ResumeWithTimeout,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalWaitRequest {
    pub kind: WaitKind,
    pub request: Value,
    pub response_schema: Option<JsonSchemaSpec>,
    pub correlation_key: Option<String>,
    pub deadline_at: Option<i64>,
    pub on_timeout: WaitTimeoutPolicy,
}

#[derive(Debug, Clone)]
pub enum LlmAttemptExecution {
    Finalize(BuiltinResult),
    Handled,
}

#[async_trait]
pub trait LlmAttemptExecutor: Send + Sync {
    async fn execute_llm_attempt(
        &self,
        attempt: &ClaimedAttempt,
        now_ms: i64,
    ) -> Result<LlmAttemptExecution, ApplicationError>;
}

#[derive(Debug, Clone)]
pub struct FinalizeAttemptCommand {
    pub wakeup_id: String,
    pub attempt_id: String,
    pub worker_id: String,
    pub lease_fence: u64,
    pub run_control_epoch: u64,
    pub result_idempotency_key: String,
    pub result: BuiltinResult,
}

#[async_trait]
pub trait SchedulerStore: Send + Sync {
    async fn process_due_timers(&self, now_ms: i64) -> Result<u64, ApplicationError>;
    async fn recover_expired_leases(&self, now_ms: i64) -> Result<u64, ApplicationError>;
    async fn claim_next_work(
        &self,
        worker_id: &str,
        now_ms: i64,
        lease_until: i64,
    ) -> Result<Option<SchedulerWork>, ApplicationError>;
    async fn mark_attempt_running(
        &self,
        attempt: &ClaimedAttempt,
        now_ms: i64,
    ) -> Result<(), ApplicationError>;
    async fn finalize_attempt(
        &self,
        command: FinalizeAttemptCommand,
        now_ms: i64,
    ) -> Result<(), ApplicationError>;
    async fn activate_if_ready(
        &self,
        wakeup_id: &str,
        run_id: &str,
        node_id: &str,
        now_ms: i64,
    ) -> Result<(), ApplicationError>;
    async fn settle_run(
        &self,
        wakeup_id: &str,
        run_id: &str,
        now_ms: i64,
    ) -> Result<(), ApplicationError>;
    async fn checkpoint_run(&self, run_id: &str, now_ms: i64) -> Result<(), ApplicationError>;
}

pub struct Scheduler {
    store: Arc<dyn SchedulerStore>,
    llm_executor: Option<Arc<dyn LlmAttemptExecutor>>,
    worker_id: String,
    lease_ms: i64,
}

impl Scheduler {
    pub fn new(store: Arc<dyn SchedulerStore>, worker_id: impl Into<String>) -> Self {
        Self {
            store,
            llm_executor: None,
            worker_id: worker_id.into(),
            lease_ms: 30_000,
        }
    }

    pub fn with_llm_executor(mut self, executor: Arc<dyn LlmAttemptExecutor>) -> Self {
        self.llm_executor = Some(executor);
        self
    }

    pub async fn run_one(&self, now_ms: i64) -> Result<bool, ApplicationError> {
        let timer_work = self.store.process_due_timers(now_ms).await?;
        self.store.recover_expired_leases(now_ms).await?;
        let lease_until = now_ms.saturating_add(self.lease_ms);
        let Some(work) = self
            .store
            .claim_next_work(&self.worker_id, now_ms, lease_until)
            .await?
        else {
            return Ok(timer_work > 0);
        };
        let checkpoint_run_id = match work {
            SchedulerWork::Noop => None,
            SchedulerWork::Attempt(attempt) => {
                let run_id = attempt.run_id.clone();
                self.store.mark_attempt_running(&attempt, now_ms).await?;
                if let LlmAttemptExecution::Finalize(result) =
                    self.execute_attempt(&attempt, now_ms).await?
                {
                    self.store
                        .finalize_attempt(
                            FinalizeAttemptCommand {
                                wakeup_id: attempt.wakeup_id.clone(),
                                attempt_id: attempt.attempt_id.clone(),
                                worker_id: attempt.worker_id.clone(),
                                lease_fence: attempt.lease_fence,
                                run_control_epoch: attempt.run_control_epoch,
                                result_idempotency_key: format!(
                                    "result:{}:{}",
                                    attempt.attempt_id, attempt.lease_fence
                                ),
                                result,
                            },
                            now_ms,
                        )
                        .await?;
                }
                Some(run_id)
            }
            SchedulerWork::Activate {
                wakeup_id,
                run_id,
                node_id,
            } => {
                self.store
                    .activate_if_ready(&wakeup_id, &run_id, &node_id, now_ms)
                    .await?;
                Some(run_id)
            }
            SchedulerWork::Settle { wakeup_id, run_id } => {
                self.store.settle_run(&wakeup_id, &run_id, now_ms).await?;
                Some(run_id)
            }
        };
        if let Some(run_id) = checkpoint_run_id {
            self.store.checkpoint_run(&run_id, now_ms).await?;
        }
        Ok(true)
    }

    pub async fn run_until_idle(
        &self,
        now_ms: i64,
        max_steps: usize,
    ) -> Result<usize, ApplicationError> {
        let mut steps = 0;
        while steps < max_steps && self.run_one(now_ms).await? {
            steps += 1;
        }
        Ok(steps)
    }

    async fn execute_attempt(
        &self,
        attempt: &ClaimedAttempt,
        now_ms: i64,
    ) -> Result<LlmAttemptExecution, ApplicationError> {
        if matches!(attempt.node.kind, DraftNodeKind::Llm { .. })
            && attempt.execution_snapshot.is_some()
            && let Some(executor) = &self.llm_executor
        {
            return executor.execute_llm_attempt(attempt, now_ms).await;
        }
        Ok(LlmAttemptExecution::Finalize(execute_builtin(attempt)))
    }
}

fn execute_builtin(attempt: &ClaimedAttempt) -> BuiltinResult {
    match &attempt.node.kind {
        DraftNodeKind::Input { .. } => BuiltinResult::Completed {
            outputs: attempt.inputs.clone(),
        },
        DraftNodeKind::Output { .. } => BuiltinResult::Completed {
            outputs: BTreeMap::new(),
        },
        DraftNodeKind::Llm { .. } if attempt.execution_snapshot.is_none() => {
            BuiltinResult::Failed {
                code: "llm_execution_snapshot_missing".into(),
                safe_message: "LLM execution snapshot is missing".into(),
            }
        }
        DraftNodeKind::Llm { .. } => BuiltinResult::Failed {
            code: "llm_executor_unavailable".into(),
            safe_message: "LLM execution adapter is not configured".into(),
        },
        DraftNodeKind::Router { .. } => {
            let Some(control) = attempt.router_control.clone() else {
                return BuiltinResult::RouterFailed {
                    error: RouterDecisionError {
                        code: "router_control_snapshot_missing".into(),
                        safe_message: "Router control snapshot is missing".into(),
                        rule_id: None,
                        evaluated_rule_ids: Vec::new(),
                    },
                };
            };
            let memory = serde_json::Value::Object(
                attempt
                    .memory
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect(),
            );
            match evaluate_router(&attempt.node, &attempt.inputs, &memory, control) {
                Ok(decision) => BuiltinResult::RouterDecision { decision },
                Err(error) => BuiltinResult::RouterFailed { error },
            }
        }
        DraftNodeKind::Merge { .. } => {
            let Some(value) = attempt.inputs.values().next().cloned() else {
                return BuiltinResult::Failed {
                    code: "merge_input_missing".into(),
                    safe_message: "Merge activation has no selected input".into(),
                };
            };
            let Some(output) = attempt.node.outputs.first() else {
                return BuiltinResult::Failed {
                    code: "merge_output_missing".into(),
                    safe_message: "Merge node has no output".into(),
                };
            };
            BuiltinResult::Completed {
                outputs: [(output.name.clone(), value)].into(),
            }
        }
        DraftNodeKind::JoinByKey { .. } => {
            let Some(coordination) = &attempt.coordination else {
                return BuiltinResult::Failed {
                    code: "join_coordination_snapshot_missing".into(),
                    safe_message: "Join coordination snapshot is missing".into(),
                };
            };
            let Some(output) = attempt.node.outputs.first() else {
                return BuiltinResult::Failed {
                    code: "join_output_missing".into(),
                    safe_message: "Join node has no output".into(),
                };
            };
            let values: serde_json::Map<String, Value> = attempt
                .node
                .inputs
                .iter()
                .filter_map(|input| {
                    attempt
                        .inputs
                        .get(&input.name)
                        .cloned()
                        .map(|value| (input.name.clone(), value))
                })
                .collect();
            BuiltinResult::Completed {
                outputs: [(
                    output.name.clone(),
                    serde_json::json!({"key":coordination.join_key,"values":values}),
                )]
                .into(),
            }
        }
        DraftNodeKind::Aggregator { .. } => BuiltinResult::Failed {
            code: "aggregator_storage_execution_required".into(),
            safe_message: "Aggregator must execute through durable storage coordination".into(),
        },
        DraftNodeKind::Expand { max_items } => {
            let Some(value) = attempt.inputs.values().next() else {
                return BuiltinResult::Failed {
                    code: "expand_input_missing".into(),
                    safe_message: "Expand activation has no selected input".into(),
                };
            };
            let Some(items) = value.as_array() else {
                return BuiltinResult::Failed {
                    code: "expand_input_not_array".into(),
                    safe_message: "Expand input must be an array".into(),
                };
            };
            if items.len() as u64 > *max_items {
                return BuiltinResult::Failed {
                    code: "expand_item_limit_exceeded".into(),
                    safe_message: "Expand input exceeds its item limit".into(),
                };
            }
            let Some(output) = attempt.node.outputs.first() else {
                return BuiltinResult::Failed {
                    code: "expand_output_missing".into(),
                    safe_message: "Expand node has no output".into(),
                };
            };
            BuiltinResult::Expanded {
                output: output.name.clone(),
                values: items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| serde_json::json!({"index":index,"item":item}))
                    .collect(),
            }
        }
    }
}

pub fn retry_delay_ms(policy: &RetryPolicy, node_instance_id: &str, retry_ordinal: u64) -> u64 {
    let mut base = policy.initial_backoff_ms.min(policy.max_backoff_ms);
    for _ in 0..retry_ordinal {
        base = ((base as u128).saturating_mul(policy.multiplier_micros as u128) / 1_000_000)
            .min(policy.max_backoff_ms as u128) as u64;
    }
    let span = ((base as u128) * (policy.jitter_ratio_micros as u128) / 1_000_000)
        .min(u64::MAX as u128) as u64;
    if span == 0 {
        return base;
    }
    let mut hash = Sha256::new();
    hash.update(b"retry-jitter/v1\0");
    hash.update(node_instance_id.as_bytes());
    hash.update([0]);
    hash.update(retry_ordinal.to_string().as_bytes());
    let digest = hash.finalize();
    let random = u64::from_be_bytes(digest[..8].try_into().expect("sha256 prefix"));
    let width = (span as u128).saturating_mul(2).saturating_add(1);
    let position = (random as u128).saturating_mul(width) >> 64;
    let adjusted = if position >= span as u128 {
        (base as u128).saturating_add(position - span as u128)
    } else {
        (base as u128).saturating_sub(span as u128 - position)
    };
    adjusted.min(policy.max_backoff_ms as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::RefreshReadSet;

    #[test]
    fn retry_jitter_is_deterministic_and_bounded() {
        let policy = RetryPolicy {
            max_retries: 5,
            retry_on: vec!["node_timeout".into()],
            initial_backoff_ms: 100,
            multiplier_micros: 2_000_000,
            max_backoff_ms: 1_000,
            jitter_ratio_micros: 250_000,
            refresh_read_set: RefreshReadSet::Never,
        };
        let first = retry_delay_ms(&policy, "instance-1", 2);
        assert_eq!(first, retry_delay_ms(&policy, "instance-1", 2));
        assert!((300..=500).contains(&first));
        assert!(retry_delay_ms(&policy, "instance-1", 10) <= 1_000);
    }
}
