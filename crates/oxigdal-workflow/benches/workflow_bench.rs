//! Workflow benchmarks.
#![allow(missing_docs, clippy::expect_used, unused_must_use)]

use criterion::{Criterion, criterion_group, criterion_main};
use oxigdal_workflow::dag::{ResourceRequirements, RetryPolicy, TaskEdge, TaskNode, WorkflowDag};
use oxigdal_workflow::engine::WorkflowDefinition;
use oxigdal_workflow::monitoring::MetricsCollector;
use oxigdal_workflow::scheduler::{ScheduleType, Scheduler};
use std::collections::HashMap;
use std::hint::black_box;

fn create_test_workflow(num_tasks: usize) -> WorkflowDefinition {
    let dag = create_test_dag(num_tasks);

    WorkflowDefinition {
        id: format!("test-workflow-{}", num_tasks),
        name: format!("Test Workflow {}", num_tasks),
        version: "1.0.0".to_string(),
        dag,
        description: Some("Benchmark workflow".to_string()),
    }
}

fn create_test_dag(num_nodes: usize) -> WorkflowDag {
    let mut dag = WorkflowDag::new();

    for i in 0..num_nodes {
        let node = TaskNode {
            id: format!("task-{}", i),
            name: format!("Task {}", i),
            description: None,
            config: serde_json::json!({}),
            retry: RetryPolicy::default(),
            timeout_secs: Some(60),
            resources: ResourceRequirements::default(),
            metadata: HashMap::new(),
        };

        dag.add_task(node).expect("Failed to add node");

        // Add dependency to previous task
        if i > 0 {
            dag.add_dependency(
                &format!("task-{}", i - 1),
                &format!("task-{}", i),
                TaskEdge::default(),
            )
            .expect("Failed to add dependency");
        }
    }

    dag
}

fn bench_dag_creation(c: &mut Criterion) {
    c.bench_function("dag_creation_10", |b| {
        b.iter(|| {
            let dag = create_test_dag(black_box(10));
            black_box(dag);
        });
    });

    c.bench_function("dag_creation_100", |b| {
        b.iter(|| {
            let dag = create_test_dag(black_box(100));
            black_box(dag);
        });
    });
}

fn bench_dag_topological_sort(c: &mut Criterion) {
    let dag_10 = create_test_dag(10);
    let dag_100 = create_test_dag(100);

    c.bench_function("dag_topological_sort_10", |b| {
        b.iter(|| {
            let result = dag_10.validate();
            black_box(result);
        });
    });

    c.bench_function("dag_topological_sort_100", |b| {
        b.iter(|| {
            let result = dag_100.validate();
            black_box(result);
        });
    });
}

fn bench_scheduler_add_schedule(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");

    c.bench_function("scheduler_add_manual_schedule", |b| {
        b.iter(|| {
            let scheduler = Scheduler::with_defaults();
            let workflow = create_test_workflow(10);

            runtime.block_on(async {
                let result = scheduler.add_schedule(workflow, ScheduleType::Manual).await;
                black_box(result);
            });
        });
    });
}

fn bench_metrics_collection(c: &mut Criterion) {
    c.bench_function("metrics_record_workflow_execution", |b| {
        let collector = MetricsCollector::new();

        b.iter(|| {
            collector.record_workflow_start("workflow1");
            collector.record_workflow_completion(
                "workflow1",
                std::time::Duration::from_secs(1),
                true,
            );
            black_box(&collector);
        });
    });

    c.bench_function("metrics_record_task_execution", |b| {
        let collector = MetricsCollector::new();

        b.iter(|| {
            collector.record_task_execution(
                "workflow1",
                "task1",
                std::time::Duration::from_millis(100),
                true,
            );
            black_box(&collector);
        });
    });
}

fn bench_concurrent_scheduling(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");

    c.bench_function("concurrent_schedule_10", |b| {
        b.iter(|| {
            let scheduler = Scheduler::with_defaults();

            runtime.block_on(async {
                let mut handles = vec![];

                for _i in 0..10 {
                    let workflow = create_test_workflow(5);
                    let result = scheduler.add_schedule(workflow, ScheduleType::Manual).await;
                    handles.push(result);
                }

                black_box(handles);
            });
        });
    });
}

criterion_group!(
    benches,
    bench_dag_creation,
    bench_dag_topological_sort,
    bench_scheduler_add_schedule,
    bench_metrics_collection,
    bench_concurrent_scheduling
);

criterion_main!(benches);
