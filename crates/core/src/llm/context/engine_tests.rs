use std::collections::BTreeMap;

use serde_json::json;

use crate::{
    graph::{InputSelector, SelectorResult},
    llm::ir::{ContextSensitivity, ContextTrust, MessageRole},
};

use super::{engine_test_support::*, *};

#[test]
fn untrusted_template_is_downgraded_to_context() {
    let source = ContextSource::Template {
        syntax: TemplateSyntax::ZhuangshengTemplateV1,
        template: "Character says: {{value}}".into(),
        variables: BTreeMap::from([(
            "value".into(),
            TemplateVariableSource::Input {
                selector: InputSelector::WholeValue,
            },
        )]),
        on_missing: TemplateMissingPolicy::Error,
        compiled: None,
    };
    let spec = spec(vec![item(
        "template",
        ContextRole::System,
        source,
        ContextPosition::Start,
        false,
        0,
        Some(OverflowPolicy::Drop),
    )]);
    let output = assemble_context(
        &assembly_input(spec, json!("hello"), BTreeMap::new(), 100),
        &ScalarCounter,
    )
    .unwrap();
    assert_eq!(
        output.instructions[0].role,
        crate::llm::ir::InstructionRole::Context
    );
    assert_eq!(
        output.instructions[0].provenance.trust,
        ContextTrust::UserInput
    );
    assert!(
        output.instructions[0]
            .provenance
            .transformations
            .iter()
            .any(|value| value == "template_taint_downgrade")
    );
}

#[test]
fn binding_role_must_explicitly_authorize_final_role() {
    let source = ContextSource::Memory {
        binding_id: "memory".into(),
        view: None,
    };
    let spec = spec(vec![item(
        "memory",
        ContextRole::Developer,
        source,
        ContextPosition::Start,
        false,
        0,
        Some(OverflowPolicy::Drop),
    )]);
    let bindings = BTreeMap::from([(
        "memory".into(),
        data_binding(
            "memory",
            vec![data_value(
                "entry",
                "facts",
                ContextTrust::TrustedConfig,
                ContextSensitivity::Private,
                vec![ContextRole::System],
                None,
            )],
        ),
    )]);
    let error = assemble_context(
        &assembly_input(spec, json!(null), bindings, 100),
        &ScalarCounter,
    )
    .unwrap_err();
    assert_eq!(error.code, "context_role_unauthorized");
}

#[test]
fn history_is_stable_and_keep_recent_uses_newest_fitting_suffix() {
    let source = ContextSource::History {
        binding_id: "history".into(),
        strategy: HistoryStrategy::All,
    };
    let spec = spec(vec![item(
        "history",
        ContextRole::Context,
        source,
        ContextPosition::History,
        false,
        0,
        Some(OverflowPolicy::KeepRecent { count: None }),
    )]);
    let binding = ResolvedContextBinding {
        binding_id: "history".into(),
        scope: "conversation:one".into(),
        version: "v1".into(),
        values: vec![
            history_value("m3", 30, MessageRole::Assistant, "cc"),
            history_value("m1", 10, MessageRole::User, "aa"),
            history_value("m2", 20, MessageRole::User, "bb"),
        ],
        template_value: None,
        template_provenance: None,
    };
    let output = assemble_context(
        &assembly_input(
            spec,
            json!(null),
            BTreeMap::from([("history".into(), binding)]),
            4,
        ),
        &ScalarCounter,
    )
    .unwrap();
    assert_eq!(
        output
            .messages
            .iter()
            .map(|message| message.id.as_str())
            .collect::<Vec<_>>(),
        vec!["m2", "m3"]
    );
    assert_eq!(
        output.budget_report.items[0].action,
        ContextBudgetAction::Truncated
    );
}

