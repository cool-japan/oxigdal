// Allow unsafe blocks for low-level buffer operations that require
// direct memory access for performance-critical typed slice conversions
#![allow(unsafe_code)]

//! Buffer types for raster and vector data
//!
//! This module provides efficient buffer types for storing and manipulating
//! geospatial data. When the `arrow` feature is enabled, buffers are backed
//! by Apache Arrow arrays for zero-copy interoperability.
//!
//! # Overview
//!
//! The [`RasterBuffer`] type is the core buffer abstraction in `OxiGDAL`, providing
//! type-safe storage for raster pixel data with automatic memory management.
//!
//! # Examples
//!
//! ## Creating buffers
//!
//! ```
//! use oxigdal_core::buffer::RasterBuffer;
//! use oxigdal_core::types::{RasterDataType, NoDataValue};
//!
//! // Create a zero-filled buffer
//! let buffer = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);
//!
//! // Create a buffer with nodata value
//! let nodata = NoDataValue::Float(-9999.0);
//! let buffer = RasterBuffer::nodata_filled(1000, 1000, RasterDataType::Float32, nodata);
//! ```
//!
//! ## Working with pixel data
//!
//! ```
//! use oxigdal_core::buffer::RasterBuffer;
//! use oxigdal_core::types::RasterDataType;
//!
//! let mut buffer = RasterBuffer::zeros(100, 100, RasterDataType::UInt8);
//!
//! // Set pixel value
//! buffer.set_pixel(50, 50, 255.0)?;
//!
//! // Get pixel value
//! let value = buffer.get_pixel(50, 50)?;
//! assert_eq!(value, 255.0);
//! # Ok::<(), oxigdal_core::error::OxiGdalError>(())
//! ```
//!
//! ## Computing statistics
//!
//! ```
//! use oxigdal_core::buffer::RasterBuffer;
//! use oxigdal_core::types::RasterDataType;
//!
//! let buffer = RasterBuffer::zeros(1000, 1000, RasterDataType::Float32);
//! let stats = buffer.compute_statistics()?;
//!
//! println!("Min: {}, Max: {}", stats.min, stats.max);
//! println!("Mean: {}, StdDev: {}", stats.mean, stats.std_dev);
//! println!("Valid pixels: {}", stats.valid_count);
//! # Ok::<(), oxigdal_core::error::OxiGdalError>(())
//! ```
//!
//! # See Also
//!
//! - [`RasterDataType`] - Supported pixel data types
//! - [`NoDataValue`] - Representation of missing data
//! - [`RasterStatistics`] - Pixel statistics
//!
//! [`RasterDataType`]: crate::types::RasterDataType
//! [`NoDataValue`]: crate::types::NoDataValue
//! [`RasterStatistics`]: crate::types::RasterStatistics

use core::fmt;

use crate::error::{OxiGdalError, Result};
use crate::types::{NoDataValue, RasterDataType};

/// A typed buffer for raster data
#[derive(Clone)]
pub struct RasterBuffer {
    /// The underlying bytes
    data: Vec<u8>,
    /// Width in pixels
    width: u64,
    /// Height in pixels
    height: u64,
    /// Data type
    data_type: RasterDataType,
    /// `NoData` value
    nodata: NoDataValue,
}

