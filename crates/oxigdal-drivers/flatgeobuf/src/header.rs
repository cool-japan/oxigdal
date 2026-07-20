//! `FlatGeobuf` header types and `FlatBuffers` (de)serialization
//!
//! The header contains metadata about the feature collection including
//! geometry type, columns, CRS information, and spatial extent. It is encoded
//! on disk as a size-prefixed `FlatBuffers` `Header` table exactly as specified
//! by the `FlatGeobuf` schema (`header.fbs`), so files produced here
//! interoperate with GDAL and other `FlatGeobuf` tooling.

use crate::error::{FlatGeobufError, Result};
use crate::fbs::{self, FbTable, Offset};
use crate::index::PackedRTree;
use flatbuffers::FlatBufferBuilder;
use std::io::Write;

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

    /// Serializes this header to a `FlatBuffers` `Header` table.
    ///
    /// The returned bytes are the bare `FlatBuffers` message (no size prefix);
    /// callers write a `u32` length before them to produce the size-prefixed
    /// header expected by the on-disk `FlatGeobuf` layout.
    pub fn to_flatbuffer(&self) -> Result<Vec<u8>> {
        let mut fbb = FlatBufferBuilder::new();
        let root = build_header(&mut fbb, self);
        fbb.finish(root, None);
        Ok(fbb.finished_data().to_vec())
    }

    /// Writes the `FlatBuffers` header body to a byte stream.
    ///
    /// This writes only the header `FlatBuffers` message; the caller is
    /// responsible for the preceding `u32` size prefix.
    pub fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        let bytes = self.to_flatbuffer()?;
        writer.write_all(&bytes)?;
        Ok(())
    }

    /// Parses a `FlatBuffers` `Header` table from `data`.
    ///
    /// `data` must be the bare header `FlatBuffers` message (the bytes that
    /// follow the on-disk `u32` size prefix), not size-prefixed.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let table = FbTable::root(data)?;
        read_header(&table)
    }
}

impl Default for Header {
    fn default() -> Self {
        Self::new(GeometryType::Unknown)
    }
}

/// Builds a `FlatBuffers` `Column` table.
fn build_column(fbb: &mut FlatBufferBuilder<'_>, col: &Column) -> Offset {
    let name_off = fbb.create_string(&col.name);
    let title_off = col.title.as_deref().map(|s| fbb.create_string(s));
    let desc_off = col.description.as_deref().map(|s| fbb.create_string(s));

    let wip = fbb.start_table();
    fbb.push_slot_always(fbs::COLUMN_VT_NAME, name_off);
    fbb.push_slot::<u8>(fbs::COLUMN_VT_TYPE, col.column_type as u8, 0);
    if let Some(o) = title_off {
        fbb.push_slot_always(fbs::COLUMN_VT_TITLE, o);
    }
    if let Some(o) = desc_off {
        fbb.push_slot_always(fbs::COLUMN_VT_DESCRIPTION, o);
    }
    fbb.push_slot::<i32>(fbs::COLUMN_VT_WIDTH, col.width.unwrap_or(-1), -1);
    fbb.push_slot::<i32>(fbs::COLUMN_VT_PRECISION, col.precision.unwrap_or(-1), -1);
    fbb.push_slot::<i32>(fbs::COLUMN_VT_SCALE, col.scale.unwrap_or(-1), -1);
    fbb.push_slot::<bool>(fbs::COLUMN_VT_NULLABLE, col.nullable, true);
    fbb.push_slot::<bool>(fbs::COLUMN_VT_UNIQUE, col.unique, false);
    fbb.push_slot::<bool>(fbs::COLUMN_VT_PRIMARY_KEY, col.primary_key, false);
    fbb.end_table(wip)
}

/// Builds a `FlatBuffers` `Crs` table.
fn build_crs(fbb: &mut FlatBufferBuilder<'_>, crs: &CrsInfo) -> Offset {
    let org_off = crs.organization.as_deref().map(|s| fbb.create_string(s));
    let name_off = crs.name.as_deref().map(|s| fbb.create_string(s));
    let desc_off = crs.description.as_deref().map(|s| fbb.create_string(s));
    let wkt_off = crs.wkt.as_deref().map(|s| fbb.create_string(s));
    let code_str_off = crs.code.as_deref().map(|s| fbb.create_string(s));

    let wip = fbb.start_table();
    if let Some(o) = org_off {
        fbb.push_slot_always(fbs::CRS_VT_ORG, o);
    }
    fbb.push_slot::<i32>(fbs::CRS_VT_CODE, crs.organization_code.unwrap_or(0), 0);
    if let Some(o) = name_off {
        fbb.push_slot_always(fbs::CRS_VT_NAME, o);
    }
    if let Some(o) = desc_off {
        fbb.push_slot_always(fbs::CRS_VT_DESCRIPTION, o);
    }
    if let Some(o) = wkt_off {
        fbb.push_slot_always(fbs::CRS_VT_WKT, o);
    }
    if let Some(o) = code_str_off {
        fbb.push_slot_always(fbs::CRS_VT_CODE_STRING, o);
    }
    fbb.end_table(wip)
}

