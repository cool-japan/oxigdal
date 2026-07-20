//! Workflow debugging utilities.

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Debug information for a workflow execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugInfo {
    /// Workflow ID.
    pub workflow_id: String,
    /// Execution ID.
    pub execution_id: String,
    /// Debug timestamp.
    pub timestamp: DateTime<Utc>,
    /// Current execution state.
    pub state: DebugState,
    /// Task states.
    pub task_states: HashMap<String, TaskDebugState>,
    /// Breakpoints.
    pub breakpoints: Vec<Breakpoint>,
    /// Variables snapshot.
    pub variables: HashMap<String, serde_json::Value>,
    /// Call stack.
    pub call_stack: Vec<StackFrame>,
}

/// Debug state for workflow execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DebugState {
    /// Execution is running.
    Running,
    /// Execution is paused at a breakpoint.
    Paused {
        /// Current task ID.
        task_id: String,
        /// Reason for pause.
        reason: String,
    },
    /// Execution is stepping through tasks.
    Stepping,
    /// Execution has completed.
    Completed,
    /// Execution has failed.
    Failed {
        /// Error message.
        error: String,
    },
}

/// Debug state for a single task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDebugState {
    /// Task ID.
    pub task_id: String,
    /// Task status.
    pub status: String,
    /// Task inputs.
    pub inputs: HashMap<String, serde_json::Value>,
    /// Task outputs.
    pub outputs: Option<HashMap<String, serde_json::Value>>,
    /// Execution duration.
    pub duration_ms: Option<u64>,
    /// Error information.
    pub error: Option<String>,
}

/// Breakpoint definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Breakpoint {
    /// Breakpoint ID.
    pub id: String,
    /// Task ID where breakpoint is set.
    pub task_id: String,
    /// Breakpoint condition (optional).
    pub condition: Option<String>,
    /// Whether the breakpoint is enabled.
    pub enabled: bool,
    /// Hit count.
    pub hit_count: usize,
}

/// Stack frame for call stack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackFrame {
    /// Task ID.
    pub task_id: String,
    /// Task name.
    pub task_name: String,
    /// Frame index.
    pub index: usize,
}

/// Debug session for a workflow execution.
pub struct DebugSession {
    execution_id: String,
    info: Arc<RwLock<DebugInfo>>,
    breakpoints: Arc<DashMap<String, Breakpoint>>,
}

impl DebugSession {
    /// Create a new debug session.
    pub fn new(workflow_id: String, execution_id: String) -> Self {
        Self {
            execution_id: execution_id.clone(),
            info: Arc::new(RwLock::new(DebugInfo {
                workflow_id,
                execution_id,
                timestamp: Utc::now(),
                state: DebugState::Running,
                task_states: HashMap::new(),
                breakpoints: Vec::new(),
                variables: HashMap::new(),
                call_stack: Vec::new(),
            })),
            breakpoints: Arc::new(DashMap::new()),
        }
    }

    /// Add a breakpoint.
    pub fn add_breakpoint(&self, task_id: String, condition: Option<String>) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let breakpoint = Breakpoint {
            id: id.clone(),
            task_id,
            condition,
            enabled: true,
            hit_count: 0,
        };

        self.breakpoints.insert(id.clone(), breakpoint);
        id
    }

    /// Remove a breakpoint.
    pub fn remove_breakpoint(&self, breakpoint_id: &str) -> Option<Breakpoint> {
        self.breakpoints.remove(breakpoint_id).map(|(_, bp)| bp)
    }

    /// Enable a breakpoint.
    pub fn enable_breakpoint(&self, breakpoint_id: &str) {
        if let Some(mut bp) = self.breakpoints.get_mut(breakpoint_id) {
            bp.enabled = true;
        }
    }

    /// Disable a breakpoint.
    pub fn disable_breakpoint(&self, breakpoint_id: &str) {
        if let Some(mut bp) = self.breakpoints.get_mut(breakpoint_id) {
            bp.enabled = false;
        }
    }

    /// Check if execution should pause at this task.
    pub async fn should_pause(&self, task_id: &str) -> bool {
        for entry in self.breakpoints.iter() {
            let bp = entry.value();
            if bp.enabled && bp.task_id == task_id {
                // Check condition if present
                if bp.condition.is_some() {
                    // Condition evaluation would go here
                    // For now, just pause
                }
                return true;
            }
        }
        false
    }

    /// Pause execution at a task.
    pub async fn pause(&self, task_id: String, reason: String) {
        let mut info = self.info.write().await;
        info.state = DebugState::Paused { task_id, reason };
        info.timestamp = Utc::now();
    }

    /// Resume execution.
    pub async fn resume(&self) {
        let mut info = self.info.write().await;
        info.state = DebugState::Running;
        info.timestamp = Utc::now();
    }

    /// Step to next task.
    pub async fn step(&self) {
        let mut info = self.info.write().await;
        info.state = DebugState::Stepping;
        info.timestamp = Utc::now();
    }

    /// Update task state.
    pub async fn update_task_state(&self, task_id: String, state: TaskDebugState) {
        let mut info = self.info.write().await;
        info.task_states.insert(task_id, state);
        info.timestamp = Utc::now();
    }

    /// Set a variable value.
    pub async fn set_variable(&self, name: String, value: serde_json::Value) {
        let mut info = self.info.write().await;
        info.variables.insert(name, value);
    }

    /// Get current debug info.
    pub async fn get_info(&self) -> DebugInfo {
        self.info.read().await.clone()
    }

    /// Get execution ID.
    pub fn execution_id(&self) -> &str {
        &self.execution_id
    }
}

