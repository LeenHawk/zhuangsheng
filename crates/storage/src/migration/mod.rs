mod m20260712_000001_graph;
mod m20260712_000002_runtime_bootstrap;
mod m20260712_000003_fifo_runtime;
mod m20260712_000004_runtime_control;
mod m20260712_000005_runtime_timers;
mod m20260712_000006_router_runtime;
mod m20260712_000007_working_context;
mod m20260712_000008_memory_read_sets;
mod m20260712_000009_long_term_memory;
mod m20260712_000010_llm_config;
mod m20260712_000011_secret_store;
mod m20260712_000012_llm_effect_ledger;

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
            Box::new(m20260712_000006_router_runtime::Migration),
            Box::new(m20260712_000007_working_context::Migration),
            Box::new(m20260712_000008_memory_read_sets::Migration),
            Box::new(m20260712_000009_long_term_memory::Migration),
            Box::new(m20260712_000010_llm_config::Migration),
            Box::new(m20260712_000011_secret_store::Migration),
            Box::new(m20260712_000012_llm_effect_ledger::Migration),
        ]
    }
}
