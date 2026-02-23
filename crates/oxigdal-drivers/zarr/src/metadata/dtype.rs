//! Data type definitions for Zarr arrays
//!
//! This module provides data type representations compatible with both
//! Zarr v2 (NumPy dtype strings) and v3 (data type objects).

use super::{ByteOrder, MetadataError};
use crate::error::{Result, ZarrError};
use serde::{Deserialize, Serialize};

/// Data type for Zarr arrays
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DType {
    /// Simple dtype string (v2 style: e.g., "<f4", ">i8")
    String(String),
    /// Structured dtype object (v3 style)
    Object(DTypeObject),
}

impl DType {
    /// Creates a dtype from a NumPy dtype string
    ///
    /// # Errors
    /// Returns error if the dtype string is invalid
    pub fn from_numpy_str(s: &str) -> Result<Self> {
        // Validate the dtype string format
        if s.is_empty() {
            return Err(ZarrError::Metadata(MetadataError::UnsupportedDataType {
                dtype: s.to_string(),
            }));
        }

        // Parse byte order
        let first_char = s.chars().next().ok_or_else(|| {
            ZarrError::Metadata(MetadataError::UnsupportedDataType {
                dtype: s.to_string(),
            })
        })?;

        ByteOrder::from_char(first_char)?;

        Ok(Self::String(s.to_string()))
    }

    /// Returns the size in bytes of a single element
    ///
    /// # Errors
    /// Returns error if dtype is invalid or size cannot be determined
    pub fn element_size(&self) -> Result<usize> {
        match self {
            Self::String(s) => parse_numpy_dtype_size(s),
            Self::Object(obj) => obj.element_size(),
        }
    }

    /// Returns the byte order
    ///
    /// # Errors
    /// Returns error if byte order cannot be determined
    pub fn byte_order(&self) -> Result<ByteOrder> {
        match self {
            Self::String(s) => {
                let first_char = s.chars().next().ok_or_else(|| {
                    ZarrError::Metadata(MetadataError::UnsupportedDataType {
                        dtype: s.to_string(),
                    })
                })?;
                ByteOrder::from_char(first_char)
            }
            Self::Object(_) => Ok(ByteOrder::native()),
        }
    }

    /// Returns true if this is a floating-point type
    #[must_use]
    pub fn is_float(&self) -> bool {
        match self {
            Self::String(s) => s.contains('f') || s.contains("float"),
            Self::Object(obj) => obj.is_float(),
        }
    }

    /// Returns true if this is an integer type
    #[must_use]
    pub fn is_int(&self) -> bool {
        match self {
            Self::String(s) => s.contains('i') || s.contains('u') || s.contains("int"),
            Self::Object(obj) => obj.is_int(),
        }
    }

    /// Returns true if this is a complex type
    #[must_use]
    pub fn is_complex(&self) -> bool {
        match self {
            Self::String(s) => s.contains('c') || s.contains("complex"),
            Self::Object(obj) => obj.is_complex(),
        }
    }

    /// Converts to a NumPy dtype string
    #[must_use]
    pub fn to_numpy_str(&self) -> String {
        match self {
            Self::String(s) => s.clone(),
            Self::Object(obj) => obj.to_numpy_str(),
        }
    }
}

/// Data type object (Zarr v3 style)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DTypeObject {
    /// Data type name
    pub name: String,
    /// Size in bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<usize>,
    /// Configuration
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Map<String, serde_json::Value>>,
}

