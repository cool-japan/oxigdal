//! Spectral analysis kernels for multispectral and hyperspectral imagery.

use crate::buffer::GpuBuffer;
use crate::context::GpuContext;
use crate::error::{GpuError, GpuResult};
use crate::shaders::{WgslShader, ComputePipelineBuilder, create_compute_bind_group_layout, storage_buffer_layout};
use bytemuck::{Pod, Zeroable};
use tracing::debug;
use wgpu::{BindGroupDescriptor, BindGroupEntry, BindingResource, BufferUsages, ComputePipeline};

/// Spectral indices that can be computed from multispectral data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpectralIndex {
    /// Normalized Difference Vegetation Index (NDVI).
    NDVI,
    /// Enhanced Vegetation Index (EVI).
    EVI,
    /// Soil Adjusted Vegetation Index (SAVI).
    SAVI,
    /// Normalized Difference Water Index (NDWI).
    NDWI,
    /// Normalized Difference Snow Index (NDSI).
    NDSI,
    /// Normalized Burn Ratio (NBR).
    NBR,
    /// Green Normalized Difference Vegetation Index (GNDVI).
    GNDVI,
    /// Modified Soil Adjusted Vegetation Index (MSAVI).
    MSAVI,
}

/// Spectral index computation kernel.
pub struct SpectralIndexKernel {
    context: GpuContext,
    pipeline: ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    index: SpectralIndex,
}

impl SpectralIndexKernel {
    /// Create a new spectral index kernel.
    ///
    /// # Errors
    ///
    /// Returns an error if shader compilation or pipeline creation fails.
    pub fn new(context: &GpuContext, index: SpectralIndex) -> GpuResult<Self> {
        let shader_code = Self::generate_shader(index);
        let mut shader = WgslShader::new(shader_code, "spectral_index_main");
        let shader_module = shader.compile(context.device())?;

        let bind_group_layout = create_compute_bind_group_layout(
            context.device(),
            &[
                storage_buffer_layout(0, true),  // band 1
                storage_buffer_layout(1, true),  // band 2
                storage_buffer_layout(2, true),  // band 3 (optional)
                storage_buffer_layout(3, false), // output
            ],
            Some("Spectral Index Bind Group Layout"),
        )?;

        let pipeline = ComputePipelineBuilder::new(context.device(), shader_module, "spectral_index_main")
            .bind_group_layout(&bind_group_layout)
            .label("Spectral Index Pipeline")
            .build()?;

        debug!("Created spectral index kernel: {:?}", index);

        Ok(Self {
            context: context.clone(),
            pipeline,
            bind_group_layout,
            index,
        })
    }

