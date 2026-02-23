//! Integration tests for distributed processing.

use arrow::array::{Float64Array, Int32Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use oxigdal_distributed::*;
use std::sync::Arc;

fn create_test_batch(rows: usize) -> RecordBatch {
    let schema = Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int32, false),
        Field::new("value", DataType::Float64, false),
        Field::new("name", DataType::Utf8, false),
    ]));

    let id_array = Int32Array::from((0..rows as i32).collect::<Vec<_>>());
    let value_array = Float64Array::from((0..rows).map(|i| i as f64 * 10.0).collect::<Vec<_>>());
    let name_array =
        StringArray::from((0..rows).map(|i| format!("name_{}", i)).collect::<Vec<_>>());

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(id_array),
            Arc::new(value_array),
            Arc::new(name_array),
        ],
    )
    .expect("valid batch")
}

#[test]
fn test_spatial_partitioning() {
    let extent = SpatialExtent::new(0.0, 0.0, 100.0, 100.0).expect("valid extent");

    // Test tile partitioner
    let partitioner = TilePartitioner::new(extent, 3, 3).expect("valid partitioner");
    let partitions = partitioner.partition();
    assert_eq!(partitions.len(), 9);

    // Verify partition extents
    for partition in &partitions {
        assert!(partition.extent.width() > 0.0);
        assert!(partition.extent.height() > 0.0);
    }
}

#[test]
fn test_strip_partitioning() {
    let extent = SpatialExtent::new(0.0, 0.0, 100.0, 100.0).expect("valid extent");

    let partitioner = StripPartitioner::new(extent, 5).expect("valid partitioner");
    let partitions = partitioner.partition();
    assert_eq!(partitions.len(), 5);

    // Each strip should have full width
    for partition in &partitions {
        assert_eq!(partition.extent.width(), 100.0);
        assert_eq!(partition.extent.height(), 20.0);
    }
}

#[test]
fn test_hash_partitioning() {
    let partitioner = HashPartitioner::new(4).expect("valid partitioner");

    // Same key should always map to same partition
    let key1 = b"test_key";
    let partition1 = partitioner.partition_for_key(key1);
    let partition2 = partitioner.partition_for_key(key1);
    assert_eq!(partition1, partition2);

    // Partition should be in valid range
    assert!(partition1.0 < 4);
}

#[test]
fn test_hash_shuffle() {
    let batch = create_test_batch(100);

    let shuffle = HashShuffle::new("id".to_string(), 4).expect("valid shuffle");
    let partitions = shuffle.shuffle(&batch).expect("shuffle should succeed");

    // Verify total rows preserved
    let total_rows: usize = partitions.values().map(|b| b.num_rows()).sum();
    assert_eq!(total_rows, 100);

    // Each partition should have some data
    assert!(!partitions.is_empty());
}

#[test]
fn test_range_shuffle() {
    let batch = create_test_batch(100);

    // Partition by value ranges
    let boundaries = vec![250.0, 500.0, 750.0];
    let shuffle = RangeShuffle::new("value".to_string(), boundaries).expect("valid shuffle");
    let partitions = shuffle.shuffle(&batch).expect("shuffle should succeed");

    // Verify total rows preserved
    let total_rows: usize = partitions.values().map(|b| b.num_rows()).sum();
    assert_eq!(total_rows, 100);
}

#[test]
fn test_broadcast_shuffle() {
    let batch = create_test_batch(50);

    let shuffle = BroadcastShuffle::new(3).expect("valid shuffle");
    let partitions = shuffle.shuffle(&batch);

    // Should have exactly 3 partitions
    assert_eq!(partitions.len(), 3);

    // Each partition should have all rows
    for partition_batch in partitions.values() {
        assert_eq!(partition_batch.num_rows(), 50);
    }
}

#[test]
fn test_task_lifecycle() {
    let task = Task::new(
        TaskId(1),
        PartitionId(0),
        TaskOperation::Filter {
            expression: "value > 100".to_string(),
        },
    );

    assert_eq!(task.status, TaskStatus::Pending);
    assert!(task.can_retry());
}

#[test]
fn test_task_scheduler() {
    let mut scheduler = TaskScheduler::new();

    // Add tasks
    for i in 0..5 {
        let task = Task::new(
            TaskId(i),
            PartitionId(i),
            TaskOperation::Filter {
                expression: format!("value > {}", i * 10),
            },
        );
        scheduler.add_task(task);
    }

    assert_eq!(scheduler.pending_count(), 5);
    assert_eq!(scheduler.running_count(), 0);

    // Get and run a task
    let task = scheduler.next_task().expect("should have task");
    scheduler.mark_running(task, "worker-1".to_string());

    assert_eq!(scheduler.pending_count(), 4);
    assert_eq!(scheduler.running_count(), 1);

    // Complete the task
    scheduler
        .mark_completed(TaskId(4))
        .expect("should complete");

    assert_eq!(scheduler.running_count(), 0);
    assert_eq!(scheduler.completed_count(), 1);
}

