//! Advanced GPU compute kernels and WGSL shaders.
//!
//! This module provides access to optimized WGSL shaders for various
//! geospatial and image processing operations.

/// WGSL shader for matrix operations (GEMM)
pub const MATRIX_OPS_SHADER: &str = include_str!("advanced/matrix_ops.wgsl");

/// WGSL shader for Fast Fourier Transform
pub const FFT_SHADER: &str = include_str!("advanced/fft.wgsl");

/// WGSL shader for histogram equalization
pub const HISTOGRAM_EQ_SHADER: &str = include_str!("advanced/histogram_eq.wgsl");

/// WGSL shader for morphological operations
pub const MORPHOLOGY_SHADER: &str = include_str!("advanced/morphology.wgsl");

/// WGSL shader for edge detection
pub const EDGE_DETECTION_SHADER: &str = include_str!("advanced/edge_detection.wgsl");

/// WGSL shader for texture analysis
pub const TEXTURE_ANALYSIS_SHADER: &str = include_str!("advanced/texture_analysis.wgsl");

/// Kernel registry for managing and accessing shaders
pub struct KernelRegistry {
    shaders: std::collections::HashMap<String, String>,
}

impl KernelRegistry {
    /// Create a new kernel registry with built-in shaders
    pub fn new() -> Self {
        let mut shaders = std::collections::HashMap::new();

        shaders.insert("matrix_ops".to_string(), MATRIX_OPS_SHADER.to_string());
        shaders.insert("fft".to_string(), FFT_SHADER.to_string());
        shaders.insert("histogram_eq".to_string(), HISTOGRAM_EQ_SHADER.to_string());
        shaders.insert("morphology".to_string(), MORPHOLOGY_SHADER.to_string());
        shaders.insert(
            "edge_detection".to_string(),
            EDGE_DETECTION_SHADER.to_string(),
        );
        shaders.insert(
            "texture_analysis".to_string(),
            TEXTURE_ANALYSIS_SHADER.to_string(),
        );

        Self { shaders }
    }

    /// Get a shader by name
    pub fn get_shader(&self, name: &str) -> Option<&str> {
        self.shaders.get(name).map(|s| s.as_str())
    }

    /// Register a custom shader
    pub fn register_shader(&mut self, name: String, source: String) {
        self.shaders.insert(name, source);
    }

    /// List all available shader names
    pub fn list_shaders(&self) -> Vec<&str> {
        self.shaders.keys().map(|k| k.as_str()).collect()
    }

    /// Check if a shader exists
    pub fn has_shader(&self, name: &str) -> bool {
        self.shaders.contains_key(name)
    }

    /// Remove a shader
    pub fn remove_shader(&mut self, name: &str) -> bool {
        self.shaders.remove(name).is_some()
    }

    /// Get the number of registered shaders
    pub fn shader_count(&self) -> usize {
        self.shaders.len()
    }
}

impl Default for KernelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Kernel execution parameters
#[derive(Debug, Clone)]
pub struct KernelParams {
    /// Workgroup size (x, y, z)
    pub workgroup_size: (u32, u32, u32),
    /// Number of workgroups to dispatch (x, y, z)
    pub dispatch_size: (u32, u32, u32),
    /// Entry point function name
    pub entry_point: String,
}

impl Default for KernelParams {
    fn default() -> Self {
        Self {
            workgroup_size: (8, 8, 1),
            dispatch_size: (1, 1, 1),
            entry_point: "main".to_string(),
        }
    }
}

impl KernelParams {
    /// Create new kernel parameters
    pub fn new(workgroup_size: (u32, u32, u32), dispatch_size: (u32, u32, u32)) -> Self {
        Self {
            workgroup_size,
            dispatch_size,
            entry_point: "main".to_string(),
        }
    }

    /// Set workgroup size
    pub fn with_workgroup_size(mut self, x: u32, y: u32, z: u32) -> Self {
        self.workgroup_size = (x, y, z);
        self
    }

    /// Set dispatch size
    pub fn with_dispatch_size(mut self, x: u32, y: u32, z: u32) -> Self {
        self.dispatch_size = (x, y, z);
        self
    }

    /// Set entry point
    pub fn with_entry_point(mut self, entry_point: impl Into<String>) -> Self {
        self.entry_point = entry_point.into();
        self
    }

    /// Calculate total number of threads
    pub fn total_threads(&self) -> u64 {
        let (wg_x, wg_y, wg_z) = self.workgroup_size;
        let (d_x, d_y, d_z) = self.dispatch_size;

        (wg_x as u64 * d_x as u64) * (wg_y as u64 * d_y as u64) * (wg_z as u64 * d_z as u64)
    }

