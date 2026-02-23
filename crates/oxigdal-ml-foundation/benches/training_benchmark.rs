//! Benchmarks for training components.
#![allow(missing_docs, clippy::expect_used)]

use criterion::{Criterion, criterion_group, criterion_main};
use ndarray::{Array2, Array3};
use oxigdal_ml_foundation::{
    augmentation::{Augmentation, geometric::HorizontalFlip},
    training::{
        losses::{CrossEntropyLoss, DiceLoss, LossFunction, MSELoss},
        optimizers::{Adam, Optimizer, SGD},
        schedulers::{CosineAnnealingLR, LRScheduler, StepLR},
    },
};
use std::hint::black_box;

fn loss_benchmark(c: &mut Criterion) {
    let predictions = Array2::from_shape_fn((32, 10), |(i, j)| (i + j) as f32 * 0.1);
    let targets = Array2::from_shape_fn((32, 10), |(i, j)| if i == j { 1.0 } else { 0.0 });

    c.bench_function("mse_loss", |b| {
        let loss = MSELoss;
        b.iter(|| {
            loss.compute(black_box(predictions.view()), black_box(targets.view()))
                .expect("Failed to compute loss")
        });
    });

    c.bench_function("cross_entropy_loss", |b| {
        let loss = CrossEntropyLoss::new();
        b.iter(|| {
            loss.compute(black_box(predictions.view()), black_box(targets.view()))
                .expect("Failed to compute loss")
        });
    });

    c.bench_function("dice_loss", |b| {
        let loss = DiceLoss::new();
        b.iter(|| {
            loss.compute(black_box(predictions.view()), black_box(targets.view()))
                .expect("Failed to compute loss")
        });
    });
}

fn optimizer_benchmark(c: &mut Criterion) {
    let gradient = Array2::from_shape_fn((100, 100), |(i, j)| (i + j) as f32 * 0.01);

    c.bench_function("sgd_step", |b| {
        let mut sgd = SGD::new(0.01).expect("Failed to create SGD");
        b.iter(|| {
            sgd.step("param1", black_box(gradient.view()))
                .expect("Failed to perform step")
        });
    });

    c.bench_function("adam_step", |b| {
        let mut adam = Adam::new(0.001).expect("Failed to create Adam");
        b.iter(|| {
            adam.step("param1", black_box(gradient.view()))
                .expect("Failed to perform step")
        });
    });
}

fn scheduler_benchmark(c: &mut Criterion) {
    c.bench_function("step_lr", |b| {
        let scheduler = StepLR::new(10, 0.1).expect("Failed to create StepLR");
        b.iter(|| scheduler.get_lr(black_box(15), black_box(1.0)));
    });

    c.bench_function("cosine_annealing_lr", |b| {
        let scheduler =
            CosineAnnealingLR::new(100, 0.0).expect("Failed to create CosineAnnealingLR");
        b.iter(|| scheduler.get_lr(black_box(50), black_box(1.0)));
    });
}

fn augmentation_benchmark(c: &mut Criterion) {
    let image = Array3::from_shape_fn((3, 224, 224), |(c, h, w)| {
        (c as f32 + h as f32 + w as f32) * 0.01
    });

    c.bench_function("horizontal_flip", |b| {
        let flip = HorizontalFlip;
        b.iter(|| flip.apply(black_box(&image)).expect("Failed to apply flip"));
    });
}

criterion_group!(
    benches,
    loss_benchmark,
    optimizer_benchmark,
    scheduler_benchmark,
    augmentation_benchmark
);
criterion_main!(benches);
