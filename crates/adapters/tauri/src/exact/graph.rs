use serde_json::Value;
use zhuangsheng_core::application::graph::{
    ApplyGraphCommand, CreateRolePlayTemplateCommand, UpdateGraphDraftCommand,
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
        "get_graph_draft"
            | "update_graph_draft"
            | "apply_graph"
            | "get_graph_revision"
            | "get_graph_revision_for_graph"
            | "create_roleplay_template"
            | "get_roleplay_settings"
    ) {
        return None;
    }
    let result: CommandResult<Value> = async {
        match operation {
            "get_graph_draft" => encode(
                state
                    .get_graph_draft(&argument::<String>(payload, "graphId")?)
                    .await,
            ),
            "update_graph_draft" => encode(
                state
                    .update_graph_draft(argument::<UpdateGraphDraftCommand>(payload, "command")?)
                    .await,
            ),
            "apply_graph" => encode(
                state
                    .apply_graph(argument::<ApplyGraphCommand>(payload, "command")?)
                    .await,
            ),
            "get_graph_revision" => encode(
                state
                    .get_graph_revision(&argument::<String>(payload, "revisionId")?)
                    .await,
            ),
            "get_graph_revision_for_graph" => encode(
                state
                    .get_graph_revision_for_graph(
                        &argument::<String>(payload, "graphId")?,
                        &argument::<String>(payload, "revisionId")?,
                    )
                    .await,
            ),
            "create_roleplay_template" => encode(
                state
                    .create_roleplay_template(argument::<CreateRolePlayTemplateCommand>(
                        payload, "command",
                    )?)
                    .await,
            ),
            "get_roleplay_settings" => encode(
                state
                    .get_roleplay_settings(&argument::<String>(payload, "revisionId")?)
                    .await,
            ),
            _ => unreachable!(),
        }
    }
    .await;
    Some(result)
}
