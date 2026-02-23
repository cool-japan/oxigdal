//! Buffer integration for Node.js bindings
//!
//! This module provides zero-copy integration between OxiGDAL RasterBuffer
//! and Node.js Buffer/TypedArray types.

use napi::bindgen_prelude::*;
use napi_derive::napi;
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::NoDataValue;
use oxigdal_core::types::RasterDataType;

use crate::error::{NodeError, ToNapiResult};

/// Wrapper around RasterBuffer for Node.js
#[napi]
pub struct BufferWrapper {
    buffer: RasterBuffer,
}

#[napi]
impl BufferWrapper {
    /// Creates a new zero-filled buffer
    #[napi(constructor)]
    pub fn new(width: u32, height: u32, data_type: String) -> Result<Self> {
        let dtype = parse_data_type(&data_type)?;
        let buffer = RasterBuffer::zeros(width as u64, height as u64, dtype);
        Ok(Self { buffer })
    }

    /// Gets the width of the buffer
    #[napi(getter)]
    pub fn width(&self) -> u32 {
        self.buffer.width() as u32
    }

    /// Gets the height of the buffer
    #[napi(getter)]
    pub fn height(&self) -> u32 {
        self.buffer.height() as u32
    }

    /// Gets the data type as a string
    #[napi(getter)]
    pub fn data_type(&self) -> String {
        format_data_type(self.buffer.data_type())
    }

    /// Gets the size in bytes
    #[napi(getter)]
    pub fn byte_size(&self) -> u32 {
        self.buffer.as_bytes().len() as u32
    }

    /// Gets the number of pixels
    #[napi(getter)]
    pub fn pixel_count(&self) -> u32 {
        (self.buffer.width() * self.buffer.height()) as u32
    }

    /// Creates a buffer from a Node.js Buffer
    #[napi(factory)]
    pub fn from_buffer(buffer: Buffer, width: u32, height: u32, data_type: String) -> Result<Self> {
        let dtype = parse_data_type(&data_type)?;
        let expected_size = (width as u64 * height as u64 * dtype.size_bytes() as u64) as usize;

        if buffer.len() != expected_size {
            return Err(NodeError {
                code: "BUFFER_SIZE_MISMATCH".to_string(),
                message: format!(
                    "Buffer size mismatch: expected {} bytes, got {}",
                    expected_size,
                    buffer.len()
                ),
            }
            .into());
        }

        let rust_buffer = RasterBuffer::new(
            buffer.as_ref().to_vec(),
            width as u64,
            height as u64,
            dtype,
            NoDataValue::None,
        )
        .to_napi()?;

        Ok(Self {
            buffer: rust_buffer,
        })
    }

    /// Converts to a Node.js Buffer (copies data)
    #[napi]
    pub fn to_buffer(&self) -> Result<Buffer> {
        let data = self.buffer.as_bytes();
        Ok(Buffer::from(data))
    }

    /// Gets a pixel value as f64
    #[napi]
    pub fn get_pixel(&self, x: u32, y: u32) -> Result<f64> {
        if x >= self.width() || y >= self.height() {
            return Err(NodeError {
                code: "OUT_OF_BOUNDS".to_string(),
                message: format!(
                    "Pixel ({}, {}) out of bounds for {}x{} raster",
                    x,
                    y,
                    self.width(),
                    self.height()
                ),
            }
            .into());
        }

        self.buffer.get_pixel(x as u64, y as u64).to_napi()
    }

    /// Sets a pixel value from f64
    #[napi]
    pub fn set_pixel(&mut self, x: u32, y: u32, value: f64) -> Result<()> {
        if x >= self.width() || y >= self.height() {
            return Err(NodeError {
                code: "OUT_OF_BOUNDS".to_string(),
                message: format!(
                    "Pixel ({}, {}) out of bounds for {}x{} raster",
                    x,
                    y,
                    self.width(),
                    self.height()
                ),
            }
            .into());
        }

        self.buffer.set_pixel(x as u64, y as u64, value).to_napi()
    }

    /// Fills the entire buffer with a value
    #[napi]
    pub fn fill(&mut self, value: f64) {
        self.buffer.fill_value(value);
    }

    /// Gets raw byte data (for advanced use)
    #[napi]
    pub fn get_bytes(&self) -> Result<Buffer> {
        let data = self.buffer.as_bytes();
        Ok(Buffer::from(data))
    }

    /// Computes statistics for the buffer
    #[napi]
    pub fn compute_statistics(&self) -> Result<Statistics> {
        let stats = self.buffer.compute_statistics().to_napi()?;
        Ok(Statistics {
            min: stats.min,
            max: stats.max,
            mean: stats.mean,
            stddev: stats.std_dev,
            valid_count: stats.valid_count as u32,
        })
    }

    /// Creates a copy of the buffer
    #[napi]
    pub fn clone(&self) -> Self {
        Self {
            buffer: self.buffer.clone(),
        }
    }

    /// Converts to a different data type
    #[napi]
    pub fn convert_to(&self, data_type: String) -> Result<Self> {
        let dtype = parse_data_type(&data_type)?;
        let converted = self.buffer.convert_to(dtype).to_napi()?;
        Ok(Self { buffer: converted })
    }
}

impl BufferWrapper {
    /// Creates a buffer wrapper from a RasterBuffer
    pub fn from_raster_buffer(buffer: RasterBuffer) -> Self {
        Self { buffer }
    }

    /// Gets the inner RasterBuffer
    pub fn inner(&self) -> &RasterBuffer {
        &self.buffer
    }
}

/// Statistics object for JavaScript
#[napi(object)]
pub struct Statistics {
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub stddev: f64,
    pub valid_count: u32,
}

/// Parse data type string to RasterDataType
fn parse_data_type(dtype: &str) -> Result<RasterDataType> {
    match dtype.to_lowercase().as_str() {
        "uint8" | "u8" => Ok(RasterDataType::UInt8),
        "int16" | "i16" => Ok(RasterDataType::Int16),
        "uint16" | "u16" => Ok(RasterDataType::UInt16),
        "int32" | "i32" => Ok(RasterDataType::Int32),
        "uint32" | "u32" => Ok(RasterDataType::UInt32),
        "float32" | "f32" => Ok(RasterDataType::Float32),
        "float64" | "f64" => Ok(RasterDataType::Float64),
        _ => Err(NodeError {
            code: "INVALID_DATA_TYPE".to_string(),
            message: format!("Unknown data type: {}", dtype),
        }
        .into()),
    }
}

/// Format RasterDataType to string
fn format_data_type(dtype: RasterDataType) -> String {
    match dtype {
        RasterDataType::UInt8 => "uint8".to_string(),
        RasterDataType::Int16 => "int16".to_string(),
        RasterDataType::UInt16 => "uint16".to_string(),
        RasterDataType::Int32 => "int32".to_string(),
        RasterDataType::UInt32 => "uint32".to_string(),
        RasterDataType::Float32 => "float32".to_string(),
        RasterDataType::Float64 => "float64".to_string(),
        _ => "unknown".to_string(),
    }
}

/// Creates a zero-filled buffer
#[allow(dead_code)]
#[napi]
pub fn create_buffer(width: u32, height: u32, data_type: String) -> Result<BufferWrapper> {
    BufferWrapper::new(width, height, data_type)
}

/// Creates a buffer from Node.js Buffer
#[allow(dead_code)]
#[napi]
pub fn buffer_from_bytes(
    buffer: Buffer,
    width: u32,
    height: u32,
    data_type: String,
) -> Result<BufferWrapper> {
    BufferWrapper::from_buffer(buffer, width, height, data_type)
}
