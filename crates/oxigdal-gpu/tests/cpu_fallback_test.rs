//! CPU-only integration tests for the cpu_fallback module.
//!
//! All tests here exercise purely in-process logic — no wgpu device is
//! created, so these pass on machines without GPU hardware.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use oxigdal_gpu::{
    GpuError,
    cpu_fallback::{ExecutionPath, FallbackConfig, FallbackResult, execute_with_fallback},
};

// ---------------------------------------------------------------------------
// FallbackConfig construction
// ---------------------------------------------------------------------------

#[test]
fn test_fallback_config_default_enabled() {
    let cfg = FallbackConfig::default();
    assert!(cfg.enabled, "FallbackConfig::default() must be enabled");
    assert!(cfg.trigger_on_device_lost);
    assert!(cfg.trigger_on_no_backend);
    assert!(cfg.timeout_ms.is_none());
    assert!(!cfg.log_on_fallback);
}

#[test]
fn test_fallback_config_disabled() {
    let cfg = FallbackConfig::disabled();
    assert!(
        !cfg.enabled,
        "FallbackConfig::disabled() must not be enabled"
    );
}

// ---------------------------------------------------------------------------
// should_fallback decision logic
// ---------------------------------------------------------------------------

#[test]
fn test_should_fallback_device_lost_error() {
    let cfg = FallbackConfig::default();
    let err = GpuError::device_lost("test device lost");
    assert!(
        cfg.should_fallback(&err),
        "DeviceLost must trigger fallback by default"
    );
}

#[test]
fn test_should_fallback_no_adapter_error() {
    let cfg = FallbackConfig::default();
    let err = GpuError::no_adapter("Vulkan, Metal, DX12");
    assert!(
        cfg.should_fallback(&err),
        "NoAdapter must trigger fallback when trigger_on_no_backend is true"
    );
}

#[test]
fn test_should_fallback_backend_not_available() {
    let cfg = FallbackConfig::default();
    let err = GpuError::backend_not_available("Vulkan");
    assert!(
        cfg.should_fallback(&err),
        "BackendNotAvailable must trigger fallback when trigger_on_no_backend is true"
    );
}

#[test]
fn test_should_fallback_non_fallback_error() {
    let cfg = FallbackConfig::default();
    // An error that is not in the fallback-trigger set.
    let err = GpuError::shader_compilation("syntax error at line 5");
    assert!(
        !cfg.should_fallback(&err),
        "ShaderCompilation must NOT trigger fallback"
    );
}

#[test]
fn test_should_fallback_disabled_config_always_false() {
    let cfg = FallbackConfig::disabled();
    let err = GpuError::device_lost("lost");
    assert!(
        !cfg.should_fallback(&err),
        "Disabled config must never return true from should_fallback"
    );
}

#[test]
fn test_should_fallback_device_lost_opt_out() {
    // Explicitly disable device-lost trigger while keeping no-backend trigger.
    let cfg = FallbackConfig {
        enabled: true,
        trigger_on_device_lost: false,
        trigger_on_no_backend: true,
        ..Default::default()
    };
    let err = GpuError::device_lost("lost");
    assert!(
        !cfg.should_fallback(&err),
        "DeviceLost must NOT trigger when trigger_on_device_lost == false"
    );
}

// ---------------------------------------------------------------------------
// execute_with_fallback — execution path selection
// ---------------------------------------------------------------------------

#[test]
fn test_execute_with_fallback_gpu_succeeds() {
    let cfg = FallbackConfig::default();
    let mut cpu_called = false;

    let result: FallbackResult<u32> = execute_with_fallback(
        &cfg,
        || Ok(99_u32),
        || {
            cpu_called = true;
            0_u32
        },
    )
    .expect("execute_with_fallback must not fail when GPU succeeds");

    assert_eq!(result.value, 99);
    assert_eq!(result.path, ExecutionPath::Gpu);
    assert!(!cpu_called, "cpu_op must not be called when GPU succeeds");
}

#[test]
fn test_execute_with_fallback_device_lost_uses_cpu() {
    let cfg = FallbackConfig::default();

    let result: FallbackResult<u32> = execute_with_fallback(
        &cfg,
        || Err::<u32, GpuError>(GpuError::device_lost("simulated loss")),
        || 42_u32,
    )
    .expect("execute_with_fallback must succeed via CPU when GPU is lost");

    assert_eq!(result.value, 42);
    assert_eq!(result.path, ExecutionPath::Cpu);
}

