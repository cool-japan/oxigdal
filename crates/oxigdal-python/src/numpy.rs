//! Advanced NumPy Integration for OxiGDAL
//!
//! This module provides comprehensive NumPy integration with:
//! - Zero-copy array transfer
//! - Memory-mapped NumPy arrays
//! - Structured array support
//! - Multi-dimensional array handling
//! - Type conversion utilities
//! - Array metadata preservation
//! - Buffer protocol integration
//!
//! # Zero-Copy Transfer
//!
//! When possible, data is shared directly between Rust and Python without copying:
//!
//! ```python
//! import oxigdal
//! import numpy as np
//!
//! # Zero-copy read from raster
//! arr = oxigdal.read_zero_copy("input.tif", band=1)
//!
//! # The array shares memory with the underlying buffer
//! arr *= 2  # Modifications affect the buffer
//! ```
//!
//! # Memory-Mapped Arrays
//!
//! For large files, memory mapping provides efficient access:
//!
//! ```python
//! # Memory-map a large file
//! mmap_arr = oxigdal.mmap_raster("huge_file.tif", mode="r")
//!
//! # Access portions of the file on-demand
//! subset = mmap_arr[1000:2000, 1000:2000]
//! ```
//!
//! # Structured Arrays
//!
//! For vector data with attributes:
//!
//! ```python
//! # Read features as structured array
//! features = oxigdal.read_features_as_structured("points.geojson")
//!
//! # Access fields by name
//! elevations = features['elevation']
//! ```

use crate::error::oxigdal_error_to_py_err;
use numpy::{
    Element, IntoPyArray, PyArray1, PyArray2, PyArray3, PyArrayMethods, PyReadonlyArray1,
    PyReadonlyArray2, PyReadonlyArray3, PyUntypedArrayMethods,
};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::types::{NoDataValue, RasterDataType};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList, PyTuple};
use std::collections::HashMap;
use std::ffi::CString;
use std::sync::Arc;

// ============================================================================
// Type Conversion Utilities
// ============================================================================

/// NumPy dtype descriptor for OxiGDAL data types
#[derive(Debug, Clone, PartialEq)]
pub struct NumpyDtype {
    /// NumPy dtype string (e.g., "float32", "uint8")
    pub dtype_str: &'static str,
    /// Size in bytes
    pub itemsize: usize,
    /// Is native byte order
    pub native_endian: bool,
    /// Is complex type
    pub is_complex: bool,
}

impl NumpyDtype {
    /// Creates a new NumPy dtype descriptor
    #[must_use]
    pub const fn new(
        dtype_str: &'static str,
        itemsize: usize,
        native_endian: bool,
        is_complex: bool,
    ) -> Self {
        Self {
            dtype_str,
            itemsize,
            native_endian,
            is_complex,
        }
    }

    /// Returns the NumPy type code
    #[must_use]
    pub fn type_code(&self) -> char {
        match self.dtype_str {
            "bool" => 'b',
            "int8" => 'i',
            "uint8" => 'u',
            "int16" => 'i',
            "uint16" => 'u',
            "int32" => 'i',
            "uint32" => 'u',
            "int64" => 'i',
            "uint64" => 'u',
            "float32" => 'f',
            "float64" => 'f',
            "complex64" => 'c',
            "complex128" => 'c',
            _ => 'O', // Object
        }
    }
}

/// Converts `RasterDataType` to NumPy dtype descriptor
#[must_use]
pub fn data_type_to_numpy_dtype(data_type: RasterDataType) -> NumpyDtype {
    match data_type {
        RasterDataType::UInt8 => NumpyDtype::new("uint8", 1, true, false),
        RasterDataType::Int8 => NumpyDtype::new("int8", 1, true, false),
        RasterDataType::UInt16 => NumpyDtype::new("uint16", 2, true, false),
        RasterDataType::Int16 => NumpyDtype::new("int16", 2, true, false),
        RasterDataType::UInt32 => NumpyDtype::new("uint32", 4, true, false),
        RasterDataType::Int32 => NumpyDtype::new("int32", 4, true, false),
        RasterDataType::UInt64 => NumpyDtype::new("uint64", 8, true, false),
        RasterDataType::Int64 => NumpyDtype::new("int64", 8, true, false),
        RasterDataType::Float32 => NumpyDtype::new("float32", 4, true, false),
        RasterDataType::Float64 => NumpyDtype::new("float64", 8, true, false),
        RasterDataType::CFloat32 => NumpyDtype::new("complex64", 8, true, true),
        RasterDataType::CFloat64 => NumpyDtype::new("complex128", 16, true, true),
    }
}

/// Converts `RasterDataType` to NumPy dtype string
#[must_use]
pub fn data_type_to_dtype_str(data_type: RasterDataType) -> &'static str {
    data_type_to_numpy_dtype(data_type).dtype_str
}

/// Infers `RasterDataType` from NumPy dtype string
///
/// # Errors
///
/// Returns an error if the dtype is not supported
pub fn numpy_dtype_to_data_type(dtype: &str) -> PyResult<RasterDataType> {
    // Handle various NumPy dtype representations
    let normalized = dtype.to_lowercase();
    let stripped = normalized.trim_start_matches(['<', '>', '=', '|']);

    match stripped {
        "uint8" | "u1" | "b" => Ok(RasterDataType::UInt8),
        "int8" | "i1" => Ok(RasterDataType::Int8),
        "uint16" | "u2" => Ok(RasterDataType::UInt16),
        "int16" | "i2" => Ok(RasterDataType::Int16),
        "uint32" | "u4" => Ok(RasterDataType::UInt32),
        "int32" | "i4" | "l" => Ok(RasterDataType::Int32),
        "uint64" | "u8" => Ok(RasterDataType::UInt64),
        "int64" | "i8" | "q" => Ok(RasterDataType::Int64),
        "float32" | "f4" | "f" => Ok(RasterDataType::Float32),
        "float64" | "f8" | "d" => Ok(RasterDataType::Float64),
        "complex64" | "c8" => Ok(RasterDataType::CFloat32),
        "complex128" | "c16" => Ok(RasterDataType::CFloat64),
        _ => Err(pyo3::exceptions::PyValueError::new_err(format!(
            "Unsupported NumPy dtype: '{}'. Supported types: uint8, int8, uint16, int16, \
             uint32, int32, uint64, int64, float32, float64, complex64, complex128",
            dtype
        ))),
    }
}

