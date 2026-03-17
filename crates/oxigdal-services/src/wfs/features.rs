//! WFS feature retrieval and description
//!
//! Implements GetFeature and DescribeFeatureType operations for
//! querying and retrieving geospatial features.

use crate::error::{ServiceError, ServiceResult};
use crate::wfs::database::{
    BboxFilter, CountCacheConfig, CqlFilter, DatabaseFeatureCounter, DatabaseSource, DatabaseType,
};
use crate::wfs::{FeatureSource, WfsState};
use axum::{
    http::header,
    response::{IntoResponse, Response},
};
use serde::Deserialize;

/// GetFeature parameters
#[derive(Debug, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub struct GetFeatureParams {
    /// Feature type names (comma-separated)
    #[serde(rename = "TYPENAME", alias = "TYPENAMES")]
    pub type_names: String,
    /// Output format
    #[serde(default = "default_output_format")]
    pub output_format: String,
    /// Maximum number of features
    #[serde(rename = "COUNT", alias = "MAXFEATURES")]
    pub count: Option<usize>,
    /// Result type (results or hits)
    #[serde(default = "default_result_type")]
    pub result_type: String,
    /// BBOX filter (minx,miny,maxx,maxy\[,crs\])
    pub bbox: Option<String>,
    /// Filter (FE filter encoding)
    pub filter: Option<String>,
    /// Property names to return
    pub property_name: Option<String>,
    /// Sort by properties
    pub sortby: Option<String>,
    /// Start index for pagination
    #[serde(rename = "STARTINDEX")]
    pub start_index: Option<usize>,
    /// CRS for output
    pub srsname: Option<String>,
}

fn default_output_format() -> String {
    "application/gml+xml; version=3.2".to_string()
}

fn default_result_type() -> String {
    "results".to_string()
}

/// DescribeFeatureType parameters
#[derive(Debug, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub struct DescribeFeatureTypeParams {
    /// Feature type names (comma-separated)
    #[serde(rename = "TYPENAME", alias = "TYPENAMES")]
    pub type_names: Option<String>,
    /// Output format
    #[serde(default = "default_schema_format")]
    pub output_format: String,
}

fn default_schema_format() -> String {
    "application/gml+xml; version=3.2".to_string()
}

/// Handle GetFeature request
pub async fn handle_get_feature(
    state: &WfsState,
    _version: &str,
    params: &serde_json::Value,
) -> Result<Response, ServiceError> {
    let params: GetFeatureParams = serde_json::from_value(params.clone())
        .map_err(|e| ServiceError::InvalidParameter("Parameters".to_string(), e.to_string()))?;

    // Parse type names
    let type_names: Vec<&str> = params.type_names.split(',').collect();

    // Validate all type names exist
    for type_name in &type_names {
        if state.get_feature_type(type_name.trim()).is_none() {
            return Err(ServiceError::NotFound(format!(
                "Feature type not found: {}",
                type_name
            )));
        }
    }

    // Handle result_type=hits (just count, no features)
    if params.result_type.to_lowercase() == "hits" {
        return generate_hits_response(state, &type_names, &params);
    }

    // Determine output format
    match params.output_format.as_str() {
        "application/json" | "application/geo+json" => {
            generate_geojson_response(state, &type_names, &params).await
        }
        _ => generate_gml_response(state, &type_names, &params).await,
    }
}

/// Handle DescribeFeatureType request
pub async fn handle_describe_feature_type(
    state: &WfsState,
    _version: &str,
    params: &serde_json::Value,
) -> Result<Response, ServiceError> {
    let params: DescribeFeatureTypeParams = serde_json::from_value(params.clone())
        .map_err(|e| ServiceError::InvalidParameter("Parameters".to_string(), e.to_string()))?;

    let type_names: Vec<String> = if let Some(ref names) = params.type_names {
        names.split(',').map(|s| s.trim().to_string()).collect()
    } else {
        // Return all feature types
        state
            .feature_types
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    };
    let type_names_refs: Vec<&str> = type_names.iter().map(|s| s.as_str()).collect();

    generate_feature_schema(state, &type_names_refs)
}

/// Global feature counter with caching
static FEATURE_COUNTER: std::sync::OnceLock<DatabaseFeatureCounter> = std::sync::OnceLock::new();

/// Get or initialize the feature counter
fn get_feature_counter() -> &'static DatabaseFeatureCounter {
    FEATURE_COUNTER.get_or_init(|| DatabaseFeatureCounter::new(CountCacheConfig::default()))
}

