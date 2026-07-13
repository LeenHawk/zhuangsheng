use zhuangsheng_core::{
    conversation::ConversationMessageRole,
    llm::{
        ir::LlmContentPartIr,
        text_transform::{
            RegexMacroMode, TextTransformPlacement, TextTransformRule, TextTransformScope,
            TextTransformSurface,
        },
    },
};

use super::display_projection::{DisplayTransformPlan, project_display_content};

#[test]
fn display_projection_uses_only_display_surface_without_mutating_canonical_content() {
    let canonical = vec![LlmContentPartIr::Text {
        text: "secret foo".into(),
    }];
    let transform_plan = plan(vec![
        rule("display", TextTransformSurface::Display, "visible"),
        rule("prompt", TextTransformSurface::Prompt, "prompt-only"),
    ]);
    let projected = project_display_content(
        &canonical,
        ConversationMessageRole::Assistant,
        0,
        &transform_plan,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        projected,
        [LlmContentPartIr::Text {
            text: "secret visible".into()
        }]
    );
    assert_eq!(
        canonical,
        [LlmContentPartIr::Text {
            text: "secret foo".into()
        }]
    );
}

#[test]
fn display_projection_honors_message_depth() {
    let mut depth_rule = rule("latest", TextTransformSurface::Display, "new");
    depth_rule.min_depth = Some(0);
    depth_rule.max_depth = Some(0);
    let content = [LlmContentPartIr::Text { text: "foo".into() }];
    assert!(
        project_display_content(
            &content,
            ConversationMessageRole::Assistant,
            1,
            &plan(vec![depth_rule.clone()])
        )
        .unwrap()
        .is_none()
    );
    assert!(
        project_display_content(
            &content,
            ConversationMessageRole::Assistant,
            0,
            &plan(vec![depth_rule])
        )
        .unwrap()
        .is_some()
    );
}

fn rule(id: &str, surface: TextTransformSurface, replacement: &str) -> TextTransformRule {
    TextTransformRule {
        id: id.into(),
        name: id.into(),
        scope: TextTransformScope::Preset,
        order: 0,
        find_regex: "/foo/g".into(),
        replace_string: replacement.into(),
        trim_strings: Vec::new(),
        placements: vec![TextTransformPlacement::AiOutput],
        surfaces: vec![surface],
        disabled: false,
        run_on_edit: false,
        macro_mode: RegexMacroMode::None,
        min_depth: None,
        max_depth: None,
    }
}

fn plan(rules: Vec<TextTransformRule>) -> DisplayTransformPlan {
    DisplayTransformPlan {
        rules,
        macros: Default::default(),
    }
}