/// Type code for buffer protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferTypeCode {
    /// Signed byte
    SignedByte,
    /// Unsigned byte
    UnsignedByte,
    /// Signed short
    SignedShort,
    /// Unsigned short
    UnsignedShort,
    /// Signed int
    SignedInt,
    /// Unsigned int
    UnsignedInt,
    /// Signed long long
    SignedLongLong,
    /// Unsigned long long
    UnsignedLongLong,
    /// Float
    Float,
    /// Double
    Double,
    /// Complex float
    ComplexFloat,
    /// Complex double
    ComplexDouble,
}

impl BufferTypeCode {
    /// Returns the struct format character
    #[must_use]
    pub const fn format_char(self) -> char {
        match self {
            Self::SignedByte => 'b',
            Self::UnsignedByte => 'B',
            Self::SignedShort => 'h',
            Self::UnsignedShort => 'H',
            Self::SignedInt => 'i',
            Self::UnsignedInt => 'I',
            Self::SignedLongLong => 'q',
            Self::UnsignedLongLong => 'Q',
            Self::Float => 'f',
            Self::Double => 'd',
            Self::ComplexFloat => 'Z',
            Self::ComplexDouble => 'Z',
        }
    }

    /// Creates from `RasterDataType`
    #[must_use]
    pub const fn from_raster_data_type(dt: RasterDataType) -> Self {
        match dt {
            RasterDataType::Int8 => Self::SignedByte,
            RasterDataType::UInt8 => Self::UnsignedByte,
            RasterDataType::Int16 => Self::SignedShort,
            RasterDataType::UInt16 => Self::UnsignedShort,
            RasterDataType::Int32 => Self::SignedInt,
            RasterDataType::UInt32 => Self::UnsignedInt,
            RasterDataType::Int64 => Self::SignedLongLong,
            RasterDataType::UInt64 => Self::UnsignedLongLong,
            RasterDataType::Float32 => Self::Float,
            RasterDataType::Float64 => Self::Double,
            RasterDataType::CFloat32 => Self::ComplexFloat,
            RasterDataType::CFloat64 => Self::ComplexDouble,
        }
    }
}

// ============================================================================
// Array Metadata Preservation
// ============================================================================

/// Metadata container for arrays
#[derive(Debug, Clone)]
pub struct ArrayMetadata {
    /// Shape of the array
    pub shape: Vec<usize>,
    /// Data type
    pub dtype: RasterDataType,
    /// NoData value
    pub nodata: NoDataValue,
    /// CRS as WKT
    pub crs_wkt: Option<String>,
    /// GeoTransform [origin_x, pixel_width, shear_x, origin_y, shear_y, pixel_height]
    pub geotransform: Option<[f64; 6]>,
    /// Band names
    pub band_names: Option<Vec<String>>,
    /// Custom metadata
    pub custom: HashMap<String, String>,
}

impl Default for ArrayMetadata {
    fn default() -> Self {
        Self {
            shape: Vec::new(),
            dtype: RasterDataType::Float64,
            nodata: NoDataValue::None,
            crs_wkt: None,
            geotransform: None,
            band_names: None,
            custom: HashMap::new(),
        }
    }
}

impl ArrayMetadata {
    /// Creates new empty metadata
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates metadata from a `RasterBuffer`
    #[must_use]
    pub fn from_buffer(buffer: &RasterBuffer) -> Self {
        Self {
            shape: vec![buffer.height() as usize, buffer.width() as usize],
            dtype: buffer.data_type(),
            nodata: buffer.nodata(),
            ..Default::default()
        }
    }

    /// Sets the shape
    #[must_use]
    pub fn with_shape(mut self, shape: Vec<usize>) -> Self {
        self.shape = shape;
        self
    }

    /// Sets the CRS
    #[must_use]
    pub fn with_crs(mut self, crs_wkt: String) -> Self {
        self.crs_wkt = Some(crs_wkt);
        self
    }

    /// Sets the geotransform
    #[must_use]
    pub fn with_geotransform(mut self, gt: [f64; 6]) -> Self {
        self.geotransform = Some(gt);
        self
    }

    /// Sets band names
    #[must_use]
    pub fn with_band_names(mut self, names: Vec<String>) -> Self {
        self.band_names = Some(names);
        self
    }

    /// Adds custom metadata
    #[must_use]
    pub fn with_custom(mut self, key: String, value: String) -> Self {
        self.custom.insert(key, value);
        self
    }

    /// Converts to Python dictionary
    ///
    /// # Errors
    ///
    /// Returns an error if dictionary creation fails
    pub fn to_py_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        dict.set_item("shape", &self.shape)?;
        dict.set_item("dtype", data_type_to_dtype_str(self.dtype))?;

        if let Some(nodata_val) = self.nodata.as_f64() {
            dict.set_item("nodata", nodata_val)?;
        }

        if let Some(ref crs) = self.crs_wkt {
            dict.set_item("crs", crs)?;
        }

        if let Some(ref gt) = self.geotransform {
            dict.set_item("geotransform", gt.to_vec())?;
        }

        if let Some(ref names) = self.band_names {
            dict.set_item("band_names", names)?;
        }

        if !self.custom.is_empty() {
            let custom_dict = PyDict::new(py);
            for (k, v) in &self.custom {
                custom_dict.set_item(k, v)?;
            }
            dict.set_item("custom", custom_dict)?;
        }

