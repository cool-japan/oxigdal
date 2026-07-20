//! Filter operator for conditional filtering
//!
//! This module provides filter operators for selectively passing items through the pipeline.

use crate::error::Result;
use crate::transform::{FilterTransform, Transform};

/// Filter operator builder
pub struct FilterOperator;

impl FilterOperator {
    /// Create a simple predicate filter
    pub fn predicate<F>(name: String, f: F) -> Box<dyn Transform>
    where
        F: Fn(&[u8]) -> bool + Send + Sync + Clone + 'static,
    {
        Box::new(FilterTransform::new(name, move |item| {
            let f_clone = f.clone();
            let item = item.to_vec();
            Box::pin(async move { Ok(f_clone(&item)) })
        }))
    }

    /// Create an async predicate filter
    pub fn predicate_async<F, Fut>(name: String, f: F) -> Box<dyn Transform>
    where
        F: Fn(Vec<u8>) -> Fut + Send + Sync + 'static,
        Fut: futures::Future<Output = Result<bool>> + Send + 'static,
    {
        Box::new(FilterTransform::new(name, move |item| {
            let fut = f(item.clone());
            Box::pin(fut)
        }))
    }

    /// Filter by minimum size
    pub fn min_size(min_bytes: usize) -> Box<dyn Transform> {
        Box::new(FilterTransform::new(
            format!("min_size_{}", min_bytes),
            move |item| {
                let len = item.len();
                Box::pin(async move { Ok(len >= min_bytes) })
            },
        ))
    }

    /// Filter by maximum size
    pub fn max_size(max_bytes: usize) -> Box<dyn Transform> {
        Box::new(FilterTransform::new(
            format!("max_size_{}", max_bytes),
            move |item| {
                let len = item.len();
                Box::pin(async move { Ok(len <= max_bytes) })
            },
        ))
    }

    /// Filter by JSON field value
    pub fn json_field_equals(field: String, value: serde_json::Value) -> Box<dyn Transform> {
        Box::new(FilterTransform::new(
            format!("json_field_equals_{}", field),
            move |item| {
                let field = field.clone();
                let value = value.clone();
                let item = item.to_vec();
                Box::pin(async move {
                    let json: serde_json::Value = serde_json::from_slice(&item).map_err(|e| {
                        crate::error::TransformError::InvalidInput {
                            message: format!("Invalid JSON: {}", e),
                        }
                    })?;

                    Ok(json.get(&field) == Some(&value))
                })
            },
        ))
    }

    /// Filter by JSON field existence
    pub fn json_has_field(field: String) -> Box<dyn Transform> {
        Box::new(FilterTransform::new(
            format!("json_has_field_{}", field),
            move |item| {
                let field = field.clone();
                let item = item.to_vec();
                Box::pin(async move {
                    let json: serde_json::Value = serde_json::from_slice(&item).map_err(|e| {
                        crate::error::TransformError::InvalidInput {
                            message: format!("Invalid JSON: {}", e),
                        }
                    })?;

                    Ok(json.get(&field).is_some())
                })
            },
        ))
    }

    /// Filter by string pattern
    pub fn contains_string(pattern: String) -> Box<dyn Transform> {
        Box::new(FilterTransform::new(
            format!("contains_{}", pattern),
            move |item| {
                let pattern = pattern.clone();
                let item = item.to_vec();
                Box::pin(async move {
                    let s = String::from_utf8_lossy(&item);
                    Ok(s.contains(&pattern))
                })
            },
        ))
    }

    /// Filter by regex pattern
    #[cfg(feature = "std")]
    pub fn matches_regex(pattern: String) -> Result<Box<dyn Transform>> {
        let regex = regex::Regex::new(&pattern).map_err(|e| {
            crate::error::TransformError::InvalidInput {
                message: format!("Invalid regex: {}", e),
            }
        })?;

        Ok(Box::new(FilterTransform::new(
            format!("matches_regex_{}", pattern),
            move |item| {
                let regex = regex.clone();
                let item = item.to_vec();
                Box::pin(async move {
                    let s = String::from_utf8_lossy(&item);
                    Ok(regex.is_match(&s))
                })
            },
        )))
    }

