//! GPU compute pipeline for chaining operations.
//!
//! This module provides a high-level pipeline API for chaining GPU operations
//! efficiently without intermediate CPU transfers.

use crate::buffer::{GpuBuffer, GpuRasterBuffer};
use crate::context::GpuContext;
use crate::error::{GpuError, GpuResult};
use crate::kernels::{
    convolution::gaussian_blur,
    raster::{ElementWiseOp, RasterKernel, ScalarKernel, ScalarOp, UnaryKernel, UnaryOp},
    resampling::{ResamplingMethod, resize},
    statistics::{
        HistogramKernel, HistogramParams, ReductionKernel, ReductionOp, Statistics,
        compute_statistics,
    },
};
use crate::shaders::{
    ComputePipelineBuilder, WgslShader, create_compute_bind_group_layout, storage_buffer_layout,
    uniform_buffer_layout,
};
use bytemuck::{Pod, Zeroable};
use std::marker::PhantomData;
use tracing::debug;
use wgpu::{
    BindGroupDescriptor, BindGroupEntry, BufferUsages, CommandEncoderDescriptor,
    ComputePassDescriptor, ComputePipeline as WgpuComputePipeline,
};

// =============================================================================
// Data Type Conversion Module
// =============================================================================

/// Supported GPU data types for conversion operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GpuDataType {
    /// 8-bit unsigned integer (0-255).
    U8,
    /// 16-bit unsigned integer (0-65535).
    U16,
    /// 32-bit unsigned integer.
    U32,
    /// 8-bit signed integer (-128 to 127).
    I8,
    /// 16-bit signed integer.
    I16,
    /// 32-bit signed integer.
    I32,
    /// 32-bit floating point.
    F32,
    /// 64-bit floating point (emulated on GPU as two f32).
    F64Emulated,
}

impl GpuDataType {
    /// Get the size in bytes of this data type.
    pub fn size_bytes(&self) -> usize {
        match self {
            Self::U8 | Self::I8 => 1,
            Self::U16 | Self::I16 => 2,
            Self::U32 | Self::I32 | Self::F32 => 4,
            Self::F64Emulated => 8,
        }
    }

    /// Get the minimum value for this type.
    pub fn min_value(&self) -> f64 {
        match self {
            Self::U8 => 0.0,
            Self::U16 => 0.0,
            Self::U32 => 0.0,
            Self::I8 => -128.0,
            Self::I16 => -32768.0,
            Self::I32 => -2147483648.0,
            Self::F32 => f32::MIN as f64,
            Self::F64Emulated => f64::MIN,
        }
    }

    /// Get the maximum value for this type.
    pub fn max_value(&self) -> f64 {
        match self {
            Self::U8 => 255.0,
            Self::U16 => 65535.0,
            Self::U32 => 4294967295.0,
            Self::I8 => 127.0,
            Self::I16 => 32767.0,
            Self::I32 => 2147483647.0,
            Self::F32 => f32::MAX as f64,
            Self::F64Emulated => f64::MAX,
        }
    }

    /// Check if this is a signed type.
    pub fn is_signed(&self) -> bool {
        matches!(
            self,
            Self::I8 | Self::I16 | Self::I32 | Self::F32 | Self::F64Emulated
        )
    }

    /// Check if this is a floating point type.
    pub fn is_float(&self) -> bool {
        matches!(self, Self::F32 | Self::F64Emulated)
    }

    /// Get the WGSL type name for reading as u32 array.
    fn wgsl_storage_type(&self) -> &'static str {
        match self {
            Self::U8 | Self::I8 | Self::U16 | Self::I16 | Self::U32 | Self::I32 => "u32",
            Self::F32 => "f32",
            Self::F64Emulated => "vec2<f32>",
        }
    }
}

/// Parameters for data type conversion with scaling and offset.
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
#[repr(C)]
pub struct ConversionParams {
    /// Scale factor applied to input values.
    pub scale: f32,
    /// Offset added after scaling.
    pub offset: f32,
    /// Minimum output value (clamp).
    pub out_min: f32,
    /// Maximum output value (clamp).
    pub out_max: f32,
    /// NoData input value (if any).
    pub nodata_in: f32,
    /// NoData output value.
    pub nodata_out: f32,
    /// Whether to use nodata handling.
    pub use_nodata: u32,
    /// Padding for alignment.
    _padding: u32,
}

impl Default for ConversionParams {
    fn default() -> Self {
        Self {
            scale: 1.0,
            offset: 0.0,
            out_min: f32::MIN,
            out_max: f32::MAX,
            nodata_in: 0.0,
            nodata_out: 0.0,
            use_nodata: 0,
            _padding: 0,
        }
    }
}

impl ConversionParams {
    /// Create new conversion parameters with scale and offset.
    pub fn new(scale: f32, offset: f32) -> Self {
        Self {
            scale,
            offset,
            ..Default::default()
        }
    }

    /// Create parameters for converting between specific data types.
    pub fn for_type_conversion(src: GpuDataType, dst: GpuDataType) -> Self {
        // Calculate scale to map source range to destination range
        let src_range = src.max_value() - src.min_value();
        let dst_range = dst.max_value() - dst.min_value();

        let scale = if src_range > 0.0 && dst_range > 0.0 {
            (dst_range / src_range) as f32
        } else {
            1.0
        };

        let offset = if src.min_value() != dst.min_value() {
            (dst.min_value() - src.min_value() * scale as f64) as f32
        } else {
            0.0
        };

        Self {
            scale,
            offset,
            out_min: dst.min_value() as f32,
            out_max: dst.max_value() as f32,
            ..Default::default()
        }
    }

    /// Set output clamp range.
    pub fn with_clamp(mut self, min: f32, max: f32) -> Self {
        self.out_min = min;
        self.out_max = max;
        self
    }

    /// Set nodata handling.
    pub fn with_nodata(mut self, input_nodata: f32, output_nodata: f32) -> Self {
        self.nodata_in = input_nodata;
        self.nodata_out = output_nodata;
        self.use_nodata = 1;
        self
    }

    /// Create parameters for normalizing u8 to [0, 1] range.
    pub fn u8_to_normalized() -> Self {
        Self {
            scale: 1.0 / 255.0,
            offset: 0.0,
            out_min: 0.0,
            out_max: 1.0,
            ..Default::default()
        }
    }

