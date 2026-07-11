use sea_orm_migration::prelude::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20260712_000012_llm_effect_ledger"
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
    r#"CREATE TABLE llm_loop_checkpoints (
        node_instance_id TEXT PRIMARY KEY NOT NULL REFERENCES node_instances(id),
        schema_version INTEGER NOT NULL CHECK (schema_version > 0),
        last_updated_by_attempt_id TEXT NOT NULL REFERENCES node_attempts(id),
        checkpoint_object_id TEXT NOT NULL REFERENCES content_objects(id),
        checkpoint_digest TEXT NOT NULL,
        effect_watermark TEXT,
        updated_at INTEGER NOT NULL
    )"#,
    r#"CREATE TABLE model_calls (
        id TEXT PRIMARY KEY NOT NULL,
        node_instance_id TEXT NOT NULL REFERENCES node_instances(id),
        originating_attempt_id TEXT NOT NULL REFERENCES node_attempts(id),
        call_no INTEGER NOT NULL CHECK (call_no > 0),
        channel_id TEXT NOT NULL REFERENCES llm_channels(id),
        channel_revision_id TEXT NOT NULL REFERENCES llm_channel_revisions(id),
        model_id TEXT NOT NULL,
        operation_key_json TEXT NOT NULL CHECK (json_valid(operation_key_json)),
        operation_taxonomy_version INTEGER NOT NULL CHECK (operation_taxonomy_version > 0),
        adapter_decoder_version INTEGER NOT NULL CHECK (adapter_decoder_version > 0),
        request_object_id TEXT NOT NULL REFERENCES content_objects(id),
        response_object_id TEXT REFERENCES content_objects(id),
        provider_request_id TEXT,
        status TEXT NOT NULL CHECK (status IN (
            'prepared','running','completed','failed','outcome_unknown','retry_ready',
            'cancelled_before_start','abandoned_unknown'
        )),
        usage_json TEXT CHECK (usage_json IS NULL OR json_valid(usage_json)),
        started_at INTEGER NOT NULL,
        finished_at INTEGER,
        UNIQUE(node_instance_id, call_no)
    )"#,
    r#"CREATE TABLE count_calls (
        id TEXT PRIMARY KEY NOT NULL,
        node_instance_id TEXT NOT NULL REFERENCES node_instances(id),
        originating_attempt_id TEXT NOT NULL REFERENCES node_attempts(id),
        count_ordinal INTEGER NOT NULL CHECK (count_ordinal > 0),
        channel_id TEXT NOT NULL REFERENCES llm_channels(id),
        channel_revision_id TEXT NOT NULL REFERENCES llm_channel_revisions(id),
        model_id TEXT NOT NULL,
        operation_key_json TEXT NOT NULL CHECK (json_valid(operation_key_json)),
        operation_taxonomy_version INTEGER NOT NULL CHECK (operation_taxonomy_version > 0),
        adapter_decoder_version INTEGER NOT NULL CHECK (adapter_decoder_version > 0),
        local_counter_id TEXT NOT NULL,
        local_counter_version INTEGER NOT NULL CHECK (local_counter_version > 0),
        fallback_policy_version INTEGER NOT NULL CHECK (fallback_policy_version > 0),
        safety_margin_tokens INTEGER NOT NULL CHECK (safety_margin_tokens >= 0),
        count_execution_pin_digest TEXT NOT NULL,
        trim_candidate_object_id TEXT NOT NULL REFERENCES content_objects(id),
        trim_candidate_digest TEXT NOT NULL,
        request_digest TEXT NOT NULL,
        request_object_id TEXT NOT NULL REFERENCES content_objects(id),
        result_source TEXT CHECK (result_source IN ('provider','local','estimate')),
        result_object_id TEXT REFERENCES content_objects(id),
        status TEXT NOT NULL CHECK (status IN (
            'prepared','running','completed','failed','retry_ready',
            'cancelled_before_start','abandoned_unknown'
        )),
        created_at INTEGER NOT NULL,
        finished_at INTEGER,
        UNIQUE(node_instance_id, count_ordinal)
    )"#,
    r#"CREATE TABLE tool_calls (
        id TEXT PRIMARY KEY NOT NULL,
        node_instance_id TEXT NOT NULL REFERENCES node_instances(id),
        originating_attempt_id TEXT NOT NULL REFERENCES node_attempts(id),
        model_call_id TEXT NOT NULL REFERENCES model_calls(id),
        provider_call_id TEXT,
        call_index INTEGER NOT NULL CHECK (call_index >= 0),
        binding_id TEXT NOT NULL,
        tool_id TEXT NOT NULL,
        tool_version TEXT NOT NULL,
        call_digest TEXT NOT NULL,
        arguments_object_id TEXT NOT NULL REFERENCES content_objects(id),
        output_object_id TEXT REFERENCES content_objects(id),
        status TEXT NOT NULL CHECK (status IN (
            'requested','validated','awaiting_approval','prepared','running','completed',
            'failed','denied','outcome_unknown','retry_ready','cancelled_before_start','abandoned_unknown'
        )),
        error_object_id TEXT REFERENCES content_objects(id),
        created_at INTEGER NOT NULL,
        finished_at INTEGER,
        UNIQUE(model_call_id, call_index)
    )"#,
    r#"CREATE TABLE tool_call_bound_read_results (
        tool_call_id TEXT PRIMARY KEY NOT NULL REFERENCES tool_calls(id),
        query_object_id TEXT NOT NULL REFERENCES content_objects(id),
        envelope_object_id TEXT NOT NULL REFERENCES content_objects(id),
        result_digest TEXT NOT NULL,
        scope_snapshot_token TEXT NOT NULL,
        truncated INTEGER NOT NULL CHECK (truncated IN (0,1)),
        created_at INTEGER NOT NULL
    )"#,
    r#"CREATE TABLE tool_call_read_set (
        tool_call_id TEXT NOT NULL REFERENCES tool_calls(id),
        memory_id TEXT NOT NULL REFERENCES memories(id),
        commit_id TEXT NOT NULL REFERENCES memory_commits(id),
        selection_ordinal INTEGER NOT NULL CHECK (selection_ordinal >= 0),
        selected_content_hash TEXT NOT NULL,
        PRIMARY KEY(tool_call_id, selection_ordinal),
        UNIQUE(tool_call_id, memory_id)
    )"#,
    r#"CREATE TABLE effects (
        id TEXT PRIMARY KEY NOT NULL,
        node_instance_id TEXT NOT NULL REFERENCES node_instances(id),
        model_call_id TEXT REFERENCES model_calls(id),
        count_call_id TEXT REFERENCES count_calls(id),
        tool_call_id TEXT REFERENCES tool_calls(id),
        effect_kind TEXT NOT NULL,
        classification TEXT NOT NULL CHECK (classification IN ('pure','idempotent','non_idempotent')),
        operation_key TEXT NOT NULL,
        idempotency_key TEXT NOT NULL UNIQUE,
        retry_policy_json TEXT NOT NULL CHECK (json_valid(retry_policy_json)),
        status TEXT NOT NULL CHECK (status IN (
            'pending','succeeded','failed','outcome_unknown','cancelled_before_start','abandoned_unknown'
        )),
        result_object_id TEXT REFERENCES content_objects(id),
        created_at INTEGER NOT NULL,
        completed_at INTEGER,
        CHECK (
            (model_call_id IS NOT NULL) +
            (count_call_id IS NOT NULL) +
            (tool_call_id IS NOT NULL) = 1
        )
    )"#,
    r#"CREATE TABLE effect_attempts (
        id TEXT PRIMARY KEY NOT NULL,
        effect_id TEXT NOT NULL REFERENCES effects(id),
        invoking_node_attempt_id TEXT NOT NULL REFERENCES node_attempts(id),
        attempt_no INTEGER NOT NULL CHECK (attempt_no > 0),
        status TEXT NOT NULL CHECK (status IN (
            'prepared','started','succeeded','failed','outcome_unknown','superseded_before_start'
        )),
        provider_request_id TEXT,
        request_object_id TEXT NOT NULL REFERENCES content_objects(id),
        result_object_id TEXT REFERENCES content_objects(id),
        error_object_id TEXT REFERENCES content_objects(id),
        started_at INTEGER,
        finished_at INTEGER,
        UNIQUE(effect_id, attempt_no),
        UNIQUE(effect_id, id)
    )"#,
    r#"CREATE TABLE effect_resolutions (
        id TEXT PRIMARY KEY NOT NULL,
        effect_id TEXT NOT NULL,
        effect_attempt_id TEXT NOT NULL UNIQUE,
        resolution_kind TEXT NOT NULL CHECK (resolution_kind IN (
            'confirm_succeeded','confirm_failed_retry_safe','abort_run',
            'run_terminal_cancel_before_start','run_terminal_abandon'
        )),
        command_idempotency_key TEXT NOT NULL,
        request_digest TEXT NOT NULL,
        decision_object_id TEXT NOT NULL REFERENCES content_objects(id),
        result_object_id TEXT REFERENCES content_objects(id),
        evidence_object_id TEXT REFERENCES content_objects(id),
        actor_kind TEXT NOT NULL,
        actor_id TEXT,
        created_at INTEGER NOT NULL,
        FOREIGN KEY(effect_id, effect_attempt_id) REFERENCES effect_attempts(effect_id, id),
        UNIQUE(effect_id, command_idempotency_key)
    )"#,
    r#"CREATE TABLE internal_sensitive_objects (
        id TEXT PRIMARY KEY NOT NULL,
        origin_effect_attempt_id TEXT NOT NULL UNIQUE REFERENCES effect_attempts(id),
        format_version INTEGER NOT NULL CHECK (format_version > 0),
        ciphertext_digest TEXT,
        byte_size INTEGER CHECK (byte_size IS NULL OR byte_size >= 0),
        purpose TEXT NOT NULL CHECK (purpose = 'provider_opaque_bundle_v1'),
        key_version INTEGER NOT NULL CHECK (key_version > 0),
        kdf_version INTEGER NOT NULL CHECK (kdf_version > 0),
        algorithm TEXT NOT NULL,
        lifecycle TEXT NOT NULL CHECK (lifecycle IN ('reserved','live','deleting','deleted')),
        lifecycle_generation INTEGER NOT NULL DEFAULT 0,
        delete_fence TEXT,
        nonce BLOB,
        ciphertext BLOB,
        storage_key TEXT,
        expires_at INTEGER,
        created_at INTEGER NOT NULL,
        deleted_at INTEGER,
        CHECK (
            (lifecycle = 'reserved' AND ciphertext_digest IS NULL AND byte_size IS NULL
                AND nonce IS NULL AND ciphertext IS NULL AND storage_key IS NULL)
            OR (lifecycle = 'live' AND ciphertext_digest IS NOT NULL AND byte_size IS NOT NULL
                AND nonce IS NOT NULL AND ((ciphertext IS NOT NULL) <> (storage_key IS NOT NULL)))
            OR lifecycle IN ('deleting','deleted')
        )
    )"#,
    "CREATE INDEX model_calls_instance_status ON model_calls(node_instance_id, status)",
    "CREATE INDEX count_calls_instance_status ON count_calls(node_instance_id, status)",
    "CREATE INDEX tool_calls_instance_status ON tool_calls(node_instance_id, status)",
    "CREATE UNIQUE INDEX effects_model_owner ON effects(model_call_id) WHERE model_call_id IS NOT NULL",
    "CREATE UNIQUE INDEX effects_count_owner ON effects(count_call_id) WHERE count_call_id IS NOT NULL",
    "CREATE UNIQUE INDEX effects_tool_owner ON effects(tool_call_id) WHERE tool_call_id IS NOT NULL",
    "CREATE INDEX effects_status_classification ON effects(status, classification)",
    "CREATE INDEX effect_attempts_invoker_status ON effect_attempts(invoking_node_attempt_id, status)",
    "CREATE INDEX effect_attempts_status_started ON effect_attempts(status, started_at)",
    "CREATE INDEX effect_resolutions_effect_created ON effect_resolutions(effect_id, created_at)",
];

const DOWN: &[&str] = &[
    "DROP TABLE internal_sensitive_objects",
    "DROP TABLE effect_resolutions",
    "DROP TABLE effect_attempts",
    "DROP TABLE effects",
    "DROP TABLE tool_call_read_set",
    "DROP TABLE tool_call_bound_read_results",
    "DROP TABLE tool_calls",
    "DROP TABLE count_calls",
    "DROP TABLE model_calls",
    "DROP TABLE llm_loop_checkpoints",
];
