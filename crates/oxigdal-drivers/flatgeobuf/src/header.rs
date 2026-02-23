//! `FlatGeobuf` header types and parsing
//!
//! The header contains metadata about the feature collection including
//! geometry type, columns, CRS information, and spatial extent.

use crate::error::{FlatGeobufError, Result};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Read, Write};

/// Geometry type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum GeometryType {
    /// Unknown geometry type
    Unknown = 0,
    /// Point
    Point = 1,
    /// `LineString`
    LineString = 2,
    /// Polygon
    Polygon = 3,
    /// `MultiPoint`
    MultiPoint = 4,
    /// `MultiLineString`
    MultiLineString = 5,
    /// `MultiPolygon`
    MultiPolygon = 6,
    /// `GeometryCollection`
    GeometryCollection = 7,
    /// `CircularString`
    CircularString = 8,
    /// `CompoundCurve`
    CompoundCurve = 9,
    /// `CurvePolygon`
    CurvePolygon = 10,
    /// `MultiCurve`
    MultiCurve = 11,
    /// `MultiSurface`
    MultiSurface = 12,
    /// Curve
    Curve = 13,
    /// Surface
    Surface = 14,
    /// `PolyhedralSurface`
    PolyhedralSurface = 15,
    /// TIN
    Tin = 16,
    /// Triangle
    Triangle = 17,
}

impl GeometryType {
    /// Converts from u8
    pub fn from_u8(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::Unknown),
            1 => Ok(Self::Point),
            2 => Ok(Self::LineString),
            3 => Ok(Self::Polygon),
            4 => Ok(Self::MultiPoint),
            5 => Ok(Self::MultiLineString),
            6 => Ok(Self::MultiPolygon),
            7 => Ok(Self::GeometryCollection),
            8 => Ok(Self::CircularString),
            9 => Ok(Self::CompoundCurve),
            10 => Ok(Self::CurvePolygon),
            11 => Ok(Self::MultiCurve),
            12 => Ok(Self::MultiSurface),
            13 => Ok(Self::Curve),
            14 => Ok(Self::Surface),
            15 => Ok(Self::PolyhedralSurface),
            16 => Ok(Self::Tin),
            17 => Ok(Self::Triangle),
            _ => Err(FlatGeobufError::UnsupportedGeometryType(value)),
        }
    }

    /// Converts to `OxiGDAL` geometry type name
    #[must_use]
    pub const fn to_name(&self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::Point => "Point",
            Self::LineString => "LineString",
            Self::Polygon => "Polygon",
            Self::MultiPoint => "MultiPoint",
            Self::MultiLineString => "MultiLineString",
            Self::MultiPolygon => "MultiPolygon",
            Self::GeometryCollection => "GeometryCollection",
            Self::CircularString => "CircularString",
            Self::CompoundCurve => "CompoundCurve",
            Self::CurvePolygon => "CurvePolygon",
            Self::MultiCurve => "MultiCurve",
            Self::MultiSurface => "MultiSurface",
            Self::Curve => "Curve",
            Self::Surface => "Surface",
            Self::PolyhedralSurface => "PolyhedralSurface",
            Self::Tin => "TIN",
            Self::Triangle => "Triangle",
        }
    }
}

/// Column data type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ColumnType {
    /// Byte (8-bit signed integer)
    Byte = 0,
    /// Unsigned byte (8-bit unsigned integer)
    UByte = 1,
    /// Boolean
    Bool = 2,
    /// Short (16-bit signed integer)
    Short = 3,
    /// Unsigned short (16-bit unsigned integer)
    UShort = 4,
    /// Int (32-bit signed integer)
    Int = 5,
    /// Unsigned int (32-bit unsigned integer)
    UInt = 6,
    /// Long (64-bit signed integer)
    Long = 7,
    /// Unsigned long (64-bit unsigned integer)
    ULong = 8,
    /// Float (32-bit)
    Float = 9,
    /// Double (64-bit)
    Double = 10,
    /// String (UTF-8)
    String = 11,
    /// JSON
    Json = 12,
    /// `DateTime` (ISO 8601 string)
    DateTime = 13,
    /// Binary data
    Binary = 14,
}

