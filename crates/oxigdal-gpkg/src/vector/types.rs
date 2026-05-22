//! Core GeoPackage data types: field types/values, coordinate structures, and geometry.

// ─────────────────────────────────────────────────────────────────────────────
// FieldType
// ─────────────────────────────────────────────────────────────────────────────

/// Column type categories used in GeoPackage / SQLite schemas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldType {
    /// Signed integer (SQLite INTEGER affinity).
    Integer,
    /// IEEE-754 double (SQLite REAL affinity).
    Real,
    /// UTF-8 text (SQLite TEXT affinity).
    Text,
    /// Raw binary (SQLite BLOB affinity).
    Blob,
    /// Boolean stored as INTEGER 0/1.
    Boolean,
    /// Calendar date stored as TEXT `"YYYY-MM-DD"`.
    Date,
    /// Date+time stored as TEXT `"YYYY-MM-DDTHH:MM:SS.sssZ"`.
    DateTime,
    /// SQL NULL / unknown type.
    Null,
}

impl FieldType {
    /// Derive a [`FieldType`] from a SQLite type-name string (case-insensitive).
    ///
    /// Unrecognised strings map to [`FieldType::Text`] following SQLite type
    /// affinity rules.
    pub fn from_sql_type(type_str: &str) -> Self {
        match type_str.to_ascii_uppercase().trim() {
            "INTEGER" | "INT" | "TINYINT" | "SMALLINT" | "MEDIUMINT" | "BIGINT"
            | "UNSIGNED BIG INT" | "INT2" | "INT8" => Self::Integer,
            "REAL" | "DOUBLE" | "DOUBLE PRECISION" | "FLOAT" | "NUMERIC" | "DECIMAL" => Self::Real,
            "BLOB" => Self::Blob,
            "BOOLEAN" | "BOOL" => Self::Boolean,
            "DATE" => Self::Date,
            "DATETIME" | "TIMESTAMP" => Self::DateTime,
            "NULL" => Self::Null,
            _ => Self::Text,
        }
    }

    /// Return the canonical SQL type name string for this field type.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Integer => "INTEGER",
            Self::Real => "REAL",
            Self::Text => "TEXT",
            Self::Blob => "BLOB",
            Self::Boolean => "BOOLEAN",
            Self::Date => "DATE",
            Self::DateTime => "DATETIME",
            Self::Null => "NULL",
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// FieldValue
// ─────────────────────────────────────────────────────────────────────────────

/// A runtime value read from a GeoPackage feature-table column.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    /// Signed 64-bit integer.
    Integer(i64),
    /// IEEE-754 double-precision float.
    Real(f64),
    /// UTF-8 text.
    Text(String),
    /// Raw binary data.
    Blob(Vec<u8>),
    /// Boolean value.
    Boolean(bool),
    /// SQL NULL.
    Null,
}

