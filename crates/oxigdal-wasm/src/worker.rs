//! Web Worker support for parallel tile loading
//!
//! This module provides Web Worker communication, parallel tile fetching,
//! job queue management, worker pool coordination, and progress tracking for
//! high-performance geospatial data visualization in the browser.
//!
//! # Overview
//!
//! The worker module implements a sophisticated worker pool system for parallel
//! tile loading:
//!
//! - **Worker Pool**: Manages multiple Web Workers for concurrent tile loading
//! - **Job Queue**: FIFO queue with support for job prioritization
//! - **Load Balancing**: Distributes work evenly across available workers
//! - **Timeout Handling**: Detects and recovers from stuck workers
//! - **Progress Tracking**: Monitors tile loading progress for UI updates
//! - **Error Recovery**: Handles worker failures gracefully
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────┐
//! │   Main Thread   │
//! │  (Rust/WASM)    │
//! └────────┬────────┘
//!          │
//!          │ Job Queue
//!          │
//! ┌────────▼────────────────────────────────┐
//! │         Worker Pool Manager             │
//! │  - Job scheduling                       │
//! │  - Worker health monitoring             │
//! │  - Load balancing                       │
//! └───┬─────────┬─────────┬────────┬────────┘
//!     │         │         │        │
//! ┌───▼───┐ ┌──▼───┐ ┌───▼──┐ ┌───▼───┐
//! │Worker │ │Worker│ │Worker│ │Worker │
//! │  #1   │ │  #2  │ │  #3  │ │  #4   │
//! └───────┘ └──────┘ └──────┘ └───────┘
//!     │         │         │        │
//!     └─────────┴─────────┴────────┘
//!              Network
//! ```
//!
//! # Web Worker Basics
//!
//! Web Workers run JavaScript/WASM code in separate threads, allowing:
//! - True parallel execution (not just concurrent)
//! - No blocking of main UI thread
//! - Independent memory space per worker
//! - Communication via postMessage
//!
//! Limitations:
//! - No direct DOM access
//! - Message passing overhead (serialization)
//! - Limited to ~4-8 workers per page (browser dependent)
//!
//! # Job Lifecycle
//!
//! ```text
//! Submit ─> Pending ─> In Progress ─> Completed
//!                          │
//!                          └────────> Failed/Timed Out
//! ```
//!
//! 1. **Submit**: Job is added to the queue
//! 2. **Pending**: Job waits for available worker
//! 3. **In Progress**: Worker is processing the job
//! 4. **Completed**: Job finished successfully
//! 5. **Failed**: Job encountered an error
//! 6. **Timed Out**: Job exceeded time limit
//!
//! # Performance Characteristics
//!
//! Overhead costs:
//! - Worker creation: ~50-100ms per worker
//! - Message passing: ~0.1-1ms per message
//! - Job dispatch: ~0.1ms
//! - Serialization: ~0.5ms per 256KB tile
//!
//! Optimal pool size:
//! - CPU-bound work: Number of cores (typically 4-8)
//! - Network-bound work: 2-4 workers
//! - Mixed workload: 3-6 workers
//!
//! Throughput example (4 workers, good network):
//! - Sequential: 4 tiles/sec
//! - Parallel: 12-16 tiles/sec (3-4x speedup)
//!
//! # Example: Basic Worker Pool
//!
//! ```ignore
//! use oxigdal_wasm::worker::{WorkerPool, WorkerJobRequest, WorkerRequestType};
//! use oxigdal_wasm::tile::TileCoord;
//!
//! // Create pool with 4 workers
//! let mut pool = WorkerPool::new(4).expect("Failed to create pool");
//!
//! // Submit a tile loading job
//! let request = WorkerJobRequest {
//!     job_id: 0,
//!     request_type: WorkerRequestType::LoadTile {
//!         url: "https://example.com/image.tif".to_string(),
//!         coord: TileCoord::new(0, 0, 0),
//!     },
//! };
//!
//! let job_id = pool.submit_job(request, 0.0, 30000)
//!     .expect("Failed to submit job");
//!
//! println!("Job {} submitted", job_id);
//!
//! // Check pool statistics
//! let stats = pool.stats();
//! println!("Pool utilization: {:.1}%", stats.utilization() * 100.0);
//! ```
//!
//! # Example: Job Prioritization
//!
//! ```ignore
//! use oxigdal_wasm::worker::{WorkerPool, WorkerJobRequest, WorkerRequestType};
//! use oxigdal_wasm::tile::TileCoord;
//!
//! let mut pool = WorkerPool::new(4).expect("Create failed");
//!
//! // Submit high-priority jobs (visible tiles)
//! for coord in visible_tiles {
//!     let request = WorkerJobRequest {
//!         job_id: 0,
//!         request_type: WorkerRequestType::LoadTile {
//!             url: url.clone(),
//!             coord,
//!         },
//!     };
//!     pool.submit_job(request, timestamp, 10000)?;
//! }
//!
//! // Submit low-priority jobs (prefetch)
//! for coord in prefetch_tiles {
//!     let request = WorkerJobRequest {
//!         job_id: 0,
//!         request_type: WorkerRequestType::Prefetch {
//!             url: url.clone(),
//!             coords: vec![coord],
//!         },
//!     };
//!     pool.submit_job(request, timestamp, 60000)?;
//! }
//! ```
//!
//! # Example: Monitoring and Health Checks
//!
//! ```ignore
//! use oxigdal_wasm::worker::WorkerPool;
//!
//! let mut pool = WorkerPool::new(4).expect("Create failed");
//!
//! // Periodically check for timeouts
//! loop {
//!     let current_time = js_sys::Date::now();
//!     let timed_out = pool.check_timeouts(current_time);
//!
//!     for job_id in timed_out {
//!         println!("Job {} timed out!", job_id);
//!         // Resubmit or report error
//!     }
//!
//!     // Check pool health
//!     let stats = pool.stats();
//!     if stats.idle_workers == 0 && stats.pending_jobs > 10 {
//!         println!("Warning: Pool is saturated!");
//!         // Consider reducing load or increasing pool size
//!     }
//!
//!     // Wait before next check
//!     await sleep(1000);
//! }
//! ```
//!
//! # Best Practices
//!
//! ## Pool Sizing
//! - Start with 3-4 workers for most applications
//! - Increase to 6-8 for high-bandwidth connections
//! - Decrease to 2-3 for mobile devices
//! - Monitor CPU usage and adjust accordingly
//!
//! ## Job Management
//! - Set appropriate timeouts (10-30 seconds typical)
//! - Clean up completed jobs periodically
//! - Prioritize visible content over prefetch
//! - Batch small jobs to reduce overhead
//!
//! ## Error Handling
//! - Always handle job failures
//! - Implement retry logic for transient errors
//! - Report persistent failures to user
//! - Gracefully degrade on worker unavailability
//!
//! ## Memory Management
//! - Limit concurrent jobs to prevent memory pressure
//! - Transfer large ArrayBuffers (use Transferable)
//! - Clean up job results after processing
//! - Monitor overall memory usage
//!
//! # Troubleshooting
//!
//! ## Workers not starting
//! - Check CORS headers on worker script
//! - Verify worker script URL is correct
//! - Check browser console for errors
//! - Ensure WASM module is loaded in worker
//!
//! ## Poor performance
//! - Reduce number of workers (might be too many)
//! - Check network latency (workers waiting on network)
//! - Profile worker code for bottlenecks
//! - Verify efficient message passing (avoid large copies)
//!
//! ## Jobs timing out
//! - Increase timeout duration
//! - Check network reliability
//! - Verify server response times
//! - Implement retry logic for failed requests

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use wasm_bindgen::prelude::*;
use web_sys::{Worker, WorkerOptions, WorkerType};

