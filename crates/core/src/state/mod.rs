mod patch;
mod types;

pub use patch::{StatePatchError, apply_patch, patches_conflict, validate_patch};
pub use types::{ActorKind, ActorRef, AggregateKind, JsonPatchOp, StatePatch};

#[cfg(test)]
mod tests;
