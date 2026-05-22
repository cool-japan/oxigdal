# TODO: oxigdal-ml-foundation

> **Purpose:** Pure-Rust training infrastructure for the OxiGDAL ML stack — architectures (UNet, ResNet), training loops, optimizers, transfer learning, augmentation, and the SciRS2-backed autograd backend.
> **Status (2026-05-16):** 9,746 LoC · 187 tests · 7+ real stubs (SciRS2 backend backward-pass, autograd-backend ONNX export, augmentation/noise.rs fixed-noise placeholder, dataset.rs batch-size placeholders).
> **Roadmap:** v0.1.5 → v0.2.0 → v1.0.0

## High Priority (verified gaps)
- [ ] Real Pure-Rust forward + backward pass on the SciRS2 backend
  - **Verified gap:** `src/backend/scirs2_backend.rs:7` — `NOTE: This module is a placeholder awaiting scirs2 API stabilization.` and `:45-49` `/// Backward pass (placeholder) … // Placeholder until scirs2-autograd is integrated / Ok(())`.
  - **Goal:** Forward pass: build a real computation graph from `UNetConfig`/`ResNetConfig` using `scirs2-autograd::Graph`; backward pass: call `Graph::backward(loss_node)` and update parameters through the registered optimizer wrapper, removing the `Ok(())` no-op.
  - **Design:** Replace the local `Variable` struct (lines 22-50) with a thin adapter over `scirs2_autograd::Tensor<f32>`; each `EncoderBlock`/`DecoderBlock` registers its convolution kernels as `Variable::trainable(shape)` parameter nodes; `forward()` returns the leaf tensor; `step(loss)` invokes `Graph::backward(&loss)` then `optimizer.step(&self.parameters())`. Reuse `scirs2-core` operator registry per scirs2-core 0.4.4.
  - **Files:** `crates/oxigdal-ml-foundation/src/backend/scirs2_backend.rs`, `src/backend/layers.rs` (ConvBlock parameter exposure).
  - **Tests:** (proposed) `test_unet_forward_shape_matches_config`, `test_unet_backward_updates_parameters`, `test_resnet_forward_skip_connection_shape`, `test_optimizer_step_decreases_quadratic_loss`, `test_scirs2_backend_parameter_count`.
  - **Risk:** scirs2-autograd 0.4.4 surface still evolving — pin minor; add `#[deprecated]` migration hooks if signatures shift.
  - **Prerequisites:** scirs2-autograd 0.4.4 stable (workspace dep already at 0.4.4).

- [ ] Real Gaussian-noise augmentation backed by `scirs2-core::random`
  - **Verified gap:** `src/augmentation/noise.rs:29-31` — `// Note: In a real implementation, we'd use a proper RNG / // For now, this is a placeholder that adds fixed noise / let noise_pattern = image.mapv(|_| self.mean);`
  - **Goal:** Draw per-pixel Gaussian samples from a thread-local PRNG via `scirs2_core::random::NormalDistribution`; emit reproducible noise when caller passes a seed.
  - **Design:** Add `seed: Option<u64>` to `GaussianNoise`; use `NormalDistribution::new(mean, std)` and `sample(rng)` per pixel; if `seed.is_some()`, instantiate `scirs2_core::random::Xoroshiro128Plus::seed_from_u64(seed)`, else `thread_rng()`. Same fix shape applies to `ChannelDropout` (currently uses `p` field without actually dropping).
  - **Files:** `crates/oxigdal-ml-foundation/src/augmentation/noise.rs`.
  - **Tests:** (proposed) `test_gaussian_noise_seeded_reproducible`, `test_gaussian_noise_mean_zero_std_zero_is_identity`, `test_gaussian_noise_distribution_mean_within_3_sigma`, `test_channel_dropout_p_zero_no_change`, `test_channel_dropout_p_one_zeros_all_channels`.
  - **Risk:** Determinism across `rayon` parallel iters — must thread the seed per worker, not share global RNG.
  - **Prerequisites:** None — `scirs2-core` `random` feature already enabled.

