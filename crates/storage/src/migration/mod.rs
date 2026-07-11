mod m20260712_000001_graph;
mod m20260712_000002_runtime_bootstrap;
mod m20260712_000003_fifo_runtime;
mod m20260712_000004_runtime_control;
mod m20260712_000005_runtime_timers;

use sea_orm_migration::prelude::*;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260712_000001_graph::Migration),
            Box::new(m20260712_000002_runtime_bootstrap::Migration),
            Box::new(m20260712_000003_fifo_runtime::Migration),
            Box::new(m20260712_000004_runtime_control::Migration),
            Box::new(m20260712_000005_runtime_timers::Migration),
        ]
    }
}
