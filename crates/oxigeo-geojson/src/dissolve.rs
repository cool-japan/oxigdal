//! Feature dissolve / merge by property key.
//!
//! Groups [`GeoJsonFeature`] objects by the value of a chosen property,
//! combines their geometries into a multi-part geometry, and aggregates
//! other property fields using a configurable [`PropertyAggregator`].

use std::collections::HashMap;

use serde_json::Value;

use crate::parser::FeatureCollection;
use crate::{GeoJsonError, GeoJsonFeature, GeoJsonGeometry};

// ─── Strategy ───────────────────────────────────────────────────────────────

/// Strategy for combining geometries when dissolving features.
pub enum DissolveStrategy {
    /// Combine into a Multi* geometry (MultiPoint, MultiLineString, MultiPolygon)
    /// or a GeometryCollection for heterogeneous input.
    Multipart,
    /// Same as [`Multipart`](Self::Multipart) for now; full topological union is deferred.
    Coalesce,
}

// ─── Aggregators ────────────────────────────────────────────────────────────

/// Describes how a property field should be aggregated across dissolved features.
pub enum PropertyAggregator {
    /// Keep the value from the first feature in the group.
    First,
    /// Keep the value from the last feature in the group.
    Last,
    /// Sum all numeric values; non-numeric values are skipped.
    Sum,
    /// Arithmetic mean of all numeric values; non-numeric values are skipped.
    Mean,
    /// Minimum numeric value in the group.
    Min,
    /// Maximum numeric value in the group.
    Max,
    /// Count of features in the group (regardless of value type).
    Count,
    /// Join all values, serialised as strings, with a separator.
    Concat {
        /// Separator inserted between consecutive values.
        sep: String,
    },
    /// Return the shared value when all features agree, else [`Value::Null`].
    KeepIfSame,
    /// Always emit [`Value::Null`] — effectively drops the field.
    Drop,
}

// ─── Options ─────────────────────────────────────────────────────────────────

/// Options controlling the dissolve operation.
pub struct DissolveOptions {
    /// Name of the property used to group features.
    pub group_by: String,
    /// Geometry-combination strategy.
    pub strategy: DissolveStrategy,
    /// Per-field aggregation rules (keyed on property name).
    pub property_aggregators: HashMap<String, PropertyAggregator>,
    /// Fallback aggregator applied to fields not listed in
    /// [`property_aggregators`](Self::property_aggregators).
    pub default_aggregator: PropertyAggregator,
}

impl Default for DissolveOptions {
    fn default() -> Self {
        Self {
            group_by: String::new(),
            strategy: DissolveStrategy::Multipart,
            property_aggregators: HashMap::new(),
            default_aggregator: PropertyAggregator::Drop,
        }
    }
}

// ─── Stats ───────────────────────────────────────────────────────────────────