impl ColumnType {
    /// Converts from u8
    pub fn from_u8(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::Byte),
            1 => Ok(Self::UByte),
            2 => Ok(Self::Bool),
            3 => Ok(Self::Short),
            4 => Ok(Self::UShort),
            5 => Ok(Self::Int),
            6 => Ok(Self::UInt),
            7 => Ok(Self::Long),
            8 => Ok(Self::ULong),
            9 => Ok(Self::Float),
            10 => Ok(Self::Double),
            11 => Ok(Self::String),
            12 => Ok(Self::Json),
            13 => Ok(Self::DateTime),
            14 => Ok(Self::Binary),
            _ => Err(FlatGeobufError::UnsupportedColumnType(value)),
        }
    }

    /// Returns the type name
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Byte => "Byte",
            Self::UByte => "UByte",
            Self::Bool => "Bool",
            Self::Short => "Short",
            Self::UShort => "UShort",
            Self::Int => "Int",
            Self::UInt => "UInt",
            Self::Long => "Long",
            Self::ULong => "ULong",
            Self::Float => "Float",
            Self::Double => "Double",
            Self::String => "String",
            Self::Json => "Json",
            Self::DateTime => "DateTime",
            Self::Binary => "Binary",
        }
    }
}

/// Column definition
#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    /// Column name
    pub name: String,
    /// Column data type
    pub column_type: ColumnType,
    /// Optional title for display
    pub title: Option<String>,
    /// Optional description
    pub description: Option<String>,
    /// Width for string/binary types
    pub width: Option<i32>,
    /// Precision for numeric types
    pub precision: Option<i32>,
    /// Scale for numeric types
    pub scale: Option<i32>,
    /// Whether the column is nullable
    pub nullable: bool,
    /// Whether values are unique
    pub unique: bool,
    /// Whether this is a primary key
    pub primary_key: bool,
}

impl Column {
    /// Creates a new column
    #[must_use]
    pub fn new<S: Into<String>>(name: S, column_type: ColumnType) -> Self {
        Self {
            name: name.into(),
            column_type,
            title: None,
            description: None,
            width: None,
            precision: None,
            scale: None,
            nullable: true,
            unique: false,
            primary_key: false,
        }
    }

    /// Sets the title
    #[must_use]
    pub fn with_title<S: Into<String>>(mut self, title: S) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Sets the description
    #[must_use]
    pub fn with_description<S: Into<String>>(mut self, description: S) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets nullable flag
    #[must_use]
    pub const fn with_nullable(mut self, nullable: bool) -> Self {
        self.nullable = nullable;
        self
    }

    /// Sets unique flag
    #[must_use]
    pub const fn with_unique(mut self, unique: bool) -> Self {
        self.unique = unique;
        self
    }

    /// Sets primary key flag
    #[must_use]
    pub const fn with_primary_key(mut self, primary_key: bool) -> Self {
        self.primary_key = primary_key;
        self
    }
}

/// CRS (Coordinate Reference System) information
#[derive(Debug, Clone, PartialEq)]
pub struct CrsInfo {
    /// Organization (e.g., "EPSG")
    pub organization: Option<String>,
    /// Organization code (e.g., 4326)
    pub organization_code: Option<i32>,
    /// CRS name
    pub name: Option<String>,
    /// CRS description
    pub description: Option<String>,
    /// WKT representation
    pub wkt: Option<String>,
    /// CRS identifier code
    pub code: Option<String>,
}

impl CrsInfo {
    /// Creates an empty CRS info
    #[must_use]
    pub const fn new() -> Self {
        Self {
            organization: None,
            organization_code: None,
            name: None,
            description: None,
            wkt: None,
            code: None,
        }
    }

    /// Creates CRS info from EPSG code
    #[must_use]
    pub fn from_epsg(code: i32) -> Self {
        Self {
            organization: Some("EPSG".to_string()),
            organization_code: Some(code),
            name: Some(format!("EPSG:{code}")),
            description: None,
            wkt: None,
            code: Some(code.to_string()),
        }
    }

    /// Creates CRS info from WKT
    #[must_use]
    pub fn from_wkt<S: Into<String>>(wkt: S) -> Self {
        Self {
            organization: None,
            organization_code: None,
            name: None,
            description: None,
            wkt: Some(wkt.into()),
            code: None,
        }
    }
}

impl Default for CrsInfo {
    fn default() -> Self {
        Self::new()
    }
}

