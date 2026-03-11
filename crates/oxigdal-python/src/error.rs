//! Error handling for Python bindings
//!
//! This module provides conversion from Rust OxiGdalError to Python exceptions.

use oxigdal_core::error::{CompressionError, CrsError, FormatError, IoError, OxiGdalError};
use pyo3::exceptions::{
    PyIOError, PyNotImplementedError, PyRuntimeError, PyTypeError, PyValueError,
};
use pyo3::prelude::*;

/// Python exception type for OxiGDAL errors
#[pyclass(name = "OxiGdalError")]
#[derive(Debug, Clone)]
pub struct OxiGdalPyError {
    #[pyo3(get, set)]
    pub message: String,
}

impl OxiGdalPyError {
    /// Creates a new Python error with the given message
    pub fn new(message: String) -> Self {
        Self { message }
    }
}

#[pymethods]
impl OxiGdalPyError {
    #[new]
    fn py_new(message: String) -> Self {
        Self::new(message)
    }

    fn __str__(&self) -> String {
        self.message.clone()
    }

    fn __repr__(&self) -> String {
        format!("OxiGdalError('{}')", self.message)
    }
}

/// Converts OxiGdalError to appropriate Python exception
pub fn oxigdal_error_to_py_err(err: OxiGdalError) -> PyErr {
    match err {
        OxiGdalError::Io(io_err) => convert_io_error(io_err),
        OxiGdalError::Format(format_err) => convert_format_error(format_err),
        OxiGdalError::Crs(crs_err) => convert_crs_error(crs_err),
        OxiGdalError::Compression(comp_err) => convert_compression_error(comp_err),
        OxiGdalError::InvalidParameter { parameter, message } => {
            PyValueError::new_err(format!("Invalid parameter '{}': {}", parameter, message))
        }
        OxiGdalError::NotSupported { operation } => {
            PyNotImplementedError::new_err(format!("Not supported: {}", operation))
        }
        OxiGdalError::OutOfBounds { message } => {
            PyValueError::new_err(format!("Out of bounds: {}", message))
        }
        OxiGdalError::Internal { message } => {
            PyRuntimeError::new_err(format!("Internal error: {}", message))
        }
    }
}

/// Converts IoError to Python exception
fn convert_io_error(err: IoError) -> PyErr {
    match err {
        IoError::NotFound { path } => PyIOError::new_err(format!("File not found: {}", path)),
        IoError::PermissionDenied { path } => {
            PyIOError::new_err(format!("Permission denied: {}", path))
        }
        IoError::Network { message } => PyIOError::new_err(format!("Network error: {}", message)),
        IoError::UnexpectedEof { offset } => {
            PyIOError::new_err(format!("Unexpected end of file at offset {}", offset))
        }
        IoError::Read { message } => PyIOError::new_err(format!("Read error: {}", message)),
        IoError::Write { message } => PyIOError::new_err(format!("Write error: {}", message)),
        IoError::Seek { position } => {
            PyIOError::new_err(format!("Seek error at position {}", position))
        }
        IoError::Http { status, message } => {
            PyIOError::new_err(format!("HTTP error {}: {}", status, message))
        }
    }
}

/// Converts FormatError to Python exception
fn convert_format_error(err: FormatError) -> PyErr {
    match err {
        FormatError::InvalidMagic { expected, actual } => PyValueError::new_err(format!(
            "Invalid magic number: expected {:?}, got {:?}",
            expected, actual
        )),
        FormatError::InvalidHeader { message } => {
            PyValueError::new_err(format!("Invalid header: {}", message))
        }
        FormatError::UnsupportedVersion { version } => {
            PyValueError::new_err(format!("Unsupported version: {}", version))
        }
        FormatError::InvalidTag { tag, message } => {
            PyValueError::new_err(format!("Invalid tag {}: {}", tag, message))
        }
        FormatError::MissingTag { tag } => {
            PyValueError::new_err(format!("Missing required tag: {}", tag))
        }
        FormatError::InvalidDataType { type_id } => {
            PyTypeError::new_err(format!("Invalid data type: {}", type_id))
        }
        FormatError::CorruptData { offset, message } => {
            PyValueError::new_err(format!("Corrupt data at offset {}: {}", offset, message))
        }
        FormatError::InvalidGeoKey { key_id, message } => {
            PyValueError::new_err(format!("Invalid GeoKey {}: {}", key_id, message))
        }
    }
}

/// Converts CrsError to Python exception
fn convert_crs_error(err: CrsError) -> PyErr {
    match err {
        CrsError::UnknownCrs { identifier } => {
            PyValueError::new_err(format!("Unknown CRS: {}", identifier))
        }
        CrsError::InvalidWkt { message } => {
            PyValueError::new_err(format!("Invalid WKT: {}", message))
        }
        CrsError::InvalidEpsg { code } => {
            PyValueError::new_err(format!("Invalid EPSG code: {}", code))
        }
        CrsError::TransformationError {
            source_crs,
            target_crs,
            message,
        } => PyRuntimeError::new_err(format!(
            "Transformation error from {} to {}: {}",
            source_crs, target_crs, message
        )),
        CrsError::DatumNotFound { datum } => {
            PyValueError::new_err(format!("Datum not found: {}", datum))
        }
    }
}

/// Converts CompressionError to Python exception
fn convert_compression_error(err: CompressionError) -> PyErr {
    match err {
        CompressionError::UnknownMethod { method } => {
            PyValueError::new_err(format!("Unknown compression method: {}", method))
        }
        CompressionError::DecompressionFailed { message } => {
            PyRuntimeError::new_err(format!("Decompression failed: {}", message))
        }
        CompressionError::CompressionFailed { message } => {
            PyRuntimeError::new_err(format!("Compression failed: {}", message))
        }
        CompressionError::InvalidData { message } => {
            PyValueError::new_err(format!("Invalid compressed data: {}", message))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_conversion() {
        // Initialize Python interpreter so pyo3 APIs are available in tests.
        pyo3::prepare_freethreaded_python();
        let err = OxiGdalError::InvalidParameter {
            parameter: "width",
            message: "must be positive".to_string(),
        };
        let py_err: PyErr = oxigdal_error_to_py_err(err);
        pyo3::Python::with_gil(|_py| {
            assert!(py_err.to_string().contains("width"));
        });
    }

    #[test]
    fn test_oxigdal_py_error() {
        let err = OxiGdalPyError::new("test error".to_string());
        assert_eq!(err.__str__(), "test error");
        assert!(err.__repr__().contains("OxiGdalError"));
    }
}
