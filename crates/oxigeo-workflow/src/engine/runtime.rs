//! Workflow runtime for managing multiple workflow executions.

use crate::dag::WorkflowDag;
use crate::engine::executor::{ExecutorConfig, TaskExecutor, WorkflowExecutor};
use crate::engine::state::{WorkflowState, WorkflowStatus};
use crate::error::{Result, WorkflowError};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tracing::{debug, info};

use uuid::Uuid;

/// Workflow definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    /// Workflow ID.
    pub id: String,
    /// Workflow name.
    pub name: String,
    /// Workflow version.
    pub version: String,
    /// Workflow DAG.
    pub dag: WorkflowDag,
    /// Workflow description.
    pub description: Option<String>,
}

/// Active workflow execution.
struct ActiveExecution {
    /// Execution handle.
    handle: JoinHandle<Result<WorkflowState>>,
    /// Workflow state.
    state: Arc<RwLock<WorkflowState>>,
}

/// Workflow runtime for managing workflow executions.
pub struct WorkflowRuntime<E: TaskExecutor> {
    /// Executor configuration.
    config: ExecutorConfig,
    /// Task executor.
    task_executor: Arc<E>,
    /// Registered workflow definitions.
    workflows: Arc<DashMap<String, WorkflowDefinition>>,
    /// Active executions.
    executions: Arc<DashMap<String, ActiveExecution>>,
}

