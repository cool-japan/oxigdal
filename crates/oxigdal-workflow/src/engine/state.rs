//! Workflow state management and persistence.

use crate::dag::WorkflowDag;
use crate::error::{Result, WorkflowError};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tokio::fs;

/// Workflow execution state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowState {
    /// Workflow ID.
    pub workflow_id: String,
    /// Workflow execution ID (unique per run).
    pub execution_id: String,
    /// Current workflow status.
    pub status: WorkflowStatus,
    /// Task states.
    pub task_states: HashMap<String, TaskState>,
    /// Workflow metadata.
    pub metadata: WorkflowMetadata,
    /// Execution context.
    pub context: ExecutionContext,
}

/// Workflow execution status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkflowStatus {
    /// Workflow is pending execution.
    Pending,
    /// Workflow is currently running.
    Running,
    /// Workflow completed successfully.
    Completed,
    /// Workflow failed.
    Failed,
    /// Workflow was cancelled.
    Cancelled,
    /// Workflow is paused.
    Paused,
}

/// Individual task state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskState {
    /// Task ID.
    pub task_id: String,
    /// Current task status.
    pub status: TaskStatus,
    /// Number of attempts.
    pub attempts: u32,
    /// Task start time.
    pub started_at: Option<DateTime<Utc>>,
    /// Task completion time.
    pub completed_at: Option<DateTime<Utc>>,
    /// Task duration in milliseconds.
    pub duration_ms: Option<u64>,
    /// Task output (if any).
    pub output: Option<serde_json::Value>,
    /// Task error (if any).
    pub error: Option<String>,
    /// Task logs.
    pub logs: Vec<String>,
}

/// Task execution status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    /// Task is pending execution.
    Pending,
    /// Task is currently running.
    Running,
    /// Task completed successfully.
    Completed,
    /// Task failed.
    Failed,
    /// Task was skipped (due to conditionals).
    Skipped,
    /// Task was cancelled.
    Cancelled,
    /// Task is waiting for retry.
    WaitingRetry,
}

/// Workflow metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowMetadata {
    /// Workflow name.
    pub name: String,
    /// Workflow version.
    pub version: String,
    /// Workflow creation time.
    pub created_at: DateTime<Utc>,
    /// Workflow start time.
    pub started_at: Option<DateTime<Utc>>,
    /// Workflow completion time.
    pub completed_at: Option<DateTime<Utc>>,
    /// Total duration in milliseconds.
    pub duration_ms: Option<u64>,
    /// Workflow creator/owner.
    pub owner: Option<String>,
    /// Custom tags.
    pub tags: HashMap<String, String>,
}

/// Execution context shared across tasks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionContext {
    /// Shared variables.
    pub variables: HashMap<String, serde_json::Value>,
    /// Workflow parameters.
    pub parameters: HashMap<String, serde_json::Value>,
    /// Environment variables.
    pub env: HashMap<String, String>,
}

impl WorkflowState {
    /// Create a new workflow state.
    pub fn new(workflow_id: String, execution_id: String, name: String) -> Self {
        Self {
            workflow_id,
            execution_id,
            status: WorkflowStatus::Pending,
            task_states: HashMap::new(),
            metadata: WorkflowMetadata {
                name,
                version: "1.0.0".to_string(),
                created_at: Utc::now(),
                started_at: None,
                completed_at: None,
                duration_ms: None,
                owner: None,
                tags: HashMap::new(),
            },
            context: ExecutionContext {
                variables: HashMap::new(),
                parameters: HashMap::new(),
                env: HashMap::new(),
            },
        }
    }

    /// Initialize a task state.
    pub fn init_task(&mut self, task_id: String) {
        self.task_states.insert(
            task_id.clone(),
            TaskState {
                task_id,
                status: TaskStatus::Pending,
                attempts: 0,
                started_at: None,
                completed_at: None,
                duration_ms: None,
                output: None,
                error: None,
                logs: Vec::new(),
            },
        );
    }

    /// Mark a task as running.
    pub fn start_task(&mut self, task_id: &str) -> Result<()> {
        let task_state = self
            .task_states
            .get_mut(task_id)
            .ok_or_else(|| WorkflowError::not_found(format!("Task '{}'", task_id)))?;

        task_state.status = TaskStatus::Running;
        task_state.started_at = Some(Utc::now());
        task_state.attempts += 1;

        Ok(())
    }