- [ ] ONNX export from the autograd backend
  - **Verified gap:** `src/backend/scirs2_backend.rs:392` + `:791` — `"ONNX export not yet implemented".to_string()`; and `src/backend/autograd_backend.rs:336` + `:588` — `"ONNX export not yet implemented for autograd backend".to_string()`.
  - **Goal:** Serialize a trained `UNet`/`ResNet` model graph to ONNX (opset 17, per ai.onnx.ml v3) so the model can be consumed by `oxigdal-ml`'s `OnnxModel::from_file` for inference.
  - **Design:** Walk the autograd graph in topological order, emit `onnx::NodeProto` per supported op (Conv2D → `Conv`, ReLU → `Relu`, MaxPool2D → `MaxPool`, Upsample2D → `Resize`, Concat → `Concat`), pack parameter tensors as `TensorProto` initializers, write `ModelProto` via `prost`-generated structs (already a workspace dep).
  - **Files:** `crates/oxigdal-ml-foundation/src/backend/onnx_export.rs` (new ~400 LoC), wire `export_onnx()` in both backends.
  - **Tests:** (proposed) `test_export_unet_roundtrip_via_oxionnx`, `test_export_resnet_inference_matches_pytorch_baseline`, `test_export_opset_17_compliance`, `test_export_handles_unsupported_op_returns_error`.
  - **Risk:** Per-op opset version drift — pin `ai.onnx.opset = 17` (per onnx.ai/Operators.html, 2024-04 release).
  - **Prerequisites:** `oxionnx` reader fully compatible with opset 17 (verify upstream).

- [ ] Adam/AdamW optimizer with weight decay + gradient clipping
  - **Goal:** Production-grade Adam (Kingma & Ba 2015) and AdamW (Loshchilov & Hutter 2019) implementations, with per-parameter state, bias correction, optional global-norm gradient clipping.
  - **Design:** `Optimizer` trait with `step(&mut self, params: &mut [Tensor], grads: &[Tensor])`; AdamW uses `θ ← θ - lr·(m̂/(√v̂ + ε) + λ·θ)` (decoupled decay per the 2019 paper). Clipping: `g_norm = sqrt(Σ ||g_i||²)`; if `g_norm > max_norm` scale all `g_i` by `max_norm / g_norm`.
  - **Files:** `crates/oxigdal-ml-foundation/src/training/optimizers.rs` (extend existing).
  - **Tests:** (proposed) `test_adam_minimizes_quadratic`, `test_adamw_decoupled_weight_decay_vs_l2`, `test_grad_clip_caps_norm`, `test_adam_bias_correction_first_step`.
  - **Risk:** Numerical-stability on f32 with small `ε` (1e-8); document and allow `f64` override.
  - **Prerequisites:** Item 1 (SciRS2 backend backward pass) for end-to-end test.

- [ ] Loss functions: CrossEntropy, Dice, Focal — connect to autograd graph
  - **Goal:** Geospatial-segmentation losses with proper gradient flow, including ignore-index handling and per-class weighting.
  - **Design:** `CrossEntropyLoss { ignore_index: Option<i64>, weight: Option<Tensor> }`; `DiceLoss { smooth: 1.0 }` computing `1 - 2·|X∩Y|/(|X|+|Y|+smooth)`; `FocalLoss { alpha: 0.25, gamma: 2.0 }` per Lin et al. 2017. All three register on the autograd graph as forward+backward op pairs.
  - **Files:** `crates/oxigdal-ml-foundation/src/training/losses.rs` (extend).
  - **Tests:** (proposed) `test_cross_entropy_matches_pytorch_reference`, `test_dice_loss_perfect_overlap_zero`, `test_focal_loss_easy_examples_downweighted`, `test_ignore_index_excluded_from_grad`.
  - **Risk:** Class imbalance edge cases — empty class returns NaN unless `smooth` applied.
  - **Prerequisites:** Item 1 (backward pass).

- [ ] Data pipeline: parallel prefetching + on-the-fly augmentation
  - **Goal:** Bounded-buffer dataloader on `rayon` worker pool, with augmentation pipeline applied per sample, supporting `Dataset::sample_batch()` for batch-size-driven iteration (replacing the `batch size placeholder` constants).
  - **Verified gap:** `src/data/dataset.rs:310, :316, :366, :372` — `1, // batch size placeholder`.
  - **Design:** `DataLoader { dataset: Arc<dyn Dataset>, batch_size: usize, prefetch: usize, augmentation: Pipeline }`; uses `crossbeam-channel` bounded queue + `rayon` for sample workers; each worker pulls next index, applies augmentation, pushes to channel; consumer collects `batch_size` samples.
  - **Files:** `crates/oxigdal-ml-foundation/src/data/dataloader.rs` (new), `src/data/dataset.rs` (replace placeholders).
  - **Tests:** (proposed) `test_dataloader_yields_correct_batch_count`, `test_dataloader_augmentation_applied_per_sample`, `test_dataloader_prefetch_does_not_block_consumer`, `test_dataloader_drop_last_partial_batch`.
  - **Risk:** Augmentation determinism across workers (see Item 2 RNG note).
  - **Prerequisites:** Item 2 (RNG-backed augmentations).

