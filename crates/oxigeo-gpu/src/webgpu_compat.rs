//! WebGPU compatibility helpers for WASM targets.
//!
//! On `wasm32` targets `wgpu` uses the browser's WebGPU API. This module
//! provides two **host-agnostic building blocks** for that path:
//!
//! - [`ShaderRegistry`]: compile-time (`include_str!`) access to the built-in
//!   WGSL shader sources, so they are available without a filesystem at runtime.
//! - [`GpuCapabilities`]: descriptors of conservative GPU limits (e.g.
//!   [`GpuCapabilities::webgpu_conservative`]) for writing limit-aware code.
//!
//! # Scope / what is NOT here (honesty)
//!
//! This module does **not** itself create a `wgpu::Instance`/adapter/device
//! against `Backends::BROWSER_WEBGPU`, nor run a compute pass in a browser — the
//! actual dispatch machinery lives in [`crate::context`]/[`crate::compute`] and
//! runs identically on native and `wasm32` because `wgpu` abstracts the backend.
//! End-to-end browser WebGPU execution has not been exercised by an automated
//! test in this crate (it requires `wasm-bindgen-test` under a headless browser);
//! treat the WebGPU path as *supported via `wgpu` but not CI-verified here*. The
//! registry and capability structs below are fully tested pure-Rust logic.

/// Shader source registry for all built-in compute shaders.
///
/// Shaders are embedded at compile time via `include_str!`, so they are
/// always available regardless of the file-system at runtime.
pub struct ShaderRegistry;

impl ShaderRegistry {
    /// Return the WGSL source for a named shader, or `None` if unknown.
    ///
    /// # Known names
    /// - `"reproject"` — raster reprojection
    /// - `"raster_algebra"` — element-wise band math
    /// - `"hillshade"` — terrain hillshade
    pub fn get(name: &str) -> Option<&'static str> {
        match name {
            "reproject" => Some(include_str!("shaders/reproject.wgsl")),
            "raster_algebra" => Some(include_str!("shaders/raster_algebra.wgsl")),
            "hillshade" => Some(include_str!("shaders/hillshade.wgsl")),
            _ => None,
        }
    }

    /// Return the list of all available shader names.
    pub fn list() -> &'static [&'static str] {
        &["reproject", "raster_algebra", "hillshade"]
    }
}

/// GPU feature capabilities descriptor.
///
/// Use [`GpuCapabilities::default`] for native targets and
/// [`GpuCapabilities::webgpu_conservative`] when targeting WebGPU / WASM.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuCapabilities {
    /// True when compute shaders are supported.
    pub has_compute: bool,
    /// True when `texture_float` / `float32-filterable` is available.
    pub has_texture_float: bool,
    /// Maximum total invocations per workgroup (product of x * y * z sizes).
    pub max_workgroup_size: u32,
    /// Maximum size of a single buffer in bytes.
    pub max_buffer_size: u64,
}

impl Default for GpuCapabilities {
    fn default() -> Self {
        Self {
            has_compute: true,
            has_texture_float: true,
            max_workgroup_size: 256,
            max_buffer_size: 256 * 1024 * 1024, // 256 MiB
        }
    }
}

impl GpuCapabilities {
    /// Conservative capabilities for a WebGPU (WASM) context.
    ///
    /// Buffer size is limited to 128 MiB as mandated by the WebGPU
    /// specification minimum.
    pub fn webgpu_conservative() -> Self {
        Self {
            has_compute: true,
            has_texture_float: true,
            max_workgroup_size: 256,
            max_buffer_size: 128 * 1024 * 1024, // 128 MiB
        }
    }

    /// Returns `true` when `size` bytes fit within the maximum buffer limit.
    pub fn validate_buffer_size(&self, size: u64) -> bool {
        size <= self.max_buffer_size
    }

    /// Returns `true` when `size` invocations fit within the workgroup limit.
    pub fn validate_workgroup(&self, size: u32) -> bool {
        size <= self.max_workgroup_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shader_registry_known_names() {
        for name in ShaderRegistry::list() {
            assert!(
                ShaderRegistry::get(name).is_some(),
                "expected shader '{}' to be registered",
                name
            );
        }
    }

    #[test]
    fn test_shader_registry_unknown() {
        assert!(ShaderRegistry::get("nonexistent_shader").is_none());
    }

    #[test]
    fn test_capabilities_default() {
        let caps = GpuCapabilities::default();
        assert!(caps.has_compute);
        assert_eq!(caps.max_workgroup_size, 256);
        assert_eq!(caps.max_buffer_size, 256 * 1024 * 1024);
    }

    #[test]
    fn test_capabilities_webgpu() {
        let caps = GpuCapabilities::webgpu_conservative();
        assert_eq!(caps.max_buffer_size, 128 * 1024 * 1024);
    }
}
