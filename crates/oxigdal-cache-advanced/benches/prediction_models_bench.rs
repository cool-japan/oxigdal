//! Benchmarks for advanced prediction models
#![allow(missing_docs, clippy::expect_used)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use oxigdal_cache_advanced::predictive::advanced::{
    HybridPredictor, LSTMPredictor, TransformerPredictor,
};
use std::hint::black_box;

fn bench_transformer_predictor(c: &mut Criterion) {
    let mut group = c.benchmark_group("transformer_predictor");

    for seq_len in [5, 10, 20].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(seq_len),
            seq_len,
            |b, &seq_len| {
                let mut predictor = TransformerPredictor::new(16, 2, seq_len);

                // Populate with some data
                for i in 0..seq_len {
                    predictor.record_access(format!("key{}", i % 5));
                }

                b.iter(|| {
                    predictor.record_access(black_box("test_key".to_string()));
                    let _predictions = predictor.predict(black_box(3));
                });
            },
        );
    }

    group.finish();
}

fn bench_lstm_predictor(c: &mut Criterion) {
    let mut group = c.benchmark_group("lstm_predictor");

    for hidden_size in [16, 32, 64].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(hidden_size),
            hidden_size,
            |b, &hidden_size| {
                let mut predictor = LSTMPredictor::new(hidden_size);

                // Populate with pattern
                for i in 0..10 {
                    predictor
                        .record_access(format!("key{}", i % 3))
                        .unwrap_or_default();
                }

                b.iter(|| {
                    predictor
                        .record_access(black_box("test_key".to_string()))
                        .unwrap_or_default();
                    let _predictions = predictor.predict(black_box(5));
                });
            },
        );
    }

    group.finish();
}

fn bench_hybrid_predictor(c: &mut Criterion) {
    c.bench_function("hybrid_predictor_prediction", |b| {
        let mut predictor = HybridPredictor::new(16, 32, 10);

        // Train with pattern
        for i in 0..50 {
            predictor
                .record_access(format!("key{}", i % 10))
                .unwrap_or_default();
        }

        b.iter(|| {
            let _predictions = predictor.predict(black_box(5));
        });
    });
}

fn bench_hybrid_online_learning(c: &mut Criterion) {
    c.bench_function("hybrid_online_learning", |b| {
        let mut predictor = HybridPredictor::new(16, 32, 10);

        b.iter(|| {
            predictor.report_accuracy(black_box("transformer"), black_box(0.85));
            predictor.report_accuracy(black_box("lstm"), black_box(0.75));
        });
    });
}

fn bench_prediction_accuracy(c: &mut Criterion) {
    let mut group = c.benchmark_group("prediction_accuracy");

    // Create predictors
    let mut transformer = TransformerPredictor::new(16, 2, 10);
    let mut lstm = LSTMPredictor::new(32);

    // Train with repeating pattern
    let pattern = vec!["A", "B", "C", "D", "E"];
    for _ in 0..10 {
        for key in &pattern {
            transformer.record_access(key.to_string());
            lstm.record_access(key.to_string()).unwrap_or_default();
        }
    }

    group.bench_function("transformer", |b| {
        b.iter(|| {
            let predictions = transformer.predict(black_box(3)).unwrap_or_default();
            black_box(predictions);
        });
    });

    group.bench_function("lstm", |b| {
        b.iter(|| {
            let predictions = lstm.predict(black_box(3)).unwrap_or_default();
            black_box(predictions);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_transformer_predictor,
    bench_lstm_predictor,
    bench_hybrid_predictor,
    bench_hybrid_online_learning,
    bench_prediction_accuracy
);

criterion_main!(benches);