    /// Calculate optimal dispatch size for a given data size
    pub fn calculate_dispatch_size(
        data_width: u32,
        data_height: u32,
        workgroup_size: (u32, u32, u32),
    ) -> (u32, u32, u32) {
        let (wg_x, wg_y, _wg_z) = workgroup_size;

        let dispatch_x = data_width.div_ceil(wg_x);
        let dispatch_y = data_height.div_ceil(wg_y);
        let dispatch_z = 1;

        (dispatch_x, dispatch_y, dispatch_z)
    }
}

/// Matrix multiplication kernel helper
pub struct MatrixMultiplyKernel;

impl MatrixMultiplyKernel {
    /// Get shader source
    pub fn shader() -> &'static str {
        MATRIX_OPS_SHADER
    }

    /// Create parameters for matrix multiplication
    pub fn params(m: u32, n: u32, _k: u32, tiled: bool) -> KernelParams {
        if tiled {
            let workgroup_size = (16, 16, 1);
            let dispatch_x = n.div_ceil(16);
            let dispatch_y = m.div_ceil(16);

            KernelParams {
                workgroup_size,
                dispatch_size: (dispatch_x, dispatch_y, 1),
                entry_point: "matrix_multiply_tiled".to_string(),
            }
        } else {
            let workgroup_size = (8, 8, 1);
            let dispatch_x = n.div_ceil(8);
            let dispatch_y = m.div_ceil(8);

            KernelParams {
                workgroup_size,
                dispatch_size: (dispatch_x, dispatch_y, 1),
                entry_point: "matrix_multiply_naive".to_string(),
            }
        }
    }
}

/// FFT kernel helper
pub struct FftKernel;

impl FftKernel {
    /// Get shader source
    pub fn shader() -> &'static str {
        FFT_SHADER
    }

    /// Create parameters for FFT
    pub fn params(n: u32) -> KernelParams {
        let workgroup_size = (256, 1, 1);
        let dispatch_size = (n.div_ceil(256), 1, 1);

        KernelParams {
            workgroup_size,
            dispatch_size,
            entry_point: "fft_cooley_tukey".to_string(),
        }
    }

    /// Calculate number of FFT stages needed
    pub fn num_stages(n: u32) -> u32 {
        (n as f32).log2() as u32
    }
}

/// Histogram equalization kernel helper
pub struct HistogramEqKernel;

impl HistogramEqKernel {
    /// Get shader source
    pub fn shader() -> &'static str {
        HISTOGRAM_EQ_SHADER
    }

    /// Create parameters for histogram computation
    pub fn compute_histogram_params(width: u32, height: u32) -> KernelParams {
        let workgroup_size = (16, 16, 1);
        let dispatch_x = width.div_ceil(16);
        let dispatch_y = height.div_ceil(16);

        KernelParams {
            workgroup_size,
            dispatch_size: (dispatch_x, dispatch_y, 1),
            entry_point: "compute_histogram".to_string(),
        }
    }

    /// Create parameters for equalization
    pub fn equalize_params(width: u32, height: u32) -> KernelParams {
        let workgroup_size = (16, 16, 1);
        let dispatch_x = width.div_ceil(16);
        let dispatch_y = height.div_ceil(16);

        KernelParams {
            workgroup_size,
            dispatch_size: (dispatch_x, dispatch_y, 1),
            entry_point: "histogram_equalize".to_string(),
        }
    }
}

/// Edge detection kernel helper
pub struct EdgeDetectionKernel;

impl EdgeDetectionKernel {
    /// Get shader source
    pub fn shader() -> &'static str {
        EDGE_DETECTION_SHADER
    }

    /// Create parameters for Sobel edge detection
    pub fn sobel_params(width: u32, height: u32) -> KernelParams {
        let workgroup_size = (16, 16, 1);
        let dispatch_x = width.div_ceil(16);
        let dispatch_y = height.div_ceil(16);

        KernelParams {
            workgroup_size,
            dispatch_size: (dispatch_x, dispatch_y, 1),
            entry_point: "sobel".to_string(),
        }
    }

    /// Create parameters for Canny edge detection (gradient step)
    pub fn canny_gradient_params(width: u32, height: u32) -> KernelParams {
        let workgroup_size = (16, 16, 1);
        let dispatch_x = width.div_ceil(16);
        let dispatch_y = height.div_ceil(16);

        KernelParams {
            workgroup_size,
            dispatch_size: (dispatch_x, dispatch_y, 1),
            entry_point: "canny_gradient".to_string(),
        }
    }
}

/// Morphology kernel helper
pub struct MorphologyKernel;

