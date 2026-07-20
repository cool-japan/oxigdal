//! Workflow orchestration engine for complex distributed tasks.
//!
//! This module provides workflow capabilities including:
//! - Workflow templates and definitions
//! - Conditional execution (if/else branching)
//! - Loops and iteration
//! - Workflow versioning
//! - Workflow resumption after failure
//! - Workflow monitoring and visualization

use crate::error::{ClusterError, Result};
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// Workflow identifier.
pub type WorkflowId = uuid::Uuid;

/// Workflow definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    /// Workflow ID
    pub id: WorkflowId,
    /// Workflow name
    pub name: String,
    /// Workflow version
    pub version: String,
    /// Workflow description
    pub description: Option<String>,
    /// Workflow steps
    pub steps: Vec<WorkflowStep>,
    /// Workflow variables
    pub variables: HashMap<String, serde_json::Value>,
    /// Created at
    pub created_at: SystemTime,
    /// Updated at
    pub updated_at: SystemTime,
}

/// Workflow step definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    /// Step ID
    pub id: String,
    /// Step name
    pub name: String,
    /// Step type
    pub step_type: StepType,
    /// Dependencies (step IDs that must complete before this step)
    pub depends_on: Vec<String>,
    /// Retry configuration
    pub retry: Option<RetryConfig>,
    /// Timeout
    pub timeout: Option<Duration>,
}

/// Workflow step type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StepType {
    /// Execute a task
    Task {
        /// Task template name to execute
        task_template: String,
        /// Parameters for the task
        parameters: HashMap<String, serde_json::Value>,
    },
    /// Conditional execution
    Condition {
        /// Condition expression to evaluate
        condition: String,
        /// Steps to execute if condition is true
        then_steps: Vec<String>,
        /// Steps to execute if condition is false
        else_steps: Option<Vec<String>>,
    },
    /// Loop iteration
    Loop {
        /// Variable name for loop iterator
        iterator: String,
        /// Items to iterate over
        items: Vec<serde_json::Value>,
        /// Steps to execute in each iteration
        body_steps: Vec<String>,
    },
    /// Parallel execution
    Parallel {
        /// Branches to execute in parallel
        branches: Vec<Vec<String>>,
    },
    /// Wait for duration
    Wait {
        /// Duration to wait
        duration: Duration,
    },
    /// Checkpoint for workflow state persistence
    Checkpoint {
        /// Checkpoint name
        name: String,
    },
}

/// Retry configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// Maximum retry attempts
    pub max_attempts: u32,
    /// Backoff strategy
    pub backoff: BackoffStrategy,
    /// Retry on specific errors
    pub retry_on: Vec<String>,
}

/// Backoff strategy for retries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BackoffStrategy {
    /// Fixed delay between retries
    Fixed {
        /// Delay duration between retries
        delay: Duration,
    },
    /// Exponential backoff with multiplier
    Exponential {
        /// Initial delay duration
        initial: Duration,
        /// Multiplier for each subsequent retry
        multiplier: f64,
        /// Maximum delay cap
        max: Duration,
    },
    /// Linear backoff with constant increment
    Linear {
        /// Initial delay duration
        initial: Duration,
        /// Increment added for each retry
        increment: Duration,
        /// Maximum delay cap
        max: Duration,
    },
}

/// Workflow execution instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowExecution {
    /// Execution ID
    pub id: uuid::Uuid,
    /// Workflow ID
    pub workflow_id: WorkflowId,
    /// Execution status
    pub status: WorkflowStatus,
    /// Current step
    pub current_step: Option<String>,
    /// Completed steps
    pub completed_steps: Vec<String>,
    /// Failed steps
    pub failed_steps: Vec<String>,
    /// Execution context (variables)
    pub context: HashMap<String, serde_json::Value>,
    /// Started at
    pub started_at: SystemTime,
    /// Completed at
    pub completed_at: Option<SystemTime>,
    /// Execution history
    pub history: Vec<ExecutionEvent>,
}

/// Workflow execution status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkflowStatus {
    /// Pending execution
    Pending,
    /// Currently running
    Running,
    /// Paused (can be resumed)
    Paused,
    /// Completed successfully
    Completed,
    /// Failed
    Failed,
    /// Cancelled
    Cancelled,
}

/// Execution event for history tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionEvent {
    /// Event type
    pub event_type: EventType,
    /// Step ID
    pub step_id: Option<String>,
    /// Timestamp
    pub timestamp: SystemTime,
    /// Event message
    pub message: String,
}

