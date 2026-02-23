//! Workflow execution engine.

use crate::dag::{ResourcePool, TaskNode, WorkflowDag, create_execution_plan};
use crate::engine::state::{
    StatePersistence, TaskStatus, WorkflowCheckpoint, WorkflowState, WorkflowStatus,
};
use crate::error::{Result, WorkflowError};
use async_trait::async_trait;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::{RwLock, Semaphore};
use tokio::time::timeout;
use tracing::{debug, error, info, warn};

/// Task executor trait - implement this to define custom task execution logic.
#[async_trait]
pub trait TaskExecutor: Send + Sync {
    /// Execute a task.
    async fn execute(&self, task: &TaskNode, context: &ExecutionContext) -> Result<TaskOutput>;
}

/// Execution context provided to task executors.
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    /// Workflow execution ID.
    pub execution_id: String,
    /// Task ID.
    pub task_id: String,
    /// Shared workflow state.
    pub state: Arc<RwLock<WorkflowState>>,
    /// Input data from previous tasks.
    pub inputs: std::collections::HashMap<String, serde_json::Value>,
}

/// Task execution output.
#[derive(Debug, Clone)]
pub struct TaskOutput {
    /// Output data.
    pub data: Option<serde_json::Value>,
    /// Execution logs.
    pub logs: Vec<String>,
}

/// Workflow executor configuration.
#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    /// Maximum concurrent tasks.
    pub max_concurrent_tasks: usize,
    /// Enable state persistence.
    pub enable_persistence: bool,
    /// State directory.
    pub state_dir: String,
    /// Resource pool.
    pub resource_pool: ResourcePool,
    /// Retry on failure.
    pub retry_on_failure: bool,
    /// Stop on first failure.
    pub stop_on_failure: bool,
    /// Checkpoint interval (save checkpoint every N tasks).
    pub checkpoint_interval: usize,
    /// Enable checkpoint-based recovery.
    pub enable_checkpointing: bool,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            max_concurrent_tasks: 10,
            enable_persistence: true,
            state_dir: "/tmp/oxigdal-workflow".to_string(),
            resource_pool: ResourcePool::default(),
            retry_on_failure: true,
            stop_on_failure: false,
            checkpoint_interval: 1, // Save after each task by default
            enable_checkpointing: true,
        }
    }
}

/// Workflow executor.
pub struct WorkflowExecutor<E: TaskExecutor> {
    /// Configuration.
    config: ExecutorConfig,
    /// Task executor implementation.
    task_executor: Arc<E>,
    /// State persistence.
    persistence: Option<StatePersistence>,
    /// Semaphore for limiting concurrent tasks (reserved for future parallel execution).
    _semaphore: Arc<Semaphore>,
    /// Checkpoint sequence counter.
    checkpoint_sequence: AtomicU64,
    /// Tasks completed since last checkpoint.
    tasks_since_checkpoint: AtomicU64,
}

impl<E: TaskExecutor> WorkflowExecutor<E> {
    /// Create a new workflow executor.
    pub fn new(config: ExecutorConfig, task_executor: E) -> Self {
        let semaphore = Arc::new(Semaphore::new(config.max_concurrent_tasks));
        let persistence = if config.enable_persistence {
            Some(StatePersistence::new(config.state_dir.clone()))
        } else {
            None
        };

        Self {
            config,
            task_executor: Arc::new(task_executor),
            persistence,
            _semaphore: semaphore,
            checkpoint_sequence: AtomicU64::new(0),
            tasks_since_checkpoint: AtomicU64::new(0),
        }
    }

    /// Save a checkpoint if conditions are met.
    async fn maybe_save_checkpoint(&self, state: &WorkflowState, dag: &WorkflowDag) -> Result<()> {
        if !self.config.enable_checkpointing {
            return Ok(());
        }

        let persistence = match &self.persistence {
            Some(p) => p,
            None => return Ok(()),
        };

        let tasks_completed = self.tasks_since_checkpoint.fetch_add(1, Ordering::SeqCst) + 1;

        if tasks_completed >= self.config.checkpoint_interval as u64 {
            self.tasks_since_checkpoint.store(0, Ordering::SeqCst);
            let seq = self.checkpoint_sequence.fetch_add(1, Ordering::SeqCst);

            let checkpoint = WorkflowCheckpoint::new(state.clone(), dag.clone(), seq);
            persistence.save_checkpoint(&checkpoint).await?;

            debug!(
                "Saved checkpoint {} for execution {}",
                seq, state.execution_id
            );
        }

        Ok(())
    }

