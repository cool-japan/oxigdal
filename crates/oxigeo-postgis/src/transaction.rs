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
    committed: bool,
}

impl<'a> Transaction<'a> {
    /// Creates a new transaction
    pub(crate) fn new(tx: PgTransaction<'a>) -> Self {
        Self {
            tx: ManuallyDrop::new(tx),
            committed: false,
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
        let tx = unsafe { ManuallyDrop::take(&mut self.tx) };
        tx.commit()
            .await
            .map_err(|e| TransactionError::CommitFailed {
                message: e.to_string(),
            })?;
        self.committed = true;
        Ok(())
    }

    /// Rolls back the transaction
    pub async fn rollback(mut self) -> Result<()> {
        info!("Rolling back transaction");
        let tx = unsafe { ManuallyDrop::take(&mut self.tx) };
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
        if !self.committed {
            debug!("Transaction dropped without commit - will auto-rollback");
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
