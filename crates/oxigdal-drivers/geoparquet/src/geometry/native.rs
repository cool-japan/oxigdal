//! GeoArrow native geometry encoding for GeoParquet 1.1.
//!
//! Spec reference: <https://geoarrow.org/format.html> (GeoArrow 1.1.0,
//! interleaved layout).  This module implements pure-Rust round-trip for the
//! six unit GeoArrow encodings — `point`, `linestring`, `polygon`,
//! `multipoint`, `multilinestring`, `multipolygon` — using only `arrow_array`
//! / `arrow_buffer` primitives (no `geoarrow-rs` dep).
//!
//! # Array shapes (interleaved coords)
//!
//! Coordinates are always stored in a `FixedSizeList<f64, N>` where
//! `N = CoordDim::arity()` (2 / 3 / 4).  The outer wrapping differs per
//! encoding:
//!
//! | Encoding | Outer shape | Note |
//! |---|---|---|
//! | `point`            | `FixedSizeList<f64, N>` | one row per point |
//! | `linestring`       | `List<FixedSizeList<f64, N>>` | one row per line |
//! | `polygon`          | `List<List<FixedSizeList<f64, N>>>` | rings: ext + holes |
//! | `multipoint`       | `List<FixedSizeList<f64, N>>` | (same shape as linestring) |
//! | `multilinestring`  | `List<List<FixedSizeList<f64, N>>>` | (same shape as polygon) |
//! | `multipolygon`     | `List<List<List<FixedSizeList<f64, N>>>>` | |
//!
//! Note that `linestring` / `multipoint` share an Arrow shape, and
//! `polygon` / `multilinestring` share an Arrow shape; disambiguation comes
//! from the field's `ARROW:extension:name` metadata, not its `DataType`.
//!
//! # Mixed types
//!
//! GeoParquet 1.1 forbids mixing geometry types in a native column (only WKB
//! columns may be heterogeneous).  Encoders here return
//! [`GeoParquetError::InvalidEncoding`] when given input that does not match
//! the declared encoding.

use crate::error::{GeoParquetError, Result};
use crate::geometry::{
    Coordinate, Geometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon,
};
use crate::metadata::{CoordDim, EncodingType};
use arrow_array::builder::{FixedSizeListBuilder, Float64Builder};
use arrow_array::{Array, FixedSizeListArray, Float64Array, ListArray};
use arrow_buffer::OffsetBuffer;
use arrow_schema::{DataType, Field};
use std::sync::Arc;

// ── Public API ──────────────────────────────────────────────────────────────────

/// Encode a slice of in-memory [`Geometry`] values into a GeoArrow native
/// Arrow array of the specified `encoding` and coordinate dimensionality.
///
/// # Errors
///
/// * Returns [`GeoParquetError::InvalidEncoding`] when any geometry's variant
///   does not match `encoding`.  GeoArrow forbids mixed types in a native
///   column.
/// * Returns [`GeoParquetError::InvalidEncoding`] when WKB is requested
///   (callers should use the WKB encoder path instead).
/// * Propagates any Arrow build errors.
pub fn encode_native_array(
    geoms: &[Geometry],
    encoding: EncodingType,
    dim: CoordDim,
) -> Result<Arc<dyn Array>> {
    match encoding {
        EncodingType::Wkb => Err(GeoParquetError::invalid_encoding(
            "encode_native_array does not support EncodingType::Wkb; use the WKB writer instead",
        )),
        EncodingType::Point => encode_point_array(geoms, dim),
        EncodingType::LineString => encode_linestring_array(geoms, dim),
        EncodingType::Polygon => encode_polygon_array(geoms, dim),
        EncodingType::MultiPoint => encode_multipoint_array(geoms, dim),
        EncodingType::MultiLineString => encode_multilinestring_array(geoms, dim),
        EncodingType::MultiPolygon => encode_multipolygon_array(geoms, dim),
    }
}

/// Decode a GeoArrow native Arrow array back into a `Vec<Geometry>`.
///
/// `encoding` selects the geometry shape and is used to disambiguate the
/// shared Arrow layouts (`linestring`/`multipoint`,
/// `polygon`/`multilinestring`).  Coordinate dimensionality is inferred from
/// the FixedSizeList arity.
///
/// Null rows produce no entry in the returned vector — callers who need to
/// preserve null positions should use [`decode_native_array_optional`].
pub fn decode_native_array(arr: &dyn Array, encoding: EncodingType) -> Result<Vec<Geometry>> {
    let optional = decode_native_array_optional(arr, encoding)?;
    Ok(optional.into_iter().flatten().collect())
}

/// Like [`decode_native_array`] but preserves null positions as `None` entries.
pub fn decode_native_array_optional(
    arr: &dyn Array,
    encoding: EncodingType,
) -> Result<Vec<Option<Geometry>>> {
    match encoding {
        EncodingType::Wkb => Err(GeoParquetError::invalid_encoding(
            "decode_native_array does not support EncodingType::Wkb; use the WKB reader instead",
        )),
        EncodingType::Point => decode_point_array(arr),
        EncodingType::LineString => decode_linestring_array(arr),
        EncodingType::Polygon => decode_polygon_array(arr),
        EncodingType::MultiPoint => decode_multipoint_array(arr),
        EncodingType::MultiLineString => decode_multilinestring_array(arr),
        EncodingType::MultiPolygon => decode_multipolygon_array(arr),
    }
}