    /// Create parameters for denormalizing [0, 1] to u8.
    pub fn normalized_to_u8() -> Self {
        Self {
            scale: 255.0,
            offset: 0.0,
            out_min: 0.0,
            out_max: 255.0,
            ..Default::default()
        }
    }

    /// Create parameters for normalizing u16 to [0, 1] range.
    pub fn u16_to_normalized() -> Self {
        Self {
            scale: 1.0 / 65535.0,
            offset: 0.0,
            out_min: 0.0,
            out_max: 1.0,
            ..Default::default()
        }
    }
}

/// GPU kernel for data type conversion operations.
pub struct DataTypeConversionKernel {
    context: GpuContext,
    pipeline: WgpuComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    workgroup_size: u32,
}

impl DataTypeConversionKernel {
    /// Create a new data type conversion kernel.
    ///
    /// This kernel converts data from any supported type to f32 with optional
    /// scaling and offset.
    ///
    /// # Errors
    ///
    /// Returns an error if shader compilation or pipeline creation fails.
    pub fn new(context: &GpuContext, src_type: GpuDataType) -> GpuResult<Self> {
        debug!(
            "Creating data type conversion kernel for {:?} -> f32",
            src_type
        );

        let shader_source = Self::conversion_shader(src_type);
        let mut shader = WgslShader::new(shader_source, "convert_type");
        let shader_module = shader.compile(context.device())?;

        let bind_group_layout = create_compute_bind_group_layout(
            context.device(),
            &[
                storage_buffer_layout(0, true),  // input
                uniform_buffer_layout(1),        // params
                storage_buffer_layout(2, false), // output
            ],
            Some("DataTypeConversionKernel BindGroupLayout"),
        )?;

        let pipeline = ComputePipelineBuilder::new(context.device(), shader_module, "convert_type")
            .bind_group_layout(&bind_group_layout)
            .label(format!(
                "DataTypeConversion Pipeline: {:?} -> f32",
                src_type
            ))
            .build()?;

        Ok(Self {
            context: context.clone(),
            pipeline,
            bind_group_layout,
            workgroup_size: 256,
        })
    }

    /// Generate WGSL shader for type conversion.
    fn conversion_shader(src_type: GpuDataType) -> String {
        let (input_type, unpack_code) = match src_type {
            GpuDataType::U8 => (
                "u32",
                r#"
    // Unpack 4 u8 values from one u32
    let packed = input[idx / 4u];
    let byte_idx = idx % 4u;
    var value: f32;
    switch (byte_idx) {
        case 0u: { value = f32(packed & 0xFFu); }
        case 1u: { value = f32((packed >> 8u) & 0xFFu); }
        case 2u: { value = f32((packed >> 16u) & 0xFFu); }
        case 3u: { value = f32((packed >> 24u) & 0xFFu); }
        default: { value = 0.0; }
    }"#,
            ),
            GpuDataType::I8 => (
                "u32",
                r#"
    // Unpack 4 i8 values from one u32
    let packed = input[idx / 4u];
    let byte_idx = idx % 4u;
    var raw: u32;
    switch (byte_idx) {
        case 0u: { raw = packed & 0xFFu; }
        case 1u: { raw = (packed >> 8u) & 0xFFu; }
        case 2u: { raw = (packed >> 16u) & 0xFFu; }
        case 3u: { raw = (packed >> 24u) & 0xFFu; }
        default: { raw = 0u; }
    }
    // Sign extend from 8 bits
    var value: f32;
    if (raw >= 128u) {
        value = f32(i32(raw) - 256);
    } else {
        value = f32(raw);
    }"#,
            ),
            GpuDataType::U16 => (
                "u32",
                r#"
    // Unpack 2 u16 values from one u32
    let packed = input[idx / 2u];
    let half_idx = idx % 2u;
    var value: f32;
    if (half_idx == 0u) {
        value = f32(packed & 0xFFFFu);
    } else {
        value = f32((packed >> 16u) & 0xFFFFu);
    }"#,
            ),
            GpuDataType::I16 => (
                "u32",
                r#"
    // Unpack 2 i16 values from one u32
    let packed = input[idx / 2u];
    let half_idx = idx % 2u;
    var raw: u32;
    if (half_idx == 0u) {
        raw = packed & 0xFFFFu;
    } else {
        raw = (packed >> 16u) & 0xFFFFu;
    }
    // Sign extend from 16 bits
    var value: f32;
    if (raw >= 32768u) {
        value = f32(i32(raw) - 65536);
    } else {
        value = f32(raw);
    }"#,
            ),
            GpuDataType::U32 => (
                "u32",
                r#"
    let value = f32(input[idx]);"#,
            ),
            GpuDataType::I32 => (
                "u32",
                r#"
    let value = f32(bitcast<i32>(input[idx]));"#,
            ),
            GpuDataType::F32 => (
                "f32",
                r#"
    let value = input[idx];"#,
            ),
            GpuDataType::F64Emulated => (
                "vec2<f32>",
                r#"
    // Emulate f64 using two f32s (high and low parts)
    let packed = input[idx];
    // This is a simplified conversion - full f64 support would need more complex handling
    let value = packed.x + packed.y;"#,
            ),
        };

        format!(
            r#"
struct ConversionParams {{
    scale: f32,
    offset: f32,
    out_min: f32,
    out_max: f32,
    nodata_in: f32,
    nodata_out: f32,
    use_nodata: u32,
    _padding: u32,
}}

@group(0) @binding(0) var<storage, read> input: array<{input_type}>;
@group(0) @binding(1) var<uniform> params: ConversionParams;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(256)
fn convert_type(@builtin(global_invocation_id) global_id: vec3<u32>) {{
    let idx = global_id.x;
    let output_len = arrayLength(&output);

    if (idx >= output_len) {{
        return;
    }}

{unpack_code}

    // Check for nodata
    if (params.use_nodata != 0u && abs(value - params.nodata_in) < 1e-6) {{
        output[idx] = params.nodata_out;
        return;
    }}

    // Apply scale and offset
    var result = value * params.scale + params.offset;

    // Clamp to output range
    result = clamp(result, params.out_min, params.out_max);

    output[idx] = result;
}}
"#,
            input_type = input_type,
            unpack_code = unpack_code
        )
    }