    /// Sample filter (keep every Nth item)
    pub fn sample(n: usize) -> Box<dyn Transform> {
        let counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

        Box::new(FilterTransform::new(
            format!("sample_{}", n),
            move |_item| {
                let counter = counter.clone();
                Box::pin(async move {
                    let count = counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    Ok(count.is_multiple_of(n))
                })
            },
        ))
    }

    /// Rate limit filter (keep at most N items per second)
    pub fn rate_limit(items_per_second: u32) -> Box<dyn Transform> {
        use std::sync::Arc;
        use tokio::sync::Mutex;
        use tokio::time::{Duration, Instant};

        let state = Arc::new(Mutex::new((Instant::now(), 0u32)));

        Box::new(FilterTransform::new(
            format!("rate_limit_{}", items_per_second),
            move |_item| {
                let state = state.clone();
                Box::pin(async move {
                    let mut guard = state.lock().await;
                    let (last_reset, count) = &mut *guard;

                    let now = Instant::now();
                    let elapsed = now.duration_since(*last_reset);

                    if elapsed >= Duration::from_secs(1) {
                        *last_reset = now;
                        *count = 0;
                    }

                    if *count < items_per_second {
                        *count += 1;
                        Ok(true)
                    } else {
                        Ok(false)
                    }
                })
            },
        ))
    }
}

/// Specialized geospatial filter operators
pub struct GeoFilterOperator;

impl GeoFilterOperator {
    /// Filter by bounding box intersection
    pub fn intersects_bbox(bbox: [f64; 4]) -> Box<dyn Transform> {
        Box::new(FilterTransform::new(
            "intersects_bbox".to_string(),
            move |item| {
                let item = item.to_vec();
                Box::pin(async move {
                    let json: serde_json::Value = serde_json::from_slice(&item).map_err(|e| {
                        crate::error::TransformError::InvalidInput {
                            message: format!("Invalid JSON: {}", e),
                        }
                    })?;

                    // Extract bbox from GeoJSON, preferring an explicit `bbox` member and
                    // falling back to computing the extent from the geometry coordinates.
                    let item_bbox = if let Some(bbox_val) = json.get("bbox") {
                        extract_bbox_array(bbox_val)?
                    } else if let Some(computed) = compute_bbox_from_geometry(&json) {
                        computed
                    } else {
                        // A feature with no `bbox` member and no numeric coordinates has no
                        // spatial extent, so it cannot intersect the query box: filter it out
                        // rather than silently letting it pass.
                        return Ok(false);
                    };

                    // Check intersection
                    Ok(bboxes_intersect(&bbox, &item_bbox))
                })
            },
        ))
    }

    /// Filter by geometry type
    pub fn geometry_type(geom_type: String) -> Box<dyn Transform> {
        Box::new(FilterTransform::new(
            format!("geometry_type_{}", geom_type),
            move |item| {
                let geom_type = geom_type.clone();
                let item = item.to_vec();
                Box::pin(async move {
                    let json: serde_json::Value = serde_json::from_slice(&item).map_err(|e| {
                        crate::error::TransformError::InvalidInput {
                            message: format!("Invalid JSON: {}", e),
                        }
                    })?;

                    if let Some(geometry) = json.get("geometry")
                        && let Some(type_val) = geometry.get("type")
                        && let Some(type_str) = type_val.as_str()
                    {
                        return Ok(type_str == geom_type);
                    }

                    Ok(false)
                })
            },
        ))
    }