/// Per-row bbox-mask fast path for native geometry arrays.
///
/// Returns a boolean mask the same length as `arr` where each `true` entry
/// means that row's geometry intersects the query bbox `(qxmin, qymin, qxmax,
/// qymax)`.  Null rows are `false`.
///
/// This is the parallel of `wkb_bbox_mask` for native arrays — it reads only
/// the coord-array buffer and never materialises a `Geometry` value.
pub(crate) fn native_bbox_mask(
    arr: &dyn Array,
    encoding: EncodingType,
    qxmin: f64,
    qymin: f64,
    qxmax: f64,
    qymax: f64,
) -> Result<Vec<bool>> {
    match encoding {
        EncodingType::Wkb => Err(GeoParquetError::invalid_encoding(
            "native_bbox_mask cannot be used with EncodingType::Wkb",
        )),
        EncodingType::Point => point_bbox_mask(arr, qxmin, qymin, qxmax, qymax),
        EncodingType::LineString | EncodingType::MultiPoint => {
            list_of_points_bbox_mask(arr, qxmin, qymin, qxmax, qymax)
        }
        EncodingType::Polygon | EncodingType::MultiLineString => {
            list_of_list_of_points_bbox_mask(arr, qxmin, qymin, qxmax, qymax)
        }
        EncodingType::MultiPolygon => {
            list_of_list_of_list_of_points_bbox_mask(arr, qxmin, qymin, qxmax, qymax)
        }
    }
}

// ── Encoders ────────────────────────────────────────────────────────────────────

fn coord_field(dim: CoordDim) -> Arc<Field> {
    let name = match dim {
        CoordDim::Xy => "xy",
        CoordDim::Xyz => "xyz",
        CoordDim::Xym => "xym",
        CoordDim::Xyzm => "xyzm",
    };
    Arc::new(Field::new(name, DataType::Float64, false))
}

/// Append a single coordinate's `arity` f64 components into a Float64Builder.
fn push_coord(coord: &Coordinate, dim: CoordDim, b: &mut Float64Builder) {
    b.append_value(coord.x);
    b.append_value(coord.y);
    match dim {
        CoordDim::Xy => {}
        CoordDim::Xyz => b.append_value(coord.z.unwrap_or(0.0)),
        CoordDim::Xym => b.append_value(coord.m.unwrap_or(0.0)),
        CoordDim::Xyzm => {
            b.append_value(coord.z.unwrap_or(0.0));
            b.append_value(coord.m.unwrap_or(0.0));
        }
    }
}

fn encode_point_array(geoms: &[Geometry], dim: CoordDim) -> Result<Arc<dyn Array>> {
    let arity = dim.arity();
    let values_builder = Float64Builder::with_capacity(geoms.len() * arity);
    let mut b =
        FixedSizeListBuilder::new(values_builder, arity as i32).with_field(coord_field(dim));
    for g in geoms {
        match g {
            Geometry::Point(p) => {
                push_coord(&p.coord, dim, b.values());
                b.append(true);
            }
            other => {
                return Err(GeoParquetError::invalid_encoding(format!(
                    "encode point array: expected Point, got {}",
                    other.type_name()
                )));
            }
        }
    }
    Ok(Arc::new(b.finish()))
}

/// Build a `FixedSizeListArray<f64, arity>` from a flat slice of coords.
/// Every position in `coords` produces one valid (non-null) entry.
fn build_coord_fsl(coords: &[Coordinate], dim: CoordDim) -> FixedSizeListArray {
    let arity = dim.arity();
    let mut values = Float64Builder::with_capacity(coords.len() * arity);
    for c in coords {
        match dim {
            CoordDim::Xy => {
                values.append_value(c.x);
                values.append_value(c.y);
            }
            CoordDim::Xyz => {
                values.append_value(c.x);
                values.append_value(c.y);
                values.append_value(c.z.unwrap_or(0.0));
            }
            CoordDim::Xym => {
                values.append_value(c.x);
                values.append_value(c.y);
                values.append_value(c.m.unwrap_or(0.0));
            }
            CoordDim::Xyzm => {
                values.append_value(c.x);
                values.append_value(c.y);
                values.append_value(c.z.unwrap_or(0.0));
                values.append_value(c.m.unwrap_or(0.0));
            }
        }
    }
    let values_arr: Float64Array = values.finish();
    FixedSizeListArray::new(coord_field(dim), arity as i32, Arc::new(values_arr), None)
}