    /// Mark a task as completed.
    pub fn complete_task(
        &mut self,
        task_id: &str,
        output: Option<serde_json::Value>,
    ) -> Result<()> {
        let task_state = self
            .task_states
            .get_mut(task_id)
            .ok_or_else(|| WorkflowError::not_found(format!("Task '{}'", task_id)))?;

        task_state.status = TaskStatus::Completed;
        task_state.completed_at = Some(Utc::now());
        task_state.output = output;

        if let Some(started) = task_state.started_at {
            task_state.duration_ms = Some(
                (Utc::now() - started)
                    .num_milliseconds()
                    .try_into()
                    .unwrap_or(0),
            );
        }

        Ok(())
    }

    /// Mark a task as failed.
    pub fn fail_task(&mut self, task_id: &str, error: String) -> Result<()> {
        let task_state = self
            .task_states
            .get_mut(task_id)
            .ok_or_else(|| WorkflowError::not_found(format!("Task '{}'", task_id)))?;

        task_state.status = TaskStatus::Failed;
        task_state.completed_at = Some(Utc::now());
        task_state.error = Some(error);

        if let Some(started) = task_state.started_at {
            task_state.duration_ms = Some(
                (Utc::now() - started)
                    .num_milliseconds()
                    .try_into()
                    .unwrap_or(0),
            );
        }

        Ok(())
    }

    /// Mark a task as skipped.
    pub fn skip_task(&mut self, task_id: &str) -> Result<()> {
        let task_state = self
            .task_states
            .get_mut(task_id)
            .ok_or_else(|| WorkflowError::not_found(format!("Task '{}'", task_id)))?;

        task_state.status = TaskStatus::Skipped;
        task_state.completed_at = Some(Utc::now());

        Ok(())
    }

    /// Add a log entry for a task.
    pub fn add_task_log(&mut self, task_id: &str, log: String) -> Result<()> {
        let task_state = self
            .task_states
            .get_mut(task_id)
            .ok_or_else(|| WorkflowError::not_found(format!("Task '{}'", task_id)))?;

        task_state.logs.push(log);

        Ok(())
    }

    /// Start the workflow execution.
    pub fn start(&mut self) {
        self.status = WorkflowStatus::Running;
        self.metadata.started_at = Some(Utc::now());
    }

    /// Mark the workflow as completed.
    pub fn complete(&mut self) {
        self.status = WorkflowStatus::Completed;
        self.metadata.completed_at = Some(Utc::now());

        if let Some(started) = self.metadata.started_at {
            self.metadata.duration_ms = Some(
                (Utc::now() - started)
                    .num_milliseconds()
                    .try_into()
                    .unwrap_or(0),
            );
        }
    }

    /// Mark the workflow as failed.
    pub fn fail(&mut self) {
        self.status = WorkflowStatus::Failed;
        self.metadata.completed_at = Some(Utc::now());

        if let Some(started) = self.metadata.started_at {
            self.metadata.duration_ms = Some(
                (Utc::now() - started)
                    .num_milliseconds()
                    .try_into()
                    .unwrap_or(0),
            );
        }
    }

    /// Mark the workflow as cancelled.
    pub fn cancel(&mut self) {
        self.status = WorkflowStatus::Cancelled;
        self.metadata.completed_at = Some(Utc::now());

        if let Some(started) = self.metadata.started_at {
            self.metadata.duration_ms = Some(
                (Utc::now() - started)
                    .num_milliseconds()
                    .try_into()
                    .unwrap_or(0),
            );
        }
    }

    /// Get task state.
    pub fn get_task_state(&self, task_id: &str) -> Option<&TaskState> {
        self.task_states.get(task_id)
    }

    /// Set a context variable.
    pub fn set_variable(&mut self, key: String, value: serde_json::Value) {
        self.context.variables.insert(key, value);
    }

    /// Get a context variable.
    pub fn get_variable(&self, key: &str) -> Option<&serde_json::Value> {
        self.context.variables.get(key)
    }

    /// Check if the workflow is terminal (completed, failed, or cancelled).
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            WorkflowStatus::Completed | WorkflowStatus::Failed | WorkflowStatus::Cancelled
        )
    }
}