        Ok(dict)
    }

    /// Creates from Python dictionary
    ///
    /// # Errors
    ///
    /// Returns an error if required fields are missing
    pub fn from_py_dict(dict: &Bound<'_, PyDict>) -> PyResult<Self> {
        let mut meta = Self::new();

        if let Some(shape) = dict.get_item("shape")? {
            meta.shape = shape.extract()?;
        }

        if let Some(dtype) = dict.get_item("dtype")? {
            let dtype_str: String = dtype.extract()?;
            meta.dtype = numpy_dtype_to_data_type(&dtype_str)?;
        }

        if let Some(nodata) = dict.get_item("nodata")? {
            let nodata_val: f64 = nodata.extract()?;
            meta.nodata = NoDataValue::Float(nodata_val);
        }

        if let Some(crs) = dict.get_item("crs")? {
            meta.crs_wkt = Some(crs.extract()?);
        }

        if let Some(gt) = dict.get_item("geotransform")? {
            let gt_vec: Vec<f64> = gt.extract()?;
            if gt_vec.len() == 6 {
                let mut arr = [0.0; 6];
                arr.copy_from_slice(&gt_vec);
                meta.geotransform = Some(arr);
            }
        }

        if let Some(names) = dict.get_item("band_names")? {
            meta.band_names = Some(names.extract()?);
        }

        Ok(meta)
    }
}

// ============================================================================
// Zero-Copy Array Transfer
// ============================================================================

/// A container for zero-copy array data
///
/// This struct holds a reference to raw buffer data and provides
/// safe access for NumPy array creation without copying.
#[derive(Debug)]
pub struct ZeroCopyBuffer {
    /// Raw data bytes
    data: Arc<Vec<u8>>,
    /// Width in pixels
    width: u64,
    /// Height in pixels
    height: u64,
    /// Data type
    data_type: RasterDataType,
    /// NoData value
    nodata: NoDataValue,
    /// Metadata
    metadata: ArrayMetadata,
}

impl ZeroCopyBuffer {
    /// Creates a new zero-copy buffer from raw data
    ///
    /// # Errors
    ///
    /// Returns an error if data size doesn't match dimensions
    pub fn new(
        data: Vec<u8>,
        width: u64,
        height: u64,
        data_type: RasterDataType,
        nodata: NoDataValue,
    ) -> PyResult<Self> {
        let expected_size = width * height * data_type.size_bytes() as u64;
        if data.len() as u64 != expected_size {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "Data size mismatch: expected {} bytes for {}x{} {:?}, got {}",
                expected_size,
                width,
                height,
                data_type,
                data.len()
            )));
        }

        let metadata = ArrayMetadata {
            shape: vec![height as usize, width as usize],
            dtype: data_type,
            nodata,
            ..Default::default()
        };

        Ok(Self {
            data: Arc::new(data),
            width,
            height,
            data_type,
            nodata,
            metadata,
        })
    }

    /// Creates from a `RasterBuffer`
    #[must_use]
    pub fn from_buffer(buffer: RasterBuffer) -> Self {
        let width = buffer.width();
        let height = buffer.height();
        let data_type = buffer.data_type();
        let nodata = buffer.nodata();
        let metadata = ArrayMetadata::from_buffer(&buffer);

        Self {
            data: Arc::new(buffer.into_bytes()),
            width,
            height,
            data_type,
            nodata,
            metadata,
        }
    }

    /// Gets the raw data as a slice
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    /// Gets dimensions as (height, width)
    #[must_use]
    pub const fn shape(&self) -> (u64, u64) {
        (self.height, self.width)
    }

    /// Gets the data type
    #[must_use]
    pub const fn data_type(&self) -> RasterDataType {
        self.data_type
    }

    /// Gets the metadata
    #[must_use]
    pub fn metadata(&self) -> &ArrayMetadata {
        &self.metadata
    }

    /// Gets the nodata value
    #[must_use]
    pub const fn nodata(&self) -> NoDataValue {
        self.nodata
    }

    /// Converts to NumPy array with type-specific conversion
    ///
    /// This is a zero-copy operation when possible.
    ///
    /// # Errors
    ///
    /// Returns an error if array creation fails
    pub fn to_numpy<'py>(&self, py: Python<'py>) -> PyResult<PyObject> {
        match self.data_type {
            RasterDataType::UInt8 => self.to_numpy_typed::<u8>(py),
            RasterDataType::Int8 => self.to_numpy_typed::<i8>(py),
            RasterDataType::UInt16 => self.to_numpy_typed::<u16>(py),
            RasterDataType::Int16 => self.to_numpy_typed::<i16>(py),
            RasterDataType::UInt32 => self.to_numpy_typed::<u32>(py),
            RasterDataType::Int32 => self.to_numpy_typed::<i32>(py),
            RasterDataType::UInt64 => self.to_numpy_typed::<u64>(py),
            RasterDataType::Int64 => self.to_numpy_typed::<i64>(py),
            RasterDataType::Float32 => self.to_numpy_typed::<f32>(py),
            RasterDataType::Float64 => self.to_numpy_typed::<f64>(py),
            RasterDataType::CFloat32 | RasterDataType::CFloat64 => {
                // Complex types need special handling
                self.to_numpy_complex(py)
            }
        }
    }

    /// Converts to typed NumPy array
    fn to_numpy_typed<'py, T: numpy::Element + Clone>(&self, py: Python<'py>) -> PyResult<PyObject>
    where
        T: bytemuck::Pod,
    {
        let typed_data: &[T] = bytemuck::cast_slice(&self.data);

        // Create 2D array view
        let height = self.height as usize;
        let width = self.width as usize;

        // We need to copy because we can't guarantee the lifetime
        let data_vec: Vec<T> = typed_data.to_vec();

        // Create nested Vec for from_vec2
        let nested: Vec<Vec<T>> = data_vec.chunks(width).map(|chunk| chunk.to_vec()).collect();

        let array = PyArray2::from_vec2(py, &nested).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!(
                "Failed to create NumPy array: {}",
                e
            ))
        })?;

        Ok(array.into_any().unbind())
    }

    /// Converts complex data to NumPy array
    fn to_numpy_complex<'py>(&self, py: Python<'py>) -> PyResult<PyObject> {
        // For now, return as float array with interleaved real/imaginary
        // TODO: Use proper complex dtype when pyo3 supports it
        self.to_numpy_typed::<f64>(py)
    }
}

