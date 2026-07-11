use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseConnection};
use sea_orm_migration::MigratorTrait;

use crate::{StorageResult, migration::Migrator};

#[derive(Clone)]
pub struct SqliteStore {
    pub(crate) db: DatabaseConnection,
}

impl SqliteStore {
    pub async fn connect(url: impl Into<String>) -> StorageResult<Self> {
        let mut options = ConnectOptions::new(url.into());
        options.max_connections(1).sqlx_logging(false);
        let db = Database::connect(options).await?;
        db.execute_unprepared("PRAGMA foreign_keys = ON").await?;
        db.execute_unprepared("PRAGMA journal_mode = WAL").await?;
        db.execute_unprepared("PRAGMA busy_timeout = 5000").await?;
        Migrator::up(&db, None).await?;
        Ok(Self { db })
    }

    pub async fn ping(&self) -> StorageResult<()> {
        self.db.execute_unprepared("SELECT 1").await?;
        Ok(())
    }
}