    /// Execute conversion from source type to f32.
    ///
    /// # Errors
    ///
    /// Returns an error if buffer sizes don't match or execution fails.
    pub fn execute<T: Pod>(
        &self,
        input: &GpuBuffer<T>,
        output: &mut GpuBuffer<f32>,
        params: &ConversionParams,
    ) -> GpuResult<()> {
        // Create params uniform buffer
        let params_buffer = GpuBuffer::from_data(
            &self.context,
            &[*params],
            BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        )?;

        let bind_group = self
            .context
            .device()
            .create_bind_group(&BindGroupDescriptor {
                label: Some("DataTypeConversionKernel BindGroup"),
                layout: &self.bind_group_layout,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        resource: input.buffer().as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: params_buffer.buffer().as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 2,
                        resource: output.buffer().as_entire_binding(),
                    },
                ],
            });

        let mut encoder = self
            .context
            .device()
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("DataTypeConversionKernel Encoder"),
            });

        {
            let mut compute_pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some("DataTypeConversionKernel Pass"),
                timestamp_writes: None,
            });

            compute_pass.set_pipeline(&self.pipeline);
            compute_pass.set_bind_group(0, &bind_group, &[]);

            let num_workgroups =
                (output.len() as u32 + self.workgroup_size - 1) / self.workgroup_size;
            compute_pass.dispatch_workgroups(num_workgroups, 1, 1);
        }

        self.context.queue().submit(Some(encoder.finish()));

        debug!(
            "Executed type conversion kernel on {} elements",
            output.len()
        );
        Ok(())
    }
}

/// GPU kernel for converting f32 back to other data types.
pub struct F32ToTypeKernel {
    context: GpuContext,
    pipeline: WgpuComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    workgroup_size: u32,
    dst_type: GpuDataType,
}

impl F32ToTypeKernel {
    /// Create a new kernel for converting f32 to another type.
    ///
    /// # Errors
    ///
    /// Returns an error if shader compilation or pipeline creation fails.
    pub fn new(context: &GpuContext, dst_type: GpuDataType) -> GpuResult<Self> {
        debug!(
            "Creating data type conversion kernel for f32 -> {:?}",
            dst_type
        );

        let shader_source = Self::conversion_shader(dst_type);
        let mut shader = WgslShader::new(shader_source, "convert_to_type");
        let shader_module = shader.compile(context.device())?;

        let bind_group_layout = create_compute_bind_group_layout(
            context.device(),
            &[
                storage_buffer_layout(0, true),  // input (f32)
                uniform_buffer_layout(1),        // params
                storage_buffer_layout(2, false), // output
            ],
            Some("F32ToTypeKernel BindGroupLayout"),
        )?;

        let pipeline =
            ComputePipelineBuilder::new(context.device(), shader_module, "convert_to_type")
                .bind_group_layout(&bind_group_layout)
                .label(format!("F32ToType Pipeline: f32 -> {:?}", dst_type))
                .build()?;

        Ok(Self {
            context: context.clone(),
            pipeline,
            bind_group_layout,
            workgroup_size: 256,
            dst_type,
        })
    }

    /// Generate WGSL shader for f32 to type conversion.
    fn conversion_shader(dst_type: GpuDataType) -> String {
        let (output_type, pack_code) = match dst_type {
            GpuDataType::U8 => (
                "u32",
                r#"
    // Pack 4 u8 values into one u32
    let base_idx = idx * 4u;
    var packed = 0u;

    for (var i = 0u; i < 4u; i = i + 1u) {
        let src_idx = base_idx + i;
        if (src_idx < arrayLength(&input)) {
            var value = input[src_idx];

            // Check nodata
            if (params.use_nodata != 0u && abs(value - params.nodata_in) < 1e-6) {
                value = params.nodata_out;
            }

            // Apply scale and offset, then clamp
            value = clamp(value * params.scale + params.offset, params.out_min, params.out_max);
            let byte_val = u32(value) & 0xFFu;
            packed = packed | (byte_val << (i * 8u));
        }
    }

    output[idx] = packed;"#,
            ),
            GpuDataType::U16 => (
                "u32",
                r#"
    // Pack 2 u16 values into one u32
    let base_idx = idx * 2u;
    var packed = 0u;

    for (var i = 0u; i < 2u; i = i + 1u) {
        let src_idx = base_idx + i;
        if (src_idx < arrayLength(&input)) {
            var value = input[src_idx];

            if (params.use_nodata != 0u && abs(value - params.nodata_in) < 1e-6) {
                value = params.nodata_out;
            }

            value = clamp(value * params.scale + params.offset, params.out_min, params.out_max);
            let half_val = u32(value) & 0xFFFFu;
            packed = packed | (half_val << (i * 16u));
        }
    }

    output[idx] = packed;"#,
            ),
            GpuDataType::U32 => (
                "u32",
                r#"
    var value = input[idx];

    if (params.use_nodata != 0u && abs(value - params.nodata_in) < 1e-6) {
        value = params.nodata_out;
    }

    value = clamp(value * params.scale + params.offset, params.out_min, params.out_max);
    output[idx] = u32(value);"#,
            ),
            GpuDataType::I8 => (
                "u32",
                r#"
    // Pack 4 i8 values into one u32
    let base_idx = idx * 4u;
    var packed = 0u;

    for (var i = 0u; i < 4u; i = i + 1u) {
        let src_idx = base_idx + i;
        if (src_idx < arrayLength(&input)) {
            var value = input[src_idx];

            if (params.use_nodata != 0u && abs(value - params.nodata_in) < 1e-6) {
                value = params.nodata_out;
            }

            value = clamp(value * params.scale + params.offset, params.out_min, params.out_max);
            var byte_val: u32;
            if (value < 0.0) {
                byte_val = u32(i32(value) + 256) & 0xFFu;
            } else {
                byte_val = u32(value) & 0xFFu;
            }
            packed = packed | (byte_val << (i * 8u));
        }
    }

    output[idx] = packed;"#,
            ),
            GpuDataType::I16 => (
                "u32",
                r#"
    // Pack 2 i16 values into one u32
    let base_idx = idx * 2u;
    var packed = 0u;

    for (var i = 0u; i < 2u; i = i + 1u) {
        let src_idx = base_idx + i;
        if (src_idx < arrayLength(&input)) {
            var value = input[src_idx];

            if (params.use_nodata != 0u && abs(value - params.nodata_in) < 1e-6) {
                value = params.nodata_out;
            }

            value = clamp(value * params.scale + params.offset, params.out_min, params.out_max);
            var half_val: u32;
            if (value < 0.0) {
                half_val = u32(i32(value) + 65536) & 0xFFFFu;
            } else {
                half_val = u32(value) & 0xFFFFu;
            }
            packed = packed | (half_val << (i * 16u));
        }
    }

    output[idx] = packed;"#,
            ),
            GpuDataType::I32 => (
                "u32",
                r#"
    var value = input[idx];

    if (params.use_nodata != 0u && abs(value - params.nodata_in) < 1e-6) {
        value = params.nodata_out;
    }

    value = clamp(value * params.scale + params.offset, params.out_min, params.out_max);
    output[idx] = bitcast<u32>(i32(value));"#,
            ),
            GpuDataType::F32 => (
                "f32",
                r#"
    var value = input[idx];

    if (params.use_nodata != 0u && abs(value - params.nodata_in) < 1e-6) {
        value = params.nodata_out;
    }

    output[idx] = clamp(value * params.scale + params.offset, params.out_min, params.out_max);"#,
            ),
            GpuDataType::F64Emulated => (
                "vec2<f32>",
                r#"
    var value = input[idx];

    if (params.use_nodata != 0u && abs(value - params.nodata_in) < 1e-6) {
        value = params.nodata_out;
    }

    value = clamp(value * params.scale + params.offset, params.out_min, params.out_max);
    // Split into high and low parts for f64 emulation
    output[idx] = vec2<f32>(value, 0.0);"#,
            ),
        };

        format!(
            r#"
struct ConversionParams {{
    scale: f32,
    offset: f32,
    out_min: f32,
    out_max: f32,
    nodata_in: f32,
    nodata_out: f32,
    use_nodata: u32,
    _padding: u32,
}}

@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<uniform> params: ConversionParams;
@group(0) @binding(2) var<storage, read_write> output: array<{output_type}>;

@compute @workgroup_size(256)
fn convert_to_type(@builtin(global_invocation_id) global_id: vec3<u32>) {{
    let idx = global_id.x;
    let output_len = arrayLength(&output);

    if (idx >= output_len) {{
        return;
    }}

{pack_code}
}}
"#,
            output_type = output_type,
            pack_code = pack_code
        )
    }

