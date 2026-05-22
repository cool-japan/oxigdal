//! WGSL shader source helpers for OxiGDAL GPU.
//!
//! This module provides utilities for generating type-parameterised WGSL
//! compute shader sources.  The primary use-case is producing element-wise
//! pass-through shaders that can be composed into larger pipelines, with
//! optional half-precision support when `BufferElementType::F16` is
//! requested.

use crate::buffer::BufferElementType;

/// Generate a single-pass element-wise identity compute shader for the given
/// element type.
///
/// The shader reads from `@binding(0)` (read-only storage) and writes the
/// same values to `@binding(1)` (read-write storage), dispatched over a
/// workgroup of size 64 in the X dimension.
///
/// When `element_type` is [`BufferElementType::F16`], the emitted WGSL
/// source begins with `enable f16;` so that `array<f16>` is a valid
/// storage type.  On adapters that do not expose `wgpu::Features::SHADER_F16`
/// this shader will fail to compile — use
/// [`crate::buffer::from_f16_slice_widening`] instead.
///
/// For all other element types no extension directive is emitted, and the
/// array element type degrades to WGSL's native types (`f32`, `u32`, `i32`).
///
/// # Examples
///
/// ```
/// use oxigdal_gpu::{BufferElementType, make_element_wise_shader_source};
///
/// let src = make_element_wise_shader_source(BufferElementType::F32);
/// assert!(src.contains("array<f32>"));
/// assert!(!src.contains("enable f16"));
///
/// let src_f16 = make_element_wise_shader_source(BufferElementType::F16);
/// assert!(src_f16.contains("enable f16"));
/// assert!(src_f16.contains("array<f16>"));
/// ```
pub fn make_element_wise_shader_source(element_type: BufferElementType) -> String {
    let enable_line = if element_type == BufferElementType::F16 {
        "enable f16;\n"
    } else {
        ""
    };
    let ty = element_type.wgsl_type();
    format!(
        r#"{enable_line}
@group(0) @binding(0) var<storage, read> input: array<{ty}>;
@group(0) @binding(1) var<storage, read_write> output: array<{ty}>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {{
    let i = gid.x;
    if i >= arrayLength(&input) {{ return; }}
    output[i] = input[i];
}}"#,
        enable_line = enable_line,
        ty = ty
    )
}