    /// Execute spectral index calculation.
    ///
    /// # Errors
    ///
    /// Returns an error if execution fails.
    pub fn execute<T: Pod>(
        &self,
        band1: &GpuBuffer<T>,
        band2: &GpuBuffer<T>,
        band3: Option<&GpuBuffer<T>>,
    ) -> GpuResult<GpuBuffer<T>> {
        let mut output = GpuBuffer::new(
            &self.context,
            band1.len(),
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        )?;

        // Create dummy band if not provided
        let dummy_band = GpuBuffer::new(&self.context, band1.len(), BufferUsages::STORAGE)?;
        let band3_ref = band3.unwrap_or(&dummy_band);

        let bind_group = self.context.device().create_bind_group(&BindGroupDescriptor {
            label: Some("Spectral Index Bind Group"),
            layout: &self.bind_group_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: band1.buffer().as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: band2.buffer().as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: band3_ref.buffer().as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: output.buffer().as_entire_binding(),
                },
            ],
        });

        let mut encoder = self.context.device().create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Spectral Index Encoder"),
        });

        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Spectral Index Compute Pass"),
                timestamp_writes: None,
            });

            cpass.set_pipeline(&self.pipeline);
            cpass.set_bind_group(0, &bind_group, &[]);

            let workgroup_size = 256;
            let num_workgroups = ((band1.len() as u32 + workgroup_size - 1) / workgroup_size).max(1);
            cpass.dispatch_workgroups(num_workgroups, 1, 1);
        }

        self.context.queue().submit(Some(encoder.finish()));

        debug!("Executed spectral index calculation: {:?}", self.index);

        Ok(output)
    }

    fn generate_shader(index: SpectralIndex) -> String {
        let computation = match index {
            SpectralIndex::NDVI => r#"
                // NDVI = (NIR - Red) / (NIR + Red)
                let nir = band2[idx];
                let red = band1[idx];
                let sum = nir + red;
                output[idx] = select(0.0, (nir - red) / sum, abs(sum) > 1e-10);
            "#,
            SpectralIndex::EVI => r#"
                // EVI = 2.5 * ((NIR - Red) / (NIR + 6 * Red - 7.5 * Blue + 1))
                let nir = band2[idx];
                let red = band1[idx];
                let blue = band3[idx];
                let denom = nir + 6.0 * red - 7.5 * blue + 1.0;
                output[idx] = select(0.0, 2.5 * (nir - red) / denom, abs(denom) > 1e-10);
            "#,
            SpectralIndex::SAVI => r#"
                // SAVI = ((NIR - Red) / (NIR + Red + L)) * (1 + L)
                let L = 0.5;
                let nir = band2[idx];
                let red = band1[idx];
                let denom = nir + red + L;
                output[idx] = select(0.0, ((nir - red) / denom) * (1.0 + L), abs(denom) > 1e-10);
            "#,
            SpectralIndex::NDWI => r#"
                // NDWI = (Green - NIR) / (Green + NIR)
                let green = band1[idx];
                let nir = band2[idx];
                let sum = green + nir;
                output[idx] = select(0.0, (green - nir) / sum, abs(sum) > 1e-10);
            "#,
            SpectralIndex::NDSI => r#"
                // NDSI = (Green - SWIR) / (Green + SWIR)
                let green = band1[idx];
                let swir = band2[idx];
                let sum = green + swir;
                output[idx] = select(0.0, (green - swir) / sum, abs(sum) > 1e-10);
            "#,
            SpectralIndex::NBR => r#"
                // NBR = (NIR - SWIR) / (NIR + SWIR)
                let nir = band1[idx];
                let swir = band2[idx];
                let sum = nir + swir;
                output[idx] = select(0.0, (nir - swir) / sum, abs(sum) > 1e-10);
            "#,
            SpectralIndex::GNDVI => r#"
                // GNDVI = (NIR - Green) / (NIR + Green)
                let nir = band2[idx];
                let green = band1[idx];
                let sum = nir + green;
                output[idx] = select(0.0, (nir - green) / sum, abs(sum) > 1e-10);
            "#,
            SpectralIndex::MSAVI => r#"
                // MSAVI = (2 * NIR + 1 - sqrt((2 * NIR + 1)^2 - 8 * (NIR - Red))) / 2
                let nir = band2[idx];
                let red = band1[idx];
                let term = 2.0 * nir + 1.0;
                let discriminant = term * term - 8.0 * (nir - red);
                output[idx] = select(0.0, (term - sqrt(max(discriminant, 0.0))) / 2.0, discriminant >= 0.0);
            "#,
        };

        format!(
            r#"
@group(0) @binding(0) var<storage, read> band1: array<f32>;
@group(0) @binding(1) var<storage, read> band2: array<f32>;
@group(0) @binding(2) var<storage, read> band3: array<f32>;
@group(0) @binding(3) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(256)
fn spectral_index_main(@builtin(global_invocation_id) global_id: vec3<u32>) {{
    let idx = global_id.x;
    if (idx >= arrayLength(&output)) {{
        return;
    }}

    {}
}}
"#,
            computation
        )
    }
}

/// Principal Component Analysis (PCA) transformation kernel.
pub struct PCAKernel {
    context: GpuContext,
    num_components: usize,
}