#[test]
fn test_coordinator() {
    let config = CoordinatorConfig::new("localhost:50051".to_string());
    let coordinator = Coordinator::new(config);

    // Add workers
    coordinator
        .add_worker("worker-1".to_string(), "localhost:50052".to_string())
        .expect("should add worker");
    coordinator
        .add_worker("worker-2".to_string(), "localhost:50053".to_string())
        .expect("should add worker");

    let workers = coordinator.list_workers().expect("should list workers");
    assert_eq!(workers.len(), 2);

    // Submit tasks
    for i in 0..10 {
        coordinator
            .submit_task(
                PartitionId(i),
                TaskOperation::Filter {
                    expression: format!("value > {}", i * 10),
                },
            )
            .expect("should submit task");
    }

    let progress = coordinator.get_progress().expect("should get progress");
    assert_eq!(progress.pending_tasks, 10);
    assert_eq!(progress.active_workers, 2);
}

#[tokio::test]
async fn test_worker() {
    let config = WorkerConfig::new("worker-test".to_string());
    let worker = Worker::new(config);

    assert_eq!(worker.worker_id(), "worker-test");
    assert!(worker.is_available());

    let health = worker.health_check();
    assert!(health.is_healthy);
    assert_eq!(health.active_tasks, 0);
}

#[tokio::test]
async fn test_worker_execute_task() {
    let config = WorkerConfig::new("worker-test".to_string());
    let worker = Worker::new(config);

    let task = Task::new(
        TaskId(1),
        PartitionId(0),
        TaskOperation::Filter {
            expression: "value > 50".to_string(),
        },
    );

    let batch = Arc::new(create_test_batch(100));
    let result = worker.execute_task(task, batch).await;

    assert!(result.is_ok());
    let task_result = result.expect("should have result");
    assert!(task_result.is_success());
    assert_eq!(task_result.task_id, TaskId(1));
}

#[test]
fn test_worker_metrics() {
    let mut metrics = WorkerMetrics::default();

    metrics.record_success(100);
    metrics.record_success(200);
    metrics.record_failure(150);

    assert_eq!(metrics.tasks_executed, 3);
    assert_eq!(metrics.tasks_succeeded, 2);
    assert_eq!(metrics.tasks_failed, 1);
    assert!((metrics.success_rate() - 0.666).abs() < 0.01);
    assert_eq!(metrics.avg_execution_time_ms(), 150.0);
}

#[test]
fn test_coordinator_progress() {
    let progress = CoordinatorProgress {
        pending_tasks: 10,
        running_tasks: 5,
        completed_tasks: 30,
        failed_tasks: 5,
        active_workers: 4,
        idle_workers: 2,
    };

    assert_eq!(progress.total_tasks(), 50);
    assert_eq!(progress.completion_percentage(), 60.0);
}

#[test]
fn test_load_balanced_partitioner() {
    let total_size = 1000 * 1024 * 1024; // 1 GB
    let num_workers = 8;

    let partitioner =
        LoadBalancedPartitioner::new(total_size, num_workers).expect("valid partitioner");

    assert_eq!(partitioner.estimated_partitions(), 8);
    assert_eq!(partitioner.target_size(), 125 * 1024 * 1024);
}

#[test]
fn test_shuffle_config() {
    let config = ShuffleConfig::new(ShuffleType::Hash, ShuffleKey::Column("id".to_string()), 4)
        .expect("valid config")
        .with_buffer_size(2 * 1024 * 1024);

    assert_eq!(config.shuffle_type, ShuffleType::Hash);
    assert_eq!(config.num_partitions, 4);
    assert_eq!(config.buffer_size, 2 * 1024 * 1024);
}

#[test]
fn test_flight_server() {
    let server = FlightServer::new();

    let batch = Arc::new(create_test_batch(50));
    server
        .store_data("test_ticket".to_string(), batch.clone())
        .expect("should store");

    let retrieved = server
        .get_data("test_ticket")
        .expect("should retrieve")
        .expect("should exist");

    assert_eq!(retrieved.num_rows(), 50);

    let tickets = server.list_tickets().expect("should list");
    assert_eq!(tickets.len(), 1);
}

#[test]
fn test_error_types() {
    let err = DistributedError::flight_rpc("test error");
    assert!(err.to_string().contains("Flight RPC error"));

    let err = DistributedError::worker_task_failure("task failed");
    assert!(err.to_string().contains("Worker task failure"));

    let err = DistributedError::timeout("operation timed out");
    assert!(err.to_string().contains("Network timeout"));
}

#[test]
fn test_task_operations() {
    // Test different operation types
    let op1 = TaskOperation::Filter {
        expression: "value > 10".to_string(),
    };
    let _task1 = Task::new(TaskId(1), PartitionId(0), op1);

    let op2 = TaskOperation::CalculateIndex {
        index_type: "NDVI".to_string(),
        bands: vec![3, 4],
    };
    let _task2 = Task::new(TaskId(2), PartitionId(1), op2);

    let op3 = TaskOperation::Reproject { target_epsg: 4326 };
    let _task3 = Task::new(TaskId(3), PartitionId(2), op3);

    let op4 = TaskOperation::Resample {
        width: 1024,
        height: 1024,
        method: "bilinear".to_string(),
    };
    let _task4 = Task::new(TaskId(4), PartitionId(3), op4);
}

#[test]
fn test_partition_extent_operations() {
    let extent1 = SpatialExtent::new(0.0, 0.0, 100.0, 100.0).expect("valid extent");
    let extent2 = SpatialExtent::new(50.0, 50.0, 150.0, 150.0).expect("valid extent");

    assert!(extent1.contains(50.0, 50.0));
    assert!(!extent1.contains(150.0, 50.0));

    assert!(extent1.intersects(&extent2));

    assert_eq!(extent1.area(), 10000.0);
}