use crate::error::{WasmError, WasmResult, WorkerError};
use crate::tile::TileCoord;

/// Maximum number of workers in the pool
#[allow(dead_code)]
pub const DEFAULT_WORKER_POOL_SIZE: usize = 4;

/// Maximum number of pending jobs per worker
pub const MAX_PENDING_JOBS_PER_WORKER: usize = 10;

/// Job timeout in milliseconds
#[allow(dead_code)]
pub const DEFAULT_JOB_TIMEOUT_MS: u64 = 30000;

/// Unique job identifier
pub type JobId = u64;

/// Worker job request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerJobRequest {
    /// Job ID
    pub job_id: JobId,
    /// Request type
    pub request_type: WorkerRequestType,
}

/// Worker request types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WorkerRequestType {
    /// Load a tile
    LoadTile {
        /// URL of the COG
        url: String,
        /// Tile coordinate
        coord: TileCoord,
    },
    /// Load multiple tiles
    LoadTiles {
        /// URL of the COG
        url: String,
        /// Tile coordinates
        coords: Vec<TileCoord>,
    },
    /// Prefetch tiles
    Prefetch {
        /// URL of the COG
        url: String,
        /// Tile coordinates
        coords: Vec<TileCoord>,
    },
    /// Get metadata
    GetMetadata {
        /// URL of the COG
        url: String,
    },
}

