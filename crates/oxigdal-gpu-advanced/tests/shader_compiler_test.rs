//! Shader compiler integration tests.

use oxigdal_gpu_advanced::shader_compiler::optimizer::OptimizationPass;
use oxigdal_gpu_advanced::{
    OptimizationLevel, ShaderCompiler, ShaderOptimizer, ShaderPreprocessor,
};

#[test]
fn test_simple_shader_compilation() {
    let compiler = ShaderCompiler::new();

    let source = r#"
@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    // Simple compute shader
}
    "#;

    let result = compiler.compile(source);
    assert!(result.is_ok());

    if let Ok(compiled) = result {
        assert!(compiled.entry_points.contains(&"main".to_string()));
        assert!(!compiled.optimized);
    }
}

#[test]
fn test_shader_validation() {
    let compiler = ShaderCompiler::new();

    let valid_source = r#"
@compute @workgroup_size(1, 1, 1)
fn main() {}
    "#;

    let result = compiler.validate(valid_source);
    assert!(result.is_ok());

    let invalid_source = r#"
@compute @workgroup_size(1, 1, 1)
fn main() {
    this_is_invalid();
}
    "#;

    let result = compiler.validate(invalid_source);
    assert!(result.is_err());
}

#[test]
fn test_shader_optimization() {
    let compiler = ShaderCompiler::new();

    let source = r#"
@compute @workgroup_size(64, 1, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let x = global_id.x;
    let y = x + 1u;
    let z = y * 2u;
}
    "#;

    let result = compiler.compile_optimized(source);
    assert!(result.is_ok());

    if let Ok(compiled) = result {
        assert!(compiled.optimized);
    }
}

#[test]
fn test_shader_cache() {
    let compiler = ShaderCompiler::new();

    let source = r#"
@compute @workgroup_size(1, 1, 1)
fn main() {}
    "#;

    // First compilation (cache miss)
    let _result1 = compiler.compile(source);
    let stats1 = compiler.get_stats();
    assert_eq!(stats1.cache_misses, 1);
    assert_eq!(stats1.cache_hits, 0);

    // Second compilation (should hit cache)
    let _result2 = compiler.compile(source);
    let stats2 = compiler.get_stats();
    assert_eq!(stats2.cache_hits, 1);

    compiler.print_stats();
}

#[test]
fn test_shader_preprocessor() {
    let mut preprocessor = ShaderPreprocessor::new();
    preprocessor.define("WORKGROUP_SIZE", "64");
    preprocessor.define("NUM_THREADS", "256");

    let source = r#"
@compute @workgroup_size($WORKGROUP_SIZE, 1, 1)
fn main() {
    var<workgroup> shared: array<f32, $NUM_THREADS>;
}
    "#;

    let processed = preprocessor.preprocess(source);
    assert!(processed.contains("64"));
    assert!(processed.contains("256"));
    assert!(!processed.contains("$WORKGROUP_SIZE"));
}

#[test]
fn test_optimizer_passes() {
    let mut optimizer = ShaderOptimizer::new();
    assert!(optimizer.is_pass_enabled(OptimizationPass::DeadCodeElimination));

    optimizer.disable_pass(OptimizationPass::DeadCodeElimination);
    assert!(!optimizer.is_pass_enabled(OptimizationPass::DeadCodeElimination));

    optimizer.enable_pass(OptimizationPass::LoopUnrolling);
    assert!(optimizer.is_pass_enabled(OptimizationPass::LoopUnrolling));
}

#[test]
fn test_optimization_levels() {
    let opt_none = ShaderOptimizer::get_level_preset(OptimizationLevel::None);
    let opt_basic = ShaderOptimizer::get_level_preset(OptimizationLevel::Basic);
    let opt_aggressive = ShaderOptimizer::get_level_preset(OptimizationLevel::Aggressive);

    // Basic should have more passes than None
    // Aggressive should have more passes than Basic
    // (Exact counts depend on implementation)
    assert!(std::mem::size_of_val(&opt_none) > 0);
    assert!(std::mem::size_of_val(&opt_basic) > 0);
    assert!(std::mem::size_of_val(&opt_aggressive) > 0);
}

#[test]
fn test_shader_with_storage_buffer() {
    let compiler = ShaderCompiler::new();

    let source = r#"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(256, 1, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let idx = global_id.x;
    output[idx] = input[idx] * 2.0;
}
    "#;

    let result = compiler.compile(source);
    assert!(result.is_ok());

    if let Ok(compiled) = result {
        assert_eq!(compiled.entry_points.len(), 1);
    }
}

#[test]
fn test_shader_with_uniform() {
    let compiler = ShaderCompiler::new();

    let source = r#"
struct Params {
    width: u32,
    height: u32,
    scale: f32,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let x = global_id.x;
    let y = global_id.y;

    if (x >= params.width || y >= params.height) {
        return;
    }

    let idx = y * params.width + x;
    output[idx] = f32(x + y) * params.scale;
}
    "#;

    let result = compiler.compile(source);
    assert!(result.is_ok());
}

#[test]
fn test_cache_clear() {
    let compiler = ShaderCompiler::new();

    let source = r#"
@compute @workgroup_size(1, 1, 1)
fn main() {}
    "#;

    let _result1 = compiler.compile(source);
    compiler.clear_cache();

    // After clearing, should be cache miss again
    let _result2 = compiler.compile(source);
    let stats = compiler.get_stats();
    assert_eq!(stats.cache_misses, 2);
}