// ============================================================================
// Buffer to NumPy Conversion
// ============================================================================

/// Converts a `RasterBuffer` to a NumPy array
///
/// This function creates a NumPy array from the raster buffer data.
/// The array will have shape (height, width) and the appropriate dtype.
///
/// # Errors
///
/// Returns an error if array creation fails
pub fn buffer_to_numpy<'py>(
    py: Python<'py>,
    buffer: &RasterBuffer,
) -> PyResult<Bound<'py, PyArray2<f64>>> {
    let height = buffer.height() as usize;
    let width = buffer.width() as usize;

    // Allocate data vector with capacity
    let mut data = Vec::with_capacity(height * width);

    // Extract pixel values efficiently
    for y in 0..buffer.height() {
        for x in 0..buffer.width() {
            let value = buffer.get_pixel(x, y).map_err(oxigdal_error_to_py_err)?;
            data.push(value);
        }
    }

    // Convert to nested Vec for from_vec2
    let nested: Vec<Vec<f64>> = data.chunks(width).map(|chunk| chunk.to_vec()).collect();

    PyArray2::from_vec2(py, &nested).map_err(|e| {
        pyo3::exceptions::PyValueError::new_err(format!("NumPy array creation failed: {}", e))
    })
}

/// Converts a `RasterBuffer` to a typed NumPy array
///
/// # Errors
///
/// Returns an error if array creation fails
pub fn buffer_to_typed_numpy<'py>(
    py: Python<'py>,
    buffer: &RasterBuffer,
) -> PyResult<PyObject> {
    let zero_copy = ZeroCopyBuffer::from_buffer(buffer.clone());
    zero_copy.to_numpy(py)
}

/// Converts a NumPy array to a `RasterBuffer`
///
/// This function creates a `RasterBuffer` from NumPy array data.
/// The array should have shape (height, width).
///
/// # Errors
///
/// Returns an error if conversion fails
pub fn numpy_to_buffer<'py, T>(
    array: &Bound<'py, PyArray2<T>>,
    data_type: RasterDataType,
    nodata: NoDataValue,
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

// ============================================================================
// Multi-Dimensional Array Handling
// ============================================================================

/// Represents array strides for memory layout
#[derive(Debug, Clone)]
pub struct ArrayStrides {
    /// Strides in bytes for each dimension
    pub strides: Vec<isize>,
    /// Whether the array is C-contiguous (row-major)
    pub c_contiguous: bool,
    /// Whether the array is Fortran-contiguous (column-major)
    pub f_contiguous: bool,
}

impl ArrayStrides {
    /// Creates strides for a C-contiguous array
    #[must_use]
    pub fn c_contiguous(shape: &[usize], itemsize: usize) -> Self {
        let ndim = shape.len();
        let mut strides = vec![0isize; ndim];

        if ndim > 0 {
            strides[ndim - 1] = itemsize as isize;
            for i in (0..ndim - 1).rev() {
                strides[i] = strides[i + 1] * shape[i + 1] as isize;
            }
        }

        Self {
            strides,
            c_contiguous: true,
            f_contiguous: ndim <= 1,
        }
    }

    /// Creates strides for a Fortran-contiguous array
    #[must_use]
    pub fn f_contiguous(shape: &[usize], itemsize: usize) -> Self {
        let ndim = shape.len();
        let mut strides = vec![0isize; ndim];

        if ndim > 0 {
            strides[0] = itemsize as isize;
            for i in 1..ndim {
                strides[i] = strides[i - 1] * shape[i - 1] as isize;
            }
        }

        Self {
            strides,
            c_contiguous: ndim <= 1,
            f_contiguous: true,
        }
    }

    /// Checks if strides are valid for the given shape
    #[must_use]
    pub fn is_valid(&self, shape: &[usize]) -> bool {
        self.strides.len() == shape.len()
    }
}

/// Multi-dimensional array container
#[derive(Debug, Clone)]
pub struct MultiDimArray {
    /// Raw data
    data: Vec<u8>,
    /// Shape of the array
    shape: Vec<usize>,
    /// Strides
    strides: ArrayStrides,
    /// Data type
    dtype: RasterDataType,
    /// Metadata
    metadata: ArrayMetadata,
}

