//! Synchronization primitives for embedded systems

use crate::error::{EmbeddedError, Result};
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use portable_atomic::AtomicI32;

/// Atomic counter for statistics and monitoring
pub struct AtomicCounter {
    value: AtomicU64,
}

impl AtomicCounter {
    /// Create a new counter with initial value
    pub const fn new(initial: u64) -> Self {
        Self {
            value: AtomicU64::new(initial),
        }
    }

    /// Increment the counter
    pub fn increment(&self) -> u64 {
        self.value.fetch_add(1, Ordering::Relaxed)
    }

    /// Decrement the counter
    pub fn decrement(&self) -> u64 {
        self.value.fetch_sub(1, Ordering::Relaxed)
    }

    /// Add to the counter
    pub fn add(&self, val: u64) -> u64 {
        self.value.fetch_add(val, Ordering::Relaxed)
    }

    /// Get current value
    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }

    /// Set value
    pub fn set(&self, val: u64) {
        self.value.store(val, Ordering::Relaxed);
    }

    /// Reset to zero
    pub fn reset(&self) {
        self.value.store(0, Ordering::Relaxed);
    }

    /// Compare and swap
    pub fn compare_and_swap(&self, current: u64, new: u64) -> Result<u64> {
        match self
            .value
            .compare_exchange(current, new, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(val) => Ok(val),
            Err(_) => Err(EmbeddedError::ResourceBusy),
        }
    }
}

impl Default for AtomicCounter {
    fn default() -> Self {
        Self::new(0)
    }
}

/// Spinlock for mutual exclusion
pub struct Spinlock {
    locked: AtomicBool,
}

impl Spinlock {
    /// Create a new unlocked spinlock
    pub const fn new() -> Self {
        Self {
            locked: AtomicBool::new(false),
        }
    }

    /// Try to acquire the lock without blocking
    pub fn try_lock(&self) -> Result<()> {
        match self
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        {
            Ok(_) => Ok(()),
            Err(_) => Err(EmbeddedError::ResourceBusy),
        }
    }

    /// Acquire the lock (spinning until available)
    pub fn lock(&self) {
        while self
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            // Spin with hint to reduce power consumption
            core::hint::spin_loop();
        }
    }

    /// Release the lock
    ///
    /// # Safety
    ///
    /// Must be called by the thread that acquired the lock
    pub unsafe fn unlock(&self) {
        self.locked.store(false, Ordering::Release);
    }

    /// Check if locked
    pub fn is_locked(&self) -> bool {
        self.locked.load(Ordering::Relaxed)
    }
}

impl Default for Spinlock {
    fn default() -> Self {
        Self::new()
    }
}

/// RAII guard for spinlock
pub struct SpinlockGuard<'a> {
    lock: &'a Spinlock,
}

impl<'a> Drop for SpinlockGuard<'a> {
    fn drop(&mut self) {
        // SAFETY: Guard owns the lock
        unsafe {
            self.lock.unlock();
        }
    }
}

impl Spinlock {
    /// Acquire lock and return RAII guard
    pub fn lock_guard(&self) -> SpinlockGuard<'_> {
        self.lock();
        SpinlockGuard { lock: self }
    }
}

/// Simple mutex using spinlock
pub struct Mutex<T> {
    lock: Spinlock,
    data: UnsafeCell<T>,
}

impl<T> Mutex<T> {
    /// Create a new mutex
    pub const fn new(data: T) -> Self {
        Self {
            lock: Spinlock::new(),
            data: UnsafeCell::new(data),
        }
    }

    /// Try to lock and get access to data
    pub fn try_lock(&self) -> Result<MutexGuard<'_, T>> {
        self.lock.try_lock()?;
        Ok(MutexGuard { mutex: self })
    }

    /// Lock and get access to data (blocking)
    pub fn lock(&self) -> MutexGuard<'_, T> {
        self.lock.lock();
        MutexGuard { mutex: self }
    }

    /// Get a mutable reference (when we have exclusive access)
    pub fn get_mut(&mut self) -> &mut T {
        self.data.get_mut()
    }
}

// SAFETY: Mutex provides exclusive access through locking
unsafe impl<T: Send> Send for Mutex<T> {}
unsafe impl<T: Send> Sync for Mutex<T> {}

/// RAII guard for mutex
pub struct MutexGuard<'a, T> {
    mutex: &'a Mutex<T>,
}

impl<'a, T> core::ops::Deref for MutexGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: Guard holds the lock
        unsafe { &*self.mutex.data.get() }
    }
}

impl<'a, T> core::ops::DerefMut for MutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: Guard holds the lock
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<'a, T> Drop for MutexGuard<'a, T> {
    fn drop(&mut self) {
        // SAFETY: Guard owns the lock
        unsafe {
            self.mutex.lock.unlock();
        }
    }
}

/// Semaphore for resource counting
pub struct Semaphore {
    count: AtomicI32,
}

impl Semaphore {
    /// Create a new semaphore with initial count
    pub const fn new(count: i32) -> Self {
        Self {
            count: AtomicI32::new(count),
        }
    }