- [ ] Checkpoint save/load for training resumption
  - **Goal:** Atomic, versioned checkpoints `{epoch, model_state, optimizer_state, rng_state, schedule_state}` serialized via `oxicode` (CBOR-compatible Pure-Rust replacement per COOLJAPAN policy), with magic-byte header for forward compatibility.
  - **Design:** `Checkpoint::save(path)` writes to `path.tmp` then atomic-rename; `Checkpoint::load(path)` validates magic, version, and SHA-256 over body. Optimizer state includes momentum/variance buffers (size = parameter count).
  - **Files:** `crates/oxigdal-ml-foundation/src/training/checkpoint.rs` (new).
  - **Tests:** (proposed) `test_checkpoint_roundtrip_preserves_weights`, `test_checkpoint_version_mismatch_errors`, `test_checkpoint_atomic_on_interrupted_write`, `test_checkpoint_corrupted_hash_rejected`.
  - **Risk:** `oxicode` schema evolution — embed schema-version field and provide migration helpers.
  - **Prerequisites:** None.

## Medium Priority
- [ ] ResNet backbone with pretrained-weight loading from ONNX initializer tensors.
  - **Files:** `src/models/resnet.rs` (extend), new `src/models/weights_loader.rs`.
  - **Why deferred:** Needs ONNX import path (mirror of Item 3 export).
- [ ] Mixed-precision training (FP16 forward, FP32 grads).
  - **Files:** `src/training/precision.rs` (new).
  - **Why deferred:** Depends on `scirs2-autograd` half-precision support.
- [ ] LR schedulers: cosine annealing, warmup, step decay.
  - **Files:** `src/training/schedulers.rs` (new).
  - **Why deferred:** Lower priority than optimizer + loss correctness.
- [ ] Distributed data-parallel training (multi-device).
  - **Files:** `src/training/distributed.rs` (new).
  - **Why deferred:** Requires NCCL-equivalent (Pure-Rust gap); coordinate with oxigdal-cluster.
- [ ] Feature pyramid network (FPN) for multi-scale detection heads.
  - **Files:** `src/models/fpn.rs` (new).
  - **Why deferred:** Detection models out of scope until UNet/ResNet stable.
- [ ] Evaluation metrics (mIoU, F1, precision, recall) with confusion-matrix accumulator.
  - **Files:** `src/metrics/mod.rs` (extend).
  - **Why deferred:** Metrics scaffold exists; need streaming accumulator for >memory datasets.
- [ ] Early stopping + model selection by validation metric.
  - **Files:** `src/training/early_stopping.rs` (new).
  - **Why deferred:** Trivial once metrics/checkpoint loop is solid.
- [ ] Geospatial augmentations (CRS-jitter, spectral-band dropout).
  - **Files:** `src/augmentation/geospatial.rs` (new).
  - **Why deferred:** Generic augmentations not yet RNG-correct (see Item 2).

## Low Priority / Future (one-liners)
- [ ] Self/cross-attention layers for transformer models.
- [ ] Vision Transformer (ViT) architecture for scene classification.
- [ ] Curriculum-learning scheduler.
- [ ] Segment-Anything-Model (SAM) adapter.
- [ ] Neural Architecture Search (NAS) for geospatial models.
- [ ] Contrastive-learning pretraining for satellite imagery.

## Cross-crate dependencies
- **Blocks:** oxigdal-ml (re-enable `temporal` feature requires foundation `ml` feature surface), oxigdal-services (training endpoints).
- **Blocked by:** scirs2-autograd 0.4.4 stabilization, scirs2-core::random API stability.

## Recently completed (verbatim)
*(No completed `[x]` entries on previous TODO — all items still open.)*

---
*Last audited: 2026-05-17*
