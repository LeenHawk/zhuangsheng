use std::{collections::HashSet, sync::Arc};

use sea_orm::ConnectionTrait;
use serde_json::{Value, json};
use zhuangsheng_core::{
    application::graph::{ApplyGraphCommand, GraphRevisionView, UpdateGraphDraftCommand},
    graph::{DraftNodeKind, GraphDraft},
    runtime::{RunContextCommand, StartRunCommand},
    scheduler::{BuiltinResult, ClaimedAttempt, FinalizeAttemptCommand, SchedulerWork},
};

use crate::{SqliteStore, graph::helpers::sql};

pub(super) struct JoinFixture {
    pub store: Arc<SqliteStore>,
    pub revision: GraphRevisionView,
    pub run_id: String,
}

pub(super) async fn fixture(
    key: &str,
    left: Vec<Value>,
    right: Vec<Value>,
    max_open_keys: u64,
    max_per_port: u64,
) -> JoinFixture {
    let store = Arc::new(super::store().await);
    let revision = join_graph(
        &store,
        key,
        left.len(),
        right.len(),
        max_open_keys,
        max_per_port,
    )
    .await;
    let run = store
        .start_run(StartRunCommand {
            graph_revision_id: revision.id.clone(),
            input: json!({"left":left,"right":right}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: format!("run-{key}"),
        })
        .await
        .unwrap();
    JoinFixture {
        store,
        revision,
        run_id: run.id,
    }
}

pub(super) async fn stage_join_inputs(fixture: &JoinFixture, now: i64) -> Vec<Activation> {
    let expected = fixture
        .revision
        .definition
        .nodes
        .iter()
        .filter(|node| matches!(&node.kind, DraftNodeKind::Input { .. }))
        .count();
    let mut inputs = Vec::new();
    while inputs.len() < expected {
        let work = fixture
            .store
            .claim_next_work("join-input-stage", now, now + 30_000)
            .await
            .unwrap()
            .unwrap();
        let SchedulerWork::Attempt(attempt) = work else {
            panic!("unexpected work while claiming join inputs: {work:?}")
        };
        assert!(matches!(&attempt.node.kind, DraftNodeKind::Input { .. }));
        inputs.push(attempt);
    }
    inputs.sort_by(|left, right| left.node.id.cmp(&right.node.id));
    for attempt in inputs {
        finish_builtin(&fixture.store, &attempt, attempt.inputs.clone(), now).await;
    }

    let target_count = incoming_join_edges(fixture).len();
    let expected_values = fixture
        .revision
        .definition
        .nodes
        .iter()
        .filter(|node| matches!(&node.kind, DraftNodeKind::Input { .. }))
        .count();
    let mut deferred = Vec::new();
    for _ in 0..512 {
        if pending_join_values(fixture).await == expected_values && !deferred.is_empty() {
            return deferred;
        }
        let Some(work) = fixture
            .store
            .claim_next_work("join-upstream-stage", now, now + 30_000)
            .await
            .unwrap()
        else {
            panic!("join upstream became idle before all values arrived")
        };
        match work {
            SchedulerWork::Attempt(attempt)
                if matches!(&attempt.node.kind, DraftNodeKind::Merge { .. }) =>
            {
                let value = attempt.inputs.values().next().cloned().unwrap();
                let output = attempt.node.outputs[0].name.clone();
                finish_builtin(&fixture.store, &attempt, [(output, value)].into(), now).await;
            }
            SchedulerWork::Activate {
                wakeup_id,
                run_id,
                node_id,
            } if node_id == "join" => deferred.push(Activation {
                wakeup_id,
                run_id,
                node_id,
            }),
            SchedulerWork::Activate {
                wakeup_id,
                run_id,
                node_id,
            } => fixture
                .store
                .activate_if_ready(&wakeup_id, &run_id, &node_id, now)
                .await
                .unwrap(),
            SchedulerWork::Settle { wakeup_id, run_id } => fixture
                .store
                .settle_run(&wakeup_id, &run_id, now)
                .await
                .unwrap(),
            SchedulerWork::Noop => {}
            other => panic!("unexpected join staging work: {other:?}"),
        }
    }
    panic!("join upstream staging exceeded step limit ({target_count} edges)")
}

pub(super) struct Activation {
    pub wakeup_id: String,
    pub run_id: String,
    pub node_id: String,
}

impl Activation {
    pub async fn run(self, store: &SqliteStore, now: i64) {
        store
            .activate_if_ready(&self.wakeup_id, &self.run_id, &self.node_id, now)
            .await
            .unwrap();
    }
}

pub(super) async fn pending_join_values(fixture: &JoinFixture) -> usize {
    let incoming = incoming_join_edges(fixture);
    fixture
        .store
        .db
        .query_all_raw(sql(
            "SELECT edge_id FROM edge_queue_values WHERE run_id = ? AND consumed_at IS NULL",
            vec![fixture.run_id.clone().into()],
        ))
        .await
        .unwrap()
        .iter()
        .filter(|row| incoming.contains(&row.try_get::<String>("", "edge_id").unwrap()))
        .count()
}

fn incoming_join_edges(fixture: &JoinFixture) -> HashSet<String> {
    fixture
        .revision
        .definition
        .edges
        .iter()
        .filter(|edge| edge.to.node_id == "join")
        .map(|edge| edge.id.clone())
        .collect()
}

async fn finish_builtin(
    store: &SqliteStore,
    attempt: &ClaimedAttempt,
    outputs: std::collections::BTreeMap<String, Value>,
    now: i64,
) {
    store.mark_attempt_running(attempt, now).await.unwrap();
    store
        .finalize_attempt(
            FinalizeAttemptCommand {
                wakeup_id: attempt.wakeup_id.clone(),
                attempt_id: attempt.attempt_id.clone(),
                worker_id: attempt.worker_id.clone(),
                lease_fence: attempt.lease_fence,
                run_control_epoch: attempt.run_control_epoch,
                result_idempotency_key: format!("stage:{}", attempt.attempt_id),
                result: BuiltinResult::Completed { outputs },
            },
            now,
        )
        .await
        .unwrap();
}

async fn join_graph(
    store: &SqliteStore,
    key: &str,
    left_count: usize,
    right_count: usize,
    max_open_keys: u64,
    max_per_port: u64,
) -> GraphRevisionView {
    let graph = super::graph(store, &format!("create-{key}")).await;
    let current = store.get_graph_draft(&graph.id).await.unwrap();
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    for (side, count) in [("left", left_count), ("right", right_count)] {
        for index in 0..count {
            let id = format!("{side}_{index:03}");
            nodes.push(json!({"id":id,"kind":"input","runInputSelector":{"type":"json_pointer","pointer":format!("/{side}/{index}")}}));
        }
        if count > 1 {
            nodes.push(json!({"id":format!("{side}_merge"),"kind":"merge","mode":"any","inputs":(0..count).map(|index| json!({"name":format!("v{index:03}")})).collect::<Vec<_>>() }));
            for index in 0..count {
                edges.push(json!({"from":{"nodeId":format!("{side}_{index:03}"),"output":"default"},"to":{"nodeId":format!("{side}_merge"),"input":format!("v{index:03}")}}));
            }
            edges.push(json!({"from":{"nodeId":format!("{side}_merge"),"output":"default"},"to":{"nodeId":"join","input":side}}));
        } else {
            edges.push(json!({"from":{"nodeId":format!("{side}_000"),"output":"default"},"to":{"nodeId":"join","input":side}}));
        }
    }
    nodes.push(json!({"id":"join","kind":"join_by_key","inputs":[{"name":"left"},{"name":"right"}],"keySelectors":{"left":"/id","right":"/id"},"maxOpenKeys":max_open_keys,"maxBufferedPerKeyPerPort":max_per_port}));
    nodes.push(json!({"id":"output","kind":"output","outputKey":"items"}));
    edges.push(json!({"from":{"nodeId":"join","output":"default"},"to":{"nodeId":"output","input":"default"}}));
    let document: GraphDraft = serde_json::from_value(json!({"graphId":graph.id,"nodes":nodes,"edges":edges,"outputContract":[{"key":"items","collection":"append","required":false}],"limits":{"maxCoordinatorBufferedValues":100}})).unwrap();
    let draft = store
        .update_graph_draft(UpdateGraphDraftCommand {
            graph_id: graph.id.clone(),
            expected_revision_token: current.revision_token,
            document,
            idempotency_key: format!("draft-{key}"),
        })
        .await
        .unwrap();
    store
        .apply_graph(ApplyGraphCommand {
            graph_id: graph.id,
            expected_revision_token: draft.revision_token,
            operation_taxonomy_version: 1,
            adapter_decoder_version: 1,
            idempotency_key: format!("apply-{key}"),
        })
        .await
        .unwrap()
}
