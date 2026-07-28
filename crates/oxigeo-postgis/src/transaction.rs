//! Transaction management for PostGIS operations
//!
//! This module provides transaction support for database operations.

use crate::error::{Result, TransactionError};
use std::mem::ManuallyDrop;
use tokio_postgres::Transaction as PgTransaction;
use tracing::{debug, info};

/// Transaction wrapper
pub struct Transaction<'a> {
    tx: ManuallyDrop<PgTransaction<'a>>,
    /// `true` once the inner [`PgTransaction`] has been moved out of the
    /// [`ManuallyDrop`] wrapper via [`ManuallyDrop::take`] (by `commit()` or
    /// `rollback()`). This guards [`Drop`] against taking the value a second
    /// time, which would be undefined behaviour (double free).
    finished: bool,
}

impl<'a> Transaction<'a> {
    /// Creates a new transaction
    pub(crate) fn new(tx: PgTransaction<'a>) -> Self {
        Self {
            tx: ManuallyDrop::new(tx),
            finished: false,
        }
    }

    /// Executes a query within the transaction
    pub async fn execute(
        &self,
        query: &str,
        params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
    ) -> Result<u64> {
        self.tx.execute(query, params).await.map_err(|e| {
            TransactionError::CommitFailed {
                message: e.to_string(),
            }
            .into()
        })
    }

    /// Queries within the transaction
    pub async fn query(
        &self,
        query: &str,
        params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
    ) -> Result<Vec<tokio_postgres::Row>> {
        self.tx.query(query, params).await.map_err(|e| {
            TransactionError::CommitFailed {
                message: e.to_string(),
            }
            .into()
        })
    }

    /// Creates a savepoint
    pub async fn savepoint(&self, name: &str) -> Result<()> {
        debug!("Creating savepoint: {name}");
        self.tx
            .execute(&format!("SAVEPOINT {name}"), &[])
            .await
            .map_err(|e| TransactionError::SavepointError {
                message: e.to_string(),
            })?;
        Ok(())
    }

    /// Releases a savepoint
    pub async fn release_savepoint(&self, name: &str) -> Result<()> {
        debug!("Releasing savepoint: {name}");
        self.tx
            .execute(&format!("RELEASE SAVEPOINT {name}"), &[])
            .await
            .map_err(|e| TransactionError::SavepointError {
                message: e.to_string(),
            })?;
        Ok(())
    }

    /// Rolls back to a savepoint
    pub async fn rollback_to_savepoint(&self, name: &str) -> Result<()> {
        debug!("Rolling back to savepoint: {name}");
        self.tx
            .execute(&format!("ROLLBACK TO SAVEPOINT {name}"), &[])
            .await
            .map_err(|e| TransactionError::SavepointError {
                message: e.to_string(),
            })?;
        Ok(())
    }

    /// Commits the transaction
    pub async fn commit(mut self) -> Result<()> {
        info!("Committing transaction");
        // SAFETY: `finished` is `false` here (a `Transaction` is only ever
        // consumed by `commit`/`rollback`, each of which takes ownership of
        // `self` and runs exactly once), so the inner `PgTransaction` has not
        // yet been taken. We set `finished` to `true` immediately after taking
        // it — before the `.await` — so that if the commit fails and `self` is
        // then dropped, `Drop` will not attempt to take it a second time.
        let tx = unsafe { ManuallyDrop::take(&mut self.tx) };
        self.finished = true;
        tx.commit()
            .await
            .map_err(|e| TransactionError::CommitFailed {
                message: e.to_string(),
            })?;
        Ok(())
    }

    /// Rolls back the transaction
    pub async fn rollback(mut self) -> Result<()> {
        info!("Rolling back transaction");
        // SAFETY: identical reasoning to `commit` — the inner `PgTransaction`
        // has not been taken yet, and `finished` is set before the `.await`
        // so a failed rollback cannot cause a double-take in `Drop`.
        let tx = unsafe { ManuallyDrop::take(&mut self.tx) };
        self.finished = true;
        tx.rollback().await.map_err(|e| {
            TransactionError::RollbackFailed {
                message: e.to_string(),
            }
            .into()
        })
    }
}

impl<'a> Drop for Transaction<'a> {
    fn drop(&mut self) {
        if !self.finished {
            // The transaction went out of scope without an explicit
            // `commit()`/`rollback()` (e.g. an early `?` return or a panic).
            // Take the inner `PgTransaction` and drop it so that
            // `tokio_postgres`'s own `Drop` impl queues a best-effort
            // `ROLLBACK` on the connection. Merely logging (as the previous
            // implementation did) left the server-side transaction open and
            // holding locks, because the `ManuallyDrop` wrapper suppressed the
            // inner value's `Drop`.
            debug!(
                "Transaction dropped without explicit commit/rollback - issuing implicit ROLLBACK"
            );
            // SAFETY: `finished` is `false`, so the inner `PgTransaction` has
            // not been taken; taking it exactly once here and dropping it is
            // sound.
            let tx = unsafe { ManuallyDrop::take(&mut self.tx) };
            drop(tx);
        }
    }
}

/// Transaction manager extension for ConnectionPool
pub trait TransactionManager {
    /// Begins a new transaction
    async fn begin_transaction(&self) -> Result<Transaction<'_>>;
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_transaction_creation() {
        // Transaction tests require actual database connection
        // These are integration tests that should be run separately
        let _placeholder = 1;
    }
}