/// Workflow checkpoint containing both state and DAG for recovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowCheckpoint {
    /// Checkpoint version for compatibility.
    pub version: u32,
    /// Timestamp when checkpoint was created.
    pub created_at: DateTime<Utc>,
    /// Checkpoint sequence number (increments with each save).
    pub sequence: u64,
    /// The workflow state.
    pub state: WorkflowState,
    /// The workflow DAG definition.
    pub dag: WorkflowDag,
}

impl WorkflowCheckpoint {
    /// Current checkpoint format version.
    pub const CURRENT_VERSION: u32 = 1;

    /// Create a new checkpoint from state and DAG.
    pub fn new(state: WorkflowState, dag: WorkflowDag, sequence: u64) -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            created_at: Utc::now(),
            sequence,
            state,
            dag,
        }
    }

    /// Get tasks that need to be executed (pending or failed but retriable).
    pub fn get_pending_tasks(&self) -> Vec<String> {
        self.state
            .task_states
            .iter()
            .filter(|(_, ts)| matches!(ts.status, TaskStatus::Pending | TaskStatus::WaitingRetry))
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Get tasks that were running when checkpoint was saved (need retry).
    pub fn get_interrupted_tasks(&self) -> Vec<String> {
        self.state
            .task_states
            .iter()
            .filter(|(_, ts)| ts.status == TaskStatus::Running)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Get tasks that completed successfully.
    pub fn get_completed_tasks(&self) -> Vec<String> {
        self.state
            .task_states
            .iter()
            .filter(|(_, ts)| ts.status == TaskStatus::Completed)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Get tasks that failed (not retriable).
    pub fn get_failed_tasks(&self) -> Vec<String> {
        self.state
            .task_states
            .iter()
            .filter(|(_, ts)| ts.status == TaskStatus::Failed)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Get tasks that were skipped.
    pub fn get_skipped_tasks(&self) -> Vec<String> {
        self.state
            .task_states
            .iter()
            .filter(|(_, ts)| ts.status == TaskStatus::Skipped)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Check if all dependencies for a task are satisfied.
    pub fn are_dependencies_satisfied(&self, task_id: &str) -> bool {
        let dependencies = self.dag.get_dependencies(task_id);
        dependencies.iter().all(|dep_id| {
            self.state
                .task_states
                .get(dep_id)
                .map(|ts| ts.status == TaskStatus::Completed)
                .unwrap_or(false)
        })
    }

    /// Get tasks ready to execute (pending with satisfied dependencies).
    pub fn get_ready_tasks(&self) -> Vec<String> {
        self.get_pending_tasks()
            .into_iter()
            .filter(|task_id| self.are_dependencies_satisfied(task_id))
            .collect()
    }

    /// Prepare state for resumption by resetting interrupted tasks.
    pub fn prepare_for_resume(&mut self) -> Result<()> {
        // Reset interrupted (running) tasks to pending for retry
        let interrupted = self.get_interrupted_tasks();
        for task_id in interrupted {
            if let Some(task_state) = self.state.task_states.get_mut(&task_id) {
                task_state.status = TaskStatus::Pending;
                // Keep attempt count for proper retry tracking
            }
        }

        // Reset workflow status to running
        if self.state.status == WorkflowStatus::Paused {
            self.state.status = WorkflowStatus::Running;
        }

        Ok(())
    }
}

/// State persistence manager.
pub struct StatePersistence {
    /// Directory for state storage.
    state_dir: String,
}

impl StatePersistence {
    /// Create a new state persistence manager.
    pub fn new(state_dir: String) -> Self {
        Self { state_dir }
    }

    /// Save workflow state to disk.
    pub async fn save(&self, state: &WorkflowState) -> Result<()> {
        let dir_path = Path::new(&self.state_dir);
        fs::create_dir_all(dir_path).await.map_err(|e| {
            WorkflowError::persistence(format!("Failed to create state dir: {}", e))
        })?;

        let file_path = dir_path.join(format!("{}.json", state.execution_id));
        let json = serde_json::to_string_pretty(state)?;

        fs::write(&file_path, json)
            .await
            .map_err(|e| WorkflowError::persistence(format!("Failed to write state: {}", e)))?;

        Ok(())
    }

    /// Load workflow state from disk.
    pub async fn load(&self, execution_id: &str) -> Result<WorkflowState> {
        let file_path = Path::new(&self.state_dir).join(format!("{}.json", execution_id));

        let json = fs::read_to_string(&file_path)
            .await
            .map_err(|e| WorkflowError::persistence(format!("Failed to read state: {}", e)))?;

        let state = serde_json::from_str(&json)?;
        Ok(state)
    }

    /// Delete workflow state from disk.
    pub async fn delete(&self, execution_id: &str) -> Result<()> {
        let file_path = Path::new(&self.state_dir).join(format!("{}.json", execution_id));

        fs::remove_file(&file_path)
            .await
            .map_err(|e| WorkflowError::persistence(format!("Failed to delete state: {}", e)))?;

        Ok(())
    }

    /// List all workflow states.
    pub async fn list(&self) -> Result<Vec<String>> {
        let dir_path = Path::new(&self.state_dir);

        if !dir_path.exists() {
            return Ok(Vec::new());
        }

        let mut entries = fs::read_dir(dir_path)
            .await
            .map_err(|e| WorkflowError::persistence(format!("Failed to read state dir: {}", e)))?;

        let mut execution_ids = Vec::new();

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| WorkflowError::persistence(format!("Failed to read entry: {}", e)))?
        {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    execution_ids.push(stem.to_string());
                }
            }
        }

        Ok(execution_ids)
    }

    /// Save a workflow checkpoint (state + DAG) to disk.
    pub async fn save_checkpoint(&self, checkpoint: &WorkflowCheckpoint) -> Result<()> {
        let dir_path = Path::new(&self.state_dir).join("checkpoints");
        fs::create_dir_all(&dir_path).await.map_err(|e| {
            WorkflowError::persistence(format!("Failed to create checkpoint dir: {}", e))
        })?;

        let file_path = dir_path.join(format!(
            "{}_checkpoint_{}.json",
            checkpoint.state.execution_id, checkpoint.sequence
        ));
        let json = serde_json::to_string_pretty(checkpoint)?;

        fs::write(&file_path, json).await.map_err(|e| {
            WorkflowError::persistence(format!("Failed to write checkpoint: {}", e))
        })?;

        // Also save a "latest" symlink/copy for easy access
        let latest_path = dir_path.join(format!("{}_latest.json", checkpoint.state.execution_id));
        let json_latest = serde_json::to_string_pretty(checkpoint)?;
        fs::write(&latest_path, json_latest).await.map_err(|e| {
            WorkflowError::persistence(format!("Failed to write latest checkpoint: {}", e))
        })?;

        Ok(())
    }

    /// Load the latest checkpoint for an execution.
    pub async fn load_checkpoint(&self, execution_id: &str) -> Result<WorkflowCheckpoint> {
        let latest_path = Path::new(&self.state_dir)
            .join("checkpoints")
            .join(format!("{}_latest.json", execution_id));

        let json = fs::read_to_string(&latest_path)
            .await
            .map_err(|e| WorkflowError::persistence(format!("Failed to read checkpoint: {}", e)))?;

        let checkpoint: WorkflowCheckpoint = serde_json::from_str(&json)?;

        // Validate checkpoint version
        if checkpoint.version > WorkflowCheckpoint::CURRENT_VERSION {
            return Err(WorkflowError::persistence(format!(
                "Checkpoint version {} is newer than supported version {}",
                checkpoint.version,
                WorkflowCheckpoint::CURRENT_VERSION
            )));
        }

        Ok(checkpoint)
    }

    /// Load a specific checkpoint by sequence number.
    pub async fn load_checkpoint_by_sequence(
        &self,
        execution_id: &str,
        sequence: u64,
    ) -> Result<WorkflowCheckpoint> {
        let file_path = Path::new(&self.state_dir)
            .join("checkpoints")
            .join(format!("{}_checkpoint_{}.json", execution_id, sequence));

        let json = fs::read_to_string(&file_path)
            .await
            .map_err(|e| WorkflowError::persistence(format!("Failed to read checkpoint: {}", e)))?;

        let checkpoint: WorkflowCheckpoint = serde_json::from_str(&json)?;
        Ok(checkpoint)
    }

    /// Delete a checkpoint.
    pub async fn delete_checkpoint(&self, execution_id: &str, sequence: u64) -> Result<()> {
        let file_path = Path::new(&self.state_dir)
            .join("checkpoints")
            .join(format!("{}_checkpoint_{}.json", execution_id, sequence));

        fs::remove_file(&file_path).await.map_err(|e| {
            WorkflowError::persistence(format!("Failed to delete checkpoint: {}", e))
        })?;

        Ok(())
    }

    /// Delete all checkpoints for an execution.
    pub async fn delete_all_checkpoints(&self, execution_id: &str) -> Result<()> {
        let checkpoints_dir = Path::new(&self.state_dir).join("checkpoints");

        if !checkpoints_dir.exists() {
            return Ok(());
        }

        let mut entries = fs::read_dir(&checkpoints_dir).await.map_err(|e| {
            WorkflowError::persistence(format!("Failed to read checkpoints dir: {}", e))
        })?;

        let prefix = format!("{}_", execution_id);

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| WorkflowError::persistence(format!("Failed to read entry: {}", e)))?
        {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                if name.starts_with(&prefix) {
                    fs::remove_file(&path).await.map_err(|e| {
                        WorkflowError::persistence(format!("Failed to delete checkpoint: {}", e))
                    })?;
                }
            }
        }

        Ok(())
    }

    /// List all checkpoints for an execution (sorted by sequence).
    pub async fn list_checkpoints(&self, execution_id: &str) -> Result<Vec<u64>> {
        let checkpoints_dir = Path::new(&self.state_dir).join("checkpoints");

        if !checkpoints_dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries = fs::read_dir(&checkpoints_dir).await.map_err(|e| {
            WorkflowError::persistence(format!("Failed to read checkpoints dir: {}", e))
        })?;

        let mut sequences = Vec::new();
        let prefix = format!("{}_checkpoint_", execution_id);

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| WorkflowError::persistence(format!("Failed to read entry: {}", e)))?
        {
            let path = entry.path();
            if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                if name.starts_with(&prefix) {
                    if let Some(seq_str) = name.strip_prefix(&prefix) {
                        if let Ok(seq) = seq_str.parse::<u64>() {
                            sequences.push(seq);
                        }
                    }
                }
            }
        }

        sequences.sort();
        Ok(sequences)
    }

    /// Check if a checkpoint exists for an execution.
    pub async fn checkpoint_exists(&self, execution_id: &str) -> bool {
        let latest_path = Path::new(&self.state_dir)
            .join("checkpoints")
            .join(format!("{}_latest.json", execution_id));
        latest_path.exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workflow_state_lifecycle() {
        let mut state = WorkflowState::new(
            "wf1".to_string(),
            "exec1".to_string(),
            "Test Workflow".to_string(),
        );

        assert_eq!(state.status, WorkflowStatus::Pending);

        state.start();
        assert_eq!(state.status, WorkflowStatus::Running);
        assert!(state.metadata.started_at.is_some());

        state.complete();
        assert_eq!(state.status, WorkflowStatus::Completed);
        assert!(state.metadata.completed_at.is_some());
        assert!(state.metadata.duration_ms.is_some());
    }

    #[test]
    fn test_task_state_lifecycle() {
        let mut state = WorkflowState::new(
            "wf1".to_string(),
            "exec1".to_string(),
            "Test Workflow".to_string(),
        );

        state.init_task("task1".to_string());
        assert_eq!(
            state.get_task_state("task1").map(|t| t.status),
            Some(TaskStatus::Pending)
        );

        state.start_task("task1").ok();
        assert_eq!(
            state.get_task_state("task1").map(|t| t.status),
            Some(TaskStatus::Running)
        );
        assert_eq!(state.get_task_state("task1").map(|t| t.attempts), Some(1));

        state
            .complete_task("task1", Some(serde_json::json!({"result": "success"})))
            .ok();
        assert_eq!(
            state.get_task_state("task1").map(|t| t.status),
            Some(TaskStatus::Completed)
        );
    }

    #[test]
    fn test_context_variables() {
        let mut state = WorkflowState::new(
            "wf1".to_string(),
            "exec1".to_string(),
            "Test Workflow".to_string(),
        );

        state.set_variable("key1".to_string(), serde_json::json!("value1"));
        assert_eq!(
            state.get_variable("key1"),
            Some(&serde_json::json!("value1"))
        );
    }
}