fn encode_linestring_array(geoms: &[Geometry], dim: CoordDim) -> Result<Arc<dyn Array>> {
    // Outer: List<FixedSizeList<f64, N>>
    let mut all_coords: Vec<Coordinate> = Vec::new();
    let mut offsets: Vec<i32> = Vec::with_capacity(geoms.len() + 1);
    offsets.push(0);
    for g in geoms {
        match g {
            Geometry::LineString(ls) => {
                all_coords.extend(ls.coords.iter().copied());
                offsets.push(all_coords.len() as i32);
            }
            other => {
                return Err(GeoParquetError::invalid_encoding(format!(
                    "encode linestring array: expected LineString, got {}",
                    other.type_name()
                )));
            }
        }
    }
    let inner = build_coord_fsl(&all_coords, dim);
    let inner_field = Arc::new(Field::new("vertices", inner.data_type().clone(), false));
    let offset_buf = OffsetBuffer::new(offsets.into());
    let arr = ListArray::new(inner_field, offset_buf, Arc::new(inner), None);
    Ok(Arc::new(arr))
}

fn encode_multipoint_array(geoms: &[Geometry], dim: CoordDim) -> Result<Arc<dyn Array>> {
    let mut all_coords: Vec<Coordinate> = Vec::new();
    let mut offsets: Vec<i32> = Vec::with_capacity(geoms.len() + 1);
    offsets.push(0);
    for g in geoms {
        match g {
            Geometry::MultiPoint(mp) => {
                for p in &mp.points {
                    all_coords.push(p.coord);
                }
                offsets.push(all_coords.len() as i32);
            }
            other => {
                return Err(GeoParquetError::invalid_encoding(format!(
                    "encode multipoint array: expected MultiPoint, got {}",
                    other.type_name()
                )));
            }
        }
    }
    let inner = build_coord_fsl(&all_coords, dim);
    let inner_field = Arc::new(Field::new("vertices", inner.data_type().clone(), false));
    let offset_buf = OffsetBuffer::new(offsets.into());
    let arr = ListArray::new(inner_field, offset_buf, Arc::new(inner), None);
    Ok(Arc::new(arr))
}

fn encode_polygon_array(geoms: &[Geometry], dim: CoordDim) -> Result<Arc<dyn Array>> {
    // Outer: List<List<FixedSizeList<f64, N>>>
    // We build the inner ring-list array, then wrap it in another ListArray.

    // Per-ring offsets (into the coord FSL).
    let mut ring_coord_offsets: Vec<i32> = vec![0];
    let mut all_coords: Vec<Coordinate> = Vec::new();
    // Per-geometry ring-count offsets (into the ring list).
    let mut geom_ring_offsets: Vec<i32> = vec![0];

    for g in geoms {
        match g {
            Geometry::Polygon(poly) => {
                // Exterior first.
                all_coords.extend(poly.exterior.coords.iter().copied());
                ring_coord_offsets.push(all_coords.len() as i32);
                for hole in &poly.interiors {
                    all_coords.extend(hole.coords.iter().copied());
                    ring_coord_offsets.push(all_coords.len() as i32);
                }
                let total_rings_so_far = ring_coord_offsets.len() as i32 - 1;
                geom_ring_offsets.push(total_rings_so_far);
            }
            other => {
                return Err(GeoParquetError::invalid_encoding(format!(
                    "encode polygon array: expected Polygon, got {}",
                    other.type_name()
                )));
            }
        }
    }

    let inner_fsl = build_coord_fsl(&all_coords, dim);
    let coord_arr_field = Arc::new(Field::new("vertices", inner_fsl.data_type().clone(), false));
    let ring_offsets_buf = OffsetBuffer::new(ring_coord_offsets.into());
    let ring_array = ListArray::new(coord_arr_field, ring_offsets_buf, Arc::new(inner_fsl), None);

    let ring_field = Arc::new(Field::new("rings", ring_array.data_type().clone(), false));
    let geom_offsets_buf = OffsetBuffer::new(geom_ring_offsets.into());
    let outer = ListArray::new(ring_field, geom_offsets_buf, Arc::new(ring_array), None);
    Ok(Arc::new(outer))
}

fn encode_multilinestring_array(geoms: &[Geometry], dim: CoordDim) -> Result<Arc<dyn Array>> {
    let mut line_coord_offsets: Vec<i32> = vec![0];
    let mut all_coords: Vec<Coordinate> = Vec::new();
    let mut geom_line_offsets: Vec<i32> = vec![0];

    for g in geoms {
        match g {
            Geometry::MultiLineString(mls) => {
                for line in &mls.linestrings {
                    all_coords.extend(line.coords.iter().copied());
                    line_coord_offsets.push(all_coords.len() as i32);
                }
                let total_lines_so_far = line_coord_offsets.len() as i32 - 1;
                geom_line_offsets.push(total_lines_so_far);
            }
            other => {
                return Err(GeoParquetError::invalid_encoding(format!(
                    "encode multilinestring array: expected MultiLineString, got {}",
                    other.type_name()
                )));
            }
        }
    }

    let inner_fsl = build_coord_fsl(&all_coords, dim);
    let coord_arr_field = Arc::new(Field::new("vertices", inner_fsl.data_type().clone(), false));
    let line_offsets_buf = OffsetBuffer::new(line_coord_offsets.into());
    let line_array = ListArray::new(coord_arr_field, line_offsets_buf, Arc::new(inner_fsl), None);

    let line_field = Arc::new(Field::new("rings", line_array.data_type().clone(), false));
    let geom_offsets_buf = OffsetBuffer::new(geom_line_offsets.into());
    let outer = ListArray::new(line_field, geom_offsets_buf, Arc::new(line_array), None);
    Ok(Arc::new(outer))
}