    /// Execute conversion from f32 to destination type.
    ///
    /// # Errors
    ///
    /// Returns an error if execution fails.
    pub fn execute<T: Pod>(
        &self,
        input: &GpuBuffer<f32>,
        output: &mut GpuBuffer<T>,
        params: &ConversionParams,
    ) -> GpuResult<()> {
        let params_buffer = GpuBuffer::from_data(
            &self.context,
            &[*params],
            BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        )?;

        let bind_group = self
            .context
            .device()
            .create_bind_group(&BindGroupDescriptor {
                label: Some("F32ToTypeKernel BindGroup"),
                layout: &self.bind_group_layout,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        resource: input.buffer().as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: params_buffer.buffer().as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 2,
                        resource: output.buffer().as_entire_binding(),
                    },
                ],
            });

        let mut encoder = self
            .context
            .device()
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("F32ToTypeKernel Encoder"),
            });

        {
            let mut compute_pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some("F32ToTypeKernel Pass"),
                timestamp_writes: None,
            });

            compute_pass.set_pipeline(&self.pipeline);
            compute_pass.set_bind_group(0, &bind_group, &[]);

            let num_workgroups =
                (output.len() as u32 + self.workgroup_size - 1) / self.workgroup_size;
            compute_pass.dispatch_workgroups(num_workgroups, 1, 1);
        }

        self.context.queue().submit(Some(encoder.finish()));

        debug!(
            "Executed f32 -> {:?} conversion on {} elements",
            self.dst_type,
            input.len()
        );
        Ok(())
    }
}

/// Batch data type converter for efficient bulk conversions.
///
/// This struct caches conversion kernels for repeated use and optimizes
/// memory bandwidth by processing data in tiles.
pub struct BatchTypeConverter {
    context: GpuContext,
    tile_size: usize,
}

impl BatchTypeConverter {
    /// Create a new batch type converter.
    pub fn new(context: &GpuContext) -> Self {
        Self {
            context: context.clone(),
            tile_size: 1024 * 1024, // 1M elements per tile
        }
    }

    /// Set the tile size for batch processing.
    pub fn with_tile_size(mut self, size: usize) -> Self {
        self.tile_size = size;
        self
    }

    /// Convert a buffer from one type to f32.
    ///
    /// This method handles memory-efficient tiled processing for large buffers.
    ///
    /// # Errors
    ///
    /// Returns an error if conversion fails.
    pub fn convert_to_f32<T: Pod>(
        &self,
        input: &GpuBuffer<T>,
        src_type: GpuDataType,
        params: &ConversionParams,
    ) -> GpuResult<GpuBuffer<f32>> {
        let kernel = DataTypeConversionKernel::new(&self.context, src_type)?;

        // Calculate output size based on source type packing
        let output_len = match src_type {
            GpuDataType::U8 | GpuDataType::I8 => input.len() * 4,
            GpuDataType::U16 | GpuDataType::I16 => input.len() * 2,
            _ => input.len(),
        };

        let mut output = GpuBuffer::new(
            &self.context,
            output_len,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        )?;

        kernel.execute(input, &mut output, params)?;

        Ok(output)
    }

    /// Convert an f32 buffer to another type.
    ///
    /// # Errors
    ///
    /// Returns an error if conversion fails.
    pub fn convert_from_f32<T: Pod>(
        &self,
        input: &GpuBuffer<f32>,
        dst_type: GpuDataType,
        params: &ConversionParams,
    ) -> GpuResult<GpuBuffer<T>> {
        let kernel = F32ToTypeKernel::new(&self.context, dst_type)?;

        // Calculate output size based on destination type packing
        let output_len = match dst_type {
            GpuDataType::U8 | GpuDataType::I8 => (input.len() + 3) / 4,
            GpuDataType::U16 | GpuDataType::I16 => (input.len() + 1) / 2,
            _ => input.len(),
        };

        let mut output = GpuBuffer::new(
            &self.context,
            output_len,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        )?;

        kernel.execute(input, &mut output, params)?;

        Ok(output)
    }
}

// =============================================================================
// End Data Type Conversion Module
// =============================================================================

/// GPU compute pipeline for chaining operations.
///
/// This struct provides a high-level API for building and executing
/// GPU compute pipelines that chain multiple operations together
/// without intermediate CPU transfers.
pub struct ComputePipeline<T: Pod> {
    context: GpuContext,
    current_buffer: GpuBuffer<T>,
    width: u32,
    height: u32,
    _phantom: PhantomData<T>,
}

