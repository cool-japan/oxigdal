//! Parallel execution planning and resource allocation.

use crate::dag::graph::{ResourceRequirements, WorkflowDag};
use crate::dag::topological_sort::create_execution_plan;
use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Resource pool for parallel execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourcePool {
    /// Total CPU cores available.
    pub total_cpu_cores: f64,
    /// Total memory in MB.
    pub total_memory_mb: u64,
    /// Number of GPUs available.
    pub total_gpus: u32,
    /// Total disk space in MB.
    pub total_disk_mb: u64,
    /// Custom resources.
    pub custom_resources: HashMap<String, f64>,
}

impl Default for ResourcePool {
    fn default() -> Self {
        Self {
            total_cpu_cores: num_cpus::get() as f64,
            total_memory_mb: 8192,
            total_gpus: 0,
            total_disk_mb: 102400,
            custom_resources: HashMap::new(),
        }
    }
}

/// Available resources at a point in time.
#[derive(Debug, Clone)]
pub struct AvailableResources {
    /// CPU cores available.
    pub cpu_cores: f64,
    /// Memory available in MB.
    pub memory_mb: u64,
    /// GPUs available.
    pub gpus: u32,
    /// Disk space available in MB.
    pub disk_mb: u64,
    /// Custom resources available.
    pub custom_resources: HashMap<String, f64>,
}

impl From<ResourcePool> for AvailableResources {
    fn from(pool: ResourcePool) -> Self {
        Self {
            cpu_cores: pool.total_cpu_cores,
            memory_mb: pool.total_memory_mb,
            gpus: pool.total_gpus,
            disk_mb: pool.total_disk_mb,
            custom_resources: pool.custom_resources,
        }
    }
}

impl AvailableResources {
    /// Check if the required resources can be allocated.
    pub fn can_allocate(&self, requirements: &ResourceRequirements) -> bool {
        if self.cpu_cores < requirements.cpu_cores {
            return false;
        }
        if self.memory_mb < requirements.memory_mb {
            return false;
        }
        if requirements.gpu && self.gpus == 0 {
            return false;
        }
        if self.disk_mb < requirements.disk_mb {
            return false;
        }

        // Check custom resources
        for (key, &required_value) in &requirements.custom {
            if let Some(&available_value) = self.custom_resources.get(key) {
                if available_value < required_value {
                    return false;
                }
            } else {
                return false;
            }
        }

        true
    }

    /// Allocate resources.
    pub fn allocate(&mut self, requirements: &ResourceRequirements) -> bool {
        if !self.can_allocate(requirements) {
            return false;
        }

        self.cpu_cores -= requirements.cpu_cores;
        self.memory_mb -= requirements.memory_mb;
        if requirements.gpu {
            self.gpus -= 1;
        }
        self.disk_mb -= requirements.disk_mb;

        for (key, &value) in &requirements.custom {
            if let Some(available) = self.custom_resources.get_mut(key) {
                *available -= value;
            }
        }

        true
    }

    /// Release resources.
    pub fn release(&mut self, requirements: &ResourceRequirements) {
        self.cpu_cores += requirements.cpu_cores;
        self.memory_mb += requirements.memory_mb;
        if requirements.gpu {
            self.gpus += 1;
        }
        self.disk_mb += requirements.disk_mb;

        for (key, &value) in &requirements.custom {
            *self.custom_resources.entry(key.clone()).or_insert(0.0) += value;
        }
    }
}

/// Parallel execution schedule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParallelSchedule {
    /// Execution waves - each wave contains tasks that can run in parallel.
    pub waves: Vec<ExecutionWave>,
    /// Total estimated execution time in seconds.
    pub estimated_time_secs: u64,
    /// Maximum parallelism (max tasks running at once).
    pub max_parallelism: usize,
}

/// A wave of parallel task execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionWave {
    /// Task IDs in this wave.
    pub task_ids: Vec<String>,
    /// Estimated execution time for this wave.
    pub estimated_time_secs: u64,
}

