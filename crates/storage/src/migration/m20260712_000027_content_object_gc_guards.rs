use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260712_000027_content_object_gc_guards"
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
        for statement in DOWN {
            manager
                .get_connection()
                .execute_unprepared(statement)
                .await?;
        }
        Ok(())
    }
}

const UP: &[&str] = &[
    "CREATE INDEX content_objects_lifecycle_created ON content_objects(lifecycle, created_at)",
    r#"CREATE TRIGGER content_object_refs_require_live_insert
       BEFORE INSERT ON content_object_refs
       WHEN NOT EXISTS (
           SELECT 1 FROM content_objects
           WHERE id = NEW.object_id AND lifecycle = 'live'
       )
       BEGIN
           SELECT RAISE(ABORT, 'content object is not live');
       END"#,
    r#"CREATE TRIGGER content_object_refs_require_live_update
       BEFORE UPDATE OF object_id ON content_object_refs
       WHEN NOT EXISTS (
           SELECT 1 FROM content_objects
           WHERE id = NEW.object_id AND lifecycle = 'live'
       )
       BEGIN
           SELECT RAISE(ABORT, 'content object is not live');
       END"#,
];

const DOWN: &[&str] = &[
    "DROP TRIGGER content_object_refs_require_live_update",
    "DROP TRIGGER content_object_refs_require_live_insert",
    "DROP INDEX content_objects_lifecycle_created",
];