/// Event type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventType {
    /// Workflow execution started
    WorkflowStarted,
    /// Step execution started
    StepStarted,
    /// Step completed successfully
    StepCompleted,
    /// Step failed
    StepFailed,
    /// Step is being retried
    StepRetrying,
    /// Workflow completed successfully
    WorkflowCompleted,
    /// Workflow failed
    WorkflowFailed,
    /// Workflow paused
    WorkflowPaused,
    /// Workflow resumed from pause
    WorkflowResumed,
    /// Workflow cancelled
    WorkflowCancelled,
}

/// Workflow engine for orchestrating executions.
pub struct WorkflowEngine {
    /// Workflow definitions
    workflows: Arc<DashMap<WorkflowId, Workflow>>,
    /// Active executions
    executions: Arc<DashMap<uuid::Uuid, RwLock<WorkflowExecution>>>,
    /// Workflow templates
    templates: Arc<DashMap<String, WorkflowTemplate>>,
    /// Execution queue
    queue: Arc<RwLock<VecDeque<uuid::Uuid>>>,
    /// Statistics
    stats: Arc<RwLock<WorkflowStats>>,
}

/// Workflow template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowTemplate {
    /// Template name
    pub name: String,
    /// Template version
    pub version: String,
    /// Parameters that can be customized
    pub parameters: Vec<TemplateParameter>,
    /// Steps in the workflow
    pub steps: Vec<WorkflowStep>,
}

/// Template parameter definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateParameter {
    /// Parameter name
    pub name: String,
    /// Parameter type (string, number, etc.)
    pub param_type: String,
    /// Whether the parameter is required
    pub required: bool,
    /// Default value if not provided
    pub default: Option<serde_json::Value>,
    /// Human-readable description
    pub description: Option<String>,
}

/// Workflow statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowStats {
    /// Total number of registered workflows
    pub total_workflows: usize,
    /// Total number of executions started
    pub total_executions: u64,
    /// Number of currently running executions
    pub running_executions: usize,
    /// Number of completed executions
    pub completed_executions: u64,
    /// Number of failed executions
    pub failed_executions: u64,
    /// Average time to complete an execution
    pub average_execution_time: Duration,
}

impl WorkflowEngine {
    /// Create a new workflow engine.
    pub fn new() -> Self {
        Self {
            workflows: Arc::new(DashMap::new()),
            executions: Arc::new(DashMap::new()),
            templates: Arc::new(DashMap::new()),
            queue: Arc::new(RwLock::new(VecDeque::new())),
            stats: Arc::new(RwLock::new(WorkflowStats::default())),
        }
    }

    /// Register a workflow definition.
    pub fn register_workflow(&self, workflow: Workflow) -> Result<WorkflowId> {
        let id = workflow.id;
        self.workflows.insert(id, workflow);

        let mut stats = self.stats.write();
        stats.total_workflows = self.workflows.len();

        Ok(id)
    }

    /// Register a workflow template.
    pub fn register_template(&self, template: WorkflowTemplate) -> Result<()> {
        self.templates.insert(template.name.clone(), template);
        Ok(())
    }

    /// Create workflow from template.
    pub fn create_from_template(
        &self,
        template_name: &str,
        parameters: HashMap<String, serde_json::Value>,
    ) -> Result<Workflow> {
        let template = self
            .templates
            .get(template_name)
            .ok_or_else(|| ClusterError::WorkflowNotFound(template_name.to_string()))?;

        // Validate required parameters
        for param in &template.parameters {
            if param.required && !parameters.contains_key(&param.name) {
                return Err(ClusterError::InvalidConfiguration(format!(
                    "Missing required parameter: {}",
                    param.name
                )));
            }
        }

        let workflow = Workflow {
            id: uuid::Uuid::new_v4(),
            name: template.name.clone(),
            version: template.version.clone(),
            description: None,
            steps: template.steps.clone(),
            variables: parameters,
            created_at: SystemTime::now(),
            updated_at: SystemTime::now(),
        };

        Ok(workflow)
    }

