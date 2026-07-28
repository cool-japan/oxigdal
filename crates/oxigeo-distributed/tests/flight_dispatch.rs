//! End-to-end integration test for the Coordinator → Flight → Worker pipeline.
//!
//! Starts a real tonic Flight server backed by a [`Worker`], registers it with a
//! [`Coordinator`], and dispatches a task over gRPC via
//! [`Coordinator::dispatch_task_to_worker`]. This exercises the wiring that
//! previously did not exist: task serialization, Arrow IPC data transfer, remote
//! execution, and result collection, all across a real socket.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use arrow::array::Int32Array;
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use oxigeo_distributed::coordinator::{Coordinator, CoordinatorConfig};
use oxigeo_distributed::flight::FlightClient;
use oxigeo_distributed::flight::server::FlightServer;
use oxigeo_distributed::task::{PartitionId, Task, TaskId, TaskOperation};
use oxigeo_distributed::worker::{Worker, WorkerConfig};

fn test_batch() -> Arc<RecordBatch> {
    let schema = Arc::new(Schema::new(vec![Field::new(
        "value",
        DataType::Int32,
        false,
    )]));
    Arc::new(
        RecordBatch::try_new(
            schema,
            vec![Arc::new(Int32Array::from(vec![1, 2, 3, 4, 5]))],
        )
        .expect("batch"),
    )
}

async fn wait_until_reachable(address: &str) {
    for _ in 0..100 {
        if FlightClient::new(address.to_string()).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("flight server at {address} never became reachable");
}

#[tokio::test]
async fn coordinator_dispatches_task_to_remote_worker_over_flight() {
    // Reserve an ephemeral port, then hand it to the tonic server.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    drop(listener);
    let socket_addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().expect("sockaddr");
    let http_addr = format!("http://127.0.0.1:{port}");

    // A real worker served over Flight.
    let worker = Arc::new(Worker::new(WorkerConfig::new("worker-remote".to_string())));
    let server = FlightServer::new().with_worker(worker);
    let service = server.into_service();

    let server_handle = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(service)
            .serve(socket_addr)
            .await
            .expect("server");
    });

    wait_until_reachable(&http_addr).await;

    // A coordinator that knows the worker by its Flight address.
    let coordinator = Coordinator::new(CoordinatorConfig::new(http_addr.clone()));
    coordinator
        .add_worker("worker-remote".to_string(), http_addr.clone())
        .expect("add worker");

    // Dispatch a Filter task (value > 2) end-to-end; 3 of 5 rows survive.
    let task = Task::new(
        TaskId(1),
        PartitionId(0),
        TaskOperation::Filter {
            expression: "value > 2".to_string(),
        },
    );

    let result = coordinator
        .dispatch_task_to_worker(task, "worker-remote", test_batch())
        .await
        .expect("dispatch");

    assert!(result.is_success(), "remote task must succeed: {result:?}");
    let output = result.data.expect("output batch");
    assert_eq!(
        output.num_rows(),
        3,
        "the filter must have really executed on the remote worker"
    );

    // The coordinator recorded the completion in its own bookkeeping.
    let progress = coordinator.get_progress().expect("progress");
    assert_eq!(progress.completed_tasks, 1);

    server_handle.abort();
}

#[tokio::test]
async fn coordinator_dispatch_reports_failure_for_bad_expression() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    drop(listener);
    let socket_addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().expect("sockaddr");
    let http_addr = format!("http://127.0.0.1:{port}");

    let worker = Arc::new(Worker::new(WorkerConfig::new("worker-remote".to_string())));
    let service = FlightServer::new().with_worker(worker).into_service();
    let server_handle = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(service)
            .serve(socket_addr)
            .await
            .expect("server");
    });

    wait_until_reachable(&http_addr).await;

    let coordinator = Coordinator::new(CoordinatorConfig::new(http_addr.clone()));
    coordinator
        .add_worker("worker-remote".to_string(), http_addr.clone())
        .expect("add worker");

    // A filter on a non-existent column must surface as a task failure, not a
    // crash or a silent success.
    let task = Task::new(
        TaskId(2),
        PartitionId(0),
        TaskOperation::Filter {
            expression: "no_such_column > 2".to_string(),
        },
    );

    let result = coordinator
        .dispatch_task_to_worker(task, "worker-remote", test_batch())
        .await
        .expect("dispatch returns a result even on task failure");

    assert!(result.is_failure(), "bad expression must fail the task");
    assert!(result.error.is_some());

    server_handle.abort();
}