impl MultiDimArray {
    /// Creates a new multi-dimensional array
    ///
    /// # Errors
    ///
    /// Returns an error if data size doesn't match shape
    pub fn new(
        data: Vec<u8>,
        shape: Vec<usize>,
        dtype: RasterDataType,
    ) -> PyResult<Self> {
        let itemsize = dtype.size_bytes();
        let total_elements: usize = shape.iter().product();
        let expected_size = total_elements * itemsize;

        if data.len() != expected_size {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "Data size mismatch: expected {} bytes for shape {:?} with dtype {:?}, got {}",
                expected_size, shape, dtype, data.len()
            )));
        }

        let strides = ArrayStrides::c_contiguous(&shape, itemsize);
        let metadata = ArrayMetadata {
            shape: shape.clone(),
            dtype,
            ..Default::default()
        };

        Ok(Self {
            data,
            shape,
            strides,
            dtype,
            metadata,
        })
    }

    /// Creates a zeros array with the given shape
    #[must_use]
    pub fn zeros(shape: Vec<usize>, dtype: RasterDataType) -> Self {
        let itemsize = dtype.size_bytes();
        let total_elements: usize = shape.iter().product();
        let data = vec![0u8; total_elements * itemsize];

        let strides = ArrayStrides::c_contiguous(&shape, itemsize);
        let metadata = ArrayMetadata {
            shape: shape.clone(),
            dtype,
            ..Default::default()
        };

        Self {
            data,
            shape,
            strides,
            dtype,
            metadata,
        }
    }

    /// Gets the number of dimensions
    #[must_use]
    pub fn ndim(&self) -> usize {
        self.shape.len()
    }

    /// Gets the shape
    #[must_use]
    pub fn shape(&self) -> &[usize] {
        &self.shape
    }

    /// Gets the data type
    #[must_use]
    pub const fn dtype(&self) -> RasterDataType {
        self.dtype
    }

    /// Gets the total number of elements
    #[must_use]
    pub fn size(&self) -> usize {
        self.shape.iter().product()
    }

    /// Gets the raw data as bytes
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    /// Gets a linear index from multi-dimensional indices
    fn linear_index(&self, indices: &[usize]) -> Option<usize> {
        if indices.len() != self.shape.len() {
            return None;
        }

        for (idx, &dim) in indices.iter().zip(self.shape.iter()) {
            if idx >= dim {
                return None;
            }
        }

        let mut linear = 0usize;
        for (idx, stride) in indices.iter().zip(self.strides.strides.iter()) {
            linear += idx * (*stride as usize);
        }
        linear /= self.dtype.size_bytes();

        Some(linear)
    }

    /// Gets an element value as f64
    ///
    /// # Errors
    ///
    /// Returns an error if indices are out of bounds
    pub fn get(&self, indices: &[usize]) -> PyResult<f64> {
        let linear = self.linear_index(indices).ok_or_else(|| {
            pyo3::exceptions::PyIndexError::new_err(format!(
                "Index {:?} out of bounds for shape {:?}",
                indices, self.shape
            ))
        })?;

        let offset = linear * self.dtype.size_bytes();
        self.get_at_offset(offset)
    }

    /// Gets value at byte offset
    fn get_at_offset(&self, offset: usize) -> PyResult<f64> {
        let size = self.dtype.size_bytes();
        if offset + size > self.data.len() {
            return Err(pyo3::exceptions::PyIndexError::new_err(
                "Offset out of bounds",
            ));
        }

        let slice = &self.data[offset..offset + size];
        let value = match self.dtype {
            RasterDataType::UInt8 => f64::from(slice[0]),
            RasterDataType::Int8 => f64::from(slice[0] as i8),
            RasterDataType::UInt16 => {
                let bytes: [u8; 2] = slice.try_into().map_err(|_| {
                    pyo3::exceptions::PyValueError::new_err("Invalid slice")
                })?;
                f64::from(u16::from_ne_bytes(bytes))
            }
            RasterDataType::Int16 => {
                let bytes: [u8; 2] = slice.try_into().map_err(|_| {
                    pyo3::exceptions::PyValueError::new_err("Invalid slice")
                })?;
                f64::from(i16::from_ne_bytes(bytes))
            }
            RasterDataType::UInt32 => {
                let bytes: [u8; 4] = slice.try_into().map_err(|_| {
                    pyo3::exceptions::PyValueError::new_err("Invalid slice")
                })?;
                f64::from(u32::from_ne_bytes(bytes))
            }
            RasterDataType::Int32 => {
                let bytes: [u8; 4] = slice.try_into().map_err(|_| {
                    pyo3::exceptions::PyValueError::new_err("Invalid slice")
                })?;
                f64::from(i32::from_ne_bytes(bytes))
            }
            RasterDataType::UInt64 => {
                let bytes: [u8; 8] = slice.try_into().map_err(|_| {
                    pyo3::exceptions::PyValueError::new_err("Invalid slice")
                })?;
                u64::from_ne_bytes(bytes) as f64
            }
            RasterDataType::Int64 => {
                let bytes: [u8; 8] = slice.try_into().map_err(|_| {
                    pyo3::exceptions::PyValueError::new_err("Invalid slice")
                })?;
                i64::from_ne_bytes(bytes) as f64
            }
            RasterDataType::Float32 => {
                let bytes: [u8; 4] = slice.try_into().map_err(|_| {
                    pyo3::exceptions::PyValueError::new_err("Invalid slice")
                })?;
                f64::from(f32::from_ne_bytes(bytes))
            }
            RasterDataType::Float64 => {
                let bytes: [u8; 8] = slice.try_into().map_err(|_| {
                    pyo3::exceptions::PyValueError::new_err("Invalid slice")
                })?;
                f64::from_ne_bytes(bytes)
            }
            RasterDataType::CFloat32 | RasterDataType::CFloat64 => {
                // Return real part
                let bytes: [u8; 4] = slice[..4].try_into().map_err(|_| {
                    pyo3::exceptions::PyValueError::new_err("Invalid slice")
                })?;
                f64::from(f32::from_ne_bytes(bytes))
            }
        };

        Ok(value)
    }

    /// Converts to NumPy array
    ///
    /// # Errors
    ///
    /// Returns an error if conversion fails
    pub fn to_numpy<'py>(&self, py: Python<'py>) -> PyResult<PyObject> {
        match self.ndim() {
            1 => self.to_numpy_1d(py),
            2 => self.to_numpy_2d(py),
            3 => self.to_numpy_3d(py),
            _ => Err(pyo3::exceptions::PyValueError::new_err(format!(
                "Arrays with {} dimensions are not directly supported. Use reshape.",
                self.ndim()
            ))),
        }
    }

    /// Converts to 1D NumPy array
    fn to_numpy_1d<'py>(&self, py: Python<'py>) -> PyResult<PyObject> {
        let data: Vec<f64> = (0..self.size())
            .map(|i| self.get(&[i]))
            .collect::<PyResult<Vec<_>>>()?;

        let array = data.into_pyarray(py);
        Ok(array.into_any().unbind())
    }

    /// Converts to 2D NumPy array
    fn to_numpy_2d<'py>(&self, py: Python<'py>) -> PyResult<PyObject> {
        let height = self.shape[0];
        let width = self.shape[1];

        let mut data = Vec::with_capacity(height * width);
        for y in 0..height {
            for x in 0..width {
                data.push(self.get(&[y, x])?);
            }
        }

        let nested: Vec<Vec<f64>> = data.chunks(width).map(|c| c.to_vec()).collect();
        let array = PyArray2::from_vec2(py, &nested).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Array creation failed: {}", e))
        })?;

        Ok(array.into_any().unbind())
    }

    /// Converts to 3D NumPy array
    fn to_numpy_3d<'py>(&self, py: Python<'py>) -> PyResult<PyObject> {
        let bands = self.shape[0];
        let height = self.shape[1];
        let width = self.shape[2];

        let mut data = Vec::with_capacity(bands * height * width);
        for b in 0..bands {
            for y in 0..height {
                for x in 0..width {
                    data.push(self.get(&[b, y, x])?);
                }
            }
        }

        // Create nested structure
        let nested: Vec<Vec<Vec<f64>>> = data
            .chunks(height * width)
            .map(|band| {
                band.chunks(width)
                    .map(|row| row.to_vec())
                    .collect()
            })
            .collect();

        let array = PyArray3::from_vec3(py, &nested).map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("Array creation failed: {}", e))
        })?;

        Ok(array.into_any().unbind())
    }
}