    /// Start a workflow execution.
    pub fn start_execution(&self, workflow_id: WorkflowId) -> Result<uuid::Uuid> {
        let workflow = self
            .workflows
            .get(&workflow_id)
            .ok_or_else(|| ClusterError::WorkflowNotFound(workflow_id.to_string()))?;

        let execution_id = uuid::Uuid::new_v4();
        let execution = WorkflowExecution {
            id: execution_id,
            workflow_id,
            status: WorkflowStatus::Running,
            current_step: None,
            completed_steps: Vec::new(),
            failed_steps: Vec::new(),
            context: workflow.variables.clone(),
            started_at: SystemTime::now(),
            completed_at: None,
            history: vec![ExecutionEvent {
                event_type: EventType::WorkflowStarted,
                step_id: None,
                timestamp: SystemTime::now(),
                message: format!("Started workflow execution: {}", workflow.name),
            }],
        };

        self.executions.insert(execution_id, RwLock::new(execution));
        self.queue.write().push_back(execution_id);

        let mut stats = self.stats.write();
        stats.total_executions += 1;
        stats.running_executions = self.count_running_executions();

        Ok(execution_id)
    }

    /// Pause a workflow execution.
    pub fn pause_execution(&self, execution_id: uuid::Uuid) -> Result<()> {
        let execution = self
            .executions
            .get(&execution_id)
            .ok_or_else(|| ClusterError::WorkflowNotFound(execution_id.to_string()))?;

        let mut exec = execution.write();
        if exec.status != WorkflowStatus::Running {
            return Err(ClusterError::InvalidOperation(format!(
                "Cannot pause workflow in status {:?}",
                exec.status
            )));
        }

        exec.status = WorkflowStatus::Paused;
        let current_step = exec.current_step.clone();
        exec.history.push(ExecutionEvent {
            event_type: EventType::WorkflowPaused,
            step_id: current_step,
            timestamp: SystemTime::now(),
            message: "Workflow paused".to_string(),
        });

        Ok(())
    }

    /// Resume a paused workflow execution.
    pub fn resume_execution(&self, execution_id: uuid::Uuid) -> Result<()> {
        let execution = self
            .executions
            .get(&execution_id)
            .ok_or_else(|| ClusterError::WorkflowNotFound(execution_id.to_string()))?;

        let mut exec = execution.write();
        if exec.status != WorkflowStatus::Paused {
            return Err(ClusterError::InvalidOperation(format!(
                "Cannot resume workflow in status {:?}",
                exec.status
            )));
        }

        exec.status = WorkflowStatus::Running;
        let current_step = exec.current_step.clone();
        exec.history.push(ExecutionEvent {
            event_type: EventType::WorkflowResumed,
            step_id: current_step,
            timestamp: SystemTime::now(),
            message: "Workflow resumed".to_string(),
        });

        self.queue.write().push_back(execution_id);

        Ok(())
    }

    /// Cancel a workflow execution.
    pub fn cancel_execution(&self, execution_id: uuid::Uuid) -> Result<()> {
        let execution = self
            .executions
            .get(&execution_id)
            .ok_or_else(|| ClusterError::WorkflowNotFound(execution_id.to_string()))?;

        let mut exec = execution.write();
        exec.status = WorkflowStatus::Cancelled;
        exec.completed_at = Some(SystemTime::now());
        let current_step = exec.current_step.clone();
        exec.history.push(ExecutionEvent {
            event_type: EventType::WorkflowCancelled,
            step_id: current_step,
            timestamp: SystemTime::now(),
            message: "Workflow cancelled".to_string(),
        });

        let mut stats = self.stats.write();
        stats.running_executions = self.count_running_executions();

        Ok(())
    }

    /// Complete a workflow step.
    pub fn complete_step(&self, execution_id: uuid::Uuid, step_id: String) -> Result<()> {
        let execution = self
            .executions
            .get(&execution_id)
            .ok_or_else(|| ClusterError::WorkflowNotFound(execution_id.to_string()))?;

        let mut exec = execution.write();
        exec.completed_steps.push(step_id.clone());
        exec.history.push(ExecutionEvent {
            event_type: EventType::StepCompleted,
            step_id: Some(step_id),
            timestamp: SystemTime::now(),
            message: "Step completed successfully".to_string(),
        });

        Ok(())
    }

    /// Fail a workflow step.
    pub fn fail_step(
        &self,
        execution_id: uuid::Uuid,
        step_id: String,
        error: String,
    ) -> Result<()> {
        let execution = self
            .executions
            .get(&execution_id)
            .ok_or_else(|| ClusterError::WorkflowNotFound(execution_id.to_string()))?;

        let mut exec = execution.write();
        exec.failed_steps.push(step_id.clone());
        exec.history.push(ExecutionEvent {
            event_type: EventType::StepFailed,
            step_id: Some(step_id),
            timestamp: SystemTime::now(),
            message: error,
        });

        Ok(())
    }