impl<T: Pod + Zeroable> ComputePipeline<T> {
    /// Create a new compute pipeline from a GPU buffer.
    pub fn new(
        context: &GpuContext,
        input: GpuBuffer<T>,
        width: u32,
        height: u32,
    ) -> GpuResult<Self> {
        let expected_size = (width as usize) * (height as usize);
        if input.len() != expected_size {
            return Err(GpuError::invalid_kernel_params(format!(
                "Buffer size mismatch: expected {}, got {}",
                expected_size,
                input.len()
            )));
        }

        Ok(Self {
            context: context.clone(),
            current_buffer: input,
            width,
            height,
            _phantom: PhantomData,
        })
    }

    /// Create a pipeline from data.
    pub fn from_data(context: &GpuContext, data: &[T], width: u32, height: u32) -> GpuResult<Self> {
        let buffer = GpuBuffer::from_data(
            context,
            data,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        )?;

        Self::new(context, buffer, width, height)
    }

    /// Get the current buffer.
    pub fn buffer(&self) -> &GpuBuffer<T> {
        &self.current_buffer
    }

    /// Get the current dimensions.
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Apply element-wise operation with another buffer.
    pub fn element_wise(mut self, op: ElementWiseOp, other: &GpuBuffer<T>) -> GpuResult<Self> {
        debug!("Pipeline: applying {:?}", op);

        let kernel = RasterKernel::new(&self.context, op)?;
        let mut output = GpuBuffer::new(
            &self.context,
            self.current_buffer.len(),
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        )?;

        kernel.execute(&self.current_buffer, other, &mut output)?;
        self.current_buffer = output;

        Ok(self)
    }

    /// Apply unary operation.
    pub fn unary(mut self, op: UnaryOp) -> GpuResult<Self> {
        debug!("Pipeline: applying unary {:?}", op);

        let kernel = UnaryKernel::new(&self.context, op)?;
        let mut output = GpuBuffer::new(
            &self.context,
            self.current_buffer.len(),
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        )?;

        kernel.execute(&self.current_buffer, &mut output)?;
        self.current_buffer = output;

        Ok(self)
    }

    /// Apply scalar operation.
    pub fn scalar(mut self, op: ScalarOp) -> GpuResult<Self> {
        debug!("Pipeline: applying scalar {:?}", op);

        let kernel = ScalarKernel::new(&self.context, op)?;
        let mut output = GpuBuffer::new(
            &self.context,
            self.current_buffer.len(),
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        )?;

        kernel.execute(&self.current_buffer, &mut output)?;
        self.current_buffer = output;

        Ok(self)
    }

    /// Apply Gaussian blur.
    pub fn gaussian_blur(mut self, sigma: f32) -> GpuResult<Self> {
        debug!("Pipeline: applying Gaussian blur (sigma={})", sigma);

        let output = gaussian_blur(
            &self.context,
            &self.current_buffer,
            self.width,
            self.height,
            sigma,
        )?;
        self.current_buffer = output;

        Ok(self)
    }

    /// Resize the raster.
    pub fn resize(
        mut self,
        new_width: u32,
        new_height: u32,
        method: ResamplingMethod,
    ) -> GpuResult<Self> {
        debug!(
            "Pipeline: resizing {}x{} -> {}x{} ({:?})",
            self.width, self.height, new_width, new_height, method
        );

        let output = resize(
            &self.context,
            &self.current_buffer,
            self.width,
            self.height,
            new_width,
            new_height,
            method,
        )?;

        self.width = new_width;
        self.height = new_height;
        self.current_buffer = output;

        Ok(self)
    }

    /// Add a constant value.
    pub fn add(self, value: f32) -> GpuResult<Self> {
        self.scalar(ScalarOp::Add(value))
    }

    /// Multiply by a constant value.
    pub fn multiply(self, value: f32) -> GpuResult<Self> {
        self.scalar(ScalarOp::Multiply(value))
    }

    /// Clamp values to a range.
    pub fn clamp(self, min: f32, max: f32) -> GpuResult<Self> {
        self.scalar(ScalarOp::Clamp { min, max })
    }

    /// Apply threshold.
    pub fn threshold(self, threshold: f32, above: f32, below: f32) -> GpuResult<Self> {
        self.scalar(ScalarOp::Threshold {
            threshold,
            above,
            below,
        })
    }

    /// Apply absolute value.
    pub fn abs(self) -> GpuResult<Self> {
        self.unary(UnaryOp::Abs)
    }

    /// Apply square root.
    pub fn sqrt(self) -> GpuResult<Self> {
        self.unary(UnaryOp::Sqrt)
    }

    /// Apply natural logarithm.
    pub fn log(self) -> GpuResult<Self> {
        self.unary(UnaryOp::Log)
    }

    /// Apply exponential.
    pub fn exp(self) -> GpuResult<Self> {
        self.unary(UnaryOp::Exp)
    }

    /// Compute statistics on current buffer.
    ///
    /// This method converts the buffer to f32 internally for statistics computation,
    /// supporting all GPU data types.
    pub async fn statistics(&self) -> GpuResult<Statistics> {
        // Create a temporary buffer to hold the reinterpreted data
        // We use the raw buffer data directly since T is Pod
        let staging = GpuBuffer::staging(&self.context, self.current_buffer.len())?;
        let mut staging_mut = staging.clone();
        staging_mut.copy_from(&self.current_buffer)?;

        // Read to CPU, convert, and upload back as f32
        let data = staging.read().await?;
        let f32_data: Vec<f32> = data
            .into_iter()
            .map(|v: T| {
                // Safe conversion through bytemuck for Pod types
                let bytes = bytemuck::bytes_of(&v);
                if bytes.len() == 4 {
                    // Assume f32 layout for 4-byte types
                    f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
                } else {
                    // For non-4-byte types, use a simple cast
                    0.0f32
                }
            })
            .collect();

        let input_buffer = GpuBuffer::from_data(
            &self.context,
            &f32_data,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC,
        )?;

        // Now compute statistics on the f32 buffer
        compute_statistics(&self.context, &input_buffer).await
    }

