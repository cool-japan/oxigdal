//! GeoPackage metadata structures.
//!
//! Implements the `gpkg_metadata` and `gpkg_metadata_reference` system tables
//! defined in OGC GeoPackage Encoding Standard v1.3.1, §10.8.
//!
//! * [`GpkgMetadata`] — a row from `gpkg_metadata` (Table 16)
//! * [`GpkgMetadataReference`] — a row from `gpkg_metadata_reference` (Table 18)
//! * [`MetadataScope`] — MD_ScopeCode enumeration per ISO 19115
//! * [`ReferenceScope`] — scope constants for `gpkg_metadata_reference`

use std::str::FromStr;

// ─────────────────────────────────────────────────────────────────────────────
// MetadataScope — OGC §10.8, Table 16 (MD_ScopeCode)
// ─────────────────────────────────────────────────────────────────────────────

/// MD_ScopeCode enumeration as required by OGC GeoPackage §10.8 (Table 16).
///
/// Each variant corresponds to the string stored in the `md_scope` column of
/// the `gpkg_metadata` table.  Unknown strings map to [`MetadataScope::Undefined`].
#[derive(Debug, Clone, PartialEq)]
pub enum MetadataScope {
    /// Scope is not defined or not known.
    Undefined,
    /// A single session of data capture in the field.
    FieldSession,
    /// A collection of field sessions.
    CollectionSession,
    /// A series of datasets sharing the same product specification.
    Series,
    /// A geographic dataset.
    Dataset,
    /// Metadata describing a feature type.
    FeatureType,
    /// Metadata describing a single feature instance.
    Feature,
    /// Metadata describing an attribute type.
    AttributeType,
    /// Metadata describing a single attribute value.
    Attribute,
    /// A tile or a collection of tiles.
    Tile,
    /// A copy or imitation of an existing or hypothetical object.
    Model,
    /// A catalog of resources.
    Catalog,
    /// A conceptual schema or description language.
    Schema,
    /// A structured system of concepts or terms in a domain.
    Taxonomy,
    /// A computer program, instruction, or set of instructions.
    Software,
    /// A capability provided by a system for a set of inputs (OGC service).
    Service,
    /// A hardware item.
    CollectionHardware,
    /// A dataset that does not have geographic extent.
    NonGeographicDataset,
    /// A named array dimension of a coverage result.
    DimensionGroup,
}

impl MetadataScope {
    /// Return the canonical OGC string for this scope code.
    ///
    /// The returned string is suitable for direct insertion into the `md_scope`
    /// column of `gpkg_metadata`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Undefined => "undefined",
            Self::FieldSession => "fieldSession",
            Self::CollectionSession => "collectionSession",
            Self::Series => "series",
            Self::Dataset => "dataset",
            Self::FeatureType => "featureType",
            Self::Feature => "feature",
            Self::AttributeType => "attributeType",
            Self::Attribute => "attribute",
            Self::Tile => "tile",
            Self::Model => "model",
            Self::Catalog => "catalog",
            Self::Schema => "schema",
            Self::Taxonomy => "taxonomy",
            Self::Software => "software",
            Self::Service => "service",
            Self::CollectionHardware => "collectionHardware",
            Self::NonGeographicDataset => "nonGeographicDataset",
            Self::DimensionGroup => "dimensionGroup",
        }
    }
}

impl FromStr for MetadataScope {
    /// Parsing is infallible: unknown strings map to [`MetadataScope::Undefined`].
    type Err = std::convert::Infallible;

