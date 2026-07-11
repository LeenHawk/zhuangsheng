# SQLite-first 持久化逻辑 Schema

## 定位与约定

本文给出可由 SeaORM 实现的逻辑 schema。数据库模型属于 storage adapter，core runtime 只依赖 repository/domain types。

阶段一使用 SQLite + WAL；PostgreSQL 保持相同语义但延后部署。约定：ID 使用 ULID/UUID 的 `TEXT`，时间使用 UTC epoch milliseconds `INTEGER`，JSON 在 SQLite 为 canonical JSON `TEXT`、PostgreSQL 为 `JSONB`，hash 使用带算法前缀的 `TEXT`。每个 SQLite connection 都启用 `PRAGMA foreign_keys=ON`。

状态字段由应用枚举和数据库 `CHECK` 双重校验，JSON 字段在 SQLite 使用 `json_valid` 检查。不可变记录不做原地业务更新。复合唯一键不依赖 nullable 列；branchless lineage 使用保留值 `"global"`。

## Graph 与配置表组

### 阶段一

- `graphs(id PK, name, created_at, updated_at)`。
- `graph_drafts(graph_id PK/FK graphs, document_json, revision_token, updated_at)`；draft 可不完整，`revision_token` 用于乐观锁。
- `graph_revisions(id PK, graph_id FK, revision_no, operation_taxonomy_version, adapter_decoder_version, definition_json, schema_bundle_object_id FK, content_hash, created_at)`；`UNIQUE(graph_id, revision_no)`、`UNIQUE(graph_id, content_hash)`，applied revision 不可变。
- `context_presets(id PK, name, head_version_id FK NULL, created_at, updated_at)`。
- `context_preset_versions(id PK, preset_id FK, version_no, spec_json, content_hash, created_at)`；`UNIQUE(preset_id, version_no)`，NodeInstance 记录实际 version ID。
- `llm_channels(id PK, name, head_revision_id FK NULL, created_at, updated_at)`。
- `llm_channel_revisions(id PK, channel_id FK, revision_no, operation_taxonomy_version, adapter_decoder_version, base_url, credential_kind, api_key_ref NULL, operation_keys_json, model_lists_json, capabilities_json, created_at)`；`UNIQUE(channel_id, revision_no)`，`CHECK((credential_kind='secret' AND api_key_ref IS NOT NULL) OR (credential_kind='none' AND api_key_ref IS NULL))`；不可变且只保存 typed SecretRef，不保存明文。
- `tool_registry_entries(tool_id, tool_version, descriptor_json, schema_bundle_object_id FK, descriptor_digest, implementation_digest, executor_key, enabled, created_at, updated_at, PRIMARY KEY(tool_id, tool_version))`；运行时 schema/implementation snapshot 与 digest 固定进 NodeInstance execution snapshot。
- `policy_revocations(id PK, revision_no UNIQUE, target_kind, target_id, deny_rules_object_id FK, created_at)`；append-only 且对旧 snapshot 累积，只表达收窄/撤销；旧 NodeInstance 在 dispatch/approval/commit 前读取截至当前 revision 的 overlay。
- `application_command_receipts(scope, idempotency_key, request_digest, command_kind, resource_kind NULL, resource_id NULL, status, result_object_id FK NULL, result_expires_at NULL, created_at, completed_at NULL, expired_at NULL, PRIMARY KEY(scope, idempotency_key))`；保存所有非 run 领域 mutation 的 immutable 幂等结果/tombstone。

顶层资源也必须有可调用的 bootstrap transaction。CreateGraph 同事务插入 `graphs`、canonical 空 draft `{ graphId, name, nodes: [], edges: [], outputContract: [] }`、初始 revision token 和 application receipt；CreateChannel/CreateContextPreset 分别插入 head 为空的资源 row 与 receipt，只有发布首个 immutable revision/version 后才能被 Graph Apply 或 runtime 引用。所有资源 ID 和 result object 在事务前预分配；同 scope/key 重放返回原 ID，不得因响应丢失创建第二个逻辑资源。

Graph node、edge、port、Router rule、MemoryBinding、model ref 和 canonical `JsonSchemaSpec` 阶段一保存在不可变 `definition_json`，避免数据库模型污染 GraphDefinition。`schema_bundle_object_id` 保存 `JsonSchemaCompilation[]` 与 compiled payload refs；每项同时含不覆盖 effective limits 的 `canonicalDocumentHash` 和覆盖完整 spec/limits 的 `schemaHash`。Tool registry 使用同一结构，owner repository 还为 bundle 内每个 canonical/compiled object 同事务写 `content_object_refs`。加载时重算两种 hash 与 compiled payload hash，owner hash/digest 覆盖 compilation tuple；未知 schema/profile/compiler/payload format 或损坏 ref 均拒绝。若之后需要跨图查询，再增加派生索引表。

Operation version 列都是非空正整数。写入前按 `07-llm-channels-counting.md` 的 support matrix 校验；storage reader 不能把缺失/未知值补成当前默认。Graph/channel 版本不匹配时不能创建 LLM execution snapshot。

Receipt `scope` 是 canonical workspace/principal + command route/resource 作用域，不是调用方可伪造的自由字符串。每个 draft/config、Conversation/selection/projection resolution、Memory proposal decision/apply、artifact、不含 secret bytes 的 lock/纯 Secret metadata 等 mutation 都在其业务事务中同时插入/finish receipt；同 scope+key+digest 在 result retention 内返回已存安全 result，不同 digest 始终返回 `idempotency_conflict`。Secret-bearing create/update/unlock 明确排除在此表外，使用 `secret_command_receipts`。Current projection 可被后续命令覆盖，receipt 不被覆盖，因此旧 selection 重放不会把 UI 切回旧 branch。

Phase-one result retention 至少 30 天且不早于目标 resource/audit lifecycle；到期可删除 result object 并把 row 置 `expired`，但 `(scope,key,request_digest)` tombstone 在 workspace 生命期内不删除/不复用。之后同 digest 返回 `idempotency_key_expired`，绝不重施 mutation；不同 digest 仍是 conflict。Secret command result 只能是非敏感 metadata；receipt 不保存 request body/secret。