/// Statistics produced by a dissolve operation.
#[derive(Debug, Clone, PartialEq)]
pub struct DissolveStats {
    /// Number of features supplied as input.
    pub input_features: usize,
    /// Number of features in the result.
    pub output_features: usize,
    /// Number of distinct groups formed.
    pub groups_formed: usize,
    /// Number of features whose group-by property was missing or null (excluded).
    pub features_with_null_group: usize,
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Dissolve a slice of features by the property named in [`DissolveOptions::group_by`].
///
/// Features whose group-by property is absent or [`Value::Null`] are excluded
/// from the output and counted in [`DissolveStats::features_with_null_group`].
///
/// # Errors
///
/// Returns [`GeoJsonError::DissolveError`] if `options.group_by` is empty.
pub fn dissolve_features(
    features: &[GeoJsonFeature],
    options: &DissolveOptions,
) -> Result<(Vec<GeoJsonFeature>, DissolveStats), GeoJsonError> {
    if options.group_by.is_empty() {
        return Err(GeoJsonError::DissolveError(
            "group_by property name must not be empty".into(),
        ));
    }

    let input_features = features.len();
    let mut features_with_null_group: usize = 0;

    // Group features by their group_by value, preserving insertion order via
    // a separate key-ordering vec so output order is deterministic.
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<&GeoJsonFeature>> = HashMap::new();

    for feature in features {
        match group_key_value(feature, &options.group_by) {
            None => {
                features_with_null_group += 1;
            }
            Some(v) => {
                let key = v.to_string();
                let entry = groups.entry(key.clone()).or_insert_with(|| {
                    order.push(key.clone());
                    Vec::new()
                });
                entry.push(feature);
            }
        }
    }

    let groups_formed = order.len();
    let mut output: Vec<GeoJsonFeature> = Vec::with_capacity(groups_formed);

    for key in &order {
        let group = &groups[key];
        let merged = build_merged_feature(group, options);
        output.push(merged);
    }

    let output_features = output.len();
    let stats = DissolveStats {
        input_features,
        output_features,
        groups_formed,
        features_with_null_group,
    };

    Ok((output, stats))
}

/// Dissolve all features in a [`FeatureCollection`] and return a new collection.
///
/// The returned collection inherits the CRS and name from the input; its bbox
/// fields are left unset (callers may recompute them via
/// [`FeatureCollection::compute_bbox`]).
///
/// # Errors
///
/// Propagates any error from [`dissolve_features`].
pub fn dissolve_feature_collection(
    fc: &FeatureCollection,
    options: &DissolveOptions,
) -> Result<(FeatureCollection, DissolveStats), GeoJsonError> {
    let (features, stats) = dissolve_features(&fc.features, options)?;
    let result = FeatureCollection {
        features,
        bbox: None,
        bbox_3d: None,
        crs: fc.crs.clone(),
        name: fc.name.clone(),
    };
    Ok((result, stats))
}

// ─── Private helpers ─────────────────────────────────────────────────────────

/// Extract the value of `prop` from a feature's properties.
///
/// Returns `None` if the properties map is absent, the key is missing, or the
/// value is [`Value::Null`].
fn group_key_value(feature: &GeoJsonFeature, prop: &str) -> Option<Value> {
    let props = feature.properties.as_ref()?;
    let v = props.get(prop)?;
    if v.is_null() {
        return None;
    }
    Some(v.clone())
}

/// Classify a geometry for merging purposes.
#[derive(Debug, Clone, Copy, PartialEq)]
enum GeomKind {
    Point,
    PointZ,
    LineString,
    LineStringZ,
    Polygon,
    PolygonZ,
    /// Already a multi-geometry, collection, or null — use GeometryCollection.
    Multi,
}

fn classify(g: &GeoJsonGeometry) -> GeomKind {
    match g {
        GeoJsonGeometry::Point(_) => GeomKind::Point,
        GeoJsonGeometry::PointZ(_) => GeomKind::PointZ,
        GeoJsonGeometry::LineString(_) => GeomKind::LineString,
        GeoJsonGeometry::LineStringZ(_) => GeomKind::LineStringZ,
        GeoJsonGeometry::Polygon(_) => GeomKind::Polygon,
        GeoJsonGeometry::PolygonZ(_) => GeomKind::PolygonZ,
        _ => GeomKind::Multi,
    }
}

/// Merge a non-empty list of geometries into the most specific multi-type.
///
/// Rules:
/// - All `Point`   → `MultiPoint`
/// - All `PointZ`  → `MultiPointZ`
/// - All `LineString`  → `MultiLineString`
/// - All `LineStringZ` → `MultiLineStringZ`
/// - All `Polygon`  → `MultiPolygon`
/// - All `PolygonZ` → `MultiPolygonZ`
/// - Mixed (or already-multi inputs) → `GeometryCollection`
/// - Empty input → `GeoJsonGeometry::Null`
fn merge_geometries_multipart(geoms: Vec<GeoJsonGeometry>) -> GeoJsonGeometry {
    if geoms.is_empty() {
        return GeoJsonGeometry::Null;
    }

    // Determine whether all geometries share the same primitive kind.
    let first_kind = classify(&geoms[0]);
    let homogeneous = geoms.iter().all(|g| classify(g) == first_kind);

    if !homogeneous || first_kind == GeomKind::Multi {
        // Heterogeneous or already-multi → GeometryCollection
        return GeoJsonGeometry::GeometryCollection(geoms);
    }

    match first_kind {
        GeomKind::Point => {
            let pts: Vec<[f64; 2]> = geoms
                .into_iter()
                .filter_map(|g| {
                    if let GeoJsonGeometry::Point(c) = g {
                        Some(c)
                    } else {
                        None
                    }
                })
                .collect();
            GeoJsonGeometry::MultiPoint(pts)
        }
        GeomKind::PointZ => {
            let pts: Vec<[f64; 3]> = geoms
                .into_iter()
                .filter_map(|g| {
                    if let GeoJsonGeometry::PointZ(c) = g {
                        Some(c)
                    } else {
                        None
                    }
                })
                .collect();
            GeoJsonGeometry::MultiPointZ(pts)
        }
        GeomKind::LineString => {
            let lines: Vec<Vec<[f64; 2]>> = geoms
                .into_iter()
                .filter_map(|g| {
                    if let GeoJsonGeometry::LineString(c) = g {
                        Some(c)
                    } else {
                        None
                    }
                })
                .collect();
            GeoJsonGeometry::MultiLineString(lines)
        }
        GeomKind::LineStringZ => {
            let lines: Vec<Vec<[f64; 3]>> = geoms
                .into_iter()
                .filter_map(|g| {
                    if let GeoJsonGeometry::LineStringZ(c) = g {
                        Some(c)
                    } else {
                        None
                    }
                })
                .collect();
            GeoJsonGeometry::MultiLineStringZ(lines)
        }
        GeomKind::Polygon => {
            let polys: Vec<Vec<Vec<[f64; 2]>>> = geoms
                .into_iter()
                .filter_map(|g| {
                    if let GeoJsonGeometry::Polygon(c) = g {
                        Some(c)
                    } else {
                        None
                    }
                })
                .collect();
            GeoJsonGeometry::MultiPolygon(polys)
        }
        GeomKind::PolygonZ => {
            let polys: Vec<Vec<Vec<[f64; 3]>>> = geoms
                .into_iter()
                .filter_map(|g| {
                    if let GeoJsonGeometry::PolygonZ(c) = g {
                        Some(c)
                    } else {
                        None
                    }
                })
                .collect();
            GeoJsonGeometry::MultiPolygonZ(polys)
        }
        GeomKind::Multi => GeoJsonGeometry::GeometryCollection(geoms), // handled above
    }
}

/// Aggregate a slice of property values with the given strategy.
///
/// When `values` is empty, always returns [`Value::Null`].
fn aggregate_property(values: &[&Value], aggregator: &PropertyAggregator) -> Value {
    if values.is_empty() {
        return Value::Null;
    }
    match aggregator {
        PropertyAggregator::First => (*values[0]).clone(),
        PropertyAggregator::Last => (*values[values.len() - 1]).clone(),
        PropertyAggregator::Sum => {
            let sum: f64 = values.iter().filter_map(|v| v.as_f64()).sum();
            numeric_value(sum)
        }
        PropertyAggregator::Mean => {
            let nums: Vec<f64> = values.iter().filter_map(|v| v.as_f64()).collect();
            if nums.is_empty() {
                Value::Null
            } else {
                let mean = nums.iter().sum::<f64>() / nums.len() as f64;
                numeric_value(mean)
            }
        }
        PropertyAggregator::Min => {
            let mut nums = values.iter().filter_map(|v| v.as_f64());
            match nums.next() {
                None => Value::Null,
                Some(first) => {
                    let min = nums.fold(first, f64::min);
                    numeric_value(min)
                }
            }
        }
        PropertyAggregator::Max => {
            let mut nums = values.iter().filter_map(|v| v.as_f64());
            match nums.next() {
                None => Value::Null,
                Some(first) => {
                    let max = nums.fold(first, f64::max);
                    numeric_value(max)
                }
            }
        }
        PropertyAggregator::Count => {
            // Count is the total number of values (= features in group).
            numeric_value(values.len() as f64)
        }
        PropertyAggregator::Concat { sep } => {
            let parts: Vec<String> = values.iter().map(|v| value_to_display_string(v)).collect();
            Value::String(parts.join(sep))
        }
        PropertyAggregator::KeepIfSame => {
            let first: &Value = values[0];
            if values.iter().all(|v| *v == first) {
                first.clone()
            } else {
                Value::Null
            }
        }
        PropertyAggregator::Drop => Value::Null,
    }
}

/// Collect the union of all property keys across a group, sorted deterministically.
fn collect_property_keys(features: &[&GeoJsonFeature]) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for feature in features {
        if let Some(props) = &feature.properties
            && let Some(map) = props.as_object()
        {
            for k in map.keys() {
                if !keys.contains(k) {
                    keys.push(k.clone());
                }
            }
        }
    }
    keys.sort();
    keys
}

