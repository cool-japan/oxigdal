//! Task executor for edge runtime

use crate::error::{EdgeError, Result};
use crate::resource::ResourceManager;
use futures::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::task::JoinHandle;

/// Wraps a [`JoinHandle`] so that the spawned task is aborted if this wrapper
/// is dropped before the task completes.
///
/// This matters because `tokio::spawn` detaches the spawned task from its
/// `JoinHandle`: simply `.await`-ing (and then dropping) a `JoinHandle` does
/// **not** stop the underlying task if the *awaiting* future itself is
/// cancelled (e.g. the caller wraps `Executor::execute` in
/// `tokio::time::timeout` and the timeout fires, or the calling task is
/// itself cancelled). Wrapping the handle in this RAII type ensures that
/// dropping the `execute(...)` future actually aborts the in-flight work
/// instead of leaving it running detached and unaccounted for by
/// [`ResourceManager`].
struct AbortOnDrop<T>(JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl<T> Future for AbortOnDrop<T> {
    type Output = std::result::Result<T, tokio::task::JoinError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // `JoinHandle` is `Unpin`, so it is sound to obtain a plain `&mut`
        // reference to it via `Pin::get_mut` and re-pin it for polling.
        Pin::new(&mut self.get_mut().0).poll(cx)
    }
}

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
        let guard = self.resource_manager.start_operation()?;

        // Move the resource guard *into* the spawned task itself so that
        // resource accounting (the active-op count) only clears once the
        // spawned work actually finishes or is aborted -- not merely when
        // the caller's `execute` future is dropped/cancelled while the
        // detached task keeps running.
        let handle: JoinHandle<Result<T>> = tokio::spawn(async move {
            let _guard = guard;
            task.await
        });

        // Abort the spawned task if this future is dropped/cancelled before
        // the task completes, so cancellation actually stops the work
        // instead of merely mis-reporting resource state.
        let abort_on_drop = AbortOnDrop(handle);

        match abort_on_drop.await {
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

    /// Regression test for the cancellation bug: if the caller cancels the
    /// `execute(...)` future (e.g. it was wrapped in a timeout that fired,
    /// or -- as reproduced here -- the task awaiting it is aborted), the
    /// spawned inner task must actually stop running rather than continuing
    /// to completion detached and unaccounted for.
    #[tokio::test]
    async fn test_executor_cancellation_aborts_inner_spawned_task() -> Result<()> {
        use std::sync::atomic::{AtomicBool, Ordering};

        let constraints = ResourceConstraints::minimal();
        let manager = Arc::new(ResourceManager::new(constraints)?);
        let executor = Arc::new(Executor::new(Arc::clone(&manager)));

        let ran_to_completion = Arc::new(AtomicBool::new(false));
        let ran_to_completion_clone = Arc::clone(&ran_to_completion);

        let executor_clone = Arc::clone(&executor);
        let outer_handle = tokio::spawn(async move {
            let _ = executor_clone
                .execute(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                    ran_to_completion_clone.store(true, Ordering::SeqCst);
                    Ok(())
                })
                .await;
        });

        // Let the outer task start and spawn the inner task, but not long
        // enough for the inner 300ms sleep to complete.
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;

        // Cancel the outer future -- this reproduces the exact scenario the
        // fix targets (e.g. a caller wrapping `execute` in
        // `tokio::time::timeout` that fires, or the calling task itself
        // being cancelled).
        outer_handle.abort();
        let _ = outer_handle.await;

        // Give the inner task a window in which it *would* have completed
        // had it not actually been aborted, to make this a meaningful
        // regression guard rather than a timing coincidence.
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;

        assert!(
            !ran_to_completion.load(Ordering::SeqCst),
            "inner spawned task must be aborted when the outer execute() future is cancelled"
        );

        // The resource guard (now owned by the aborted inner task) must
        // also have been released, so active-op accounting isn't left
        // permanently elevated by the cancelled operation.
        assert_eq!(manager.metrics().active_operations, 0);

        Ok(())
    }
}