impl MorphologyKernel {
    /// Get shader source
    pub fn shader() -> &'static str {
        MORPHOLOGY_SHADER
    }

    /// Create parameters for dilation
    pub fn dilate_params(width: u32, height: u32) -> KernelParams {
        let workgroup_size = (16, 16, 1);
        let dispatch_x = width.div_ceil(16);
        let dispatch_y = height.div_ceil(16);

        KernelParams {
            workgroup_size,
            dispatch_size: (dispatch_x, dispatch_y, 1),
            entry_point: "dilate".to_string(),
        }
    }

    /// Create parameters for erosion
    pub fn erode_params(width: u32, height: u32) -> KernelParams {
        let workgroup_size = (16, 16, 1);
        let dispatch_x = width.div_ceil(16);
        let dispatch_y = height.div_ceil(16);

        KernelParams {
            workgroup_size,
            dispatch_size: (dispatch_x, dispatch_y, 1),
            entry_point: "erode".to_string(),
        }
    }
}

/// Texture analysis kernel helper
pub struct TextureAnalysisKernel;

impl TextureAnalysisKernel {
    /// Get shader source
    pub fn shader() -> &'static str {
        TEXTURE_ANALYSIS_SHADER
    }

    /// Create parameters for GLCM computation
    pub fn glcm_params(width: u32, height: u32) -> KernelParams {
        let workgroup_size = (16, 16, 1);
        let dispatch_x = width.div_ceil(16);
        let dispatch_y = height.div_ceil(16);

        KernelParams {
            workgroup_size,
            dispatch_size: (dispatch_x, dispatch_y, 1),
            entry_point: "compute_glcm".to_string(),
        }
    }

    /// Create parameters for LBP (Local Binary Pattern)
    pub fn lbp_params(width: u32, height: u32) -> KernelParams {
        let workgroup_size = (16, 16, 1);
        let dispatch_x = width.div_ceil(16);
        let dispatch_y = height.div_ceil(16);

        KernelParams {
            workgroup_size,
            dispatch_size: (dispatch_x, dispatch_y, 1),
            entry_point: "local_binary_pattern".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kernel_registry() {
        let registry = KernelRegistry::new();
        assert_eq!(registry.shader_count(), 6);
        assert!(registry.has_shader("matrix_ops"));
        assert!(registry.has_shader("fft"));
        assert!(registry.has_shader("histogram_eq"));
    }

    #[test]
    fn test_kernel_registry_custom() {
        let mut registry = KernelRegistry::new();
        let initial_count = registry.shader_count();

        registry.register_shader("custom".to_string(), "custom shader code".to_string());
        assert_eq!(registry.shader_count(), initial_count + 1);
        assert!(registry.has_shader("custom"));

        assert!(registry.remove_shader("custom"));
        assert_eq!(registry.shader_count(), initial_count);
    }

    #[test]
    fn test_kernel_params() {
        let params = KernelParams::default();
        assert_eq!(params.workgroup_size, (8, 8, 1));
        assert_eq!(params.entry_point, "main");
    }

    #[test]
    fn test_kernel_params_total_threads() {
        let params = KernelParams::new((8, 8, 1), (10, 10, 1));
        assert_eq!(params.total_threads(), 8 * 8 * 10 * 10);
    }

    #[test]
    fn test_calculate_dispatch_size() {
        let (dx, dy, dz) = KernelParams::calculate_dispatch_size(1920, 1080, (16, 16, 1));
        assert_eq!(dx, 1920_u32.div_ceil(16));
        assert_eq!(dy, 1080_u32.div_ceil(16));
        assert_eq!(dz, 1);
    }

    #[test]
    fn test_matrix_multiply_kernel() {
        let params = MatrixMultiplyKernel::params(1024, 1024, 1024, true);
        assert_eq!(params.entry_point, "matrix_multiply_tiled");
        assert_eq!(params.workgroup_size, (16, 16, 1));
    }

    #[test]
    fn test_fft_kernel() {
        let params = FftKernel::params(1024);
        assert_eq!(params.entry_point, "fft_cooley_tukey");

        let stages = FftKernel::num_stages(1024);
        assert_eq!(stages, 10); // log2(1024) = 10
    }

    #[test]
    fn test_all_shaders_available() {
        assert!(!MATRIX_OPS_SHADER.is_empty());
        assert!(!FFT_SHADER.is_empty());
        assert!(!HISTOGRAM_EQ_SHADER.is_empty());
        assert!(!MORPHOLOGY_SHADER.is_empty());
        assert!(!EDGE_DETECTION_SHADER.is_empty());
        assert!(!TEXTURE_ANALYSIS_SHADER.is_empty());
    }
}
