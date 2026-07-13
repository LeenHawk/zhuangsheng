use serde_json::Value;
use zhuangsheng_core::application::memory::{
    ApplyMemoryProposalCommand, ListMemoryProposalsCommand, MemorySearchCommand,
};

use crate::{CommandResult, DecideMemoryProposalInput, ProposeMemoryChangeInput, TauriAdapter};

use super::{argument, encode};

pub async fn dispatch(
    state: &TauriAdapter,
    operation: &str,
    payload: &Value,
) -> Option<CommandResult<Value>> {
    if !matches!(
        operation,
        "list_memory_proposals"
            | "propose_memory_change"
            | "decide_memory_proposal"
            | "apply_memory_proposal"
            | "get_memory_record"
            | "search_memory"
    ) {
        return None;
    }
    let result: CommandResult<Value> = async {
        match operation {
            "list_memory_proposals" => encode(
                state
                    .list_memory_proposals(argument::<ListMemoryProposalsCommand>(
                        payload, "command",
                    )?)
                    .await,
            ),
            "propose_memory_change" => encode(
                state
                    .propose_memory_change(argument::<ProposeMemoryChangeInput>(payload, "input")?)
                    .await,
            ),
            "decide_memory_proposal" => encode(
                state
                    .decide_memory_proposal(argument::<DecideMemoryProposalInput>(
                        payload, "input",
                    )?)
                    .await,
            ),
            "apply_memory_proposal" => encode(
                state
                    .apply_memory_proposal(argument::<ApplyMemoryProposalCommand>(
                        payload, "command",
                    )?)
                    .await,
            ),
            "get_memory_record" => encode(
                state
                    .get_memory_record(&argument::<String>(payload, "memoryId")?)
                    .await,
            ),
            "search_memory" => encode(
                state
                    .search_memory(argument::<MemorySearchCommand>(payload, "command")?)
                    .await,
            ),
            _ => unreachable!(),
        }
    }
    .await;
    Some(result)
}