    /// Filter by property value range
    pub fn property_in_range(property: String, min: f64, max: f64) -> Box<dyn Transform> {
        Box::new(FilterTransform::new(
            format!("property_range_{}_{:.2}_{:.2}", property, min, max),
            move |item| {
                let property = property.clone();
                let item = item.to_vec();
                Box::pin(async move {
                    let json: serde_json::Value = serde_json::from_slice(&item).map_err(|e| {
                        crate::error::TransformError::InvalidInput {
                            message: format!("Invalid JSON: {}", e),
                        }
                    })?;

                    if let Some(properties) = json.get("properties")
                        && let Some(value) = properties.get(&property)
                        && let Some(num) = value.as_f64()
                    {
                        return Ok(num >= min && num <= max);
                    }

                    Ok(false)
                })
            },
        ))
    }

    /// Filter by NDVI threshold
    pub fn ndvi_threshold(min_ndvi: f64) -> Box<dyn Transform> {
        Box::new(FilterTransform::new(
            format!("ndvi_threshold_{:.2}", min_ndvi),
            move |item| {
                let item = item.to_vec();
                Box::pin(async move {
                    // In real implementation, calculate NDVI from raster data
                    // For now, assume item contains NDVI value
                    let json: serde_json::Value = serde_json::from_slice(&item).map_err(|e| {
                        crate::error::TransformError::InvalidInput {
                            message: format!("Invalid JSON: {}", e),
                        }
                    })?;

                    if let Some(ndvi) = json.get("ndvi").and_then(|v| v.as_f64()) {
                        return Ok(ndvi >= min_ndvi);
                    }

                    Ok(false)
                })
            },
        ))
    }
}

/// Helper function to extract bbox array from JSON value
fn extract_bbox_array(value: &serde_json::Value) -> Result<[f64; 4]> {
    if let Some(arr) = value.as_array()
        && arr.len() >= 4
    {
        let bbox = [
            arr[0]
                .as_f64()
                .ok_or_else(|| crate::error::TransformError::InvalidInput {
                    message: "Invalid bbox value".to_string(),
                })?,
            arr[1]
                .as_f64()
                .ok_or_else(|| crate::error::TransformError::InvalidInput {
                    message: "Invalid bbox value".to_string(),
                })?,
            arr[2]
                .as_f64()
                .ok_or_else(|| crate::error::TransformError::InvalidInput {
                    message: "Invalid bbox value".to_string(),
                })?,
            arr[3]
                .as_f64()
                .ok_or_else(|| crate::error::TransformError::InvalidInput {
                    message: "Invalid bbox value".to_string(),
                })?,
        ];
        return Ok(bbox);
    }

    Err(crate::error::TransformError::InvalidInput {
        message: "Invalid bbox format".to_string(),
    }
    .into())
}

/// Compute the `[west, south, east, north]` bounding box of a GeoJSON value from its geometry
/// coordinates.
///
/// Walks the document looking for `"coordinates"` members (handling bare geometries, `Feature`,
/// `FeatureCollection` and `GeometryCollection` without geometry-type-specific logic) and folds
/// every leaf `[x, y(, z)]` tuple into a running extent. Returns `None` when the value contains
/// no numeric coordinate tuples (e.g. a geometry-less feature), so callers can distinguish
/// "no spatial extent" from a real bounding box.
fn compute_bbox_from_geometry(value: &serde_json::Value) -> Option<[f64; 4]> {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    accumulate_coordinates(value, &mut min_x, &mut min_y, &mut max_x, &mut max_y);

    if min_x.is_finite() && min_y.is_finite() && max_x.is_finite() && max_y.is_finite() {
        Some([min_x, min_y, max_x, max_y])
    } else {
        None
    }
}

/// Recursively descends a JSON value looking for GeoJSON `"coordinates"` members and folds each
/// coordinate tuple found beneath them into the running extent.
fn accumulate_coordinates(
    value: &serde_json::Value,
    min_x: &mut f64,
    min_y: &mut f64,
    max_x: &mut f64,
    max_y: &mut f64,
) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                if key == "coordinates" {
                    accumulate_coordinate_array(child, min_x, min_y, max_x, max_y);
                } else {
                    accumulate_coordinates(child, min_x, min_y, max_x, max_y);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                accumulate_coordinates(item, min_x, min_y, max_x, max_y);
            }
        }
        _ => {}
    }
}

