# TODO: oxigdal-workflow

> **Purpose:** DAG-based workflow engine — orchestrates geospatial processing pipelines with cycle detection, retries, timeouts, scheduling.
> **Status (2026-05-16):** 20,632 LoC · 197 tests · 4 real-code stubs (scheduler persistence, Temporal/Airflow/Prefect import, external HMAC)
> **Roadmap:** v0.1.7 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [x] Implement scheduler state persistence (save/load to disk)
  - Done: 2026-05-31 (Slice 28). Tests: 9 new (scheduler_persist_test) + 150 existing = 159 total.
  - **Verified gap:** `src/scheduler/mod.rs:413-417` — `if let Some(_path) = &self.config.persistence_path { /* Persistence implementation would go here */ /* For now, this is a placeholder */ }`. Same pattern at `:422-428` for `load_state`.
  - **Goal:** Persist `Schedule` collection + `execution_history` to JSON file under `persistence_path`; on `Scheduler::start()`, reload pending/in-flight schedules so crash-recovery resumes within the next tick.
  - **Design:** `persist_state` serializes `DashMap<ScheduleId, Schedule>` snapshot via `tokio::fs::write` with `<path>.tmp` rename (atomic). Format: JSON-Lines, one `Schedule` per line for incremental updates. `load_state` reads file, repopulates map. Trigger save on `add_schedule`/`remove_schedule`/`record_execution` (debounced via `tokio::time::interval` to reduce write amplification).
  - **Files:** `src/scheduler/mod.rs:412-428` (replace stub bodies), add `persist_interval: Duration` to `SchedulerConfig`.
  - **Tests:** *(proposed)* `test_scheduler_persists_on_add`, `test_scheduler_load_state_restores_schedules`, `test_scheduler_atomic_rename`, `test_scheduler_corrupt_file_returns_error`, `test_scheduler_missing_file_starts_empty`.
  - **Risk:** Race between in-flight `update_execution_status` and snapshot read — take a clone-snapshot under lock before flushing.
  - **Prerequisites:** None.

- [x] Replace HMAC-SHA256 placeholder in external WebhookTrigger with real crypto
  - Done: 2026-05-31 (Slice 29). Tests: 9 new (webhook_hmac_test) + 174 existing = 183 total.
  - Real `hmac::Hmac<Sha256>` (RustCrypto 0.13/0.11). `hex_encode` → `hmac_sha256_hex`; uppercase normalization; RFC 4231 KAT pinned. Also fixed orphaned `pub mod external;` in `integrations/mod.rs`.

- [ ] Implement workflow import for Airflow, Prefect, Temporal
  - **Verified gap:** `src/integrations/airflow.rs:62` — `"Import from Airflow not yet implemented"`; `src/integrations/prefect.rs:45` — `"Import from Prefect not yet implemented"`; `src/integrations/temporal.rs:65` — `"Import from Temporal not yet implemented"`; `src/integrations/temporal.rs:27` — `go_code.push_str("    // TODO: Implement activity logic\n");` (TODO emitted into exported Go).
  - **Goal:** Each `import_workflow` parses the source dialect (Python AST for Airflow/Prefect, Go AST for Temporal) and constructs a `WorkflowDefinition` with equivalent tasks + dependency edges.
  - **Design:** Out-of-scope for parsing native Python/Go in Rust. Instead, accept the dialect's JSON serialization: Airflow exports DAGs as JSON via `airflow dags show --output json`; Prefect via `prefect flow inspect`; Temporal via its query API. Parse JSON → walk task/dep tree → emit `TaskNode` + `add_dependency`. Document the JSON contract per dialect.
  - **Files:** `src/integrations/airflow.rs:60-65` (replace stub), `src/integrations/prefect.rs:43-48`, `src/integrations/temporal.rs:61-67`, `src/integrations/temporal.rs:27` (replace TODO comment with parameterized activity stub generator).
  - **Tests:** *(proposed)* `test_airflow_import_simple_dag_json`, `test_prefect_import_with_dependencies`, `test_temporal_import_workflow_signature`, `test_airflow_import_malformed_json_errors`.
  - **Risk:** Python-side AST → JSON converters are not stable across Airflow versions; document tested versions in rustdoc and pin one schema.
  - **Prerequisites:** None.

## Medium Priority
- [ ] Persistent execution history backed by SQLite (or oxiarc-archive batched JSON)
  - **Goal:** Survive process restarts with full audit trail; current `ExecutionHistory` is in-memory.
  - **Files:** `src/monitoring/tracker/types.rs` (1520 LoC, refactor candidate per <2000 LoC policy), `src/engine/state.rs` (756 LoC).
  - **Why deferred:** In-memory + serializable today; persistent layer requires schema design.