#[test]
fn test_execute_with_fallback_no_adapter_uses_cpu() {
    let cfg = FallbackConfig::default();

    let result: FallbackResult<String> = execute_with_fallback(
        &cfg,
        || Err::<String, GpuError>(GpuError::no_adapter("none")),
        || "cpu-result".to_owned(),
    )
    .expect("execute_with_fallback must succeed via CPU when no adapter");

    assert_eq!(result.value, "cpu-result");
    assert_eq!(result.path, ExecutionPath::Cpu);
}

#[test]
fn test_execute_with_fallback_disabled_propagates_error() {
    let cfg = FallbackConfig::disabled();

    let err = execute_with_fallback(
        &cfg,
        || Err::<u32, GpuError>(GpuError::device_lost("lost")),
        || 0_u32,
    )
    .expect_err("Disabled fallback config must propagate the GPU error");

    assert!(
        matches!(err, GpuError::DeviceLost { .. }),
        "propagated error must be DeviceLost, got: {err}"
    );
}

#[test]
fn test_execute_with_fallback_non_fallback_error_propagates() {
    let cfg = FallbackConfig::default();

    let err = execute_with_fallback(
        &cfg,
        || Err::<u32, GpuError>(GpuError::shader_compilation("bad shader")),
        || 0_u32,
    )
    .expect_err("Non-fallback GPU error must be propagated");

    assert!(
        matches!(err, GpuError::ShaderCompilation { .. }),
        "propagated error must be ShaderCompilation, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// FallbackResult path field
// ---------------------------------------------------------------------------

#[test]
fn test_fallback_result_path_field_gpu() {
    let result: FallbackResult<i32> =
        execute_with_fallback(&FallbackConfig::default(), || Ok(7_i32), || -1_i32).unwrap();
    assert_eq!(result.path, ExecutionPath::Gpu);
}

#[test]
fn test_fallback_result_path_field_cpu() {
    let result: FallbackResult<i32> = execute_with_fallback(
        &FallbackConfig::default(),
        || Err::<i32, GpuError>(GpuError::device_lost("gone")),
        || -1_i32,
    )
    .unwrap();
    assert_eq!(result.path, ExecutionPath::Cpu);
}

/// Verifies that `ExecutionPath::Gpu` and `Cpu` are distinct values.
#[test]
fn test_execution_path_variants_are_distinct() {
    assert_ne!(ExecutionPath::Gpu, ExecutionPath::Cpu);
}

// ---------------------------------------------------------------------------
// cpu primitive operations
// ---------------------------------------------------------------------------

#[test]
fn test_cpu_add_slices_correct() {
    use oxigdal_gpu::cpu;
    let a = [1.0_f32, 2.0, 3.0];
    let b = [4.0_f32, 5.0, 6.0];
    let out = cpu::add_slices(&a, &b);
    assert_eq!(out, vec![5.0_f32, 7.0, 9.0]);
}

#[test]
fn test_cpu_sub_slices_correct() {
    use oxigdal_gpu::cpu;
    let out = cpu::sub_slices(&[10.0_f32, 5.0, 1.0], &[1.0, 2.0, 1.0]);
    assert_eq!(out, vec![9.0_f32, 3.0, 0.0]);
}

#[test]
fn test_cpu_mul_slices_correct() {
    use oxigdal_gpu::cpu;
    let out = cpu::mul_slices(&[2.0_f32, 3.0, 4.0], &[2.0, 2.0, 2.0]);
    assert_eq!(out, vec![4.0_f32, 6.0, 8.0]);
}

#[test]
fn test_cpu_max_slices_correct() {
    use oxigdal_gpu::cpu;
    let out = cpu::max_slices(&[1.0_f32, 5.0, 3.0], &[4.0, 2.0, 3.0]);
    assert_eq!(out, vec![4.0_f32, 5.0, 3.0]);
}

#[test]
fn test_cpu_min_slices_correct() {
    use oxigdal_gpu::cpu;
    let out = cpu::min_slices(&[1.0_f32, 5.0, 3.0], &[4.0, 2.0, 3.0]);
    assert_eq!(out, vec![1.0_f32, 2.0, 3.0]);
}

#[test]
fn test_cpu_add_scalar_correct() {
    use oxigdal_gpu::cpu;
    let out = cpu::add_scalar(&[1.0_f32, 2.0, 3.0], 10.0);
    assert_eq!(out, vec![11.0_f32, 12.0, 13.0]);
}

#[test]
fn test_cpu_mul_scalar_correct() {
    use oxigdal_gpu::cpu;
    let out = cpu::mul_scalar(&[1.0_f32, 2.0, 3.0], 3.0);
    assert_eq!(out, vec![3.0_f32, 6.0, 9.0]);
}

#[test]
fn test_cpu_mean_empty_returns_zero() {
    use oxigdal_gpu::cpu;
    assert_eq!(cpu::mean(&[]), 0.0_f32);
}

#[test]
fn test_cpu_mean_non_empty() {
    use oxigdal_gpu::cpu;
    let m = cpu::mean(&[1.0_f32, 2.0, 3.0, 4.0]);
    assert!((m - 2.5).abs() < 1e-6, "mean should be 2.5, got {m}");
}

#[test]
fn test_cpu_min_value_empty() {
    use oxigdal_gpu::cpu;
    assert_eq!(cpu::min_value(&[]), f32::MAX);
}

#[test]
fn test_cpu_max_value_empty() {
    use oxigdal_gpu::cpu;
    assert_eq!(cpu::max_value(&[]), f32::MIN);
}

#[test]
fn test_cpu_min_value_correct() {
    use oxigdal_gpu::cpu;
    assert_eq!(cpu::min_value(&[3.0_f32, 1.0, 4.0, 1.5]), 1.0);
}

#[test]
fn test_cpu_max_value_correct() {
    use oxigdal_gpu::cpu;
    assert_eq!(cpu::max_value(&[3.0_f32, 1.0, 4.0, 1.5]), 4.0);
}

#[test]
fn test_cpu_variance_single_element_is_zero() {
    use oxigdal_gpu::cpu;
    assert_eq!(cpu::variance(&[7.0_f32]), 0.0);
}

#[test]
fn test_cpu_variance_correct() {
    use oxigdal_gpu::cpu;
    // Population variance of [2, 4, 4, 4, 5, 5, 7, 9] = 4.0
    let data = [2.0_f32, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
    let v = cpu::variance(&data);
    assert!((v - 4.0).abs() < 1e-5, "variance should be 4.0, got {v}");
}

#[test]
fn test_cpu_ndvi_zero_denominator() {
    use oxigdal_gpu::cpu;
    // red == 0 and nir == 0 → denominator is 0 → result is 0.0, not NaN
    let out = cpu::ndvi(&[0.0_f32], &[0.0_f32]);
    assert_eq!(out, vec![0.0_f32]);
}

#[test]
fn test_cpu_ndvi_correct() {
    use oxigdal_gpu::cpu;
    // NDVI = (0.4 - 0.1) / (0.4 + 0.1) = 0.3 / 0.5 = 0.6
    let out = cpu::ndvi(&[0.1_f32], &[0.4_f32]);
    assert!(
        (out[0] - 0.6).abs() < 1e-6,
        "NDVI should be 0.6, got {}",
        out[0]
    );
}

#[test]
fn test_cpu_clamp_correct() {
    use oxigdal_gpu::cpu;
    let out = cpu::clamp(&[-1.0_f32, 0.5, 2.0], 0.0, 1.0);
    assert_eq!(out, vec![0.0_f32, 0.5, 1.0]);
}

#[test]
fn test_cpu_abs_correct() {
    use oxigdal_gpu::cpu;
    let out = cpu::abs(&[-3.0_f32, 0.0, 4.0]);
    assert_eq!(out, vec![3.0_f32, 0.0, 4.0]);
}

#[test]
fn test_cpu_sqrt_correct() {
    use oxigdal_gpu::cpu;
    let out = cpu::sqrt(&[0.0_f32, 1.0, 4.0, 9.0]);
    assert_eq!(out, vec![0.0_f32, 1.0, 2.0, 3.0]);
}
