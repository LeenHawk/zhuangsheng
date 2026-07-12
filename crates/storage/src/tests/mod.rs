mod config;
mod context_patch;
mod context_replay;
mod graph_apply;
mod graph_commands;
mod llm_count_ledger;
mod llm_count_recovery;
mod llm_effect_resolution;
mod llm_graph;
mod llm_initial_model_call;
mod llm_ledger;
mod llm_ledger_recovery;
mod llm_ledger_retry;
mod llm_memory_search_support;
mod llm_memory_search_tool;
mod llm_output_repair;
mod llm_started_recovery;
mod llm_static_context;
mod llm_stream_chunk;
mod llm_terminal_fencing;
mod llm_tool_approval;
mod llm_tool_approval_support;
mod llm_tool_batch_recovery;
mod llm_tool_ledger;
mod llm_tool_support;
mod llm_tool_test_helpers;
mod memory;
mod runtime_control;
mod runtime_join_by_key;
mod runtime_join_support;
mod runtime_merge;
mod runtime_router;
mod runtime_router_long_memory;
mod runtime_router_support;
mod runtime_scheduler;
mod runtime_start;
mod runtime_timers;
mod secret_runtime_wait;
mod secret_store;
mod tool_registry;

use serde_json::json;
use zhuangsheng_core::{
    application::graph::{
        ApplyGraphCommand, CreateGraphCommand, GraphRevisionView, GraphView,
        UpdateGraphDraftCommand,
    },
    graph::{DraftNodeKind, GraphDraft, InputSelector, OutputPortDefinition},
    schema::{DIALECT_2020_12, JsonSchemaLimits, JsonSchemaSpec},
};

use crate::SqliteStore;

async fn store() -> SqliteStore {
    SqliteStore::connect("sqlite::memory:").await.unwrap()
}

async fn graph(store: &SqliteStore, key: &str) -> GraphView {
    store
        .create_graph(CreateGraphCommand {
            name: "Story Graph".into(),
            idempotency_key: key.into(),
        })
        .await
        .unwrap()
        .graph
}

fn valid_draft(graph_id: &str, name: &str) -> GraphDraft {
    serde_json::from_value(json!({
        "graphId": graph_id,
        "name": name,
        "nodes": [
            {"id":"input","kind":"input","runInputSelector":{"type":"whole_value"}},
            {"id":"output","kind":"output","outputKey":"reply"}
        ],
        "edges": [{
            "from":{"nodeId":"input","output":"default"},
            "to":{"nodeId":"output","input":"default"}
        }],
        "runInputSchema": null,
        "outputContract": [{
            "key":"reply",
            "schema":null,
            "collection":"single",
            "required":true
        }],
        "limits": null
    }))
    .unwrap()
}

fn schema(document: serde_json::Value) -> JsonSchemaSpec {
    JsonSchemaSpec {
        schema_version: 1,
        dialect: DIALECT_2020_12.into(),
        validation_profile_version: 1,
        format_policy_version: 1,
        document,
        limits: JsonSchemaLimits::default(),
    }
}

fn run_draft(graph_id: &str) -> GraphDraft {
    let mut draft = valid_draft(graph_id, "Runnable");
    draft.run_input_schema = Some(schema(json!({
        "type":"object",
        "properties":{"message":{"type":"string","minLength":1}},
        "required":["message"],
        "additionalProperties":false
    })));
    let DraftNodeKind::Input { run_input_selector } = &mut draft.nodes[0].kind else {
        unreachable!()
    };
    *run_input_selector = InputSelector::JsonPointer {
        pointer: "/message".into(),
    };
    draft.nodes[0].outputs = vec![OutputPortDefinition {
        name: "default".into(),
        schema: Some(schema(json!({"type":"string"}))),
    }];
    draft.output_contract[0].schema = Some(schema(json!({"type":"string"})));
    draft
}

async fn applied_graph(store: &SqliteStore, key: &str) -> GraphRevisionView {
    let graph = graph(store, &format!("create-{key}")).await;
    let initial = store.get_graph_draft(&graph.id).await.unwrap();
    let draft = store
        .update_graph_draft(UpdateGraphDraftCommand {
            graph_id: graph.id.clone(),
            expected_revision_token: initial.revision_token,
            document: run_draft(&graph.id),
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