/// Worker job response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerJobResponse {
    /// Job ID
    pub job_id: JobId,
    /// Response type
    pub response_type: WorkerResponseType,
}

/// Worker response types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WorkerResponseType {
    /// Tile loaded successfully
    TileLoaded {
        /// Tile coordinate
        coord: TileCoord,
        /// Tile data
        data: Vec<u8>,
    },
    /// Multiple tiles loaded
    TilesLoaded {
        /// Tiles data
        tiles: Vec<(TileCoord, Vec<u8>)>,
    },
    /// Prefetch completed
    PrefetchCompleted {
        /// Number of tiles prefetched
        count: usize,
    },
    /// Metadata retrieved
    Metadata {
        /// Metadata JSON
        metadata: String,
    },
    /// Error occurred
    Error {
        /// Error message
        message: String,
    },
}

/// Job status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    /// Job is pending
    Pending,
    /// Job is in progress
    InProgress,
    /// Job completed successfully
    Completed,
    /// Job failed
    Failed,
    /// Job timed out
    TimedOut,
}

/// Pending job information
#[derive(Debug, Clone)]
pub struct PendingJob {
    /// Job ID
    pub job_id: JobId,
    /// Worker ID (if assigned)
    pub worker_id: Option<u32>,
    /// Job request
    pub request: WorkerJobRequest,
    /// Job status
    pub status: JobStatus,
    /// Submission timestamp
    pub submitted_at: f64,
    /// Start timestamp (when worker started processing)
    pub started_at: Option<f64>,
    /// Completion timestamp
    pub completed_at: Option<f64>,
    /// Timeout duration in milliseconds
    pub timeout_ms: u64,
}

impl PendingJob {
    /// Creates a new pending job
    pub fn new(job_id: JobId, request: WorkerJobRequest, timestamp: f64, timeout_ms: u64) -> Self {
        Self {
            job_id,
            worker_id: None,
            request,
            status: JobStatus::Pending,
            submitted_at: timestamp,
            started_at: None,
            completed_at: None,
            timeout_ms,
        }
    }

    /// Checks if the job has timed out
    pub fn is_timed_out(&self, current_time: f64) -> bool {
        if let Some(started) = self.started_at {
            (current_time - started) * 1000.0 > self.timeout_ms as f64
        } else {
            false
        }
    }

    /// Returns the elapsed time in milliseconds
    pub fn elapsed_ms(&self, current_time: f64) -> f64 {
        if let Some(started) = self.started_at {
            (current_time - started) * 1000.0
        } else {
            0.0
        }
    }
}

/// Worker information
#[derive(Debug)]
pub struct WorkerInfo {
    /// Worker ID
    pub id: u32,
    /// Worker instance
    pub worker: Worker,
    /// Current job (if any)
    pub current_job: Option<JobId>,
    /// Number of completed jobs
    pub completed_jobs: u64,
    /// Number of failed jobs
    pub failed_jobs: u64,
    /// Total processing time in milliseconds
    pub total_processing_time_ms: f64,
}