    /// Force save a checkpoint immediately.
    async fn save_checkpoint_now(&self, state: &WorkflowState, dag: &WorkflowDag) -> Result<()> {
        if !self.config.enable_checkpointing {
            return Ok(());
        }

        let persistence = match &self.persistence {
            Some(p) => p,
            None => return Ok(()),
        };

        self.tasks_since_checkpoint.store(0, Ordering::SeqCst);
        let seq = self.checkpoint_sequence.fetch_add(1, Ordering::SeqCst);

        let checkpoint = WorkflowCheckpoint::new(state.clone(), dag.clone(), seq);
        persistence.save_checkpoint(&checkpoint).await?;

        info!(
            "Saved checkpoint {} for execution {}",
            seq, state.execution_id
        );
        Ok(())
    }

    /// Execute a workflow.
    pub async fn execute(
        &self,
        workflow_id: String,
        execution_id: String,
        dag: WorkflowDag,
    ) -> Result<WorkflowState> {
        info!(
            "Starting workflow execution: workflow_id={}, execution_id={}",
            workflow_id, execution_id
        );

        // Validate DAG
        dag.validate()?;

        // Create initial workflow state
        let mut state = WorkflowState::new(workflow_id.clone(), execution_id.clone(), workflow_id);

        // Initialize task states
        for task in dag.tasks() {
            state.init_task(task.id.clone());
        }

        state.start();

        // Save initial state
        if let Some(ref persistence) = self.persistence {
            persistence.save(&state).await?;
        }

        // Save initial checkpoint with DAG
        self.save_checkpoint_now(&state, &dag).await?;

        let state_arc = Arc::new(RwLock::new(state));

        // Create execution plan
        let execution_plan = create_execution_plan(&dag)?;

        info!(
            "Execution plan created with {} levels",
            execution_plan.len()
        );

        // Execute tasks level by level
        for (level_idx, level) in execution_plan.iter().enumerate() {
            info!("Executing level {} with {} tasks", level_idx, level.len());

            let results = self.execute_level(&dag, &state_arc, level).await;

            // Save checkpoint after each level
            {
                let state_guard = state_arc.read().await;
                self.maybe_save_checkpoint(&state_guard, &dag).await?;
            }

            // Check for failures
            let failed_tasks: Vec<_> = results
                .iter()
                .filter_map(|(task_id, result)| {
                    if result.is_err() {
                        Some(task_id.clone())
                    } else {
                        None
                    }
                })
                .collect();

            if !failed_tasks.is_empty() {
                error!("Tasks failed: {:?}", failed_tasks);

                if self.config.stop_on_failure {
                    warn!("Stopping workflow execution due to failures");
                    let mut state_guard = state_arc.write().await;
                    state_guard.fail();

                    if let Some(ref persistence) = self.persistence {
                        persistence.save(&state_guard).await?;
                    }

                    // Save final checkpoint on failure
                    self.save_checkpoint_now(&state_guard, &dag).await?;

                    drop(state_guard);

                    return Ok(Arc::try_unwrap(state_arc)
                        .map(|rw| rw.into_inner())
                        .unwrap_or_else(|arc| {
                            tokio::task::block_in_place(|| arc.blocking_read().clone())
                        }));
                }
            }
        }

        // Complete workflow
        let mut state_guard = state_arc.write().await;

        // Check if all tasks completed successfully
        let all_completed = state_guard
            .task_states
            .values()
            .all(|ts| ts.status == TaskStatus::Completed || ts.status == TaskStatus::Skipped);

        if all_completed {
            state_guard.complete();
        } else {
            state_guard.fail();
        }

        // Save final state
        if let Some(ref persistence) = self.persistence {
            persistence.save(&state_guard).await?;
        }

        // Save final checkpoint
        self.save_checkpoint_now(&state_guard, &dag).await?;

        info!(
            "Workflow execution completed: status={:?}",
            state_guard.status
        );

        drop(state_guard);

        Ok(Arc::try_unwrap(state_arc)
            .map(|rw| rw.into_inner())
            .unwrap_or_else(|arc| tokio::task::block_in_place(|| arc.blocking_read().clone())))
    }

