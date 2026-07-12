use std::collections::{BTreeSet, HashMap};

use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    application::tool::RegisteredToolView,
    canonical,
    graph::ToolGrant,
    llm::{
        ResolvedToolDescriptor, ToolDescriptor, compile_tool_descriptor,
        validate_resolved_tool_descriptor,
    },
};

use crate::{
    StorageError, StorageResult,
    graph::{helpers::sql, schema_bundle::verify_compilation_bundle},
};

pub(crate) async fn load_registered_tool<C: ConnectionTrait>(
    connection: &C,
    tool_id: &str,
    version: &str,
) -> StorageResult<RegisteredToolView> {
    let row = connection.query_one_raw(sql(
        "SELECT descriptor_json, schema_bundle_object_id, descriptor_digest, implementation_digest, executor_key, enabled, created_at, updated_at FROM tool_registry_entries WHERE tool_id = ? AND tool_version = ?",
        vec![tool_id.into(), version.into()],
    )).await?.ok_or_else(|| StorageError::NotFound {
        kind: "tool_descriptor",
        id: format!("{tool_id}:{version}"),
    })?;
    let descriptor_json: String = row.try_get("", "descriptor_json")?;
    let descriptor: ToolDescriptor = serde_json::from_str(&descriptor_json)
        .map_err(|_| StorageError::Integrity("tool descriptor JSON is invalid".into()))?;
    if descriptor.tool_id != tool_id
        || descriptor.version != version
        || canonical::to_string(&descriptor)? != descriptor_json
    {
        return Err(StorageError::Integrity(
            "tool descriptor identity or canonical encoding mismatch".into(),
        ));
    }
    let compilations = compile_tool_descriptor(&descriptor)
        .map_err(|error| StorageError::Integrity(error.to_string()))?;
    let bundle_id: String = row.try_get("", "schema_bundle_object_id")?;
    verify_compilation_bundle(connection, &bundle_id, &compilations).await?;
    let resolved = ResolvedToolDescriptor {
        descriptor,
        descriptor_digest: row.try_get("", "descriptor_digest")?,
        schema_compilation_digests: compilations
            .iter()
            .map(|item| item.compiled_payload_hash.clone())
            .collect(),
        implementation_digest: row.try_get("", "implementation_digest")?,
        executor_key: row.try_get("", "executor_key")?,
    };
    validate_resolved_tool_descriptor(&resolved)
        .map_err(|error| StorageError::Integrity(error.to_string()))?;
    Ok(RegisteredToolView {
        resolved,
        enabled: row.try_get::<i64>("", "enabled")? != 0,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

pub(crate) async fn load_granted_tools<C: ConnectionTrait>(
    connection: &C,
    grants: &[ToolGrant],
    require_enabled: bool,
) -> StorageResult<Vec<ResolvedToolDescriptor>> {
    let identities: BTreeSet<_> = grants
        .iter()
        .map(|grant| (grant.tool_id.clone(), grant.version.clone()))
        .collect();
    let mut resolved = Vec::with_capacity(identities.len());
    for (tool_id, version) in identities {
        let registered = load_registered_tool(connection, &tool_id, &version).await?;
        if require_enabled && !registered.enabled {
            return Err(StorageError::Conflict("tool_descriptor_disabled"));
        }
        resolved.push(registered.resolved);
    }
    resolved.sort_by(|left, right| {
        (&left.descriptor.tool_id, &left.descriptor.version)
            .cmp(&(&right.descriptor.tool_id, &right.descriptor.version))
    });
    Ok(resolved)
}

pub(crate) async fn load_tool_dependency_map<C: ConnectionTrait>(
    connection: &C,
    grants: &[ToolGrant],
) -> StorageResult<HashMap<(String, String), ResolvedToolDescriptor>> {
    let identities: BTreeSet<_> = grants
        .iter()
        .map(|grant| (grant.tool_id.clone(), grant.version.clone()))
        .collect();
    let mut result = HashMap::new();
    for (tool_id, version) in identities {
        match load_registered_tool(connection, &tool_id, &version).await {
            Ok(registered) if registered.enabled => {
                result.insert((tool_id, version), registered.resolved);
            }
            Ok(_) | Err(StorageError::NotFound { .. }) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(result)
}