fn encode_multipolygon_array(geoms: &[Geometry], dim: CoordDim) -> Result<Arc<dyn Array>> {
    // List<List<List<FixedSizeList<f64,N>>>>
    let mut ring_coord_offsets: Vec<i32> = vec![0];
    let mut all_coords: Vec<Coordinate> = Vec::new();
    let mut polygon_ring_offsets: Vec<i32> = vec![0];
    let mut geom_polygon_offsets: Vec<i32> = vec![0];

    for g in geoms {
        match g {
            Geometry::MultiPolygon(mp) => {
                for poly in &mp.polygons {
                    all_coords.extend(poly.exterior.coords.iter().copied());
                    ring_coord_offsets.push(all_coords.len() as i32);
                    for hole in &poly.interiors {
                        all_coords.extend(hole.coords.iter().copied());
                        ring_coord_offsets.push(all_coords.len() as i32);
                    }
                    let total_rings = ring_coord_offsets.len() as i32 - 1;
                    polygon_ring_offsets.push(total_rings);
                }
                let total_polygons = polygon_ring_offsets.len() as i32 - 1;
                geom_polygon_offsets.push(total_polygons);
            }
            other => {
                return Err(GeoParquetError::invalid_encoding(format!(
                    "encode multipolygon array: expected MultiPolygon, got {}",
                    other.type_name()
                )));
            }
        }
    }

    let inner_fsl = build_coord_fsl(&all_coords, dim);
    let coord_field = Arc::new(Field::new("vertices", inner_fsl.data_type().clone(), false));
    let ring_offsets_buf = OffsetBuffer::new(ring_coord_offsets.into());
    let ring_array = ListArray::new(coord_field, ring_offsets_buf, Arc::new(inner_fsl), None);

    let ring_field = Arc::new(Field::new("rings", ring_array.data_type().clone(), false));
    let polygon_offsets_buf = OffsetBuffer::new(polygon_ring_offsets.into());
    let polygon_array = ListArray::new(ring_field, polygon_offsets_buf, Arc::new(ring_array), None);

    let polygon_field = Arc::new(Field::new(
        "polygons",
        polygon_array.data_type().clone(),
        false,
    ));
    let geom_offsets_buf = OffsetBuffer::new(geom_polygon_offsets.into());
    let outer = ListArray::new(
        polygon_field,
        geom_offsets_buf,
        Arc::new(polygon_array),
        None,
    );
    Ok(Arc::new(outer))
}

// ── Decoders ────────────────────────────────────────────────────────────────────

/// Coordinate-array reader backed by a `FixedSizeList<f64, arity>` slice.
struct CoordReader<'a> {
    values: &'a [f64],
    arity: usize,
    has_z: bool,
    has_m: bool,
}

impl<'a> CoordReader<'a> {
    fn new(fsl: &'a FixedSizeListArray) -> Result<Self> {
        let arity = fsl.value_length() as usize;
        let dim = CoordDim::from_arity(arity).ok_or_else(|| {
            GeoParquetError::invalid_encoding(format!(
                "FixedSizeList arity {arity} is not a valid coord dimensionality (2/3/4)"
            ))
        })?;
        let values_arr = fsl
            .values()
            .as_any()
            .downcast_ref::<Float64Array>()
            .ok_or_else(|| {
                GeoParquetError::invalid_encoding("FixedSizeList values are not Float64Array")
            })?;
        Ok(Self {
            values: values_arr.values(),
            arity,
            has_z: dim.has_z(),
            has_m: dim.has_m(),
        })
    }

    fn coord_at(&self, idx: usize) -> Coordinate {
        let base = idx * self.arity;
        // Guard with a defensive default — this is an internal API and the
        // length is checked by callers.  The match below is structurally
        // exhaustive given arity ∈ {2, 3, 4}.
        match self.arity {
            2 => Coordinate {
                x: self.values[base],
                y: self.values[base + 1],
                z: None,
                m: None,
            },
            3 if self.has_z => Coordinate {
                x: self.values[base],
                y: self.values[base + 1],
                z: Some(self.values[base + 2]),
                m: None,
            },
            3 => Coordinate {
                x: self.values[base],
                y: self.values[base + 1],
                z: None,
                m: Some(self.values[base + 2]),
            },
            4 => Coordinate {
                x: self.values[base],
                y: self.values[base + 1],
                z: Some(self.values[base + 2]),
                m: Some(self.values[base + 3]),
            },
            _ => Coordinate {
                x: self.values[base],
                y: self.values[base + 1],
                z: None,
                m: None,
            },
        }
    }
}

fn downcast_fsl(arr: &dyn Array) -> Result<&FixedSizeListArray> {
    arr.as_any()
        .downcast_ref::<FixedSizeListArray>()
        .ok_or_else(|| {
            GeoParquetError::invalid_encoding(format!(
                "expected FixedSizeListArray, got {:?}",
                arr.data_type()
            ))
        })
}