同一 tool version 的 descriptor/digests 不可原地替换；实现变化必须发布新版本。`enabled` 只影响新 snapshot discovery，紧急阻止已有 instance 必须发布 policy revocation。Run manifest 记录解析策略/显式 pins，实际 registry/config snapshot 记录在 NodeInstance 中。

### 延后

workspace config、graph revision 差量存储、多人 draft 协作、registry 发布审批和动态插件安装。

## Runtime 表组

### 阶段一

- `graph_runs(id PK, request_idempotency_scope, request_idempotency_key, request_digest, graph_revision_id FK, graph_content_hash, execution_manifest_object_id FK, context_id FK, branch_id, input_commit_id FK, output_commit_id FK NULL, status, control_epoch, drain_epoch NULL, limits_object_id FK, run_input_object_id FK, run_outputs_object_id FK NULL, terminal_error_object_id FK NULL, started_at NULL, deadline_at, created_at, updated_at, finished_at NULL)`；`UNIQUE(request_idempotency_scope, request_idempotency_key)`，`FOREIGN KEY(context_id, branch_id)` 指向同一 context 的 branch。
- `run_execution_counters(run_id PK/FK, next_enqueue_seq, next_output_seq, total_activations, total_attempts, total_queue_values, pending_queue_values, open_waits, coordinator_buffered_values)`；所有计数在相同 run 写事务内分配/校验 hard limits。
- `node_scheduling_cursors(run_id FK, node_id, next_activation_seq, PRIMARY KEY(run_id, node_id))`；scheduler 竞争时锁定/CAS 此行。
- `node_instances(id PK, run_id FK, node_id, activation_seq, status, graph_revision_id FK, execution_snapshot_object_id FK NULL, operation_taxonomy_version NULL, adapter_decoder_version NULL, preset_version_id FK NULL, inputs_object_id FK, final_outputs_object_id FK NULL, created_at, updated_at)`；`UNIQUE(run_id, node_id, activation_seq)`，两个 operation version 必须同时为空或同时非空；LLM instance 必须非空且与 snapshot payload完全一致。
- `node_attempts(id PK, node_instance_id FK, attempt_no, retry_ordinal, invocation_kind, status, run_control_epoch, lease_fence, worker_id NULL, lease_until NULL, deadline_at NULL, idempotency_key, result_idempotency_key NULL, executor_object_id FK, continuation_object_id FK NULL, error_object_id FK NULL, started_at NULL, finished_at NULL)`；`UNIQUE(node_instance_id, attempt_no)`、`UNIQUE(idempotency_key)`、`UNIQUE(result_idempotency_key)`；lease CAS 使用 `(attempt id, lease_fence, worker_id)`，fence 只需在单 attempt 内递增。
- `node_read_set(node_attempt_id FK, aggregate_kind, aggregate_id, lineage_key, commit_id FK, binding_id, selection_ordinal NULL, selected_content_hash NULL, consistency, PRIMARY KEY(node_attempt_id, aggregate_kind, aggregate_id, lineage_key, binding_id))`；long-term selected rows 额外 `UNIQUE(node_attempt_id, binding_id, selection_ordinal) WHERE selection_ordinal IS NOT NULL`。
- `node_bound_read_results(node_attempt_id FK, binding_id, envelope_object_id FK, result_digest, scope_snapshot_token NULL, truncated, PRIMARY KEY(node_attempt_id, binding_id))`；保存 canonical `BoundReadResult`，零结果也必须有 row。
- `node_output_commits(node_instance_id FK, commit_id FK, output_order, PRIMARY KEY(node_instance_id, output_order), UNIQUE(node_instance_id, commit_id))`。
- `edge_queue_values(id PK, run_id FK, edge_id, enqueue_seq, producer_instance_id FK, producer_emission_index, value_object_id FK, consumed_by_instance_id FK NULL, consumed_at NULL, created_at)`；`UNIQUE(run_id, enqueue_seq)`、`UNIQUE(run_id, edge_id, producer_instance_id, producer_emission_index)`。
- `run_output_values(id PK, run_id FK, output_key, collection_mode, output_seq, node_instance_id FK, value_object_id FK, created_at)`；`UNIQUE(run_id, output_key, output_seq)`、`UNIQUE(run_id, output_key) WHERE collection_mode='single'`，`append` 按 output_seq 稳定排序。
- `node_waits(id PK, run_id FK, node_instance_id FK, node_attempt_id FK, kind, correlation_key NULL, request_object_id FK, continuation_object_id FK, response_schema_object_id FK NULL, response_schema_compilation_object_id FK NULL, deadline_at NULL, on_timeout, status, response_object_id FK NULL, accepted_delivery_id NULL, created_at, resolved_at NULL)`；`UNIQUE(node_instance_id) WHERE status='open'`；两个 response schema ref 必须同时为空或同时非空。
- `wait_blockers(wait_id FK, blocker_kind, blocker_id, blocker_order, status, decision_object_id FK NULL, PRIMARY KEY(wait_id, blocker_kind, blocker_id), UNIQUE(wait_id, blocker_order))`；kind 只允许 `tool_call | memory_proposal | effect`，status 只允许 `open | satisfied | rejected | aborted`；只有 open 的 decision为空，terminal必须有 decision。全部 terminal且无 aborted 后才 resolve wait。
- `wait_deliveries(wait_id FK, delivery_id, payload_digest, result_object_id FK, created_at, PRIMARY KEY(wait_id, delivery_id))`；重复 delivery 返回原 result，不同 delivery 不能覆盖已 resolved wait。
- `router_visits(run_id FK, router_node_id, visits, started_at, PRIMARY KEY(run_id, router_node_id))`。
- `router_activation_controls(node_instance_id PK/FK, visit_no, first_visited_at, decision_at, elapsed_ms, limit_reasons_json, created_at)`；保存 `14-router-node.md` 的不可变 per-activation control snapshot。
- `scheduler_wakeups(id PK, run_id FK, node_id NULL, kind, caused_by_seq, dedupe_key UNIQUE, status, available_at, claimed_by NULL, lease_until NULL, created_at)`；notification 只负责唤醒，pending row 才是 durable work。
- `runtime_timers(id PK, run_id FK, node_instance_id FK NULL, node_attempt_id FK NULL, kind, due_at, dedupe_key UNIQUE, status, payload_object_id FK NULL, created_at, fired_at NULL)`；wait、retry、attempt/run deadline 和 Aggregator timeout 共用 durable timer。
- `run_commands(id PK, run_id FK, command_kind, idempotency_key, request_digest, expected_control_epoch NULL, payload_object_id FK NULL, status, result_object_id FK NULL, created_at, applied_at NULL)`；`UNIQUE(run_id, idempotency_key)`；同 key 不同 digest 返回 conflict。
- `coordination_index_cursors(run_id FK, node_id, input_port, last_enqueue_seq, PRIMARY KEY(run_id, node_id, input_port))`。
- `coordination_buffer_items(run_id FK, node_id, input_port, edge_queue_value_id FK UNIQUE, enqueue_seq, key_json NULL, status, PRIMARY KEY(run_id, node_id, input_port, edge_queue_value_id))`。
- `aggregation_windows(id PK, run_id FK, node_id, node_instance_id FK UNIQUE, activation_seq, opened_at, deadline_at, status, close_reason NULL, created_at, updated_at)`；同 `(runId,nodeId)` 最多一个 open window。
- `aggregation_window_items(window_id FK, edge_queue_value_id FK UNIQUE, item_order, PRIMARY KEY(window_id, item_order))`。

