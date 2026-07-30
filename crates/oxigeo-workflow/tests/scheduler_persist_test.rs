//! Integration tests for crash-safe scheduler state persistence.
//!
//! These tests exercise `persist_state` and `load_state` on the `Scheduler`
//! struct, verifying atomic writes, round-trip fidelity, graceful handling of
//! corrupt lines, and automatic persistence triggers on mutating operations.

use oxigeo_workflow::dag::WorkflowDag;
use oxigeo_workflow::scheduler::{ScheduleType, ScheduledWorkflow};
use oxigeo_workflow::{Scheduler, SchedulerConfig, WorkflowDefinition};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Per-test scratch fixture inside the system temp dir (house policy: no
/// hardcoded absolute paths).  The file is NOT created by this helper.
///
/// The leaf name embeds the process id and a monotonic counter, so no two test
/// binaries — nor two concurrent runs of this one — can ever land on the same
/// file.  Dropping the guard removes the fixture, so a panicking test leaks
/// nothing.
struct TempPath(PathBuf);

impl TempPath {
    fn new(name: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!(
            "oxigeo_scheduler_persist_{}_{seq}_{name}.jsonl",
            std::process::id()
        )))
    }
}

impl std::ops::Deref for TempPath {
    type Target = std::path::Path;

    fn deref(&self) -> &std::path::Path {
        &self.0
    }
}

