use serde_json::Value;
use zhuangsheng_core::{
    conversation::AssistantReplyPayloadV1,
    graph::{LlmNodeExecutionSnapshot, LlmOutputSpec},
    llm::{
        context::ContextConfigSnapshot,
        ir::LlmContentPartIr,
        text_transform::{
            TextTransformContext, TextTransformSurface, TextTransformTarget, apply_text_transforms,
        },
    },
    schema,
};

pub(super) struct OutputTransformError {
    pub code: &'static str,
    pub message: String,
}

pub(super) fn apply_canonical_output_transforms(
    mut value: Value,
    execution: &LlmNodeExecutionSnapshot,
) -> Result<Value, OutputTransformError> {
    let spec = match &execution.context {
        ContextConfigSnapshot::Preset { spec, .. }
        | ContextConfigSnapshot::GraphInline { spec, .. } => spec,
    };
    if spec.text_transforms.is_empty() {
        return Ok(value);
    }
    let context = TextTransformContext {
        target: Some(TextTransformTarget::AssistantOutput),
        surface: Some(TextTransformSurface::Canonical),
        depth: Some(0),
        is_edit: false,
        macros: spec.text_transform_macros.clone(),
    };
    match &mut value {
        Value::String(text) => {
            *text = transform(text, &spec.text_transforms, &context)?;
        }
        Value::Object(object)
            if object.get("type").and_then(Value::as_str) == Some("assistant_reply") =>
        {
            let mut payload: AssistantReplyPayloadV1 = serde_json::from_value(value.clone())
                .map_err(|_| {
                    error(
                        "text_transform_output_invalid",
                        "assistant reply cannot be decoded after model output validation",
                    )
                })?;
            for part in &mut payload.content {
                if let LlmContentPartIr::Text { text } = part {
                    *text = transform(text, &spec.text_transforms, &context)?;
                }
            }
            payload
                .validate()
                .map_err(|message| error("text_transform_output_invalid", message))?;
            value = serde_json::to_value(payload).map_err(|_| {
                error(
                    "text_transform_output_invalid",
                    "assistant reply cannot be encoded",
                )
            })?;
        }
        _ => return Ok(value),
    }
    if let Some(LlmOutputSpec::Json {
        schema: contract, ..
    }) = &execution.output
    {
        schema::validate(contract, &value).map_err(|_| {
            error(
                "text_transform_output_invalid",
                "canonical text transforms violated the configured output contract",
            )
        })?;
    }
    Ok(value)
}

fn transform(
    text: &str,
    rules: &[zhuangsheng_core::llm::text_transform::TextTransformRule],
    context: &TextTransformContext,
) -> Result<String, OutputTransformError> {
    apply_text_transforms(text, rules, context)
        .map(|output| output.text)
        .map_err(|cause| error(cause.code, cause.message))
}

fn error(code: &'static str, message: impl Into<String>) -> OutputTransformError {
    OutputTransformError {
        code,
        message: message.into(),
    }
}
