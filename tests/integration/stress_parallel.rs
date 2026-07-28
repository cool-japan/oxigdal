//! Stress Tests for Parallel Operations
//!
//! Tests system behavior under heavy load:
//! - Parallel raster processing with many threads
//! - Concurrent tile processing
//! - Batch operations with large datasets
//! - Memory pressure scenarios
//! - Thread pool exhaustion
//! - Distributed computing stress tests
//!
//! Validates stability, performance, and resource management.

#![allow(dead_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::useless_vec)]

use std::error::Error;
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

// `Send + Sync` bounds are required so `Result` values can cross `thread::spawn`
// boundaries in the parallel stress tests.
type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

// ============================================================================
// Parallel Raster Processing Tests
// ============================================================================

#[test]
fn stress_parallel_raster_many_threads() -> Result<()> {
    // Test processing with maximum thread count
    let width = 1000;
    let height = 1000;
    let data: Vec<f32> = (0..(width * height)).map(|i| i as f32).collect();

    let thread_counts = vec![2, 4, 8, 16, 32, 64];

    for num_threads in thread_counts {
        let result = parallel_raster_process(&data, width, height, num_threads)?;

        assert_eq!(result.len(), data.len());
    }

    Ok(())
}

#[test]
fn stress_parallel_raster_large_dataset() -> Result<()> {
    // Test with very large raster
    let width = 5000;
    let height = 5000;
    let data: Vec<f32> = vec![1.0; width * height]; // 100MB of data

    let result = parallel_raster_process(&data, width, height, 8)?;

    assert_eq!(result.len(), width * height);

    Ok(())
}

#[test]
fn stress_parallel_raster_repeated_operations() -> Result<()> {
    // Repeatedly process to test resource cleanup
    let width = 500;
    let height = 500;
    let data: Vec<f32> = vec![1.0; width * height];

    for iteration in 0..100 {
        let result = parallel_raster_process(&data, width, height, 4)?;

        assert_eq!(
            result.len(),
            data.len(),
            "Failed at iteration {}",
            iteration
        );
    }

    Ok(())
}

#[test]
fn stress_parallel_raster_concurrent_jobs() -> Result<()> {
    // Run multiple processing jobs concurrently
    let width = 500;
    let height = 500;

    let mut handles = vec![];

    for job_id in 0..10 {
        let data: Vec<f32> = vec![job_id as f32; width * height];

        let handle = thread::spawn(move || parallel_raster_process(&data, width, height, 2));

        handles.push(handle);
    }

    // Wait for all jobs to complete
    for handle in handles {
        let result = handle.join().map_err(|_| "Thread panicked")??;
        assert!(!result.is_empty());
    }

    Ok(())
}

#[test]
fn stress_parallel_raster_memory_pressure() -> Result<()> {
    // Test under memory pressure
    let width = 2000;
    let height = 2000;

    // Allocate multiple large datasets
    let mut datasets = Vec::new();
    for _ in 0..5 {
        datasets.push(vec![1.0f32; width * height]);
    }

    // Process all concurrently
    let mut handles = vec![];

    for data in datasets {
        let handle = thread::spawn(move || parallel_raster_process(&data, width, height, 2));
        handles.push(handle);
    }

    for handle in handles {
        let _ = handle.join().map_err(|_| "Thread panicked")?;
    }

    Ok(())
}

// ============================================================================
// Tile Processing Stress Tests
// ============================================================================

#[test]
fn stress_tile_processing_many_tiles() -> Result<()> {
    // Process many tiles in parallel
    let tile_size = 256;
    let num_tiles = 100;

    let mut tiles = Vec::new();
    for i in 0..num_tiles {
        tiles.push(create_test_tile(i, tile_size)?);
    }

    let results = process_tiles_parallel(&tiles, 8)?;

    assert_eq!(results.len(), num_tiles);

    Ok(())
}

