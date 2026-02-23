//! HA performance benchmarks.
#![allow(missing_docs, clippy::expect_used, unused_must_use)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_ha::replication::active_active::ActiveActiveReplication;
use oxigdal_ha::replication::{
    ReplicaNode, ReplicationConfig, ReplicationEvent, ReplicationManager, ReplicationState,
};
use std::hint::black_box;
use tokio::runtime::Runtime;
use uuid::Uuid;

fn replication_throughput(c: &mut Criterion) {
    let rt = Runtime::new().expect("runtime should be created for benchmark");
    let mut group = c.benchmark_group("replication_throughput");

    for batch_size in [100, 1000, 10000] {
        group.throughput(Throughput::Elements(batch_size as u64));

        group.bench_with_input(
            BenchmarkId::from_parameter(batch_size),
            &batch_size,
            |b, &size| {
                b.iter(|| {
                    rt.block_on(async move {
                        let node_id = Uuid::new_v4();
                        let config = ReplicationConfig {
                            batch_size: size,
                            ..Default::default()
                        };
                        let replication = ActiveActiveReplication::new(node_id, config);

                        replication
                            .start()
                            .await
                            .expect("replication should start successfully");

                        let replica_id = Uuid::new_v4();
                        let replica = ReplicaNode {
                            id: replica_id,
                            name: "bench-replica".to_string(),
                            address: "localhost:5000".to_string(),
                            priority: 100,
                            state: ReplicationState::Active,
                            last_replicated_at: None,
                            lag_ms: None,
                        };

                        replication
                            .add_replica(replica)
                            .await
                            .expect("replica should be added successfully");

                        let events: Vec<_> = (0..size)
                            .map(|i| {
                                ReplicationEvent::new(
                                    node_id,
                                    replica_id,
                                    vec![i as u8; 100],
                                    i as u64,
                                )
                            })
                            .collect();

                        replication
                            .replicate_batch(events)
                            .await
                            .expect("batch replication should succeed");

                        replication
                            .stop()
                            .await
                            .expect("replication should stop successfully");

                        black_box(size);
                    })
                });
            },
        );
    }

    group.finish();
}

fn failover_latency(c: &mut Criterion) {
    let rt = Runtime::new().expect("runtime should be created for benchmark");
    let mut group = c.benchmark_group("failover_latency");

    group.bench_function("leader_election", |b| {
        b.iter(|| {
            rt.block_on(async {
                use oxigdal_ha::failover::{FailoverConfig, election::LeaderElection};

                let config = FailoverConfig::default();
                let election = LeaderElection::new(Uuid::new_v4(), 100, config);

                let result = election.become_leader().await;

                black_box(result);
            })
        });
    });

    group.finish();
}

fn recovery_time(c: &mut Criterion) {
    let rt = Runtime::new().expect("runtime should be created for benchmark");
    let mut group = c.benchmark_group("recovery_time");

    group.bench_function("pitr_recovery", |b| {
        b.iter(|| {
            rt.block_on(async {
                use oxigdal_ha::recovery::{RecoveryConfig, RecoveryTarget, pitr::PitrManager};

                let config = RecoveryConfig::default();
                let data_dir = std::env::temp_dir().join("oxigdal-ha-bench-pitr");
                let manager = PitrManager::new(config, data_dir);

                let result = manager.recover(RecoveryTarget::Latest).await;

                black_box(result);
            })
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    replication_throughput,
    failover_latency,
    recovery_time
);
criterion_main!(benches);