fn downcast_list(arr: &dyn Array) -> Result<&ListArray> {
    arr.as_any().downcast_ref::<ListArray>().ok_or_else(|| {
        GeoParquetError::invalid_encoding(format!("expected ListArray, got {:?}", arr.data_type()))
    })
}

fn decode_point_array(arr: &dyn Array) -> Result<Vec<Option<Geometry>>> {
    let fsl = downcast_fsl(arr)?;
    let coords = CoordReader::new(fsl)?;
    let mut out = Vec::with_capacity(fsl.len());
    for i in 0..fsl.len() {
        if fsl.is_null(i) {
            out.push(None);
            continue;
        }
        let coord = coords.coord_at(i);
        out.push(Some(Geometry::Point(Point::new(coord))));
    }
    Ok(out)
}

/// Decode a `List<FixedSizeList<f64, N>>` array as either LineString or
/// MultiPoint, depending on `into_geom`.
fn decode_list_of_points<F>(arr: &dyn Array, mut into_geom: F) -> Result<Vec<Option<Geometry>>>
where
    F: FnMut(Vec<Coordinate>) -> Geometry,
{
    let outer = downcast_list(arr)?;
    let values = outer.values();
    let fsl = downcast_fsl(values.as_ref())?;
    let coords = CoordReader::new(fsl)?;

    let offsets = outer.offsets();
    let mut out = Vec::with_capacity(outer.len());
    for i in 0..outer.len() {
        if outer.is_null(i) {
            out.push(None);
            continue;
        }
        let start = offsets[i] as usize;
        let end = offsets[i + 1] as usize;
        let mut buf = Vec::with_capacity(end - start);
        for j in start..end {
            buf.push(coords.coord_at(j));
        }
        out.push(Some(into_geom(buf)));
    }
    Ok(out)
}

fn decode_linestring_array(arr: &dyn Array) -> Result<Vec<Option<Geometry>>> {
    decode_list_of_points(arr, |coords| Geometry::LineString(LineString::new(coords)))
}

fn decode_multipoint_array(arr: &dyn Array) -> Result<Vec<Option<Geometry>>> {
    decode_list_of_points(arr, |coords| {
        let pts = coords.into_iter().map(Point::new).collect();
        Geometry::MultiPoint(MultiPoint::new(pts))
    })
}

/// Decode a `List<List<FixedSizeList<f64, N>>>` array into a Vec of ring lists,
/// one per outer-list entry.  Inner-most level is a coord list.
fn collect_list_of_list_of_coords(arr: &dyn Array) -> Result<Vec<Option<Vec<Vec<Coordinate>>>>> {
    let outer = downcast_list(arr)?;
    let mid = downcast_list(outer.values().as_ref())?;
    let fsl = downcast_fsl(mid.values().as_ref())?;
    let coords = CoordReader::new(fsl)?;

    let outer_offsets = outer.offsets();
    let mid_offsets = mid.offsets();

    let mut out = Vec::with_capacity(outer.len());
    for i in 0..outer.len() {
        if outer.is_null(i) {
            out.push(None);
            continue;
        }
        let geom_start = outer_offsets[i] as usize;
        let geom_end = outer_offsets[i + 1] as usize;
        let mut sub: Vec<Vec<Coordinate>> = Vec::with_capacity(geom_end - geom_start);
        for j in geom_start..geom_end {
            let coord_start = mid_offsets[j] as usize;
            let coord_end = mid_offsets[j + 1] as usize;
            let mut buf = Vec::with_capacity(coord_end - coord_start);
            for k in coord_start..coord_end {
                buf.push(coords.coord_at(k));
            }
            sub.push(buf);
        }
        out.push(Some(sub));
    }
    Ok(out)
}

fn decode_polygon_array(arr: &dyn Array) -> Result<Vec<Option<Geometry>>> {
    let raw = collect_list_of_list_of_coords(arr)?;
    let mut out = Vec::with_capacity(raw.len());
    for entry in raw {
        match entry {
            None => out.push(None),
            Some(rings) => {
                if rings.is_empty() {
                    return Err(GeoParquetError::invalid_encoding(
                        "polygon must have at least one ring",
                    ));
                }
                let mut rings_iter = rings.into_iter();
                let exterior_coords = rings_iter
                    .next()
                    .ok_or_else(|| GeoParquetError::invalid_encoding("missing exterior ring"))?;
                let interiors = rings_iter.map(LineString::new).collect();
                let exterior = LineString::new(exterior_coords);
                out.push(Some(Geometry::Polygon(Polygon::new(exterior, interiors))));
            }
        }
    }
    Ok(out)
}

fn decode_multilinestring_array(arr: &dyn Array) -> Result<Vec<Option<Geometry>>> {
    let raw = collect_list_of_list_of_coords(arr)?;
    let mut out = Vec::with_capacity(raw.len());
    for entry in raw {
        match entry {
            None => out.push(None),
            Some(lines) => {
                let lss = lines.into_iter().map(LineString::new).collect();
                out.push(Some(Geometry::MultiLineString(MultiLineString::new(lss))));
            }
        }
    }
    Ok(out)
}