    /// Get workflow execution status.
    pub fn get_execution(&self, execution_id: uuid::Uuid) -> Option<WorkflowExecution> {
        self.executions.get(&execution_id).map(|e| e.read().clone())
    }

    /// List all executions for a workflow.
    pub fn list_executions(&self, workflow_id: WorkflowId) -> Vec<WorkflowExecution> {
        self.executions
            .iter()
            .filter(|entry| entry.value().read().workflow_id == workflow_id)
            .map(|entry| entry.value().read().clone())
            .collect()
    }

    /// List running executions.
    pub fn list_running_executions(&self) -> Vec<WorkflowExecution> {
        self.executions
            .iter()
            .filter(|entry| entry.value().read().status == WorkflowStatus::Running)
            .map(|entry| entry.value().read().clone())
            .collect()
    }

    fn count_running_executions(&self) -> usize {
        self.executions
            .iter()
            .filter(|entry| entry.value().read().status == WorkflowStatus::Running)
            .count()
    }

    /// Get workflow statistics.
    pub fn get_stats(&self) -> WorkflowStats {
        self.stats.read().clone()
    }
}

impl Default for WorkflowEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_workflow_creation() {
        let workflow = Workflow {
            id: uuid::Uuid::new_v4(),
            name: "test-workflow".to_string(),
            version: "1.0.0".to_string(),
            description: Some("Test workflow".to_string()),
            steps: vec![],
            variables: HashMap::new(),
            created_at: SystemTime::now(),
            updated_at: SystemTime::now(),
        };

        let engine = WorkflowEngine::new();
        let result = engine.register_workflow(workflow.clone());

        assert!(result.is_ok());
        assert_eq!(
            result.expect("workflow registration should succeed"),
            workflow.id
        );
    }

    #[test]
    fn test_workflow_execution() {
        let workflow = Workflow {
            id: uuid::Uuid::new_v4(),
            name: "test-workflow".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            steps: vec![],
            variables: HashMap::new(),
            created_at: SystemTime::now(),
            updated_at: SystemTime::now(),
        };

        let engine = WorkflowEngine::new();
        let workflow_id = engine
            .register_workflow(workflow)
            .expect("workflow registration should succeed");

        let execution_id = engine
            .start_execution(workflow_id)
            .expect("workflow execution should start");
        let execution = engine.get_execution(execution_id);

        assert!(execution.is_some());
        let execution = execution.expect("execution should exist");
        assert_eq!(execution.status, WorkflowStatus::Running);
    }

    #[test]
    fn test_workflow_pause_resume() {
        let workflow = Workflow {
            id: uuid::Uuid::new_v4(),
            name: "test-workflow".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            steps: vec![],
            variables: HashMap::new(),
            created_at: SystemTime::now(),
            updated_at: SystemTime::now(),
        };

        let engine = WorkflowEngine::new();
        let workflow_id = engine
            .register_workflow(workflow)
            .expect("workflow registration should succeed");
        let execution_id = engine
            .start_execution(workflow_id)
            .expect("workflow execution should start");

        // Pause
        engine.pause_execution(execution_id).ok();
        let execution = engine
            .get_execution(execution_id)
            .expect("execution should exist after pause");
        assert_eq!(execution.status, WorkflowStatus::Paused);

        // Resume
        engine.resume_execution(execution_id).ok();
        let execution = engine
            .get_execution(execution_id)
            .expect("execution should exist after resume");
        assert_eq!(execution.status, WorkflowStatus::Running);
    }

    #[test]
    fn test_template_creation() {
        let template = WorkflowTemplate {
            name: "test-template".to_string(),
            version: "1.0.0".to_string(),
            parameters: vec![TemplateParameter {
                name: "input".to_string(),
                param_type: "string".to_string(),
                required: true,
                default: None,
                description: Some("Input parameter".to_string()),
            }],
            steps: vec![],
        };

        let engine = WorkflowEngine::new();
        engine.register_template(template.clone()).ok();

        let mut params = HashMap::new();
        params.insert("input".to_string(), serde_json::json!("test-value"));

        let workflow = engine.create_from_template("test-template", params);
        assert!(workflow.is_ok());
    }
}