/// `FlatGeobuf` header
#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    /// Geometry type
    pub geometry_type: GeometryType,
    /// Whether geometries have Z dimension
    pub has_z: bool,
    /// Whether geometries have M dimension
    pub has_m: bool,
    /// Whether geometries can have different types (for `GeometryCollection`)
    pub has_t: bool,
    /// Whether geometries can have different M flags
    pub has_tm: bool,
    /// Column definitions
    pub columns: Vec<Column>,
    /// Total feature count (optional)
    pub features_count: Option<u64>,
    /// Whether the file has a spatial index
    pub has_index: bool,
    /// CRS information
    pub crs: Option<CrsInfo>,
    /// Title of the dataset
    pub title: Option<String>,
    /// Description of the dataset
    pub description: Option<String>,
    /// Metadata (as JSON string)
    pub metadata: Option<String>,
    /// Bounding box: [`min_x`, `min_y`, `max_x`, `max_y`]
    pub extent: Option<[f64; 4]>,
}

impl Header {
    /// Creates a new header with the specified geometry type
    #[must_use]
    pub const fn new(geometry_type: GeometryType) -> Self {
        Self {
            geometry_type,
            has_z: false,
            has_m: false,
            has_t: false,
            has_tm: false,
            columns: Vec::new(),
            features_count: None,
            has_index: false,
            crs: None,
            title: None,
            description: None,
            metadata: None,
            extent: None,
        }
    }

    /// Sets the Z dimension flag
    #[must_use]
    pub const fn with_z(mut self) -> Self {
        self.has_z = true;
        self
    }

    /// Sets the M dimension flag
    #[must_use]
    pub const fn with_m(mut self) -> Self {
        self.has_m = true;
        self
    }

    /// Sets the index flag
    #[must_use]
    pub const fn with_index(mut self, has_index: bool) -> Self {
        self.has_index = has_index;
        self
    }

    /// Sets the CRS
    #[must_use]
    pub fn with_crs(mut self, crs: CrsInfo) -> Self {
        self.crs = Some(crs);
        self
    }

    /// Adds a column
    pub fn add_column(&mut self, column: Column) {
        self.columns.push(column);
    }

    /// Sets the extent
    #[must_use]
    pub const fn with_extent(mut self, extent: [f64; 4]) -> Self {
        self.extent = Some(extent);
        self
    }

    /// Sets the feature count
    #[must_use]
    pub const fn with_features_count(mut self, count: u64) -> Self {
        self.features_count = Some(count);
        self
    }

    /// Reads header from a byte stream
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        // For now, we'll implement a simplified binary reading
        // In a full implementation, this would use FlatBuffers
        let geometry_type = GeometryType::from_u8(reader.read_u8()?)?;
        let flags = reader.read_u8()?;

        let has_z = (flags & 0x01) != 0;
        let has_m = (flags & 0x02) != 0;
        let has_t = (flags & 0x04) != 0;
        let has_tm = (flags & 0x08) != 0;
        let has_index = (flags & 0x10) != 0;

        let column_count = reader.read_u32::<LittleEndian>()?;
        let mut columns = Vec::with_capacity(column_count as usize);

        for _ in 0..column_count {
            let name_len = reader.read_u32::<LittleEndian>()?;
            let mut name_bytes = vec![0u8; name_len as usize];
            reader.read_exact(&mut name_bytes)?;
            let name = String::from_utf8(name_bytes)?;

            let column_type = ColumnType::from_u8(reader.read_u8()?)?;

            columns.push(Column::new(name, column_type));
        }

        let has_extent = reader.read_u8()? != 0;
        let extent = if has_extent {
            Some([
                reader.read_f64::<LittleEndian>()?,
                reader.read_f64::<LittleEndian>()?,
                reader.read_f64::<LittleEndian>()?,
                reader.read_f64::<LittleEndian>()?,
            ])
        } else {
            None
        };

        // Read features count
        let has_features_count = reader.read_u8()? != 0;
        let features_count = if has_features_count {
            Some(reader.read_u64::<LittleEndian>()?)
        } else {
            None
        };

        // Read CRS
        let has_crs = reader.read_u8()? != 0;
        let crs = if has_crs {
            let has_org = reader.read_u8()? != 0;
            let organization = if has_org {
                let len = reader.read_u32::<LittleEndian>()?;
                let mut bytes = vec![0u8; len as usize];
                reader.read_exact(&mut bytes)?;
                Some(String::from_utf8(bytes)?)
            } else {
                None
            };

            let has_code = reader.read_u8()? != 0;
            let organization_code = if has_code {
                Some(reader.read_i32::<LittleEndian>()?)
            } else {
                None
            };

            let has_wkt = reader.read_u8()? != 0;
            let wkt = if has_wkt {
                let len = reader.read_u32::<LittleEndian>()?;
                let mut bytes = vec![0u8; len as usize];
                reader.read_exact(&mut bytes)?;
                Some(String::from_utf8(bytes)?)
            } else {
                None
            };

            Some(CrsInfo {
                organization,
                organization_code,
                name: organization_code.map(|c| format!("EPSG:{c}")),
                description: None,
                wkt,
                code: organization_code.map(|c| c.to_string()),
            })
        } else {
            None
        };