关键索引：`graph_runs(status, updated_at)`、`node_instances(run_id, status)`、`UNIQUE node_instances(run_id, node_id) WHERE status IN ('ready','running','waiting')`、`node_attempts(status, lease_until)`、`UNIQUE node_attempts(node_instance_id) WHERE status IN ('queued','leased','running')`、`edge_queue_values(run_id, edge_id, enqueue_seq) WHERE consumed_at IS NULL`、`UNIQUE node_waits(kind, correlation_key) WHERE status='open' AND correlation_key IS NOT NULL`、`node_waits(status, deadline_at)`、`scheduler_wakeups(status, available_at)`、`runtime_timers(status, due_at)`、`coordination_buffer_items(run_id,node_id,key_json,status,input_port,enqueue_seq)`、`UNIQUE aggregation_windows(run_id,node_id) WHERE status='open'`。

`context_id/branch_id` 对普通工作流仍必填：runtime 可创建临时 context，避免 branch 重新归属于 run。GraphRun 内只有一个 execution namespace；edge、router、activation 和 wakeup 只以 `runId` 隔离，branch 不参与其 key。所有 runtime ValueRef 都指向 `content_objects`，小值由该表自行内联。

Open `aggregation_windows` 是 coordinator internal blocker：opening built-in attempt 已经 completed，关联 NodeInstance 为 waiting。它不创建 `node_waits`、不增加 `run_execution_counters.open_waits`，也不出现在外部 wait API；count/timeout 关闭窗口时创建 coordinator resume attempt，并在同一事务终结 window、timer 和 NodeInstance。

### 延后

分布式 ready queue、worker heartbeat service、priority/fairness、per-node 并发槽、quorum、keyed/sliding/session window 和 watermark。

## Context 与 Version 表组

### 阶段一

- `contexts(id PK, kind, status, created_at, updated_at)`。
- `context_branches(id PK, context_id FK, parent_branch_id NULL, fork_commit_id FK, head_commit_id FK, creation_operation_id, status, name NULL, retention_until NULL, pinned, audit_hold, created_at, updated_at)`；`UNIQUE(context_id, id)`、`UNIQUE(context_id, creation_operation_id)`，parent 使用同 context 的复合 FK，head 通过 CAS 更新。
- `version_commits(id PK, aggregate_kind, aggregate_id, lineage_key, sequence_no, operation_id, patch_object_id FK NULL, initial_snapshot_object_id FK NULL, merge_resolution_object_id FK NULL, schema_version, policy_version, author_kind, author_id NULL, origin_run_id FK NULL, origin_node_instance_id FK NULL, created_at)`；`UNIQUE(aggregate_kind, aggregate_id, lineage_key, sequence_no)`、`UNIQUE(aggregate_kind, aggregate_id, lineage_key, operation_id)`，patch/initial snapshot 至少有一个。
- `commit_parents(commit_id FK, parent_commit_id FK, parent_order, PRIMARY KEY(commit_id, parent_order), UNIQUE(commit_id, parent_commit_id))`；root 0 个、普通 1 个、merge 2 个 parent，由 domain 校验。
- `materialized_projections(aggregate_kind, aggregate_id, lineage_key, head_commit_id FK, projection_json NULL, projection_object_id FK NULL, schema_version, updated_at, PRIMARY KEY(aggregate_kind, aggregate_id, lineage_key))`。
- `memory_records(id PK, scope, status, head_commit_id FK NULL, current_content_object_id FK NULL, created_at, updated_at)`；status 为 `proposed | active | obsolete | deleted | discarded`，LongTermMemory 的 branchless lineage 固定为 `global`；deleted/discarded 必须没有 current content，discarded 还必须没有 head。
- `memory_scope_versions(scope PK, revision_no, updated_at)`；任何会改变该 scope 可见/可搜索 record 集合或内容的 memory apply 事务都原子递增，作为 query phantom CAS token。
- `memory_search_documents(memory_id PK/FK, head_commit_id FK, content_hash, searchable_text, tags_json, status, updated_at)` + SQLite FTS5 virtual index；它是可重建 projection，查询 score tie 按 memory_id。
- `memory_change_proposals(id PK, memory_id FK, expected_head_commit_id FK NULL, change_kind, change_object_id FK, reason, requested_by_kind, requested_by_id NULL, idempotency_key, schema_version, policy_version, status, retention_until NULL, origin_run_id FK NULL, origin_node_instance_id FK NULL, created_at, resolved_at NULL)`；`UNIQUE(idempotency_key)`；change object 是 `16-domain-consistency.md` 的 versioned discriminated payload。
- `memory_proposal_evidence(proposal_id FK, evidence_kind, evidence_id, PRIMARY KEY(proposal_id, evidence_kind, evidence_id))`。
- `memory_proposal_commits(proposal_id PK/FK, commit_id FK UNIQUE, applied_at)`；apply 事务同时写入，domain projection 由此得到 proposal.appliedCommitId / commit.sourceProposalId。
- `context_merge_operations(id PK, context_id FK, source_branch_id FK, target_branch_id FK, source_head_id FK, target_head_id FK, base_commit_id FK NULL, source_disposition, idempotency_key, request_digest, status, result_commit_id FK NULL, result_object_id FK NULL, created_at, completed_at NULL)`；`UNIQUE(context_id, idempotency_key)`，同 key 不同 digest 返回 conflict。
- `merge_conflicts(id PK, context_id FK, merge_operation_id FK, source_branch_id FK, target_branch_id FK, base_commit_id FK, source_head_id FK, target_head_id FK, path, values_object_id FK, status, resolution_object_id FK NULL, created_at, resolved_at NULL)`；`UNIQUE(context_id, merge_operation_id, path)`。