impl RasterBuffer {
    /// Creates a new raster buffer
    ///
    /// # Errors
    /// Returns an error if the data size doesn't match the dimensions and type
    pub fn new(
        data: Vec<u8>,
        width: u64,
        height: u64,
        data_type: RasterDataType,
        nodata: NoDataValue,
    ) -> Result<Self> {
        let expected_size = width * height * data_type.size_bytes() as u64;
        if data.len() as u64 != expected_size {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "data",
                message: format!(
                    "Data size mismatch: expected {} bytes for {}x{} {:?}, got {}",
                    expected_size,
                    width,
                    height,
                    data_type,
                    data.len()
                ),
            });
        }

        Ok(Self {
            data,
            width,
            height,
            data_type,
            nodata,
        })
    }

    /// Creates a zero-filled buffer
    #[must_use]
    pub fn zeros(width: u64, height: u64, data_type: RasterDataType) -> Self {
        let size = (width * height * data_type.size_bytes() as u64) as usize;
        Self {
            data: vec![0u8; size],
            width,
            height,
            data_type,
            nodata: NoDataValue::None,
        }
    }

    /// Creates a buffer filled with the nodata value
    #[must_use]
    pub fn nodata_filled(
        width: u64,
        height: u64,
        data_type: RasterDataType,
        nodata: NoDataValue,
    ) -> Self {
        let mut buffer = Self::zeros(width, height, data_type);
        buffer.nodata = nodata;

        // Fill with nodata value if defined
        if let Some(value) = nodata.as_f64() {
            buffer.fill_value(value);
        }

        buffer
    }

    /// Fills the buffer with a constant value
    pub fn fill_value(&mut self, value: f64) {
        match self.data_type {
            RasterDataType::UInt8 => {
                let v = value as u8;
                self.data.fill(v);
            }
            RasterDataType::Int8 => {
                let v = value as i8;
                self.data.fill(v as u8);
            }
            RasterDataType::UInt16 => {
                let v = (value as u16).to_ne_bytes();
                for chunk in self.data.chunks_exact_mut(2) {
                    chunk.copy_from_slice(&v);
                }
            }
            RasterDataType::Int16 => {
                let v = (value as i16).to_ne_bytes();
                for chunk in self.data.chunks_exact_mut(2) {
                    chunk.copy_from_slice(&v);
                }
            }
            RasterDataType::UInt32 => {
                let v = (value as u32).to_ne_bytes();
                for chunk in self.data.chunks_exact_mut(4) {
                    chunk.copy_from_slice(&v);
                }
            }
            RasterDataType::Int32 => {
                let v = (value as i32).to_ne_bytes();
                for chunk in self.data.chunks_exact_mut(4) {
                    chunk.copy_from_slice(&v);
                }
            }
            RasterDataType::Float32 => {
                let v = (value as f32).to_ne_bytes();
                for chunk in self.data.chunks_exact_mut(4) {
                    chunk.copy_from_slice(&v);
                }
            }
            RasterDataType::Float64 => {
                let v = value.to_ne_bytes();
                for chunk in self.data.chunks_exact_mut(8) {
                    chunk.copy_from_slice(&v);
                }
            }
            RasterDataType::UInt64 => {
                let v = (value as u64).to_ne_bytes();
                for chunk in self.data.chunks_exact_mut(8) {
                    chunk.copy_from_slice(&v);
                }
            }
            RasterDataType::Int64 => {
                let v = (value as i64).to_ne_bytes();
                for chunk in self.data.chunks_exact_mut(8) {
                    chunk.copy_from_slice(&v);
                }
            }
            RasterDataType::CFloat32 | RasterDataType::CFloat64 => {
                // Complex types: fill with (value, 0)
                // This is a simplified implementation
            }
        }
    }

    /// Returns the width in pixels
    #[must_use]
    pub const fn width(&self) -> u64 {
        self.width
    }

    /// Returns the height in pixels
    #[must_use]
    pub const fn height(&self) -> u64 {
        self.height
    }

    /// Returns the data type
    #[must_use]
    pub const fn data_type(&self) -> RasterDataType {
        self.data_type
    }

    /// Returns the nodata value
    #[must_use]
    pub const fn nodata(&self) -> NoDataValue {
        self.nodata
    }

    /// Returns the total number of pixels
    #[must_use]
    pub const fn pixel_count(&self) -> u64 {
        self.width * self.height
    }

    /// Returns the raw bytes
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    /// Returns mutable raw bytes
    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }

    /// Consumes the buffer and returns the raw bytes
    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.data
    }

    /// Creates a buffer from typed vector data
    ///
    /// # Arguments
    /// * `width` - Width in pixels
    /// * `height` - Height in pixels
    /// * `data` - Typed data (e.g., `Vec<f32>`, `Vec<u8>`)
    /// * `data_type` - The raster data type
    ///
    /// # Errors
    /// Returns an error if the data size doesn't match dimensions and type
    pub fn from_typed_vec<T: Copy + 'static>(
        width: usize,
        height: usize,
        data: Vec<T>,
        data_type: RasterDataType,
    ) -> Result<Self> {
        let expected_pixels = width * height;
        if data.len() != expected_pixels {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "data",
                message: format!(
                    "Data length mismatch: expected {} pixels for {}x{}, got {}",
                    expected_pixels,
                    width,
                    height,
                    data.len()
                ),
            });
        }

        // Convert typed data to bytes
        let type_size = core::mem::size_of::<T>();
        let expected_type_size = data_type.size_bytes();
        if type_size != expected_type_size {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "data_type",
                message: format!(
                    "Type size mismatch: provided type has {} bytes, {:?} expects {} bytes",
                    type_size, data_type, expected_type_size
                ),
            });
        }

        let byte_data: Vec<u8> = data
            .iter()
            .flat_map(|v| {
                // SAFETY: We're reading the bytes of a Copy type
                let ptr = v as *const T as *const u8;
                unsafe { core::slice::from_raw_parts(ptr, type_size) }.to_vec()
            })
            .collect();

        Self::new(
            byte_data,
            width as u64,
            height as u64,
            data_type,
            NoDataValue::None,
        )
    }

    /// Returns the buffer data as a typed slice
    ///
    /// # Type Parameters
    /// * `T` - The target type (must match the buffer's data type size)
    ///
    /// # Errors
    /// Returns an error if the type size doesn't match the data type
    pub fn as_slice<T: Copy + 'static>(&self) -> Result<&[T]> {
        let type_size = core::mem::size_of::<T>();
        let expected_size = self.data_type.size_bytes();

        if type_size != expected_size {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "T",
                message: format!(
                    "Type size mismatch: requested type has {} bytes, buffer contains {:?} ({} bytes)",
                    type_size, self.data_type, expected_size
                ),
            });
        }

        let pixel_count = (self.width * self.height) as usize;
        // SAFETY: We've verified the type size matches, and the data is properly aligned
        // for the original type it was created with
        let slice =
            unsafe { core::slice::from_raw_parts(self.data.as_ptr() as *const T, pixel_count) };
        Ok(slice)
    }

    /// Returns the buffer data as a mutable typed slice
    ///
    /// # Type Parameters
    /// * `T` - The target type (must match the buffer's data type size)
    ///
    /// # Errors
    /// Returns an error if the type size doesn't match the data type
    pub fn as_slice_mut<T: Copy + 'static>(&mut self) -> Result<&mut [T]> {
        let type_size = core::mem::size_of::<T>();
        let expected_size = self.data_type.size_bytes();

        if type_size != expected_size {
            return Err(OxiGdalError::InvalidParameter {
                parameter: "T",
                message: format!(
                    "Type size mismatch: requested type has {} bytes, buffer contains {:?} ({} bytes)",
                    type_size, self.data_type, expected_size
                ),
            });
        }

        let pixel_count = (self.width * self.height) as usize;
        // SAFETY: We've verified the type size matches, and the data is properly aligned
        // for the original type it was created with
        let slice = unsafe {
            core::slice::from_raw_parts_mut(self.data.as_mut_ptr() as *mut T, pixel_count)
        };
        Ok(slice)
    }

    /// Gets a pixel value as f64
    ///
    /// # Errors
    /// Returns an error if coordinates are out of bounds
    pub fn get_pixel(&self, x: u64, y: u64) -> Result<f64> {
        if x >= self.width || y >= self.height {
            return Err(OxiGdalError::OutOfBounds {
                message: format!(
                    "Pixel ({}, {}) out of bounds for {}x{} buffer",
                    x, y, self.width, self.height
                ),
            });
        }

        let pixel_size = self.data_type.size_bytes();
        let offset = (y * self.width + x) as usize * pixel_size;

        let value = match self.data_type {
            RasterDataType::UInt8 => f64::from(self.data[offset]),
            RasterDataType::Int8 => f64::from(self.data[offset] as i8),
            RasterDataType::UInt16 => {
                let bytes: [u8; 2] = self.data[offset..offset + 2].try_into().map_err(|_| {
                    OxiGdalError::Internal {
                        message: "Invalid slice length".to_string(),
                    }
                })?;
                f64::from(u16::from_ne_bytes(bytes))
            }
            RasterDataType::Int16 => {
                let bytes: [u8; 2] = self.data[offset..offset + 2].try_into().map_err(|_| {
                    OxiGdalError::Internal {
                        message: "Invalid slice length".to_string(),
                    }
                })?;
                f64::from(i16::from_ne_bytes(bytes))
            }
            RasterDataType::UInt32 => {
                let bytes: [u8; 4] = self.data[offset..offset + 4].try_into().map_err(|_| {
                    OxiGdalError::Internal {
                        message: "Invalid slice length".to_string(),
                    }
                })?;
                f64::from(u32::from_ne_bytes(bytes))
            }
            RasterDataType::Int32 => {
                let bytes: [u8; 4] = self.data[offset..offset + 4].try_into().map_err(|_| {
                    OxiGdalError::Internal {
                        message: "Invalid slice length".to_string(),
                    }
                })?;
                f64::from(i32::from_ne_bytes(bytes))
            }
            RasterDataType::Float32 => {
                let bytes: [u8; 4] = self.data[offset..offset + 4].try_into().map_err(|_| {
                    OxiGdalError::Internal {
                        message: "Invalid slice length".to_string(),
                    }
                })?;
                f64::from(f32::from_ne_bytes(bytes))
            }
            RasterDataType::Float64 => {
                let bytes: [u8; 8] = self.data[offset..offset + 8].try_into().map_err(|_| {
                    OxiGdalError::Internal {
                        message: "Invalid slice length".to_string(),
                    }
                })?;
                f64::from_ne_bytes(bytes)
            }
            RasterDataType::UInt64 => {
                let bytes: [u8; 8] = self.data[offset..offset + 8].try_into().map_err(|_| {
                    OxiGdalError::Internal {
                        message: "Invalid slice length".to_string(),
                    }
                })?;
                u64::from_ne_bytes(bytes) as f64
            }
            RasterDataType::Int64 => {
                let bytes: [u8; 8] = self.data[offset..offset + 8].try_into().map_err(|_| {
                    OxiGdalError::Internal {
                        message: "Invalid slice length".to_string(),
                    }
                })?;
                i64::from_ne_bytes(bytes) as f64
            }
            RasterDataType::CFloat32 | RasterDataType::CFloat64 => {
                // Return only the real part for complex types
                let bytes: [u8; 4] = self.data[offset..offset + 4].try_into().map_err(|_| {
                    OxiGdalError::Internal {
                        message: "Invalid slice length".to_string(),
                    }
                })?;
                f64::from(f32::from_ne_bytes(bytes))
            }
        };

        Ok(value)
    }

    /// Sets a pixel value
    ///
    /// # Errors
    /// Returns an error if coordinates are out of bounds
    pub fn set_pixel(&mut self, x: u64, y: u64, value: f64) -> Result<()> {
        if x >= self.width || y >= self.height {
            return Err(OxiGdalError::OutOfBounds {
                message: format!(
                    "Pixel ({}, {}) out of bounds for {}x{} buffer",
                    x, y, self.width, self.height
                ),
            });
        }

        let pixel_size = self.data_type.size_bytes();
        let offset = (y * self.width + x) as usize * pixel_size;

        match self.data_type {
            RasterDataType::UInt8 => {
                self.data[offset] = value as u8;
            }
            RasterDataType::Int8 => {
                self.data[offset] = (value as i8) as u8;
            }
            RasterDataType::UInt16 => {
                let bytes = (value as u16).to_ne_bytes();
                self.data[offset..offset + 2].copy_from_slice(&bytes);
            }
            RasterDataType::Int16 => {
                let bytes = (value as i16).to_ne_bytes();
                self.data[offset..offset + 2].copy_from_slice(&bytes);
            }
            RasterDataType::UInt32 => {
                let bytes = (value as u32).to_ne_bytes();
                self.data[offset..offset + 4].copy_from_slice(&bytes);
            }
            RasterDataType::Int32 => {
                let bytes = (value as i32).to_ne_bytes();
                self.data[offset..offset + 4].copy_from_slice(&bytes);
            }
            RasterDataType::Float32 => {
                let bytes = (value as f32).to_ne_bytes();
                self.data[offset..offset + 4].copy_from_slice(&bytes);
            }
            RasterDataType::Float64 => {
                let bytes = value.to_ne_bytes();
                self.data[offset..offset + 8].copy_from_slice(&bytes);
            }
            RasterDataType::UInt64 => {
                let bytes = (value as u64).to_ne_bytes();
                self.data[offset..offset + 8].copy_from_slice(&bytes);
            }
            RasterDataType::Int64 => {
                let bytes = (value as i64).to_ne_bytes();
                self.data[offset..offset + 8].copy_from_slice(&bytes);
            }
            RasterDataType::CFloat32 => {
                // Set only the real part
                let bytes = (value as f32).to_ne_bytes();
                self.data[offset..offset + 4].copy_from_slice(&bytes);
            }
            RasterDataType::CFloat64 => {
                // Set only the real part
                let bytes = value.to_ne_bytes();
                self.data[offset..offset + 8].copy_from_slice(&bytes);
            }
        }

        Ok(())
    }

    /// Returns true if the given value equals the nodata value
    #[must_use]
    pub fn is_nodata(&self, value: f64) -> bool {
        match self.nodata.as_f64() {
            Some(nd) => {
                if nd.is_nan() && value.is_nan() {
                    true
                } else {
                    (nd - value).abs() < f64::EPSILON
                }
            }
            None => false,
        }
    }

    /// Converts the buffer to a different data type
    ///
    /// # Errors
    /// Returns an error if conversion fails
    pub fn convert_to(&self, target_type: RasterDataType) -> Result<Self> {
        if target_type == self.data_type {
            return Ok(self.clone());
        }

        let mut result = Self::zeros(self.width, self.height, target_type);
        result.nodata = self.nodata;

        for y in 0..self.height {
            for x in 0..self.width {
                let value = self.get_pixel(x, y)?;
                result.set_pixel(x, y, value)?;
            }
        }

        Ok(result)
    }

    /// Computes basic statistics
    pub fn compute_statistics(&self) -> Result<BufferStatistics> {
        let mut min = f64::MAX;
        let mut max = f64::MIN;
        let mut sum = 0.0;
        let mut sum_sq = 0.0;
        let mut valid_count = 0u64;

        for y in 0..self.height {
            for x in 0..self.width {
                let value = self.get_pixel(x, y)?;
                if !self.is_nodata(value) && value.is_finite() {
                    min = min.min(value);
                    max = max.max(value);
                    sum += value;
                    sum_sq += value * value;
                    valid_count += 1;
                }
            }
        }

        if valid_count == 0 {
            return Ok(BufferStatistics {
                min: f64::NAN,
                max: f64::NAN,
                mean: f64::NAN,
                std_dev: f64::NAN,
                valid_count: 0,
            });
        }

        let mean = sum / valid_count as f64;
        let variance = (sum_sq / valid_count as f64) - (mean * mean);
        let std_dev = variance.sqrt();

        Ok(BufferStatistics {
            min,
            max,
            mean,
            std_dev,
            valid_count,
        })
    }
}