    /// Parse the `md_scope` column value from `gpkg_metadata`.
    ///
    /// Follows the exact case-sensitive string values mandated by the OGC
    /// GeoPackage specification.  Any unrecognised value maps to
    /// [`MetadataScope::Undefined`].
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let scope = match s {
            "undefined" => Self::Undefined,
            "fieldSession" => Self::FieldSession,
            "collectionSession" => Self::CollectionSession,
            "series" => Self::Series,
            "dataset" => Self::Dataset,
            "featureType" => Self::FeatureType,
            "feature" => Self::Feature,
            "attributeType" => Self::AttributeType,
            "attribute" => Self::Attribute,
            "tile" => Self::Tile,
            "model" => Self::Model,
            "catalog" => Self::Catalog,
            "schema" => Self::Schema,
            "taxonomy" => Self::Taxonomy,
            "software" => Self::Software,
            "service" => Self::Service,
            "collectionHardware" => Self::CollectionHardware,
            "nonGeographicDataset" => Self::NonGeographicDataset,
            "dimensionGroup" => Self::DimensionGroup,
            _ => Self::Undefined,
        };
        Ok(scope)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// GpkgMetadata — OGC §10.8 Table 16
// ─────────────────────────────────────────────────────────────────────────────

/// A row from the `gpkg_metadata` table (OGC GeoPackage §10.8, Table 16).
///
/// | Column            | Type    | Notes                                  |
/// |-------------------|---------|----------------------------------------|
/// | `id`              | INTEGER | Primary key, auto-increment            |
/// | `md_scope`        | TEXT    | MD_ScopeCode string                    |
/// | `md_standard_uri` | TEXT    | URI identifying the metadata standard  |
/// | `mime_type`       | TEXT    | MIME type of the metadata content      |
/// | `metadata`        | TEXT    | Metadata content (XML, JSON, etc.)     |
#[derive(Debug, Clone, PartialEq)]
pub struct GpkgMetadata {
    /// Primary-key identifier.  Corresponds to the `id` column.
    pub id: i64,
    /// Scope code for this metadata document.
    pub md_scope: MetadataScope,
    /// URI of the metadata standard referenced by this record.
    pub md_standard_uri: String,
    /// MIME type of the metadata content (e.g. `"text/xml"`, `"application/json"`).
    pub mime_type: String,
    /// The raw metadata content as a UTF-8 string.
    pub metadata: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// ReferenceScope — OGC §10.8.5, Table 18
// ─────────────────────────────────────────────────────────────────────────────

/// Reference scope constants for the `reference_scope` column of
/// `gpkg_metadata_reference` (OGC GeoPackage §10.8.5, Table 18).
///
/// The scope determines which combination of `table_name`, `column_name`, and
/// `row_id_value` columns carry meaningful values.
#[derive(Debug, Clone, PartialEq)]
pub enum ReferenceScope {
    /// Metadata applies to the GeoPackage as a whole.
    GeoPackage,
    /// Metadata applies to a specific user-data table.
    Table,
    /// Metadata applies to a specific column within a table.
    Column,
    /// Metadata applies to a specific row (identified by `row_id_value`).
    Row,
    /// Metadata applies to a specific cell (row + column combination).
    RowCol,
}

impl ReferenceScope {
    /// Return the canonical OGC string for this reference scope.
    ///
    /// The returned string matches the values stored in the `reference_scope`
    /// column of `gpkg_metadata_reference`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::GeoPackage => "geopackage",
            Self::Table => "table",
            Self::Column => "column",
            Self::Row => "row",
            Self::RowCol => "row/col",
        }
    }
}

impl FromStr for ReferenceScope {
    /// Parsing is infallible: unknown strings map to [`ReferenceScope::GeoPackage`].
    type Err = std::convert::Infallible;

    /// Parse the `reference_scope` column value from `gpkg_metadata_reference`.
    ///
    /// Comparison is case-sensitive against the OGC-specified lower-case string
    /// values.  Unknown strings fall back to [`ReferenceScope::GeoPackage`].
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let scope = match s {
            "geopackage" => Self::GeoPackage,
            "table" => Self::Table,
            "column" => Self::Column,
            "row" => Self::Row,
            "row/col" => Self::RowCol,
            _ => Self::GeoPackage,
        };
        Ok(scope)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// GpkgMetadataReference — OGC §10.8.5 Table 18
// ─────────────────────────────────────────────────────────────────────────────

/// A row from the `gpkg_metadata_reference` table (OGC GeoPackage §10.8.5, Table 18).
///
/// Links a [`GpkgMetadata`] record (via `md_file_id`) to the database
/// object it describes.  Nullable columns are represented as `Option<_>`.
///
/// Column layout:
///
/// | # | Column            | Type              | Nullable |
/// |---|-------------------|-------------------|----------|
/// | 0 | `reference_scope` | TEXT              | No       |
/// | 1 | `table_name`      | TEXT              | Yes      |
/// | 2 | `column_name`     | TEXT              | Yes      |
/// | 3 | `row_id_value`    | INTEGER           | Yes      |
/// | 4 | `timestamp`       | DATETIME (TEXT)   | No       |
/// | 5 | `md_file_id`      | INTEGER           | No (FK)  |
/// | 6 | `md_parent_id`    | INTEGER           | Yes (FK) |
#[derive(Debug, Clone, PartialEq)]
pub struct GpkgMetadataReference {
    /// Scope of the reference (geopackage / table / column / row / row/col).
    pub reference_scope: ReferenceScope,
    /// User-data table name; `None` when `reference_scope` is `"geopackage"`.
    pub table_name: Option<String>,
    /// Column name within `table_name`; `None` unless scope is `"column"` or
    /// `"row/col"`.
    pub column_name: Option<String>,
    /// Row identifier within `table_name`; `None` unless scope is `"row"` or
    /// `"row/col"`.
    pub row_id_value: Option<i64>,
    /// ISO 8601 timestamp of when the reference was last updated.
    pub timestamp: String,
    /// Foreign key into `gpkg_metadata.id` — the referenced metadata record.
    pub md_file_id: i64,
    /// Optional foreign key into `gpkg_metadata.id` — the parent metadata
    /// record in a hierarchical metadata graph.
    pub md_parent_id: Option<i64>,
}