索引：`context_branches(context_id, status)`、`version_commits(aggregate_kind, aggregate_id, lineage_key, sequence_no DESC)`、`memory_records(scope, status)`、`memory_change_proposals(status, created_at)`、`merge_conflicts(target_branch_id, status)`。

`version_commits` 与 parent rows append-only；branch head 和 projection 是可重建缓存。`lineage_key` 对 WorkingContext 是 branch ID，对 LongTermMemory 是 `global`。新建长期记忆时先在 proposal 事务中保留 `memory_records.id` 并置为 proposed，apply create 后写 initial snapshot/root commit、置 active 并推进 head。Replace/obsolete/tombstone 基于 expected head 生成确定 patch；tombstone 清空 current content 与 search projection。

Merge 命令先按 request digest 幂等创建 operation；冲突结果也是该 operation 的持久结果。解决命令在同一事务中验证 conflict/head，写 final patch/merge commit，CAS target，更新 conflict 和 source status，并终结 operation；任一校验或 CAS 失败都不可部分可见。

Root 使用 initial snapshot，普通/merge commit 都必须保存最终 patch；`merge_resolution_object_id` 只保存 provenance，不能代替可 replay 的 merge patch。

### 延后

长期记忆 branch、vector index/embedding 版本、跨 context merge、任意 reducer 和在线 schema transform。

## Conversation 表组

Conversation/Turn 是 adapter domain；Message 与 commit 是历史权威，Turn 只是查询索引。

### 阶段一

- `conversations(id PK, context_id FK UNIQUE, active_branch_id FK, active_head_commit_id FK, default_graph_revision_id FK NULL, default_reply_output_key NULL, run_profile_revision_no NULL, title NULL, created_at, updated_at, FOREIGN KEY(context_id, active_branch_id) REFERENCES context_branches(context_id, id))`；active head 必须等于该 branch 当前 head；default run三字段必须同时为空或同时非空，revision从1递增。
- `conversation_messages(id PK, conversation_id FK, turn_id FK, branch_id FK, commit_id FK UNIQUE, parent_message_id FK NULL, role, source_kind, content_object_id FK, origin_run_id FK NULL, created_at)`；role/source 只允许 `user+user_input | assistant+run_output | assistant+saved_partial`，user 的 origin run 为空、assistant 的 origin run/parent message 非空；与 `conversation_turns` 的循环 FK 使用同事务预分配 ID 和 deferred constraint。
- `conversation_turns(id PK, conversation_id FK, user_message_id FK UNIQUE, user_commit_id FK UNIQUE, idempotency_key UNIQUE, created_at)`。
- `turn_candidates(turn_id FK, run_id FK UNIQUE, branch_id FK, base_commit_id FK, reply_output_key, creation_idempotency_key UNIQUE, assistant_message_id FK NULL, candidate_commit_id FK NULL, projection_error_object_id FK NULL, status, created_at, PRIMARY KEY(turn_id, run_id))`；projector CAS 失败时 status=`projection_conflicted` 并保存脱敏 error ref。
- `candidate_projection_jobs(run_id PK/FK turn_candidates, terminal_event_seq, terminal_status, status, available_at, claimed_by NULL, lease_until NULL, attempt_count, last_error_object_id FK NULL, created_at, completed_at NULL)`；status 为 `pending | claimed | done | conflicted | failed`，一个 candidate run 最多一个 durable job；failed/conflicted/done 都是 projector terminal，只有 operator projection-resolution 可改 conflicted candidate。
- `conversation_selections(turn_id PK/FK, selected_run_id, selection_idempotency_key UNIQUE, selected_at, FOREIGN KEY(turn_id, selected_run_id) REFERENCES turn_candidates(turn_id, run_id))`。

索引：`conversation_messages(conversation_id, branch_id, created_at)`、`turn_candidates(run_id)`、`candidate_projection_jobs(status, available_at)`。regenerate 复用同一 user-message commit；swipe 事务性更新 selection 与 conversation active branch/head，不复制聊天历史。Notifier 只唤醒 projector；启动/周期 reconciliation 使用 `turn_candidates JOIN graph_runs` 为所有 terminal run 补建 job，因此不依赖可丢的 commit-after callback。

