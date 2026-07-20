//! Shapefile reader - coordinates reading from .shp, .dbf, and .shx files
//!
//! This module provides a high-level interface for reading Shapefiles,
//! combining geometry from .shp, attributes from .dbf, and spatial index from .shx.

use crate::dbf::{DbfReader, FieldDescriptor, MemoFile};
use crate::error::{Result, ShapefileError};
use crate::shp::{Shape, ShapefileHeader, ShpReader};
use crate::shx::{IndexEntry, ShxReader};
use oxigeo_core::vector::{
    Coordinate, Feature, FieldValue, Geometry, LineString as CoreLineString,
    MultiLineString as CoreMultiLineString, MultiPoint as CoreMultiPoint, Point as CorePoint,
    Polygon as CorePolygon,
};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

/// A complete Shapefile feature (geometry + attributes)
#[derive(Debug, Clone)]
pub struct ShapefileFeature {
    /// Record number (1-based)
    pub record_number: i32,
    /// Geometry
    pub geometry: Option<Geometry>,
    /// Attributes (field name -> value)
    pub attributes: HashMap<String, FieldValue>,
}

impl ShapefileFeature {
    /// Creates a new Shapefile feature
    pub fn new(
        record_number: i32,
        geometry: Option<Geometry>,
        attributes: HashMap<String, FieldValue>,
    ) -> Self {
        Self {
            record_number,
            geometry,
            attributes,
        }
    }

    /// Converts to an OxiGeo Feature
    pub fn to_oxigeo_feature(&self) -> Result<Feature> {
        let geometry = self
            .geometry
            .clone()
            .ok_or_else(|| ShapefileError::invalid_geometry("feature has no geometry"))?;

        let mut feature = Feature::new(geometry);

        // Convert attributes
        for (key, value) in &self.attributes {
            feature.set_property(key, value.clone());
        }

        Ok(feature)
    }
}

/// Shapefile reader that coordinates .shp, .dbf, and optionally .shx files
pub struct ShapefileReader {
    /// Base path (without extension)
    base_path: PathBuf,
    /// .shp file header
    header: ShapefileHeader,
    /// Field descriptors from .dbf
    field_descriptors: Vec<FieldDescriptor>,
    /// Index entries from .shx (if available)
    index_entries: Option<Vec<IndexEntry>>,
    /// CRS as WKT string from .prj file (if present)
    pub crs: Option<String>,
    /// Character encoding label from .cpg file (if present)
    pub encoding: Option<String>,
    /// Resolved encoding used to transcode DBF text fields.
    ///
    /// Chosen by priority: an explicit override → the `.cpg` label → the DBF
    /// header code page (LDID byte) → UTF-8.
    resolved_encoding: &'static ::encoding_rs::Encoding,
}

impl ShapefileReader {
    /// Opens a Shapefile from a base path (without extension)
    ///
    /// Reads the .shp, .dbf, and optionally .shx, .prj, and .cpg files. The DBF
    /// text encoding is auto-detected from the `.cpg` file or the DBF header
    /// code page; use [`ShapefileReader::open_with_encoding`] to force one.
    pub fn open<P: AsRef<Path>>(base_path: P) -> Result<Self> {
        Self::open_impl(base_path.as_ref(), None)
    }

    /// Opens a Shapefile, forcing a specific DBF text encoding.
    ///
    /// `encoding_label` is any label `encoding_rs` accepts (e.g. `"UTF-8"`,
    /// `"windows-1252"`, `"GBK"`) or a numeric code page (e.g. `"1252"`,
    /// `"65001"`). This overrides both the `.cpg` file and the header code page,
    /// which is useful when those are absent or wrong.
    ///
    /// # Errors
    ///
    /// Returns an error if `encoding_label` is not a recognised encoding, or if
    /// the shapefile cannot be opened.
    pub fn open_with_encoding<P: AsRef<Path>>(base_path: P, encoding_label: &str) -> Result<Self> {
        let encoding = crate::dbf::resolve_cpg(encoding_label).ok_or_else(|| {
            ShapefileError::encoding_error(format!("unknown encoding label: '{encoding_label}'"))
        })?;
        Self::open_impl(base_path.as_ref(), Some(encoding))
    }