impl DTypeObject {
    /// Creates a new data type object
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            size: None,
            config: None,
        }
    }

    /// Sets the size
    #[must_use]
    pub fn with_size(mut self, size: usize) -> Self {
        self.size = Some(size);
        self
    }

    /// Returns the element size in bytes
    ///
    /// # Errors
    /// Returns error if size cannot be determined
    pub fn element_size(&self) -> Result<usize> {
        if let Some(size) = self.size {
            return Ok(size);
        }

        // Try to infer from name
        match self.name.as_str() {
            "int8" | "uint8" | "bool" => Ok(1),
            "int16" | "uint16" => Ok(2),
            "int32" | "uint32" | "float32" => Ok(4),
            "int64" | "uint64" | "float64" => Ok(8),
            "complex64" => Ok(8),
            "complex128" => Ok(16),
            _ => Err(ZarrError::Metadata(MetadataError::UnsupportedDataType {
                dtype: self.name.clone(),
            })),
        }
    }

    /// Returns true if this is a floating-point type
    #[must_use]
    pub fn is_float(&self) -> bool {
        self.name.contains("float")
    }

    /// Returns true if this is an integer type
    #[must_use]
    pub fn is_int(&self) -> bool {
        self.name.contains("int")
    }

    /// Returns true if this is a complex type
    #[must_use]
    pub fn is_complex(&self) -> bool {
        self.name.contains("complex")
    }

    /// Converts to a NumPy dtype string
    #[must_use]
    pub fn to_numpy_str(&self) -> String {
        let byte_order = ByteOrder::native().as_char();
        let size = self.element_size().ok();

        match self.name.as_str() {
            "int8" => "|i1".to_string(),
            "int16" => format!("{byte_order}i2"),
            "int32" => format!("{byte_order}i4"),
            "int64" => format!("{byte_order}i8"),
            "uint8" => "|u1".to_string(),
            "uint16" => format!("{byte_order}u2"),
            "uint32" => format!("{byte_order}u4"),
            "uint64" => format!("{byte_order}u8"),
            "float32" => format!("{byte_order}f4"),
            "float64" => format!("{byte_order}f8"),
            "complex64" => format!("{byte_order}c8"),
            "complex128" => format!("{byte_order}c16"),
            "bool" => "|b1".to_string(),
            _ => {
                if let Some(s) = size {
                    format!("{byte_order}V{s}")
                } else {
                    format!("{byte_order}V")
                }
            }
        }
    }
}

/// Parses the size from a NumPy dtype string
fn parse_numpy_dtype_size(dtype: &str) -> Result<usize> {
    if dtype.len() < 2 {
        return Err(ZarrError::Metadata(MetadataError::UnsupportedDataType {
            dtype: dtype.to_string(),
        }));
    }

    let type_char = dtype.chars().nth(1).ok_or_else(|| {
        ZarrError::Metadata(MetadataError::UnsupportedDataType {
            dtype: dtype.to_string(),
        })
    })?;

    let size_str = &dtype[2..];

    match type_char {
        'b' => Ok(1), // bool
        'i' | 'u' | 'f' | 'c' => size_str.parse::<usize>().map_err(|_| {
            ZarrError::Metadata(MetadataError::UnsupportedDataType {
                dtype: dtype.to_string(),
            })
        }),
        'S' | 'U' | 'V' => {
            // String, Unicode, or raw bytes
            if size_str.is_empty() {
                Ok(0)
            } else {
                size_str.parse::<usize>().map_err(|_| {
                    ZarrError::Metadata(MetadataError::UnsupportedDataType {
                        dtype: dtype.to_string(),
                    })
                })
            }
        }
        _ => Err(ZarrError::Metadata(MetadataError::UnsupportedDataType {
            dtype: dtype.to_string(),
        })),
    }
}

/// Common data type constructors
impl DType {
    /// Creates a bool dtype
    #[must_use]
    pub fn bool() -> Self {
        Self::String("|b1".to_string())
    }

    /// Creates an int8 dtype
    #[must_use]
    pub fn int8() -> Self {
        Self::String("|i1".to_string())
    }

    /// Creates an int16 dtype
    #[must_use]
    pub fn int16() -> Self {
        let byte_order = ByteOrder::native().as_char();
        Self::String(format!("{byte_order}i2"))
    }

    /// Creates an int32 dtype
    #[must_use]
    pub fn int32() -> Self {
        let byte_order = ByteOrder::native().as_char();
        Self::String(format!("{byte_order}i4"))
    }

    /// Creates an int64 dtype
    #[must_use]
    pub fn int64() -> Self {
        let byte_order = ByteOrder::native().as_char();
        Self::String(format!("{byte_order}i8"))
    }

    /// Creates a uint8 dtype
    #[must_use]
    pub fn uint8() -> Self {
        Self::String("|u1".to_string())
    }

    /// Creates a uint16 dtype
    #[must_use]
    pub fn uint16() -> Self {
        let byte_order = ByteOrder::native().as_char();
        Self::String(format!("{byte_order}u2"))
    }

    /// Creates a uint32 dtype
    #[must_use]
    pub fn uint32() -> Self {
        let byte_order = ByteOrder::native().as_char();
        Self::String(format!("{byte_order}u4"))
    }

    /// Creates a uint64 dtype
    #[must_use]
    pub fn uint64() -> Self {
        let byte_order = ByteOrder::native().as_char();
        Self::String(format!("{byte_order}u8"))
    }

    /// Creates a float32 dtype
    #[must_use]
    pub fn float32() -> Self {
        let byte_order = ByteOrder::native().as_char();
        Self::String(format!("{byte_order}f4"))
    }

