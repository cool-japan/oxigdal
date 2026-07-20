//! GeoPackage feature row and feature table types.

use std::collections::HashMap;

use super::types::{FieldDefinition, FieldValue, GpkgGeometry};

// ─────────────────────────────────────────────────────────────────────────────
// FeatureRow
// ─────────────────────────────────────────────────────────────────────────────

/// A single feature (row) read from a GeoPackage feature table.
#[derive(Debug, Clone)]
pub struct FeatureRow {
    /// Feature identifier (primary key value).
    pub fid: i64,
    /// Decoded geometry, or `None` when the geometry column is NULL.
    pub geometry: Option<GpkgGeometry>,
    /// Non-geometry attribute values, keyed by column name.
    pub fields: HashMap<String, FieldValue>,
}

impl FeatureRow {
    /// Look up a field by name.
    pub fn get_field(&self, name: &str) -> Option<&FieldValue> {
        self.fields.get(name)
    }

    /// Convenience: return the integer value of a field, or `None`.
    pub fn get_integer(&self, name: &str) -> Option<i64> {
        self.fields.get(name)?.as_integer()
    }

    /// Convenience: return the real value of a field, or `None`.
    pub fn get_real(&self, name: &str) -> Option<f64> {
        self.fields.get(name)?.as_real()
    }

