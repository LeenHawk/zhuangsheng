use sea_orm::ConnectionTrait;

use crate::graph::helpers::{put_inline_object, sql};

use super::store;

const NOW: i64 = 1_700_000_800_000;
const GRACE: i64 = 60_000;

#[tokio::test]
async fn gc_deletes_only_unrooted_objects_after_grace_and_fences_new_refs() {
    let store = store().await;
    let orphan = put_inline_object(&store.db, b"orphan", NOW - GRACE - 1)
        .await
        .unwrap();
    let owner_root = put_inline_object(&store.db, b"owner-root", NOW - GRACE - 1)
        .await
        .unwrap();
    store.db.execute_raw(sql(
        "INSERT INTO content_object_refs (object_id, owner_kind, owner_id, role, created_at) VALUES (?, 'test', 'owner', 'root', ?)",
        vec![owner_root.clone().into(), (NOW - GRACE).into()],
    )).await.unwrap();
    let foreign_key_root = put_inline_object(&store.db, b"foreign-key-root", NOW - GRACE - 1)
        .await
        .unwrap();
    store.db.execute_raw(sql(
        "INSERT INTO application_command_receipts (scope, idempotency_key, request_digest, command_kind, status, result_object_id, created_at, completed_at) VALUES ('gc-test', 'root', 'digest', 'gc.test', 'completed', ?, ?, ?)",
        vec![foreign_key_root.clone().into(), (NOW - GRACE).into(), (NOW - GRACE).into()],
    )).await.unwrap();
    let recent = put_inline_object(&store.db, b"recent", NOW - GRACE + 1)
        .await
        .unwrap();

    let report = store
        .maintain_content_objects(NOW, GRACE, 100)
        .await
        .unwrap();
    assert_eq!(report.scanned, 2);
    assert_eq!(report.deleted, 1);
    assert_eq!(report.rooted_without_owner_ref, 1);
    assert_eq!(lifecycle(&store, &orphan).await, "deleted");
    assert_eq!(lifecycle(&store, &owner_root).await, "live");
    assert_eq!(lifecycle(&store, &foreign_key_root).await, "live");
    assert_eq!(lifecycle(&store, &recent).await, "live");

    let bytes: Option<Vec<u8>> = store
        .db
        .query_one_raw(sql(
            "SELECT inline_bytes FROM content_objects WHERE id = ?",
            vec![orphan.clone().into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "inline_bytes")
        .unwrap();
    assert_eq!(bytes, None);
    assert!(store.db.execute_raw(sql(
        "INSERT INTO content_object_refs (object_id, owner_kind, owner_id, role, created_at) VALUES (?, 'test', 'late-owner', 'root', ?)",
        vec![orphan.clone().into(), NOW.into()],
    )).await.is_err());
    assert_eq!(
        put_inline_object(&store.db, b"orphan", NOW + 1)
            .await
            .unwrap(),
        orphan
    );
    store.db.execute_raw(sql(
        "INSERT INTO content_object_refs (object_id, owner_kind, owner_id, role, created_at) VALUES (?, 'test', 'rehydrated-owner', 'root', ?)",
        vec![orphan.clone().into(), (NOW + 1).into()],
    )).await.unwrap();
    assert_eq!(lifecycle(&store, &orphan).await, "live");
}

async fn lifecycle(store: &crate::SqliteStore, object_id: &str) -> String {
    store
        .db
        .query_one_raw(sql(
            "SELECT lifecycle FROM content_objects WHERE id = ?",
            vec![object_id.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "lifecycle")
        .unwrap()
}
