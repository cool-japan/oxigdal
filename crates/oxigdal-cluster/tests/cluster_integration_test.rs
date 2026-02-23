//! Integration tests for oxigdal-cluster.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use oxigdal_cluster::*;
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn test_cluster_creation() {
    let cluster = Cluster::new();
    assert!(Arc::strong_count(&cluster.task_graph) > 0);
}

#[tokio::test]
async fn test_cluster_builder() {
    let cluster = ClusterBuilder::new()
        .with_scheduler_config(SchedulerConfig {
            max_queue_size: 5000,
            ..Default::default()
        })
        .build();

    assert!(Arc::strong_count(&cluster.scheduler) > 0);
}

#[tokio::test]
async fn test_task_submission_and_execution() {
    let cluster = Cluster::new();

    // Register a worker
    let worker = Worker {
        id: WorkerId::new(),
        name: "test_worker".to_string(),
        address: "localhost:8080".to_string(),
        capabilities: WorkerCapabilities::default(),
        capacity: WorkerCapacity {
            cpu_cores: 8.0,
            memory_bytes: 16_000_000_000,
            storage_bytes: 0,
            gpu_count: 0,
            network_bandwidth: 0,
        },
        usage: WorkerUsage::default(),
        status: WorkerStatus::Active,
        last_heartbeat: std::time::Instant::now(),
        registered_at: std::time::Instant::now(),
        last_health_check: None,
        health_check_failures: 0,
        tasks_completed: 0,
        tasks_failed: 0,
        version: "1.0.0".to_string(),
        metadata: std::collections::HashMap::new(),
    };

    cluster.worker_pool.register_worker(worker).ok();

    // Start cluster
    cluster.start().await.ok();

    // Submit a task
    let task = Task {
        id: TaskId::new(),
        name: "test_task".to_string(),
        task_type: "processing".to_string(),
        priority: 10,
        payload: vec![],
        dependencies: vec![],
        estimated_duration: Some(Duration::from_secs(1)),
        resources: ResourceRequirements::default(),
        locality_hints: vec![],
        created_at: std::time::Instant::now(),
        scheduled_at: None,
        started_at: None,
        completed_at: None,
        status: TaskStatus::Ready,
        result: None,
        error: None,
        retry_count: 0,
        checkpoint: None,
    };

    let result = cluster.scheduler.submit_task(task).await;
    assert!(result.is_ok());

    // Stop cluster
    cluster.stop().await.ok();
}

#[tokio::test]
async fn test_task_dependencies() {
    let graph = TaskGraph::new();

    let task1 = Task {
        id: TaskId::new(),
        name: "task1".to_string(),
        task_type: "test".to_string(),
        priority: 0,
        payload: vec![],
        dependencies: vec![],
        estimated_duration: Some(Duration::from_secs(1)),
        resources: ResourceRequirements::default(),
        locality_hints: vec![],
        created_at: std::time::Instant::now(),
        scheduled_at: None,
        started_at: None,
        completed_at: None,
        status: TaskStatus::Ready,
        result: None,
        error: None,
        retry_count: 0,
        checkpoint: None,
    };

    let task1_id = graph.add_task(task1).ok().unwrap_or_default();

    let task2 = Task {
        id: TaskId::new(),
        name: "task2".to_string(),
        task_type: "test".to_string(),
        priority: 0,
        payload: vec![],
        dependencies: vec![task1_id],
        estimated_duration: Some(Duration::from_secs(1)),
        resources: ResourceRequirements::default(),
        locality_hints: vec![],
        created_at: std::time::Instant::now(),
        scheduled_at: None,
        started_at: None,
        completed_at: None,
        status: TaskStatus::Pending,
        result: None,
        error: None,
        retry_count: 0,
        checkpoint: None,
    };

    let result = graph.add_task(task2);
    assert!(result.is_ok());

    let plan = graph.build_execution_plan();
    assert!(plan.is_ok());
}

#[tokio::test]
async fn test_fault_tolerance() {
    let ft = FaultToleranceManager::with_defaults();
    let task_id = TaskId::new();
    let worker_id = WorkerId::new();

    // Simulate task failure
    let decision = ft
        .handle_task_failure(task_id, worker_id, "Test error".to_string())
        .await;

    assert!(decision.is_ok());
    assert!(matches!(decision, Ok(RetryDecision::Retry { .. })));

    let stats = ft.get_statistics();
    assert_eq!(stats.total_retries, 1);
}

#[tokio::test]
async fn test_data_locality() {
    let locality = DataLocalityOptimizer::with_defaults();
    let worker1 = WorkerId::new();
    let worker2 = WorkerId::new();

    // Register data locations
    locality.register_data("data1".to_string(), worker1).ok();
    locality.register_data("data2".to_string(), worker1).ok();
    locality.register_data("data3".to_string(), worker2).ok();

    // Get placement recommendation
    let required_data = vec!["data1".to_string(), "data2".to_string()];
    let candidates = vec![worker1, worker2];

    let recommendation = locality.recommend_placement(&required_data, &candidates);
    assert!(recommendation.is_ok());

    if let Ok(rec) = recommendation {
        assert_eq!(rec.worker_id, worker1);
        assert_eq!(rec.locality_score, 1.0);
    }
}

#[tokio::test]
async fn test_distributed_cache() {
    let cache = DistributedCache::with_defaults();
    let worker_id = WorkerId::new();

    let key = CacheKey::new("test".to_string(), "key1".to_string());
    let data = vec![1, 2, 3, 4, 5];

    // Put data in cache
    cache.put(key.clone(), data.clone(), worker_id).ok();

    // Get data from cache
    let result = cache.get(&key);
    assert!(result.is_ok());

    if let Ok(Some(retrieved)) = result {
        assert_eq!(retrieved, data);
    }

    let stats = cache.get_statistics();
    assert_eq!(stats.hits, 1);
}

#[tokio::test]
async fn test_replication() {
    let replication = ReplicationManager::with_defaults();

    let workers: Vec<_> = (0..5).map(|_| WorkerId::new()).collect();

    // Create replicas
    let result = replication.create_replicas("data1".to_string(), 1000, &workers);
    assert!(result.is_ok());

    if let Ok(replica_set) = result {
        assert_eq!(replica_set.replicas.len(), 3); // default replication factor
    }

    // Test quorum read
    let read_result = replication.quorum_read("data1").await;
    assert!(read_result.is_ok());

    let stats = replication.get_statistics();
    assert_eq!(stats.replicas_created, 3);
}

#[tokio::test]
async fn test_coordinator() {
    let coordinator = ClusterCoordinator::with_defaults();

    // Start coordinator
    coordinator.start().await.ok();

    // Get node ID
    let node_id = coordinator.node_id();
    assert_ne!(node_id.0, uuid::Uuid::nil());

    // Stop coordinator
    coordinator.stop().await.ok();
}

#[tokio::test]
async fn test_cluster_statistics() {
    let cluster = Cluster::new();

    let stats = cluster.get_statistics();

    assert_eq!(stats.metrics.tasks_submitted, 0);
    assert_eq!(stats.scheduler.tasks_scheduled, 0);
    assert_eq!(stats.worker_pool.total_workers, 0);
}