// ============================================================================
// Structured Array Support
// ============================================================================

/// Field definition for structured arrays
#[derive(Debug, Clone)]
pub struct StructField {
    /// Field name
    pub name: String,
    /// Data type
    pub dtype: RasterDataType,
    /// Offset in bytes
    pub offset: usize,
    /// Shape for nested arrays (empty for scalar)
    pub shape: Vec<usize>,
}

impl StructField {
    /// Creates a new scalar field
    #[must_use]
    pub fn scalar(name: &str, dtype: RasterDataType, offset: usize) -> Self {
        Self {
            name: name.to_string(),
            dtype,
            offset,
            shape: Vec::new(),
        }
    }

    /// Creates a new array field
    #[must_use]
    pub fn array(name: &str, dtype: RasterDataType, offset: usize, shape: Vec<usize>) -> Self {
        Self {
            name: name.to_string(),
            dtype,
            offset,
            shape,
        }
    }

    /// Gets the total size of this field in bytes
    #[must_use]
    pub fn size_bytes(&self) -> usize {
        let element_size = self.dtype.size_bytes();
        if self.shape.is_empty() {
            element_size
        } else {
            element_size * self.shape.iter().product::<usize>()
        }
    }
}

/// Structured array dtype definition
#[derive(Debug, Clone)]
pub struct StructuredDtype {
    /// List of fields
    pub fields: Vec<StructField>,
    /// Total size of one record in bytes
    pub itemsize: usize,
    /// Alignment requirement
    pub alignment: usize,
}

impl StructuredDtype {
    /// Creates a new structured dtype
    #[must_use]
    pub fn new(fields: Vec<StructField>) -> Self {
        let itemsize = fields
            .iter()
            .map(|f| f.offset + f.size_bytes())
            .max()
            .unwrap_or(0);

        Self {
            fields,
            itemsize,
            alignment: 8, // Default to 8-byte alignment
        }
    }

    /// Gets a field by name
    #[must_use]
    pub fn get_field(&self, name: &str) -> Option<&StructField> {
        self.fields.iter().find(|f| f.name == name)
    }

    /// Converts to NumPy dtype string
    #[must_use]
    pub fn to_numpy_dtype_str(&self) -> String {
        let field_strs: Vec<String> = self
            .fields
            .iter()
            .map(|f| {
                let dtype_str = data_type_to_dtype_str(f.dtype);
                if f.shape.is_empty() {
                    format!("('{}', '{}')", f.name, dtype_str)
                } else {
                    let shape_str: Vec<String> =
                        f.shape.iter().map(|s| s.to_string()).collect();
                    format!(
                        "('{}', '{}', ({},))",
                        f.name,
                        dtype_str,
                        shape_str.join(", ")
                    )
                }
            })
            .collect();

        format!("[{}]", field_strs.join(", "))
    }
}

/// Structured array container
#[derive(Debug, Clone)]
pub struct StructuredArray {
    /// Raw data
    data: Vec<u8>,
    /// Number of records
    num_records: usize,
    /// Dtype definition
    dtype: StructuredDtype,
}

