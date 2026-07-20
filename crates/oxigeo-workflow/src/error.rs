//! Error types for the workflow engine.

/// Result type for workflow operations.
pub type Result<T> = std::result::Result<T, WorkflowError>;

/// Comprehensive error types for the workflow engine.
#[derive(Debug, thiserror::Error)]
pub enum WorkflowError {
    /// Workflow execution error.
    #[error("Workflow execution error: {0}")]
    Execution(String),

    /// Workflow validation error.
    #[error("Workflow validation error: {0}")]
    Validation(String),

    /// Workflow scheduling error.
    #[error("Workflow scheduling error: {0}")]
    Scheduling(String),

    /// DAG construction error.
    #[error("DAG error: {0}")]
    Dag(#[from] DagError),

    /// Task execution error.
    #[error("Task execution error in '{task_id}': {message}")]
    TaskExecution {
        /// Task identifier.
        task_id: String,
        /// Error message.
        message: String,
    },

    /// Task timeout error.
    #[error("Task '{task_id}' timed out after {timeout_secs}s")]
    TaskTimeout {
        /// Task identifier.
        task_id: String,
        /// Timeout duration in seconds.
        timeout_secs: u64,
    },

    /// Workflow state error.
    #[error("Workflow state error: {0}")]
    State(String),

    /// Workflow not found error.
    #[error("Workflow not found: {0}")]
    NotFound(String),

    /// Workflow already exists error.
    #[error("Workflow already exists: {0}")]
    AlreadyExists(String),

    /// Conditional expression error.
    #[error("Conditional expression error: {0}")]
    ConditionalExpression(String),

    /// Template error.
    #[error("Template error: {0}")]
    Template(String),

    /// Versioning error.
    #[error("Versioning error: {0}")]
    Versioning(String),

    /// Integration error.
    #[error("Integration error with {service}: {message}")]
    Integration {
        /// Service name.
        service: String,
        /// Error message.
        message: String,
    },

    /// Serialization error.
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Cron expression error.
    #[error("Invalid cron expression: {0}")]
    CronExpression(String),

    /// Persistence error.
    #[error("Persistence error: {0}")]
    Persistence(String),

    /// Monitoring error.
    #[error("Monitoring error: {0}")]
    Monitoring(String),

    /// Resource exhaustion error.
    #[error("Resource exhausted: {0}")]
    ResourceExhausted(String),

    /// Deadlock detected error.
    #[error("Deadlock detected in workflow '{workflow_id}'")]
    Deadlock {
        /// Workflow identifier.
        workflow_id: String,
    },

    /// Invalid parameter error.
    #[error("Invalid parameter '{param}': {message}")]
    InvalidParameter {
        /// Parameter name.
        param: String,
        /// Error message.
        message: String,
    },

    /// Internal error.
    #[error("Internal error: {0}")]
    Internal(String),
}

/// DAG-specific errors.
#[derive(Debug, thiserror::Error)]
pub enum DagError {
    /// Cycle detected in DAG.
    #[error("Cycle detected in DAG: {0}")]
    CycleDetected(String),

    /// Invalid node ID.
    #[error("Invalid node ID: {0}")]
    InvalidNode(String),

    /// Invalid edge.
    #[error("Invalid edge from '{from}' to '{to}': {message}")]
    InvalidEdge {
        /// Source node ID.
        from: String,
        /// Target node ID.
        to: String,
        /// Error message.
        message: String,
    },

    /// Empty DAG error.
    #[error("DAG is empty")]
    EmptyDag,

    /// Unreachable node error.
    #[error("Unreachable node: {0}")]
    UnreachableNode(String),

    /// Missing dependency error.
    #[error("Missing dependency: {0}")]
    MissingDependency(String),
}

impl WorkflowError {
    /// Create an execution error.
    pub fn execution<S: Into<String>>(msg: S) -> Self {
        Self::Execution(msg.into())
    }