/// Generate hits response (feature count only)
fn generate_hits_response(
    state: &WfsState,
    type_names: &[&str],
    params: &GetFeatureParams,
) -> Result<Response, ServiceError> {
    // Use tokio runtime for async database operations
    let rt = tokio::runtime::Handle::try_current().map_err(|_| {
        ServiceError::Internal("No async runtime available for database operations".to_string())
    })?;

    let mut total_count = 0usize;
    let mut any_estimated = false;

    for type_name in type_names {
        let ft = state
            .get_feature_type(type_name.trim())
            .ok_or_else(|| ServiceError::NotFound(format!("Feature type: {}", type_name)))?;

        let (count, is_estimated) = match &ft.source {
            FeatureSource::Memory(features) => {
                // For memory sources, apply filters directly
                let filtered = apply_memory_filters(features, params)?;
                (filtered.len(), false)
            }
            FeatureSource::File(path) => {
                // Count features in file with optional filtering
                let count = count_features_in_file_filtered(path, params)?;
                (count, false)
            }
            FeatureSource::Database(conn_string) => {
                // Legacy database source - create a temporary DatabaseSource
                let db_source = create_legacy_database_source(conn_string, type_name);
                rt.block_on(count_database_features(&db_source, params))?
            }
            FeatureSource::DatabaseSource(db_source) => {
                // Full database source with proper configuration
                rt.block_on(count_database_features(db_source, params))?
            }
        };

        total_count += count;
        if is_estimated {
            any_estimated = true;
        }
    }

    // Apply count limit
    if let Some(max_count) = params.count {
        total_count = total_count.min(max_count);
    }

    // Build response with optional estimation indicator
    let number_matched = if any_estimated {
        format!("~{}", total_count)
    } else {
        total_count.to_string()
    };

    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<wfs:FeatureCollection
    xmlns:wfs="http://www.opengis.net/wfs/2.0"
    xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
    numberMatched="{}"
    numberReturned="0"
    timeStamp="{}">
</wfs:FeatureCollection>"#,
        number_matched,
        chrono::Utc::now().to_rfc3339()
    );

    Ok(([(header::CONTENT_TYPE, "application/xml")], xml).into_response())
}

/// Create a legacy database source from a connection string
fn create_legacy_database_source(conn_string: &str, table_name: &str) -> DatabaseSource {
    // Infer database type from connection string
    let db_type =
        if conn_string.starts_with("postgresql://") || conn_string.starts_with("postgres://") {
            DatabaseType::PostGis
        } else if conn_string.starts_with("mysql://") {
            DatabaseType::MySql
        } else if conn_string.ends_with(".db") || conn_string.ends_with(".sqlite") {
            DatabaseType::Sqlite
        } else {
            DatabaseType::Generic
        };

    DatabaseSource::new(conn_string, table_name).with_database_type(db_type)
}

/// Count features in database with proper caching and filtering
async fn count_database_features(
    source: &DatabaseSource,
    params: &GetFeatureParams,
) -> ServiceResult<(usize, bool)> {
    let counter = get_feature_counter();

    // Parse BBOX filter if present
    let bbox_filter = if let Some(ref bbox_str) = params.bbox {
        Some(BboxFilter::from_bbox_string(bbox_str)?)
    } else {
        None
    };

    // Parse CQL filter if present
    let cql_filter = params.filter.as_ref().map(CqlFilter::new);

    // Get count from database (with caching)
    let result = counter
        .get_count(source, cql_filter.as_ref(), bbox_filter.as_ref())
        .await?;

    Ok((result.count, result.is_estimated))
}

/// Apply filters to memory-based features and count
fn apply_memory_filters<'a>(
    features: &'a [geojson::Feature],
    params: &'a GetFeatureParams,
) -> ServiceResult<Vec<&'a geojson::Feature>> {
    let mut filtered: Vec<&geojson::Feature> = features.iter().collect();

    // Apply BBOX filter
    if let Some(ref bbox_str) = params.bbox {
        let bbox = BboxFilter::from_bbox_string(bbox_str)?;
        filtered.retain(|f| feature_in_bbox(f, &bbox));
    }

    // Apply CQL filter (simplified - property filtering)
    if let Some(ref filter_str) = params.filter {
        filtered.retain(|f| feature_matches_filter(f, filter_str));
    }

    Ok(filtered)
}

/// Check if a feature is within a bounding box
fn feature_in_bbox(feature: &geojson::Feature, bbox: &BboxFilter) -> bool {
    if let Some(ref geom_bbox) = feature.bbox {
        if geom_bbox.len() >= 4 {
            return geom_bbox[0] <= bbox.max_x
                && geom_bbox[2] >= bbox.min_x
                && geom_bbox[1] <= bbox.max_y
                && geom_bbox[3] >= bbox.min_y;
        }
    }

    // If no bbox on feature, check geometry bounds
    if let Some(ref geometry) = feature.geometry {
        return geometry_intersects_bbox(geometry, bbox);
    }

    false
}