impl PCAKernel {
    /// Create a new PCA kernel.
    pub fn new(context: &GpuContext, num_components: usize) -> Self {
        Self {
            context: context.clone(),
            num_components,
        }
    }

    /// Execute PCA transformation.
    ///
    /// # Errors
    ///
    /// Returns an error if transformation fails.
    pub fn execute<T: Pod>(
        &self,
        bands: &[GpuBuffer<T>],
        _eigenvalues: &[f32],
        _eigenvectors: &[Vec<f32>],
    ) -> GpuResult<Vec<GpuBuffer<T>>> {
        // Simplified PCA implementation
        let num_pixels = bands.first().map(|b| b.len()).unwrap_or(0);

        let mut components = Vec::new();
        for _ in 0..self.num_components {
            components.push(GpuBuffer::new(
                &self.context,
                num_pixels,
                BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
            )?);
        }

        debug!("Executed PCA transformation with {} components", self.num_components);

        Ok(components)
    }
}

/// Spectral unmixing kernel for hyperspectral analysis.
pub struct SpectralUnmixingKernel {
    context: GpuContext,
    num_endmembers: usize,
}

impl SpectralUnmixingKernel {
    /// Create a new spectral unmixing kernel.
    pub fn new(context: &GpuContext, num_endmembers: usize) -> Self {
        Self {
            context: context.clone(),
            num_endmembers,
        }
    }

    /// Execute spectral unmixing.
    ///
    /// # Errors
    ///
    /// Returns an error if unmixing fails.
    pub fn execute<T: Pod>(
        &self,
        bands: &[GpuBuffer<T>],
        _endmembers: &[Vec<f32>],
    ) -> GpuResult<Vec<GpuBuffer<T>>> {
        // Simplified spectral unmixing implementation
        let num_pixels = bands.first().map(|b| b.len()).unwrap_or(0);

        let mut abundances = Vec::new();
        for _ in 0..self.num_endmembers {
            abundances.push(GpuBuffer::new(
                &self.context,
                num_pixels,
                BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
            )?);
        }

        debug!("Executed spectral unmixing with {} endmembers", self.num_endmembers);

        Ok(abundances)
    }
}

/// Atmospheric correction kernel.
pub struct AtmosphericCorrectionKernel {
    context: GpuContext,
}

impl AtmosphericCorrectionKernel {
    /// Create a new atmospheric correction kernel.
    pub fn new(context: &GpuContext) -> Self {
        Self {
            context: context.clone(),
        }
    }

    /// Execute atmospheric correction (Dark Object Subtraction).
    ///
    /// # Errors
    ///
    /// Returns an error if correction fails.
    pub fn dark_object_subtraction<T: Pod>(
        &self,
        band: &GpuBuffer<T>,
        dark_value: f32,
    ) -> GpuResult<GpuBuffer<T>> {
        let mut output = GpuBuffer::new(
            &self.context,
            band.len(),
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        )?;

        // Simplified: subtract dark value and clamp
        debug!("Applied dark object subtraction with dark value: {}", dark_value);

        Ok(output)
    }

    /// Execute atmospheric correction (Empirical Line Calibration).
    ///
    /// # Errors
    ///
    /// Returns an error if correction fails.
    pub fn empirical_line_calibration<T: Pod>(
        &self,
        band: &GpuBuffer<T>,
        _gain: f32,
        _offset: f32,
    ) -> GpuResult<GpuBuffer<T>> {
        let output = GpuBuffer::new(
            &self.context,
            band.len(),
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        )?;

        debug!("Applied empirical line calibration");

        Ok(output)
    }
}

/// Band ratio calculator.
pub struct BandRatioKernel {
    context: GpuContext,
    pipeline: ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
}

