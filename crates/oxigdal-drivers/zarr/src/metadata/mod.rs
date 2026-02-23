//! Zarr metadata structures for v2 and v3 specifications
//!
//! This module provides types for representing Zarr array and group metadata
//! according to both v2 and v3 specifications.

pub mod array;
pub mod attrs;
pub mod dtype;
pub mod group;

#[cfg(feature = "v2")]
pub mod v2;

#[cfg(feature = "v3")]
pub mod v3;

use crate::error::{MetadataError, Result, ZarrError};
use serde::{Deserialize, Serialize};

/// Zarr format version
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ZarrFormat {
    /// Zarr format version 2
    #[serde(rename = "2")]
    #[default]
    V2,
    /// Zarr format version 3
    #[serde(rename = "3")]
    V3,
}

impl ZarrFormat {
    /// Returns the version number
    #[must_use]
    pub const fn version(&self) -> u8 {
        match self {
            Self::V2 => 2,
            Self::V3 => 3,
        }
    }

    /// Creates from a version number
    ///
    /// # Errors
    /// Returns error if version is not 2 or 3
    pub fn from_version(version: u8) -> Result<Self> {
        match version {
            2 => Ok(Self::V2),
            3 => Ok(Self::V3),
            _ => Err(ZarrError::UnsupportedVersion { version }),
        }
    }

    /// Returns the default metadata file name
    #[must_use]
    pub const fn metadata_file_name(&self) -> &'static str {
        match self {
            Self::V2 => ".zarray",
            Self::V3 => "zarr.json",
        }
    }

    /// Returns the group metadata file name
    #[must_use]
    pub const fn group_file_name(&self) -> &'static str {
        match self {
            Self::V2 => ".zgroup",
            Self::V3 => "zarr.json",
        }
    }

    /// Returns the attributes file name
    #[must_use]
    pub const fn attrs_file_name(&self) -> &'static str {
        match self {
            Self::V2 => ".zattrs",
            Self::V3 => "zarr.json", // Attributes embedded in v3
        }
    }
}

impl core::fmt::Display for ZarrFormat {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.version())
    }
}

/// Node type in Zarr hierarchy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeType {
    /// Array node
    Array,
    /// Group node
    Group,
}

impl NodeType {
    /// Returns the string representation
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Array => "array",
            Self::Group => "group",
        }
    }
}

impl core::fmt::Display for NodeType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Array order - C (row-major) or F (column-major)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ArrayOrder {
    /// C order (row-major)
    #[serde(rename = "C")]
    #[default]
    C,
    /// Fortran order (column-major)
    #[serde(rename = "F")]
    F,
}

impl ArrayOrder {
    /// Creates from a character
    ///
    /// # Errors
    /// Returns error if character is not 'C' or 'F'
    pub fn from_char(c: char) -> Result<Self> {
        match c {
            'C' => Ok(Self::C),
            'F' => Ok(Self::F),
            _ => Err(ZarrError::Metadata(MetadataError::InvalidArrayOrder {
                order: c,
            })),
        }
    }

    /// Returns the character representation
    #[must_use]
    pub const fn as_char(&self) -> char {
        match self {
            Self::C => 'C',
            Self::F => 'F',
        }
    }

    /// Returns true if this is C order
    #[must_use]
    pub const fn is_c_order(&self) -> bool {
        matches!(self, Self::C)
    }

    /// Returns true if this is Fortran order
    #[must_use]
    pub const fn is_f_order(&self) -> bool {
        matches!(self, Self::F)
    }
}

impl core::fmt::Display for ArrayOrder {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.as_char())
    }
}

/// Byte order - little-endian or big-endian
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ByteOrder {
    /// Little-endian
    #[serde(rename = "<")]
    Little,
    /// Big-endian
    #[serde(rename = ">")]
    Big,
    /// Not applicable (for single-byte types)
    #[serde(rename = "|")]
    NotApplicable,
}

impl ByteOrder {
    /// Creates from a character
    ///
    /// # Errors
    /// Returns error if character is not '<', '>', or '|'
    pub fn from_char(c: char) -> Result<Self> {
        match c {
            '<' => Ok(Self::Little),
            '>' => Ok(Self::Big),
            '|' => Ok(Self::NotApplicable),
            _ => Err(ZarrError::Metadata(MetadataError::InvalidByteOrder {
                order: c,
            })),
        }
    }

    /// Returns the character representation
    #[must_use]
    pub const fn as_char(&self) -> char {
        match self {
            Self::Little => '<',
            Self::Big => '>',
            Self::NotApplicable => '|',
        }
    }

    /// Returns the native byte order for the platform
    #[must_use]
    pub const fn native() -> Self {
        #[cfg(target_endian = "little")]
        {
            Self::Little
        }
        #[cfg(target_endian = "big")]
        {
            Self::Big
        }
    }
}

impl Default for ByteOrder {
    fn default() -> Self {
        Self::native()
    }
}

impl core::fmt::Display for ByteOrder {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.as_char())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zarr_format() {
        assert_eq!(ZarrFormat::V2.version(), 2);
        assert_eq!(ZarrFormat::V3.version(), 3);

        assert_eq!(ZarrFormat::from_version(2).expect("v2"), ZarrFormat::V2);
        assert_eq!(ZarrFormat::from_version(3).expect("v3"), ZarrFormat::V3);
        assert!(ZarrFormat::from_version(1).is_err());

        assert_eq!(ZarrFormat::V2.metadata_file_name(), ".zarray");
        assert_eq!(ZarrFormat::V3.metadata_file_name(), "zarr.json");
    }

    #[test]
    fn test_node_type() {
        assert_eq!(NodeType::Array.as_str(), "array");
        assert_eq!(NodeType::Group.as_str(), "group");
    }

    #[test]
    fn test_array_order() {
        assert_eq!(ArrayOrder::from_char('C').expect("C"), ArrayOrder::C);
        assert_eq!(ArrayOrder::from_char('F').expect("F"), ArrayOrder::F);
        assert!(ArrayOrder::from_char('X').is_err());

        assert_eq!(ArrayOrder::C.as_char(), 'C');
        assert_eq!(ArrayOrder::F.as_char(), 'F');

        assert!(ArrayOrder::C.is_c_order());
        assert!(!ArrayOrder::C.is_f_order());
        assert!(ArrayOrder::F.is_f_order());
        assert!(!ArrayOrder::F.is_c_order());
    }

    #[test]
    fn test_byte_order() {
        assert_eq!(
            ByteOrder::from_char('<').expect("little"),
            ByteOrder::Little
        );
        assert_eq!(ByteOrder::from_char('>').expect("big"), ByteOrder::Big);
        assert_eq!(
            ByteOrder::from_char('|').expect("na"),
            ByteOrder::NotApplicable
        );
        assert!(ByteOrder::from_char('x').is_err());

        assert_eq!(ByteOrder::Little.as_char(), '<');
        assert_eq!(ByteOrder::Big.as_char(), '>');
        assert_eq!(ByteOrder::NotApplicable.as_char(), '|');

        // Test native
        #[cfg(target_endian = "little")]
        assert_eq!(ByteOrder::native(), ByteOrder::Little);

        #[cfg(target_endian = "big")]
        assert_eq!(ByteOrder::native(), ByteOrder::Big);
    }
}
