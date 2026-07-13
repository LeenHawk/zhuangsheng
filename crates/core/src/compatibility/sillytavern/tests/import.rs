use serde_json::json;

use super::super::*;
use crate::llm::context::{ContextAssemblyMode, ContextAssemblySpec};

#[test]
fn detects_supported_preset_families() {
    assert_eq!(
        detect_preset_kind(&json!([])),
        SillyTavernPresetKind::RegexScripts
    );
    assert_eq!(
        detect_preset_kind(&json!({"prompts":[],"prompt_order":[]})),
        SillyTavernPresetKind::OpenAi
    );
    assert_eq!(
        detect_preset_kind(&json!({"name":"Default","story_string":"{{description}}"})),
        SillyTavernPresetKind::Context
    );
    assert_eq!(
        detect_preset_kind(&json!({"name":"Roleplay","content":"Act as {{char}}"})),
        SillyTavernPresetKind::SystemPrompt
    );
}

#[test]
fn imports_openai_order_generation_and_embedded_regex() {
    let preview = preview_import(SillyTavernImportInput {
        document: json!({
            "name":"Roleplay",
            "temperature":0.7,
            "top_p":0.9,
            "openai_max_tokens":512,
            "seed":42,
            "frequency_penalty":0.2,
            "reverse_proxy":"https://secret.invalid",
            "proxy_password":"do-not-leak",
            "prompts":[
                {"identifier":"main","name":"Main","role":"system","content":"Write as {{char}}"},
                {"identifier":"chatHistory","name":"History","marker":true}
            ],
            "prompt_order":[
                {"character_id":100000,"order":[]},
                {"character_id":100001,"order":[
                    {"identifier":"main","enabled":true},
                    {"identifier":"chatHistory","enabled":true}
                ]}
            ],
            "extensions":{"regex_scripts":[regex_json("clean", "/foo/gi", "bar", vec![2])]}
        }),
        source_name: None,
        base_spec: None,
    })
    .unwrap();
    assert_eq!(preview.kind, SillyTavernPresetKind::OpenAi);
    assert_eq!(
        preview.generation.as_ref().unwrap().max_output_tokens,
        Some(512)
    );
    assert!(preview.provider_extensions.is_some());
    assert_eq!(preview.text_transforms.len(), 1);
    let spec = preview.context_spec.as_ref().unwrap();
    assert!(
        spec.items
            .iter()
            .any(|item| item.id.starts_with("st:0:main"))
    );
    assert!(spec.items.iter().any(|item| item.id == "history"));
    assert_eq!(spec.text_transforms, preview.text_transforms);
    let encoded = serde_json::to_string(&preview).unwrap();
    assert!(!encoded.contains("do-not-leak"));
    assert!(!encoded.contains("secret.invalid"));
    assert!(preview.inactive_fields.contains(&"proxy_password".into()));
}

#[test]
fn marker_order_updates_existing_canonical_items_without_overwriting_sources() {
    let base: ContextAssemblySpec = serde_json::from_value(json!({
        "mode":"chat",
        "items":[{
            "id":"character","name":"Character","enabled":true,"requestedRole":"system",
            "source":{"type":"literal","text":"Alice"},"position":{"type":"start"},
            "order":99,"priority":0,"insertionDepth":0,"budget":{"required":true},"overflow":null
        }],
        "budget":null,"postProcess":[],"textTransforms":[],"preview":null
    }))
    .unwrap();
    let preview = preview_import(SillyTavernImportInput {
        document: json!({
            "prompts":[{"identifier":"charDescription","marker":true}],
            "prompt_order":[{"character_id":100001,"order":[{"identifier":"charDescription","enabled":false}]}]
        }),
        source_name: Some("Imported".into()),
        base_spec: Some(base),
    }).unwrap();
    let character = &preview.context_spec.unwrap().items[0];
    assert!(!character.enabled);
    assert_eq!(character.order, 0);
    assert!(
        matches!(&character.source, crate::llm::context::ContextSource::Literal { text } if text == "Alice")
    );
}

#[test]
fn master_import_combines_system_prompt_and_generation() {
    let preview = preview_import(SillyTavernImportInput {
        document: json!({
            "sysprompt":{"name":"Actor","content":"You are the actor."},
            "preset":{"name":"Creative","temp":1.1,"top_k":40,"top_p":0.95,"rep_pen":1.05}
        }),
        source_name: Some("Master".into()),
        base_spec: None,
    })
    .unwrap();
    assert_eq!(preview.kind, SillyTavernPresetKind::Master);
    assert_eq!(preview.generation.unwrap().temperature, Some(1.1));
    assert!(
        preview
            .context_spec
            .unwrap()
            .items
            .iter()
            .any(|item| item.id == "st:system-prompt")
    );
}

fn regex_json(id: &str, find: &str, replace: &str, placement: Vec<i64>) -> serde_json::Value {
    json!({
        "id":id,"scriptName":id,"findRegex":find,"replaceString":replace,
        "trimStrings":[],"placement":placement,"disabled":false,"markdownOnly":false,
        "promptOnly":false,"runOnEdit":false,"substituteRegex":0,"minDepth":null,"maxDepth":null
    })
}

#[allow(dead_code)]
fn empty_spec() -> ContextAssemblySpec {
    ContextAssemblySpec {
        id: None,
        name: None,
        mode: ContextAssemblyMode::Chat,
        items: Vec::new(),
        budget: None,
        post_process: Vec::new(),
        text_transforms: Vec::new(),
        preview: None,
    }
}
