use zhuangsheng_core::{
    graph::{
        InputSelector, LlmMemoryBinding, MemoryReadConsistency, NodeMemoryBinding,
        PreExecutionValueSelector, PreExecutionValueSource, StaticMemoryRead,
        StaticMemoryReadSource,
    },
    llm::{
        context::ResolvedContextValue,
        ir::{ContextSensitivity, ContextTrust, LlmContentPartIr},
    },
};

use sea_orm::ConnectionTrait;

use crate::{
    graph::helpers::sql,
    tests::{
        llm_tool_support::{
            prepare_running_tool_attempt_with_memory, try_prepare_running_tool_attempt_with_memory,
        },
        store,
    },
};

#[tokio::test]
async fn llm_claim_contains_durable_working_context_binding_and_read_set() {
    let store = store().await;
    let claimed = prepare_running_tool_attempt_with_memory(
        &store,
        LlmMemoryBinding {
            node: NodeMemoryBinding {
                reads: vec![StaticMemoryRead {
                    id: "working".into(),
                    alias: "working".into(),
                    source: StaticMemoryReadSource::WorkingContext {
                        scope: "run-context".into(),
                        path: "".into(),
                    },
                    required: true,
                    consistency: MemoryReadConsistency::Snapshot,
                    limit: None,
                    max_bytes: 1024,
                }],
                working_writes: Vec::new(),
            },
            tools: Vec::new(),
        },
    )
    .await;
    let snapshot = claimed.context_snapshot.expect("LLM context snapshot");
    assert_eq!(
        snapshot.read_set_ref,
        format!("node-instance:{}:read-set:v1", claimed.node_instance_id)
    );
    assert!(snapshot.read_set_digest.starts_with("sha256:"));
    let binding = snapshot.bindings.get("working").unwrap();
    assert_eq!(binding.scope, "run-context");
    assert!(!binding.version.is_empty());
    let ResolvedContextValue::Data {
        content,
        provenance,
        allowed_roles,
        ..
    } = &binding.values[0]
    else {
        panic!("expected data binding")
    };
    assert_eq!(content, &[LlmContentPartIr::Text { text: "{}".into() }]);
    assert_eq!(provenance.trust, ContextTrust::ExternalUntrusted);
    assert_eq!(provenance.sensitivity, ContextSensitivity::Private);
    assert_eq!(
        allowed_roles,
        &[zhuangsheng_core::llm::context::ContextRole::Context]
    );
}

#[tokio::test]
async fn optional_artifact_selector_pins_an_empty_binding_without_hidden_lookup() {
    let store = store().await;
    let claimed = prepare_running_tool_attempt_with_memory(
        &store,
        LlmMemoryBinding {
            node: NodeMemoryBinding {
                reads: vec![StaticMemoryRead {
                    id: "document".into(),
                    alias: "document_alias".into(),
                    source: StaticMemoryReadSource::Artifact {
                        scope: "run-context".into(),
                        artifact_ref_from: PreExecutionValueSelector {
                            source: PreExecutionValueSource::Input,
                            source_name: "default".into(),
                            selector: InputSelector::JsonPointer {
                                pointer: "/artifact".into(),
                            },
                        },
                    },
                    required: false,
                    consistency: MemoryReadConsistency::Snapshot,
                    limit: None,
                    max_bytes: 1024,
                }],
                working_writes: vec![],
            },
            tools: vec![],
        },
    )
    .await;
    let snapshot = claimed.context_snapshot.unwrap();
    assert!(!snapshot.bindings.contains_key("document"));
    let binding = &snapshot.bindings["document_alias"];
    assert_eq!(binding.binding_id, "document_alias");
    assert!(binding.values.is_empty());
    assert!(binding.template_value.is_none());
    assert!(binding.version.starts_with("sha256:"));
}

#[tokio::test]
async fn required_artifact_miss_fails_the_activation_durably() {
    let store = store().await;
    let attempt = try_prepare_running_tool_attempt_with_memory(
        &store,
        LlmMemoryBinding {
            node: NodeMemoryBinding {
                reads: vec![StaticMemoryRead {
                    id: "document".into(),
                    alias: "document".into(),
                    source: StaticMemoryReadSource::Artifact {
                        scope: "run-context".into(),
                        artifact_ref_from: PreExecutionValueSelector {
                            source: PreExecutionValueSource::Input,
                            source_name: "default".into(),
                            selector: InputSelector::JsonPointer {
                                pointer: "/artifact".into(),
                            },
                        },
                    },
                    required: true,
                    consistency: MemoryReadConsistency::Snapshot,
                    limit: None,
                    max_bytes: 1024,
                }],
                working_writes: vec![],
            },
            tools: vec![],
        },
    )
    .await;
    assert!(attempt.is_none());
    let row = store
        .db
        .query_one_raw(sql(
            "SELECT r.status AS run_status,ni.status AS node_status,a.status AS attempt_status FROM graph_runs r JOIN node_instances ni ON ni.run_id=r.id AND ni.node_id='generate' JOIN node_attempts a ON a.node_instance_id=ni.id",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.try_get::<String>("", "run_status").unwrap(), "failed");
    assert_eq!(row.try_get::<String>("", "node_status").unwrap(), "failed");
    assert_eq!(
        row.try_get::<String>("", "attempt_status").unwrap(),
        "failed"
    );
}
