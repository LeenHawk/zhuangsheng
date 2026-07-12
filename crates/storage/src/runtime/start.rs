use sea_orm::{ConnectionTrait, TransactionTrait};
use zhuangsheng_core::runtime::{RunView, StartRunCommand};

use crate::{
    SqliteStore, StorageError, StorageResult,
    graph::helpers::{new_id, now_ms, sql},
};

use super::{
    query::load_run,
    start_insert::{insert_new_run, run_identity},
};

impl SqliteStore {
    pub async fn start_run(&self, command: StartRunCommand) -> StorageResult<RunView> {
        let identity = run_identity(&command)?;
        let transaction = self.db.begin().await?;
        if let Some(run) = find_existing_run(
            &transaction,
            &identity.scope,
            &command.idempotency_key,
            &identity.digest,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(run);
        }
        let run_id = new_id("run");
        insert_new_run(&transaction, &command, &run_id, now_ms()).await?;
        transaction.commit().await?;
        load_run(&self.db, &run_id).await
    }
}

async fn find_existing_run<C: ConnectionTrait>(
    connection: &C,
    scope: &str,
    key: &str,
    digest: &str,
) -> StorageResult<Option<RunView>> {
    let row = connection
        .query_one_raw(sql(
            "SELECT id, request_digest FROM graph_runs WHERE request_idempotency_scope = ? AND request_idempotency_key = ?",
            vec![scope.into(), key.into()],
        ))
        .await?;
    let Some(row) = row else { return Ok(None) };
    if row.try_get::<String>("", "request_digest")? != digest {
        return Err(StorageError::IdempotencyConflict);
    }
    let id: String = row.try_get("", "id")?;
    Ok(Some(load_run(connection, &id).await?))
}
