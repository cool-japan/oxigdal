#![allow(missing_docs, clippy::expect_used, clippy::panic, clippy::unit_arg)]
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use oxigdal_sync::crdt::{Crdt, GCounter, LwwRegister, OrSet, PnCounter};
use oxigdal_sync::delta::DeltaEncoder;
use oxigdal_sync::merkle::MerkleTree;
use oxigdal_sync::vector_clock::VectorClock;
use std::hint::black_box;

fn bench_vector_clock(c: &mut Criterion) {
    let mut group = c.benchmark_group("vector_clock");

    group.bench_function("tick", |b| {
        let mut clock = VectorClock::new("device-1".to_string());
        b.iter(|| {
            black_box(clock.tick());
        });
    });

    group.bench_function("merge", |b| {
        let mut clock1 = VectorClock::new("device-1".to_string());
        let mut clock2 = VectorClock::new("device-2".to_string());

        for _ in 0..10 {
            clock1.tick();
            clock2.tick();
        }

        b.iter(|| {
            let mut c = clock1.clone();
            c.merge(&clock2);
            black_box(());
        });
    });

    group.bench_function("compare", |b| {
        let mut clock1 = VectorClock::new("device-1".to_string());
        let mut clock2 = VectorClock::new("device-2".to_string());

        for _ in 0..10 {
            clock1.tick();
            clock2.tick();
        }

        b.iter(|| {
            black_box(clock1.compare(&clock2));
        });
    });

    group.finish();
}

fn bench_crdt_g_counter(c: &mut Criterion) {
    let mut group = c.benchmark_group("g_counter");

    group.bench_function("increment", |b| {
        let mut counter = GCounter::new("device-1".to_string());
        b.iter(|| {
            black_box(counter.increment(1));
        });
    });

    group.bench_function("merge", |b| {
        let mut counter1 = GCounter::new("device-1".to_string());
        let mut counter2 = GCounter::new("device-2".to_string());

        for _ in 0..100 {
            counter1.increment(1);
            counter2.increment(1);
        }

        b.iter(|| {
            let mut c = counter1.clone();
            black_box(c.merge(&counter2).ok());
        });
    });

    group.finish();
}

fn bench_crdt_pn_counter(c: &mut Criterion) {
    let mut group = c.benchmark_group("pn_counter");

    group.bench_function("increment", |b| {
        let mut counter = PnCounter::new("device-1".to_string());
        b.iter(|| {
            black_box(counter.increment(1));
        });
    });

    group.bench_function("decrement", |b| {
        let mut counter = PnCounter::new("device-1".to_string());
        b.iter(|| {
            black_box(counter.decrement(1));
        });
    });

    group.bench_function("merge", |b| {
        let mut counter1 = PnCounter::new("device-1".to_string());
        let mut counter2 = PnCounter::new("device-2".to_string());

        for _ in 0..100 {
            counter1.increment(1);
            counter2.decrement(1);
        }

        b.iter(|| {
            let mut c = counter1.clone();
            black_box(c.merge(&counter2).ok());
        });
    });

    group.finish();
}

fn bench_crdt_lww_register(c: &mut Criterion) {
    let mut group = c.benchmark_group("lww_register");

    group.bench_function("set", |b| {
        let mut register = LwwRegister::new("device-1".to_string(), 0);
        let mut counter = 0;
        b.iter(|| {
            counter += 1;
            register.set(counter);
            black_box(());
        });
    });

    group.bench_function("merge", |b| {
        let mut register1 = LwwRegister::new("device-1".to_string(), 0);
        let mut register2 = LwwRegister::new("device-2".to_string(), 0);

        for i in 0..10 {
            register1.set(i);
            register2.set(i * 2);
        }

        b.iter(|| {
            let mut r = register1.clone();
            black_box(r.merge(&register2).ok());
        });
    });

    group.finish();
}

fn bench_crdt_or_set(c: &mut Criterion) {
    let mut group = c.benchmark_group("or_set");

    group.bench_function("insert", |b| {
        let mut set = OrSet::new("device-1".to_string());
        let mut counter = 0;
        b.iter(|| {
            counter += 1;
            black_box(set.insert(counter));
        });
    });

    group.bench_function("merge_small", |b| {
        let mut set1 = OrSet::new("device-1".to_string());
        let mut set2 = OrSet::new("device-2".to_string());

        for i in 0..10 {
            set1.insert(i);
            set2.insert(i + 5);
        }

        b.iter(|| {
            let mut s = set1.clone();
            black_box(s.merge(&set2).ok());
        });
    });

    group.bench_function("merge_large", |b| {
        let mut set1 = OrSet::new("device-1".to_string());
        let mut set2 = OrSet::new("device-2".to_string());

        for i in 0..1000 {
            set1.insert(i);
            set2.insert(i + 500);
        }

        b.iter(|| {
            let mut s = set1.clone();
            black_box(s.merge(&set2).ok());
        });
    });

    group.finish();
}

fn bench_merkle_tree(c: &mut Criterion) {
    let mut group = c.benchmark_group("merkle_tree");

    for size in [10, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::new("from_data", size), size, |b, &size| {
            let data: Vec<Vec<u8>> = (0..size)
                .map(|i| format!("block-{}", i).into_bytes())
                .collect();

            b.iter(|| {
                black_box(MerkleTree::from_data(data.clone()).ok());
            });
        });
    }

    group.bench_function("verify", |b| {
        let data: Vec<Vec<u8>> = (0..100)
            .map(|i| format!("block-{}", i).into_bytes())
            .collect();
        let tree = MerkleTree::from_data(data.clone()).ok();

        if let Some(t) = tree {
            b.iter(|| {
                black_box(t.verify(&data).ok());
            });
        }
    });

    group.bench_function("diff", |b| {
        let data1: Vec<Vec<u8>> = (0..100)
            .map(|i| format!("block-{}", i).into_bytes())
            .collect();
        let mut data2 = data1.clone();
        data2[50] = b"modified".to_vec();

        let tree1 = MerkleTree::from_data(data1).ok();
        let tree2 = MerkleTree::from_data(data2).ok();

        if let (Some(t1), Some(t2)) = (tree1, tree2) {
            b.iter(|| {
                black_box(t1.diff(&t2));
            });
        }
    });

    group.finish();
}

fn bench_delta_encoding(c: &mut Criterion) {
    let mut group = c.benchmark_group("delta_encoding");

    for size in [100, 1000, 10000].iter() {
        group.bench_with_input(BenchmarkId::new("encode", size), size, |b, &size| {
            let base = vec![b'A'; size];
            let mut target = base.clone();
            // Modify 10% of the data
            for i in (0..size).step_by(10) {
                target[i] = b'B';
            }

            let encoder = DeltaEncoder::default_encoder();

            b.iter(|| {
                black_box(encoder.encode(&base, &target).ok());
            });
        });
    }

    for size in [100, 1000, 10000].iter() {
        group.bench_with_input(BenchmarkId::new("apply", size), size, |b, &size| {
            let base = vec![b'A'; size];
            let mut target = base.clone();
            for i in (0..size).step_by(10) {
                target[i] = b'B';
            }

            let encoder = DeltaEncoder::default_encoder();
            let delta = encoder.encode(&base, &target).ok();

            if let Some(d) = delta {
                b.iter(|| {
                    black_box(d.apply(&base).ok());
                });
            }
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_vector_clock,
    bench_crdt_g_counter,
    bench_crdt_pn_counter,
    bench_crdt_lww_register,
    bench_crdt_or_set,
    bench_merkle_tree,
    bench_delta_encoding
);

criterion_main!(benches);