- [ ] Resource limits per task (memory, CPU, disk I/O)
  - **Goal:** `ResourceRequirements` (already at `src/dag/graph.rs:58-69`) is recorded but not enforced; cgroups (Linux) or rlimits required for real enforcement.
  - **Files:** `src/engine/executor.rs` (843 LoC).
  - **Why deferred:** Pure Rust portable enforcement is hard; document declarative-only for now.

- [ ] Dynamic DAG modification at runtime (add/remove tasks mid-execution)
  - **Goal:** Mutate running DAG while preserving consistency (e.g., add tasks downstream of an active node).
  - **Files:** `src/dag/graph.rs` (1136 LoC), `src/engine/runtime.rs` (325 LoC).
  - **Why deferred:** Most workflows are static; complex invariant preservation deferred.

- [ ] Sub-workflow (nested DAG) execution
  - **Goal:** `TaskNode` with `config` referencing another `WorkflowDefinition`; engine recurses.
  - **Files:** `src/engine/executor.rs`.
  - **Why deferred:** Requires shared `TaskExecutor` registry — design open.

- [ ] Webhook trigger end-to-end (axum/server feature already gated)
  - **Goal:** HTTP server accepting webhooks, validating HMAC signature, instantiating workflow.
  - **Files:** `src/integrations/external.rs` (1739 LoC — refactor candidate per <2000 LoC policy).
  - **Why deferred:** Server feature already gated; full wiring needs HMAC fix above first.

- [ ] Map/reduce pattern operator for parallel batch processing
  - **Goal:** Fan-out N parallel tasks, fan-in collector task; declarative.
  - **Files:** `src/dag/parallelism.rs` (354 LoC).
  - **Why deferred:** Manual DAG construction supports this today; sugar deferred.

## Low Priority / Future (one-liners)
- [ ] Workflow marketplace/registry for sharing reusable pipelines
- [ ] A/B testing workflow variants with metric comparison
- [ ] Cloud cost estimation (AWS/GCP pricing per task type)
- [ ] Workflow diff/versioning with semantic change detection (`src/versioning/diff.rs` exists, 791 LoC)
- [ ] Kubernetes Job/CronJob manifest generation from `WorkflowDefinition`
- [ ] Data lineage tracking across workflow executions (oxigdal-security lineage integration)
- [ ] SLA monitoring with deadline-based alerting
- [ ] Cost estimation for cloud-executed workflows (AWS/GCP pricing)

## Cross-crate dependencies
- **Blocks:** oxigdal-etl (shares pipeline patterns), oxigdal-services (workflow REST API)
- **Blocked by:** None for High Priority items

## Recently completed (verbatim)
- [x] DAG cycle detection with DFS — `src/dag/graph.rs:241-284` (`check_cycles` + `dfs_cycle_check`); also dependency-graph cycle check at `src/scheduler/dependency.rs:105-141`
- [x] DAG validation: empty-graph + cycle + reachability — `src/dag/graph.rs:226-238`
- [x] Topological sort for execution order — `src/dag/topological_sort.rs` (340 LoC)
- [x] Retry policy with exponential backoff per task — `src/engine/executor.rs:366` (uses `backoff_multiplier.powi(attempt)`); policy struct at `src/dag/graph.rs:33-54`
- [x] Task timeout enforcement via `tokio::time::timeout` — `src/engine/executor.rs:387-388`
- [x] Workflow cancellation API — `src/engine/runtime.rs:166-180`
- [x] Cron-based scheduling — `src/scheduler/cron.rs` (292 LoC), interval at `src/scheduler/interval.rs` (388 LoC), event-driven at `src/scheduler/event.rs` (432 LoC)
- [x] Workflow templates + parameterization with `{{name}}` placeholders — `src/templates/parameterization.rs` (354 LoC), library at `library.rs` (435 LoC), geospatial templates at `geospatial.rs` (1343 LoC)
- [x] Workflow versioning with rollback + history + diff + migration — `src/versioning/` (2632 LoC across 4 files)
- [x] Conditional execution and branching via `Expression` evaluator — `src/conditional/expressions.rs` (408 LoC), `branching.rs` (278 LoC)
- [x] Monitoring: metrics, logging, debugging, tracker, visualization — `src/monitoring/` (4159 LoC across 6+ files; `visualization.rs` 1551 LoC — close to <2000 LoC limit)
- [x] Temporal.io Go export (workflow → Go code) — `src/integrations/temporal.rs:11-59`
- [x] Airflow Python DAG export — `src/integrations/airflow.rs`
- [x] Prefect flow export — `src/integrations/prefect.rs`
- [x] Resource requirements struct (CPU cores, memory MB, GPU flag, disk MB, custom map) — `src/dag/graph.rs:56-69`

---
*Last audited: 2026-05-16*