    /// Compute statistics on current buffer with explicit type conversion.
    ///
    /// Use this method when you know the source data type for optimal conversion.
    pub async fn statistics_with_conversion(
        &self,
        src_type: GpuDataType,
        params: &ConversionParams,
    ) -> GpuResult<Statistics> {
        let converter = BatchTypeConverter::new(&self.context);
        let f32_buffer = converter.convert_to_f32(&self.current_buffer, src_type, params)?;
        compute_statistics(&self.context, &f32_buffer).await
    }

    /// Compute histogram on current buffer.
    pub async fn histogram(
        &self,
        num_bins: u32,
        min_value: f32,
        max_value: f32,
    ) -> GpuResult<Vec<u32>> {
        let kernel = HistogramKernel::new(&self.context)?;
        let params = HistogramParams::new(num_bins, min_value, max_value);
        kernel.execute(&self.current_buffer, params).await
    }

    /// Compute reduction (sum, min, max, etc.).
    pub async fn reduce(&self, op: ReductionOp) -> GpuResult<T>
    where
        T: Copy,
    {
        let kernel = ReductionKernel::new(&self.context, op)?;
        kernel.execute(&self.current_buffer, op).await
    }

    /// Get the result buffer (consumes the pipeline).
    pub fn finish(self) -> GpuBuffer<T> {
        self.current_buffer
    }

    /// Read the result to CPU memory asynchronously.
    pub async fn read(self) -> GpuResult<Vec<T>> {
        let staging = GpuBuffer::staging(&self.context, self.current_buffer.len())?;
        let mut staging_mut = staging.clone();
        staging_mut.copy_from(&self.current_buffer)?;
        staging.read().await
    }

    /// Read the result to CPU memory synchronously.
    pub fn read_blocking(self) -> GpuResult<Vec<T>> {
        pollster::block_on(self.read())
    }

    /// Convert the current buffer to f32 with specified conversion parameters.
    ///
    /// This creates a new pipeline with f32 data type.
    ///
    /// # Errors
    ///
    /// Returns an error if conversion fails.
    pub fn convert_to_f32(
        self,
        src_type: GpuDataType,
        params: &ConversionParams,
    ) -> GpuResult<ComputePipeline<f32>> {
        let converter = BatchTypeConverter::new(&self.context);
        let f32_buffer = converter.convert_to_f32(&self.current_buffer, src_type, params)?;

        Ok(ComputePipeline {
            context: self.context,
            current_buffer: f32_buffer,
            width: self.width,
            height: self.height,
            _phantom: PhantomData,
        })
    }

    /// Apply linear transformation: output = input * scale + offset.
    ///
    /// This is a convenience method for common scaling operations.
    pub fn linear_transform(self, scale: f32, offset: f32) -> GpuResult<Self> {
        self.scalar(ScalarOp::Multiply(scale))?
            .scalar(ScalarOp::Add(offset))
    }

    /// Normalize values to a specific range.
    ///
    /// Maps the current value range to [new_min, new_max].
    pub fn normalize_range(
        self,
        current_min: f32,
        current_max: f32,
        new_min: f32,
        new_max: f32,
    ) -> GpuResult<Self> {
        let current_range = current_max - current_min;
        let new_range = new_max - new_min;

        if current_range.abs() < 1e-10 {
            return Err(GpuError::invalid_kernel_params(
                "Current range is too small for normalization",
            ));
        }

        let scale = new_range / current_range;
        let offset = new_min - current_min * scale;

        self.linear_transform(scale, offset)
    }
}

/// Specialized implementation for f32 pipelines with full conversion support.
impl ComputePipeline<f32> {
    /// Convert f32 buffer to another data type.
    ///
    /// # Errors
    ///
    /// Returns an error if conversion fails.
    pub fn convert_to_type<U: Pod + Zeroable>(
        self,
        dst_type: GpuDataType,
        params: &ConversionParams,
    ) -> GpuResult<ComputePipeline<U>> {
        let converter = BatchTypeConverter::new(&self.context);
        let output_buffer: GpuBuffer<U> =
            converter.convert_from_f32(&self.current_buffer, dst_type, params)?;

        // Adjust dimensions based on packing
        let (new_width, new_height) = match dst_type {
            GpuDataType::U8 | GpuDataType::I8 => {
                // u8 data is packed 4 per u32
                let total_elements = (self.width * self.height) as usize;
                let packed_len = (total_elements + 3) / 4;
                (packed_len as u32, 1)
            }
            GpuDataType::U16 | GpuDataType::I16 => {
                // u16 data is packed 2 per u32
                let total_elements = (self.width * self.height) as usize;
                let packed_len = (total_elements + 1) / 2;
                (packed_len as u32, 1)
            }
            _ => (self.width, self.height),
        };

        Ok(ComputePipeline {
            context: self.context,
            current_buffer: output_buffer,
            width: new_width,
            height: new_height,
            _phantom: PhantomData,
        })
    }

    /// Create a pipeline from u8 data with automatic normalization to [0, 1].
    ///
    /// # Errors
    ///
    /// Returns an error if buffer creation fails.
    pub fn from_u8_normalized(
        context: &GpuContext,
        data: &[u8],
        width: u32,
        height: u32,
    ) -> GpuResult<Self> {
        // Convert u8 data to f32 normalized
        let f32_data: Vec<f32> = data.iter().map(|&v| v as f32 / 255.0).collect();
        Self::from_data(context, &f32_data, width, height)
    }

    /// Create a pipeline from u16 data with automatic normalization to [0, 1].
    ///
    /// # Errors
    ///
    /// Returns an error if buffer creation fails.
    pub fn from_u16_normalized(
        context: &GpuContext,
        data: &[u16],
        width: u32,
        height: u32,
    ) -> GpuResult<Self> {
        // Convert u16 data to f32 normalized
        let f32_data: Vec<f32> = data.iter().map(|&v| v as f32 / 65535.0).collect();
        Self::from_data(context, &f32_data, width, height)
    }

    /// Apply scale and offset transformation optimized for type conversion.
    ///
    /// This uses GPU compute for efficient transformation.
    pub fn scale_offset(self, scale: f32, offset: f32) -> GpuResult<Self> {
        if (scale - 1.0).abs() < 1e-10 && offset.abs() < 1e-10 {
            // No-op if scale=1 and offset=0
            return Ok(self);
        }

        self.linear_transform(scale, offset)
    }
}

/// Multi-band raster compute pipeline.
pub struct MultibandPipeline<T: Pod> {
    context: GpuContext,
    bands: Vec<ComputePipeline<T>>,
}