CreateConversation 预分配 conversation/context/root branch/root commit 与 root snapshot object ID，在一个事务中插入 `contexts(kind='conversation')`、`ConversationContextV1 { schemaVersion: 1, messages: [] }` initial snapshot/root VersionCommit、无 parent 的 root branch、对应 materialized projection、指向该 branch/head 的 Conversation，以及 application receipt/result。可选 default run在事务前验证并以profile revision 1写入三列。Root operation/branch creation operation 使用 `13-conversation-turn-run.md` 的确定字符串；任一冲突或写入失败全部回滚，新安装不需要先调用内部 context API。

每次消息 append 都把 `conversation_messages` row、`/messages` append patch object、VersionCommit、branch head CAS、materialized projection和需要推进时的 Conversation active branch/head放在同一事务；candidate 非当前 selection 时只推进 candidate branch，不能覆盖 active pointer。Append-only validator 拒绝对已提交 `/messages` element 的 replace/remove，storage repository 不能绕过该领域校验。

### 延后

多人会话成员、消息编辑协作、全文搜索和服务端 retention policy。

## Model、Tool 与 Effect 表组

### 阶段一

- `llm_loop_checkpoints(node_instance_id PK/FK, schema_version, last_updated_by_attempt_id FK, checkpoint_object_id FK, checkpoint_digest, effect_watermark NULL, updated_at)`；每个 model/count terminal、count/tool transition、wait 和 batch 收敛后更新，last updater 必须属于该 instance；未知 checkpoint version fail closed，不能用当前 struct 猜读。
- `model_calls(id PK, node_instance_id FK, originating_attempt_id FK, call_no, channel_id FK, channel_revision_id FK, model_id, operation_key_json, operation_taxonomy_version, adapter_decoder_version, request_object_id FK, response_object_id FK NULL, provider_request_id NULL, status, usage_json NULL, started_at, finished_at NULL)`；`UNIQUE(node_instance_id, call_no)`；originating attempt 必须属于该 instance，两个 version 必须与 NodeInstance execution snapshot 相同，status 允许 `prepared | running | completed | failed | outcome_unknown | retry_ready | cancelled_before_start | abandoned_unknown`。
- `count_calls(id PK, node_instance_id FK, originating_attempt_id FK, count_ordinal, channel_id FK, channel_revision_id FK, model_id, operation_key_json, operation_taxonomy_version, adapter_decoder_version, local_counter_id, local_counter_version, fallback_policy_version, safety_margin_tokens, count_execution_pin_digest, trim_candidate_object_id FK, trim_candidate_digest, request_digest, request_object_id FK, result_source NULL, result_object_id FK NULL, status, created_at, finished_at NULL)`；`UNIQUE(node_instance_id, count_ordinal)`，operation/local-counter/fallback versions、safety margin、pin/candidate/request digest 必须与 NodeInstance execution snapshot/checkpoint 一致，status 允许 `prepared | running | completed | failed | retry_ready | cancelled_before_start | abandoned_unknown`。
- `tool_calls(id PK, node_instance_id FK, originating_attempt_id FK, model_call_id FK, provider_call_id NULL, call_index, binding_id, tool_id, tool_version, call_digest, arguments_object_id FK, output_object_id FK NULL, status, error_object_id FK NULL, created_at, finished_at NULL)`；`UNIQUE(model_call_id, call_index)`；model call/originating attempt 必须属于同一 instance，status 允许 `requested | validated | awaiting_approval | prepared | running | completed | failed | denied | outcome_unknown | retry_ready | cancelled_before_start | abandoned_unknown`。
- `tool_call_bound_read_results(tool_call_id PK/FK, query_object_id FK, envelope_object_id FK, result_digest, scope_snapshot_token, truncated, created_at)`；只用于内建 dynamic `search_memory`，零结果也必须有 row。
- `tool_call_read_set(tool_call_id FK, memory_id FK, commit_id FK, selection_ordinal, selected_content_hash, PRIMARY KEY(tool_call_id, selection_ordinal), UNIQUE(tool_call_id, memory_id))`；保存 call-level 有序 selected records。
- `effects(id PK, node_instance_id FK, model_call_id FK NULL, count_call_id FK NULL, tool_call_id FK NULL, effect_kind, classification, operation_key, idempotency_key, retry_policy_json, status, result_object_id FK NULL, created_at, completed_at NULL, CHECK (exactly_one(model_call_id, count_call_id, tool_call_id)))`；`UNIQUE(idempotency_key)`，owner 必须属于该 instance，并分别建立三个非空 owner partial unique index；status 允许 `pending | succeeded | failed | outcome_unknown | cancelled_before_start | abandoned_unknown`。
- `effect_attempts(id PK, effect_id FK, invoking_node_attempt_id FK, attempt_no, status, provider_request_id NULL, request_object_id FK, result_object_id FK NULL, error_object_id FK NULL, started_at, finished_at NULL)`；`UNIQUE(effect_id, attempt_no)`、`UNIQUE(effect_id, id)`；invoking attempt 必须属于 effect 的 NodeInstance。
- `effect_resolutions(id PK, effect_id, effect_attempt_id UNIQUE, resolution_kind, command_idempotency_key, request_digest, decision_object_id FK, result_object_id FK NULL, evidence_object_id FK NULL, actor_kind, actor_id NULL, created_at, FOREIGN KEY(effect_id, effect_attempt_id) REFERENCES effect_attempts(effect_id, id))`；resolution kind允许人工 `confirm_succeeded | confirm_failed_retry_safe | abort_run` 或 system-only `run_terminal_cancel_before_start | run_terminal_abandon`，`UNIQUE(effect_id, command_idempotency_key)`；同 key不同 digest冲突，一条 effect attempt最多一个结论。System row只写给尚无 resolution、且本次 supersede/abandon 的真实 attempt；retry_ready 已有人工 resolution或 awaiting-approval 尚无 effect时只写 terminal journal/blocker decision，不伪造第二条或空 attempt resolution。