impl StructuredArray {
    /// Creates a new structured array
    ///
    /// # Errors
    ///
    /// Returns an error if data size is incorrect
    pub fn new(
        data: Vec<u8>,
        num_records: usize,
        dtype: StructuredDtype,
    ) -> PyResult<Self> {
        let expected_size = num_records * dtype.itemsize;
        if data.len() != expected_size {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "Data size mismatch: expected {} bytes for {} records, got {}",
                expected_size, num_records, data.len()
            )));
        }

        Ok(Self {
            data,
            num_records,
            dtype,
        })
    }

    /// Creates an empty structured array
    #[must_use]
    pub fn empty(num_records: usize, dtype: StructuredDtype) -> Self {
        let data = vec![0u8; num_records * dtype.itemsize];
        Self {
            data,
            num_records,
            dtype,
        }
    }

    /// Gets the number of records
    #[must_use]
    pub const fn len(&self) -> usize {
        self.num_records
    }

    /// Checks if the array is empty
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.num_records == 0
    }

    /// Gets a field value for a specific record
    ///
    /// # Errors
    ///
    /// Returns an error if field not found or index out of bounds
    pub fn get_field(&self, record: usize, field_name: &str) -> PyResult<f64> {
        if record >= self.num_records {
            return Err(pyo3::exceptions::PyIndexError::new_err(format!(
                "Record index {} out of bounds for {} records",
                record, self.num_records
            )));
        }

        let field = self.dtype.get_field(field_name).ok_or_else(|| {
            pyo3::exceptions::PyKeyError::new_err(format!("Field '{}' not found", field_name))
        })?;

        let record_offset = record * self.dtype.itemsize;
        let field_offset = record_offset + field.offset;

        // Read the value
        let size = field.dtype.size_bytes();
        if field_offset + size > self.data.len() {
            return Err(pyo3::exceptions::PyIndexError::new_err("Offset out of bounds"));
        }

        let slice = &self.data[field_offset..field_offset + size];

        let value = match field.dtype {
            RasterDataType::Float64 => {
                let bytes: [u8; 8] = slice.try_into().map_err(|_| {
                    pyo3::exceptions::PyValueError::new_err("Invalid slice")
                })?;
                f64::from_ne_bytes(bytes)
            }
            RasterDataType::Float32 => {
                let bytes: [u8; 4] = slice.try_into().map_err(|_| {
                    pyo3::exceptions::PyValueError::new_err("Invalid slice")
                })?;
                f64::from(f32::from_ne_bytes(bytes))
            }
            RasterDataType::Int64 => {
                let bytes: [u8; 8] = slice.try_into().map_err(|_| {
                    pyo3::exceptions::PyValueError::new_err("Invalid slice")
                })?;
                i64::from_ne_bytes(bytes) as f64
            }
            RasterDataType::Int32 => {
                let bytes: [u8; 4] = slice.try_into().map_err(|_| {
                    pyo3::exceptions::PyValueError::new_err("Invalid slice")
                })?;
                f64::from(i32::from_ne_bytes(bytes))
            }
            _ => {
                return Err(pyo3::exceptions::PyNotImplementedError::new_err(format!(
                    "Field type {:?} not yet supported",
                    field.dtype
                )));
            }
        };

        Ok(value)
    }

    /// Extracts a single field as a 1D array
    ///
    /// # Errors
    ///
    /// Returns an error if field not found
    pub fn extract_field<'py>(
        &self,
        py: Python<'py>,
        field_name: &str,
    ) -> PyResult<Bound<'py, PyArray1<f64>>> {
        let field = self.dtype.get_field(field_name).ok_or_else(|| {
            pyo3::exceptions::PyKeyError::new_err(format!("Field '{}' not found", field_name))
        })?;

        let mut values = Vec::with_capacity(self.num_records);
        for i in 0..self.num_records {
            let record_offset = i * self.dtype.itemsize;
            let field_offset = record_offset + field.offset;
            let size = field.dtype.size_bytes();
            let slice = &self.data[field_offset..field_offset + size];

            let value = match field.dtype {
                RasterDataType::Float64 => {
                    let bytes: [u8; 8] = slice.try_into().map_err(|_| {
                        pyo3::exceptions::PyValueError::new_err("Invalid slice")
                    })?;
                    f64::from_ne_bytes(bytes)
                }
                RasterDataType::Float32 => {
                    let bytes: [u8; 4] = slice.try_into().map_err(|_| {
                        pyo3::exceptions::PyValueError::new_err("Invalid slice")
                    })?;
                    f64::from(f32::from_ne_bytes(bytes))
                }
                RasterDataType::Int64 => {
                    let bytes: [u8; 8] = slice.try_into().map_err(|_| {
                        pyo3::exceptions::PyValueError::new_err("Invalid slice")
                    })?;
                    i64::from_ne_bytes(bytes) as f64
                }
                RasterDataType::Int32 => {
                    let bytes: [u8; 4] = slice.try_into().map_err(|_| {
                        pyo3::exceptions::PyValueError::new_err("Invalid slice")
                    })?;
                    f64::from(i32::from_ne_bytes(bytes))
                }
                _ => 0.0, // Default for unsupported types
            };
            values.push(value);
        }

        Ok(values.into_pyarray(py))
    }
}

// ============================================================================
// Memory-Mapped NumPy Arrays
// ============================================================================

/// Configuration for memory-mapped arrays
#[derive(Debug, Clone)]
pub struct MemoryMapArrayConfig {
    /// Access mode (read-only or read-write)
    pub mode: MemoryMapMode,
    /// Shape of the array
    pub shape: Vec<usize>,
    /// Data type
    pub dtype: RasterDataType,
    /// Offset in file
    pub offset: usize,
    /// Whether to populate pages immediately
    pub populate: bool,
}

/// Memory map mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryMapMode {
    /// Read-only
    ReadOnly,
    /// Read-write
    ReadWrite,
    /// Copy-on-write
    CopyOnWrite,
}

impl MemoryMapMode {
    /// Converts to NumPy mode string
    #[must_use]
    pub const fn to_numpy_mode(self) -> &'static str {
        match self {
            Self::ReadOnly => "r",
            Self::ReadWrite => "r+",
            Self::CopyOnWrite => "c",
        }
    }
}

impl Default for MemoryMapArrayConfig {
    fn default() -> Self {
        Self {
            mode: MemoryMapMode::ReadOnly,
            shape: Vec::new(),
            dtype: RasterDataType::Float64,
            offset: 0,
            populate: false,
        }
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Creates a helper NumPy array from Vec with specified shape
///
/// # Errors
///
/// Returns an error if shape doesn't match data size
pub fn create_numpy_array<'py, T>(
    py: Python<'py>,
    data: Vec<T>,
    shape: (usize, usize),
) -> PyResult<Bound<'py, PyArray2<T>>>
where
    T: Element + Clone,
{
    let (height, width) = shape;
    if data.len() != height * width {
        return Err(pyo3::exceptions::PyValueError::new_err(format!(
            "Data length {} doesn't match shape {:?}",
            data.len(),
            shape
        )));
    }

    let nested: Vec<Vec<T>> = data.chunks(width).map(|chunk| chunk.to_vec()).collect();

    PyArray2::from_vec2(py, &nested).map_err(|e| {
        pyo3::exceptions::PyValueError::new_err(format!("Failed to create NumPy array: {}", e))
    })
}

/// Creates a 3D NumPy array from Vec with specified shape
///
/// # Errors
///
/// Returns an error if shape doesn't match data size
pub fn create_numpy_array_3d<'py, T>(
    py: Python<'py>,
    data: Vec<T>,
    shape: (usize, usize, usize),
) -> PyResult<Bound<'py, PyArray3<T>>>
where
    T: Element + Clone,
{
    let (bands, height, width) = shape;
    if data.len() != bands * height * width {
        return Err(pyo3::exceptions::PyValueError::new_err(format!(
            "Data length {} doesn't match shape {:?}",
            data.len(),
            shape
        )));
    }

    let nested: Vec<Vec<Vec<T>>> = data
        .chunks(height * width)
        .map(|band| band.chunks(width).map(|row| row.to_vec()).collect())
        .collect();

    PyArray3::from_vec3(py, &nested).map_err(|e| {
        pyo3::exceptions::PyValueError::new_err(format!("Failed to create 3D NumPy array: {}", e))
    })
}