#[test]
fn required_context_overflow_fails_closed() {
    let spec = spec(vec![item(
        "required",
        ContextRole::System,
        ContextSource::Literal {
            text: "12345".into(),
        },
        ContextPosition::Start,
        true,
        0,
        None,
    )]);
    let error = assemble_context(
        &assembly_input(spec, json!(null), BTreeMap::new(), 4),
        &ScalarCounter,
    )
    .unwrap_err();
    assert_eq!(error.code, "context_budget_exceeded");
}

#[test]
fn optional_budget_is_allocated_by_priority_not_display_order() {
    let low = item(
        "low",
        ContextRole::Context,
        ContextSource::Literal { text: "low".into() },
        ContextPosition::Start,
        false,
        0,
        Some(OverflowPolicy::Drop),
    );
    let high = item(
        "high",
        ContextRole::Context,
        ContextSource::Literal { text: "top".into() },
        ContextPosition::End,
        false,
        10,
        Some(OverflowPolicy::Drop),
    );
    let output = assemble_context(
        &assembly_input(spec(vec![low, high]), json!(null), BTreeMap::new(), 3),
        &ScalarCounter,
    )
    .unwrap();
    assert_eq!(instruction_text(&output.instructions[0]), "top");
    assert!(!output.budget_report.items[0].included);
    assert!(output.budget_report.items[1].included);
}

#[test]
fn unicode_truncation_removes_the_named_side() {
    let tail = truncation_output(OverflowPolicy::TruncateTail);
    let head = truncation_output(OverflowPolicy::TruncateHead);
    assert_eq!(instruction_text(&tail.instructions[0]), "甲乙");
    assert_eq!(instruction_text(&head.instructions[0]), "丙丁");
}

#[test]
fn top_k_uses_score_then_original_binding_order() {
    let source = ContextSource::WorldInfo {
        binding_id: "world".into(),
        selector: WorldInfoSelector::All,
    };
    let spec = spec(vec![item(
        "world",
        ContextRole::Context,
        source,
        ContextPosition::Start,
        false,
        0,
        Some(OverflowPolicy::TopK { k: 2 }),
    )]);
    let values = vec![
        data_value(
            "a",
            "a",
            ContextTrust::ExternalUntrusted,
            ContextSensitivity::Public,
            vec![ContextRole::Context],
            Some(5),
        ),
        data_value(
            "b",
            "b",
            ContextTrust::ExternalUntrusted,
            ContextSensitivity::Public,
            vec![ContextRole::Context],
            Some(10),
        ),
        data_value(
            "c",
            "c",
            ContextTrust::ExternalUntrusted,
            ContextSensitivity::Public,
            vec![ContextRole::Context],
            Some(10),
        ),
    ];
    let output = assemble_context(
        &assembly_input(
            spec,
            json!(null),
            BTreeMap::from([("world".into(), data_binding("world", values))]),
            100,
        ),
        &ScalarCounter,
    )
    .unwrap();
    assert_eq!(
        output
            .instructions
            .iter()
            .map(instruction_text)
            .collect::<Vec<_>>(),
        vec!["b", "c"]
    );
}

#[test]
fn dedupe_keeps_the_first_display_candidate() {
    let first = item(
        "first",
        ContextRole::Context,
        ContextSource::Literal {
            text: "same".into(),
        },
        ContextPosition::Start,
        false,
        0,
        Some(OverflowPolicy::Drop),
    );
    let second = item(
        "second",
        ContextRole::Context,
        ContextSource::Literal {
            text: "same".into(),
        },
        ContextPosition::End,
        false,
        0,
        Some(OverflowPolicy::Dedupe),
    );
    let output = assemble_context(
        &assembly_input(spec(vec![first, second]), json!(null), BTreeMap::new(), 100),
        &ScalarCounter,
    )
    .unwrap();
    assert_eq!(output.instructions.len(), 1);
    assert_eq!(
        output.budget_report.items[1].action,
        ContextBudgetAction::Deduped
    );
    assert!(!output.budget_report.items[1].included);
}

