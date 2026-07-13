use serde_json::Value;
use zhuangsheng_core::{
    context_merge::MergeContextCommand,
    runtime::{ForkContextCommand, StartRunCommand},
};

use crate::{CommandResult, ResolveEffectUnknownInput, SatisfyWaitInput, TauriAdapter};

use super::{argument, encode};

pub async fn dispatch(
    state: &TauriAdapter,
    operation: &str,
    payload: &Value,
) -> Option<CommandResult<Value>> {
    if !matches!(
        operation,
        "start_run"
            | "get_run_outputs"
            | "list_open_waits"
            | "list_run_events"
            | "satisfy_wait"
            | "resolve_effect_unknown"
            | "fork_context"
            | "merge_context"
    ) {
        return None;
    }
    let result: CommandResult<Value> = async {
        match operation {
            "start_run" => encode(
                state
                    .start_run(argument::<StartRunCommand>(payload, "command")?)
                    .await,
            ),
            "get_run_outputs" => encode(
                state
                    .get_run_outputs(&argument::<String>(payload, "runId")?)
                    .await,
            ),
            "list_open_waits" => encode(
                state
                    .list_open_waits(&argument::<String>(payload, "runId")?)
                    .await,
            ),
            "list_run_events" => encode(
                state
                    .list_run_events(
                        &argument::<String>(payload, "runId")?,
                        argument::<u64>(payload, "afterDurableSeq")?,
                        argument::<u32>(payload, "limit")?,
                    )
                    .await,
            ),
            "satisfy_wait" => encode(
                state
                    .satisfy_wait(argument::<SatisfyWaitInput>(payload, "input")?)
                    .await,
            ),
            "resolve_effect_unknown" => encode(
                state
                    .resolve_effect_unknown(argument::<ResolveEffectUnknownInput>(
                        payload, "input",
                    )?)
                    .await,
            ),
            "fork_context" => encode(
                state
                    .fork_context(argument::<ForkContextCommand>(payload, "command")?)
                    .await,
            ),
            "merge_context" => encode(
                state
                    .merge_context(argument::<MergeContextCommand>(payload, "command")?)
                    .await,
            ),
            _ => unreachable!(),
        }
    }
    .await;
    Some(result)
}