    /// Execute a level of tasks in parallel.
    async fn execute_level(
        &self,
        dag: &WorkflowDag,
        state: &Arc<RwLock<WorkflowState>>,
        level: &[String],
    ) -> Vec<(String, Result<()>)> {
        let mut results = Vec::new();

        for task_id in level {
            let result = self
                .execute_task(
                    task_id,
                    dag,
                    state,
                    &*self.task_executor,
                    self.config.retry_on_failure,
                )
                .await;
            results.push((task_id.clone(), result));
        }

        results
    }

    /// Execute a single task.
    async fn execute_task(
        &self,
        task_id: &str,
        dag: &WorkflowDag,
        state: &Arc<RwLock<WorkflowState>>,
        executor: &E,
        retry_on_failure: bool,
    ) -> Result<()> {
        let task = dag
            .get_task(task_id)
            .ok_or_else(|| WorkflowError::not_found(format!("Task '{}'", task_id)))?;

        debug!("Executing task: {}", task_id);

        // Check dependencies
        if !self.check_dependencies(task_id, dag, state).await? {
            warn!("Skipping task {} due to failed dependencies", task_id);
            let mut state_guard = state.write().await;
            state_guard.skip_task(task_id)?;
            return Ok(());
        }

        // Mark task as started
        {
            let mut state_guard = state.write().await;
            state_guard.start_task(task_id)?;
        }

        // Execute with retry
        let max_attempts = if retry_on_failure {
            task.retry.max_attempts
        } else {
            1
        };

        let mut last_error = None;

        for attempt in 0..max_attempts {
            if attempt > 0 {
                debug!("Retrying task {} (attempt {})", task_id, attempt + 1);

                let delay_ms =
                    task.retry.delay_ms as f64 * task.retry.backoff_multiplier.powi(attempt as i32);
                let delay_ms = delay_ms.min(task.retry.max_delay_ms as f64) as u64;

                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }

            // Gather inputs from dependencies
            let inputs = self.gather_inputs(task_id, dag, state).await?;

            // Create execution context
            let ctx = ExecutionContext {
                execution_id: {
                    let state_guard = state.read().await;
                    state_guard.execution_id.clone()
                },
                task_id: task_id.to_string(),
                state: Arc::clone(state),
                inputs,
            };

            // Execute task with timeout
            let task_timeout = Duration::from_secs(task.timeout_secs.unwrap_or(300));
            let execute_result = timeout(task_timeout, executor.execute(task, &ctx)).await;

            match execute_result {
                Ok(Ok(output)) => {
                    // Task succeeded
                    let mut state_guard = state.write().await;
                    state_guard.complete_task(task_id, output.data)?;

                    for log in output.logs {
                        state_guard.add_task_log(task_id, log)?;
                    }

                    info!("Task {} completed successfully", task_id);
                    return Ok(());
                }
                Ok(Err(e)) => {
                    warn!("Task {} failed: {}", task_id, e);
                    last_error = Some(e);
                }
                Err(_) => {
                    let timeout_error =
                        WorkflowError::task_timeout(task_id, task_timeout.as_secs());
                    warn!("Task {} timed out", task_id);
                    last_error = Some(timeout_error);
                }
            }
        }

        // All attempts failed
        let error = last_error.unwrap_or_else(|| WorkflowError::execution("Unknown error"));
        let mut state_guard = state.write().await;
        state_guard.fail_task(task_id, error.to_string())?;

        error!("Task {} failed after {} attempts", task_id, max_attempts);
        Err(error)
    }