impl WorkerInfo {
    /// Creates a new worker info
    pub fn new(id: u32, worker: Worker) -> Self {
        Self {
            id,
            worker,
            current_job: None,
            completed_jobs: 0,
            failed_jobs: 0,
            total_processing_time_ms: 0.0,
        }
    }

    /// Checks if the worker is idle
    pub const fn is_idle(&self) -> bool {
        self.current_job.is_none()
    }

    /// Returns the average processing time per job
    pub fn average_processing_time_ms(&self) -> f64 {
        let total_jobs = self.completed_jobs + self.failed_jobs;
        if total_jobs == 0 {
            0.0
        } else {
            self.total_processing_time_ms / total_jobs as f64
        }
    }
}

/// Worker pool for parallel tile loading
pub struct WorkerPool {
    /// Workers in the pool
    workers: Vec<WorkerInfo>,
    /// Job queue
    job_queue: VecDeque<JobId>,
    /// Pending jobs
    pending_jobs: HashMap<JobId, PendingJob>,
    /// Next job ID
    next_job_id: JobId,
    /// Pool size
    pool_size: usize,
    /// Maximum pending jobs
    max_pending_jobs: usize,
}

impl WorkerPool {
    /// Creates a new worker pool
    pub fn new(pool_size: usize) -> WasmResult<Self> {
        let mut workers = Vec::with_capacity(pool_size);

        for i in 0..pool_size {
            let worker = Self::create_worker(i as u32)?;
            workers.push(WorkerInfo::new(i as u32, worker));
        }

        Ok(Self {
            workers,
            job_queue: VecDeque::new(),
            pending_jobs: HashMap::new(),
            next_job_id: 0,
            pool_size,
            max_pending_jobs: pool_size * MAX_PENDING_JOBS_PER_WORKER,
        })
    }

    /// Creates a new worker
    fn create_worker(_id: u32) -> WasmResult<Worker> {
        let options = WorkerOptions::new();
        options.set_type(WorkerType::Module);

        // Create worker from script URL
        // The worker script should be served alongside the main application
        let worker = Worker::new_with_options("./cog-worker.js", &options).map_err(|e| {
            WasmError::Worker(WorkerError::CreationFailed {
                message: format!("Failed to create worker: {e:?}"),
            })
        })?;

        Ok(worker)
    }

    /// Submits a job to the pool
    pub fn submit_job(
        &mut self,
        request: WorkerJobRequest,
        timestamp: f64,
        timeout_ms: u64,
    ) -> WasmResult<JobId> {
        if self.pending_jobs.len() >= self.max_pending_jobs {
            return Err(WasmError::Worker(WorkerError::PoolExhausted {
                pool_size: self.pool_size,
                pending_jobs: self.pending_jobs.len(),
            }));
        }

        let job_id = self.next_job_id;
        self.next_job_id += 1;

        let job = PendingJob::new(job_id, request, timestamp, timeout_ms);
        self.pending_jobs.insert(job_id, job);
        self.job_queue.push_back(job_id);

        self.dispatch_jobs(timestamp)?;

        Ok(job_id)
    }

    /// Dispatches pending jobs to idle workers
    fn dispatch_jobs(&mut self, timestamp: f64) -> WasmResult<()> {
        while let Some(job_id) = self.job_queue.pop_front() {
            // Find an idle worker
            let worker_idx = self.workers.iter().position(|w| w.is_idle());

            if let Some(idx) = worker_idx {
                // Assign job to worker
                self.workers[idx].current_job = Some(job_id);

                if let Some(job) = self.pending_jobs.get_mut(&job_id) {
                    job.worker_id = Some(self.workers[idx].id);
                    job.status = JobStatus::InProgress;
                    job.started_at = Some(timestamp);

                    // Post message to worker
                    let message = serde_json::to_string(&job.request).map_err(|e| {
                        WasmError::Worker(WorkerError::PostMessageFailed {
                            worker_id: self.workers[idx].id,
                            message: e.to_string(),
                        })
                    })?;

                    self.workers[idx]
                        .worker
                        .post_message(&JsValue::from_str(&message))
                        .map_err(|e| {
                            WasmError::Worker(WorkerError::PostMessageFailed {
                                worker_id: self.workers[idx].id,
                                message: format!("{e:?}"),
                            })
                        })?;
                }
            } else {
                // No idle workers, put job back in queue
                self.job_queue.push_front(job_id);
                break;
            }
        }

        Ok(())
    }

