//! Benchmarks for tiering policies
#![allow(missing_docs, clippy::expect_used, clippy::panic)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use oxigdal_cache_advanced::tiering::policy::{
    AdaptiveTierSizer, CostAwarePolicy, FrequencyBasedPolicy, TierInfo,
};
use std::hint::black_box;
use std::time::Duration;

fn create_test_tiers() -> Vec<TierInfo> {
    vec![
        TierInfo {
            name: "L1".to_string(),
            level: 0,
            cost_per_byte: 1.0,
            latency_us: 10,
            current_size: 0,
            max_size: 1024 * 1024,
        },
        TierInfo {
            name: "L2".to_string(),
            level: 1,
            cost_per_byte: 0.1,
            latency_us: 100,
            current_size: 0,
            max_size: 10 * 1024 * 1024,
        },
        TierInfo {
            name: "L3".to_string(),
            level: 2,
            cost_per_byte: 0.01,
            latency_us: 1000,
            current_size: 0,
            max_size: 100 * 1024 * 1024,
        },
    ]
}

fn bench_frequency_based_policy(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap_or_else(|_| panic!("Failed to create runtime"));

    c.bench_function("frequency_policy_record_access", |b| {
        b.iter(|| {
            rt.block_on(async {
                let tiers = create_test_tiers();
                let policy = FrequencyBasedPolicy::new(tiers, 5.0, 0.1);

                for i in 0..100 {
                    policy
                        .record_access(black_box(format!("key{}", i % 10)), 0, 1024)
                        .await;
                }
            })
        });
    });

    c.bench_function("frequency_policy_evaluate", |b| {
        b.iter(|| {
            rt.block_on(async {
                let tiers = create_test_tiers();
                let policy = FrequencyBasedPolicy::new(tiers, 5.0, 0.1);

                // Populate
                for _ in 0..10 {
                    policy.record_access("test_key".to_string(), 1, 1024).await;
                }

                let _action = policy.evaluate(&"test_key".to_string()).await;
            })
        });
    });

    c.bench_function("frequency_policy_get_candidates", |b| {
        b.iter(|| {
            rt.block_on(async {
                let tiers = create_test_tiers();
                let policy = FrequencyBasedPolicy::new(tiers, 1.0, 0.1);

                // Create many items
                for i in 0..100 {
                    for _ in 0..5 {
                        policy.record_access(format!("key{}", i), 1, 1024).await;
                    }
                }

                let _candidates = policy.get_promotion_candidates(1, black_box(10)).await;
            })
        });
    });
}

fn bench_cost_aware_policy(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap_or_else(|_| panic!("Failed to create runtime"));

    c.bench_function("cost_aware_record_access", |b| {
        b.iter(|| {
            rt.block_on(async {
                let tiers = create_test_tiers();
                let policy = CostAwarePolicy::new(tiers, Duration::from_secs(60));

                for i in 0..100 {
                    policy
                        .record_access(black_box(format!("key{}", i % 10)), 1, 1024)
                        .await;
                }
            })
        });
    });

    c.bench_function("cost_aware_get_optimal_tier", |b| {
        b.iter(|| {
            rt.block_on(async {
                let tiers = create_test_tiers();
                let policy = CostAwarePolicy::new(tiers, Duration::from_secs(60));

                // Populate
                for _ in 0..10 {
                    policy.record_access("test_key".to_string(), 1, 1024).await;
                }

                let _tier = policy.get_optimal_tier(&"test_key".to_string()).await;
            })
        });
    });
}

fn bench_adaptive_tier_sizer(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap_or_else(|_| panic!("Failed to create runtime"));

    c.bench_function("adaptive_tier_sizing", |b| {
        b.iter(|| {
            rt.block_on(async {
                let tiers = create_test_tiers();
                let sizer = AdaptiveTierSizer::new(tiers, 80.0, 0.1);

                let _adjusted = sizer.adjust_sizes().await;
            })
        });
    });
}

fn bench_tiering_workload(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap_or_else(|_| panic!("Failed to create runtime"));

    let mut group = c.benchmark_group("tiering_workload");

    for num_keys in [10, 100, 1000].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(num_keys),
            num_keys,
            |b, &num_keys| {
                b.iter(|| {
                    rt.block_on(async move {
                        let tiers = create_test_tiers();
                        let policy = FrequencyBasedPolicy::new(tiers, 5.0, 0.1);

                        // Simulate workload
                        for i in 0..num_keys {
                            let key = format!("key{}", i);
                            let accesses = (i % 10) + 1; // Variable frequency

                            for _ in 0..accesses {
                                policy.record_access(key.clone(), 1, 1024).await;
                            }

                            let _action = policy.evaluate(&key).await;
                        }
                    })
                });
            },
        );
    }

    group.finish();
}

fn bench_promotion_latency(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap_or_else(|_| panic!("Failed to create runtime"));

    c.bench_function("promotion_decision_latency", |b| {
        b.iter(|| {
            rt.block_on(async {
                let tiers = create_test_tiers();
                let policy = FrequencyBasedPolicy::new(tiers, 5.0, 0.1);

                // Hot item
                for _ in 0..20 {
                    policy.record_access("hot_key".to_string(), 2, 1024).await;
                }

                // Make promotion decision
                let start = std::time::Instant::now();
                let _action = policy.evaluate(&"hot_key".to_string()).await;
                let _latency = start.elapsed();
            })
        });
    });
}

criterion_group!(
    benches,
    bench_frequency_based_policy,
    bench_cost_aware_policy,
    bench_adaptive_tier_sizer,
    bench_tiering_workload,
    bench_promotion_latency
);

criterion_main!(benches);
