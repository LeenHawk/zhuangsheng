use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseConnection};
use sea_orm_migration::MigratorTrait;

use std::sync::Arc;

use crate::{StorageResult, migration::Migrator, secret::SecretSessionRegistry};

#[derive(Clone)]
pub struct SqliteStore {
    pub(crate) db: DatabaseConnection,
    pub(crate) secret_sessions: Arc<tokio::sync::Mutex<SecretSessionRegistry>>,
}

impl SqliteStore {
    pub async fn connect(url: impl Into<String>) -> StorageResult<Self> {
        let url = url.into();
        let file_path = sqlite_file_path(&url);
        let mut options = ConnectOptions::new(url);
        options.max_connections(1).sqlx_logging(false);
        let db = Database::connect(options).await?;
        db.execute_unprepared("PRAGMA foreign_keys = ON").await?;
        db.execute_unprepared("PRAGMA journal_mode = WAL").await?;
        db.execute_unprepared("PRAGMA busy_timeout = 5000").await?;
        Migrator::up(&db, None).await?;
        tighten_file_permissions(file_path.as_deref())?;
        Ok(Self {
            db,
            secret_sessions: Arc::new(tokio::sync::Mutex::new(SecretSessionRegistry::new(
                format!("process_{}", ulid::Ulid::new()),
            ))),
        })
    }

    pub async fn ping(&self) -> StorageResult<()> {
        self.db.execute_unprepared("SELECT 1").await?;
        Ok(())
    }
}

fn sqlite_file_path(url: &str) -> Option<std::path::PathBuf> {
    let value = url.strip_prefix("sqlite://")?.split('?').next()?;
    (!value.is_empty() && value != ":memory:").then(|| std::path::PathBuf::from(value))
}

#[cfg(unix)]
fn tighten_file_permissions(path: Option<&std::path::Path>) -> StorageResult<()> {
    use std::os::unix::fs::PermissionsExt;

    if let Some(path) = path {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn tighten_file_permissions(_path: Option<&std::path::Path>) -> StorageResult<()> {
    Ok(())
}
