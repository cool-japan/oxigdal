//! Credit-based flow control for geospatial stream processing.
//!
//! Producers are given a credit budget. They may only emit items when
//! they have available credits. Consumers replenish credits as they
//! consume. This prevents unbounded buffering under load.
//!
//! Design:
//! - `CreditPool`: shared atomic credit counter (Arc-wrapped for multi-producer/consumer use)
//! - `BackpressureProducer`: wraps a data source with credit checking; stages pending items
//! - `BackpressureConsumer`: wraps a sink with credit replenishment

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use crate::error::StreamingError;

/// A pool of credits shared between producer and consumer.
///
/// Credits are atomically tracked; producers consume credits when emitting items
/// and consumers release credits as they process those items.
#[derive(Debug, Clone)]
pub struct CreditPool {
    credits: Arc<AtomicI64>,
    capacity: i64,
}

impl CreditPool {
    /// Create a pool with the given initial (and maximum) capacity.
    pub fn new(capacity: i64) -> Self {
        assert!(capacity > 0, "CreditPool capacity must be positive");
        Self {
            credits: Arc::new(AtomicI64::new(capacity)),
            capacity,
        }
    }

    /// Try to acquire `n` credits (non-blocking).
    ///
    /// Returns `true` if the credits were successfully acquired, `false` if the
    /// pool does not currently hold enough credits (backpressure signal).
    pub fn try_acquire(&self, n: i64) -> bool {
        assert!(n > 0, "must acquire at least 1 credit");
        let mut current = self.credits.load(Ordering::Relaxed);
        loop {
            if current < n {
                return false;
            }
            match self.credits.compare_exchange_weak(
                current,
                current - n,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => return true,
                Err(actual) => current = actual,
            }
        }
    }

    /// Release `n` credits back to the pool (consumer-side).
    ///
    /// The pool is clamped to its capacity to prevent over-replenishment.
    pub fn release(&self, n: i64) {
        assert!(n > 0, "must release at least 1 credit");
        // fetch_add then clamp in a CAS loop
        let prev = self.credits.fetch_add(n, Ordering::AcqRel);
        let after = prev + n;
        if after > self.capacity {
            // Clamp: try to bring the counter back down to capacity.
            // A CAS loop is needed because another thread may have changed it.
            let mut current = after;
            loop {
                if current <= self.capacity {
                    break;
                }
                match self.credits.compare_exchange_weak(
                    current,
                    self.capacity,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(actual) => current = actual,
                }
            }
        }
    }

    /// Number of credits currently available.
    pub fn available(&self) -> i64 {
        self.credits.load(Ordering::Acquire)
    }

    /// Maximum credits this pool can hold.
    pub fn capacity(&self) -> i64 {
        self.capacity
    }

    /// Utilization fraction: `0.0` when pool is full (nothing consumed),
    /// `1.0` when completely empty (maximum backpressure).
    pub fn utilization(&self) -> f64 {
        let avail = self.available().max(0);
        1.0 - (avail as f64 / self.capacity as f64)
    }
}

// ─── PendingItem ──────────────────────────────────────────────────────────────

/// An item staged by the producer, waiting to be drained by the consumer.
#[derive(Debug)]
pub struct PendingItem<T> {
    /// The payload.
    pub item: T,
    /// Credits that were consumed when this item was emitted.
    pub credits_required: i64,
}

// ─── BackpressureProducer ─────────────────────────────────────────────────────

/// A producer that checks the shared `CreditPool` before emitting items.
///
/// Items that are successfully emitted are staged in an internal `VecDeque`;
/// the consumer should call `drain()` to retrieve them and then call
/// `BackpressureConsumer::consume()` for each item processed.
pub struct BackpressureProducer<T> {
    pool: CreditPool,
    pending: VecDeque<PendingItem<T>>,
    emitted_total: u64,
    dropped_total: u64,
}

impl<T> BackpressureProducer<T> {
    /// Create a producer that shares the given `CreditPool`.
    pub fn new(pool: CreditPool) -> Self {
        Self {
            pool,
            pending: VecDeque::new(),
            emitted_total: 0,
            dropped_total: 0,
        }
    }

    /// Try to emit an item consuming `credits` credits.
    ///
    /// Returns:
    /// - `Ok(true)` — item was staged successfully.
    /// - `Ok(false)` — backpressured; caller should retry later or drop the item.
    pub fn try_emit(&mut self, item: T, credits: i64) -> Result<bool, StreamingError> {
        if credits <= 0 {
            return Err(StreamingError::InvalidOperation(
                "credits must be positive".into(),
            ));
        }
        if self.pool.try_acquire(credits) {
            self.pending.push_back(PendingItem {
                item,
                credits_required: credits,
            });
            self.emitted_total += 1;
            Ok(true)
        } else {
            self.dropped_total += 1;
            Ok(false)
        }
    }