`classification` 为 `pure | idempotent | non_idempotent`；attempt 状态为 `prepared | started | succeeded | failed | outcome_unknown | superseded_before_start`。Effect owner是 effects row上唯一权威的 model/count/tool tagged association；repository还校验 owner/effect logical rows 属于同一 NodeInstance，而每次执行的 fence 取 effect_attempts.invoking_node_attempt_id。Prepared row 只能与同 fence CAS 为 started 或 superseded；lease recovery 将后者与 owner/checkpoint retry_ready、reconcile wakeup 同事务写入，新 NodeAttempt 创建新 effect attempt。EffectAttempt 的 `outcome_unknown` 不回写为人工猜测，resolution row才是后续结论。人工 resolution 只用于需协调的 model/tool owner；pure count unknown 按原 logical effect 自动重试或 fallback local count，不进人工 wait。tool output 的 artifact/state/memory parts必须通过各自事务落表，不能只藏在 `output_json`。

内建 `search_memory` 的同一 model batch 按 callIndex 在一个 SQLite read snapshot 中解析，并在任一结果返回模型前原子写入上述两表/tool output/checkpoint。已有 result 的 toolCallId 永不重搜；新的后续 tool call 可持久新 scope token。

Tool call 的幂等身份是 `(model_call_id, call_index)` 对应的稳定 local toolCallId；`call_digest` 只校验 arguments/material/grant/policy 内容并绑定 approval，不是唯一身份。同一 model response 中两个参数完全相同的 call 仍是两次合法调用，尤其不得用 digest 对 non-idempotent call 偶然去重。

索引：`model_calls(node_instance_id, status)`、`count_calls(node_instance_id, status)`、`tool_calls(node_instance_id, status)`、`effect_attempts(invoking_node_attempt_id, status)`、三个 non-null effect owner 各自 UNIQUE partial index、`effects(status, classification)`、`effect_attempts(status, started_at)`、`effect_resolutions(effect_id, created_at)`。

### 延后

hosted tool 细粒度步骤、分布式 rate limit、tool secret permission、provider callback inbox/outbox。

## Artifact 与 Object 表组

### 阶段一

- `content_objects(id PK, content_hash UNIQUE, byte_size, storage_kind, lifecycle, lifecycle_generation, delete_fence NULL, inline_bytes BLOB NULL, storage_key NULL, created_at, deleted_at NULL)`；live 时 inline bytes/storage key 恰有一个，hash 基于原始 bytes，JSON 内容先 canonicalize。
- `internal_sensitive_objects(id PK, origin_effect_attempt_id FK UNIQUE, format_version, ciphertext_digest NULL, byte_size NULL, purpose, key_version, kdf_version, algorithm, lifecycle, lifecycle_generation, delete_fence NULL, nonce BLOB NULL, ciphertext BLOB NULL, storage_key NULL, expires_at NULL, created_at, deleted_at NULL)`；需要 opaque continuation 的每个 provider EffectAttempt 至多一个 reserved/live bundle，purpose=`provider_opaque_bundle_v1`、format/kdf/key version 阶段一均为 1；ciphertext/storage key 在 live 时恰有一个，store/effect attempt/object id/purpose/versions/algorithm进入 AAD/HKDF context，不保存 plaintext hash，不参与普通 content dedup 或 Artifact 枚举。
- `artifact_staging(id PK, context_id FK NULL, node_attempt_id FK NULL, tool_call_id FK NULL, temp_storage_key NULL, expected_media_type NULL, validated_media_type NULL, byte_size NULL, content_hash NULL, metadata_draft_object_id FK, metadata_draft_digest, validated_content_object_id FK NULL, status, lifecycle_generation, delete_fence NULL, lease_until, expires_at, quarantined_at NULL, commit_request_digest NULL, committed_artifact_id FK UNIQUE NULL, commit_result_object_id FK NULL, committed_at NULL, deleted_at NULL, created_at, updated_at)`；canonical metadata draft/object/digest 在 row 创建时非空且不可变，commit 不接受替换；expected media type 只是 caller hint。Validated/committed 必须有 scanner/policy 固定的 canonical validated media type，ArtifactRef/artifact row只能复制后者。Staging ID 不可作为 ArtifactRef，status 只允许 `uploading | staged | validated | quarantined | deleting | deleted | committed`；delete fence 只在 deleting/deleted 非空，quarantined_at 自进入 quarantine 后不可改，committed/deleted terminal。
- `artifacts(id PK, context_id FK NULL, source_staging_id FK UNIQUE, content_object_id FK, metadata_head_commit_id FK, media_type, name NULL, classification, retention_kind, retention_until NULL, status, origin_run_id FK NULL, origin_node_instance_id FK NULL, origin_tool_call_id FK NULL, created_at, updated_at)`；`content_object_id/media_type` 创建后不可更改，替换 bytes 创建新 artifact row/ID；同 bytes 的不同 artifact 可以有不同、经 policy 验证的 media type。
- `content_object_refs(object_id FK, owner_kind, owner_id, role, created_at, PRIMARY KEY(object_id, owner_kind, owner_id, role))`；作为 GC 反向索引，由 owner repository 在同事务锁定 object 并确认 `lifecycle=live` 后同步维护，再由一致性任务复核。

阶段一按整对象 hash 去重；文件写入使用 temp + fsync + atomic rename，再提交 artifact metadata/owner ref。GC 只能对无 refs、无 active staging lease、未被隐式 root 引用且超过宽限期的 live 对象执行 fenced `live -> deleting -> deleted`。Owner 遇到 deleting/deleted 不能插入 ref；物理删除失败由 delete fence repair，不得把行直接删掉后猜测文件状态。Internal-sensitive 与 staging 使用同等 lifecycle/fence，但各自的 owner/lease 校验不进入普通 ref index。

Staging 状态机是 `uploading -> staged -> validated -> committed`，且任一 uploading/staged/validated 可因 cancel/expiry/failure 进入 `quarantined -> deleting -> deleted`。Uploading 持 writer lease/temp key；staged 已 fsync 并固定 hash/size；scanner 通过后先发布/lock live content object，再在 `staged -> validated` 事务写 `validated_content_object_id + validated_media_type` 和 staging owner ref。每次转换以 `(id,status,lifecycle_generation)` CAS 并把 generation 加一。Commit/cancel/GC 互斥，commit 只接受 validated。Quarantined 以固定 `quarantined_at` 计算宽限，超宽限且无 lease才在同事务移除 staging ref、写唯一 delete fence并进 deleting；repair 只复用相同 generation/fence。Content bytes 交由普通 object lifecycle GC。Committed 清空 temp key/移除 staging ref，但 row 作为 source FK 和 receipt 保留。