/// Builds a `FlatBuffers` `Header` table.
fn build_header(fbb: &mut FlatBufferBuilder<'_>, header: &Header) -> Offset {
    // All child offsets must be created before the table is started.
    let title_off = header.title.as_deref().map(|s| fbb.create_string(s));
    let desc_off = header.description.as_deref().map(|s| fbb.create_string(s));
    let meta_off = header.metadata.as_deref().map(|s| fbb.create_string(s));

    let envelope_off = header.extent.map(|e| fbb.create_vector::<f64>(&e[..]));

    let columns_off = if header.columns.is_empty() {
        None
    } else {
        let col_offs: Vec<Offset> = header
            .columns
            .iter()
            .map(|c| build_column(fbb, c))
            .collect();
        Some(fbb.create_vector(&col_offs))
    };

    let crs_off = header.crs.as_ref().map(|c| build_crs(fbb, c));

    let index_node_size = if header.has_index {
        PackedRTree::DEFAULT_NODE_SIZE as u16
    } else {
        0
    };

    let wip = fbb.start_table();
    if let Some(o) = envelope_off {
        fbb.push_slot_always(fbs::HEADER_VT_ENVELOPE, o);
    }
    fbb.push_slot::<u8>(fbs::HEADER_VT_GEOMETRY_TYPE, header.geometry_type as u8, 0);
    fbb.push_slot::<bool>(fbs::HEADER_VT_HAS_Z, header.has_z, false);
    fbb.push_slot::<bool>(fbs::HEADER_VT_HAS_M, header.has_m, false);
    fbb.push_slot::<bool>(fbs::HEADER_VT_HAS_T, header.has_t, false);
    fbb.push_slot::<bool>(fbs::HEADER_VT_HAS_TM, header.has_tm, false);
    if let Some(o) = columns_off {
        fbb.push_slot_always(fbs::HEADER_VT_COLUMNS, o);
    }
    fbb.push_slot::<u64>(
        fbs::HEADER_VT_FEATURES_COUNT,
        header.features_count.unwrap_or(0),
        0,
    );
    // Note: the schema default for `index_node_size` is 16. Writing 16 is a
    // no-op (index present, the FlatGeobuf default); an explicit 0 is stored to
    // signal "no index".
    fbb.push_slot::<u16>(fbs::HEADER_VT_INDEX_NODE_SIZE, index_node_size, 16);
    if let Some(o) = crs_off {
        fbb.push_slot_always(fbs::HEADER_VT_CRS, o);
    }
    if let Some(o) = title_off {
        fbb.push_slot_always(fbs::HEADER_VT_TITLE, o);
    }
    if let Some(o) = desc_off {
        fbb.push_slot_always(fbs::HEADER_VT_DESCRIPTION, o);
    }
    if let Some(o) = meta_off {
        fbb.push_slot_always(fbs::HEADER_VT_METADATA, o);
    }
    fbb.end_table(wip)
}

/// Reads a `FlatBuffers` `Column` table into a [`Column`].
fn read_column(t: &FbTable<'_>) -> Result<Column> {
    let name = t.get_string(fbs::COLUMN_VT_NAME)?.unwrap_or_default();
    let column_type = ColumnType::from_u8(t.get_u8(fbs::COLUMN_VT_TYPE, 0)?)?;
    Ok(Column {
        name,
        column_type,
        title: t.get_string(fbs::COLUMN_VT_TITLE)?,
        description: t.get_string(fbs::COLUMN_VT_DESCRIPTION)?,
        width: opt_i32(t.get_i32(fbs::COLUMN_VT_WIDTH, -1)?),
        precision: opt_i32(t.get_i32(fbs::COLUMN_VT_PRECISION, -1)?),
        scale: opt_i32(t.get_i32(fbs::COLUMN_VT_SCALE, -1)?),
        nullable: t.get_bool(fbs::COLUMN_VT_NULLABLE, true)?,
        unique: t.get_bool(fbs::COLUMN_VT_UNIQUE, false)?,
        primary_key: t.get_bool(fbs::COLUMN_VT_PRIMARY_KEY, false)?,
    })
}