impl<E: TaskExecutor + Clone + 'static> WorkflowRuntime<E> {
    /// Create a new workflow runtime.
    pub fn new(config: ExecutorConfig, task_executor: E) -> Self {
        Self {
            config,
            task_executor: Arc::new(task_executor),
            workflows: Arc::new(DashMap::new()),
            executions: Arc::new(DashMap::new()),
        }
    }

    /// Register a workflow definition.
    pub fn register_workflow(&self, definition: WorkflowDefinition) -> Result<()> {
        if self.workflows.contains_key(&definition.id) {
            return Err(WorkflowError::already_exists(format!(
                "Workflow '{}'",
                definition.id
            )));
        }

        // Validate the DAG
        definition.dag.validate()?;

        info!("Registering workflow: {}", definition.id);
        self.workflows.insert(definition.id.clone(), definition);

        Ok(())
    }

    /// Unregister a workflow definition.
    pub fn unregister_workflow(&self, workflow_id: &str) -> Result<()> {
        self.workflows
            .remove(workflow_id)
            .ok_or_else(|| WorkflowError::not_found(format!("Workflow '{}'", workflow_id)))?;

        info!("Unregistered workflow: {}", workflow_id);
        Ok(())
    }

    /// Get a workflow definition.
    pub fn get_workflow(&self, workflow_id: &str) -> Option<WorkflowDefinition> {
        self.workflows
            .get(workflow_id)
            .map(|entry| entry.value().clone())
    }

    /// List all registered workflows.
    pub fn list_workflows(&self) -> Vec<WorkflowDefinition> {
        self.workflows
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Start a workflow execution.
    pub fn start_workflow(&self, workflow_id: &str) -> Result<String> {
        let definition = self
            .get_workflow(workflow_id)
            .ok_or_else(|| WorkflowError::not_found(format!("Workflow '{}'", workflow_id)))?;

        let execution_id = Uuid::new_v4().to_string();

        info!(
            "Starting workflow execution: workflow_id={}, execution_id={}",
            workflow_id, execution_id
        );

        let executor = WorkflowExecutor::new(self.config.clone(), (*self.task_executor).clone());

        let wf_id = workflow_id.to_string();
        let exec_id = execution_id.clone();
        let dag = definition.dag.clone();

        // Create initial state
        let state = WorkflowState::new(wf_id.clone(), exec_id.clone(), definition.name.clone());
        let state_arc = Arc::new(RwLock::new(state));
        let state_arc_clone = Arc::clone(&state_arc);

        // Spawn execution task
        let handle = tokio::spawn(async move { executor.execute(wf_id, exec_id, dag).await });

        self.executions.insert(
            execution_id.clone(),
            ActiveExecution {
                handle,
                state: state_arc_clone,
            },
        );

        Ok(execution_id)
    }

    /// Get the status of a workflow execution.
    pub async fn get_execution_status(&self, execution_id: &str) -> Result<WorkflowStatus> {
        let execution = self
            .executions
            .get(execution_id)
            .ok_or_else(|| WorkflowError::not_found(format!("Execution '{}'", execution_id)))?;

        let state = execution.state.read().await;
        Ok(state.status)
    }

    /// Get the full state of a workflow execution.
    pub async fn get_execution_state(&self, execution_id: &str) -> Result<WorkflowState> {
        let execution = self
            .executions
            .get(execution_id)
            .ok_or_else(|| WorkflowError::not_found(format!("Execution '{}'", execution_id)))?;

        let state = execution.state.read().await;
        Ok(state.clone())
    }

    /// Cancel a workflow execution.
    pub async fn cancel_execution(&self, execution_id: &str) -> Result<()> {
        let (_, execution) = self
            .executions
            .remove(execution_id)
            .ok_or_else(|| WorkflowError::not_found(format!("Execution '{}'", execution_id)))?;

        debug!("Cancelling workflow execution: {}", execution_id);

        // Cancel the execution
        execution.handle.abort();

        // Update state
        let mut state = execution.state.write().await;
        state.cancel();

        info!("Cancelled workflow execution: {}", execution_id);

        Ok(())
    }

    /// Wait for a workflow execution to complete.
    pub async fn wait_for_completion(&self, execution_id: &str) -> Result<WorkflowState> {
        let (_, execution) = self
            .executions
            .remove(execution_id)
            .ok_or_else(|| WorkflowError::not_found(format!("Execution '{}'", execution_id)))?;

        debug!("Waiting for workflow execution: {}", execution_id);

        match execution.handle.await {
            Ok(result) => result,
            Err(e) => {
                if e.is_cancelled() {
                    let state = execution.state.read().await;
                    Ok(state.clone())
                } else {
                    Err(WorkflowError::execution(format!(
                        "Execution task panicked: {}",
                        e
                    )))
                }
            }
        }
    }

    /// List all active executions.
    pub fn list_active_executions(&self) -> Vec<String> {
        self.executions
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Get the number of active executions.
    pub fn active_execution_count(&self) -> usize {
        self.executions.len()
    }

    /// Clean up completed executions.
    pub async fn cleanup_completed(&self) -> usize {
        let mut completed = Vec::new();

        for entry in self.executions.iter() {
            let execution_id = entry.key().clone();
            let state = entry.value().state.read().await;

            if state.is_terminal() {
                completed.push(execution_id);
            }
        }

        let count = completed.len();

        for execution_id in completed {
            self.executions.remove(&execution_id);
        }

        if count > 0 {
            info!("Cleaned up {} completed executions", count);
        }

        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::graph::{ResourceRequirements, RetryPolicy, TaskNode};
    use crate::engine::executor::{ExecutionContext, TaskExecutor, TaskOutput};
    use async_trait::async_trait;
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

    impl Clone for DummyExecutor {
        fn clone(&self) -> Self {
            Self
        }
    }

    fn create_test_workflow() -> WorkflowDefinition {
        let mut dag = WorkflowDag::new();
        dag.add_task(TaskNode {
            id: "task1".to_string(),
            name: "Task 1".to_string(),
            description: None,
            config: serde_json::json!({}),
            retry: RetryPolicy::default(),
            timeout_secs: Some(60),
            resources: ResourceRequirements::default(),
            metadata: HashMap::new(),
        })
        .ok();

        WorkflowDefinition {
            id: "wf1".to_string(),
            name: "Test Workflow".to_string(),
            version: "1.0.0".to_string(),
            dag,
            description: Some("Test workflow".to_string()),
        }
    }

    #[tokio::test]
    async fn test_register_workflow() {
        let runtime = WorkflowRuntime::new(ExecutorConfig::default(), DummyExecutor);
        let workflow = create_test_workflow();

        let result = runtime.register_workflow(workflow);
        assert!(result.is_ok());

        assert!(runtime.get_workflow("wf1").is_some());
    }

    #[tokio::test]
    async fn test_start_workflow() {
        let runtime = WorkflowRuntime::new(ExecutorConfig::default(), DummyExecutor);
        let workflow = create_test_workflow();

        runtime.register_workflow(workflow).ok();

        let execution_id = runtime.start_workflow("wf1");
        assert!(execution_id.is_ok());
    }
}
