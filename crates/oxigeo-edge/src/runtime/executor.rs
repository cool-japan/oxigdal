//! Task executor for edge runtime

use crate::error::{EdgeError, Result};
use crate::resource::ResourceManager;
use futures::Future;
use std::sync::Arc;
use tokio::task::JoinHandle;

/// Task executor with resource management
pub struct Executor {
    resource_manager: Arc<ResourceManager>,
}

impl Executor {
    /// Create new executor
    pub fn new(resource_manager: Arc<ResourceManager>) -> Self {
        Self { resource_manager }
    }

    /// Execute a task with resource tracking
    pub async fn execute<F, T>(&self, task: F) -> Result<T>
    where
        F: Future<Output = Result<T>> + Send + 'static,
        T: Send + 'static,
    {
        // Check if we can start operation
        self.resource_manager.can_start_operation()?;

        // Start operation with guard
        let _guard = self.resource_manager.start_operation()?;

        // Execute task
        let handle: JoinHandle<Result<T>> = tokio::spawn(task);

        match handle.await {
            Ok(result) => {
                if result.is_err() {
                    self.resource_manager.record_failure();
                }
                result
            }
            Err(e) => {
                self.resource_manager.record_failure();
                Err(EdgeError::runtime(format!("Task panicked: {}", e)))
            }
        }
    }

    /// Execute multiple tasks concurrently
    pub async fn execute_batch<F, T>(&self, tasks: Vec<F>) -> Vec<Result<T>>
    where
        F: Future<Output = Result<T>> + Send + 'static,
        T: Send + 'static,
    {
        let mut handles = Vec::with_capacity(tasks.len());

        for task in tasks {
            let executor = Self {
                resource_manager: Arc::clone(&self.resource_manager),
            };
            let handle = tokio::spawn(async move { executor.execute(task).await });
            handles.push(handle);
        }

        let mut results = Vec::with_capacity(handles.len());
        for handle in handles {
            match handle.await {
                Ok(result) => results.push(result),
                Err(e) => results.push(Err(EdgeError::runtime(format!("Task failed: {}", e)))),
            }
        }

        results
    }

    /// Get resource manager
    pub fn resource_manager(&self) -> &Arc<ResourceManager> {
        &self.resource_manager
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::ResourceConstraints;

    #[tokio::test]
    async fn test_executor_execute() -> Result<()> {
        let constraints = ResourceConstraints::minimal();
        let manager = ResourceManager::new(constraints)?;
        let executor = Executor::new(Arc::new(manager));

        let result = executor.execute(async { Ok(42) }).await?;
        assert_eq!(result, 42);

        Ok(())
    }

    #[tokio::test]
    async fn test_executor_execute_error() {
        let constraints = ResourceConstraints::minimal();
        let manager = ResourceManager::new(constraints).expect("Failed to create manager");
        let executor = Executor::new(Arc::new(manager));

        let result: Result<i32> = executor
            .execute(async { Err(EdgeError::runtime("test error")) })
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_executor_batch() -> Result<()> {
        let constraints = ResourceConstraints::minimal();
        let manager = ResourceManager::new(constraints)?;
        let executor = Executor::new(Arc::new(manager));

        // Execute multiple tasks individually to test concurrent execution
        let result1 = executor.execute(async { Ok(1i32) }).await?;
        let result2 = executor.execute(async { Ok(2i32) }).await?;
        let result3 = executor.execute(async { Ok(3i32) }).await?;

        assert_eq!(result1, 1);
        assert_eq!(result2, 2);
        assert_eq!(result3, 3);

        Ok(())
    }

    #[tokio::test]
    async fn test_executor_resource_limit() -> Result<()> {
        let mut constraints = ResourceConstraints::minimal();
        constraints.max_concurrent_ops = 1;
        let manager = Arc::new(ResourceManager::new(constraints)?);
        let executor = Executor::new(Arc::clone(&manager));

        // Start a long-running task
        let _guard = manager.start_operation()?;

        // This should fail due to concurrent ops limit
        let result: Result<i32> = executor.execute(async { Ok(42) }).await;
        assert!(result.is_err());

        Ok(())
    }
}