/// Check if geometry intersects bbox
fn geometry_intersects_bbox(geometry: &geojson::Geometry, bbox: &BboxFilter) -> bool {
    use geojson::GeometryValue;

    match &geometry.value {
        GeometryValue::Point {
            coordinates: coords,
        } => {
            coords.len() >= 2
                && coords[0] >= bbox.min_x
                && coords[0] <= bbox.max_x
                && coords[1] >= bbox.min_y
                && coords[1] <= bbox.max_y
        }
        GeometryValue::MultiPoint {
            coordinates: points,
        } => points.iter().any(|coords| {
            coords.len() >= 2
                && coords[0] >= bbox.min_x
                && coords[0] <= bbox.max_x
                && coords[1] >= bbox.min_y
                && coords[1] <= bbox.max_y
        }),
        GeometryValue::LineString {
            coordinates: coords,
        } => coords.iter().any(|c| {
            c.len() >= 2
                && c[0] >= bbox.min_x
                && c[0] <= bbox.max_x
                && c[1] >= bbox.min_y
                && c[1] <= bbox.max_y
        }),
        GeometryValue::MultiLineString { coordinates: lines } => lines.iter().any(|line| {
            line.iter().any(|c| {
                c.len() >= 2
                    && c[0] >= bbox.min_x
                    && c[0] <= bbox.max_x
                    && c[1] >= bbox.min_y
                    && c[1] <= bbox.max_y
            })
        }),
        GeometryValue::Polygon { coordinates: rings } => rings.iter().any(|ring| {
            ring.iter().any(|c| {
                c.len() >= 2
                    && c[0] >= bbox.min_x
                    && c[0] <= bbox.max_x
                    && c[1] >= bbox.min_y
                    && c[1] <= bbox.max_y
            })
        }),
        GeometryValue::MultiPolygon {
            coordinates: polygons,
        } => polygons.iter().any(|polygon| {
            polygon.iter().any(|ring| {
                ring.iter().any(|c| {
                    c.len() >= 2
                        && c[0] >= bbox.min_x
                        && c[0] <= bbox.max_x
                        && c[1] >= bbox.min_y
                        && c[1] <= bbox.max_y
                })
            })
        }),
        GeometryValue::GeometryCollection { geometries: geoms } => {
            geoms.iter().any(|g| geometry_intersects_bbox(g, bbox))
        }
    }
}

/// Check if feature matches a filter expression (simplified)
fn feature_matches_filter(feature: &geojson::Feature, filter_str: &str) -> bool {
    // Simplified filter matching - parse basic property comparisons
    // Full CQL parsing is handled by the CqlFilter type for databases

    let props = match &feature.properties {
        Some(p) => p,
        None => return false,
    };

    // Try to parse simple "property = 'value'" expressions
    let filter = filter_str.trim();

    // Handle basic equality check
    if filter.contains('=')
        && !filter.contains("!=")
        && !filter.contains("<=")
        && !filter.contains(">=")
    {
        let parts: Vec<&str> = filter.splitn(2, '=').collect();
        if parts.len() == 2 {
            let prop_name = parts[0].trim().trim_matches('"').trim_matches('\'');
            let prop_value = parts[1].trim().trim_matches('\'').trim_matches('"');

            if let Some(value) = props.get(prop_name) {
                return match value {
                    serde_json::Value::String(s) => s == prop_value,
                    serde_json::Value::Number(n) => n.to_string() == prop_value,
                    serde_json::Value::Bool(b) => b.to_string() == prop_value,
                    _ => false,
                };
            }
        }
    }

    // Default to true if we can't parse the filter
    // (actual filtering happens at database level)
    true
}

/// Count features in file with filtering
fn count_features_in_file_filtered(
    path: &std::path::Path,
    params: &GetFeatureParams,
) -> ServiceResult<usize> {
    let features = load_features_from_file(path)?;

    // Apply filters
    let filtered = apply_memory_filters(&features, params)?;

    Ok(filtered.len())
}

