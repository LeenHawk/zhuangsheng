use sea_orm::ConnectionTrait;
use serde_json::json;
use zhuangsheng_core::{
    DomainError,
    runtime::{RunContextCommand, StartRunCommand},
};

use crate::{StorageError, graph::helpers::sql};

use super::{applied_graph, store};

#[tokio::test]
async fn temporary_start_is_atomic_idempotent_and_durable() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let url = format!("sqlite://{}?mode=rwc", file.path().display());
    let (run_id, revision_id, context_id, branch_id, commit_id) = {
        let store = crate::SqliteStore::connect(&url).await.unwrap();
        let revision = applied_graph(&store, "durable-run").await;
        let command = start(
            &revision.id,
            "run-1",
            RunContextCommand::Temporary,
            json!({"message":"hello"}),
        );
        let first = store.start_run(command.clone()).await.unwrap();
        let replay = store.start_run(command).await.unwrap();
        assert_eq!(first, replay);
        assert_eq!(first.last_durable_seq, 3);
        assert_counts(&store, 1, 1, 1, 1, 1, 3).await;

        let conflict = store
            .start_run(start(
                &revision.id,
                "run-1",
                RunContextCommand::Temporary,
                json!({"message":"different"}),
            ))
            .await
            .unwrap_err();
        assert!(matches!(conflict, StorageError::IdempotencyConflict));
        (
            first.id,
            revision.id,
            first.context_id,
            first.branch_id,
            first.input_commit_id,
        )
    };

    let reopened = crate::SqliteStore::connect(&url).await.unwrap();
    let loaded = reopened.get_run(&run_id).await.unwrap();
    assert_eq!(loaded.context_id, context_id);
    assert_eq!(loaded.branch_id, branch_id);
    assert_eq!(loaded.input_commit_id, commit_id);
    reopened.get_graph_revision(&revision_id).await.unwrap();
    let head: String = reopened
        .db
        .query_one(sql(
            "SELECT head_commit_id FROM context_branches WHERE context_id = ? AND id = ?",
            vec![context_id.into(), branch_id.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "head_commit_id")
        .unwrap();
    assert_eq!(head, commit_id);
}

#[tokio::test]
async fn existing_binding_checks_head_and_invalid_input_writes_nothing() {
    let store = store().await;
    let revision = applied_graph(&store, "existing-run").await;
    let first = store
        .start_run(start(
            &revision.id,
            "temporary",
            RunContextCommand::Temporary,
            json!({"message":"first"}),
        ))
        .await
        .unwrap();
    let existing = RunContextCommand::Existing {
        context_id: first.context_id.clone(),
        branch_id: first.branch_id.clone(),
        expected_head_commit_id: first.input_commit_id.clone(),
    };
    let second = store
        .start_run(start(
            &revision.id,
            "existing",
            existing,
            json!({"message":"second"}),
        ))
        .await
        .unwrap();
    assert_eq!(second.context_id, first.context_id);
    assert_eq!(second.input_commit_id, first.input_commit_id);

    let stale = store
        .start_run(start(
            &revision.id,
            "stale",
            RunContextCommand::Existing {
                context_id: first.context_id.clone(),
                branch_id: first.branch_id.clone(),
                expected_head_commit_id: "commit_stale".into(),
            },
            json!({"message":"stale"}),
        ))
        .await
        .unwrap_err();
    assert!(matches!(stale, StorageError::Conflict("context_head")));

    let invalid = store
        .start_run(start(
            &revision.id,
            "invalid",
            RunContextCommand::Temporary,
            json!({"message":2}),
        ))
        .await
        .unwrap_err();
    assert!(matches!(
        invalid,
        StorageError::Domain(DomainError::SchemaValidation(_))
    ));
    assert_counts(&store, 2, 1, 1, 2, 2, 6).await;
}

fn start(
    revision_id: &str,
    key: &str,
    context: RunContextCommand,
    input: serde_json::Value,
) -> StartRunCommand {
    StartRunCommand {
        graph_revision_id: revision_id.into(),
        input,
        context,
        deadline_at: None,
        idempotency_key: key.into(),
    }
}

async fn assert_counts(
    store: &crate::SqliteStore,
    runs: i64,
    contexts: i64,
    branches: i64,
    instances: i64,
    attempts: i64,
    events: i64,
) {
    for (table, expected) in [
        ("graph_runs", runs),
        ("contexts", contexts),
        ("context_branches", branches),
        ("node_instances", instances),
        ("node_attempts", attempts),
        ("run_events", events),
    ] {
        let row = store
            .db
            .query_one(sql(
                &format!("SELECT COUNT(*) AS count FROM {table}"),
                vec![],
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            row.try_get::<i64>("", "count").unwrap(),
            expected,
            "{table}"
        );
    }
}
