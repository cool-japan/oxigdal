//! Error types for cloud-enhanced operations.

/// Result type alias for cloud-enhanced operations.
pub type Result<T> = std::result::Result<T, CloudEnhancedError>;

/// Errors that can occur during cloud-enhanced operations.
#[derive(Debug, thiserror::Error)]
pub enum CloudEnhancedError {
    /// AWS service error
    #[error("AWS service error: {0}")]
    AwsService(String),

    /// Azure service error
    #[error("Azure service error: {0}")]
    AzureService(String),

    /// GCP service error
    #[error("GCP service error: {0}")]
    GcpService(String),

    /// Authentication error
    #[error("Authentication error: {0}")]
    Authentication(String),

    /// Query execution error
    #[error("Query execution error: {0}")]
    QueryExecution(String),

    /// Data catalog error
    #[error("Data catalog error: {0}")]
    DataCatalog(String),

    /// ML service error
    #[error("ML service error: {0}")]
    MlService(String),

    /// Monitoring error
    #[error("Monitoring error: {0}")]
    Monitoring(String),

    /// Cost optimization error
    #[error("Cost optimization error: {0}")]
    CostOptimization(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Arrow error
    #[error("Arrow error: {0}")]
    Arrow(String),

    /// Parquet error
    #[error("Parquet error: {0}")]
    Parquet(String),

    /// Invalid state
    #[error("Invalid state: {0}")]
    InvalidState(String),

    /// Timeout error
    #[error("Operation timed out: {0}")]
    Timeout(String),

    /// Not found error
    #[error("Resource not found: {0}")]
    NotFound(String),

    /// Permission denied
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    /// Quota exceeded
    #[error("Quota exceeded: {0}")]
    QuotaExceeded(String),

    /// Invalid argument
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    /// Generic error
    #[error("{0}")]
    Generic(String),
}

impl CloudEnhancedError {
    /// Creates a new AWS service error.
    pub fn aws_service(msg: impl Into<String>) -> Self {
        Self::AwsService(msg.into())
    }

    /// Creates a new Azure service error.
    pub fn azure_service(msg: impl Into<String>) -> Self {
        Self::AzureService(msg.into())
    }

    /// Creates a new GCP service error.
    pub fn gcp_service(msg: impl Into<String>) -> Self {
        Self::GcpService(msg.into())
    }

    /// Creates a new authentication error.
    pub fn authentication(msg: impl Into<String>) -> Self {
        Self::Authentication(msg.into())
    }

    /// Creates a new query execution error.
    pub fn query_execution(msg: impl Into<String>) -> Self {
        Self::QueryExecution(msg.into())
    }

    /// Creates a new data catalog error.
    pub fn data_catalog(msg: impl Into<String>) -> Self {
        Self::DataCatalog(msg.into())
    }

    /// Creates a new ML service error.
    pub fn ml_service(msg: impl Into<String>) -> Self {
        Self::MlService(msg.into())
    }

    /// Creates a new monitoring error.
    pub fn monitoring(msg: impl Into<String>) -> Self {
        Self::Monitoring(msg.into())
    }

    /// Creates a new cost optimization error.
    pub fn cost_optimization(msg: impl Into<String>) -> Self {
        Self::CostOptimization(msg.into())
    }

    /// Creates a new configuration error.
    pub fn configuration(msg: impl Into<String>) -> Self {
        Self::Configuration(msg.into())
    }

    /// Creates a new serialization error.
    pub fn serialization(msg: impl Into<String>) -> Self {
        Self::Serialization(msg.into())
    }

    /// Creates a new Arrow error.
    pub fn arrow(msg: impl Into<String>) -> Self {
        Self::Arrow(msg.into())
    }

    /// Creates a new Parquet error.
    pub fn parquet(msg: impl Into<String>) -> Self {
        Self::Parquet(msg.into())
    }

    /// Creates a new invalid state error.
    pub fn invalid_state(msg: impl Into<String>) -> Self {
        Self::InvalidState(msg.into())
    }

    /// Creates a new timeout error.
    pub fn timeout(msg: impl Into<String>) -> Self {
        Self::Timeout(msg.into())
    }

    /// Creates a new not found error.
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }

    /// Creates a new permission denied error.
    pub fn permission_denied(msg: impl Into<String>) -> Self {
        Self::PermissionDenied(msg.into())
    }

    /// Creates a new quota exceeded error.
    pub fn quota_exceeded(msg: impl Into<String>) -> Self {
        Self::QuotaExceeded(msg.into())
    }

    /// Creates a new invalid argument error.
    pub fn invalid_argument(msg: impl Into<String>) -> Self {
        Self::InvalidArgument(msg.into())
    }

    /// Creates a new generic error.
    pub fn generic(msg: impl Into<String>) -> Self {
        Self::Generic(msg.into())
    }
}

impl From<serde_json::Error> for CloudEnhancedError {
    fn from(err: serde_json::Error) -> Self {
        Self::serialization(err.to_string())
    }
}

impl From<arrow::error::ArrowError> for CloudEnhancedError {
    fn from(err: arrow::error::ArrowError) -> Self {
        Self::arrow(err.to_string())
    }
}

impl From<parquet::errors::ParquetError> for CloudEnhancedError {
    fn from(err: parquet::errors::ParquetError) -> Self {
        Self::parquet(err.to_string())
    }
}
