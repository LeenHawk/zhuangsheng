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
mod m20260712_000013_durable_waits;
mod m20260712_000014_fix_tool_read_set_fk;
mod m20260712_000015_tool_registry;
mod m20260712_000016_llm_stream_chunks;
mod m20260712_000017_llm_output_repairs;
mod m20260712_000018_join_by_key;
mod m20260712_000019_aggregation_windows;
mod m20260712_000020_artifact_staging;
mod m20260712_000021_artifacts;
mod m20260712_000022_conversations;
mod m20260712_000023_conversation_turns;
mod m20260712_000024_candidate_projection_jobs;
mod m20260712_000025_conversation_selections;
mod m20260712_000026_context_merge_conflicts;
mod m20260712_000027_content_object_gc_guards;
mod m20260712_000028_runtime_checkpoints;
mod m20260712_000029_static_context_writes;

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
            Box::new(m20260712_000013_durable_waits::Migration),
            Box::new(m20260712_000014_fix_tool_read_set_fk::Migration),
            Box::new(m20260712_000015_tool_registry::Migration),
            Box::new(m20260712_000016_llm_stream_chunks::Migration),
            Box::new(m20260712_000017_llm_output_repairs::Migration),
            Box::new(m20260712_000018_join_by_key::Migration),
            Box::new(m20260712_000019_aggregation_windows::Migration),
            Box::new(m20260712_000020_artifact_staging::Migration),
            Box::new(m20260712_000021_artifacts::Migration),
            Box::new(m20260712_000022_conversations::Migration),
            Box::new(m20260712_000023_conversation_turns::Migration),
            Box::new(m20260712_000024_candidate_projection_jobs::Migration),
            Box::new(m20260712_000025_conversation_selections::Migration),
            Box::new(m20260712_000026_context_merge_conflicts::Migration),
            Box::new(m20260712_000027_content_object_gc_guards::Migration),
            Box::new(m20260712_000028_runtime_checkpoints::Migration),
            Box::new(m20260712_000029_static_context_writes::Migration),
        ]
    }
}