/// Workflow debugger.
pub struct Debugger {
    sessions: Arc<DashMap<String, Arc<DebugSession>>>,
}

impl Debugger {
    /// Create a new debugger.
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(DashMap::new()),
        }
    }

    /// Start a debug session.
    pub fn start_session(&self, workflow_id: String, execution_id: String) -> Arc<DebugSession> {
        let session = Arc::new(DebugSession::new(workflow_id, execution_id.clone()));
        self.sessions.insert(execution_id, session.clone());
        session
    }

    /// Get a debug session.
    pub fn get_session(&self, execution_id: &str) -> Option<Arc<DebugSession>> {
        self.sessions.get(execution_id).map(|entry| entry.clone())
    }

    /// End a debug session.
    pub fn end_session(&self, execution_id: &str) {
        self.sessions.remove(execution_id);
    }

    /// Get all active sessions.
    pub fn get_all_sessions(&self) -> Vec<Arc<DebugSession>> {
        self.sessions
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Get session count.
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }
}

impl Default for Debugger {
    fn default() -> Self {
        Self::new()
    }
}

/// Debug command for interactive debugging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DebugCommand {
    /// Continue execution.
    Continue,
    /// Step to next task.
    Step,
    /// Step over (skip subtasks).
    StepOver,
    /// Step into subtask.
    StepInto,
    /// Step out of current context.
    StepOut,
    /// Pause execution.
    Pause,
    /// Add breakpoint.
    AddBreakpoint {
        /// Task ID.
        task_id: String,
        /// Condition.
        condition: Option<String>,
    },
    /// Remove breakpoint.
    RemoveBreakpoint {
        /// Breakpoint ID.
        breakpoint_id: String,
    },
    /// Inspect variable.
    InspectVariable {
        /// Variable name.
        name: String,
    },
    /// Set variable value.
    SetVariable {
        /// Variable name.
        name: String,
        /// Variable value.
        value: serde_json::Value,
    },
    /// Get call stack.
    GetCallStack,
    /// Terminate execution.
    Terminate,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_debug_session_creation() {
        let session = DebugSession::new("workflow1".to_string(), "exec1".to_string());
        assert_eq!(session.execution_id(), "exec1");

        let info = session.get_info().await;
        assert_eq!(info.workflow_id, "workflow1");
    }

    #[tokio::test]
    async fn test_breakpoints() {
        let session = DebugSession::new("workflow1".to_string(), "exec1".to_string());

        let bp_id = session.add_breakpoint("task1".to_string(), None);
        assert!(session.should_pause("task1").await);

        session.remove_breakpoint(&bp_id);
        assert!(!session.should_pause("task1").await);
    }

    #[tokio::test]
    async fn test_pause_resume() {
        let session = DebugSession::new("workflow1".to_string(), "exec1".to_string());

        session
            .pause("task1".to_string(), "breakpoint".to_string())
            .await;
        let info = session.get_info().await;
        assert!(matches!(info.state, DebugState::Paused { .. }));

        session.resume().await;
        let info = session.get_info().await;
        assert!(matches!(info.state, DebugState::Running));
    }

    #[tokio::test]
    async fn test_debugger() {
        let debugger = Debugger::new();

        let _session = debugger.start_session("workflow1".to_string(), "exec1".to_string());
        assert_eq!(debugger.session_count(), 1);

        let retrieved = debugger.get_session("exec1");
        assert!(retrieved.is_some());

        debugger.end_session("exec1");
        assert_eq!(debugger.session_count(), 0);
    }

    #[tokio::test]
    async fn test_variables() {
        let session = DebugSession::new("workflow1".to_string(), "exec1".to_string());

        session
            .set_variable("test_var".to_string(), serde_json::json!(42))
            .await;

        let info = session.get_info().await;
        assert_eq!(info.variables.get("test_var"), Some(&serde_json::json!(42)));
    }
}