impl FieldValue {
    /// Return the contained integer, or `None` for other variants.
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Self::Integer(v) => Some(*v),
            _ => None,
        }
    }

    /// Return the contained float, or `None` for other variants.
    pub fn as_real(&self) -> Option<f64> {
        match self {
            Self::Real(v) => Some(*v),
            _ => None,
        }
    }

    /// Return a reference to the contained text, or `None` for other variants.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Return the contained boolean, or `None` for other variants.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    /// Return `true` if this is the SQL NULL variant.
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Return the [`FieldType`] that corresponds to this value's variant.
    pub fn field_type(&self) -> FieldType {
        match self {
            Self::Integer(_) => FieldType::Integer,
            Self::Real(_) => FieldType::Real,
            Self::Text(_) => FieldType::Text,
            Self::Blob(_) => FieldType::Blob,
            Self::Boolean(_) => FieldType::Boolean,
            Self::Null => FieldType::Null,
        }
    }

    /// Serialise this value as a JSON fragment (no trailing newline).
    pub(crate) fn to_json(&self) -> String {
        match self {
            Self::Integer(v) => v.to_string(),
            Self::Real(v) => {
                if v.is_finite() {
                    format!("{v}")
                } else {
                    "null".into()
                }
            }
            Self::Text(s) => json_string_escape(s),
            Self::Blob(b) => {
                // Encode as a hex string prefixed with "0x"
                let hex: String = b.iter().map(|byte| format!("{byte:02x}")).collect();
                json_string_escape(&format!("0x{hex}"))
            }
            Self::Boolean(b) => if *b { "true" } else { "false" }.into(),
            Self::Null => "null".into(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// FieldDefinition
// ─────────────────────────────────────────────────────────────────────────────

/// Schema description of a single column in a feature table.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldDefinition {
    /// Column name.
    pub name: String,
    /// Declared column type.
    pub field_type: FieldType,
    /// `true` when a NOT NULL constraint is present.
    pub not_null: bool,
    /// `true` when this column is (part of) the primary key.
    pub primary_key: bool,
    /// Optional DEFAULT expression as a raw SQL string.
    pub default_value: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// CoordDim and Point4D
// ─────────────────────────────────────────────────────────────────────────────

/// Coordinate dimensionality derived from a WKB type code.
///
/// Used internally to select the correct coordinate reader during WKB parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordDim {
    /// 2D: X and Y only.
    XY,
    /// 3D: X, Y, and Z (elevation / height).
    XYZ,
    /// 3D: X, Y, and M (measure / linear reference).
    XYM,
    /// 4D: X, Y, Z (elevation), and M (measure).
    XYZM,
}

/// A 4D coordinate carrying optional Z and M values.
///
/// Created by the WKB parser for ZM geometries; Z and M are `Some` only when
/// the source WKB type includes those dimensions.
#[derive(Debug, Clone, PartialEq)]
pub struct Point4D {
    /// X coordinate (longitude / easting).
    pub x: f64,
    /// Y coordinate (latitude / northing).
    pub y: f64,
    /// Optional Z coordinate (elevation / height).
    pub z: Option<f64>,
    /// Optional M coordinate (measure / linear reference value).
    pub m: Option<f64>,
}

impl Point4D {
    /// Project to a 2D (x, y) tuple, discarding Z and M.
    pub fn to_xy(&self) -> (f64, f64) {
        (self.x, self.y)
    }

    /// Project to a 3D (x, y, z) tuple, substituting 0.0 when Z is absent.
    pub fn to_xyz(&self) -> (f64, f64, f64) {
        (self.x, self.y, self.z.unwrap_or(0.0))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// GpkgGeometry
// ─────────────────────────────────────────────────────────────────────────────

/// A decoded GeoPackage geometry value.
///
/// Coordinates are always (x, y) pairs — typically (longitude, latitude) for
/// geographic SRSs or (easting, northing) for projected ones.
/// Z variants carry an additional elevation / height coordinate per vertex.
#[derive(Debug, Clone, PartialEq)]
pub enum GpkgGeometry {
    /// A single point.
    Point {
        /// X coordinate (longitude / easting).
        x: f64,
        /// Y coordinate (latitude / northing).
        y: f64,
    },
    /// An ordered sequence of points forming a line.
    LineString {
        /// Coordinate pairs along the line.
        coords: Vec<(f64, f64)>,
    },
    /// A polygon defined by one exterior ring and zero or more interior rings.
    Polygon {
        /// Rings: index 0 is the exterior ring; subsequent entries are holes.
        rings: Vec<Vec<(f64, f64)>>,
    },
    /// A collection of points.
    MultiPoint {
        /// Individual point coordinates.
        points: Vec<(f64, f64)>,
    },
    /// A collection of line strings.
    MultiLineString {
        /// Individual line strings, each as a coordinate sequence.
        lines: Vec<Vec<(f64, f64)>>,
    },
    /// A collection of polygons.
    MultiPolygon {
        /// Individual polygons, each as a list of rings.
        polygons: Vec<Vec<Vec<(f64, f64)>>>,
    },
    /// A heterogeneous collection of geometries.
    GeometryCollection(Vec<GpkgGeometry>),
    /// A single 3D point with Z coordinate.
    PointZ {
        /// X coordinate (longitude / easting).
        x: f64,
        /// Y coordinate (latitude / northing).
        y: f64,
        /// Z coordinate (elevation / height).
        z: f64,
    },
    /// An ordered sequence of 3D points forming a line.
    LineStringZ {
        /// (x, y, z) coordinate triples along the line.
        coords: Vec<(f64, f64, f64)>,
    },
    /// A 3D polygon defined by one exterior ring and zero or more interior rings.
    PolygonZ {
        /// Rings of (x, y, z) triples; index 0 is the exterior ring.
        rings: Vec<Vec<(f64, f64, f64)>>,
    },
    /// A collection of 3D points.
    MultiPointZ {
        /// Individual (x, y, z) point coordinates.
        points: Vec<(f64, f64, f64)>,
    },
    /// A collection of 3D line strings.
    MultiLineStringZ {
        /// Individual line strings, each as a (x, y, z) sequence.
        lines: Vec<Vec<(f64, f64, f64)>>,
    },
    /// A collection of 3D polygons.
    MultiPolygonZ {
        /// Individual 3D polygons, each as a list of (x, y, z) rings.
        polygons: Vec<Vec<Vec<(f64, f64, f64)>>>,
    },
    /// A heterogeneous collection of geometries (may include Z variants).
    GeometryCollectionZ(Vec<GpkgGeometry>),
    /// A single XYM point (x, y, m — measure coordinate).
    PointM {
        /// X coordinate.
        x: f64,
        /// Y coordinate.
        y: f64,
        /// M (measure / linear reference) coordinate.
        m: f64,
    },
    /// An ordered sequence of XYM points forming a line.
    LineStringM {
        /// (x, y, m) coordinate triples along the line.
        coords: Vec<(f64, f64, f64)>,
    },
    /// An XYM polygon (exterior ring + optional interior rings).
    PolygonM {
        /// Rings of (x, y, m) triples; index 0 is the exterior ring.
        rings: Vec<Vec<(f64, f64, f64)>>,
    },
    /// A collection of XYM points.
    MultiPointM {
        /// Individual (x, y, m) point coordinates.
        points: Vec<(f64, f64, f64)>,
    },
    /// A collection of XYM line strings.
    MultiLineStringM {
        /// Individual line strings, each as an (x, y, m) sequence.
        lines: Vec<Vec<(f64, f64, f64)>>,
    },
    /// A collection of XYM polygons.
    MultiPolygonM {
        /// Individual XYM polygons, each as a list of (x, y, m) rings.
        polygons: Vec<Vec<Vec<(f64, f64, f64)>>>,
    },
    /// A heterogeneous collection of geometries that may include M variants.
    GeometryCollectionM(Vec<GpkgGeometry>),
    /// A single XYZM point (x, y, z, m).
    PointZM(Point4D),
    /// An ordered sequence of XYZM points forming a line.
    LineStringZM {
        /// [`Point4D`] vertices along the line.
        coords: Vec<Point4D>,
    },
    /// An XYZM polygon (exterior ring + optional interior rings).
    PolygonZM {
        /// Rings of [`Point4D`] vertices; index 0 is the exterior ring.
        rings: Vec<Vec<Point4D>>,
    },
    /// A collection of XYZM points.
    MultiPointZM {
        /// Individual [`Point4D`] coordinates.
        points: Vec<Point4D>,
    },
    /// A collection of XYZM line strings.
    MultiLineStringZM {
        /// Individual line strings, each as a [`Point4D`] sequence.
        lines: Vec<Vec<Point4D>>,
    },
    /// A collection of XYZM polygons.
    MultiPolygonZM {
        /// Individual XYZM polygons, each as a list of [`Point4D`] rings.
        polygons: Vec<Vec<Vec<Point4D>>>,
    },
    /// A heterogeneous collection of geometries that may include ZM variants.
    GeometryCollectionZM(Vec<GpkgGeometry>),
    /// An explicitly empty geometry (GeoPackage envelope-indicator = 0, empty flag set).
    Empty,
}

impl GpkgGeometry {
    /// Return the OGC geometry-type name.
    pub fn geometry_type(&self) -> &'static str {
        match self {
            Self::Point { .. } => "Point",
            Self::LineString { .. } => "LineString",
            Self::Polygon { .. } => "Polygon",
            Self::MultiPoint { .. } => "MultiPoint",
            Self::MultiLineString { .. } => "MultiLineString",
            Self::MultiPolygon { .. } => "MultiPolygon",
            Self::GeometryCollection(_) => "GeometryCollection",
            Self::PointZ { .. } => "PointZ",
            Self::LineStringZ { .. } => "LineStringZ",
            Self::PolygonZ { .. } => "PolygonZ",
            Self::MultiPointZ { .. } => "MultiPointZ",
            Self::MultiLineStringZ { .. } => "MultiLineStringZ",
            Self::MultiPolygonZ { .. } => "MultiPolygonZ",
            Self::GeometryCollectionZ(_) => "GeometryCollectionZ",
            Self::PointM { .. } => "PointM",
            Self::LineStringM { .. } => "LineStringM",
            Self::PolygonM { .. } => "PolygonM",
            Self::MultiPointM { .. } => "MultiPointM",
            Self::MultiLineStringM { .. } => "MultiLineStringM",
            Self::MultiPolygonM { .. } => "MultiPolygonM",
            Self::GeometryCollectionM(_) => "GeometryCollectionM",
            Self::PointZM(_) => "PointZM",
            Self::LineStringZM { .. } => "LineStringZM",
            Self::PolygonZM { .. } => "PolygonZM",
            Self::MultiPointZM { .. } => "MultiPointZM",
            Self::MultiLineStringZM { .. } => "MultiLineStringZM",
            Self::MultiPolygonZM { .. } => "MultiPolygonZM",
            Self::GeometryCollectionZM(_) => "GeometryCollectionZM",
            Self::Empty => "Empty",
        }
    }

    /// Return the total number of coordinate points in this geometry.
    pub fn point_count(&self) -> usize {
        match self {
            Self::Point { .. } | Self::PointZ { .. } | Self::PointM { .. } | Self::PointZM(_) => 1,
            Self::LineString { coords } => coords.len(),
            Self::LineStringZ { coords } => coords.len(),
            Self::LineStringM { coords } => coords.len(),
            Self::LineStringZM { coords } => coords.len(),
            Self::Polygon { rings } => rings.iter().map(|r| r.len()).sum(),
            Self::PolygonZ { rings } => rings.iter().map(|r| r.len()).sum(),
            Self::PolygonM { rings } => rings.iter().map(|r| r.len()).sum(),
            Self::PolygonZM { rings } => rings.iter().map(|r| r.len()).sum(),
            Self::MultiPoint { points } => points.len(),
            Self::MultiPointZ { points } => points.len(),
            Self::MultiPointM { points } => points.len(),
            Self::MultiPointZM { points } => points.len(),
            Self::MultiLineString { lines } => lines.iter().map(|l| l.len()).sum(),
            Self::MultiLineStringZ { lines } => lines.iter().map(|l| l.len()).sum(),
            Self::MultiLineStringM { lines } => lines.iter().map(|l| l.len()).sum(),
            Self::MultiLineStringZM { lines } => lines.iter().map(|l| l.len()).sum(),
            Self::MultiPolygon { polygons } => polygons
                .iter()
                .flat_map(|poly| poly.iter())
                .map(|ring| ring.len())
                .sum(),
            Self::MultiPolygonZ { polygons } => polygons
                .iter()
                .flat_map(|poly| poly.iter())
                .map(|ring| ring.len())
                .sum(),
            Self::MultiPolygonM { polygons } => polygons
                .iter()
                .flat_map(|poly| poly.iter())
                .map(|ring| ring.len())
                .sum(),
            Self::MultiPolygonZM { polygons } => polygons
                .iter()
                .flat_map(|poly| poly.iter())
                .map(|ring| ring.len())
                .sum(),
            Self::GeometryCollection(geoms)
            | Self::GeometryCollectionZ(geoms)
            | Self::GeometryCollectionM(geoms)
            | Self::GeometryCollectionZM(geoms) => geoms.iter().map(|g| g.point_count()).sum(),
            Self::Empty => 0,
        }
    }

    /// Return the axis-aligned bounding box `(min_x, min_y, max_x, max_y)`, or
    /// `None` for empty / zero-point geometries.
    pub fn bbox(&self) -> Option<(f64, f64, f64, f64)> {
        let coords: Vec<(f64, f64)> = self.collect_coords();
        if coords.is_empty() {
            return None;
        }
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for (x, y) in &coords {
            if *x < min_x {
                min_x = *x;
            }
            if *y < min_y {
                min_y = *y;
            }
            if *x > max_x {
                max_x = *x;
            }
            if *y > max_y {
                max_y = *y;
            }
        }
        if min_x.is_finite() {
            Some((min_x, min_y, max_x, max_y))
        } else {
            None
        }
    }

    /// Collect all coordinate pairs depth-first (Z/M variants project to 2D).
    fn collect_coords(&self) -> Vec<(f64, f64)> {
        match self {
            Self::Point { x, y } => vec![(*x, *y)],
            Self::PointZ { x, y, .. } => vec![(*x, *y)],
            Self::PointM { x, y, .. } => vec![(*x, *y)],
            Self::PointZM(p) => vec![(p.x, p.y)],
            Self::LineString { coords } => coords.clone(),
            Self::LineStringZ { coords } => coords.iter().map(|(x, y, _)| (*x, *y)).collect(),
            Self::LineStringM { coords } => coords.iter().map(|(x, y, _)| (*x, *y)).collect(),
            Self::LineStringZM { coords } => coords.iter().map(|p| (p.x, p.y)).collect(),
            Self::Polygon { rings } => rings.iter().flatten().copied().collect(),
            Self::PolygonZ { rings } => rings.iter().flatten().map(|(x, y, _)| (*x, *y)).collect(),
            Self::PolygonM { rings } => rings.iter().flatten().map(|(x, y, _)| (*x, *y)).collect(),
            Self::PolygonZM { rings } => rings.iter().flatten().map(|p| (p.x, p.y)).collect(),
            Self::MultiPoint { points } => points.clone(),
            Self::MultiPointZ { points } => points.iter().map(|(x, y, _)| (*x, *y)).collect(),
            Self::MultiPointM { points } => points.iter().map(|(x, y, _)| (*x, *y)).collect(),
            Self::MultiPointZM { points } => points.iter().map(|p| (p.x, p.y)).collect(),
            Self::MultiLineString { lines } => lines.iter().flatten().copied().collect(),
            Self::MultiLineStringZ { lines } => {
                lines.iter().flatten().map(|(x, y, _)| (*x, *y)).collect()
            }
            Self::MultiLineStringM { lines } => {
                lines.iter().flatten().map(|(x, y, _)| (*x, *y)).collect()
            }
            Self::MultiLineStringZM { lines } => {
                lines.iter().flatten().map(|p| (p.x, p.y)).collect()
            }
            Self::MultiPolygon { polygons } => polygons
                .iter()
                .flat_map(|poly| poly.iter().flatten())
                .copied()
                .collect(),
            Self::MultiPolygonZ { polygons } => polygons
                .iter()
                .flat_map(|poly| poly.iter().flatten())
                .map(|(x, y, _)| (*x, *y))
                .collect(),
            Self::MultiPolygonM { polygons } => polygons
                .iter()
                .flat_map(|poly| poly.iter().flatten())
                .map(|(x, y, _)| (*x, *y))
                .collect(),
            Self::MultiPolygonZM { polygons } => polygons
                .iter()
                .flat_map(|poly| poly.iter().flatten())
                .map(|p| (p.x, p.y))
                .collect(),
            Self::GeometryCollection(geoms)
            | Self::GeometryCollectionZ(geoms)
            | Self::GeometryCollectionM(geoms)
            | Self::GeometryCollectionZM(geoms) => {
                geoms.iter().flat_map(|g| g.collect_coords()).collect()
            }
            Self::Empty => vec![],
        }
    }

    /// Serialise this geometry as a GeoJSON geometry object string.
    ///
    /// Z variants emit `[x,y,z]` coordinate arrays per RFC 7946.
    pub(crate) fn to_geojson_geometry(&self) -> String {
        match self {
            Self::Point { x, y } => {
                format!(r#"{{"type":"Point","coordinates":[{x},{y}]}}"#)
            }
            Self::PointZ { x, y, z } => {
                format!(r#"{{"type":"Point","coordinates":[{x},{y},{z}]}}"#)
            }
            Self::LineString { coords } => {
                let pts = coords_to_json_array(coords);
                format!(r#"{{"type":"LineString","coordinates":{pts}}}"#)
            }
            Self::LineStringZ { coords } => {
                let pts = coords_z_to_json_array(coords);
                format!(r#"{{"type":"LineString","coordinates":{pts}}}"#)
            }
            Self::Polygon { rings } => {
                let rings_json = rings
                    .iter()
                    .map(|r| coords_to_json_array(r))
                    .collect::<Vec<_>>()
                    .join(",");
                format!(r#"{{"type":"Polygon","coordinates":[{rings_json}]}}"#)
            }
            Self::PolygonZ { rings } => {
                let rings_json = rings
                    .iter()
                    .map(|r| coords_z_to_json_array(r))
                    .collect::<Vec<_>>()
                    .join(",");
                format!(r#"{{"type":"Polygon","coordinates":[{rings_json}]}}"#)
            }
            Self::MultiPoint { points } => {
                let pts = coords_to_json_array(points);
                format!(r#"{{"type":"MultiPoint","coordinates":{pts}}}"#)
            }
            Self::MultiPointZ { points } => {
                let pts = coords_z_to_json_array(points);
                format!(r#"{{"type":"MultiPoint","coordinates":{pts}}}"#)
            }
            Self::MultiLineString { lines } => {
                let lines_json = lines
                    .iter()
                    .map(|l| coords_to_json_array(l))
                    .collect::<Vec<_>>()
                    .join(",");
                format!(r#"{{"type":"MultiLineString","coordinates":[{lines_json}]}}"#)
            }
            Self::MultiLineStringZ { lines } => {
                let lines_json = lines
                    .iter()
                    .map(|l| coords_z_to_json_array(l))
                    .collect::<Vec<_>>()
                    .join(",");
                format!(r#"{{"type":"MultiLineString","coordinates":[{lines_json}]}}"#)
            }
            Self::MultiPolygon { polygons } => {
                let polys_json = polygons
                    .iter()
                    .map(|poly| {
                        let rings_json = poly
                            .iter()
                            .map(|r| coords_to_json_array(r))
                            .collect::<Vec<_>>()
                            .join(",");
                        format!("[{rings_json}]")
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                format!(r#"{{"type":"MultiPolygon","coordinates":[{polys_json}]}}"#)
            }
            Self::MultiPolygonZ { polygons } => {
                let polys_json = polygons
                    .iter()
                    .map(|poly| {
                        let rings_json = poly
                            .iter()
                            .map(|r| coords_z_to_json_array(r))
                            .collect::<Vec<_>>()
                            .join(",");
                        format!("[{rings_json}]")
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                format!(r#"{{"type":"MultiPolygon","coordinates":[{polys_json}]}}"#)
            }
            Self::GeometryCollection(geoms)
            | Self::GeometryCollectionZ(geoms)
            | Self::GeometryCollectionM(geoms)
            | Self::GeometryCollectionZM(geoms) => {
                let geom_json = geoms
                    .iter()
                    .map(|g| g.to_geojson_geometry())
                    .collect::<Vec<_>>()
                    .join(",");
                format!(r#"{{"type":"GeometryCollection","geometries":[{geom_json}]}}"#)
            }
            // M variants: GeoJSON does not have an M dimension, so we emit XY only.
            Self::PointM { x, y, .. } => {
                format!(r#"{{"type":"Point","coordinates":[{x},{y}]}}"#)
            }
            Self::LineStringM { coords } => {
                let pts = coords_to_json_array(
                    &coords.iter().map(|(x, y, _)| (*x, *y)).collect::<Vec<_>>(),
                );
                format!(r#"{{"type":"LineString","coordinates":{pts}}}"#)
            }
            Self::PolygonM { rings } => {
                let rings_json = rings
                    .iter()
                    .map(|r| {
                        coords_to_json_array(
                            &r.iter().map(|(x, y, _)| (*x, *y)).collect::<Vec<_>>(),
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                format!(r#"{{"type":"Polygon","coordinates":[{rings_json}]}}"#)
            }
            Self::MultiPointM { points } => {
                let pts = coords_to_json_array(
                    &points.iter().map(|(x, y, _)| (*x, *y)).collect::<Vec<_>>(),
                );
                format!(r#"{{"type":"MultiPoint","coordinates":{pts}}}"#)
            }
            Self::MultiLineStringM { lines } => {
                let lines_json = lines
                    .iter()
                    .map(|l| {
                        coords_to_json_array(
                            &l.iter().map(|(x, y, _)| (*x, *y)).collect::<Vec<_>>(),
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                format!(r#"{{"type":"MultiLineString","coordinates":[{lines_json}]}}"#)
            }
            Self::MultiPolygonM { polygons } => {
                let polys_json = polygons
                    .iter()
                    .map(|poly| {
                        let rings_json = poly
                            .iter()
                            .map(|r| {
                                coords_to_json_array(
                                    &r.iter().map(|(x, y, _)| (*x, *y)).collect::<Vec<_>>(),
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(",");
                        format!("[{rings_json}]")
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                format!(r#"{{"type":"MultiPolygon","coordinates":[{polys_json}]}}"#)
            }
            // ZM variants: GeoJSON emits [x,y,z], M is dropped per RFC 7946.
            Self::PointZM(p) => {
                let (x, y, z) = p.to_xyz();
                format!(r#"{{"type":"Point","coordinates":[{x},{y},{z}]}}"#)
            }
            Self::LineStringZM { coords } => {
                let pts =
                    coords_z_to_json_array(&coords.iter().map(|p| p.to_xyz()).collect::<Vec<_>>());
                format!(r#"{{"type":"LineString","coordinates":{pts}}}"#)
            }
            Self::PolygonZM { rings } => {
                let rings_json = rings
                    .iter()
                    .map(|r| {
                        coords_z_to_json_array(&r.iter().map(|p| p.to_xyz()).collect::<Vec<_>>())
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                format!(r#"{{"type":"Polygon","coordinates":[{rings_json}]}}"#)
            }
            Self::MultiPointZM { points } => {
                let pts =
                    coords_z_to_json_array(&points.iter().map(|p| p.to_xyz()).collect::<Vec<_>>());
                format!(r#"{{"type":"MultiPoint","coordinates":{pts}}}"#)
            }
            Self::MultiLineStringZM { lines } => {
                let lines_json = lines
                    .iter()
                    .map(|l| {
                        coords_z_to_json_array(&l.iter().map(|p| p.to_xyz()).collect::<Vec<_>>())
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                format!(r#"{{"type":"MultiLineString","coordinates":[{lines_json}]}}"#)
            }
            Self::MultiPolygonZM { polygons } => {
                let polys_json = polygons
                    .iter()
                    .map(|poly| {
                        let rings_json = poly
                            .iter()
                            .map(|r| {
                                coords_z_to_json_array(
                                    &r.iter().map(|p| p.to_xyz()).collect::<Vec<_>>(),
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(",");
                        format!("[{rings_json}]")
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                format!(r#"{{"type":"MultiPolygon","coordinates":[{polys_json}]}}"#)
            }
            Self::Empty => "null".into(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// JSON helper utilities (used by GpkgGeometry and FeatureTable)
// ─────────────────────────────────────────────────────────────────────────────

/// Escape a string for use as a JSON string value (including the surrounding quotes).
pub(crate) fn json_string_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Render a coordinate sequence as a JSON array of `[x,y]` arrays.
pub(crate) fn coords_to_json_array(coords: &[(f64, f64)]) -> String {
    let inner: String = coords
        .iter()
        .map(|(x, y)| format!("[{x},{y}]"))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{inner}]")
}

/// Render a 3D coordinate sequence as a JSON array of `[x,y,z]` arrays.
pub(crate) fn coords_z_to_json_array(coords: &[(f64, f64, f64)]) -> String {
    let inner: String = coords
        .iter()
        .map(|(x, y, z)| format!("[{x},{y},{z}]"))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{inner}]")
}
