use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260712_000002_runtime_bootstrap"
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
    r#"CREATE TABLE contexts (
        id TEXT PRIMARY KEY NOT NULL,
        kind TEXT NOT NULL CHECK (kind IN ('temporary','conversation')),
        status TEXT NOT NULL CHECK (status IN ('active','archived')),
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    )"#,
    r#"CREATE TABLE version_commits (
        id TEXT PRIMARY KEY NOT NULL,
        aggregate_kind TEXT NOT NULL CHECK (aggregate_kind IN ('working_context','long_term_memory','artifact_metadata')),
        aggregate_id TEXT NOT NULL,
        lineage_key TEXT NOT NULL,
        sequence_no INTEGER NOT NULL CHECK (sequence_no > 0),
        operation_id TEXT NOT NULL,
        patch_object_id TEXT REFERENCES content_objects(id),
        initial_snapshot_object_id TEXT REFERENCES content_objects(id),
        merge_resolution_object_id TEXT REFERENCES content_objects(id),
        schema_version INTEGER NOT NULL CHECK (schema_version > 0),
        policy_version INTEGER NOT NULL CHECK (policy_version > 0),
        author_kind TEXT NOT NULL CHECK (author_kind IN ('user','system','node','tool','application')),
        author_id TEXT,
        origin_run_id TEXT REFERENCES graph_runs(id),
        origin_node_instance_id TEXT REFERENCES node_instances(id),
        created_at INTEGER NOT NULL,
        CHECK (patch_object_id IS NOT NULL OR initial_snapshot_object_id IS NOT NULL),
        UNIQUE(aggregate_kind, aggregate_id, lineage_key, sequence_no),
        UNIQUE(aggregate_kind, aggregate_id, lineage_key, operation_id)
    )"#,
    r#"CREATE TABLE commit_parents (
        commit_id TEXT NOT NULL REFERENCES version_commits(id),
        parent_commit_id TEXT NOT NULL REFERENCES version_commits(id),
        parent_order INTEGER NOT NULL CHECK (parent_order IN (0,1)),
        PRIMARY KEY(commit_id, parent_order),
        UNIQUE(commit_id, parent_commit_id)
    )"#,
    r#"CREATE TABLE context_branches (
        id TEXT PRIMARY KEY NOT NULL,
        context_id TEXT NOT NULL REFERENCES contexts(id),
        parent_branch_id TEXT,
        fork_commit_id TEXT NOT NULL REFERENCES version_commits(id),
        head_commit_id TEXT NOT NULL REFERENCES version_commits(id),
        creation_operation_id TEXT NOT NULL,
        status TEXT NOT NULL CHECK (status IN ('active','merged','abandoned')),
        name TEXT,
        retention_until INTEGER,
        pinned INTEGER NOT NULL DEFAULT 0 CHECK (pinned IN (0,1)),
        audit_hold INTEGER NOT NULL DEFAULT 0 CHECK (audit_hold IN (0,1)),
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        UNIQUE(context_id, id),
        UNIQUE(context_id, creation_operation_id),
        FOREIGN KEY(context_id, parent_branch_id) REFERENCES context_branches(context_id, id)
    )"#,
    r#"CREATE TABLE materialized_projections (
        aggregate_kind TEXT NOT NULL,
        aggregate_id TEXT NOT NULL,
        lineage_key TEXT NOT NULL,
        head_commit_id TEXT NOT NULL REFERENCES version_commits(id),
        projection_json TEXT CHECK (projection_json IS NULL OR json_valid(projection_json)),
        projection_object_id TEXT REFERENCES content_objects(id),
        schema_version INTEGER NOT NULL CHECK (schema_version > 0),
        updated_at INTEGER NOT NULL,
        CHECK ((projection_json IS NULL) <> (projection_object_id IS NULL)),
        PRIMARY KEY(aggregate_kind, aggregate_id, lineage_key)
    )"#,
    r#"CREATE TABLE graph_runs (
        id TEXT PRIMARY KEY NOT NULL,
        request_idempotency_scope TEXT NOT NULL,
        request_idempotency_key TEXT NOT NULL,
        request_digest TEXT NOT NULL,
        graph_revision_id TEXT NOT NULL REFERENCES graph_revisions(id),
        graph_content_hash TEXT NOT NULL,
        execution_manifest_object_id TEXT NOT NULL REFERENCES content_objects(id),
        context_id TEXT NOT NULL REFERENCES contexts(id),
        branch_id TEXT NOT NULL,
        input_commit_id TEXT NOT NULL REFERENCES version_commits(id),
        output_commit_id TEXT REFERENCES version_commits(id),
        status TEXT NOT NULL CHECK (status IN ('created','running','waiting','interrupting','interrupted','completed','failed','cancelled')),
        control_epoch INTEGER NOT NULL DEFAULT 0 CHECK (control_epoch >= 0),
        drain_epoch INTEGER,
        limits_object_id TEXT NOT NULL REFERENCES content_objects(id),
        run_input_object_id TEXT NOT NULL REFERENCES content_objects(id),
        run_outputs_object_id TEXT REFERENCES content_objects(id),
        terminal_error_object_id TEXT REFERENCES content_objects(id),
        started_at INTEGER,
        deadline_at INTEGER NOT NULL,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        finished_at INTEGER,
        UNIQUE(request_idempotency_scope, request_idempotency_key),
        FOREIGN KEY(context_id, branch_id) REFERENCES context_branches(context_id, id)
    )"#,
    r#"CREATE TABLE run_execution_counters (
        run_id TEXT PRIMARY KEY NOT NULL REFERENCES graph_runs(id),
        next_enqueue_seq INTEGER NOT NULL,
        next_output_seq INTEGER NOT NULL,
        total_activations INTEGER NOT NULL,
        total_attempts INTEGER NOT NULL,
        total_queue_values INTEGER NOT NULL,
        pending_queue_values INTEGER NOT NULL,
        open_waits INTEGER NOT NULL,
        coordinator_buffered_values INTEGER NOT NULL
    )"#,
    r#"CREATE TABLE node_scheduling_cursors (
        run_id TEXT NOT NULL REFERENCES graph_runs(id),
        node_id TEXT NOT NULL,
        next_activation_seq INTEGER NOT NULL CHECK (next_activation_seq > 0),
        PRIMARY KEY(run_id, node_id)
    )"#,
    r#"CREATE TABLE node_instances (
        id TEXT PRIMARY KEY NOT NULL,
        run_id TEXT NOT NULL REFERENCES graph_runs(id),
        node_id TEXT NOT NULL,
        activation_seq INTEGER NOT NULL CHECK (activation_seq > 0),
        status TEXT NOT NULL CHECK (status IN ('ready','running','waiting','completed','failed','cancelled')),
        graph_revision_id TEXT NOT NULL REFERENCES graph_revisions(id),
        execution_snapshot_object_id TEXT REFERENCES content_objects(id),
        operation_taxonomy_version INTEGER,
        adapter_decoder_version INTEGER,
        preset_version_id TEXT,
        inputs_object_id TEXT NOT NULL REFERENCES content_objects(id),
        final_outputs_object_id TEXT REFERENCES content_objects(id),
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        CHECK ((operation_taxonomy_version IS NULL) = (adapter_decoder_version IS NULL)),
        UNIQUE(run_id, node_id, activation_seq)
    )"#,
    r#"CREATE TABLE node_attempts (
        id TEXT PRIMARY KEY NOT NULL,
        node_instance_id TEXT NOT NULL REFERENCES node_instances(id),
        attempt_no INTEGER NOT NULL CHECK (attempt_no > 0),
        retry_ordinal INTEGER NOT NULL DEFAULT 0,
        invocation_kind TEXT NOT NULL CHECK (invocation_kind IN ('start','retry','resume','reconcile')),
        status TEXT NOT NULL CHECK (status IN ('queued','leased','running','waiting','completed','failed','timed_out','cancelled','outcome_unknown')),
        run_control_epoch INTEGER NOT NULL,
        lease_fence INTEGER NOT NULL DEFAULT 0,
        worker_id TEXT,
        lease_until INTEGER,
        deadline_at INTEGER,
        idempotency_key TEXT NOT NULL UNIQUE,
        result_idempotency_key TEXT UNIQUE,
        executor_object_id TEXT NOT NULL REFERENCES content_objects(id),
        continuation_object_id TEXT REFERENCES content_objects(id),
        error_object_id TEXT REFERENCES content_objects(id),
        started_at INTEGER,
        finished_at INTEGER,
        UNIQUE(node_instance_id, attempt_no)
    )"#,
    r#"CREATE TABLE scheduler_wakeups (
        id TEXT PRIMARY KEY NOT NULL,
        run_id TEXT NOT NULL REFERENCES graph_runs(id),
        node_id TEXT,
        kind TEXT NOT NULL CHECK (kind IN ('node_maybe_ready','attempt_ready','timer','settle_run')),
        caused_by_seq INTEGER NOT NULL,
        dedupe_key TEXT NOT NULL UNIQUE,
        status TEXT NOT NULL CHECK (status IN ('pending','claimed','done')),
        available_at INTEGER NOT NULL,
        claimed_by TEXT,
        lease_until INTEGER,
        created_at INTEGER NOT NULL
    )"#,
    r#"CREATE TABLE run_event_counters (
        run_id TEXT PRIMARY KEY NOT NULL REFERENCES graph_runs(id),
        next_seq INTEGER NOT NULL CHECK (next_seq > 0)
    )"#,
    r#"CREATE TABLE run_events (
        id TEXT PRIMARY KEY NOT NULL,
        run_id TEXT NOT NULL REFERENCES graph_runs(id),
        seq INTEGER NOT NULL CHECK (seq > 0),
        context_branch_id TEXT,
        node_instance_id TEXT REFERENCES node_instances(id),
        attempt_id TEXT REFERENCES node_attempts(id),
        causation_event_id TEXT REFERENCES run_events(id),
        correlation_id TEXT,
        event_type TEXT NOT NULL,
        schema_version INTEGER NOT NULL CHECK (schema_version > 0),
        importance TEXT NOT NULL CHECK (importance IN ('critical','info','debug')),
        payload_json TEXT CHECK (payload_json IS NULL OR json_valid(payload_json)),
        payload_object_id TEXT REFERENCES content_objects(id),
        created_at INTEGER NOT NULL,
        UNIQUE(run_id, seq),
        CHECK ((payload_json IS NULL) <> (payload_object_id IS NULL))
    )"#,
    "CREATE INDEX context_branches_status ON context_branches(context_id, status)",
    "CREATE INDEX graph_runs_status ON graph_runs(status, updated_at)",
    "CREATE INDEX node_instances_status ON node_instances(run_id, status)",
    "CREATE UNIQUE INDEX node_instances_one_active ON node_instances(run_id, node_id) WHERE status IN ('ready','running','waiting')",
    "CREATE UNIQUE INDEX node_attempts_one_active ON node_attempts(node_instance_id) WHERE status IN ('queued','leased','running')",
    "CREATE INDEX scheduler_wakeups_ready ON scheduler_wakeups(status, available_at)",
    "CREATE INDEX run_events_sequence ON run_events(run_id, seq)",
];

const DOWN: &[&str] = &[
    "DROP TABLE run_events",
    "DROP TABLE run_event_counters",
    "DROP TABLE scheduler_wakeups",
    "DROP TABLE node_attempts",
    "DROP TABLE node_instances",
    "DROP TABLE node_scheduling_cursors",
    "DROP TABLE run_execution_counters",
    "DROP TABLE graph_runs",
    "DROP TABLE materialized_projections",
    "DROP TABLE context_branches",
    "DROP TABLE commit_parents",
    "DROP TABLE version_commits",
    "DROP TABLE contexts",
];