    fn open_impl(
        base_path: &Path,
        encoding_override: Option<&'static ::encoding_rs::Encoding>,
    ) -> Result<Self> {
        // Construct file paths
        let shp_path = Self::with_extension(base_path, "shp");
        let dbf_path = Self::with_extension(base_path, "dbf");
        let shx_path = Self::with_extension(base_path, "shx");
        let prj_path = Self::with_extension(base_path, "prj");
        let cpg_path = Self::with_extension(base_path, "cpg");

        // Open .shp file
        let shp_file = File::open(&shp_path).map_err(|_| ShapefileError::MissingFile {
            file_type: ".shp".to_string(),
        })?;
        let shp_reader = BufReader::new(shp_file);
        let shp_reader = ShpReader::new(shp_reader)?;
        let header = shp_reader.header().clone();

        // Open .dbf file
        let dbf_file = File::open(&dbf_path).map_err(|_| ShapefileError::MissingFile {
            file_type: ".dbf".to_string(),
        })?;
        let dbf_reader = BufReader::new(dbf_file);
        let mut dbf_reader = DbfReader::new(dbf_reader)?;
        let header_code_page = dbf_reader.header().code_page;
        // Discover and attach the sibling memo file (`.dbt` / `.DBT`) if present.
        if let Some(memo_path) = Self::discover_memo_path(&dbf_path)
            && let Ok(memo) = MemoFile::open(&memo_path)
        {
            dbf_reader.set_memo_file(memo);
        }
        let field_descriptors = dbf_reader.field_descriptors().to_vec();

        // Open .shx file (optional)
        let index_entries = if shx_path.exists() {
            let shx_file = File::open(&shx_path).ok();
            if let Some(file) = shx_file {
                let shx_reader = BufReader::new(file);
                let mut shx_reader = ShxReader::new(shx_reader)?;
                Some(shx_reader.read_all_entries()?)
            } else {
                None
            }
        } else {
            None
        };

        // Read .prj file (optional) — contains CRS as WKT string
        let crs = if prj_path.exists() {
            std::fs::read_to_string(&prj_path)
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        } else {
            None
        };

        // Read .cpg file (optional) — contains the character-encoding label.
        let encoding = if cpg_path.exists() {
            std::fs::read_to_string(&cpg_path)
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        } else {
            None
        };

        // Resolve the DBF text encoding: explicit override → .cpg label →
        // DBF header code page (LDID) → UTF-8.
        let resolved_encoding = encoding_override
            .or_else(|| encoding.as_deref().and_then(crate::dbf::resolve_cpg))
            .or_else(|| crate::dbf::resolve_ldid(header_code_page))
            .unwrap_or(::encoding_rs::UTF_8);

        Ok(Self {
            base_path: base_path.to_path_buf(),
            header,
            field_descriptors,
            index_entries,
            crs,
            encoding,
            resolved_encoding,
        })
    }

    /// Returns the Shapefile header
    pub fn header(&self) -> &ShapefileHeader {
        &self.header
    }

    /// Returns the field descriptors
    pub fn field_descriptors(&self) -> &[FieldDescriptor] {
        &self.field_descriptors
    }

    /// Returns the index entries (if .shx was loaded)
    pub fn index_entries(&self) -> Option<&[IndexEntry]> {
        self.index_entries.as_deref()
    }

    /// Returns the CRS as a WKT string, if a .prj file was present
    pub fn crs(&self) -> Option<&str> {
        self.crs.as_deref()
    }

    /// Returns the character encoding label from the .cpg file, if present.
    ///
    /// Common values: `"UTF-8"`, `"CP1252"`, `"ISO-8859-1"`. This is the raw
    /// label; for the encoding actually used to decode DBF text (which also
    /// considers the DBF header code page and any override) see
    /// [`ShapefileReader::resolved_encoding`].
    pub fn encoding(&self) -> Option<&str> {
        self.encoding.as_deref()
    }