/// Build a single output feature by merging the group's geometries and
/// aggregating all of its property fields.
fn build_merged_feature(group: &[&GeoJsonFeature], options: &DissolveOptions) -> GeoJsonFeature {
    // ── Geometry ────────────────────────────────────────────────────────────
    let geoms: Vec<GeoJsonGeometry> = group.iter().filter_map(|f| f.geometry.clone()).collect();
    let merged_geom = if geoms.is_empty() {
        None
    } else {
        let g = merge_geometries_multipart(geoms);
        if matches!(g, GeoJsonGeometry::Null) {
            None
        } else {
            Some(g)
        }
    };

    // ── Properties ──────────────────────────────────────────────────────────
    let all_keys = collect_property_keys(group);
    let mut out_props = serde_json::Map::new();

    for key in &all_keys {
        // Collect the value for this key from each feature (absent = Null).
        let null_sentinel = Value::Null;
        let vals: Vec<&Value> = group
            .iter()
            .map(|f| {
                f.properties
                    .as_ref()
                    .and_then(|p| p.get(key.as_str()))
                    .unwrap_or(&null_sentinel)
            })
            .collect();

        // Choose the aggregator: per-field override wins, else default.
        let aggregator = options
            .property_aggregators
            .get(key.as_str())
            .unwrap_or(&options.default_aggregator);

        let aggregated = aggregate_property(&vals, aggregator);
        out_props.insert(key.clone(), aggregated);
    }

    GeoJsonFeature {
        id: None,
        geometry: merged_geom,
        properties: Some(Value::Object(out_props)),
    }
}

/// Convert an `f64` to a JSON [`Value::Number`].
///
/// Falls back to [`Value::Null`] for non-finite values (NaN / ±Infinity)
/// which are not representable in JSON.
fn numeric_value(v: f64) -> Value {
    serde_json::Number::from_f64(v)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

/// Serialize a [`Value`] to a human-readable string for `Concat` aggregation.
fn value_to_display_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "null".to_string(),
        other => other.to_string(),
    }
}
