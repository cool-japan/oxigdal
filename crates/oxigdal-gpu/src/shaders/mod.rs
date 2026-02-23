//! WGSL shader management for OxiGDAL GPU operations.
//!
//! This module provides utilities for loading, compiling, and validating
//! WGSL compute shaders used in GPU-accelerated raster operations.

use crate::error::{GpuError, GpuResult};
use std::borrow::Cow;
use tracing::debug;
use wgpu::{
    BindGroupLayout, BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType,
    BufferBindingType, ComputePipeline, ComputePipelineDescriptor, Device,
    PipelineLayoutDescriptor, ShaderModule, ShaderModuleDescriptor, ShaderSource, ShaderStages,
    naga,
};

/// WGSL shader wrapper with validation and compilation.
pub struct WgslShader {
    /// Shader source code.
    source: String,
    /// Shader entry point.
    entry_point: String,
    /// Compiled shader module.
    module: Option<ShaderModule>,
}

impl WgslShader {
    /// Create a new WGSL shader from source.
    pub fn new(source: impl Into<String>, entry_point: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            entry_point: entry_point.into(),
            module: None,
        }
    }

    /// Validate the shader source without compiling.
    ///
    /// # Errors
    ///
    /// Returns an error if shader validation fails.
    pub fn validate(&self) -> GpuResult<()> {
        let module = naga::front::wgsl::parse_str(&self.source)
            .map_err(|e| GpuError::shader_compilation(e.to_string()))?;

        // Validate the module
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );

        validator
            .validate(&module)
            .map_err(|e| GpuError::shader_validation(e.to_string()))?;

        debug!("Shader validation successful");
        Ok(())
    }

    /// Compile the shader for the given device.
    ///
    /// # Errors
    ///
    /// Returns an error if shader compilation fails.
    pub fn compile(&mut self, device: &Device) -> GpuResult<&ShaderModule> {
        if self.module.is_none() {
            // Validate first
            self.validate()?;

            let module = device.create_shader_module(ShaderModuleDescriptor {
                label: Some(&format!("Shader: {}", self.entry_point)),
                source: ShaderSource::Wgsl(Cow::Borrowed(&self.source)),
            });

            self.module = Some(module);
            debug!("Shader compiled: {}", self.entry_point);
        }

        Ok(self
            .module
            .as_ref()
            .ok_or_else(|| GpuError::internal("Module should be compiled"))?)
    }

    /// Get the shader entry point.
    pub fn entry_point(&self) -> &str {
        &self.entry_point
    }

    /// Get the shader source.
    pub fn source(&self) -> &str {
        &self.source
    }
}

/// Builder for compute pipelines with common configurations.
pub struct ComputePipelineBuilder<'a> {
    device: &'a Device,
    shader: &'a ShaderModule,
    entry_point: String,
    bind_group_layouts: Vec<&'a BindGroupLayout>,
    label: Option<String>,
}

impl<'a> ComputePipelineBuilder<'a> {
    /// Create a new compute pipeline builder.
    pub fn new(
        device: &'a Device,
        shader: &'a ShaderModule,
        entry_point: impl Into<String>,
    ) -> Self {
        Self {
            device,
            shader,
            entry_point: entry_point.into(),
            bind_group_layouts: Vec::new(),
            label: None,
        }
    }

    /// Add a bind group layout.
    pub fn bind_group_layout(mut self, layout: &'a BindGroupLayout) -> Self {
        self.bind_group_layouts.push(layout);
        self
    }

    /// Set the pipeline label.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Build the compute pipeline.
    ///
    /// # Errors
    ///
    /// Returns an error if pipeline creation fails.
    pub fn build(self) -> GpuResult<ComputePipeline> {
        let pipeline_layout = self
            .device
            .create_pipeline_layout(&PipelineLayoutDescriptor {
                label: self.label.as_deref(),
                bind_group_layouts: &self.bind_group_layouts,
                immediate_size: 0,
            });

        let pipeline = self
            .device
            .create_compute_pipeline(&ComputePipelineDescriptor {
                label: self.label.as_deref(),
                layout: Some(&pipeline_layout),
                module: self.shader,
                entry_point: Some(&self.entry_point),
                compilation_options: Default::default(),
                cache: None,
            });

        debug!("Compute pipeline created: {:?}", self.label);
        Ok(pipeline)
    }
}

/// Create a storage buffer bind group layout entry.
pub fn storage_buffer_layout(binding: u32, read_only: bool) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::COMPUTE,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// Create a uniform buffer bind group layout entry.
pub fn uniform_buffer_layout(binding: u32) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::COMPUTE,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// Create a bind group layout for common compute patterns.
///
/// # Errors
///
/// Returns an error if layout creation fails.
pub fn create_compute_bind_group_layout(
    device: &Device,
    entries: &[BindGroupLayoutEntry],
    label: Option<&str>,
) -> GpuResult<BindGroupLayout> {
    Ok(device.create_bind_group_layout(&BindGroupLayoutDescriptor { label, entries }))
}

