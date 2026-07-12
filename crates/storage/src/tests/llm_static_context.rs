use zhuangsheng_core::{
    graph::{
        LlmMemoryBinding, MemoryReadConsistency, NodeMemoryBinding, StaticMemoryRead,
        StaticMemoryReadSource,
    },
    llm::{
        context::ResolvedContextValue,
        ir::{ContextSensitivity, ContextTrust, LlmContentPartIr},
    },
};

use crate::tests::{llm_tool_support::prepare_running_tool_attempt_with_memory, store};

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