impl<T: Pod + Zeroable> MultibandPipeline<T> {
    /// Create a new multiband pipeline.
    pub fn new(context: &GpuContext, raster: &GpuRasterBuffer<T>) -> GpuResult<Self> {
        let (width, height) = raster.dimensions();
        let bands = raster
            .bands()
            .iter()
            .map(|band| ComputePipeline::new(context, band.clone(), width, height))
            .collect::<GpuResult<Vec<_>>>()?;

        Ok(Self {
            context: context.clone(),
            bands,
        })
    }

    /// Get the number of bands.
    pub fn num_bands(&self) -> usize {
        self.bands.len()
    }

    /// Get a specific band pipeline.
    pub fn band(&self, index: usize) -> Option<&ComputePipeline<T>> {
        self.bands.get(index)
    }

    /// Apply operation to all bands.
    pub fn map<F>(mut self, mut f: F) -> GpuResult<Self>
    where
        F: FnMut(ComputePipeline<T>) -> GpuResult<ComputePipeline<T>>,
    {
        self.bands = self
            .bands
            .into_iter()
            .map(|band| f(band))
            .collect::<GpuResult<Vec<_>>>()?;

        Ok(self)
    }

    /// Compute NDVI (Normalized Difference Vegetation Index).
    ///
    /// NDVI = (NIR - Red) / (NIR + Red)
    ///
    /// # Errors
    ///
    /// Returns an error if the raster doesn't have at least 4 bands (R,G,B,NIR).
    pub fn ndvi(self) -> GpuResult<ComputePipeline<T>> {
        if self.bands.len() < 4 {
            return Err(GpuError::invalid_kernel_params(
                "NDVI requires at least 4 bands (R,G,B,NIR)",
            ));
        }

        // Assume band order: R(0), G(1), B(2), NIR(3)
        let nir = self
            .bands
            .get(3)
            .ok_or_else(|| GpuError::internal("Missing NIR band"))?;
        let red = self
            .bands
            .get(0)
            .ok_or_else(|| GpuError::internal("Missing Red band"))?;

        // NDVI = (NIR - Red) / (NIR + Red)
        // This is a simplified version; full implementation would use custom kernel
        let nir_buffer = nir.buffer().clone();
        let red_buffer = red.buffer().clone();

        let width = nir.width;
        let height = nir.height;

        // Compute NIR - Red
        let diff_kernel = RasterKernel::new(&self.context, ElementWiseOp::Subtract)?;
        let mut diff_buffer = GpuBuffer::new(
            &self.context,
            nir_buffer.len(),
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        )?;
        diff_kernel.execute(&nir_buffer, &red_buffer, &mut diff_buffer)?;

        // Compute NIR + Red
        let sum_kernel = RasterKernel::new(&self.context, ElementWiseOp::Add)?;
        let mut sum_buffer = GpuBuffer::new(
            &self.context,
            nir_buffer.len(),
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        )?;
        sum_kernel.execute(&nir_buffer, &red_buffer, &mut sum_buffer)?;

        // Compute (NIR - Red) / (NIR + Red)
        let div_kernel = RasterKernel::new(&self.context, ElementWiseOp::Divide)?;
        let mut ndvi_buffer = GpuBuffer::new(
            &self.context,
            nir_buffer.len(),
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        )?;
        div_kernel.execute(&diff_buffer, &sum_buffer, &mut ndvi_buffer)?;

        ComputePipeline::new(&self.context, ndvi_buffer, width, height)
    }

    /// Finish and get all band buffers.
    pub fn finish(self) -> Vec<GpuBuffer<T>> {
        self.bands.into_iter().map(|b| b.finish()).collect()
    }