    /// Returns the encoding used to transcode DBF text fields.
    ///
    /// Resolved by priority: an explicit override (see
    /// [`ShapefileReader::open_with_encoding`]) → the `.cpg` label → the DBF
    /// header code page → UTF-8. Use `.name()` for its canonical label.
    pub fn resolved_encoding(&self) -> &'static ::encoding_rs::Encoding {
        self.resolved_encoding
    }

    /// Returns features whose bounding box intersects the given query bbox.
    ///
    /// Reads all shape records from `.shp` and `.dbf`, then filters to those
    /// whose bounding box overlaps `[min_x, min_y, max_x, max_y]` (inclusive).
    /// Point shapes use the point coordinate as a degenerate bbox.
    /// Null shapes are excluded.
    ///
    /// For large shapefiles this reads all geometry upfront; a full R-tree
    /// spatial index backed by lazy `.shx` seeks is a follow-up item.
    pub fn features_in_bbox(
        &mut self,
        min_x: f64,
        min_y: f64,
        max_x: f64,
        max_y: f64,
    ) -> Result<Vec<ShapefileFeature>> {
        let all_features = self.read_features()?;

        let filtered = all_features
            .into_iter()
            .filter(|feature| {
                let Some(ref geom) = feature.geometry else {
                    return false;
                };
                if let Some((fx_min, fy_min, fx_max, fy_max)) = Self::geometry_bbox(geom) {
                    // Standard AABB intersection test (inclusive on edges)
                    !(fx_max < min_x || fx_min > max_x || fy_max < min_y || fy_min > max_y)
                } else {
                    false
                }
            })
            .collect();

        Ok(filtered)
    }

    /// Extracts a 2-D bounding box `(x_min, y_min, x_max, y_max)` from a geometry.
    ///
    /// Delegates to the `Geometry::bounds()` method defined in `oxigeo-core`,
    /// which returns `None` for degenerate or empty geometries.
    fn geometry_bbox(geom: &Geometry) -> Option<(f64, f64, f64, f64)> {
        geom.bounds()
    }

    /// Returns a streaming iterator over the Shapefile's features.
    ///
    /// Unlike [`read_features`], which loads everything into memory, this opens
    /// fresh buffered readers and reads one SHP record + one DBF record per
    /// [`Iterator::next`] call.  Memory usage is therefore O(1) with respect
    /// to the number of features.
    ///
    /// # Errors
    ///
    /// Returns an error if the `.shp` or `.dbf` files cannot be opened, or if
    /// the header/field-descriptor section cannot be read.  Individual record
    /// errors are surfaced as `Err` items from the iterator.
    ///
    /// [`read_features`]: ShapefileReader::read_features
    pub fn iter_features(&self) -> Result<FeatureIter<'_>> {
        let shp_path = Self::with_extension(&self.base_path, "shp");
        let dbf_path = Self::with_extension(&self.base_path, "dbf");

        let shp_file = File::open(&shp_path)?;
        let shp_reader = BufReader::new(shp_file);
        let shp_reader = ShpReader::new(shp_reader)?;

        let dbf_file = File::open(&dbf_path)?;
        let dbf_reader = BufReader::new(dbf_file);
        let mut dbf_reader = DbfReader::new(dbf_reader)?;
        // Re-attach the sibling memo file so streamed records dereference memos.
        if let Some(memo_path) = Self::discover_memo_path(&dbf_path)
            && let Ok(memo) = MemoFile::open(&memo_path)
        {
            dbf_reader.set_memo_file(memo);
        }
        dbf_reader.set_encoding(self.resolved_encoding);

        Ok(FeatureIter {
            shp_reader,
            dbf_reader,
            field_descriptors: &self.field_descriptors,
            done: false,
        })
    }

    /// Reads features that satisfy an arbitrary predicate closure.
    ///
    /// This is a convenience wrapper over [`ShapefileReader::read_features`] that filters the
    /// result set in-place without a second allocation.  The predicate receives
    /// a shared reference to each [`ShapefileFeature`] and returns `true` for
    /// features that should be included in the output.
    ///
    /// For structured attribute comparisons prefer
    /// [`read_features_filtered`](ShapefileReader::read_features_filtered),
    /// which accepts a [`crate::filter::FieldFilter`] directly.
    ///
    /// # Errors
    ///
    /// Propagates any I/O or parse errors from the underlying read.
    pub fn read_features_where<F>(&self, predicate: F) -> Result<Vec<ShapefileFeature>>
    where
        F: Fn(&ShapefileFeature) -> bool,
    {
        let all = self.read_features()?;
        Ok(all.into_iter().filter(|f| predicate(f)).collect())
    }

    /// Reads features that match a structured [`crate::filter::FieldFilter`].
    ///
    /// This is a thin convenience method over
    /// [`read_features_where`](ShapefileReader::read_features_where) that
    /// accepts a [`crate::filter::FieldFilter`] directly.
    ///
    /// # Errors
    ///
    /// Propagates any I/O or parse errors from the underlying read.
    pub fn read_features_filtered(
        &self,
        filter: &crate::filter::FieldFilter,
    ) -> Result<Vec<ShapefileFeature>> {
        self.read_features_where(|f| filter.matches(f))
    }

    /// Reads all features from the Shapefile
    pub fn read_features(&self) -> Result<Vec<ShapefileFeature>> {
        // Open files
        let shp_path = Self::with_extension(&self.base_path, "shp");
        let dbf_path = Self::with_extension(&self.base_path, "dbf");

        let shp_file = File::open(&shp_path)?;
        let shp_reader = BufReader::new(shp_file);
        let mut shp_reader = ShpReader::new(shp_reader)?;

        let dbf_file = File::open(&dbf_path)?;
        let dbf_reader = BufReader::new(dbf_file);
        let mut dbf_reader = DbfReader::new(dbf_reader)?;
        // Re-attach the sibling memo file so batch-read records dereference memos.
        if let Some(memo_path) = Self::discover_memo_path(&dbf_path)
            && let Ok(memo) = MemoFile::open(&memo_path)
        {
            dbf_reader.set_memo_file(memo);
        }
        dbf_reader.set_encoding(self.resolved_encoding);

        // Read all shape records
        let shape_records = shp_reader.read_all_records()?;

        // Read all DBF records
        let dbf_records = dbf_reader.read_all_records()?;

        // Verify record counts match
        if shape_records.len() != dbf_records.len() {
            return Err(ShapefileError::RecordMismatch {
                shp_count: shape_records.len(),
                dbf_count: dbf_records.len(),
            });
        }

        // Combine into features
        let mut features = Vec::with_capacity(shape_records.len());
        for (shape_record, dbf_record) in shape_records.iter().zip(dbf_records.iter()) {
            let geometry = Self::shape_to_geometry(&shape_record.shape)?;

            // Convert DBF record to attributes
            let attributes = Self::dbf_to_attributes(dbf_record, &self.field_descriptors);

            features.push(ShapefileFeature::new(
                shape_record.record_number,
                geometry,
                attributes,
            ));
        }

        Ok(features)
    }

    /// Converts a Shape to an OxiGeo Geometry
    fn shape_to_geometry(shape: &Shape) -> Result<Option<Geometry>> {
        match shape {
            Shape::Null => Ok(None),
            Shape::Point(point) => {
                let oxigeo_point = CorePoint::new(point.x, point.y);
                Ok(Some(Geometry::Point(oxigeo_point)))
            }
            Shape::PointZ(point) => {
                use oxigeo_core::vector::Coordinate;
                let coord = if let Some(m) = point.m {
                    Coordinate::new_3dm(point.x, point.y, point.z, m)
                } else {
                    Coordinate::new_3d(point.x, point.y, point.z)
                };
                Ok(Some(Geometry::Point(CorePoint::from_coord(coord))))
            }
            Shape::PointM(point) => {
                use oxigeo_core::vector::Coordinate;
                let coord = Coordinate::new_2dm(point.x, point.y, point.m);
                Ok(Some(Geometry::Point(CorePoint::from_coord(coord))))
            }
            Shape::PolyLine(multi_part) => {
                if multi_part.parts.len() == 1 {
                    // Single part - convert to LineString
                    let coords: Vec<Coordinate> = multi_part
                        .points
                        .iter()
                        .map(|p| Coordinate::new_2d(p.x, p.y))
                        .collect();

                    if coords.len() < 2 {
                        return Ok(None);
                    }

                    let linestring = CoreLineString::new(coords).map_err(|e| {
                        ShapefileError::invalid_geometry(format!("Invalid LineString: {}", e))
                    })?;
                    Ok(Some(Geometry::LineString(linestring)))
                } else {
                    // Multiple parts - convert to MultiLineString
                    let mut linestrings = Vec::new();

                    for i in 0..multi_part.parts.len() {
                        let start_idx = multi_part.parts[i] as usize;
                        let end_idx = if i + 1 < multi_part.parts.len() {
                            multi_part.parts[i + 1] as usize
                        } else {
                            multi_part.points.len()
                        };

                        let coords: Vec<Coordinate> = multi_part.points[start_idx..end_idx]
                            .iter()
                            .map(|p| Coordinate::new_2d(p.x, p.y))
                            .collect();

                        if coords.len() >= 2
                            && let Ok(linestring) = CoreLineString::new(coords)
                        {
                            linestrings.push(linestring);
                        }
                    }

                    if linestrings.is_empty() {
                        Ok(None)
                    } else {
                        Ok(Some(Geometry::MultiLineString(CoreMultiLineString::new(
                            linestrings,
                        ))))
                    }
                }
            }
            Shape::Polygon(multi_part) => {
                if multi_part.parts.is_empty() {
                    return Ok(None);
                }

                // First part is exterior ring
                let exterior_start = multi_part.parts[0] as usize;
                let exterior_end = if multi_part.parts.len() > 1 {
                    multi_part.parts[1] as usize
                } else {
                    multi_part.points.len()
                };

                let exterior_coords: Vec<Coordinate> = multi_part.points
                    [exterior_start..exterior_end]
                    .iter()
                    .map(|p| Coordinate::new_2d(p.x, p.y))
                    .collect();

                if exterior_coords.len() < 4 {
                    return Ok(None);
                }

                let exterior = CoreLineString::new(exterior_coords).map_err(|e| {
                    ShapefileError::invalid_geometry(format!("Invalid exterior ring: {}", e))
                })?;

                // Remaining parts are interior rings (holes)
                let mut interiors = Vec::new();
                for i in 1..multi_part.parts.len() {
                    let start_idx = multi_part.parts[i] as usize;
                    let end_idx = if i + 1 < multi_part.parts.len() {
                        multi_part.parts[i + 1] as usize
                    } else {
                        multi_part.points.len()
                    };

                    let interior_coords: Vec<Coordinate> = multi_part.points[start_idx..end_idx]
                        .iter()
                        .map(|p| Coordinate::new_2d(p.x, p.y))
                        .collect();

                    if interior_coords.len() >= 4
                        && let Ok(interior) = CoreLineString::new(interior_coords)
                    {
                        interiors.push(interior);
                    }
                }

                let polygon = CorePolygon::new(exterior, interiors).map_err(|e| {
                    ShapefileError::invalid_geometry(format!("Invalid polygon: {}", e))
                })?;

                Ok(Some(Geometry::Polygon(polygon)))
            }
            Shape::MultiPoint(multi_part) => {
                let points: Vec<CorePoint> = multi_part
                    .points
                    .iter()
                    .map(|p| CorePoint::new(p.x, p.y))
                    .collect();

                if points.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(Geometry::MultiPoint(CoreMultiPoint::new(points))))
                }
            }
            // Z variants: reconstruct with Z (and optionally M) coordinates
            Shape::PolyLineZ(shape_z) => Self::multipart_z_to_linestring_geometry(
                &shape_z.base,
                &shape_z.z_values,
                shape_z.m_values.as_deref(),
            ),
            Shape::PolygonZ(shape_z) => Self::multipart_z_to_polygon_geometry(
                &shape_z.base,
                &shape_z.z_values,
                shape_z.m_values.as_deref(),
            ),
            Shape::MultiPointZ(shape_z) => Self::multipart_z_to_multipoint_geometry(
                &shape_z.base,
                &shape_z.z_values,
                shape_z.m_values.as_deref(),
            ),
            // M variants: reconstruct with M coordinates
            Shape::PolyLineM(shape_m) => {
                Self::multipart_m_to_linestring_geometry(&shape_m.base, &shape_m.m_values)
            }
            Shape::PolygonM(shape_m) => {
                Self::multipart_m_to_polygon_geometry(&shape_m.base, &shape_m.m_values)
            }
            Shape::MultiPointM(shape_m) => {
                Self::multipart_m_to_multipoint_geometry(&shape_m.base, &shape_m.m_values)
            }
            // MultiPatch: expose as a MultiPolygon using ring parts (outer/inner ring types)
            // or fall back to a point collection of the vertices.
            Shape::MultiPatch(mp_shape) => {
                // Represent patch vertices as a MultiPoint carrying Z coordinates.
                use oxigeo_core::vector::Coordinate;
                let points: Vec<CorePoint> = mp_shape
                    .base
                    .points
                    .iter()
                    .zip(mp_shape.z_values.iter())
                    .map(|(p, z)| CorePoint::from_coord(Coordinate::new_3d(p.x, p.y, *z)))
                    .collect();

                if points.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(Geometry::MultiPoint(CoreMultiPoint::new(points))))
                }
            }
        }
    }

    /// Converts a multi-part Z shape into a LineString or MultiLineString Geometry
    /// with Z (and optionally M) coordinates preserved.
    fn multipart_z_to_linestring_geometry(
        base: &crate::shp::MultiPartShape,
        z_values: &[f64],
        m_values: Option<&[f64]>,
    ) -> Result<Option<Geometry>> {
        use oxigeo_core::vector::Coordinate;

        let make_coord = |i: usize, p: &crate::shp::shapes::Point| -> Coordinate {
            let z = z_values.get(i).copied().unwrap_or(0.0);
            if let Some(mv) = m_values {
                Coordinate::new_3dm(p.x, p.y, z, mv.get(i).copied().unwrap_or(0.0))
            } else {
                Coordinate::new_3d(p.x, p.y, z)
            }
        };

        if base.parts.len() == 1 {
            let coords: Vec<Coordinate> = base
                .points
                .iter()
                .enumerate()
                .map(|(i, p)| make_coord(i, p))
                .collect();
            if coords.len() < 2 {
                return Ok(None);
            }
            let linestring = CoreLineString::new(coords).map_err(|e| {
                ShapefileError::invalid_geometry(format!("Invalid LineString: {}", e))
            })?;
            Ok(Some(Geometry::LineString(linestring)))
        } else {
            let mut linestrings = Vec::new();
            for i in 0..base.parts.len() {
                let start = base.parts[i] as usize;
                let end = if i + 1 < base.parts.len() {
                    base.parts[i + 1] as usize
                } else {
                    base.points.len()
                };
                let coords: Vec<Coordinate> = base.points[start..end]
                    .iter()
                    .enumerate()
                    .map(|(j, p)| make_coord(start + j, p))
                    .collect();
                if coords.len() >= 2
                    && let Ok(ls) = CoreLineString::new(coords)
                {
                    linestrings.push(ls);
                }
            }
            if linestrings.is_empty() {
                Ok(None)
            } else {
                Ok(Some(Geometry::MultiLineString(CoreMultiLineString::new(
                    linestrings,
                ))))
            }
        }
    }

    /// Converts a multi-part Z shape into a Polygon Geometry with Z coordinates.
    fn multipart_z_to_polygon_geometry(
        base: &crate::shp::MultiPartShape,
        z_values: &[f64],
        m_values: Option<&[f64]>,
    ) -> Result<Option<Geometry>> {
        use oxigeo_core::vector::Coordinate;

        if base.parts.is_empty() {
            return Ok(None);
        }

        let make_coord = |i: usize, p: &crate::shp::shapes::Point| -> Coordinate {
            let z = z_values.get(i).copied().unwrap_or(0.0);
            if let Some(mv) = m_values {
                Coordinate::new_3dm(p.x, p.y, z, mv.get(i).copied().unwrap_or(0.0))
            } else {
                Coordinate::new_3d(p.x, p.y, z)
            }
        };

        let ext_start = base.parts[0] as usize;
        let ext_end = if base.parts.len() > 1 {
            base.parts[1] as usize
        } else {
            base.points.len()
        };
        let ext_coords: Vec<Coordinate> = base.points[ext_start..ext_end]
            .iter()
            .enumerate()
            .map(|(j, p)| make_coord(ext_start + j, p))
            .collect();
        if ext_coords.len() < 4 {
            return Ok(None);
        }
        let exterior = CoreLineString::new(ext_coords).map_err(|e| {
            ShapefileError::invalid_geometry(format!("Invalid exterior Z ring: {}", e))
        })?;

        let mut interiors = Vec::new();
        for i in 1..base.parts.len() {
            let start = base.parts[i] as usize;
            let end = if i + 1 < base.parts.len() {
                base.parts[i + 1] as usize
            } else {
                base.points.len()
            };
            let coords: Vec<Coordinate> = base.points[start..end]
                .iter()
                .enumerate()
                .map(|(j, p)| make_coord(start + j, p))
                .collect();
            if coords.len() >= 4
                && let Ok(ring) = CoreLineString::new(coords)
            {
                interiors.push(ring);
            }
        }
        let polygon = CorePolygon::new(exterior, interiors)
            .map_err(|e| ShapefileError::invalid_geometry(format!("Invalid polygon Z: {}", e)))?;
        Ok(Some(Geometry::Polygon(polygon)))
    }

    /// Converts a multi-part Z shape into a MultiPoint Geometry with Z coordinates.
    fn multipart_z_to_multipoint_geometry(
        base: &crate::shp::MultiPartShape,
        z_values: &[f64],
        m_values: Option<&[f64]>,
    ) -> Result<Option<Geometry>> {
        use oxigeo_core::vector::Coordinate;
        let points: Vec<CorePoint> = base
            .points
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let z = z_values.get(i).copied().unwrap_or(0.0);
                let coord = if let Some(mv) = m_values {
                    Coordinate::new_3dm(p.x, p.y, z, mv.get(i).copied().unwrap_or(0.0))
                } else {
                    Coordinate::new_3d(p.x, p.y, z)
                };
                CorePoint::from_coord(coord)
            })
            .collect();
        if points.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Geometry::MultiPoint(CoreMultiPoint::new(points))))
        }
    }

    /// Converts a multi-part M shape into a LineString/MultiLineString with M.
    fn multipart_m_to_linestring_geometry(
        base: &crate::shp::MultiPartShape,
        m_values: &[f64],
    ) -> Result<Option<Geometry>> {
        use oxigeo_core::vector::Coordinate;

        let make_coord = |i: usize, p: &crate::shp::shapes::Point| -> Coordinate {
            Coordinate::new_2dm(p.x, p.y, m_values.get(i).copied().unwrap_or(0.0))
        };

        if base.parts.len() == 1 {
            let coords: Vec<Coordinate> = base
                .points
                .iter()
                .enumerate()
                .map(|(i, p)| make_coord(i, p))
                .collect();
            if coords.len() < 2 {
                return Ok(None);
            }
            let linestring = CoreLineString::new(coords).map_err(|e| {
                ShapefileError::invalid_geometry(format!("Invalid LineStringM: {}", e))
            })?;
            Ok(Some(Geometry::LineString(linestring)))
        } else {
            let mut linestrings = Vec::new();
            for i in 0..base.parts.len() {
                let start = base.parts[i] as usize;
                let end = if i + 1 < base.parts.len() {
                    base.parts[i + 1] as usize
                } else {
                    base.points.len()
                };
                let coords: Vec<Coordinate> = base.points[start..end]
                    .iter()
                    .enumerate()
                    .map(|(j, p)| make_coord(start + j, p))
                    .collect();
                if coords.len() >= 2
                    && let Ok(ls) = CoreLineString::new(coords)
                {
                    linestrings.push(ls);
                }
            }
            if linestrings.is_empty() {
                Ok(None)
            } else {
                Ok(Some(Geometry::MultiLineString(CoreMultiLineString::new(
                    linestrings,
                ))))
            }
        }
    }

    /// Converts a multi-part M shape into a Polygon with M.
    fn multipart_m_to_polygon_geometry(
        base: &crate::shp::MultiPartShape,
        m_values: &[f64],
    ) -> Result<Option<Geometry>> {
        use oxigeo_core::vector::Coordinate;

        if base.parts.is_empty() {
            return Ok(None);
        }

        let make_coord = |i: usize, p: &crate::shp::shapes::Point| -> Coordinate {
            Coordinate::new_2dm(p.x, p.y, m_values.get(i).copied().unwrap_or(0.0))
        };

        let ext_start = base.parts[0] as usize;
        let ext_end = if base.parts.len() > 1 {
            base.parts[1] as usize
        } else {
            base.points.len()
        };
        let ext_coords: Vec<Coordinate> = base.points[ext_start..ext_end]
            .iter()
            .enumerate()
            .map(|(j, p)| make_coord(ext_start + j, p))
            .collect();
        if ext_coords.len() < 4 {
            return Ok(None);
        }
        let exterior = CoreLineString::new(ext_coords).map_err(|e| {
            ShapefileError::invalid_geometry(format!("Invalid exterior M ring: {}", e))
        })?;

        let mut interiors = Vec::new();
        for i in 1..base.parts.len() {
            let start = base.parts[i] as usize;
            let end = if i + 1 < base.parts.len() {
                base.parts[i + 1] as usize
            } else {
                base.points.len()
            };
            let coords: Vec<Coordinate> = base.points[start..end]
                .iter()
                .enumerate()
                .map(|(j, p)| make_coord(start + j, p))
                .collect();
            if coords.len() >= 4
                && let Ok(ring) = CoreLineString::new(coords)
            {
                interiors.push(ring);
            }
        }
        let polygon = CorePolygon::new(exterior, interiors)
            .map_err(|e| ShapefileError::invalid_geometry(format!("Invalid polygon M: {}", e)))?;
        Ok(Some(Geometry::Polygon(polygon)))
    }

    /// Converts a multi-part M shape into a MultiPoint with M.
    fn multipart_m_to_multipoint_geometry(
        base: &crate::shp::MultiPartShape,
        m_values: &[f64],
    ) -> Result<Option<Geometry>> {
        use oxigeo_core::vector::Coordinate;
        let points: Vec<CorePoint> = base
            .points
            .iter()
            .enumerate()
            .map(|(i, p)| {
                CorePoint::from_coord(Coordinate::new_2dm(
                    p.x,
                    p.y,
                    m_values.get(i).copied().unwrap_or(0.0),
                ))
            })
            .collect();
        if points.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Geometry::MultiPoint(CoreMultiPoint::new(points))))
        }
    }

    /// Converts a DBF record to FieldValue attributes
    fn dbf_to_attributes(
        dbf_record: &crate::dbf::DbfRecord,
        field_descriptors: &[FieldDescriptor],
    ) -> HashMap<String, FieldValue> {
        let mut attributes = HashMap::new();

        for (field, value) in field_descriptors.iter().zip(&dbf_record.values) {
            let property_value = match value {
                crate::dbf::FieldValue::String(s) => FieldValue::String(s.clone()),
                crate::dbf::FieldValue::Integer(i) => FieldValue::Integer(*i),
                crate::dbf::FieldValue::Float(f) => FieldValue::Float(*f),
                crate::dbf::FieldValue::Boolean(b) => FieldValue::Bool(*b),
                crate::dbf::FieldValue::Date(d) => FieldValue::String(d.clone()),
                crate::dbf::FieldValue::Null => FieldValue::Null,
            };

            attributes.insert(field.name.clone(), property_value);
        }

        attributes
    }

    /// Converts a DBF record to FieldValue attributes (static version for FeatureIter)
    pub(crate) fn dbf_to_attributes_pub(
        dbf_record: &crate::dbf::DbfRecord,
        field_descriptors: &[FieldDescriptor],
    ) -> HashMap<String, FieldValue> {
        Self::dbf_to_attributes(dbf_record, field_descriptors)
    }

    /// Converts a Shape to an OxiGeo Geometry (public version for FeatureIter)
    pub(crate) fn shape_to_geometry_pub(shape: &Shape) -> Result<Option<Geometry>> {
        Self::shape_to_geometry(shape)
    }

    /// Discovers the sibling memo file (`.dbt` / `.DBT`) for the given `.dbf`
    /// path, returning the first variant that exists on disk.
    ///
    /// Returns `None` when neither sibling exists; the caller treats that as
    /// "no memo support" and emits a one-time warning if the table later
    /// turns out to declare memo fields.
    fn discover_memo_path(dbf_path: &Path) -> Option<PathBuf> {
        let lower = dbf_path.with_extension("dbt");
        if lower.exists() {
            return Some(lower);
        }
        let upper = dbf_path.with_extension("DBT");
        if upper.exists() {
            return Some(upper);
        }
        None
    }

    /// Helper to add extension to base path
    fn with_extension<P: AsRef<Path>>(base_path: P, ext: &str) -> PathBuf {
        let base = base_path.as_ref();

        // If base already has an extension, replace it
        if base.extension().is_some() {
            base.with_extension(ext)
        } else {
            // Otherwise, add the extension
            let mut path = base.to_path_buf();
            path.set_extension(ext);
            path
        }
    }
}