    /// Check if all task dependencies are met.
    async fn check_dependencies(
        &self,
        task_id: &str,
        dag: &WorkflowDag,
        state: &Arc<RwLock<WorkflowState>>,
    ) -> Result<bool> {
        let dependencies = dag.get_dependencies(task_id);
        let state_guard = state.read().await;

        for dep_id in dependencies {
            if let Some(dep_state) = state_guard.get_task_state(&dep_id) {
                if dep_state.status != TaskStatus::Completed {
                    return Ok(false);
                }
            } else {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Gather inputs from task dependencies.
    async fn gather_inputs(
        &self,
        task_id: &str,
        dag: &WorkflowDag,
        state: &Arc<RwLock<WorkflowState>>,
    ) -> Result<std::collections::HashMap<String, serde_json::Value>> {
        let dependencies = dag.get_dependencies(task_id);
        let state_guard = state.read().await;
        let mut inputs = std::collections::HashMap::new();

        for dep_id in dependencies {
            if let Some(dep_state) = state_guard.get_task_state(&dep_id) {
                if let Some(ref output) = dep_state.output {
                    inputs.insert(dep_id.clone(), output.clone());
                }
            }
        }

        Ok(inputs)
    }

    /// Resume a workflow from a saved checkpoint.
    ///
    /// This method reconstructs the DAG from the checkpoint and continues
    /// execution from where it left off, handling:
    /// - Completed tasks (skipped)
    /// - Interrupted tasks (reset to pending for retry)
    /// - Failed tasks (depending on configuration)
    /// - Pending tasks (executed normally)
    pub async fn resume(&self, execution_id: String) -> Result<WorkflowState> {
        let persistence = self
            .persistence
            .as_ref()
            .ok_or_else(|| WorkflowError::state("Persistence is not enabled"))?;

        // Try to load checkpoint first (includes DAG)
        let mut checkpoint = persistence.load_checkpoint(&execution_id).await.map_err(|e| {
            WorkflowError::state(format!(
                "Failed to load checkpoint for recovery: {}. Ensure checkpointing was enabled during execution.",
                e
            ))
        })?;

        if checkpoint.state.is_terminal() {
            return Err(WorkflowError::state("Cannot resume a terminal workflow"));
        }

        info!(
            "Resuming workflow execution: execution_id={}, checkpoint_sequence={}",
            execution_id, checkpoint.sequence
        );

        // Prepare checkpoint for resumption (reset interrupted tasks)
        checkpoint.prepare_for_resume()?;

        // Update checkpoint sequence for this executor
        self.checkpoint_sequence
            .store(checkpoint.sequence + 1, Ordering::SeqCst);

        // Resume execution with recovered DAG and state
        self.resume_from_checkpoint(checkpoint).await
    }

    /// Resume execution from a checkpoint.
    async fn resume_from_checkpoint(
        &self,
        checkpoint: WorkflowCheckpoint,
    ) -> Result<WorkflowState> {
        let dag = checkpoint.dag.clone();

        // Log recovery information (before moving state)
        let completed = checkpoint.get_completed_tasks();
        let pending = checkpoint.get_pending_tasks();
        let interrupted = checkpoint.get_interrupted_tasks();
        let failed = checkpoint.get_failed_tasks();

        let mut state = checkpoint.state;

        // Ensure workflow is in running state
        if state.status != WorkflowStatus::Running {
            state.status = WorkflowStatus::Running;
        }

        info!(
            "Recovery state: {} completed, {} pending, {} interrupted, {} failed",
            completed.len(),
            pending.len(),
            interrupted.len(),
            failed.len()
        );

        // Save state update
        if let Some(ref persistence) = self.persistence {
            persistence.save(&state).await?;
        }

        let state_arc = Arc::new(RwLock::new(state));

        // Create execution plan from DAG
        let execution_plan = create_execution_plan(&dag)?;

        info!("Resuming execution with {} levels", execution_plan.len());

        // Execute tasks level by level, skipping completed ones
        for (level_idx, level) in execution_plan.iter().enumerate() {
            // Filter out already completed or skipped tasks
            let tasks_to_execute: Vec<String> = {
                let state_guard = state_arc.read().await;
                level
                    .iter()
                    .filter(|task_id| {
                        state_guard
                            .get_task_state(task_id)
                            .map(|ts| {
                                !matches!(ts.status, TaskStatus::Completed | TaskStatus::Skipped)
                            })
                            .unwrap_or(true)
                    })
                    .cloned()
                    .collect()
            };

            if tasks_to_execute.is_empty() {
                debug!("Level {} has no tasks to execute, skipping", level_idx);
                continue;
            }

            info!(
                "Resuming level {} with {} tasks (skipping {} completed)",
                level_idx,
                tasks_to_execute.len(),
                level.len() - tasks_to_execute.len()
            );

            let results = self
                .execute_level(&dag, &state_arc, &tasks_to_execute)
                .await;

            // Save checkpoint after each level
            {
                let state_guard = state_arc.read().await;
                self.maybe_save_checkpoint(&state_guard, &dag).await?;
            }

            // Check for failures
            let failed_tasks: Vec<_> = results
                .iter()
                .filter_map(|(task_id, result)| {
                    if result.is_err() {
                        Some(task_id.clone())
                    } else {
                        None
                    }
                })
                .collect();

            if !failed_tasks.is_empty() {
                error!("Tasks failed during resume: {:?}", failed_tasks);

                if self.config.stop_on_failure {
                    warn!("Stopping resumed workflow execution due to failures");
                    let mut state_guard = state_arc.write().await;
                    state_guard.fail();

                    if let Some(ref persistence) = self.persistence {
                        persistence.save(&state_guard).await?;
                    }

                    // Save final checkpoint on failure
                    self.save_checkpoint_now(&state_guard, &dag).await?;

                    drop(state_guard);

                    return Ok(Arc::try_unwrap(state_arc)
                        .map(|rw| rw.into_inner())
                        .unwrap_or_else(|arc| {
                            tokio::task::block_in_place(|| arc.blocking_read().clone())
                        }));
                }
            }
        }

        // Complete workflow
        let mut state_guard = state_arc.write().await;

        // Check if all tasks completed successfully
        let all_completed = state_guard
            .task_states
            .values()
            .all(|ts| ts.status == TaskStatus::Completed || ts.status == TaskStatus::Skipped);

        if all_completed {
            state_guard.complete();
        } else {
            state_guard.fail();
        }

        // Save final state
        if let Some(ref persistence) = self.persistence {
            persistence.save(&state_guard).await?;
        }

        // Save final checkpoint
        self.save_checkpoint_now(&state_guard, &dag).await?;

        info!(
            "Resumed workflow execution completed: status={:?}",
            state_guard.status
        );

        drop(state_guard);

        Ok(Arc::try_unwrap(state_arc)
            .map(|rw| rw.into_inner())
            .unwrap_or_else(|arc| tokio::task::block_in_place(|| arc.blocking_read().clone())))
    }

    /// Resume a workflow from a specific checkpoint sequence.
    pub async fn resume_from_sequence(
        &self,
        execution_id: String,
        sequence: u64,
    ) -> Result<WorkflowState> {
        let persistence = self
            .persistence
            .as_ref()
            .ok_or_else(|| WorkflowError::state("Persistence is not enabled"))?;

        let mut checkpoint = persistence
            .load_checkpoint_by_sequence(&execution_id, sequence)
            .await?;

        if checkpoint.state.is_terminal() {
            return Err(WorkflowError::state("Cannot resume a terminal workflow"));
        }

        info!(
            "Resuming workflow from specific checkpoint: execution_id={}, sequence={}",
            execution_id, sequence
        );

        // Prepare checkpoint for resumption
        checkpoint.prepare_for_resume()?;

        // Update checkpoint sequence
        self.checkpoint_sequence
            .store(sequence + 1, Ordering::SeqCst);

        self.resume_from_checkpoint(checkpoint).await
    }

    /// Get recovery information for an execution.
    pub async fn get_recovery_info(&self, execution_id: &str) -> Result<RecoveryInfo> {
        let persistence = self
            .persistence
            .as_ref()
            .ok_or_else(|| WorkflowError::state("Persistence is not enabled"))?;

        let checkpoint = persistence.load_checkpoint(execution_id).await?;

        Ok(RecoveryInfo {
            execution_id: execution_id.to_string(),
            checkpoint_sequence: checkpoint.sequence,
            checkpoint_created_at: checkpoint.created_at,
            workflow_status: checkpoint.state.status,
            completed_tasks: checkpoint.get_completed_tasks(),
            pending_tasks: checkpoint.get_pending_tasks(),
            interrupted_tasks: checkpoint.get_interrupted_tasks(),
            failed_tasks: checkpoint.get_failed_tasks(),
            skipped_tasks: checkpoint.get_skipped_tasks(),
            can_resume: !checkpoint.state.is_terminal(),
        })
    }

    /// List available checkpoints for an execution.
    pub async fn list_checkpoints(&self, execution_id: &str) -> Result<Vec<u64>> {
        let persistence = self
            .persistence
            .as_ref()
            .ok_or_else(|| WorkflowError::state("Persistence is not enabled"))?;

        persistence.list_checkpoints(execution_id).await
    }

    /// Clean up old checkpoints, keeping only the latest N.
    pub async fn cleanup_checkpoints(
        &self,
        execution_id: &str,
        keep_count: usize,
    ) -> Result<usize> {
        let persistence = self
            .persistence
            .as_ref()
            .ok_or_else(|| WorkflowError::state("Persistence is not enabled"))?;

        let checkpoints = persistence.list_checkpoints(execution_id).await?;

        if checkpoints.len() <= keep_count {
            return Ok(0);
        }

        let to_delete = checkpoints.len() - keep_count;
        let mut deleted = 0;

        for seq in checkpoints.iter().take(to_delete) {
            if persistence
                .delete_checkpoint(execution_id, *seq)
                .await
                .is_ok()
            {
                deleted += 1;
            }
        }

        Ok(deleted)
    }
}

/// Information about workflow recovery state.
#[derive(Debug, Clone)]
pub struct RecoveryInfo {
    /// Execution ID.
    pub execution_id: String,
    /// Latest checkpoint sequence number.
    pub checkpoint_sequence: u64,
    /// When the checkpoint was created.
    pub checkpoint_created_at: chrono::DateTime<chrono::Utc>,
    /// Current workflow status.
    pub workflow_status: WorkflowStatus,
    /// Tasks that completed successfully.
    pub completed_tasks: Vec<String>,
    /// Tasks that are pending execution.
    pub pending_tasks: Vec<String>,
    /// Tasks that were interrupted (running when checkpoint saved).
    pub interrupted_tasks: Vec<String>,
    /// Tasks that failed.
    pub failed_tasks: Vec<String>,
    /// Tasks that were skipped.
    pub skipped_tasks: Vec<String>,
    /// Whether the workflow can be resumed.
    pub can_resume: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::graph::{ResourceRequirements, RetryPolicy};
    use crate::engine::state::WorkflowStatus;
    use std::collections::HashMap;

    struct DummyExecutor;

    #[async_trait]
    impl TaskExecutor for DummyExecutor {
        async fn execute(
            &self,
            _task: &TaskNode,
            _context: &ExecutionContext,
        ) -> Result<TaskOutput> {
            Ok(TaskOutput {
                data: Some(serde_json::json!({"result": "success"})),
                logs: vec!["Task executed".to_string()],
            })
        }
    }

    fn create_test_task(id: &str) -> TaskNode {
        TaskNode {
            id: id.to_string(),
            name: id.to_string(),
            description: None,
            config: serde_json::json!({}),
            retry: RetryPolicy::default(),
            timeout_secs: Some(60),
            resources: ResourceRequirements::default(),
            metadata: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn test_simple_workflow() {
        let mut dag = WorkflowDag::new();
        dag.add_task(create_test_task("task1")).ok();

        let executor = WorkflowExecutor::new(ExecutorConfig::default(), DummyExecutor);

        let result = executor
            .execute("wf1".to_string(), "exec1".to_string(), dag)
            .await;

        assert!(result.is_ok());
        let state = result.expect("Expected workflow state");
        assert_eq!(state.status, WorkflowStatus::Completed);
    }
}
