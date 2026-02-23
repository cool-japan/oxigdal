//! NumPy array integration for OxiGDAL
//!
//! This module provides conversion between OxiGDAL RasterBuffer and NumPy arrays.

use crate::error::oxigdal_error_to_py_err;
use numpy::{Element, PyArray2, PyArrayMethods, PyUntypedArrayMethods};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{NoDataValue, RasterDataType};
use pyo3::prelude::*;

/// Converts a RasterBuffer to a NumPy array
///
/// This function creates a NumPy array from the raster buffer data.
/// The array will have shape (height, width) and the appropriate dtype.
pub fn buffer_to_numpy<'py>(
    py: Python<'py>,
    buffer: &RasterBuffer,
) -> PyResult<Bound<'py, PyArray2<f64>>> {
    let height = buffer.height() as usize;
    let width = buffer.width() as usize;

    // Create a vector to hold the data
    let mut data = Vec::with_capacity(height * width);

    // Extract pixel values
    for y in 0..buffer.height() {
        for x in 0..buffer.width() {
            let value = buffer.get_pixel(x, y).map_err(oxigdal_error_to_py_err)?;
            data.push(value);
        }
    }

    // Convert to NumPy array with shape (height, width)
    let array = PyArray2::from_vec2(
        py,
        &data
            .chunks(width)
            .map(|chunk| chunk.to_vec())
            .collect::<Vec<_>>(),
    )
    .map_err(|e| {
        pyo3::exceptions::PyValueError::new_err(format!("NumPy array creation failed: {}", e))
    })?;

    Ok(array)
}

/// Converts a NumPy array to a RasterBuffer
///
/// This function creates a RasterBuffer from NumPy array data.
/// The array should have shape (height, width).
#[allow(dead_code)]
pub fn numpy_to_buffer<'py, T>(
    array: &Bound<'py, PyArray2<T>>,
    data_type: RasterDataType,
    _nodata: NoDataValue,
) -> PyResult<RasterBuffer>
where
    T: Element + Into<f64> + Copy,
{
    let shape = array.shape();
    if shape.len() != 2 {
        return Err(pyo3::exceptions::PyValueError::new_err(format!(
            "Expected 2D array, got {}D",
            shape.len()
        )));
    }

    let height = shape[0] as u64;
    let width = shape[1] as u64;

    // Read array data
    let readonly = array.readonly();
    let slice = readonly
        .as_slice()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("Array must be contiguous"))?;

    // Create buffer and fill with data
    let mut buffer = RasterBuffer::zeros(width, height, data_type);

    for (idx, &value) in slice.iter().enumerate() {
        let y = (idx / width as usize) as u64;
        let x = (idx % width as usize) as u64;
        let f64_value: f64 = value.into();
        buffer
            .set_pixel(x, y, f64_value)
            .map_err(oxigdal_error_to_py_err)?;
    }

    Ok(buffer)
}

/// Converts RasterDataType to NumPy dtype string
pub fn data_type_to_numpy_dtype(data_type: RasterDataType) -> &'static str {
    match data_type {
        RasterDataType::UInt8 => "uint8",
        RasterDataType::Int8 => "int8",
        RasterDataType::UInt16 => "uint16",
        RasterDataType::Int16 => "int16",
        RasterDataType::UInt32 => "uint32",
        RasterDataType::Int32 => "int32",
        RasterDataType::UInt64 => "uint64",
        RasterDataType::Int64 => "int64",
        RasterDataType::Float32 => "float32",
        RasterDataType::Float64 => "float64",
        RasterDataType::CFloat32 => "complex64",
        RasterDataType::CFloat64 => "complex128",
    }
}

/// Infers RasterDataType from NumPy dtype string
#[allow(dead_code)]
pub fn numpy_dtype_to_data_type(dtype: &str) -> PyResult<RasterDataType> {
    match dtype {
        "uint8" | "u1" => Ok(RasterDataType::UInt8),
        "int8" | "i1" => Ok(RasterDataType::Int8),
        "uint16" | "u2" => Ok(RasterDataType::UInt16),
        "int16" | "i2" => Ok(RasterDataType::Int16),
        "uint32" | "u4" => Ok(RasterDataType::UInt32),
        "int32" | "i4" => Ok(RasterDataType::Int32),
        "uint64" | "u8" => Ok(RasterDataType::UInt64),
        "int64" | "i8" => Ok(RasterDataType::Int64),
        "float32" | "f4" => Ok(RasterDataType::Float32),
        "float64" | "f8" => Ok(RasterDataType::Float64),
        "complex64" | "c8" => Ok(RasterDataType::CFloat32),
        "complex128" | "c16" => Ok(RasterDataType::CFloat64),
        _ => Err(pyo3::exceptions::PyValueError::new_err(format!(
            "Unsupported NumPy dtype: {}",
            dtype
        ))),
    }
}

/// Helper to create a NumPy array from raw data with specified dtype
#[allow(dead_code)]
pub fn create_numpy_array<'py, T>(
    py: Python<'py>,
    data: Vec<T>,
    shape: (usize, usize),
) -> PyResult<Bound<'py, PyArray2<T>>>
where
    T: Element + Clone,
{
    let (_height, width) = shape;
    let array = data
        .chunks(width)
        .map(|chunk| chunk.to_vec())
        .collect::<Vec<_>>();

    PyArray2::from_vec2(py, &array).map_err(|e| {
        pyo3::exceptions::PyValueError::new_err(format!("Failed to create NumPy array: {}", e))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_type_conversion() {
        assert_eq!(data_type_to_numpy_dtype(RasterDataType::UInt8), "uint8");
        assert_eq!(data_type_to_numpy_dtype(RasterDataType::Float32), "float32");
        assert_eq!(data_type_to_numpy_dtype(RasterDataType::Float64), "float64");
    }

    #[test]
    fn test_numpy_dtype_to_data_type() {
        assert!(matches!(
            numpy_dtype_to_data_type("uint8"),
            Ok(RasterDataType::UInt8)
        ));
        assert!(matches!(
            numpy_dtype_to_data_type("float32"),
            Ok(RasterDataType::Float32)
        ));
        assert!(matches!(
            numpy_dtype_to_data_type("f8"),
            Ok(RasterDataType::Float64)
        ));
        assert!(numpy_dtype_to_data_type("invalid").is_err());
    }
}
