use serde_json::Value;
use zhuangsheng_core::application::{
    channel::{DiscoverChannelModelsCommand, PublishChannelRevisionCommand},
    preset::{PreviewContextPresetCommand, PublishContextPresetVersionCommand},
    sillytavern::{
        ApplySillyTavernImportCommand, PreviewSillyTavernImportCommand, TestSillyTavernRegexCommand,
    },
};

use crate::{CommandResult, TauriAdapter};

use super::{argument, encode};

pub async fn dispatch(
    state: &TauriAdapter,
    operation: &str,
    payload: &Value,
) -> Option<CommandResult<Value>> {
    if !matches!(
        operation,
        "publish_channel_revision"
            | "get_channel_revision"
            | "get_channel_head_revision"
            | "discover_channel_models"
            | "publish_context_preset_version"
            | "get_context_preset_version"
            | "get_context_preset_head"
            | "preview_context_preset"
            | "preview_sillytavern_import"
            | "test_sillytavern_regex"
            | "apply_sillytavern_import"
    ) {
        return None;
    }
    let result: CommandResult<Value> = async {
        match operation {
            "publish_channel_revision" => encode(
                state
                    .publish_channel_revision(argument::<PublishChannelRevisionCommand>(
                        payload, "command",
                    )?)
                    .await,
            ),
            "get_channel_revision" => encode(
                state
                    .get_channel_revision(&argument::<String>(payload, "revisionId")?)
                    .await,
            ),
            "get_channel_head_revision" => encode(
                state
                    .get_channel_head_revision(&argument::<String>(payload, "channelId")?)
                    .await,
            ),
            "discover_channel_models" => encode(
                state
                    .discover_channel_models(argument::<DiscoverChannelModelsCommand>(
                        payload, "command",
                    )?)
                    .await,
            ),
            "publish_context_preset_version" => encode(
                state
                    .publish_context_preset_version(argument::<PublishContextPresetVersionCommand>(
                        payload, "command",
                    )?)
                    .await,
            ),
            "get_context_preset_version" => encode(
                state
                    .get_context_preset_version(&argument::<String>(payload, "versionId")?)
                    .await,
            ),
            "get_context_preset_head" => encode(
                state
                    .get_context_preset_head(&argument::<String>(payload, "presetId")?)
                    .await,
            ),
            "preview_context_preset" => encode(
                state
                    .preview_context_preset(argument::<PreviewContextPresetCommand>(
                        payload, "command",
                    )?)
                    .await,
            ),
            "preview_sillytavern_import" => encode(
                state
                    .preview_sillytavern_import(argument::<PreviewSillyTavernImportCommand>(
                        payload, "command",
                    )?)
                    .await,
            ),
            "test_sillytavern_regex" => encode(
                state
                    .test_sillytavern_regex(argument::<TestSillyTavernRegexCommand>(
                        payload, "command",
                    )?)
                    .await,
            ),
            "apply_sillytavern_import" => encode(
                state
                    .apply_sillytavern_import(argument::<ApplySillyTavernImportCommand>(
                        payload, "command",
                    )?)
                    .await,
            ),
            _ => unreachable!(),
        }
    }
    .await;
    Some(result)
}