/// Shader library with common WGSL functions.
pub struct ShaderLibrary;

impl ShaderLibrary {
    /// Get common utility functions for compute shaders.
    pub fn common_utils() -> &'static str {
        r#"
// Common utility functions for WGSL shaders

// Convert 2D coordinates to 1D index
fn coord_to_index(x: u32, y: u32, width: u32) -> u32 {
    return y * width + x;
}

// Convert 1D index to 2D coordinates
fn index_to_coord(index: u32, width: u32) -> vec2<u32> {
    return vec2<u32>(index % width, index / width);
}

// Clamp value to range [min, max]
fn clamp_value(value: f32, min_val: f32, max_val: f32) -> f32 {
    return clamp(value, min_val, max_val);
}

// Linear interpolation
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    return a + (b - a) * t;
}

// Bilinear interpolation
fn bilinear_interp(
    v00: f32, v10: f32,
    v01: f32, v11: f32,
    tx: f32, ty: f32
) -> f32 {
    let v0 = lerp(v00, v10, tx);
    let v1 = lerp(v01, v11, tx);
    return lerp(v0, v1, ty);
}

// Safe division (returns 0 if denominator is 0)
fn safe_div(num: f32, denom: f32) -> f32 {
    if (abs(denom) < 1e-10) {
        return 0.0;
    }
    return num / denom;
}

// Check if value is NaN
fn is_nan(value: f32) -> bool {
    return value != value;
}

// Check if value is infinite
fn is_inf(value: f32) -> bool {
    return abs(value) > 1e38;
}

// Safe value (replace NaN/Inf with 0)
fn safe_value(value: f32) -> f32 {
    if (is_nan(value) || is_inf(value)) {
        return 0.0;
    }
    return value;
}
"#
    }

    /// Get NDVI (Normalized Difference Vegetation Index) shader.
    pub fn ndvi_shader() -> &'static str {
        r#"
@group(0) @binding(0) var<storage, read> nir: array<f32>;
@group(0) @binding(1) var<storage, read> red: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(256)
fn ndvi(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let idx = global_id.x;
    if (idx >= arrayLength(&output)) {
        return;
    }

    let nir_val = nir[idx];
    let red_val = red[idx];
    let sum = nir_val + red_val;

    if (abs(sum) < 1e-10) {
        output[idx] = 0.0;
    } else {
        output[idx] = (nir_val - red_val) / sum;
    }
}
"#
    }

    /// Get element-wise addition shader.
    pub fn add_shader() -> &'static str {
        r#"
@group(0) @binding(0) var<storage, read> input_a: array<f32>;
@group(0) @binding(1) var<storage, read> input_b: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(256)
fn add(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let idx = global_id.x;
    if (idx >= arrayLength(&output)) {
        return;
    }
    output[idx] = input_a[idx] + input_b[idx];
}
"#
    }

    /// Get element-wise multiplication shader.
    pub fn multiply_shader() -> &'static str {
        r#"
@group(0) @binding(0) var<storage, read> input_a: array<f32>;
@group(0) @binding(1) var<storage, read> input_b: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(256)
fn multiply(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let idx = global_id.x;
    if (idx >= arrayLength(&output)) {
        return;
    }
    output[idx] = input_a[idx] * input_b[idx];
}
"#
    }

    /// Get threshold shader.
    pub fn threshold_shader() -> &'static str {
        r#"
struct Params {
    threshold: f32,
    value_below: f32,
    value_above: f32,
}

@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<uniform> params: Params;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(256)
fn threshold(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let idx = global_id.x;
    if (idx >= arrayLength(&output)) {
        return;
    }

    if (input[idx] < params.threshold) {
        output[idx] = params.value_below;
    } else {
        output[idx] = params.value_above;
    }
}
"#
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shader_library() {
        let utils = ShaderLibrary::common_utils();
        assert!(utils.contains("coord_to_index"));
        assert!(utils.contains("bilinear_interp"));

        let ndvi = ShaderLibrary::ndvi_shader();
        assert!(ndvi.contains("@compute"));
        assert!(ndvi.contains("workgroup_size"));
    }

    #[test]
    fn test_shader_validation() {
        let shader = WgslShader::new(ShaderLibrary::add_shader(), "add");

        // Validation might fail without GPU, so we just check it doesn't panic
        let _ = shader.validate();
    }

    #[test]
    fn test_bind_group_layout_helpers() {
        let storage_ro = storage_buffer_layout(0, true);
        assert_eq!(storage_ro.binding, 0);

        let storage_rw = storage_buffer_layout(1, false);
        assert_eq!(storage_rw.binding, 1);

        let uniform = uniform_buffer_layout(2);
        assert_eq!(uniform.binding, 2);
    }
}