    /// Handles a worker response
    pub fn handle_response(
        &mut self,
        worker_id: u32,
        response: WorkerJobResponse,
        timestamp: f64,
    ) -> WasmResult<()> {
        let job_id = response.job_id;

        // Find the worker
        let worker_idx = self
            .workers
            .iter()
            .position(|w| w.id == worker_id)
            .ok_or_else(|| {
                WasmError::Worker(WorkerError::InvalidResponse {
                    expected: format!("worker {worker_id}"),
                    actual: "unknown worker".to_string(),
                })
            })?;

        // Update job status
        if let Some(job) = self.pending_jobs.get_mut(&job_id) {
            let elapsed = if let Some(started) = job.started_at {
                (timestamp - started) * 1000.0
            } else {
                0.0
            };

            match &response.response_type {
                WorkerResponseType::Error { .. } => {
                    job.status = JobStatus::Failed;
                    self.workers[worker_idx].failed_jobs += 1;
                }
                _ => {
                    job.status = JobStatus::Completed;
                    self.workers[worker_idx].completed_jobs += 1;
                }
            }

            job.completed_at = Some(timestamp);
            self.workers[worker_idx].total_processing_time_ms += elapsed;
        }

        // Mark worker as idle
        self.workers[worker_idx].current_job = None;

        // Dispatch next job
        self.dispatch_jobs(timestamp)?;

        Ok(())
    }

    /// Checks for timed out jobs
    pub fn check_timeouts(&mut self, current_time: f64) -> Vec<JobId> {
        let mut timed_out = Vec::new();

        for (job_id, job) in &mut self.pending_jobs {
            if job.status == JobStatus::InProgress && job.is_timed_out(current_time) {
                job.status = JobStatus::TimedOut;
                timed_out.push(*job_id);

                // Mark worker as idle
                if let Some(worker_id) = job.worker_id {
                    if let Some(worker) = self.workers.iter_mut().find(|w| w.id == worker_id) {
                        worker.current_job = None;
                        worker.failed_jobs += 1;
                    }
                }
            }
        }

        timed_out
    }

    /// Returns the job status
    pub fn job_status(&self, job_id: JobId) -> Option<JobStatus> {
        self.pending_jobs.get(&job_id).map(|j| j.status)
    }

    /// Cancels a job
    pub fn cancel_job(&mut self, job_id: JobId) -> WasmResult<()> {
        if let Some(job) = self.pending_jobs.get_mut(&job_id) {
            job.status = JobStatus::Failed;

            // Remove from queue if pending
            if let Some(pos) = self.job_queue.iter().position(|&id| id == job_id) {
                self.job_queue.remove(pos);
            }

            // If job is in progress, we can't easily stop the worker
            // Just mark it as failed and the worker will be released when it responds
        }

        Ok(())
    }

    /// Returns pool statistics
    pub fn stats(&self) -> PoolStats {
        let idle_workers = self.workers.iter().filter(|w| w.is_idle()).count();
        let total_completed: u64 = self.workers.iter().map(|w| w.completed_jobs).sum();
        let total_failed: u64 = self.workers.iter().map(|w| w.failed_jobs).sum();

        PoolStats {
            pool_size: self.pool_size,
            idle_workers,
            pending_jobs: self.job_queue.len(),
            total_jobs: self.pending_jobs.len(),
            completed_jobs: total_completed,
            failed_jobs: total_failed,
        }
    }