    /// Try to acquire (decrement count)
    pub fn try_acquire(&self) -> Result<()> {
        let current = self.count.load(Ordering::Acquire);
        if current <= 0 {
            return Err(EmbeddedError::ResourceBusy);
        }

        match self
            .count
            .compare_exchange(current, current - 1, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => Ok(()),
            Err(_) => Err(EmbeddedError::ResourceBusy),
        }
    }

    /// Acquire (blocking)
    pub fn acquire(&self) {
        loop {
            if self.try_acquire().is_ok() {
                return;
            }
            core::hint::spin_loop();
        }
    }

    /// Release (increment count)
    pub fn release(&self) {
        self.count.fetch_add(1, Ordering::Release);
    }

    /// Get current count
    pub fn count(&self) -> i32 {
        self.count.load(Ordering::Relaxed)
    }
}

/// Barrier for synchronizing multiple threads/tasks
pub struct Barrier {
    threshold: u32,
    count: AtomicU32,
    generation: AtomicU32,
}

impl Barrier {
    /// Create a new barrier
    pub const fn new(threshold: u32) -> Self {
        Self {
            threshold,
            count: AtomicU32::new(0),
            generation: AtomicU32::new(0),
        }
    }

    /// Wait at the barrier
    pub fn wait(&self) -> bool {
        let current_gen = self.generation.load(Ordering::Acquire);
        let count = self.count.fetch_add(1, Ordering::AcqRel);

        if count + 1 >= self.threshold {
            // Last thread to arrive
            self.count.store(0, Ordering::Release);
            self.generation.fetch_add(1, Ordering::Release);
            true
        } else {
            // Wait for all threads
            while self.generation.load(Ordering::Acquire) == current_gen {
                core::hint::spin_loop();
            }
            false
        }
    }

    /// Get the threshold
    pub const fn threshold(&self) -> u32 {
        self.threshold
    }
}

/// Once cell for one-time initialization
pub struct Once {
    state: AtomicU32,
}

const ONCE_INCOMPLETE: u32 = 0;
const ONCE_RUNNING: u32 = 1;
const ONCE_COMPLETE: u32 = 2;

impl Once {
    /// Create a new Once cell
    pub const fn new() -> Self {
        Self {
            state: AtomicU32::new(ONCE_INCOMPLETE),
        }
    }

    /// Call a function once
    pub fn call_once<F>(&self, f: F)
    where
        F: FnOnce(),
    {
        if self.state.load(Ordering::Acquire) == ONCE_COMPLETE {
            return;
        }

        match self.state.compare_exchange(
            ONCE_INCOMPLETE,
            ONCE_RUNNING,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => {
                f();
                self.state.store(ONCE_COMPLETE, Ordering::Release);
            }
            Err(ONCE_RUNNING) => {
                // Another thread is running, wait for completion
                while self.state.load(Ordering::Acquire) != ONCE_COMPLETE {
                    core::hint::spin_loop();
                }
            }
            Err(ONCE_COMPLETE) => {
                // Already complete
            }
            Err(_) => {
                // Unexpected state
            }
        }
    }

    /// Check if already called
    pub fn is_complete(&self) -> bool {
        self.state.load(Ordering::Acquire) == ONCE_COMPLETE
    }
}

impl Default for Once {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atomic_counter() {
        let counter = AtomicCounter::new(0);
        assert_eq!(counter.get(), 0);

        counter.increment();
        counter.increment();
        assert_eq!(counter.get(), 2);

        counter.decrement();
        assert_eq!(counter.get(), 1);

        counter.reset();
        assert_eq!(counter.get(), 0);
    }

    #[test]
    fn test_spinlock() {
        let lock = Spinlock::new();
        assert!(!lock.is_locked());

        lock.try_lock().expect("lock failed");
        assert!(lock.is_locked());
        assert!(lock.try_lock().is_err());

        unsafe { lock.unlock() };
        assert!(!lock.is_locked());
    }

    #[test]
    fn test_mutex() {
        let mutex = Mutex::new(42);
        {
            let guard = mutex.lock();
            assert_eq!(*guard, 42);
        }

        {
            let mut guard = mutex.lock();
            *guard = 100;
        }

        let guard = mutex.lock();
        assert_eq!(*guard, 100);
    }

    #[test]
    fn test_semaphore() {
        let sem = Semaphore::new(2);
        assert_eq!(sem.count(), 2);

        sem.try_acquire().expect("acquire failed");
        assert_eq!(sem.count(), 1);

        sem.try_acquire().expect("acquire failed");
        assert_eq!(sem.count(), 0);

        assert!(sem.try_acquire().is_err());

        sem.release();
        assert_eq!(sem.count(), 1);
    }

    #[test]
    fn test_once() {
        let once = Once::new();
        let counter = AtomicCounter::new(0);

        once.call_once(|| {
            counter.increment();
        });

        once.call_once(|| {
            counter.increment();
        });

        assert_eq!(counter.get(), 1);
        assert!(once.is_complete());
    }
}