    /// Drain all staged items, yielding them to the consumer.
    pub fn drain(&mut self) -> impl Iterator<Item = PendingItem<T>> + '_ {
        self.pending.drain(..)
    }

    /// Total items successfully emitted since creation.
    pub fn emitted_total(&self) -> u64 {
        self.emitted_total
    }

    /// Total items dropped due to backpressure since creation.
    pub fn dropped_total(&self) -> u64 {
        self.dropped_total
    }

    /// Number of items currently staged (not yet drained).
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Shared reference to the underlying credit pool.
    pub fn pool(&self) -> &CreditPool {
        &self.pool
    }
}

// ─── BackpressureConsumer ─────────────────────────────────────────────────────

/// A consumer that releases credits back to the pool as items are processed.
pub struct BackpressureConsumer {
    pool: CreditPool,
    consumed_total: u64,
}

impl BackpressureConsumer {
    /// Create a consumer that shares the given `CreditPool`.
    pub fn new(pool: CreditPool) -> Self {
        Self {
            pool,
            consumed_total: 0,
        }
    }

    /// Mark `credits` worth of processing as complete, releasing those credits
    /// back to the pool so the producer can emit more items.
    pub fn consume(&mut self, credits: i64) {
        self.pool.release(credits);
        self.consumed_total += 1;
    }

    /// Total items consumed (i.e. times `consume()` was called) since creation.
    pub fn consumed_total(&self) -> u64 {
        self.consumed_total
    }

    /// Shared reference to the underlying credit pool.
    pub fn pool(&self) -> &CreditPool {
        &self.pool
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_credit_pool_initial_credits() {
        let pool = CreditPool::new(100);
        assert_eq!(pool.available(), 100);
        assert_eq!(pool.capacity(), 100);
    }

    #[test]
    fn test_credit_pool_try_acquire_success() {
        let pool = CreditPool::new(50);
        assert!(pool.try_acquire(30));
        assert_eq!(pool.available(), 20);
    }

    #[test]
    fn test_credit_pool_try_acquire_fail_insufficient() {
        let pool = CreditPool::new(10);
        assert!(!pool.try_acquire(11));
        assert_eq!(pool.available(), 10); // unchanged
    }

    #[test]
    fn test_credit_pool_release_replenishes() {
        let pool = CreditPool::new(100);
        assert!(pool.try_acquire(40));
        pool.release(40);
        assert_eq!(pool.available(), 100);
    }

    #[test]
    fn test_credit_pool_over_release_clamped_to_capacity() {
        let pool = CreditPool::new(50);
        pool.release(30); // release without prior acquire — should clamp to capacity
        assert_eq!(pool.available(), 50);
    }

    #[test]
    fn test_credit_pool_utilization_zero_when_full() {
        let pool = CreditPool::new(100);
        assert!((pool.utilization() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_credit_pool_utilization_one_when_empty() {
        let pool = CreditPool::new(100);
        assert!(pool.try_acquire(100));
        assert!((pool.utilization() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_producer_emit_success() {
        let pool = CreditPool::new(10);
        let mut producer = BackpressureProducer::new(pool);
        let ok = producer
            .try_emit("hello", 5)
            .expect("try_emit should not error");
        assert!(ok);
        assert_eq!(producer.emitted_total(), 1);
        assert_eq!(producer.pending_count(), 1);
    }

    #[test]
    fn test_producer_backpressure_when_no_credits() {
        let pool = CreditPool::new(5);
        let mut producer = BackpressureProducer::new(pool);
        // consume all credits
        assert!(
            producer
                .try_emit("first", 5)
                .expect("emit should not error")
        );
        // now no credits remain
        let ok = producer
            .try_emit("second", 1)
            .expect("emit should not error");
        assert!(!ok);
        assert_eq!(producer.dropped_total(), 1);
    }

    #[test]
    fn test_producer_drain_yields_pending_items() {
        let pool = CreditPool::new(20);
        let mut producer = BackpressureProducer::new(pool);
        producer.try_emit(1u32, 4).expect("emit ok");
        producer.try_emit(2u32, 4).expect("emit ok");
        let items: Vec<_> = producer.drain().map(|p| p.item).collect();
        assert_eq!(items, vec![1, 2]);
        assert_eq!(producer.pending_count(), 0);
    }

    #[test]
    fn test_consumer_consume_increments_count() {
        let pool = CreditPool::new(100);
        let mut consumer = BackpressureConsumer::new(pool);
        consumer.consume(10);
        consumer.consume(10);
        assert_eq!(consumer.consumed_total(), 2);
    }

    #[test]
    fn test_consumer_consume_releases_credits() {
        let pool = CreditPool::new(100);
        let consumer_pool = pool.clone();
        // drain all credits via producer
        let mut producer = BackpressureProducer::new(pool);
        producer.try_emit("x", 100).expect("emit ok");
        assert_eq!(producer.pool().available(), 0);

        let mut consumer = BackpressureConsumer::new(consumer_pool);
        consumer.consume(50);
        assert_eq!(consumer.pool().available(), 50);
    }

    #[test]
    fn test_credit_pool_clone_shares_state() {
        let pool = CreditPool::new(100);
        let pool2 = pool.clone();
        assert!(pool.try_acquire(40));
        // pool2 reflects the same atomic
        assert_eq!(pool2.available(), 60);
    }
}