fn decode_multipolygon_array(arr: &dyn Array) -> Result<Vec<Option<Geometry>>> {
    let outer = downcast_list(arr)?;
    let mid = downcast_list(outer.values().as_ref())?;
    let inner = downcast_list(mid.values().as_ref())?;
    let fsl = downcast_fsl(inner.values().as_ref())?;
    let coords = CoordReader::new(fsl)?;

    let outer_offsets = outer.offsets();
    let mid_offsets = mid.offsets();
    let inner_offsets = inner.offsets();

    let mut out = Vec::with_capacity(outer.len());
    for i in 0..outer.len() {
        if outer.is_null(i) {
            out.push(None);
            continue;
        }
        let polygon_start = outer_offsets[i] as usize;
        let polygon_end = outer_offsets[i + 1] as usize;
        let mut polygons = Vec::with_capacity(polygon_end - polygon_start);
        for p in polygon_start..polygon_end {
            let ring_start = mid_offsets[p] as usize;
            let ring_end = mid_offsets[p + 1] as usize;
            if ring_end <= ring_start {
                return Err(GeoParquetError::invalid_encoding(
                    "multipolygon entry with zero rings",
                ));
            }
            let mut rings = Vec::with_capacity(ring_end - ring_start);
            for r in ring_start..ring_end {
                let coord_start = inner_offsets[r] as usize;
                let coord_end = inner_offsets[r + 1] as usize;
                let mut buf = Vec::with_capacity(coord_end - coord_start);
                for k in coord_start..coord_end {
                    buf.push(coords.coord_at(k));
                }
                rings.push(LineString::new(buf));
            }
            let mut rings_iter = rings.into_iter();
            let exterior = rings_iter
                .next()
                .ok_or_else(|| GeoParquetError::invalid_encoding("missing exterior ring"))?;
            polygons.push(Polygon::new(exterior, rings_iter.collect()));
        }
        out.push(Some(Geometry::MultiPolygon(MultiPolygon::new(polygons))));
    }
    Ok(out)
}

// ── Bbox-mask helpers ───────────────────────────────────────────────────────────

fn bbox_intersects(
    xmin: f64,
    ymin: f64,
    xmax: f64,
    ymax: f64,
    qxmin: f64,
    qymin: f64,
    qxmax: f64,
    qymax: f64,
) -> bool {
    xmax >= qxmin && xmin <= qxmax && ymax >= qymin && ymin <= qymax
}

fn point_bbox_mask(
    arr: &dyn Array,
    qxmin: f64,
    qymin: f64,
    qxmax: f64,
    qymax: f64,
) -> Result<Vec<bool>> {
    let fsl = downcast_fsl(arr)?;
    let coords = CoordReader::new(fsl)?;
    let mut mask = vec![false; fsl.len()];
    for (i, m) in mask.iter_mut().enumerate() {
        if fsl.is_null(i) {
            continue;
        }
        let c = coords.coord_at(i);
        if bbox_intersects(c.x, c.y, c.x, c.y, qxmin, qymin, qxmax, qymax) {
            *m = true;
        }
    }
    Ok(mask)
}

fn coord_range_bbox(
    coords: &CoordReader<'_>,
    start: usize,
    end: usize,
) -> Option<(f64, f64, f64, f64)> {
    if end <= start {
        return None;
    }
    let mut xmin = f64::INFINITY;
    let mut ymin = f64::INFINITY;
    let mut xmax = f64::NEG_INFINITY;
    let mut ymax = f64::NEG_INFINITY;
    for i in start..end {
        let c = coords.coord_at(i);
        xmin = xmin.min(c.x);
        ymin = ymin.min(c.y);
        xmax = xmax.max(c.x);
        ymax = ymax.max(c.y);
    }
    if xmin.is_finite() {
        Some((xmin, ymin, xmax, ymax))
    } else {
        None
    }
}

fn list_of_points_bbox_mask(
    arr: &dyn Array,
    qxmin: f64,
    qymin: f64,
    qxmax: f64,
    qymax: f64,
) -> Result<Vec<bool>> {
    let outer = downcast_list(arr)?;
    let fsl = downcast_fsl(outer.values().as_ref())?;
    let coords = CoordReader::new(fsl)?;
    let offsets = outer.offsets();

    let mut mask = vec![false; outer.len()];
    for (i, m) in mask.iter_mut().enumerate() {
        if outer.is_null(i) {
            continue;
        }
        let start = offsets[i] as usize;
        let end = offsets[i + 1] as usize;
        if let Some((xmin, ymin, xmax, ymax)) = coord_range_bbox(&coords, start, end)
            && bbox_intersects(xmin, ymin, xmax, ymax, qxmin, qymin, qxmax, qymax)
        {
            *m = true;
        }
    }
    Ok(mask)
}

