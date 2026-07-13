use std::collections::BTreeMap;

use serde_json::Value;

use crate::graph::{GenerationOptionsIr, ProviderExtensionsIr, ProviderExtraIr};

use super::support::{ImportParts, warning};

pub(super) fn import_openai_generation(document: &Value, parts: &mut ImportParts) {
    let generation = GenerationOptionsIr {
        temperature: number(document, "temperature"),
        top_p: number(document, "top_p"),
        max_output_tokens: unsigned(document, "openai_max_tokens"),
        stop: stop_strings(document),
        seed: document
            .get("seed")
            .and_then(Value::as_i64)
            .filter(|value| *value >= 0),
    };
    if generation != empty_generation() {
        parts.generation = Some(generation);
    }
    let extra_body = provider_options(
        document,
        &[
            "frequency_penalty",
            "presence_penalty",
            "top_k",
            "top_a",
            "min_p",
            "repetition_penalty",
        ],
    );
    if !extra_body.is_empty() {
        parts.provider_extensions = Some(ProviderExtensionsIr {
            openai: Some(ProviderExtraIr {
                options: BTreeMap::new(),
                extra_body,
                extra_headers: BTreeMap::new(),
            }),
            ..Default::default()
        });
        warning(
            &mut parts.warnings,
            "sillytavern_provider_extensions",
            "non-portable generation fields were preserved as OpenAI extra-body options",
            None,
        );
    }
}

pub(super) fn import_text_completion_generation(document: &Value, parts: &mut ImportParts) {
    let generation = GenerationOptionsIr {
        temperature: number(document, "temp"),
        top_p: number(document, "top_p"),
        max_output_tokens: unsigned(document, "genamt")
            .or_else(|| unsigned(document, "max_length")),
        stop: stop_strings(document),
        seed: document
            .get("seed")
            .and_then(Value::as_i64)
            .filter(|value| *value >= 0),
    };
    if generation != empty_generation() {
        parts.generation = Some(generation);
    }
    let extra_body = provider_options(
        document,
        &[
            "top_k",
            "min_p",
            "rep_pen",
            "rep_pen_range",
            "typical_p",
            "tfs",
        ],
    );
    if !extra_body.is_empty() {
        parts.provider_extensions = Some(ProviderExtensionsIr {
            openai: Some(ProviderExtraIr {
                options: BTreeMap::new(),
                extra_body,
                extra_headers: BTreeMap::new(),
            }),
            ..Default::default()
        });
        warning(
            &mut parts.warnings,
            "sillytavern_provider_extensions",
            "text-completion sampler fields were preserved as provider extra-body options",
            None,
        );
    }
}

fn empty_generation() -> GenerationOptionsIr {
    GenerationOptionsIr {
        temperature: None,
        top_p: None,
        max_output_tokens: None,
        stop: Vec::new(),
        seed: None,
    }
}

fn number(document: &Value, key: &str) -> Option<f64> {
    document
        .get(key)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
}

fn unsigned(document: &Value, key: &str) -> Option<u64> {
    document
        .get(key)
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
}

fn provider_options(document: &Value, keys: &[&str]) -> BTreeMap<String, Value> {
    keys.iter()
        .filter_map(|key| {
            document
                .get(*key)
                .filter(|value| value.is_number() || value.is_boolean())
                .map(|value| ((*key).to_owned(), value.clone()))
        })
        .collect()
}

fn stop_strings(document: &Value) -> Vec<String> {
    for key in ["stop", "stopping_strings"] {
        if let Some(values) = document.get(key).and_then(Value::as_array) {
            return values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
        }
    }
    let Some(raw) = document
        .get("custom_stopping_strings")
        .and_then(Value::as_str)
    else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<String>>(raw).unwrap_or_default()
}