    /// Creates a float64 dtype
    #[must_use]
    pub fn float64() -> Self {
        let byte_order = ByteOrder::native().as_char();
        Self::String(format!("{byte_order}f8"))
    }

    /// Creates a complex64 dtype
    #[must_use]
    pub fn complex64() -> Self {
        let byte_order = ByteOrder::native().as_char();
        Self::String(format!("{byte_order}c8"))
    }

    /// Creates a complex128 dtype
    #[must_use]
    pub fn complex128() -> Self {
        let byte_order = ByteOrder::native().as_char();
        Self::String(format!("{byte_order}c16"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dtype_from_numpy_str() {
        assert!(DType::from_numpy_str("<f4").is_ok());
        assert!(DType::from_numpy_str(">i8").is_ok());
        assert!(DType::from_numpy_str("|u1").is_ok());
        assert!(DType::from_numpy_str("").is_err());
    }

    #[test]
    fn test_dtype_element_size() {
        assert_eq!(
            DType::from_numpy_str("<f4")
                .expect("ok")
                .element_size()
                .expect("size"),
            4
        );
        assert_eq!(
            DType::from_numpy_str(">i8")
                .expect("ok")
                .element_size()
                .expect("size"),
            8
        );
        assert_eq!(
            DType::from_numpy_str("|u1")
                .expect("ok")
                .element_size()
                .expect("size"),
            1
        );
        assert_eq!(
            DType::from_numpy_str("<c8")
                .expect("ok")
                .element_size()
                .expect("size"),
            8
        );
    }

    #[test]
    fn test_dtype_byte_order() {
        assert_eq!(
            DType::from_numpy_str("<f4")
                .expect("ok")
                .byte_order()
                .expect("order"),
            ByteOrder::Little
        );
        assert_eq!(
            DType::from_numpy_str(">i8")
                .expect("ok")
                .byte_order()
                .expect("order"),
            ByteOrder::Big
        );
        assert_eq!(
            DType::from_numpy_str("|u1")
                .expect("ok")
                .byte_order()
                .expect("order"),
            ByteOrder::NotApplicable
        );
    }

    #[test]
    fn test_dtype_type_checks() {
        let float_dtype = DType::from_numpy_str("<f4").expect("ok");
        assert!(float_dtype.is_float());
        assert!(!float_dtype.is_int());
        assert!(!float_dtype.is_complex());

        let int_dtype = DType::from_numpy_str(">i8").expect("ok");
        assert!(!int_dtype.is_float());
        assert!(int_dtype.is_int());
        assert!(!int_dtype.is_complex());

        let complex_dtype = DType::from_numpy_str("<c16").expect("ok");
        assert!(!complex_dtype.is_float());
        assert!(!complex_dtype.is_int());
        assert!(complex_dtype.is_complex());
    }

    #[test]
    fn test_dtype_object() {
        let obj = DTypeObject::new("float32").with_size(4);
        assert_eq!(obj.element_size().expect("size"), 4);
        assert!(obj.is_float());
        assert!(!obj.is_int());

        let int_obj = DTypeObject::new("int64");
        assert_eq!(int_obj.element_size().expect("size"), 8);
        assert!(!int_obj.is_float());
        assert!(int_obj.is_int());
    }

    #[test]
    fn test_dtype_constructors() {
        assert_eq!(DType::bool().element_size().expect("size"), 1);
        assert_eq!(DType::int8().element_size().expect("size"), 1);
        assert_eq!(DType::int16().element_size().expect("size"), 2);
        assert_eq!(DType::int32().element_size().expect("size"), 4);
        assert_eq!(DType::int64().element_size().expect("size"), 8);
        assert_eq!(DType::uint8().element_size().expect("size"), 1);
        assert_eq!(DType::uint16().element_size().expect("size"), 2);
        assert_eq!(DType::uint32().element_size().expect("size"), 4);
        assert_eq!(DType::uint64().element_size().expect("size"), 8);
        assert_eq!(DType::float32().element_size().expect("size"), 4);
        assert_eq!(DType::float64().element_size().expect("size"), 8);
        assert_eq!(DType::complex64().element_size().expect("size"), 8);
        assert_eq!(DType::complex128().element_size().expect("size"), 16);
    }

    #[test]
    fn test_dtype_object_to_numpy_str() {
        let obj = DTypeObject::new("float32");
        let numpy_str = obj.to_numpy_str();
        assert!(numpy_str.contains("f4"));

        let int_obj = DTypeObject::new("int64");
        let numpy_str2 = int_obj.to_numpy_str();
        assert!(numpy_str2.contains("i8"));
    }
}
