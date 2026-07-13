use serde_json::Value;
use zhuangsheng_core::application::context::CommitContextPatchCommand;

use crate::{CommandResult, TauriAdapter};

use super::{argument, encode};

pub async fn dispatch(
    state: &TauriAdapter,
    operation: &str,
    payload: &Value,
) -> Option<CommandResult<Value>> {
    if !matches!(
        operation,
        "commit_context_patch"
            | "get_working_context"
            | "get_context_at_commit"
            | "diff_context_commits"
    ) {
        return None;
    }
    let result: CommandResult<Value> = async {
        match operation {
            "commit_context_patch" => encode(
                state
                    .commit_context_patch(argument::<CommitContextPatchCommand>(
                        payload, "command",
                    )?)
                    .await,
            ),
            "get_working_context" => encode(
                state
                    .get_working_context(
                        &argument::<String>(payload, "contextId")?,
                        &argument::<String>(payload, "branchId")?,
                    )
                    .await,
            ),
            "get_context_at_commit" => encode(
                state
                    .get_context_at_commit(&argument::<String>(payload, "commitId")?)
                    .await,
            ),
            "diff_context_commits" => encode(
                state
                    .diff_context_commits(
                        &argument::<String>(payload, "contextId")?,
                        &argument::<String>(payload, "fromCommitId")?,
                        &argument::<String>(payload, "toCommitId")?,
                    )
                    .await,
            ),
            _ => unreachable!(),
        }
    }
    .await;
    Some(result)
}
