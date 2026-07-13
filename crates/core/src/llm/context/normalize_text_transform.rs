use std::collections::BTreeMap;

use crate::llm::{LlmConfigError, LlmConfigResult};

pub(super) fn normalize_text_transform_macros(
    macros: &mut BTreeMap<String, String>,
) -> LlmConfigResult<()> {
    if macros.len() > 128 {
        return Err(LlmConfigError::new(
            "text_transform_macro_limit",
            "context preset has more than 128 text transform macros",
        ));
    }
    for (name, value) in macros.iter() {
        if name.is_empty()
            || name.len() > 128
            || !name
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_')
            || value.len() > 64 * 1024
        {
            return Err(LlmConfigError::new(
                "invalid_text_transform_macro",
                "text transform macro names or values exceed their safe limits",
            ));
        }
    }
    Ok(())
}