`artifacts` 是 artifact metadata 的当前 projection；只有重命名、classification 和 retention/status 变化通过 StatePatch/commit 留下历史，content binding 不是可 patch 字段。

Staging commit 事务以 staging ID 为作用域内幂等身份：CAS validated → committed 时同时写 artifact/source 反向关联，以完整 ArtifactMetadata（含 immutable content binding）创建 `artifact_metadata/global` root VersionCommit + initial snapshot/materialized projection，写 `metadata_head_commit_id`，再写 request digest 和含 ArtifactRef/metadata head 的 result object。响应丢失后同 digest 重投返回原 result，不同 digest 返回 `idempotency_conflict`；不得靠 content hash 在多个 artifact metadata row 中猜测原结果。Staging receipt 至少保留到 API 幂等窗口和所有 owner audit root 过期。

### 延后

chunk manifest、远程 S3、tenant-scoped encryption/dedup、冷热分层和增量上传。

## Event、Checkpoint 与 Secret 表组

### 阶段一

- `run_event_counters(run_id PK/FK, next_seq)`；在写事务内分配 run-local durable seq。
- `run_events(id PK, run_id FK, seq, context_branch_id FK NULL, node_instance_id FK NULL, attempt_id FK NULL, causation_event_id FK NULL, correlation_id NULL, event_type, schema_version, importance, payload_json NULL, payload_object_id FK NULL, created_at)`；`UNIQUE(run_id, seq)`；context branch 仅为冗余 trace binding，不参与排序/隔离。
- `domain_event_counters(aggregate_kind, aggregate_id, lineage_key, next_seq, PRIMARY KEY(aggregate_kind, aggregate_id, lineage_key))`。
- `domain_events(id PK, aggregate_kind, aggregate_id, lineage_key, seq, event_type, schema_version, payload_json NULL, payload_object_id FK NULL, created_at, UNIQUE(aggregate_kind, aggregate_id, lineage_key, seq))`；承载没有 runId 的 branch/memory audit 与 outbox，不替代 version log。
- `runtime_checkpoints(id PK, run_id FK, context_branch_id FK, through_seq, graph_revision_id FK, head_commit_id FK, snapshot_object_id FK, effect_watermark NULL, schema_version, checksum, created_at)`；`UNIQUE(run_id, through_seq)`；context branch 是与 run binding 校验的一致性副本，不是 checkpoint namespace。
- `version_snapshots(commit_id PK/FK, snapshot_object_id FK, schema_version, checksum, retention_until NULL, pinned, created_at)`；可在 commit 后增加，只加速版本 replay，不回写 commit，也不恢复 scheduler；到期且未 pin 不再作为独立 GC root。
- `secret_store_headers(singleton PK CHECK singleton=1, format_version, store_id UNIQUE, kdf_algorithm, kdf_version, kdf_salt BLOB, kdf_params_json, wrap_algorithm, wrap_nonce BLOB, wrapped_data_key BLOB, active_key_version, created_at, updated_at)`。
- `secrets(id PK, name NULL, kind, key_version, algorithm, nonce BLOB, ciphertext BLOB, created_at, updated_at)`；`UNIQUE(key_version, nonce)`，record AAD 绑定 format/store/id/kind/key version。
- `secret_command_receipts(scope, idempotency_key, command_kind, receipt_key_version, request_hmac BLOB, status, result_object_id FK NULL, unlock_session_id NULL, unlock_process_generation NULL, result_expires_at NULL, created_at, completed_at NULL, expired_at NULL, PRIMARY KEY(scope, idempotency_key))`；只存 `12-secret-store.md` 的 data-key-derived HMAC 和非敏感 result，不存普通 request digest/body；session 两列只允许 initialize/unlock command 同时非空。
- `secret_audit(id PK, action, secret_id NULL, caller_kind NULL, caller_id NULL, result_kind, created_at)`；只记 metadata/result，不记明文、header、URL 或 resolver buffer。

索引：`run_events(run_id, seq)`、`run_events(run_id, event_type, seq)`、`domain_events(aggregate_kind, aggregate_id, lineage_key, seq)`、`runtime_checkpoints(run_id, through_seq DESC)`。ephemeral stream event 不进入事件表，也没有 durable seq。

Secret 表不保存 master password、KEK、解包后的 data key 或 resolver session key；receipt 中的随机 session ID/generation 只是非敏感幂等绑定，不是 credential。解密值不进入 event、state、memory、node output。header 与 record 更新作为一个可恢复事务提交；`api_key_ref` 由 SecretResolver 校验，不让 core runtime 直接查询此表。阶段一 `active_key_version/key_version/receipt_key_version` 恒为 1；secret receipt HMAC 只在成功 initialize/unlock/write 后使用内存 data key 派生的专用 key 计算，失败密码不落 digest。Initialize/unlock 重放还必须命中当前进程内存 registry 的同 session/generation；DB status 即使因 crash 未标 expired，也不能使旧 session 复活。Internal-sensitive object 使用同一 data key 的另一 purpose-bound subkey，两者不复用 key/info；解密 port 仍位于受控 storage/provider 边界。

### 延后

event archive/partition、跨 run subscription cursor、KMS/keychain、OAuth refresh、secret rotation 和多租户 ACL。

## 关键事务

