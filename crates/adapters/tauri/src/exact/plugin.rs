use serde_json::Value;
use zhuangsheng_core::application::plugin::{
    ActivatePluginCandidateCommand, ConfigurePluginCommand, InspectGitPluginCommand,
    RollbackPluginCommand,
};

use super::{argument, encode};
use crate::{CommandResult, TauriAdapter};

pub(super) async fn dispatch(
    adapter: &TauriAdapter,
    operation: &str,
    payload: &Value,
) -> Option<CommandResult<Value>> {
    if !matches!(
        operation,
        "inspect_git_plugin_source"
            | "activate_plugin_candidate"
            | "list_plugins"
            | "configure_plugin"
            | "check_plugin_update"
            | "rollback_plugin"
            | "get_plugin_entrypoint"
    ) {
        return None;
    }
    let result: CommandResult<Value> = async {
        match operation {
            "inspect_git_plugin_source" => encode(
                adapter
                    .inspect_git_plugin_source(argument::<InspectGitPluginCommand>(
                        payload, "command",
                    )?)
                    .await,
            ),
            "activate_plugin_candidate" => encode(
                adapter
                    .activate_plugin_candidate(argument::<ActivatePluginCandidateCommand>(
                        payload, "command",
                    )?)
                    .await,
            ),
            "list_plugins" => encode(adapter.list_plugins().await),
            "configure_plugin" => encode(
                adapter
                    .configure_plugin(argument::<ConfigurePluginCommand>(payload, "command")?)
                    .await,
            ),
            "check_plugin_update" => encode(
                adapter
                    .check_plugin_update(&argument::<String>(payload, "pluginId")?)
                    .await,
            ),
            "rollback_plugin" => encode(
                adapter
                    .rollback_plugin(argument::<RollbackPluginCommand>(payload, "command")?)
                    .await,
            ),
            "get_plugin_entrypoint" => encode(
                adapter
                    .get_plugin_entrypoint(&argument::<String>(payload, "pluginId")?)
                    .await,
            ),
            _ => unreachable!(),
        }
    }
    .await;
    Some(result)
}