fn list_of_list_of_points_bbox_mask(
    arr: &dyn Array,
    qxmin: f64,
    qymin: f64,
    qxmax: f64,
    qymax: f64,
) -> Result<Vec<bool>> {
    let outer = downcast_list(arr)?;
    let mid = downcast_list(outer.values().as_ref())?;
    let fsl = downcast_fsl(mid.values().as_ref())?;
    let coords = CoordReader::new(fsl)?;
    let outer_offsets = outer.offsets();
    let mid_offsets = mid.offsets();

    let mut mask = vec![false; outer.len()];
    for (i, m) in mask.iter_mut().enumerate() {
        if outer.is_null(i) {
            continue;
        }
        let geom_start = outer_offsets[i] as usize;
        let geom_end = outer_offsets[i + 1] as usize;
        let mut overall: Option<(f64, f64, f64, f64)> = None;
        for j in geom_start..geom_end {
            let coord_start = mid_offsets[j] as usize;
            let coord_end = mid_offsets[j + 1] as usize;
            if let Some(b) = coord_range_bbox(&coords, coord_start, coord_end) {
                overall = Some(match overall {
                    None => b,
                    Some(o) => (o.0.min(b.0), o.1.min(b.1), o.2.max(b.2), o.3.max(b.3)),
                });
            }
        }
        if let Some((xmin, ymin, xmax, ymax)) = overall
            && bbox_intersects(xmin, ymin, xmax, ymax, qxmin, qymin, qxmax, qymax)
        {
            *m = true;
        }
    }
    Ok(mask)
}

fn list_of_list_of_list_of_points_bbox_mask(
    arr: &dyn Array,
    qxmin: f64,
    qymin: f64,
    qxmax: f64,
    qymax: f64,
) -> Result<Vec<bool>> {
    let outer = downcast_list(arr)?;
    let mid = downcast_list(outer.values().as_ref())?;
    let inner = downcast_list(mid.values().as_ref())?;
    let fsl = downcast_fsl(inner.values().as_ref())?;
    let coords = CoordReader::new(fsl)?;

    let outer_offsets = outer.offsets();
    let mid_offsets = mid.offsets();
    let inner_offsets = inner.offsets();

    let mut mask = vec![false; outer.len()];
    for (i, m) in mask.iter_mut().enumerate() {
        if outer.is_null(i) {
            continue;
        }
        let polygon_start = outer_offsets[i] as usize;
        let polygon_end = outer_offsets[i + 1] as usize;
        let mut overall: Option<(f64, f64, f64, f64)> = None;
        for p in polygon_start..polygon_end {
            let ring_start = mid_offsets[p] as usize;
            let ring_end = mid_offsets[p + 1] as usize;
            for r in ring_start..ring_end {
                let coord_start = inner_offsets[r] as usize;
                let coord_end = inner_offsets[r + 1] as usize;
                if let Some(b) = coord_range_bbox(&coords, coord_start, coord_end) {
                    overall = Some(match overall {
                        None => b,
                        Some(o) => (o.0.min(b.0), o.1.min(b.1), o.2.max(b.2), o.3.max(b.3)),
                    });
                }
            }
        }
        if let Some((xmin, ymin, xmax, ymax)) = overall
            && bbox_intersects(xmin, ymin, xmax, ymax, qxmin, qymin, qxmax, qymax)
        {
            *m = true;
        }
    }
    Ok(mask)
}