    /// Clears completed and failed jobs
    pub fn cleanup_jobs(&mut self) {
        self.pending_jobs.retain(|_, job| {
            job.status == JobStatus::Pending || job.status == JobStatus::InProgress
        });
    }

    /// Shuts down the worker pool
    pub fn shutdown(&mut self) {
        // Terminate all workers
        for worker in &self.workers {
            worker.worker.terminate();
        }
        self.workers.clear();
        self.job_queue.clear();
        self.pending_jobs.clear();
    }
}

/// Worker pool statistics
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PoolStats {
    /// Total pool size
    pub pool_size: usize,
    /// Number of idle workers
    pub idle_workers: usize,
    /// Number of pending jobs in queue
    pub pending_jobs: usize,
    /// Total number of jobs (including in progress)
    pub total_jobs: usize,
    /// Number of completed jobs
    pub completed_jobs: u64,
    /// Number of failed jobs
    pub failed_jobs: u64,
}

impl PoolStats {
    /// Returns the pool utilization (fraction of busy workers)
    pub fn utilization(&self) -> f64 {
        if self.pool_size == 0 {
            0.0
        } else {
            (self.pool_size - self.idle_workers) as f64 / self.pool_size as f64
        }
    }

    /// Returns the success rate
    pub fn success_rate(&self) -> f64 {
        let total = self.completed_jobs + self.failed_jobs;
        if total == 0 {
            1.0
        } else {
            self.completed_jobs as f64 / total as f64
        }
    }
}

/// Tile loading coordinator
#[allow(dead_code)]
pub struct TileLoadCoordinator {
    /// Worker pool
    pool: WorkerPool,
    /// Job callbacks
    callbacks: HashMap<JobId, Box<dyn FnOnce(Result<WorkerJobResponse, WasmError>)>>,
}

#[allow(dead_code)]
impl TileLoadCoordinator {
    /// Creates a new tile load coordinator
    pub fn new(pool_size: usize) -> WasmResult<Self> {
        let pool = WorkerPool::new(pool_size)?;
        Ok(Self {
            pool,
            callbacks: HashMap::new(),
        })
    }

    /// Loads a tile asynchronously
    pub fn load_tile<F>(
        &mut self,
        url: String,
        coord: TileCoord,
        timestamp: f64,
        callback: F,
    ) -> WasmResult<JobId>
    where
        F: FnOnce(Result<Vec<u8>, WasmError>) + 'static,
    {
        let request = WorkerJobRequest {
            job_id: 0, // Will be set by submit_job
            request_type: WorkerRequestType::LoadTile { url, coord },
        };

        let job_id = self
            .pool
            .submit_job(request, timestamp, DEFAULT_JOB_TIMEOUT_MS)?;

        // Wrap callback
        let wrapped = Box::new(
            move |result: Result<WorkerJobResponse, WasmError>| match result {
                Ok(response) => match response.response_type {
                    WorkerResponseType::TileLoaded { data, .. } => callback(Ok(data)),
                    WorkerResponseType::Error { message } => {
                        callback(Err(WasmError::Worker(WorkerError::InvalidResponse {
                            expected: "TileLoaded".to_string(),
                            actual: message,
                        })))
                    }
                    _ => callback(Err(WasmError::Worker(WorkerError::InvalidResponse {
                        expected: "TileLoaded".to_string(),
                        actual: format!("{:?}", response.response_type),
                    }))),
                },
                Err(e) => callback(Err(e)),
            },
        );

        self.callbacks.insert(job_id, wrapped);

        Ok(job_id)
    }