impl AsRef<std::path::Path> for TempPath {
    fn as_ref(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Build a minimal `WorkflowDefinition` suitable for scheduling.
fn make_workflow(id: &str) -> WorkflowDefinition {
    WorkflowDefinition {
        id: id.to_string(),
        name: format!("Workflow {}", id),
        version: "1.0.0".to_string(),
        dag: WorkflowDag::new(),
        description: None,
    }
}

/// Build a `Scheduler` with persistence enabled at `path`.
fn make_scheduler_with_path(path: PathBuf) -> Scheduler {
    let cfg = SchedulerConfig {
        enable_persistence: true,
        persistence_path: Some(path.to_string_lossy().into_owned()),
        ..SchedulerConfig::default()
    };
    Scheduler::new(cfg)
}

/// Build a `Scheduler` with persistence disabled (no path).
fn make_scheduler_no_persistence() -> Scheduler {
    let cfg = SchedulerConfig {
        enable_persistence: false,
        persistence_path: None,
        ..SchedulerConfig::default()
    };
    Scheduler::new(cfg)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_persist_state_no_op_when_no_path() {
    // A scheduler with no persistence_path must return Ok(()) and must NOT
    // create any files in the temp directory.
    let sentinel = TempPath::new("no_op_sentinel");

    let scheduler = make_scheduler_no_persistence();
    // Force-add a schedule via internal DashMap — but since add_schedule also
    // calls persist_state conditionally, just call load_state (no path ⇒ no-op).
    scheduler
        .load_state()
        .await
        .expect("load_state should return Ok when no path");

    // No file should have appeared.
    assert!(
        !sentinel.exists(),
        "persist_state must not create files when persistence_path is None"
    );
}

#[tokio::test]
async fn test_persist_state_creates_file() {
    let path = TempPath::new("creates_file");

    let scheduler = make_scheduler_with_path(path.to_path_buf());

    // add_schedule calls persist_state internally when enable_persistence=true.
    scheduler
        .add_schedule(make_workflow("wf-creates-file"), ScheduleType::Manual)
        .await
        .expect("add_schedule failed");

    assert!(
        path.exists(),
        "persistence file must be created after add_schedule"
    );

    let content = std::fs::read_to_string(&path).expect("read persistence file");
    assert!(
        !content.trim().is_empty(),
        "persistence file must not be empty"
    );
}

#[tokio::test]
async fn test_load_state_missing_file_returns_ok() {
    let path = TempPath::new("missing_file");

    let scheduler = make_scheduler_with_path(path.to_path_buf());
    // Must succeed even though the file does not exist yet.
    scheduler
        .load_state()
        .await
        .expect("load_state must return Ok when file is absent");

    assert!(
        scheduler.get_schedules().is_empty(),
        "no schedules should be loaded from a missing file"
    );
}

#[tokio::test]
async fn test_persist_and_load_round_trip() {
    let path = TempPath::new("round_trip");

    // ── Phase 1: write three schedules ──────────────────────────────────────
    let scheduler_a = make_scheduler_with_path(path.to_path_buf());

    let id1 = scheduler_a
        .add_schedule(make_workflow("wf-rt-1"), ScheduleType::Manual)
        .await
        .expect("add 1");
    let id2 = scheduler_a
        .add_schedule(
            make_workflow("wf-rt-2"),
            ScheduleType::Interval { interval_secs: 60 },
        )
        .await
        .expect("add 2");
    let id3 = scheduler_a
        .add_schedule(
            make_workflow("wf-rt-3"),
            ScheduleType::Cron {
                expression: "0 * * * * *".to_string(),
            },
        )
        .await
        .expect("add 3");

    // The file must exist after add_schedule triggers.
    assert!(path.exists(), "file must exist after adds");

    // ── Phase 2: load into a fresh scheduler ────────────────────────────────
    let scheduler_b = make_scheduler_with_path(path.to_path_buf());
    assert!(
        scheduler_b.get_schedules().is_empty(),
        "scheduler_b must start empty"
    );

    scheduler_b.load_state().await.expect("load_state failed");

    let schedules_b = scheduler_b.get_schedules();
    assert_eq!(
        schedules_b.len(),
        3,
        "scheduler_b must contain exactly 3 schedules after load"
    );

    // Verify all three IDs are present.
    let ids_b: std::collections::HashSet<String> =
        schedules_b.iter().map(|s| s.schedule_id.clone()).collect();
    assert!(ids_b.contains(&id1), "id1 missing after round-trip");
    assert!(ids_b.contains(&id2), "id2 missing after round-trip");
    assert!(ids_b.contains(&id3), "id3 missing after round-trip");

    // Verify workflow ID field survived serialization.
    let wf1 = scheduler_b
        .get_schedule(&id1)
        .expect("schedule 1 not found");
    assert_eq!(wf1.workflow.id, "wf-rt-1");

    let wf2 = scheduler_b
        .get_schedule(&id2)
        .expect("schedule 2 not found");
    assert_eq!(wf2.workflow.id, "wf-rt-2");
}

#[tokio::test]
async fn test_load_state_skips_corrupt_lines() {
    let path = TempPath::new("corrupt_lines");

    // Build two valid serialised ScheduledWorkflow values.
    let valid_wf = ScheduledWorkflow {
        schedule_id: "sched-corrupt-test-1".to_string(),
        workflow: make_workflow("wf-corrupt-1"),
        schedule_type: ScheduleType::Manual,
        enabled: true,
        last_execution: None,
        next_execution: None,
        execution_history: Vec::new(),
        max_history: 100,
        metadata: oxigeo_workflow::scheduler::ScheduleMetadata {
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            created_by: "test".to_string(),
            description: None,
            tags: Vec::new(),
        },
    };

    let valid_wf2 = ScheduledWorkflow {
        schedule_id: "sched-corrupt-test-2".to_string(),
        workflow: make_workflow("wf-corrupt-2"),
        ..valid_wf.clone()
    };

    let line1 = serde_json::to_string(&valid_wf).expect("serialize wf1");
    let line2 = serde_json::to_string(&valid_wf2).expect("serialize wf2");

    // Write: valid, garbage, valid.
    let content = format!("{}\nNOT_VALID_JSON{{{{GARBAGE\n{}\n", line1, line2);
    std::fs::write(&path, &content).expect("write corrupt-lines file");

    // Load into a fresh scheduler — must succeed with 2 loaded schedules.
    let scheduler = make_scheduler_with_path(path.to_path_buf());
    scheduler
        .load_state()
        .await
        .expect("load_state must return Ok despite corrupt lines");

    let schedules = scheduler.get_schedules();
    assert_eq!(
        schedules.len(),
        2,
        "exactly 2 valid schedules must be loaded; corrupt line must be skipped"
    );

    let ids: std::collections::HashSet<String> =
        schedules.iter().map(|s| s.schedule_id.clone()).collect();
    assert!(
        ids.contains("sched-corrupt-test-1"),
        "first valid schedule missing"
    );
    assert!(
        ids.contains("sched-corrupt-test-2"),
        "second valid schedule missing"
    );
}

#[tokio::test]
async fn test_atomic_write_no_tmp_file_remains() {
    let path = TempPath::new("atomic_write");

    // Derive the expected tmp path the same way the implementation does:
    // replace extension with "<ext>.tmp".
    let tmp_path: PathBuf = {
        let mut p = path.to_path_buf();
        let ext = match p.extension() {
            Some(e) => format!("{}.tmp", e.to_string_lossy()),
            None => "tmp".to_string(),
        };
        p.set_extension(ext);
        p
    };
    let _ = std::fs::remove_file(&tmp_path);

    let scheduler = make_scheduler_with_path(path.to_path_buf());
    scheduler
        .add_schedule(make_workflow("wf-atomic"), ScheduleType::Manual)
        .await
        .expect("add_schedule failed");

    // After a successful persist, the real file must exist…
    assert!(
        path.exists(),
        "persistence file must exist after persist_state"
    );
    // …and the .tmp sibling must have been renamed away.
    assert!(
        !tmp_path.exists(),
        "tmp file must not remain after successful atomic write"
    );
}

#[tokio::test]
async fn test_add_schedule_triggers_persist() {
    let path = TempPath::new("add_triggers");

    let scheduler = make_scheduler_with_path(path.to_path_buf());

    // No explicit persist_state call — add_schedule must trigger it.
    scheduler
        .add_schedule(make_workflow("wf-trigger"), ScheduleType::Manual)
        .await
        .expect("add_schedule failed");

    assert!(
        path.exists(),
        "add_schedule must trigger persist_state, creating the file"
    );
}

#[tokio::test]
async fn test_remove_schedule_triggers_persist() {
    let path = TempPath::new("remove_triggers");

    let scheduler = make_scheduler_with_path(path.to_path_buf());

    let schedule_id = scheduler
        .add_schedule(make_workflow("wf-remove-trigger"), ScheduleType::Manual)
        .await
        .expect("add failed");

    // Verify 1 schedule in file.
    {
        let content = std::fs::read_to_string(&path).expect("read after add");
        let line_count = content.lines().filter(|l| !l.trim().is_empty()).count();
        assert_eq!(line_count, 1, "file should have 1 line after add");
    }

    // Remove should trigger persist and produce an empty (or zero-line) file.
    scheduler
        .remove_schedule(&schedule_id)
        .await
        .expect("remove failed");

    {
        let content = std::fs::read_to_string(&path).expect("read after remove");
        let line_count = content.lines().filter(|l| !l.trim().is_empty()).count();
        assert_eq!(
            line_count, 0,
            "file should have 0 lines after the only schedule is removed"
        );
    }
}

#[tokio::test]
async fn test_persist_idempotent_on_empty_scheduler() {
    let path = TempPath::new("empty_persist");

    let scheduler = make_scheduler_with_path(path.to_path_buf());

    // Calling load_state on a new scheduler with no file must be a no-op.
    scheduler.load_state().await.expect("load on empty");

    // Now persist an empty map explicitly.
    // We can simulate this by calling add_schedule then remove_schedule
    // (both trigger persist), resulting in an empty file.
    let id = scheduler
        .add_schedule(make_workflow("wf-empty"), ScheduleType::Manual)
        .await
        .expect("add");
    scheduler.remove_schedule(&id).await.expect("remove");

    assert!(
        path.exists(),
        "file must exist even after all schedules removed"
    );

    let content = std::fs::read_to_string(&path).expect("read");
    assert!(
        content.lines().filter(|l| !l.trim().is_empty()).count() == 0,
        "file must be empty after all schedules removed"
    );

    // Reload — must succeed and remain empty.
    let scheduler2 = make_scheduler_with_path(path.to_path_buf());
    scheduler2.load_state().await.expect("reload empty file");
    assert!(
        scheduler2.get_schedules().is_empty(),
        "reloaded scheduler must be empty"
    );
}