1. 应用 graph draft：校验完整定义，插入 immutable revision，再推进 draft token；已有 run 不变。
2. 创建 run：读取 context branch head，固定 graph revision/content hash、execution manifest 和 input commit，创建 run、counter/cursor、入口 NodeInstance、wakeup 和 started events。
3. firing：`BEGIN IMMEDIATE`，确认同 `(runId, nodeId)` 无非终态 instance，原子消费各 required edge FIFO 队首，创建 instance/attempt/read set/event/wakeup。
4. node finalized：CAS run epoch/lease/head；同事务写 commit/projection、完成 attempt、发射 output edge、更新 wait/run、追加 durable events/outbox；commit 后 publish。
5. memory apply / context merge：前者校验 proposal change 与 expected memory head，后者校验双 branch heads；各自写 commit parents、CAS head、projection、conflict/proposal 状态和 event。
6. effect：事务写 pending logical effect + prepared attempt；事务外调用；再以事务写 attempt/result/effect status。crash 中间态按 classification 处理。
7. wait response：先按 delivery digest幂等，再锁定 ordered blockers并逐项校验；同事务更新 tool/proposal + blocker decision，全部 terminal且无 aborted才 resolve wait、置 NodeInstance ready、写 checkpoint/wakeup/journal。存在 open effect blocker时 generic response零写入拒绝。
8. effect resolution / run terminal fencing：前者先按 command digest幂等并校验 expected attempt/epoch，同事务写人工 resolution、CAS logical effect/model-or-tool owner/checkpoint/blocker并 settle wait。后者先锁定 run 下全部非终态 model/count/tool owner/effect，而非只扫 active attempt：无 unresolved started 的 prepared/retry-ready pending effect置 cancelled_before_start；unresolved started/unknown置 factual unknown + logical abandoned_unknown；尚无 effect 的 requested/validated/awaiting-approval tool置 cancelled_before_start并 abort blocker。System resolution只关联尚无 resolution的真实 attempt，其余以 terminal journal引用既有结论；最后复核无可 dispatch logical work。任一 CAS不匹配则整笔重新分类/回滚。
9. checkpoint：在同一一致读写切面记录 `through_seq`、head 和 snapshot；恢复只 replay 更大 seq。
10. 提交用户消息：CAS active head H，写 message/commit/Turn，把当前 branch 与 Conversation active head推进到 U，再从 U fork candidate branch并创建 run/wakeup；幂等失败不留下半个 Turn。
11. run 完成投影：在 candidate branch final head 后追加 assistant message commit，并原子更新 candidate/message；failed/cancelled 不创建消息。
12. swipe：校验 ready candidate，原子更新 selection 与 Conversation active branch/head。
13. 创建顶层资源：Graph 写 canonical 空 draft，Channel/Preset 写空 head；各自与 immutable application receipt/result 同事务提交。
14. 创建 Conversation：写 context/root snapshot/root commit/root branch/projection/active pointer/application receipt；所有 ID 预分配，任一失败全回滚。
15. 初始化 Secret Store：CAS singleton header 不存在，同事务写 header、store-created audit、secret HMAC receipt/result；commit 后才安装进程内 session，header 已存在绝不覆盖。
16. CountCall：首次创建 `(nodeInstanceId,countOrdinal)` 时，同事务写 exact pin/candidate/request digest、pure effect/prepared attempt、checkpoint、journal 并仅此时递增 `countCallsUsed`；prepared→running、terminal、local fallback 和 retry-ready→prepared 各自原子更新 ledger/CountCall/checkpoint/journal。Crash/replay 复用同 logical row/result，不重复扣预算或重新裁剪候选。
17. Conversation run profile：校验新 GraphRevision/input/reply contract，以 expected revision CAS更新default run三字段并写application receipt；expected=0只匹配当前三字段为空并创建revision 1。它不更新active head或历史Candidate，后续Turn仍把实际spec复制到candidate binding。

## SQLite 与 PostgreSQL 并发

SQLite 阶段一启用 WAL、`busy_timeout`，保持写事务短小；需要抢占/CAS 时使用 `BEGIN IMMEDIATE`、条件 `UPDATE ... WHERE head/epoch/status = ?` 并检查 affected rows。SQLite 没有 `SELECT FOR UPDATE`/`SKIP LOCKED`，不能先读后无条件写。每 run seq 通过 counter row 分配。

构建/启动检查 SQLite FTS5 capability（desktop/mobile 均跑 repository contract test）；缺失时明确禁用 `search_memory` 并报 capability error，不能静默换 ranking。若平台无法稳定提供 FTS5，再通过设计变更选择自带索引实现。

PostgreSQL 可用 row lock、`FOR UPDATE SKIP LOCKED` 领取任务，JSONB/TIMESTAMPTZ/BYTEA 替代对应类型。常规事务使用 Read Committed + 显式 CAS，复杂多 head merge 可选择 Serializable。不能因换库改变 FIFO、commit、event seq 或 effect 语义。

## Migration 规则

- 使用 SeaORM migration 的单调、forward-only 版本；禁止修改已发布 migration。
- applied graph revision、commit、event、message、effect attempt 和 effect resolution 不原地改写；修正用新记录。
- SQLite 复杂约束变更使用“建新表、复制校验、交换表”，迁移前备份并运行 FK/integrity check。
- preset head、branch head、commit origin 等循环引用先以 nullable pointer 建根记录，再在同一事务补齐；需要跨行原子校验时显式声明 deferred FK，不能临时关闭 foreign keys。
- 大表变更分为新增 nullable 字段、分批 backfill、代码双读/双写、最后加约束；不要在迁移中调用 LLM、tool 或非确定 reducer。
- event、commit、checkpoint payload 自带 schema version；至少保留上一版本 reader，历史 replay 使用记录时的 reducer/policy version。
- graph/channel/execution snapshot/model call 的 operation taxonomy 与 adapter decoder version 不做默认 backfill；reader 只接受显式 support matrix，未知值 fail closed，兼容升级必须先迁移并校验 payload/hash。
- Secret 重加密是显式可恢复任务，不把明文写入 migration 日志。
- CI 对 SQLite 执行 fresh install、逐版本 upgrade 和备份恢复 smoke test；引入 PostgreSQL 时对两库运行同一 repository contract tests。
