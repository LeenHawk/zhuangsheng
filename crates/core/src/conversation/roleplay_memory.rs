use crate::graph::{LlmMemoryBinding, MemoryReadConsistency, StaticMemoryReadSource};

pub(super) fn is_user_mode_compatible(memory: Option<&LlmMemoryBinding>) -> bool {
    let Some(memory) = memory else {
        return true;
    };
    if !memory.node.working_writes.is_empty()
        || !memory.tools.is_empty()
        || memory.node.reads.len() != 1
    {
        return false;
    }
    let read = &memory.node.reads[0];
    read.id == "history"
        && read.alias == "history"
        && read.required
        && read.consistency == MemoryReadConsistency::Snapshot
        && read.limit.is_none()
        && read.max_bytes == 16 * 1024 * 1024
        && matches!(
            &read.source,
            StaticMemoryReadSource::ConversationHistory { scope }
                if scope == "run-context"
        )
}