    /// Convenience: return the text value of a field, or `None`.
    pub fn get_text(&self, name: &str) -> Option<&str> {
        self.fields.get(name)?.as_text()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// FeatureTable
// ─────────────────────────────────────────────────────────────────────────────

/// An in-memory representation of a GeoPackage feature table.
///
/// Holds the table schema and all feature rows that have been loaded.
#[derive(Debug, Clone)]
pub struct FeatureTable {
    /// Name of the feature table (matches `gpkg_contents.table_name`).
    pub name: String,
    /// Name of the geometry column.
    pub geometry_column: String,
    /// Spatial reference system ID, or `None` when unknown.
    pub srs_id: Option<i32>,
    /// Column definitions (excludes the geometry column and FID).
    pub schema: Vec<FieldDefinition>,
    /// Loaded feature rows.
    pub features: Vec<FeatureRow>,
}

impl FeatureTable {
    /// Create a new, empty feature table with the given name and geometry column.
    pub fn new(name: impl Into<String>, geometry_column: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            geometry_column: geometry_column.into(),
            srs_id: None,
            schema: Vec::new(),
            features: Vec::new(),
        }
    }

    /// Return the number of loaded feature rows.
    pub fn feature_count(&self) -> usize {
        self.features.len()
    }

    /// Append a feature row to the table.
    pub fn add_feature(&mut self, row: FeatureRow) {
        self.features.push(row);
    }

    /// Find a feature by its FID, or return `None`.
    pub fn get_feature(&self, fid: i64) -> Option<&FeatureRow> {
        self.features.iter().find(|r| r.fid == fid)
    }

    /// Return the union bounding box of all feature geometries, or `None` when
    /// there are no geometries.
    pub fn bbox(&self) -> Option<(f64, f64, f64, f64)> {
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        let mut found = false;

        for row in &self.features {
            if let Some(geom) = &row.geometry
                && let Some((gx0, gy0, gx1, gy1)) = geom.bbox()
            {
                found = true;
                if gx0 < min_x {
                    min_x = gx0;
                }
                if gy0 < min_y {
                    min_y = gy0;
                }
                if gx1 > max_x {
                    max_x = gx1;
                }
                if gy1 > max_y {
                    max_y = gy1;
                }
            }
        }

        if found {
            Some((min_x, min_y, max_x, max_y))
        } else {
            None
        }
    }

    /// Return all features whose geometry bounding box intersects the query bbox.
    ///
    /// Features with `None` geometry are excluded.  The check is a simple AABB
    /// intersection test (not precise polygon intersection).
    pub fn features_in_bbox(
        &self,
        min_x: f64,
        min_y: f64,
        max_x: f64,
        max_y: f64,
    ) -> Vec<&FeatureRow> {
        self.features
            .iter()
            .filter(|row| {
                if let Some(geom) = &row.geometry
                    && let Some((gx0, gy0, gx1, gy1)) = geom.bbox()
                {
                    // AABB intersects when not separated on either axis
                    return gx0 <= max_x && gx1 >= min_x && gy0 <= max_y && gy1 >= min_y;
                }
                false
            })
            .collect()
    }

    /// Collect all distinct (non-Null) values for a named field across all features.
    ///
    /// The returned vec is deduplicated by equality.
    pub fn distinct_values(&self, field_name: &str) -> Vec<FieldValue> {
        let mut seen: Vec<FieldValue> = Vec::new();
        for row in &self.features {
            if let Some(val) = row.fields.get(field_name)
                && !val.is_null()
                && !seen.contains(val)
            {
                seen.push(val.clone());
            }
        }
        seen
    }

    /// Serialise the feature table as a GeoJSON FeatureCollection string.
    ///
    /// Geometry `None` is encoded as `"geometry":null`.
    pub fn to_geojson(&self) -> String {
        let features_json: String = self
            .features
            .iter()
            .map(|row| {
                let geom_json = match &row.geometry {
                    Some(g) => g.to_geojson_geometry(),
                    None => "null".into(),
                };
                let props_json = build_properties_json(&row.fields);
                format!(r#"{{"type":"Feature","geometry":{geom_json},"properties":{props_json}}}"#)
            })
            .collect::<Vec<_>>()
            .join(",");

        format!(r#"{{"type":"FeatureCollection","features":[{features_json}]}}"#)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SrsInfo
// ─────────────────────────────────────────────────────────────────────────────

/// Spatial reference system metadata (from `gpkg_spatial_ref_sys`).
#[derive(Debug, Clone, PartialEq)]
pub struct SrsInfo {
    /// Human-readable name for this SRS.
    pub srs_name: String,
    /// Numeric SRS identifier (primary key in `gpkg_spatial_ref_sys`).
    pub srs_id: i32,
    /// Defining organisation (e.g. `"EPSG"`).
    pub organization: String,
    /// Organisation-assigned CRS code.
    pub org_coord_sys_id: i32,
    /// WKT or PROJ definition of the SRS.
    pub definition: String,
    /// Optional free-text description.
    pub description: Option<String>,
}

impl SrsInfo {
    /// Return the standard WGS 84 geographic SRS (EPSG:4326).
    pub fn wgs84() -> Self {
        Self {
            srs_name: "WGS 84".into(),
            srs_id: 4326,
            organization: "EPSG".into(),
            org_coord_sys_id: 4326,
            definition: concat!(
                "GEOGCS[\"WGS 84\",DATUM[\"WGS_1984\",",
                "SPHEROID[\"WGS 84\",6378137,298.257223563]],",
                "PRIMEM[\"Greenwich\",0],",
                "UNIT[\"degree\",0.0174532925199433]]"
            )
            .into(),
            description: Some("World Geodetic System 1984".into()),
        }
    }

    /// Return the Web Mercator (Pseudo-Mercator) projected SRS (EPSG:3857).
    pub fn web_mercator() -> Self {
        Self {
            srs_name: "WGS 84 / Pseudo-Mercator".into(),
            srs_id: 3857,
            organization: "EPSG".into(),
            org_coord_sys_id: 3857,
            definition: concat!(
                "PROJCS[\"WGS 84 / Pseudo-Mercator\",",
                "GEOGCS[\"WGS 84\",DATUM[\"WGS_1984\",",
                "SPHEROID[\"WGS 84\",6378137,298.257223563]],",
                "PRIMEM[\"Greenwich\",0],",
                "UNIT[\"degree\",0.0174532925199433]],",
                "PROJECTION[\"Mercator_1SP\"],",
                "PARAMETER[\"central_meridian\",0],",
                "PARAMETER[\"scale_factor\",1],",
                "PARAMETER[\"false_easting\",0],",
                "PARAMETER[\"false_northing\",0],",
                "UNIT[\"metre\",1]]"
            )
            .into(),
            description: Some("Web Mercator projection used by many web mapping services".into()),
        }
    }

    /// Return `true` when this SRS uses geographic (lat/lon) coordinates.
    ///
    /// Heuristic: considers `srs_id` values in the range 4000–4999 as geographic.
    pub fn is_geographic(&self) -> bool {
        (4000..5000).contains(&self.srs_id)
    }

    /// Return the EPSG code when the defining organisation is `"EPSG"`.
    pub fn epsg_code(&self) -> Option<i32> {
        if self.organization.eq_ignore_ascii_case("EPSG") {
            Some(self.org_coord_sys_id)
        } else {
            None
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// JSON helper utility
// ─────────────────────────────────────────────────────────────────────────────

/// Render a `HashMap<String, FieldValue>` as a JSON object.
pub(super) fn build_properties_json(fields: &HashMap<String, FieldValue>) -> String {
    if fields.is_empty() {
        return "{}".into();
    }
    // Sort keys for deterministic output
    let mut pairs: Vec<(&String, &FieldValue)> = fields.iter().collect();
    pairs.sort_by_key(|(k, _)| k.as_str());
    let members: String = pairs
        .iter()
        .map(|(k, v)| format!("{}:{}", super::types::json_string_escape(k), v.to_json()))
        .collect::<Vec<_>>()
        .join(",");
    format!("{{{members}}}")
}