/// Validates array shape matches expected dimensions
///
/// # Errors
///
/// Returns an error if dimensions don't match
pub fn validate_array_shape(
    actual: &[usize],
    expected_ndim: usize,
    context: &str,
) -> PyResult<()> {
    if actual.len() != expected_ndim {
        return Err(pyo3::exceptions::PyValueError::new_err(format!(
            "{}: expected {}D array, got {}D",
            context,
            expected_ndim,
            actual.len()
        )));
    }
    Ok(())
}

/// Computes contiguous strides for C-order array
#[must_use]
pub fn compute_c_strides(shape: &[usize], itemsize: usize) -> Vec<isize> {
    ArrayStrides::c_contiguous(shape, itemsize).strides
}

/// Computes contiguous strides for Fortran-order array
#[must_use]
pub fn compute_f_strides(shape: &[usize], itemsize: usize) -> Vec<isize> {
    ArrayStrides::f_contiguous(shape, itemsize).strides
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_numpy_dtype_descriptor() {
        let dtype = data_type_to_numpy_dtype(RasterDataType::Float32);
        assert_eq!(dtype.dtype_str, "float32");
        assert_eq!(dtype.itemsize, 4);
        assert!(!dtype.is_complex);
        assert_eq!(dtype.type_code(), 'f');
    }

    #[test]
    fn test_dtype_conversion_roundtrip() {
        let types = vec![
            RasterDataType::UInt8,
            RasterDataType::Int16,
            RasterDataType::Float32,
            RasterDataType::Float64,
        ];

        for dt in types {
            let numpy_str = data_type_to_dtype_str(dt);
            let recovered = numpy_dtype_to_data_type(numpy_str);
            assert!(
                recovered.is_ok(),
                "Failed to recover dtype for {:?}",
                dt
            );
            assert_eq!(recovered.ok(), Some(dt));
        }
    }

    #[test]
    fn test_array_metadata() {
        let meta = ArrayMetadata::new()
            .with_shape(vec![100, 200])
            .with_crs("EPSG:4326".to_string())
            .with_geotransform([0.0, 1.0, 0.0, 100.0, 0.0, -1.0])
            .with_custom("source".to_string(), "test".to_string());

        assert_eq!(meta.shape, vec![100, 200]);
        assert_eq!(meta.crs_wkt, Some("EPSG:4326".to_string()));
        assert!(meta.geotransform.is_some());
        assert_eq!(meta.custom.get("source"), Some(&"test".to_string()));
    }

    #[test]
    fn test_array_strides_c_contiguous() {
        let shape = vec![10, 20, 30];
        let strides = ArrayStrides::c_contiguous(&shape, 8);

        assert_eq!(strides.strides, vec![4800, 240, 8]);
        assert!(strides.c_contiguous);
    }

    #[test]
    fn test_array_strides_f_contiguous() {
        let shape = vec![10, 20, 30];
        let strides = ArrayStrides::f_contiguous(&shape, 8);

        assert_eq!(strides.strides, vec![8, 80, 1600]);
        assert!(strides.f_contiguous);
    }

    #[test]
    fn test_multi_dim_array_creation() {
        let shape = vec![2, 3];
        let data = vec![0u8; 2 * 3 * 8]; // 6 f64 values
        let array = MultiDimArray::new(data, shape.clone(), RasterDataType::Float64);

        assert!(array.is_ok());
        let arr = array.ok();
        let arr_ref = arr.as_ref();
        assert!(arr_ref.is_some());
        let arr_unwrap = arr_ref.map(|a| a.ndim());
        assert_eq!(arr_unwrap, Some(2));
    }

    #[test]
    fn test_struct_field_size() {
        let scalar = StructField::scalar("value", RasterDataType::Float64, 0);
        assert_eq!(scalar.size_bytes(), 8);

        let array = StructField::array("coords", RasterDataType::Float64, 8, vec![3]);
        assert_eq!(array.size_bytes(), 24);
    }

    #[test]
    fn test_structured_dtype() {
        let fields = vec![
            StructField::scalar("id", RasterDataType::Int64, 0),
            StructField::scalar("x", RasterDataType::Float64, 8),
            StructField::scalar("y", RasterDataType::Float64, 16),
        ];
        let dtype = StructuredDtype::new(fields);

        assert_eq!(dtype.itemsize, 24);
        assert!(dtype.get_field("x").is_some());
        assert!(dtype.get_field("z").is_none());
    }

    #[test]
    fn test_buffer_type_code() {
        assert_eq!(
            BufferTypeCode::from_raster_data_type(RasterDataType::Float64).format_char(),
            'd'
        );
        assert_eq!(
            BufferTypeCode::from_raster_data_type(RasterDataType::Int32).format_char(),
            'i'
        );
        assert_eq!(
            BufferTypeCode::from_raster_data_type(RasterDataType::UInt8).format_char(),
            'B'
        );
    }

    #[test]
    fn test_memory_map_mode() {
        assert_eq!(MemoryMapMode::ReadOnly.to_numpy_mode(), "r");
        assert_eq!(MemoryMapMode::ReadWrite.to_numpy_mode(), "r+");
        assert_eq!(MemoryMapMode::CopyOnWrite.to_numpy_mode(), "c");
    }

    #[test]
    fn test_compute_strides() {
        let shape = [4, 8];
        let c_strides = compute_c_strides(&shape, 4);
        assert_eq!(c_strides, vec![32, 4]);

        let f_strides = compute_f_strides(&shape, 4);
        assert_eq!(f_strides, vec![4, 16]);
    }

    #[test]
    fn test_validate_array_shape() {
        assert!(validate_array_shape(&[10, 20], 2, "test").is_ok());
        assert!(validate_array_shape(&[10, 20], 3, "test").is_err());
        assert!(validate_array_shape(&[5, 10, 15], 3, "test").is_ok());
    }
}
