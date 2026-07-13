use serde_json::{Map, Number, Value, json};

use crate::graph::{GenerationOptionsIr, ProviderExtensionsIr};

use super::warning;
use crate::compatibility::sillytavern::SillyTavernImportWarning;

pub(super) fn export_generation(
    target: &mut Map<String, Value>,
    generation: Option<&GenerationOptionsIr>,
    extensions: Option<&ProviderExtensionsIr>,
    warnings: &mut Vec<SillyTavernImportWarning>,
) {
    if let Some(generation) = generation {
        insert_float(target, "temperature", generation.temperature);
        insert_float(target, "top_p", generation.top_p);
        insert_option(target, "openai_max_tokens", generation.max_output_tokens);
        insert_option(target, "seed", generation.seed);
        target.insert("stop".into(), json!(generation.stop));
    }
    let Some(openai) = extensions.and_then(|value| value.openai.as_ref()) else {
        return;
    };
    for key in [
        "frequency_penalty",
        "presence_penalty",
        "top_k",
        "top_a",
        "min_p",
        "repetition_penalty",
    ] {
        if let Some(value) = openai
            .extra_body
            .get(key)
            .filter(|value| value.is_number() || value.is_boolean())
        {
            target.insert(key.into(), value.clone());
        }
    }
    if !openai.options.is_empty() || !openai.extra_headers.is_empty() {
        warning(
            warnings,
            "sillytavern_export_provider_fields",
            "provider options and headers were intentionally omitted",
            None,
        );
    }
}

fn insert_float(target: &mut Map<String, Value>, key: &str, value: Option<f64>) {
    if let Some(number) = value.and_then(Number::from_f64) {
        target.insert(key.into(), Value::Number(number));
    }
}

fn insert_option<T: serde::Serialize>(
    target: &mut Map<String, Value>,
    key: &str,
    value: Option<T>,
) {
    if let Some(value) = value.and_then(|value| serde_json::to_value(value).ok()) {
        target.insert(key.into(), value);
    }
}