// ── Tests ───────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::panic, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::geometry::{Coordinate, LineString};

    #[test]
    fn test_native_point_2d_encode_decode() {
        let geoms = vec![
            Geometry::Point(Point::new_2d(1.0, 2.0)),
            Geometry::Point(Point::new_2d(3.0, 4.0)),
            Geometry::Point(Point::new_2d(-5.0, 6.0)),
        ];
        let arr = encode_native_array(&geoms, EncodingType::Point, CoordDim::Xy).expect("encode");
        let back = decode_native_array(arr.as_ref(), EncodingType::Point).expect("decode");
        assert_eq!(back, geoms);
    }

    #[test]
    fn test_native_point_xyz_encode_decode() {
        let geoms = vec![
            Geometry::Point(Point::new_3d(1.0, 2.0, 3.0)),
            Geometry::Point(Point::new_3d(4.0, 5.0, 6.0)),
        ];
        let arr =
            encode_native_array(&geoms, EncodingType::Point, CoordDim::Xyz).expect("encode xyz");
        let back = decode_native_array(arr.as_ref(), EncodingType::Point).expect("decode xyz");
        assert_eq!(back.len(), 2);
        for (got, expected) in back.iter().zip(geoms.iter()) {
            assert_eq!(got, expected);
        }
    }

    #[test]
    fn test_native_linestring_roundtrip() {
        let coords1 = vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(2.0, 0.5),
        ];
        let coords2 = vec![
            Coordinate::new_2d(10.0, 10.0),
            Coordinate::new_2d(11.0, 11.0),
        ];
        let geoms = vec![
            Geometry::LineString(LineString::new(coords1)),
            Geometry::LineString(LineString::new(coords2)),
        ];
        let arr =
            encode_native_array(&geoms, EncodingType::LineString, CoordDim::Xy).expect("encode");
        let back = decode_native_array(arr.as_ref(), EncodingType::LineString).expect("decode");
        assert_eq!(back, geoms);
    }

    #[test]
    fn test_native_polygon_with_holes_roundtrip() {
        let exterior = LineString::new(vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(10.0, 0.0),
            Coordinate::new_2d(10.0, 10.0),
            Coordinate::new_2d(0.0, 10.0),
            Coordinate::new_2d(0.0, 0.0),
        ]);
        let hole = LineString::new(vec![
            Coordinate::new_2d(2.0, 2.0),
            Coordinate::new_2d(4.0, 2.0),
            Coordinate::new_2d(4.0, 4.0),
            Coordinate::new_2d(2.0, 4.0),
            Coordinate::new_2d(2.0, 2.0),
        ]);
        let poly = Polygon::new(exterior, vec![hole]);
        let geoms = vec![Geometry::Polygon(poly)];
        let arr = encode_native_array(&geoms, EncodingType::Polygon, CoordDim::Xy).expect("encode");
        let back = decode_native_array(arr.as_ref(), EncodingType::Polygon).expect("decode");
        assert_eq!(back, geoms);
    }

    #[test]
    fn test_native_multipoint_roundtrip() {
        let mp = MultiPoint::new(vec![
            Point::new_2d(0.0, 0.0),
            Point::new_2d(1.0, 0.0),
            Point::new_2d(0.0, 1.0),
        ]);
        let geoms = vec![Geometry::MultiPoint(mp)];
        let arr =
            encode_native_array(&geoms, EncodingType::MultiPoint, CoordDim::Xy).expect("encode");
        let back = decode_native_array(arr.as_ref(), EncodingType::MultiPoint).expect("decode");
        assert_eq!(back, geoms);
    }

    #[test]
    fn test_native_multilinestring_roundtrip() {
        let mls = MultiLineString::new(vec![
            LineString::new(vec![
                Coordinate::new_2d(0.0, 0.0),
                Coordinate::new_2d(1.0, 1.0),
            ]),
            LineString::new(vec![
                Coordinate::new_2d(5.0, 5.0),
                Coordinate::new_2d(6.0, 6.0),
            ]),
        ]);
        let geoms = vec![Geometry::MultiLineString(mls)];
        let arr = encode_native_array(&geoms, EncodingType::MultiLineString, CoordDim::Xy)
            .expect("encode");
        let back =
            decode_native_array(arr.as_ref(), EncodingType::MultiLineString).expect("decode");
        assert_eq!(back, geoms);
    }

    #[test]
    fn test_native_multipolygon_roundtrip() {
        let p1 = Polygon::new_simple(LineString::new(vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(0.0, 0.0),
        ]));
        let p2 = Polygon::new_simple(LineString::new(vec![
            Coordinate::new_2d(10.0, 10.0),
            Coordinate::new_2d(11.0, 10.0),
            Coordinate::new_2d(11.0, 11.0),
            Coordinate::new_2d(10.0, 10.0),
        ]));
        let mp = MultiPolygon::new(vec![p1, p2]);
        let geoms = vec![Geometry::MultiPolygon(mp)];
        let arr =
            encode_native_array(&geoms, EncodingType::MultiPolygon, CoordDim::Xy).expect("encode");
        let back = decode_native_array(arr.as_ref(), EncodingType::MultiPolygon).expect("decode");
        assert_eq!(back, geoms);
    }

    #[test]
    fn test_native_mixed_types_rejected_by_encode() {
        let geoms = vec![
            Geometry::Point(Point::new_2d(0.0, 0.0)),
            Geometry::LineString(LineString::new(vec![
                Coordinate::new_2d(1.0, 1.0),
                Coordinate::new_2d(2.0, 2.0),
            ])),
        ];
        let result = encode_native_array(&geoms, EncodingType::Point, CoordDim::Xy);
        assert!(result.is_err(), "mixed types must be rejected");
    }

    #[test]
    fn test_native_point_bbox_mask() {
        let geoms = vec![
            Geometry::Point(Point::new_2d(0.0, 0.0)),
            Geometry::Point(Point::new_2d(50.0, 50.0)),
            Geometry::Point(Point::new_2d(2.0, 2.0)),
        ];
        let arr = encode_native_array(&geoms, EncodingType::Point, CoordDim::Xy).expect("encode");
        let mask = native_bbox_mask(arr.as_ref(), EncodingType::Point, -1.0, -1.0, 5.0, 5.0)
            .expect("mask");
        assert_eq!(mask, vec![true, false, true]);
    }

    #[test]
    fn test_native_linestring_bbox_mask() {
        let ls_in = LineString::new(vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(2.0, 2.0),
        ]);
        let ls_out = LineString::new(vec![
            Coordinate::new_2d(100.0, 100.0),
            Coordinate::new_2d(101.0, 101.0),
        ]);
        let geoms = vec![Geometry::LineString(ls_in), Geometry::LineString(ls_out)];
        let arr =
            encode_native_array(&geoms, EncodingType::LineString, CoordDim::Xy).expect("encode");
        let mask = native_bbox_mask(arr.as_ref(), EncodingType::LineString, -1.0, -1.0, 5.0, 5.0)
            .expect("mask");
        assert_eq!(mask, vec![true, false]);
    }
}