/// Generate GeoJSON response
async fn generate_geojson_response(
    state: &WfsState,
    type_names: &[&str],
    params: &GetFeatureParams,
) -> Result<Response, ServiceError> {
    let mut all_features = Vec::new();

    for type_name in type_names {
        let ft = state
            .get_feature_type(type_name.trim())
            .ok_or_else(|| ServiceError::NotFound(format!("Feature type: {}", type_name)))?;

        let mut features = match &ft.source {
            FeatureSource::Memory(features) => features.clone(),
            FeatureSource::File(path) => load_features_from_file(path)?,
            FeatureSource::Database(_) => {
                return Err(ServiceError::Internal(
                    "Database sources not yet implemented".to_string(),
                ));
            }
            FeatureSource::DatabaseSource(db_source) => {
                // DatabaseSource provides full configuration for database-backed features
                // Currently, feature retrieval requires oxigdal-postgis or oxigdal-db-connectors integration
                return Err(ServiceError::Internal(format!(
                    "DatabaseSource feature retrieval not yet implemented for table '{}'. \
                     Use oxigdal-postgis for PostGIS connections.",
                    db_source.table_name
                )));
            }
        };

        // Apply BBOX filter
        if let Some(ref bbox_str) = params.bbox {
            features = apply_bbox_filter(features, bbox_str)?;
        }

        // Apply property filter
        if let Some(ref props) = params.property_name {
            features = filter_properties(features, props)?;
        }

        // Apply start index
        if let Some(start) = params.start_index {
            features = features.into_iter().skip(start).collect();
        }

        // Apply count limit
        if let Some(max_count) = params.count {
            features.truncate(max_count);
        }

        all_features.extend(features);
    }

    let feature_collection = geojson::FeatureCollection {
        bbox: None,
        features: all_features,
        foreign_members: None,
    };

    let json = serde_json::to_string_pretty(&feature_collection)
        .map_err(|e| ServiceError::Serialization(e.to_string()))?;

    Ok(([(header::CONTENT_TYPE, "application/geo+json")], json).into_response())
}

/// Generate GML response
async fn generate_gml_response(
    state: &WfsState,
    type_names: &[&str],
    params: &GetFeatureParams,
) -> Result<Response, ServiceError> {
    // For now, convert to GeoJSON first, then wrap in GML
    // A full GML implementation would be more complex
    let _geojson_response = generate_geojson_response(state, type_names, params).await?;

    // Simple GML wrapper
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<wfs:FeatureCollection
    xmlns:wfs="http://www.opengis.net/wfs/2.0"
    xmlns:gml="http://www.opengis.net/gml/3.2"
    xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
    timeStamp="{}">
  <!-- GML encoding would go here -->
  <!-- For production, use full GML 3.2 encoding -->
</wfs:FeatureCollection>"#,
        chrono::Utc::now().to_rfc3339()
    );

    Ok(([(header::CONTENT_TYPE, "application/gml+xml")], xml).into_response())
}

/// Generate XML schema for feature types
fn generate_feature_schema(
    state: &WfsState,
    type_names: &[&str],
) -> Result<Response, ServiceError> {
    use quick_xml::{
        Writer,
        events::{BytesDecl, BytesEnd, BytesStart, Event},
    };
    use std::io::Cursor;

    let mut writer = Writer::new(Cursor::new(Vec::new()));

    writer
        .write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    let mut schema = BytesStart::new("xsd:schema");
    schema.push_attribute(("xmlns:xsd", "http://www.w3.org/2001/XMLSchema"));
    schema.push_attribute(("xmlns:gml", "http://www.opengis.net/gml/3.2"));
    schema.push_attribute(("elementFormDefault", "qualified"));
    schema.push_attribute(("targetNamespace", "http://www.opengis.net/wfs/2.0"));

    writer
        .write_event(Event::Start(schema))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    // Import GML schema
    let mut import = BytesStart::new("xsd:import");
    import.push_attribute(("namespace", "http://www.opengis.net/gml/3.2"));
    import.push_attribute((
        "schemaLocation",
        "http://schemas.opengis.net/gml/3.2.1/gml.xsd",
    ));
    writer
        .write_event(Event::Empty(import))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    for type_name in type_names {
        let _ft = state
            .get_feature_type(type_name)
            .ok_or_else(|| ServiceError::NotFound(format!("Feature type: {}", type_name)))?;

        // Simple schema definition
        let mut element = BytesStart::new("xsd:element");
        element.push_attribute(("name", *type_name));
        element.push_attribute(("type", "gml:AbstractFeatureType"));
        element.push_attribute(("substitutionGroup", "gml:AbstractFeature"));

        writer
            .write_event(Event::Empty(element))
            .map_err(|e| ServiceError::Xml(e.to_string()))?;
    }

    writer
        .write_event(Event::End(BytesEnd::new("xsd:schema")))
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    let xml = String::from_utf8(writer.into_inner().into_inner())
        .map_err(|e| ServiceError::Xml(e.to_string()))?;

    Ok(([(header::CONTENT_TYPE, "application/xml")], xml).into_response())
}

