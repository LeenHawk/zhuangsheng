use std::collections::BTreeMap;

use zhuangsheng_core::{
    application::ApplicationError,
    llm::{
        LlmOperationExecutionPin,
        adapter::AdapterResources,
        ir::{LlmRequestIr, LlmTurnItemIr, OpaqueContinuationRef},
    },
};

use super::LocalLlmExecutor;

pub(super) async fn resolve_opaque_resources(
    executor: &LocalLlmExecutor,
    operation: &LlmOperationExecutionPin,
    request: &LlmRequestIr,
    now: i64,
) -> Result<AdapterResources, ApplicationError> {
    let mut references: BTreeMap<String, OpaqueContinuationRef> = BTreeMap::new();
    if let Some(reference) = &request.continuation {
        insert(&mut references, reference)?;
    }
    for item in &request.transcript {
        let reference = match item {
            LlmTurnItemIr::HostedTool {
                opaque_item_ref, ..
            }
            | LlmTurnItemIr::Reasoning {
                opaque_item_ref, ..
            } => opaque_item_ref.as_ref(),
            _ => None,
        };
        if let Some(reference) = reference {
            insert(&mut references, reference)?;
        }
    }
    let references: Vec<_> = references.into_values().collect();
    let opaque_entries = executor
        .store
        .load_llm_opaque_entries(&references, operation, now)
        .await
        .map_err(ApplicationError::from)?;
    Ok(AdapterResources {
        opaque_entries,
        ..AdapterResources::default()
    })
}

fn insert(
    references: &mut BTreeMap<String, OpaqueContinuationRef>,
    reference: &OpaqueContinuationRef,
) -> Result<(), ApplicationError> {
    let key = format!(
        "{}:{}",
        reference.entry_ref.object_id, reference.entry_ref.entry_key
    );
    if let Some(existing) = references.insert(key, reference.clone())
        && existing != *reference
    {
        return Err(ApplicationError::Internal);
    }
    Ok(())
}