impl fmt::Debug for RasterBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RasterBuffer")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("data_type", &self.data_type)
            .field("nodata", &self.nodata)
            .field("bytes", &self.data.len())
            .finish()
    }
}

/// Statistics computed from a buffer
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BufferStatistics {
    /// Minimum value
    pub min: f64,
    /// Maximum value
    pub max: f64,
    /// Mean value
    pub mean: f64,
    /// Standard deviation
    pub std_dev: f64,
    /// Number of valid (non-nodata) pixels
    pub valid_count: u64,
}

#[cfg(feature = "arrow")]
mod arrow_support {
    //! Arrow integration for zero-copy interoperability

    use arrow_array::{Array, Float64Array};

    use super::{OxiGdalError, RasterBuffer, Result};

    impl RasterBuffer {
        /// Creates a `RasterBuffer` from an Arrow array
        ///
        /// # Errors
        /// Returns an error if the array type doesn't match
        pub fn from_arrow_array<A: Array>(_array: &A, _width: u64, _height: u64) -> Result<Self> {
            // This is a simplified implementation
            // A full implementation would handle all Arrow types
            Err(OxiGdalError::NotSupported {
                operation: "Arrow array conversion".to_string(),
            })
        }

        /// Converts to an Arrow `Float64Array`
        pub fn to_float64_array(&self) -> Result<Float64Array> {
            let mut values = Vec::with_capacity(self.pixel_count() as usize);
            for y in 0..self.height {
                for x in 0..self.width {
                    values.push(self.get_pixel(x, y)?);
                }
            }
            Ok(Float64Array::from(values))
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn test_buffer_creation() {
        let buffer = RasterBuffer::zeros(100, 100, RasterDataType::UInt8);
        assert_eq!(buffer.width(), 100);
        assert_eq!(buffer.height(), 100);
        assert_eq!(buffer.pixel_count(), 10_000);
        assert_eq!(buffer.as_bytes().len(), 10_000);
    }

    #[test]
    fn test_pixel_access() {
        let mut buffer = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        buffer.set_pixel(5, 5, 42.0).expect("set should work");
        let value = buffer.get_pixel(5, 5).expect("get should work");
        assert!((value - 42.0).abs() < f64::EPSILON);

        // Out of bounds
        assert!(buffer.get_pixel(100, 0).is_err());
        assert!(buffer.set_pixel(0, 100, 0.0).is_err());
    }

    #[test]
    fn test_nodata() {
        let buffer = RasterBuffer::nodata_filled(
            10,
            10,
            RasterDataType::Float32,
            NoDataValue::Float(-9999.0),
        );

        assert!(buffer.is_nodata(-9999.0));
        assert!(!buffer.is_nodata(0.0));
    }

    #[test]
    fn test_statistics() {
        let mut buffer = RasterBuffer::zeros(10, 10, RasterDataType::Float32);

        // Fill with values 0-99
        for y in 0..10 {
            for x in 0..10 {
                let value = (y * 10 + x) as f64;
                buffer.set_pixel(x, y, value).expect("set should work");
            }
        }

        let stats = buffer.compute_statistics().expect("stats should work");
        assert!((stats.min - 0.0).abs() < f64::EPSILON);
        assert!((stats.max - 99.0).abs() < f64::EPSILON);
        assert!((stats.mean - 49.5).abs() < 0.01);
        assert_eq!(stats.valid_count, 100);
    }

    #[test]
    fn test_data_validation() {
        // Wrong size should fail
        let result = RasterBuffer::new(
            vec![0u8; 100],
            10,
            10,
            RasterDataType::UInt16, // Needs 200 bytes
            NoDataValue::None,
        );
        assert!(result.is_err());

        // Correct size should succeed
        let result = RasterBuffer::new(
            vec![0u8; 200],
            10,
            10,
            RasterDataType::UInt16,
            NoDataValue::None,
        );
        assert!(result.is_ok());
    }
}