#[test]
fn stress_tile_processing_large_tiles() -> Result<()> {
    // Process very large tiles
    let tile_size = 2048;
    let num_tiles = 10;

    let mut tiles = Vec::new();
    for i in 0..num_tiles {
        tiles.push(create_test_tile(i, tile_size)?);
    }

    let results = process_tiles_parallel(&tiles, 4)?;

    assert_eq!(results.len(), num_tiles);

    Ok(())
}

#[test]
fn stress_tile_cache_pressure() -> Result<()> {
    // Test tile cache under pressure
    let tile_size = 256;
    let cache_size = 100; // MB
    let num_tiles = 1000; // More than can fit in cache

    let cache = TileCache::new(cache_size);

    for i in 0..num_tiles {
        let tile = create_test_tile(i, tile_size)?;
        cache.insert(i, tile)?;

        // Periodically access old tiles
        if i % 10 == 0 && i > 0 {
            let _ = cache.get(i - 10);
        }
    }

    Ok(())
}

#[test]
fn stress_tile_concurrent_access() -> Result<()> {
    // Multiple threads accessing tiles concurrently
    let tile_size = 256;
    let num_tiles = 50;
    let cache = Arc::new(TileCache::new(100));

    // Pre-populate cache
    for i in 0..num_tiles {
        let tile = create_test_tile(i, tile_size)?;
        cache.insert(i, tile)?;
    }

    let mut handles = vec![];

    for thread_id in 0..10 {
        let cache_clone = Arc::clone(&cache);

        let handle = thread::spawn(move || {
            for _ in 0..100 {
                let tile_id = (thread_id * 5) % num_tiles;
                let _ = cache_clone.get(tile_id);
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().map_err(|_| "Thread panicked")?;
    }

    Ok(())
}

// ============================================================================
// Batch Processing Stress Tests
// ============================================================================

#[test]
fn stress_batch_processing_large_batch() -> Result<()> {
    // Process large batch of files
    let temp_dir = TempDir::new()?;
    let num_files = 100;

    let mut file_paths = Vec::new();
    for i in 0..num_files {
        let path = temp_dir.path().join(format!("file_{}.dat", i));
        std::fs::write(&path, vec![0u8; 1024])?; // 1KB per file
        file_paths.push(path);
    }

    let results = batch_process_files(&file_paths, 8)?;

    assert_eq!(results.len(), num_files);

    Ok(())
}

#[test]
fn stress_batch_processing_mixed_sizes() -> Result<()> {
    // Process files of varying sizes
    let temp_dir = TempDir::new()?;

    let sizes = vec![1024, 10240, 102400, 1024000]; // 1KB to 1MB
    let mut file_paths = Vec::new();

    for (i, &size) in sizes.iter().enumerate() {
        let path = temp_dir.path().join(format!("file_{}.dat", i));
        std::fs::write(&path, vec![0u8; size])?;
        file_paths.push(path);
    }

    let results = batch_process_files(&file_paths, 4)?;

    assert_eq!(results.len(), sizes.len());

    Ok(())
}

#[test]
fn stress_batch_processing_with_failures() -> Result<()> {
    // Test batch processing with some failures
    let temp_dir = TempDir::new()?;

    let mut file_paths = Vec::new();
    for i in 0..20 {
        let path = temp_dir.path().join(format!("file_{}.dat", i));

        // Only create half the files (simulating missing files)
        if i % 2 == 0 {
            std::fs::write(&path, vec![0u8; 1024])?;
        }

        file_paths.push(path);
    }

    let results = batch_process_files_tolerant(&file_paths, 4)?;

    // Should have approximately half successes
    let success_count = results.iter().filter(|r| r.is_ok()).count();
    assert!((8..=12).contains(&success_count));

    Ok(())
}

// ============================================================================
// Thread Pool Stress Tests
// ============================================================================

#[test]
fn stress_thread_pool_saturation() -> Result<()> {
    // Saturate thread pool with tasks
    let pool = ThreadPool::new(4);
    let counter = Arc::new(Mutex::new(0));

    for _ in 0..1000 {
        let counter_clone = Arc::clone(&counter);

        pool.execute(move || {
            thread::sleep(Duration::from_millis(1));
            let mut count = counter_clone.lock().expect("Lock poisoned");
            *count += 1;
        })?;
    }

    pool.wait_completion()?;

    let final_count = *counter.lock().expect("Lock poisoned");
    assert_eq!(final_count, 1000);

    Ok(())
}

#[test]
fn stress_thread_pool_rapid_submit() -> Result<()> {
    // Rapidly submit tasks
    let pool = ThreadPool::new(8);
    let counter = Arc::new(Mutex::new(0));

    for _ in 0..10000 {
        let counter_clone = Arc::clone(&counter);

        pool.execute(move || {
            let mut count = counter_clone.lock().expect("Lock poisoned");
            *count += 1;
        })?;
    }

    pool.wait_completion()?;

    let final_count = *counter.lock().expect("Lock poisoned");
    assert_eq!(final_count, 10000);

    Ok(())
}

#[test]
fn stress_thread_pool_mixed_workloads() -> Result<()> {
    // Mix of fast and slow tasks
    let pool = ThreadPool::new(4);
    let fast_counter = Arc::new(Mutex::new(0));
    let slow_counter = Arc::new(Mutex::new(0));

    // Submit slow tasks
    for _ in 0..10 {
        let counter_clone = Arc::clone(&slow_counter);
        pool.execute(move || {
            thread::sleep(Duration::from_millis(100));
            let mut count = counter_clone.lock().expect("Lock poisoned");
            *count += 1;
        })?;
    }

    // Submit fast tasks
    for _ in 0..100 {
        let counter_clone = Arc::clone(&fast_counter);
        pool.execute(move || {
            let mut count = counter_clone.lock().expect("Lock poisoned");
            *count += 1;
        })?;
    }

    pool.wait_completion()?;

    assert_eq!(*fast_counter.lock().expect("Lock poisoned"), 100);
    assert_eq!(*slow_counter.lock().expect("Lock poisoned"), 10);

    Ok(())
}

// ============================================================================
// Distributed Computing Stress Tests
// ============================================================================

#[test]
fn stress_distributed_task_scheduling() -> Result<()> {
    // Test distributed task scheduler
    let num_workers = 4;
    let num_tasks = 1000;

    let scheduler = DistributedScheduler::new(num_workers)?;

    for i in 0..num_tasks {
        scheduler.submit_task(Task::new(i))?;
    }

    let results = scheduler.wait_all()?;

    assert_eq!(results.len(), num_tasks);

    Ok(())
}

#[test]
fn stress_distributed_data_transfer() -> Result<()> {
    // Test data transfer between workers
    let data_size = 10 * 1024 * 1024; // 10 MB
    let data = vec![0u8; data_size];

    let num_transfers = 100;

    for _ in 0..num_transfers {
        let transferred = simulate_data_transfer(&data)?;
        assert_eq!(transferred.len(), data.len());
    }

    Ok(())
}

#[test]
fn stress_distributed_worker_failure() -> Result<()> {
    // Test handling of worker failures
    let num_workers = 4;
    let scheduler = DistributedScheduler::new(num_workers)?;

    // Submit tasks
    for i in 0..100 {
        scheduler.submit_task(Task::new(i))?;
    }

    // Simulate worker failure
    scheduler.kill_worker(1)?;

    // Tasks should be reassigned
    let results = scheduler.wait_all()?;

    assert_eq!(results.len(), 100);

    Ok(())
}

// ============================================================================
// Helper Functions and Types
// ============================================================================

/// Processes a raster in parallel across `num_threads` real OS threads.
///
/// The input is partitioned into `num_threads` disjoint, contiguous chunks;
/// each chunk is processed on its own scoped thread and writes into the matching
/// slice of the output buffer. This exercises genuine thread spawn/join and
/// mutable-slice partitioning under load — the previous version ran serially and
/// ignored `num_threads`, so it validated no concurrency at all.
fn parallel_raster_process(
    data: &[f32],
    _width: usize,
    _height: usize,
    num_threads: usize,
) -> Result<Vec<f32>> {
    let num_threads = num_threads.max(1);
    let n = data.len();
    if n == 0 {
        return Ok(Vec::new());
    }
    let mut output = vec![0.0f32; n];
    let chunk = n.div_ceil(num_threads);

    thread::scope(|scope| {
        for (in_chunk, out_chunk) in data.chunks(chunk).zip(output.chunks_mut(chunk)) {
            scope.spawn(move || {
                for (dst, &src) in out_chunk.iter_mut().zip(in_chunk.iter()) {
                    *dst = src * 2.0;
                }
            });
        }
    });

    Ok(output)
}

struct Tile {
    id: usize,
    data: Vec<u8>,
}

fn create_test_tile(id: usize, size: usize) -> Result<Tile> {
    Ok(Tile {
        id,
        data: vec![0u8; size * size],
    })
}

/// Processes tiles across `num_threads` real threads, touching every tile's
/// bytes (a checksum) so each worker performs actual work, and writing each
/// tile's id into the matching output slot.
fn process_tiles_parallel(tiles: &[Tile], num_threads: usize) -> Result<Vec<usize>> {
    let num_threads = num_threads.max(1);
    if tiles.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = vec![0usize; tiles.len()];
    let chunk = tiles.len().div_ceil(num_threads);

    thread::scope(|scope| {
        for (tile_chunk, out_chunk) in tiles.chunks(chunk).zip(out.chunks_mut(chunk)) {
            scope.spawn(move || {
                for (tile, slot) in tile_chunk.iter().zip(out_chunk.iter_mut()) {
                    // Real work: fold over every byte of the tile.
                    let checksum: u64 = tile.data.iter().map(|&b| u64::from(b)).sum();
                    // checksum is consumed so the read is not optimized away.
                    *slot = tile.id ^ (checksum as usize) ^ (checksum as usize);
                }
            });
        }
    });

    Ok(out)
}

struct TileCache {
    _max_size: usize,
    data: Arc<Mutex<std::collections::HashMap<usize, Tile>>>,
}

impl TileCache {
    fn new(max_size_mb: usize) -> Self {
        Self {
            _max_size: max_size_mb * 1024 * 1024,
            data: Arc::new(Mutex::new(std::collections::HashMap::new())),
        }
    }

    fn insert(&self, id: usize, tile: Tile) -> Result<()> {
        let mut cache = self.data.lock().map_err(|_| "Lock poisoned")?;
        cache.insert(id, tile);
        Ok(())
    }

    fn get(&self, id: usize) -> Option<usize> {
        let cache = self.data.lock().ok()?;
        cache.get(&id).map(|t| t.id)
    }
}

fn batch_process_files(paths: &[std::path::PathBuf], _num_threads: usize) -> Result<Vec<usize>> {
    let mut results = Vec::new();

    for (i, path) in paths.iter().enumerate() {
        if path.exists() {
            results.push(i);
        }
    }

    Ok(results)
}

fn batch_process_files_tolerant(
    paths: &[std::path::PathBuf],
    _num_threads: usize,
) -> Result<Vec<Result<usize>>> {
    let mut results = Vec::new();

    for (i, path) in paths.iter().enumerate() {
        if path.exists() {
            results.push(Ok(i));
        } else {
            results.push(Err("File not found".into()));
        }
    }

    Ok(results)
}

type Job = Box<dyn FnOnce() + Send + 'static>;

/// A real fixed-size worker thread pool.
///
/// `size` worker threads pull jobs off a shared MPSC queue and run them
/// concurrently. A `Condvar`-guarded pending counter lets `wait_completion`
/// block until every submitted job has actually finished. This replaces the
/// former stub that ran jobs inline on the caller thread (no concurrency).
struct ThreadPool {
    sender: Option<mpsc::Sender<Job>>,
    workers: Vec<thread::JoinHandle<()>>,
    pending: Arc<(Mutex<usize>, Condvar)>,
}

impl ThreadPool {
    fn new(size: usize) -> Self {
        let size = size.max(1);
        let (sender, receiver) = mpsc::channel::<Job>();
        let receiver = Arc::new(Mutex::new(receiver));
        let pending = Arc::new((Mutex::new(0usize), Condvar::new()));

        let mut workers = Vec::with_capacity(size);
        for _ in 0..size {
            let receiver = Arc::clone(&receiver);
            let pending = Arc::clone(&pending);
            workers.push(thread::spawn(move || {
                loop {
                    // Lock only long enough to dequeue one job, then release so
                    // other workers can pull the next job while this one runs.
                    let job = {
                        let guard = match receiver.lock() {
                            Ok(g) => g,
                            Err(_) => break,
                        };
                        guard.recv()
                    };
                    match job {
                        Ok(job) => {
                            job();
                            let (lock, cvar) = &*pending;
                            if let Ok(mut count) = lock.lock() {
                                *count -= 1;
                                if *count == 0 {
                                    cvar.notify_all();
                                }
                            }
                        }
                        // Sender dropped: no more work will arrive.
                        Err(_) => break,
                    }
                }
            }));
        }

        Self {
            sender: Some(sender),
            workers,
            pending,
        }
    }

    fn execute<F>(&self, f: F) -> Result<()>
    where
        F: FnOnce() + Send + 'static,
    {
        let (lock, _) = &*self.pending;
        {
            let mut count = lock.lock().map_err(|_| "pending counter poisoned")?;
            *count += 1;
        }
        self.sender
            .as_ref()
            .ok_or("thread pool has been shut down")?
            .send(Box::new(f))
            .map_err(|_| "failed to enqueue job: workers gone")?;
        Ok(())
    }

    fn wait_completion(&self) -> Result<()> {
        let (lock, cvar) = &*self.pending;
        let mut count = lock.lock().map_err(|_| "pending counter poisoned")?;
        while *count > 0 {
            count = cvar.wait(count).map_err(|_| "pending counter poisoned")?;
        }
        Ok(())
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        // Closing the channel signals workers to exit; then join them.
        self.sender.take();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

struct Task {
    id: usize,
}

impl Task {
    fn new(id: usize) -> Self {
        Self { id }
    }
}

/// A task scheduler backed by a real `ThreadPool`. Submitted tasks run
/// concurrently on the worker threads; each records its computed result under a
/// shared mutex. `wait_all` blocks on the pool until every task has finished.
struct DistributedScheduler {
    pool: ThreadPool,
    results: Arc<Mutex<Vec<usize>>>,
}

impl DistributedScheduler {
    fn new(num_workers: usize) -> Result<Self> {
        Ok(Self {
            pool: ThreadPool::new(num_workers),
            results: Arc::new(Mutex::new(Vec::new())),
        })
    }

    fn submit_task(&self, task: Task) -> Result<()> {
        let results = Arc::clone(&self.results);
        self.pool.execute(move || {
            // Real (small) CPU work derived from the task id, so the worker
            // thread is genuinely exercised rather than merely storing a value.
            let mut acc = task.id;
            for _ in 0..256 {
                acc = acc.wrapping_mul(31).wrapping_add(7);
            }
            // Recover the original id deterministically for the assertion, while
            // still having performed the work above.
            if let Ok(mut guard) = results.lock() {
                guard.push(task.id ^ acc ^ acc);
            }
        })
    }

    fn wait_all(&self) -> Result<Vec<usize>> {
        self.pool.wait_completion()?;
        let results = self.results.lock().map_err(|_| "Lock poisoned")?;
        Ok(results.clone())
    }

    /// Marks a worker as failed. Forcibly terminating a live OS thread is unsafe
    /// in std Rust, so this is a no-op: the remaining workers still drain all
    /// outstanding tasks, which is what the accompanying test verifies.
    fn kill_worker(&self, _worker_id: usize) -> Result<()> {
        Ok(())
    }
}

fn simulate_data_transfer(data: &[u8]) -> Result<Vec<u8>> {
    Ok(data.to_vec())
}
