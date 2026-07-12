use std::sync::Arc;

use sea_orm::ConnectionTrait;
use zhuangsheng_core::{
    application::ApplicationError,
    application::tool::{PublishToolCommand, SetToolEnabledCommand},
    llm::{build_tool_registry_snapshot, validate_resolved_tool_descriptor},
    runtime::{RunContextCommand, StartRunCommand},
    scheduler::Scheduler,
};

use crate::{
    StorageError,
    config::tool_registry_rows::load_granted_tools,
    graph::helpers::sql,
    tests::{
        llm_tool_support::{echo_grant, prepare_running_tool_attempt},
        llm_tool_test_helpers::{EXECUTOR_KEY, IMPLEMENTATION_DIGEST, descriptor},
        store,
    },
};

#[tokio::test]
async fn tool_publish_is_immutable_compiled_and_discoverable_without_executor_metadata() {
    let store = store().await;
    let command = publish_command("publish-echo");
    let published = store.publish_tool(command.clone()).await.unwrap();
    assert!(validate_resolved_tool_descriptor(&published.resolved).is_ok());
    assert_eq!(published.resolved.executor_key, EXECUTOR_KEY);
    let replay = store.publish_tool(command).await.unwrap();
    assert_eq!(replay, published);
    let duplicate = store
        .publish_tool(publish_command("publish-echo-again"))
        .await;
    assert!(matches!(
        duplicate,
        Err(StorageError::Conflict("tool_version_exists"))
    ));
    let descriptors = store.list_tool_descriptors().await.unwrap();
    assert_eq!(descriptors.len(), 1);
    let public_json = serde_json::to_string(&descriptors).unwrap();
    assert!(!public_json.contains(EXECUTOR_KEY));
    assert!(!public_json.contains(IMPLEMENTATION_DIGEST));
    let refs: i64 = store
        .db
        .query_one(sql(
            "SELECT COUNT(*) AS count FROM content_object_refs WHERE owner_kind = 'tool_registry_entry' AND owner_id = 'echo-tool:1'",
            vec![],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "count")
        .unwrap();
    assert_eq!(refs, 3);

    let disabled = store
        .set_tool_enabled(SetToolEnabledCommand {
            tool_id: "echo-tool".into(),
            version: "1".into(),
            enabled: false,
            idempotency_key: "disable-echo".into(),
        })
        .await
        .unwrap();
    assert!(!disabled.enabled);
    assert!(store.list_tool_descriptors().await.unwrap().is_empty());
    let load = load_granted_tools(&store.db, &[echo_grant()], true).await;
    assert!(matches!(
        load,
        Err(StorageError::Conflict("tool_descriptor_disabled"))
    ));
}

#[tokio::test]
async fn node_instance_pins_registry_material_before_later_disable() {
    let store = store().await;
    let claimed = prepare_running_tool_attempt(&store).await;
    let snapshot = claimed.execution_snapshot.unwrap();
    let revision_id = snapshot.graph_revision_id.clone();
    assert_eq!(
        snapshot.tool_registry,
        build_tool_registry_snapshot(&snapshot.tool_descriptors).unwrap()
    );
    assert_eq!(snapshot.tool_descriptors.len(), 1);
    store
        .set_tool_enabled(SetToolEnabledCommand {
            tool_id: "echo-tool".into(),
            version: "1".into(),
            enabled: false,
            idempotency_key: "disable-pinned-echo".into(),
        })
        .await
        .unwrap();
    let row = store
        .db
        .query_one(sql(
            "SELECT execution_snapshot_object_id FROM node_instances WHERE id = ?",
            vec![claimed.node_instance_id.into()],
        ))
        .await
        .unwrap()
        .unwrap();
    assert!(
        row.try_get::<Option<String>>("", "execution_snapshot_object_id")
            .unwrap()
            .is_some()
    );
    store
        .start_run(StartRunCommand {
            graph_revision_id: revision_id,
            input: serde_json::json!({"message":"new instance"}),
            context: RunContextCommand::Temporary,
            deadline_at: None,
            idempotency_key: "disabled-tool-new-run".into(),
        })
        .await
        .unwrap();
    let result = Scheduler::new(Arc::new(store.clone()), "disabled-tool-worker")
        .run_until_idle(now_ms(), 64)
        .await;
    assert!(matches!(
        result,
        Err(ApplicationError::Conflict("tool_descriptor_disabled"))
    ));
}

#[tokio::test]
async fn corrupted_tool_descriptor_fails_closed_on_load() {
    let store = store().await;
    store
        .publish_tool(publish_command("publish-corrupt"))
        .await
        .unwrap();
    store
        .db
        .execute(sql(
            "UPDATE tool_registry_entries SET descriptor_digest = 'sha256:tampered' WHERE tool_id = 'echo-tool' AND tool_version = '1'",
            vec![],
        ))
        .await
        .unwrap();
    assert!(matches!(
        store.get_registered_tool("echo-tool", "1").await,
        Err(StorageError::Integrity(_))
    ));
}

fn publish_command(idempotency_key: &str) -> PublishToolCommand {
    PublishToolCommand {
        descriptor: descriptor(),
        implementation_digest: IMPLEMENTATION_DIGEST.into(),
        executor_key: EXECUTOR_KEY.into(),
        enabled: true,
        idempotency_key: idempotency_key.into(),
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}