/// Folds a GeoJSON `"coordinates"` value (nested to any depth) into the running extent.
fn accumulate_coordinate_array(
    value: &serde_json::Value,
    min_x: &mut f64,
    min_y: &mut f64,
    max_x: &mut f64,
    max_y: &mut f64,
) {
    let Some(items) = value.as_array() else {
        return;
    };

    let is_leaf_tuple = !items.is_empty() && items.iter().all(serde_json::Value::is_number);
    if is_leaf_tuple {
        if let (Some(x), Some(y)) = (
            items.first().and_then(serde_json::Value::as_f64),
            items.get(1).and_then(serde_json::Value::as_f64),
        ) {
            *min_x = min_x.min(x);
            *min_y = min_y.min(y);
            *max_x = max_x.max(x);
            *max_y = max_y.max(y);
        }
    } else {
        for item in items {
            accumulate_coordinate_array(item, min_x, min_y, max_x, max_y);
        }
    }
}

/// Check if two bounding boxes intersect
fn bboxes_intersect(bbox1: &[f64; 4], bbox2: &[f64; 4]) -> bool {
    // [west, south, east, north]
    !(bbox1[2] < bbox2[0] || bbox1[0] > bbox2[2] || bbox1[3] < bbox2[1] || bbox1[1] > bbox2[3])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_min_size_filter() {
        let filter = FilterOperator::min_size(5);

        let result1 = filter.transform(vec![1, 2, 3, 4, 5]).await.expect("Failed");
        assert_eq!(result1.len(), 1); // Passes

        let result2 = filter.transform(vec![1, 2, 3]).await.expect("Failed");
        assert_eq!(result2.len(), 0); // Filtered out
    }

    #[tokio::test]
    async fn test_json_field_equals() {
        let filter =
            FilterOperator::json_field_equals("type".to_string(), serde_json::json!("Feature"));

        let json1 = serde_json::json!({"type": "Feature", "properties": {}});
        let item1 = serde_json::to_vec(&json1).expect("Failed to serialize");
        let result1 = filter.transform(item1).await.expect("Failed");
        assert_eq!(result1.len(), 1); // Passes

        let json2 = serde_json::json!({"type": "FeatureCollection"});
        let item2 = serde_json::to_vec(&json2).expect("Failed to serialize");
        let result2 = filter.transform(item2).await.expect("Failed");
        assert_eq!(result2.len(), 0); // Filtered out
    }

    #[tokio::test]
    async fn test_contains_string() {
        let filter = FilterOperator::contains_string("test".to_string());

        let result1 = filter
            .transform(b"this is a test".to_vec())
            .await
            .expect("Failed");
        assert_eq!(result1.len(), 1); // Passes

        let result2 = filter
            .transform(b"no match here".to_vec())
            .await
            .expect("Failed");
        assert_eq!(result2.len(), 0); // Filtered out
    }

    #[tokio::test]
    async fn test_sample_filter() {
        let filter = FilterOperator::sample(2); // Keep every 2nd item

        let result0 = filter.transform(vec![1]).await.expect("Failed");
        assert_eq!(result0.len(), 1); // 0th item, passes

        let result1 = filter.transform(vec![2]).await.expect("Failed");
        assert_eq!(result1.len(), 0); // 1st item, filtered

        let result2 = filter.transform(vec![3]).await.expect("Failed");
        assert_eq!(result2.len(), 1); // 2nd item, passes
    }

    #[tokio::test]
    async fn test_bbox_intersection() {
        let bbox1 = [0.0, 0.0, 10.0, 10.0];
        let bbox2 = [5.0, 5.0, 15.0, 15.0];
        assert!(bboxes_intersect(&bbox1, &bbox2));

        let bbox3 = [20.0, 20.0, 30.0, 30.0];
        assert!(!bboxes_intersect(&bbox1, &bbox3));
    }

    #[test]
    fn test_compute_bbox_from_point_geometry() {
        let json = serde_json::json!({
            "type": "Feature",
            "geometry": { "type": "Point", "coordinates": [5.0, 7.0] }
        });
        assert_eq!(
            compute_bbox_from_geometry(&json),
            Some([5.0, 7.0, 5.0, 7.0])
        );
    }

    #[test]
    fn test_compute_bbox_from_polygon_geometry() {
        let json = serde_json::json!({
            "type": "Feature",
            "geometry": {
                "type": "Polygon",
                "coordinates": [[[10.0, 20.0], [15.0, 25.0], [5.0, 30.0], [10.0, 20.0]]]
            }
        });
        assert_eq!(
            compute_bbox_from_geometry(&json),
            Some([5.0, 20.0, 15.0, 30.0])
        );
    }

    #[test]
    fn test_compute_bbox_none_without_coordinates() {
        let json = serde_json::json!({"type": "Feature", "properties": {"name": "x"}});
        assert_eq!(compute_bbox_from_geometry(&json), None);
    }

    #[tokio::test]
    async fn test_intersects_bbox_geometry_inside() {
        // Query box [0,0,10,10]; point geometry at (5,5) with no explicit bbox must pass.
        let filter = GeoFilterOperator::intersects_bbox([0.0, 0.0, 10.0, 10.0]);
        let json = serde_json::json!({
            "type": "Feature",
            "geometry": { "type": "Point", "coordinates": [5.0, 5.0] }
        });
        let item = serde_json::to_vec(&json).expect("serialize");
        let result = filter.transform(item).await.expect("transform");
        assert_eq!(result.len(), 1, "geometry inside query box should pass");
    }

    #[tokio::test]
    async fn test_intersects_bbox_geometry_outside() {
        // Query box [0,0,10,10]; point geometry at (50,50) with no explicit bbox must be dropped.
        let filter = GeoFilterOperator::intersects_bbox([0.0, 0.0, 10.0, 10.0]);
        let json = serde_json::json!({
            "type": "Feature",
            "geometry": { "type": "Point", "coordinates": [50.0, 50.0] }
        });
        let item = serde_json::to_vec(&json).expect("serialize");
        let result = filter.transform(item).await.expect("transform");
        assert_eq!(
            result.len(),
            0,
            "geometry outside query box must be filtered out, not passed through"
        );
    }

    #[tokio::test]
    async fn test_intersects_bbox_no_geometry_filtered() {
        // A feature with neither `bbox` nor coordinates has no spatial extent and is dropped.
        let filter = GeoFilterOperator::intersects_bbox([0.0, 0.0, 10.0, 10.0]);
        let json = serde_json::json!({"type": "Feature", "properties": {}});
        let item = serde_json::to_vec(&json).expect("serialize");
        let result = filter.transform(item).await.expect("transform");
        assert_eq!(
            result.len(),
            0,
            "feature without geometry must be filtered out"
        );
    }

    #[tokio::test]
    async fn test_intersects_bbox_explicit_bbox_still_used() {
        // An explicit `bbox` member takes precedence and is honored.
        let filter = GeoFilterOperator::intersects_bbox([0.0, 0.0, 10.0, 10.0]);
        let json = serde_json::json!({
            "type": "Feature",
            "bbox": [20.0, 20.0, 30.0, 30.0],
            "geometry": { "type": "Point", "coordinates": [5.0, 5.0] }
        });
        let item = serde_json::to_vec(&json).expect("serialize");
        let result = filter.transform(item).await.expect("transform");
        assert_eq!(
            result.len(),
            0,
            "explicit non-intersecting bbox must take precedence over geometry"
        );
    }
}