/// Reads a `FlatBuffers` `Crs` table into a [`CrsInfo`].
fn read_crs(t: &FbTable<'_>) -> Result<CrsInfo> {
    let code = t.get_i32(fbs::CRS_VT_CODE, 0)?;
    Ok(CrsInfo {
        organization: t.get_string(fbs::CRS_VT_ORG)?,
        organization_code: if code == 0 { None } else { Some(code) },
        name: t.get_string(fbs::CRS_VT_NAME)?,
        description: t.get_string(fbs::CRS_VT_DESCRIPTION)?,
        wkt: t.get_string(fbs::CRS_VT_WKT)?,
        code: t.get_string(fbs::CRS_VT_CODE_STRING)?,
    })
}

/// Reads a `FlatBuffers` `Header` table into a [`Header`].
fn read_header(t: &FbTable<'_>) -> Result<Header> {
    let geometry_type = GeometryType::from_u8(t.get_u8(fbs::HEADER_VT_GEOMETRY_TYPE, 0)?)?;
    let has_z = t.get_bool(fbs::HEADER_VT_HAS_Z, false)?;
    let has_m = t.get_bool(fbs::HEADER_VT_HAS_M, false)?;
    let has_t = t.get_bool(fbs::HEADER_VT_HAS_T, false)?;
    let has_tm = t.get_bool(fbs::HEADER_VT_HAS_TM, false)?;

    let mut columns = Vec::new();
    for col_table in t.get_table_vector(fbs::HEADER_VT_COLUMNS)? {
        columns.push(read_column(&col_table)?);
    }

    let index_node_size = t.get_u16(fbs::HEADER_VT_INDEX_NODE_SIZE, 16)?;
    let has_index = index_node_size != 0;
    let features_count = Some(t.get_u64(fbs::HEADER_VT_FEATURES_COUNT, 0)?);

    let crs = match t.get_table(fbs::HEADER_VT_CRS)? {
        Some(c) => Some(read_crs(&c)?),
        None => None,
    };

    let extent = match t.get_f64_vector(fbs::HEADER_VT_ENVELOPE)? {
        Some(v) if v.len() == 4 => Some([v[0], v[1], v[2], v[3]]),
        _ => None,
    };

    Ok(Header {
        geometry_type,
        has_z,
        has_m,
        has_t,
        has_tm,
        columns,
        features_count,
        has_index,
        crs,
        title: t.get_string(fbs::HEADER_VT_TITLE)?,
        description: t.get_string(fbs::HEADER_VT_DESCRIPTION)?,
        metadata: t.get_string(fbs::HEADER_VT_METADATA)?,
        extent,
    })
}

#[inline]
const fn opt_i32(v: i32) -> Option<i32> {
    if v == -1 { None } else { Some(v) }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
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

    /// The header must round-trip through the real `FlatBuffers` encoding.
    #[test]
    fn test_header_flatbuffer_roundtrip() {
        let mut header = Header::new(GeometryType::MultiPolygon)
            .with_z()
            .with_index(true)
            .with_crs(CrsInfo::from_epsg(4326))
            .with_extent([-10.0, -20.0, 30.0, 40.0])
            .with_features_count(7);
        header.add_column(Column::new("name", ColumnType::String));
        header.add_column(Column::new("count", ColumnType::Int));
        header.title = Some("My Layer".to_string());

        let bytes = header.to_flatbuffer().expect("encode header");
        let decoded = Header::from_bytes(&bytes).expect("decode header");

        assert_eq!(decoded.geometry_type, GeometryType::MultiPolygon);
        assert!(decoded.has_z);
        assert!(!decoded.has_m);
        assert!(decoded.has_index);
        assert_eq!(decoded.features_count, Some(7));
        assert_eq!(decoded.extent, Some([-10.0, -20.0, 30.0, 40.0]));
        assert_eq!(decoded.columns.len(), 2);
        assert_eq!(decoded.columns[0].name, "name");
        assert_eq!(decoded.columns[0].column_type, ColumnType::String);
        assert_eq!(decoded.columns[1].name, "count");
        assert_eq!(decoded.title.as_deref(), Some("My Layer"));
        let crs = decoded.crs.expect("crs present");
        assert_eq!(crs.organization.as_deref(), Some("EPSG"));
        assert_eq!(crs.organization_code, Some(4326));
    }

    /// A header with no index must encode `index_node_size = 0`.
    #[test]
    fn test_header_no_index_roundtrip() {
        let header = Header::new(GeometryType::Point);
        let bytes = header.to_flatbuffer().expect("encode header");
        let decoded = Header::from_bytes(&bytes).expect("decode header");
        assert!(!decoded.has_index);
        assert!(decoded.crs.is_none());
        assert!(decoded.extent.is_none());
    }
}