/// Create a parallel execution schedule considering resource constraints.
pub fn create_parallel_schedule(
    dag: &WorkflowDag,
    resource_pool: &ResourcePool,
) -> Result<ParallelSchedule> {
    let execution_plan = create_execution_plan(dag)?;
    let mut waves = Vec::new();
    let mut total_time = 0u64;
    let mut max_parallelism = 0usize;

    for level in execution_plan {
        let mut available_resources = AvailableResources::from(resource_pool.clone());
        let mut current_wave = Vec::new();
        let mut waiting_tasks = level.clone();
        let mut wave_time = 0u64;

        // First pass: allocate resources to as many tasks as possible
        let mut i = 0;
        while i < waiting_tasks.len() {
            let task_id = &waiting_tasks[i];
            if let Some(task) = dag.get_task(task_id) {
                if available_resources.can_allocate(&task.resources) {
                    available_resources.allocate(&task.resources);
                    current_wave.push(task_id.clone());
                    wave_time = wave_time.max(task.timeout_secs.unwrap_or(60));
                    waiting_tasks.remove(i);
                } else {
                    i += 1;
                }
            } else {
                i += 1;
            }
        }

        if !current_wave.is_empty() {
            max_parallelism = max_parallelism.max(current_wave.len());
            waves.push(ExecutionWave {
                task_ids: current_wave,
                estimated_time_secs: wave_time,
            });
            total_time += wave_time;
        }

        // Process remaining tasks that couldn't fit in the first wave
        while !waiting_tasks.is_empty() {
            let mut available_resources = AvailableResources::from(resource_pool.clone());
            let mut current_wave = Vec::new();
            let mut wave_time = 0u64;
            let mut i = 0;

            while i < waiting_tasks.len() {
                let task_id = &waiting_tasks[i];
                if let Some(task) = dag.get_task(task_id) {
                    if available_resources.can_allocate(&task.resources) {
                        available_resources.allocate(&task.resources);
                        current_wave.push(task_id.clone());
                        wave_time = wave_time.max(task.timeout_secs.unwrap_or(60));
                        waiting_tasks.remove(i);
                    } else {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }

            if !current_wave.is_empty() {
                max_parallelism = max_parallelism.max(current_wave.len());
                waves.push(ExecutionWave {
                    task_ids: current_wave,
                    estimated_time_secs: wave_time,
                });
                total_time += wave_time;
            } else {
                // Can't make progress, break to avoid infinite loop
                break;
            }
        }
    }

    Ok(ParallelSchedule {
        waves,
        estimated_time_secs: total_time,
        max_parallelism,
    })
}

/// Calculate resource utilization over time.
pub fn calculate_resource_utilization(
    dag: &WorkflowDag,
    schedule: &ParallelSchedule,
) -> Vec<ResourceUtilization> {
    let mut utilization = Vec::new();
    let mut current_time = 0u64;

    for wave in &schedule.waves {
        let mut cpu_used = 0.0;
        let mut memory_used = 0u64;
        let mut gpus_used = 0u32;

        for task_id in &wave.task_ids {
            if let Some(task) = dag.get_task(task_id) {
                cpu_used += task.resources.cpu_cores;
                memory_used += task.resources.memory_mb;
                if task.resources.gpu {
                    gpus_used += 1;
                }
            }
        }

        utilization.push(ResourceUtilization {
            time_secs: current_time,
            cpu_cores_used: cpu_used,
            memory_mb_used: memory_used,
            gpus_used,
            task_count: wave.task_ids.len(),
        });

        current_time += wave.estimated_time_secs;
    }

    utilization
}

/// Resource utilization at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceUtilization {
    /// Time in seconds from start.
    pub time_secs: u64,
    /// CPU cores in use.
    pub cpu_cores_used: f64,
    /// Memory in use (MB).
    pub memory_mb_used: u64,
    /// GPUs in use.
    pub gpus_used: u32,
    /// Number of tasks running.
    pub task_count: usize,
}

/// Optimize the schedule for better resource utilization.
pub fn optimize_schedule(
    dag: &WorkflowDag,
    resource_pool: &ResourcePool,
) -> Result<ParallelSchedule> {
    // Start with the basic schedule
    let schedule = create_parallel_schedule(dag, resource_pool)?;

    // Work over an owned, mutable copy of the waves so tasks pulled forward can
    // be removed from their original wave (otherwise a merged task would be
    // emitted in two waves at once).
    let mut waves = schedule.waves;

    // Try to merge waves with low resource utilization
    let mut optimized_waves: Vec<ExecutionWave> = Vec::new();
    let mut i = 0;

    while i < waves.len() {
        let mut current_wave = waves[i].clone();
        let mut current_resources = AvailableResources::from(resource_pool.clone());

        // Allocate current wave resources
        for task_id in &current_wave.task_ids {
            if let Some(task) = dag.get_task(task_id) {
                current_resources.allocate(&task.resources);
            }
        }

        // Try to merge tasks from the next wave into this one if resources allow
        // AND doing so does not violate DAG ordering.
        if i + 1 < waves.len() {
            // Set of task ids already committed to the current wave, updated as
            // we pull tasks in so later candidates see earlier merges.
            let mut current_ids: std::collections::HashSet<String> =
                current_wave.task_ids.iter().cloned().collect();
            // Task ids remaining in the next wave: a candidate that depends on
            // one of these must NOT be pulled forward past it.
            let next_ids: std::collections::HashSet<String> =
                waves[i + 1].task_ids.iter().cloned().collect();

            let candidates = waves[i + 1].task_ids.clone();
            let mut pulled = Vec::new();

            for task_id in &candidates {
                if let Some(task) = dag.get_task(task_id) {
                    // A task may only move into the current wave if none of its
                    // dependencies live in the current wave (would run
                    // simultaneously with an unfinished dependency) or remain in
                    // the next wave (would run before an unfinished dependency).
                    let deps = dag.get_dependencies(task_id);
                    let violates_ordering = deps
                        .iter()
                        .any(|d| current_ids.contains(d) || next_ids.contains(d));
                    if violates_ordering {
                        continue;
                    }

                    if current_resources.can_allocate(&task.resources) {
                        current_resources.allocate(&task.resources);
                        current_wave.estimated_time_secs = current_wave
                            .estimated_time_secs
                            .max(task.timeout_secs.unwrap_or(60));
                        current_ids.insert(task_id.clone());
                        current_wave.task_ids.push(task_id.clone());
                        pulled.push(task_id.clone());
                    }
                }
            }

            // Remove pulled tasks from the next wave so they are not scheduled
            // twice.
            if !pulled.is_empty() {
                let pulled_set: std::collections::HashSet<&String> = pulled.iter().collect();
                waves[i + 1].task_ids.retain(|id| !pulled_set.contains(id));
            }
        }

        optimized_waves.push(current_wave);
        i += 1;
    }

    // Drop any waves left empty after their tasks were pulled forward.
    optimized_waves.retain(|w| !w.task_ids.is_empty());

    // Recalculate statistics
    let total_time = optimized_waves.iter().map(|w| w.estimated_time_secs).sum();
    let max_parallelism = optimized_waves
        .iter()
        .map(|w| w.task_ids.len())
        .max()
        .unwrap_or(0);

    Ok(ParallelSchedule {
        waves: optimized_waves,
        estimated_time_secs: total_time,
        max_parallelism,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dag::graph::{ResourceRequirements, RetryPolicy, TaskEdge, TaskNode};
    use std::collections::HashMap;

    fn small_task(id: &str) -> TaskNode {
        TaskNode {
            id: id.to_string(),
            name: id.to_string(),
            description: None,
            config: serde_json::json!({}),
            retry: RetryPolicy::default(),
            timeout_secs: Some(60),
            resources: ResourceRequirements {
                cpu_cores: 1.0,
                memory_mb: 128,
                gpu: false,
                disk_mb: 0,
                custom: HashMap::new(),
            },
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn test_optimize_schedule_respects_dependencies() {
        // A -> B chain: B depends on A, so they occupy separate waves. Even with
        // ample resources the optimizer must NOT merge B into A's wave (ordering
        // violation), and must never emit B twice.
        let mut dag = WorkflowDag::new();
        dag.add_task(small_task("a")).expect("add a");
        dag.add_task(small_task("b")).expect("add b");
        dag.add_dependency("a", "b", TaskEdge::default())
            .expect("add edge");

        // Generous pool so a naive optimizer would try to pull B forward.
        let pool = ResourcePool::default();
        let schedule = optimize_schedule(&dag, &pool).expect("optimize");

        let wave_of = |id: &str| {
            schedule
                .waves
                .iter()
                .position(|w| w.task_ids.iter().any(|t| t == id))
        };
        let a_wave = wave_of("a").expect("a scheduled");
        let b_wave = wave_of("b").expect("b scheduled");

        assert_ne!(a_wave, b_wave, "B must not share a wave with dependency A");
        assert!(a_wave < b_wave, "A must run before B");

        // B appears exactly once across the whole schedule.
        let b_count: usize = schedule
            .waves
            .iter()
            .map(|w| w.task_ids.iter().filter(|t| *t == "b").count())
            .sum();
        assert_eq!(b_count, 1, "B must be scheduled exactly once");
    }

    #[test]
    fn test_optimize_schedule_merges_independent_tasks() {
        // Two independent tasks placed in separate waves only because of a tight
        // resource pool SHOULD be merged when a larger pool is used for
        // optimization (no dependency prevents it).
        let mut dag = WorkflowDag::new();
        dag.add_task(small_task("x")).expect("add x");
        dag.add_task(small_task("y")).expect("add y");
        // No dependency between x and y.

        let pool = ResourcePool::default();
        let schedule = optimize_schedule(&dag, &pool).expect("optimize");

        // Both independent tasks fit in the first wave.
        assert_eq!(schedule.waves.len(), 1);
        assert_eq!(schedule.waves[0].task_ids.len(), 2);
    }
}