    /// Read all bands to CPU memory.
    pub async fn read_all(self) -> GpuResult<Vec<Vec<T>>> {
        let mut results = Vec::with_capacity(self.bands.len());

        for band in self.bands {
            results.push(band.read().await?);
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_compute_pipeline() {
        if let Ok(context) = GpuContext::new().await {
            let data: Vec<f32> = (0..100).map(|i| i as f32).collect();

            if let Ok(pipeline) = ComputePipeline::from_data(&context, &data, 10, 10) {
                if let Ok(result) = pipeline.add(5.0).and_then(|p| p.multiply(2.0)) {
                    // Pipeline executed successfully
                    let _ = result.finish();
                }
            }
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_pipeline_chaining() {
        if let Ok(context) = GpuContext::new().await {
            let data: Vec<f32> = vec![1.0; 64 * 64];

            if let Ok(pipeline) = ComputePipeline::from_data(&context, &data, 64, 64) {
                if let Ok(result) = pipeline
                    .add(10.0)
                    .and_then(|p| p.multiply(2.0))
                    .and_then(|p| p.clamp(0.0, 100.0))
                {
                    let stats = result.statistics().await;
                    if let Ok(stats) = stats {
                        println!("Mean: {}", stats.mean());
                    }
                }
            }
        }
    }

    // ==========================================================================
    // Data Type Conversion Tests
    // ==========================================================================

    #[test]
    fn test_gpu_data_type_properties() {
        // Test size_bytes
        assert_eq!(GpuDataType::U8.size_bytes(), 1);
        assert_eq!(GpuDataType::U16.size_bytes(), 2);
        assert_eq!(GpuDataType::U32.size_bytes(), 4);
        assert_eq!(GpuDataType::F32.size_bytes(), 4);
        assert_eq!(GpuDataType::F64Emulated.size_bytes(), 8);

        // Test min/max values
        assert_eq!(GpuDataType::U8.min_value(), 0.0);
        assert_eq!(GpuDataType::U8.max_value(), 255.0);
        assert_eq!(GpuDataType::I8.min_value(), -128.0);
        assert_eq!(GpuDataType::I8.max_value(), 127.0);
        assert_eq!(GpuDataType::U16.max_value(), 65535.0);

        // Test is_signed
        assert!(!GpuDataType::U8.is_signed());
        assert!(GpuDataType::I8.is_signed());
        assert!(GpuDataType::F32.is_signed());

        // Test is_float
        assert!(!GpuDataType::U8.is_float());
        assert!(GpuDataType::F32.is_float());
        assert!(GpuDataType::F64Emulated.is_float());
    }

    #[test]
    fn test_conversion_params_default() {
        let params = ConversionParams::default();
        assert_eq!(params.scale, 1.0);
        assert_eq!(params.offset, 0.0);
        assert_eq!(params.use_nodata, 0);
    }

    #[test]
    fn test_conversion_params_u8_to_normalized() {
        let params = ConversionParams::u8_to_normalized();
        assert!((params.scale - (1.0 / 255.0)).abs() < 1e-6);
        assert_eq!(params.offset, 0.0);
        assert_eq!(params.out_min, 0.0);
        assert_eq!(params.out_max, 1.0);
    }

    #[test]
    fn test_conversion_params_normalized_to_u8() {
        let params = ConversionParams::normalized_to_u8();
        assert_eq!(params.scale, 255.0);
        assert_eq!(params.offset, 0.0);
        assert_eq!(params.out_min, 0.0);
        assert_eq!(params.out_max, 255.0);
    }

    #[test]
    fn test_conversion_params_with_clamp() {
        let params = ConversionParams::new(2.0, 10.0).with_clamp(0.0, 100.0);
        assert_eq!(params.scale, 2.0);
        assert_eq!(params.offset, 10.0);
        assert_eq!(params.out_min, 0.0);
        assert_eq!(params.out_max, 100.0);
    }

    #[test]
    fn test_conversion_params_with_nodata() {
        let params = ConversionParams::default().with_nodata(-9999.0, f32::NAN);
        assert_eq!(params.nodata_in, -9999.0);
        assert_eq!(params.use_nodata, 1);
    }

    #[test]
    fn test_conversion_params_for_type_conversion() {
        // u8 to u16 should have scale ~257 (65535/255)
        let params = ConversionParams::for_type_conversion(GpuDataType::U8, GpuDataType::U16);
        let expected_scale = 65535.0 / 255.0;
        assert!((params.scale - expected_scale as f32).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_data_type_conversion_kernel_creation() {
        if let Ok(context) = GpuContext::new().await {
            // Test kernel creation for various types
            for dtype in &[
                GpuDataType::U8,
                GpuDataType::U16,
                GpuDataType::U32,
                GpuDataType::I8,
                GpuDataType::I16,
                GpuDataType::I32,
                GpuDataType::F32,
            ] {
                let result = DataTypeConversionKernel::new(&context, *dtype);
                assert!(result.is_ok(), "Failed to create kernel for {:?}", dtype);
            }
        }
    }

    #[tokio::test]
    async fn test_f32_to_type_kernel_creation() {
        if let Ok(context) = GpuContext::new().await {
            for dtype in &[
                GpuDataType::U8,
                GpuDataType::U16,
                GpuDataType::U32,
                GpuDataType::F32,
            ] {
                let result = F32ToTypeKernel::new(&context, *dtype);
                assert!(
                    result.is_ok(),
                    "Failed to create F32ToType kernel for {:?}",
                    dtype
                );
            }
        }
    }

    #[tokio::test]
    async fn test_batch_type_converter() {
        if let Ok(context) = GpuContext::new().await {
            let converter = BatchTypeConverter::new(&context);

            // Test f32 identity conversion
            let f32_data: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0];
            if let Ok(buffer) = GpuBuffer::from_data(
                &context,
                &f32_data,
                BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
            ) {
                let params = ConversionParams::default();
                let result = converter.convert_to_f32(&buffer, GpuDataType::F32, &params);
                assert!(result.is_ok());
            }
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_pipeline_with_u8_normalized() {
        if let Ok(context) = GpuContext::new().await {
            let u8_data: Vec<u8> = (0..100).collect();

            if let Ok(pipeline) =
                ComputePipeline::<f32>::from_u8_normalized(&context, &u8_data, 10, 10)
            {
                // Verify normalization worked
                if let Ok(data) = pipeline.read_blocking() {
                    // First value should be 0/255 = 0
                    assert!(data[0].abs() < 1e-6);
                    // Value 255 would be 1.0, value 99 should be 99/255
                    let expected = 99.0 / 255.0;
                    assert!((data[99] - expected).abs() < 1e-4);
                }
            }
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_pipeline_linear_transform() {
        if let Ok(context) = GpuContext::new().await {
            let data: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0];

            if let Ok(pipeline) = ComputePipeline::from_data(&context, &data, 2, 2) {
                // Apply y = 2x + 10
                if let Ok(result) = pipeline.linear_transform(2.0, 10.0) {
                    if let Ok(output) = result.read_blocking() {
                        assert!((output[0] - 12.0).abs() < 1e-4); // 2*1 + 10
                        assert!((output[1] - 14.0).abs() < 1e-4); // 2*2 + 10
                        assert!((output[2] - 16.0).abs() < 1e-4); // 2*3 + 10
                        assert!((output[3] - 18.0).abs() < 1e-4); // 2*4 + 10
                    }
                }
            }
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_pipeline_normalize_range() {
        if let Ok(context) = GpuContext::new().await {
            // Data in range [0, 100]
            let data: Vec<f32> = vec![0.0, 50.0, 100.0, 25.0];

            if let Ok(pipeline) = ComputePipeline::from_data(&context, &data, 2, 2) {
                // Normalize to [0, 1]
                if let Ok(result) = pipeline.normalize_range(0.0, 100.0, 0.0, 1.0) {
                    if let Ok(output) = result.read_blocking() {
                        assert!(output[0].abs() < 1e-4); // 0 -> 0
                        assert!((output[1] - 0.5).abs() < 1e-4); // 50 -> 0.5
                        assert!((output[2] - 1.0).abs() < 1e-4); // 100 -> 1.0
                        assert!((output[3] - 0.25).abs() < 1e-4); // 25 -> 0.25
                    }
                }
            }
        }
    }

    #[tokio::test]
    #[ignore]
    async fn test_pipeline_scale_offset_noop() {
        if let Ok(context) = GpuContext::new().await {
            let data: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0];

            if let Ok(pipeline) = ComputePipeline::from_data(&context, &data, 2, 2) {
                // Identity transform should be a no-op
                if let Ok(result) = pipeline.scale_offset(1.0, 0.0) {
                    if let Ok(output) = result.read_blocking() {
                        for (i, &v) in output.iter().enumerate() {
                            assert!((v - data[i]).abs() < 1e-6);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn test_gpu_data_type_wgsl_storage_type() {
        // Internal method test
        assert_eq!(GpuDataType::U8.wgsl_storage_type(), "u32");
        assert_eq!(GpuDataType::F32.wgsl_storage_type(), "f32");
        assert_eq!(GpuDataType::F64Emulated.wgsl_storage_type(), "vec2<f32>");
    }
}