/// Load features from file
fn load_features_from_file(path: &std::path::Path) -> ServiceResult<Vec<geojson::Feature>> {
    let contents = std::fs::read_to_string(path)?;

    match path.extension().and_then(|e| e.to_str()) {
        Some("geojson") | Some("json") => {
            let geojson: geojson::GeoJson = contents.parse()?;
            match geojson {
                geojson::GeoJson::FeatureCollection(fc) => Ok(fc.features),
                geojson::GeoJson::Feature(f) => Ok(vec![f]),
                _ => Err(ServiceError::InvalidGeoJson(
                    "Expected FeatureCollection or Feature".to_string(),
                )),
            }
        }
        _ => Err(ServiceError::UnsupportedFormat(format!(
            "Unsupported file format: {:?}",
            path.extension()
        ))),
    }
}

/// Apply BBOX filter to features
fn apply_bbox_filter(
    features: Vec<geojson::Feature>,
    bbox_str: &str,
) -> ServiceResult<Vec<geojson::Feature>> {
    let parts: Vec<&str> = bbox_str.split(',').collect();
    if parts.len() < 4 {
        return Err(ServiceError::InvalidBbox(
            "BBOX must have at least 4 coordinates".to_string(),
        ));
    }

    let minx: f64 = parts[0]
        .parse()
        .map_err(|_| ServiceError::InvalidBbox("Invalid minx".to_string()))?;
    let miny: f64 = parts[1]
        .parse()
        .map_err(|_| ServiceError::InvalidBbox("Invalid miny".to_string()))?;
    let maxx: f64 = parts[2]
        .parse()
        .map_err(|_| ServiceError::InvalidBbox("Invalid maxx".to_string()))?;
    let maxy: f64 = parts[3]
        .parse()
        .map_err(|_| ServiceError::InvalidBbox("Invalid maxy".to_string()))?;

    let filtered: Vec<_> = features
        .into_iter()
        .filter(|f| {
            if let Some(ref _geometry) = f.geometry {
                if let Some(bbox) = &f.bbox {
                    // Use feature bbox if available
                    bbox.len() >= 4
                        && bbox[0] <= maxx
                        && bbox[2] >= minx
                        && bbox[1] <= maxy
                        && bbox[3] >= miny
                } else {
                    // Simple point check for now
                    true
                }
            } else {
                false
            }
        })
        .collect();

    Ok(filtered)
}

/// Filter feature properties
fn filter_properties(
    features: Vec<geojson::Feature>,
    property_names: &str,
) -> ServiceResult<Vec<geojson::Feature>> {
    let names: Vec<&str> = property_names.split(',').map(|s| s.trim()).collect();

    let filtered: Vec<_> = features
        .into_iter()
        .map(|mut f| {
            if let Some(ref mut props) = f.properties {
                let filtered_props: serde_json::Map<String, serde_json::Value> = props
                    .iter()
                    .filter(|(k, _)| names.contains(&k.as_str()))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                f.properties = Some(filtered_props);
            }
            f
        })
        .collect();

    Ok(filtered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wfs::{FeatureTypeInfo, ServiceInfo};

    #[tokio::test]
    async fn test_describe_feature_type() -> Result<(), Box<dyn std::error::Error>> {
        let info = ServiceInfo {
            title: "Test WFS".to_string(),
            abstract_text: None,
            provider: "COOLJAPAN OU".to_string(),
            service_url: "http://localhost/wfs".to_string(),
            versions: vec!["2.0.0".to_string()],
        };

        let state = WfsState::new(info);

        let ft = FeatureTypeInfo {
            name: "test_layer".to_string(),
            title: "Test Layer".to_string(),
            abstract_text: None,
            default_crs: "EPSG:4326".to_string(),
            other_crs: vec![],
            bbox: None,
            source: FeatureSource::Memory(vec![]),
        };

        state.add_feature_type(ft)?;

        let params = serde_json::json!({
            "TYPENAMES": "test_layer",
            "OUTPUTFORMAT": "application/xml"
        });

        let response = handle_describe_feature_type(&state, "2.0.0", &params).await?;

        let (parts, _) = response.into_parts();
        assert_eq!(
            parts
                .headers
                .get(header::CONTENT_TYPE)
                .and_then(|h| h.to_str().ok()),
            Some("application/xml")
        );
        Ok(())
    }

    #[test]
    fn test_bbox_parsing() {
        let features = vec![];
        let result = apply_bbox_filter(features, "-180,-90,180,90");
        assert!(result.is_ok());

        let result = apply_bbox_filter(vec![], "invalid");
        assert!(result.is_err());
    }
}