        Ok(Self {
            geometry_type,
            has_z,
            has_m,
            has_t,
            has_tm,
            columns,
            features_count,
            has_index,
            crs,
            title: None,
            description: None,
            metadata: None,
            extent,
        })
    }

    /// Writes header to a byte stream
    pub fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        writer.write_u8(self.geometry_type as u8)?;

        let mut flags = 0u8;
        if self.has_z {
            flags |= 0x01;
        }
        if self.has_m {
            flags |= 0x02;
        }
        if self.has_t {
            flags |= 0x04;
        }
        if self.has_tm {
            flags |= 0x08;
        }
        if self.has_index {
            flags |= 0x10;
        }
        writer.write_u8(flags)?;

        writer.write_u32::<LittleEndian>(self.columns.len() as u32)?;
        for column in &self.columns {
            let name_bytes = column.name.as_bytes();
            writer.write_u32::<LittleEndian>(name_bytes.len() as u32)?;
            writer.write_all(name_bytes)?;
            writer.write_u8(column.column_type as u8)?;
        }

        if let Some(extent) = self.extent {
            writer.write_u8(1)?;
            writer.write_f64::<LittleEndian>(extent[0])?;
            writer.write_f64::<LittleEndian>(extent[1])?;
            writer.write_f64::<LittleEndian>(extent[2])?;
            writer.write_f64::<LittleEndian>(extent[3])?;
        } else {
            writer.write_u8(0)?;
        }

        // Write features count
        if let Some(count) = self.features_count {
            writer.write_u8(1)?;
            writer.write_u64::<LittleEndian>(count)?;
        } else {
            writer.write_u8(0)?;
        }

        // Write CRS
        if let Some(ref crs) = self.crs {
            writer.write_u8(1)?;

            // Write organization
            if let Some(ref org) = crs.organization {
                writer.write_u8(1)?;
                let bytes = org.as_bytes();
                writer.write_u32::<LittleEndian>(bytes.len() as u32)?;
                writer.write_all(bytes)?;
            } else {
                writer.write_u8(0)?;
            }

            // Write organization_code
            if let Some(code) = crs.organization_code {
                writer.write_u8(1)?;
                writer.write_i32::<LittleEndian>(code)?;
            } else {
                writer.write_u8(0)?;
            }

            // Write WKT
            if let Some(ref wkt) = crs.wkt {
                writer.write_u8(1)?;
                let bytes = wkt.as_bytes();
                writer.write_u32::<LittleEndian>(bytes.len() as u32)?;
                writer.write_all(bytes)?;
            } else {
                writer.write_u8(0)?;
            }
        } else {
            writer.write_u8(0)?;
        }

        Ok(())
    }
}

impl Default for Header {
    fn default() -> Self {
        Self::new(GeometryType::Unknown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_geometry_type() {
        assert_eq!(GeometryType::Point as u8, 1);
        assert_eq!(GeometryType::from_u8(1).ok(), Some(GeometryType::Point));
        assert_eq!(GeometryType::Point.to_name(), "Point");
    }

    #[test]
    fn test_column_type() {
        assert_eq!(ColumnType::String as u8, 11);
        assert_eq!(ColumnType::from_u8(11).ok(), Some(ColumnType::String));
        assert_eq!(ColumnType::String.name(), "String");
    }

    #[test]
    fn test_column_creation() {
        let col = Column::new("test", ColumnType::String)
            .with_nullable(false)
            .with_unique(true);

        assert_eq!(col.name, "test");
        assert_eq!(col.column_type, ColumnType::String);
        assert!(!col.nullable);
        assert!(col.unique);
    }

    #[test]
    fn test_crs_info() {
        let crs = CrsInfo::from_epsg(4326);
        assert_eq!(crs.organization, Some("EPSG".to_string()));
        assert_eq!(crs.organization_code, Some(4326));
    }

    #[test]
    fn test_header_creation() {
        let header = Header::new(GeometryType::Point)
            .with_z()
            .with_index(true)
            .with_extent([-180.0, -90.0, 180.0, 90.0]);

        assert_eq!(header.geometry_type, GeometryType::Point);
        assert!(header.has_z);
        assert!(header.has_index);
        assert_eq!(header.extent, Some([-180.0, -90.0, 180.0, 90.0]));
    }
}