// ─── Streaming feature iterator ──────────────────────────────────────────────

/// A lazy, streaming iterator over the features in a Shapefile.
///
/// Created by [`ShapefileReader::iter_features`].  Reads one SHP record and one
/// DBF record per call to [`Iterator::next`], keeping memory usage at O(1) with
/// respect to the number of features.
///
/// The iterator is exhausted as soon as either the `.shp` or the `.dbf` reader
/// returns `None`, or a record-level I/O error occurs (which is surfaced as an
/// `Err` item).
pub struct FeatureIter<'a> {
    shp_reader: ShpReader<BufReader<File>>,
    dbf_reader: DbfReader<BufReader<File>>,
    /// Reference to the field descriptors owned by the parent `ShapefileReader`.
    field_descriptors: &'a [FieldDescriptor],
    /// Set to `true` after the first `None` or error to make the iterator fused.
    done: bool,
}

impl<'a> Iterator for FeatureIter<'a> {
    type Item = Result<ShapefileFeature>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }

        // Read one SHP record
        let shp_record = match self.shp_reader.read_record() {
            Ok(Some(r)) => r,
            Ok(None) => {
                self.done = true;
                return None;
            }
            Err(e) => {
                self.done = true;
                return Some(Err(e));
            }
        };

        // Read one DBF record
        let dbf_record = match self.dbf_reader.read_record() {
            Ok(Some(r)) => r,
            Ok(None) => {
                self.done = true;
                return None;
            }
            Err(e) => {
                self.done = true;
                return Some(Err(e));
            }
        };

        // Convert shape to geometry
        let geometry = match ShapefileReader::shape_to_geometry_pub(&shp_record.shape) {
            Ok(g) => g,
            Err(e) => {
                self.done = true;
                return Some(Err(e));
            }
        };

        // Convert DBF record to attributes
        let attributes =
            ShapefileReader::dbf_to_attributes_pub(&dbf_record, self.field_descriptors);

        Some(Ok(ShapefileFeature::new(
            shp_record.record_number,
            geometry,
            attributes,
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_extension_helper() {
        let base = std::env::temp_dir().join("oxigeo_shapefile_test");
        let expected_shp = std::env::temp_dir().join("oxigeo_shapefile_test.shp");
        assert_eq!(ShapefileReader::with_extension(&base, "shp"), expected_shp);

        let base_shp = std::env::temp_dir().join("oxigeo_shapefile_test.shp");
        let expected_dbf = std::env::temp_dir().join("oxigeo_shapefile_test.dbf");
        assert_eq!(
            ShapefileReader::with_extension(&base_shp, "dbf"),
            expected_dbf
        );
    }

    #[test]
    fn test_shapefile_feature_creation() {
        let mut attributes = HashMap::new();
        attributes.insert("name".to_string(), FieldValue::String("Test".to_string()));
        attributes.insert("value".to_string(), FieldValue::Integer(42));

        let geometry = Some(Geometry::Point(CorePoint::new(10.0, 20.0)));

        let feature = ShapefileFeature::new(1, geometry, attributes);
        assert_eq!(feature.record_number, 1);
        assert!(feature.geometry.is_some());
        assert_eq!(feature.attributes.len(), 2);
    }
}
