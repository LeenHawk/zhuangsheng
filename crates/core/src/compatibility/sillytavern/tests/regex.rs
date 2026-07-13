use std::collections::BTreeMap;

use serde_json::json;

use super::super::*;

#[test]
fn executes_global_captures_trim_and_match_macro() {
    let rule = imported_rule(json!({
        "id":"captures","scriptName":"Captures","findRegex":"/(?<tag>foo)-(bar)/gi",
        "replaceString":"$<tag>:$2:{{match}}","trimStrings":["foo"],"placement":[2],
        "disabled":false,"markdownOnly":false,"promptOnly":false,"runOnEdit":false,"substituteRegex":0
    }));
    let output = apply_text_transforms(
        "foo-bar FOO-bar",
        &[rule],
        &context(
            TextTransformPlacement::AiOutput,
            TextTransformSurface::Canonical,
            0,
        ),
    )
    .unwrap();
    assert_eq!(output.text, ":bar:-bar FOO:bar:FOO-bar");
    assert_eq!(output.applied_rule_ids, ["captures"]);
}

#[test]
fn escaped_find_macros_and_replacement_macros_are_deterministic() {
    let rule = imported_rule(json!({
        "id":"macro","scriptName":"Macro","findRegex":"/{{char}}/g",
        "replaceString":"{{user}}:$0","trimStrings":[],"placement":[1],
        "disabled":false,"markdownOnly":false,"promptOnly":true,"runOnEdit":true,"substituteRegex":2
    }));
    let mut context = context(
        TextTransformPlacement::UserInput,
        TextTransformSurface::Prompt,
        0,
    );
    context.macros = BTreeMap::from([("char".into(), "A.*".into()), ("user".into(), "Lin".into())]);
    assert_eq!(
        apply_text_transforms("A.* Axx", &[rule], &context)
            .unwrap()
            .text,
        "Lin:A.* Axx"
    );
}

#[test]
fn surfaces_depth_and_edit_flags_filter_rules() {
    let rule = imported_rule(json!({
        "id":"display","scriptName":"Display","findRegex":"/x/g","replaceString":"y",
        "trimStrings":[],"placement":[2],"disabled":false,"markdownOnly":true,
        "promptOnly":false,"runOnEdit":false,"substituteRegex":0,"minDepth":1,"maxDepth":2
    }));
    assert_eq!(
        apply_text_transforms(
            "x",
            &[rule.clone()],
            &context(
                TextTransformPlacement::AiOutput,
                TextTransformSurface::Canonical,
                1
            )
        )
        .unwrap()
        .text,
        "x"
    );
    assert_eq!(
        apply_text_transforms(
            "x",
            &[rule.clone()],
            &context(
                TextTransformPlacement::AiOutput,
                TextTransformSurface::Display,
                0
            )
        )
        .unwrap()
        .text,
        "x"
    );
    assert_eq!(
        apply_text_transforms(
            "x",
            &[rule.clone()],
            &context(
                TextTransformPlacement::AiOutput,
                TextTransformSurface::Display,
                1
            )
        )
        .unwrap()
        .text,
        "y"
    );
    let mut edit = context(
        TextTransformPlacement::AiOutput,
        TextTransformSurface::Display,
        1,
    );
    edit.is_edit = true;
    assert_eq!(
        apply_text_transforms("x", &[rule], &edit).unwrap().text,
        "x"
    );
}

#[test]
fn legacy_placements_follow_sillytavern_migration() {
    let display = imported_rule(json!({
        "id":"legacy-display","scriptName":"Legacy","findRegex":"x","replaceString":"y",
        "trimStrings":[],"placement":[0],"disabled":false,"markdownOnly":false,
        "promptOnly":false,"runOnEdit":false,"substituteRegex":0
    }));
    assert!(display.surfaces.contains(&TextTransformSurface::Display));
    assert!(display.surfaces.contains(&TextTransformSurface::Prompt));
    assert!(
        display
            .placements
            .contains(&TextTransformPlacement::AiOutput)
    );
    let send_as = imported_rule(json!({
        "id":"legacy-sendas","scriptName":"Legacy","findRegex":"x","replaceString":"y",
        "trimStrings":[],"placement":[4],"disabled":false,"markdownOnly":false,
        "promptOnly":false,"runOnEdit":false,"substituteRegex":0
    }));
    assert_eq!(send_as.placements, [TextTransformPlacement::SlashCommand]);
}

#[test]
fn invalid_flags_fail_closed_during_preview() {
    let error = preview_import(SillyTavernImportInput {
        document: json!([{
            "id":"bad","scriptName":"Bad","findRegex":"/x/z","replaceString":"y",
            "trimStrings":[],"placement":[1],"disabled":false,"markdownOnly":false,
            "promptOnly":false,"runOnEdit":false,"substituteRegex":0
        }]),
        source_name: None,
        base_spec: None,
    })
    .unwrap_err();
    assert_eq!(error.code, "invalid_text_transform_pattern");
}

fn imported_rule(value: serde_json::Value) -> TextTransformRule {
    preview_import(SillyTavernImportInput {
        document: json!([value]),
        source_name: None,
        base_spec: None,
    })
    .unwrap()
    .text_transforms
    .remove(0)
}

fn context(
    placement: TextTransformPlacement,
    surface: TextTransformSurface,
    depth: u32,
) -> TextTransformContext {
    TextTransformContext {
        placement: Some(placement),
        surface: Some(surface),
        depth: Some(depth),
        is_edit: false,
        macros: BTreeMap::new(),
    }
}