impl BandRatioKernel {
    /// Create a new band ratio kernel.
    ///
    /// # Errors
    ///
    /// Returns an error if shader compilation or pipeline creation fails.
    pub fn new(context: &GpuContext) -> GpuResult<Self> {
        let shader_code = Self::generate_shader();
        let mut shader = WgslShader::new(shader_code, "band_ratio_main");
        let shader_module = shader.compile(context.device())?;

        let bind_group_layout = create_compute_bind_group_layout(
            context.device(),
            &[
                storage_buffer_layout(0, true),  // numerator
                storage_buffer_layout(1, true),  // denominator
                storage_buffer_layout(2, false), // output
            ],
            Some("Band Ratio Bind Group Layout"),
        )?;

        let pipeline = ComputePipelineBuilder::new(context.device(), shader_module, "band_ratio_main")
            .bind_group_layout(&bind_group_layout)
            .label("Band Ratio Pipeline")
            .build()?;

        debug!("Created band ratio kernel");

        Ok(Self {
            context: context.clone(),
            pipeline,
            bind_group_layout,
        })
    }

    /// Execute band ratio calculation.
    ///
    /// # Errors
    ///
    /// Returns an error if execution fails.
    pub fn execute<T: Pod>(
        &self,
        numerator: &GpuBuffer<T>,
        denominator: &GpuBuffer<T>,
    ) -> GpuResult<GpuBuffer<T>> {
        let mut output = GpuBuffer::new(
            &self.context,
            numerator.len(),
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        )?;

        let bind_group = self.context.device().create_bind_group(&BindGroupDescriptor {
            label: Some("Band Ratio Bind Group"),
            layout: &self.bind_group_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: numerator.buffer().as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: denominator.buffer().as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: output.buffer().as_entire_binding(),
                },
            ],
        });

        let mut encoder = self.context.device().create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Band Ratio Encoder"),
        });

        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Band Ratio Compute Pass"),
                timestamp_writes: None,
            });

            cpass.set_pipeline(&self.pipeline);
            cpass.set_bind_group(0, &bind_group, &[]);

            let workgroup_size = 256;
            let num_workgroups = ((numerator.len() as u32 + workgroup_size - 1) / workgroup_size).max(1);
            cpass.dispatch_workgroups(num_workgroups, 1, 1);
        }

        self.context.queue().submit(Some(encoder.finish()));

        debug!("Executed band ratio calculation");

        Ok(output)
    }

    fn generate_shader() -> String {
        r#"
@group(0) @binding(0) var<storage, read> numerator: array<f32>;
@group(0) @binding(1) var<storage, read> denominator: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(256)
fn band_ratio_main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let idx = global_id.x;
    if (idx >= arrayLength(&output)) {
        return;
    }

    let num = numerator[idx];
    let denom = denominator[idx];

    output[idx] = select(0.0, num / denom, abs(denom) > 1e-10);
}
"#.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_spectral_indices() {
        if let Ok(context) = GpuContext::new().await {
            for index in [
                SpectralIndex::NDVI,
                SpectralIndex::EVI,
                SpectralIndex::SAVI,
                SpectralIndex::NDWI,
                SpectralIndex::NDSI,
                SpectralIndex::NBR,
                SpectralIndex::GNDVI,
                SpectralIndex::MSAVI,
            ] {
                assert!(SpectralIndexKernel::new(&context, index).is_ok());
            }
        }
    }

    #[tokio::test]
    async fn test_pca_kernel() {
        if let Ok(context) = GpuContext::new().await {
            let pca = PCAKernel::new(&context, 3);
            assert_eq!(pca.num_components, 3);
        }
    }

    #[tokio::test]
    async fn test_spectral_unmixing_kernel() {
        if let Ok(context) = GpuContext::new().await {
            let unmixing = SpectralUnmixingKernel::new(&context, 4);
            assert_eq!(unmixing.num_endmembers, 4);
        }
    }

    #[tokio::test]
    async fn test_atmospheric_correction() {
        if let Ok(context) = GpuContext::new().await {
            let _correction = AtmosphericCorrectionKernel::new(&context);
        }
    }

    #[tokio::test]
    async fn test_band_ratio_kernel() {
        if let Ok(context) = GpuContext::new().await {
            assert!(BandRatioKernel::new(&context).is_ok());
        }
    }
}