    /// Create a validation error.
    pub fn validation<S: Into<String>>(msg: S) -> Self {
        Self::Validation(msg.into())
    }

    /// Create a scheduling error.
    pub fn scheduling<S: Into<String>>(msg: S) -> Self {
        Self::Scheduling(msg.into())
    }

    /// Create a state error.
    pub fn state<S: Into<String>>(msg: S) -> Self {
        Self::State(msg.into())
    }

    /// Create a not found error.
    pub fn not_found<S: Into<String>>(id: S) -> Self {
        Self::NotFound(id.into())
    }

    /// Create an already exists error.
    pub fn already_exists<S: Into<String>>(id: S) -> Self {
        Self::AlreadyExists(id.into())
    }

    /// Create a task execution error.
    pub fn task_execution<S1: Into<String>, S2: Into<String>>(task_id: S1, message: S2) -> Self {
        Self::TaskExecution {
            task_id: task_id.into(),
            message: message.into(),
        }
    }

    /// Create a task timeout error.
    pub fn task_timeout<S: Into<String>>(task_id: S, timeout_secs: u64) -> Self {
        Self::TaskTimeout {
            task_id: task_id.into(),
            timeout_secs,
        }
    }

    /// Create a conditional expression error.
    pub fn conditional<S: Into<String>>(msg: S) -> Self {
        Self::ConditionalExpression(msg.into())
    }

    /// Create a template error.
    pub fn template<S: Into<String>>(msg: S) -> Self {
        Self::Template(msg.into())
    }

    /// Create a versioning error.
    pub fn versioning<S: Into<String>>(msg: S) -> Self {
        Self::Versioning(msg.into())
    }

    /// Create an integration error.
    pub fn integration<S1: Into<String>, S2: Into<String>>(service: S1, message: S2) -> Self {
        Self::Integration {
            service: service.into(),
            message: message.into(),
        }
    }

    /// Create a cron expression error.
    pub fn cron_expression<S: Into<String>>(msg: S) -> Self {
        Self::CronExpression(msg.into())
    }

    /// Create a persistence error.
    pub fn persistence<S: Into<String>>(msg: S) -> Self {
        Self::Persistence(msg.into())
    }

    /// Create a monitoring error.
    pub fn monitoring<S: Into<String>>(msg: S) -> Self {
        Self::Monitoring(msg.into())
    }

    /// Create a resource exhausted error.
    pub fn resource_exhausted<S: Into<String>>(msg: S) -> Self {
        Self::ResourceExhausted(msg.into())
    }

    /// Create a deadlock error.
    pub fn deadlock<S: Into<String>>(workflow_id: S) -> Self {
        Self::Deadlock {
            workflow_id: workflow_id.into(),
        }
    }

    /// Create an invalid parameter error.
    pub fn invalid_parameter<S1: Into<String>, S2: Into<String>>(param: S1, message: S2) -> Self {
        Self::InvalidParameter {
            param: param.into(),
            message: message.into(),
        }
    }

    /// Create an internal error.
    pub fn internal<S: Into<String>>(msg: S) -> Self {
        Self::Internal(msg.into())
    }
}

impl DagError {
    /// Create a cycle detected error.
    pub fn cycle<S: Into<String>>(path: S) -> Self {
        Self::CycleDetected(path.into())
    }

    /// Create an invalid node error.
    pub fn invalid_node<S: Into<String>>(node_id: S) -> Self {
        Self::InvalidNode(node_id.into())
    }

    /// Create an invalid edge error.
    pub fn invalid_edge<S1: Into<String>, S2: Into<String>, S3: Into<String>>(
        from: S1,
        to: S2,
        message: S3,
    ) -> Self {
        Self::InvalidEdge {
            from: from.into(),
            to: to.into(),
            message: message.into(),
        }
    }

    /// Create a missing dependency error.
    pub fn missing_dependency<S: Into<String>>(dep: S) -> Self {
        Self::MissingDependency(dep.into())
    }
}