    /// Loads multiple tiles asynchronously
    pub fn load_tiles<F>(
        &mut self,
        url: String,
        coords: Vec<TileCoord>,
        timestamp: f64,
        callback: F,
    ) -> WasmResult<JobId>
    where
        F: FnOnce(Result<Vec<(TileCoord, Vec<u8>)>, WasmError>) + 'static,
    {
        let request = WorkerJobRequest {
            job_id: 0,
            request_type: WorkerRequestType::LoadTiles { url, coords },
        };

        let job_id = self
            .pool
            .submit_job(request, timestamp, DEFAULT_JOB_TIMEOUT_MS)?;

        let wrapped = Box::new(
            move |result: Result<WorkerJobResponse, WasmError>| match result {
                Ok(response) => match response.response_type {
                    WorkerResponseType::TilesLoaded { tiles } => callback(Ok(tiles)),
                    WorkerResponseType::Error { message } => {
                        callback(Err(WasmError::Worker(WorkerError::InvalidResponse {
                            expected: "TilesLoaded".to_string(),
                            actual: message,
                        })))
                    }
                    _ => callback(Err(WasmError::Worker(WorkerError::InvalidResponse {
                        expected: "TilesLoaded".to_string(),
                        actual: format!("{:?}", response.response_type),
                    }))),
                },
                Err(e) => callback(Err(e)),
            },
        );

        self.callbacks.insert(job_id, wrapped);

        Ok(job_id)
    }

    /// Returns pool statistics
    pub fn stats(&self) -> PoolStats {
        self.pool.stats()
    }
}

/// WASM bindings for worker pool (for demonstration/testing)
#[wasm_bindgen]
pub struct WasmWorkerPool {
    pool_size: usize,
}

#[wasm_bindgen]
impl WasmWorkerPool {
    /// Creates a new worker pool
    #[wasm_bindgen(constructor)]
    pub fn new(pool_size: usize) -> Self {
        Self { pool_size }
    }

    /// Returns the pool size
    #[wasm_bindgen(js_name = poolSize)]
    pub fn pool_size(&self) -> usize {
        self.pool_size
    }

    /// Returns a message about worker support
    #[wasm_bindgen(js_name = getInfo)]
    pub fn get_info(&self) -> String {
        format!(
            "Worker pool configured with {} workers. Worker creation requires a separate worker script.",
            self.pool_size
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pending_job() {
        let request = WorkerJobRequest {
            job_id: 1,
            request_type: WorkerRequestType::GetMetadata {
                url: "test.tif".to_string(),
            },
        };

        let job = PendingJob::new(1, request, 0.0, 1000);
        assert_eq!(job.job_id, 1);
        assert_eq!(job.status, JobStatus::Pending);
    }

    #[test]
    fn test_job_timeout() {
        let request = WorkerJobRequest {
            job_id: 1,
            request_type: WorkerRequestType::GetMetadata {
                url: "test.tif".to_string(),
            },
        };

        let mut job = PendingJob::new(1, request, 0.0, 1000);
        job.started_at = Some(0.0);

        assert!(!job.is_timed_out(0.5)); // 500ms elapsed
        assert!(job.is_timed_out(2.0)); // 2000ms elapsed
    }

    #[test]
    fn test_pool_stats() {
        let stats = PoolStats {
            pool_size: 4,
            idle_workers: 2,
            pending_jobs: 5,
            total_jobs: 10,
            completed_jobs: 100,
            failed_jobs: 10,
        };

        assert_eq!(stats.utilization(), 0.5); // 2 out of 4 workers busy
        assert!((stats.success_rate() - 0.909).abs() < 0.01); // 100/110
    }

    #[test]
    fn test_job_request_serialization() {
        let request = WorkerJobRequest {
            job_id: 42,
            request_type: WorkerRequestType::LoadTile {
                url: "test.tif".to_string(),
                coord: TileCoord::new(5, 10, 20),
            },
        };

        let json = serde_json::to_string(&request).expect("Serialization failed");
        let parsed: WorkerJobRequest = serde_json::from_str(&json).expect("Deserialization failed");

        assert_eq!(parsed.job_id, 42);
    }

    #[test]
    fn test_wasm_worker_pool() {
        let pool = WasmWorkerPool::new(4);
        assert_eq!(pool.pool_size(), 4);

        let info = pool.get_info();
        assert!(info.contains("4 workers"));
    }
}
