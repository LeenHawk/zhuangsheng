use serde_json::{Value, json};

use super::super::*;

#[test]
fn import_export_import_preserves_supported_prompt_generation_and_regex() {
    let imported = preview_import(SillyTavernImportInput {
        document: openai_document(),
        source_name: Some("source.json".into()),
        base_spec: None,
    })
    .unwrap();
    let with_global = preview_import(SillyTavernImportInput {
        document: json!([regex("global", vec![1])]),
        source_name: Some("global.json".into()),
        base_spec: imported.context_spec.clone(),
    })
    .unwrap();
    let with_character = preview_import(SillyTavernImportInput {
        document: json!({"data":{"extensions":{"regex_scripts":[regex("character", vec![2])]}}}),
        source_name: Some("character.json".into()),
        base_spec: with_global.context_spec,
    })
    .unwrap();
    assert_eq!(with_character.kind, SillyTavernPresetKind::RegexScripts);
    let original_spec = with_character.context_spec.unwrap();
    let bundle = export_sillytavern_bundle(
        "Roleplay",
        &original_spec,
        imported.generation.as_ref(),
        imported.provider_extensions.as_ref(),
    )
    .unwrap();
    assert_eq!(bundle.documents.len(), 3);

    let mut base_spec = None;
    let mut generation = None;
    let mut extensions = None;
    for document in bundle.documents {
        let preview = preview_import(SillyTavernImportInput {
            document: document.document,
            source_name: Some(document.file_name),
            base_spec,
        })
        .unwrap();
        generation = preview.generation.or(generation);
        extensions = preview.provider_extensions.or(extensions);
        base_spec = preview.context_spec;
    }
    let roundtrip = base_spec.unwrap();
    assert_eq!(roundtrip.items, original_spec.items);
    assert_eq!(roundtrip.text_transforms, original_spec.text_transforms);
    assert_eq!(generation, imported.generation);
    assert_eq!(extensions, imported.provider_extensions);
}

fn openai_document() -> Value {
    json!({
        "name":"Roleplay", "temperature":0.8, "top_p":0.9,
        "openai_max_tokens":512, "frequency_penalty":0.2,
        "prompts":[
            {"identifier":"main","name":"Main","role":"system","content":"Write a reply."},
            {"identifier":"chatHistory","name":"History","marker":true}
        ],
        "prompt_order":[{"character_id":100001,"order":[
            {"identifier":"main","enabled":true},
            {"identifier":"chatHistory","enabled":true}
        ]}],
        "extensions":{"regex_scripts":[regex("preset", vec![2])]}
    })
}

fn regex(id: &str, placement: Vec<i64>) -> Value {
    json!({
        "id":id, "scriptName":id, "findRegex":"/foo/g", "replaceString":"bar",
        "trimStrings":[], "placement":placement, "disabled":false,
        "markdownOnly":false, "promptOnly":false, "runOnEdit":false,
        "substituteRegex":0, "minDepth":null, "maxDepth":null
    })
}
