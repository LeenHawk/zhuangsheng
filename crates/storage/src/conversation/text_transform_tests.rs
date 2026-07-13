use std::collections::BTreeMap;

use zhuangsheng_core::llm::{
    context::{ContextAssemblyMode, ContextAssemblySpec},
    ir::LlmContentPartIr,
    text_transform::{
        RegexMacroMode, TextTransformPlacement, TextTransformRule, TextTransformScope,
        TextTransformSurface,
    },
};

use super::text_transform::apply_user_content;

#[test]
fn canonical_user_transform_uses_versioned_macros_and_ignores_display_rules() {
    let mut spec = ContextAssemblySpec {
        id: None,
        name: None,
        mode: ContextAssemblyMode::Chat,
        items: Vec::new(),
        budget: None,
        post_process: Vec::new(),
        text_transforms: vec![
            rule("canonical", TextTransformSurface::Canonical, "{{char}}"),
            rule("display", TextTransformSurface::Display, "display-only"),
        ],
        text_transform_macros: BTreeMap::from([("char".into(), "Alice".into())]),
        preview: None,
    };
    spec.text_transforms[1].order = 1;
    let source = [LlmContentPartIr::Text {
        text: "hello foo".into(),
    }];
    let transformed = apply_user_content(&spec, &source).unwrap();
    assert_eq!(
        transformed,
        [LlmContentPartIr::Text {
            text: "hello Alice".into()
        }]
    );
    assert_eq!(
        source,
        [LlmContentPartIr::Text {
            text: "hello foo".into()
        }]
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
        placements: vec![TextTransformPlacement::UserInput],
        surfaces: vec![surface],
        disabled: false,
        run_on_edit: false,
        macro_mode: RegexMacroMode::None,
        min_depth: None,
        max_depth: None,
    }
}