#[test]
fn snapshot_digests_are_deterministic_and_bind_versioned_values() {
    let spec = spec(vec![item(
        "input",
        ContextRole::User,
        ContextSource::Input {
            path: "/text".into(),
        },
        ContextPosition::UserInput,
        true,
        0,
        None,
    )]);
    let input = assembly_input(spec, json!({"text":"hello"}), BTreeMap::new(), 100);
    let first = assemble_context(&input, &ScalarCounter).unwrap();
    let second = assemble_context(&input, &ScalarCounter).unwrap();
    assert_eq!(first.snapshot, second.snapshot);
    let mut changed = input.clone();
    changed.node_input = json!({"text":"different"});
    let third = assemble_context(&changed, &ScalarCounter).unwrap();
    assert_ne!(
        first.snapshot.assembly_digest,
        third.snapshot.assembly_digest
    );
}

#[test]
fn completion_and_post_process_produce_safe_message_shapes() {
    let mut prompt = spec(vec![input_item("a", "/a", 0), input_item("b", "/b", 1)]);
    prompt.mode = ContextAssemblyMode::Completion;
    prompt.post_process = vec![PromptPostProcessRule::MergeAdjacentMessages];
    let output = assemble_context(
        &assembly_input(prompt, json!({"a":"A","b":"B"}), BTreeMap::new(), 100),
        &ScalarCounter,
    )
    .unwrap();
    assert_eq!(output.messages.len(), 1);
    assert_eq!(output.messages[0].role, MessageRole::User);
    assert_eq!(message_text(&output.messages[0]), "AB");

    let mut alternating = spec(vec![input_item("a", "/a", 0), input_item("b", "/b", 1)]);
    alternating.post_process = vec![PromptPostProcessRule::StrictAlternation];
    let output = assemble_context(
        &assembly_input(alternating, json!({"a":"A","b":"B"}), BTreeMap::new(), 100),
        &ScalarCounter,
    )
    .unwrap();
    assert_eq!(
        output
            .messages
            .iter()
            .map(|value| value.role)
            .collect::<Vec<_>>(),
        vec![MessageRole::User, MessageRole::Assistant, MessageRole::User]
    );
    assert!(output.messages[1].content.is_empty());
}

#[test]
fn sensitive_context_and_non_missing_template_errors_fail_closed() {
    let source = ContextSource::Memory {
        binding_id: "secret".into(),
        view: None,
    };
    let sensitive_spec = spec(vec![item(
        "secret",
        ContextRole::Context,
        source,
        ContextPosition::Start,
        false,
        0,
        Some(OverflowPolicy::Drop),
    )]);
    let value = data_value(
        "secret",
        "private",
        ContextTrust::ExternalUntrusted,
        ContextSensitivity::Sensitive,
        vec![ContextRole::Context],
        None,
    );
    let error = assemble_context(
        &assembly_input(
            sensitive_spec,
            json!(null),
            BTreeMap::from([("secret".into(), data_binding("secret", vec![value]))]),
            100,
        ),
        &ScalarCounter,
    )
    .unwrap_err();
    assert_eq!(error.code, "context_sensitive_not_allowed");

    let template = ContextSource::Template {
        syntax: TemplateSyntax::ZhuangshengTemplateV1,
        template: "{{value}}".into(),
        variables: BTreeMap::from([(
            "value".into(),
            TemplateVariableSource::Input {
                selector: InputSelector::JsonPath {
                    path: "$[*]".into(),
                    result: SelectorResult::One,
                },
            },
        )]),
        on_missing: TemplateMissingPolicy::Empty,
        compiled: None,
    };
    let spec = spec(vec![item(
        "template",
        ContextRole::Context,
        template,
        ContextPosition::Start,
        false,
        0,
        Some(OverflowPolicy::Drop),
    )]);
    let error = assemble_context(
        &assembly_input(spec, json!([1, 2]), BTreeMap::new(), 100),
        &ScalarCounter,
    )
    .unwrap_err();
    assert_eq!(error.code, "context_template_selection_failed");
}
