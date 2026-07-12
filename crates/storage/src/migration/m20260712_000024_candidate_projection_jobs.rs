use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260712_000024_candidate_projection_jobs"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for statement in UP {
            manager
                .get_connection()
                .execute_unprepared(statement)
                .await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE candidate_projection_jobs")
            .await?;
        Ok(())
    }
}

const UP: &[&str] = &[
    r#"CREATE TABLE candidate_projection_jobs (
        run_id TEXT PRIMARY KEY REFERENCES graph_runs(id),
        terminal_event_seq INTEGER NOT NULL CHECK (terminal_event_seq > 0),
        terminal_status TEXT NOT NULL CHECK (terminal_status IN ('completed','failed','cancelled')),
        status TEXT NOT NULL CHECK (status IN ('pending','claimed','done','conflicted','failed')),
        available_at INTEGER NOT NULL,
        claimed_by TEXT,
        lease_until INTEGER,
        attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
        last_error_object_id TEXT REFERENCES content_objects(id),
        created_at INTEGER NOT NULL,
        completed_at INTEGER,
        CHECK ((status = 'claimed') = (claimed_by IS NOT NULL AND lease_until IS NOT NULL)),
        CHECK ((status IN ('done','conflicted','failed')) = (completed_at IS NOT NULL))
    )"#,
    "CREATE INDEX candidate_projection_jobs_ready ON candidate_projection_jobs(status, available_at)",
];
