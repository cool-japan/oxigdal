//! Geometry encoding and decoding for the `FlatGeobuf` format
//!
//! Geometries are stored as `FlatBuffers` `Geometry` tables holding a flat
//! coordinate array (`xy`, optionally `z`/`m`), part end indices (`ends`) for
//! multi-ring / multi-part geometries, and nested `parts` tables for
//! `MultiPolygon` and `GeometryCollection`. This module converts between the
//! `OxiGDAL` geometry model and that on-disk representation, following the
//! official `FlatGeobuf` schema (`feature.fbs`).

use crate::error::{FlatGeobufError, Result};
use crate::fbs::{self, FbTable, Offset};
use crate::header::GeometryType;
use flatbuffers::FlatBufferBuilder;
use oxigdal_core::vector::{
    Coordinate, Geometry, GeometryCollection, LineString, MultiLineString, MultiPoint,
    MultiPolygon, Point, Polygon,
};

/// Geometry encoder/decoder bound to a coordinate dimensionality.
pub struct GeometryCodec {
    has_z: bool,
    has_m: bool,
}

impl GeometryCodec {
    /// Creates a new geometry codec
    #[must_use]
    pub const fn new(has_z: bool, has_m: bool) -> Self {
        Self { has_z, has_m }
    }

    /// Builds a top-level `Geometry` table for `geometry` into `fbb`.
    ///
    /// The `type` field is left at its default (`Unknown`) because the concrete
    /// type is carried by the enclosing header; elements of a
    /// `GeometryCollection` instead carry their own type.
    pub fn build(&self, fbb: &mut FlatBufferBuilder<'_>, geometry: &Geometry) -> Result<Offset> {
        self.build_geom(fbb, geometry, false)
    }

    /// Reads a `Geometry` table into an `OxiGDAL` [`Geometry`] using `geom_type`
    /// (typically the header geometry type). When `geom_type` is
    /// [`GeometryType::Unknown`] the table's own `type` field is consulted.
    pub fn read(&self, t: &FbTable<'_>, geom_type: GeometryType) -> Result<Geometry> {
        let gt = if geom_type == GeometryType::Unknown {
            GeometryType::from_u8(t.get_u8(fbs::GEOM_VT_TYPE, 0)?)?
        } else {
            geom_type
        };

        match gt {
            GeometryType::Point => {
                let coords = self.read_coords(t)?;
                let coord = coords
                    .into_iter()
                    .next()
                    .ok_or_else(|| FlatGeobufError::InvalidGeometry("empty Point".to_string()))?;
                Ok(Geometry::Point(Point::from_coord(coord)))
            }
            GeometryType::MultiPoint => {
                let coords = self.read_coords(t)?;
                let points = coords.into_iter().map(Point::from_coord).collect();
                Ok(Geometry::MultiPoint(MultiPoint::new(points)))
            }
            GeometryType::LineString => {
                let coords = self.read_coords(t)?;
                Ok(Geometry::LineString(
                    LineString::new(coords).map_err(FlatGeobufError::OxiGdal)?,
                ))
            }
            GeometryType::MultiLineString => {
                let coords = self.read_coords(t)?;
                let ends = t.get_u32_vector(fbs::GEOM_VT_ENDS)?;
                let parts = split_by_ends(&coords, ends.as_deref())?;
                let mut line_strings = Vec::with_capacity(parts.len());
                for part in parts {
                    line_strings.push(LineString::new(part).map_err(FlatGeobufError::OxiGdal)?);
                }
                Ok(Geometry::MultiLineString(MultiLineString::new(
                    line_strings,
                )))
            }
            GeometryType::Polygon => Ok(Geometry::Polygon(self.read_polygon(t)?)),
            GeometryType::MultiPolygon => {
                let parts = t.get_table_vector(fbs::GEOM_VT_PARTS)?;
                let mut polygons = Vec::new();
                if parts.is_empty() {
                    // A single-polygon MultiPolygon may be stored flat.
                    polygons.push(self.read_polygon(t)?);
                } else {
                    for part in &parts {
                        polygons.push(self.read_polygon(part)?);
                    }
                }
                Ok(Geometry::MultiPolygon(MultiPolygon::new(polygons)))
            }
            GeometryType::GeometryCollection => {
                let parts = t.get_table_vector(fbs::GEOM_VT_PARTS)?;
                let mut geometries = Vec::with_capacity(parts.len());
                for part in &parts {
                    // Each part declares its own type.
                    geometries.push(self.read(part, GeometryType::Unknown)?);
                }
                Ok(Geometry::GeometryCollection(GeometryCollection::new(
                    geometries,
                )))
            }
            other => Err(FlatGeobufError::UnsupportedGeometryType(other as u8)),
        }
    }

    /// Reads a `Polygon` from a `Geometry` table (exterior + optional holes).
    fn read_polygon(&self, t: &FbTable<'_>) -> Result<Polygon> {
        let coords = self.read_coords(t)?;
        let ends = t.get_u32_vector(fbs::GEOM_VT_ENDS)?;
        let rings = split_by_ends(&coords, ends.as_deref())?;
        let mut iter = rings.into_iter();
        let exterior = iter
            .next()
            .ok_or_else(|| FlatGeobufError::InvalidGeometry("Polygon has no rings".to_string()))?;
        let exterior = LineString::new(exterior).map_err(FlatGeobufError::OxiGdal)?;
        let mut interiors = Vec::new();
        for ring in iter {
            interiors.push(LineString::new(ring).map_err(FlatGeobufError::OxiGdal)?);
        }
        Polygon::new(exterior, interiors).map_err(FlatGeobufError::OxiGdal)
    }

    /// Reconstructs coordinates from the `xy` (and optional `z`/`m`) arrays.
    fn read_coords(&self, t: &FbTable<'_>) -> Result<Vec<Coordinate>> {
        let xy = t.get_f64_vector(fbs::GEOM_VT_XY)?.unwrap_or_default();
        if xy.len() % 2 != 0 {
            return Err(FlatGeobufError::InvalidGeometry(
                "xy array has an odd number of values".to_string(),
            ));
        }
        let n = xy.len() / 2;
        let z = if self.has_z {
            t.get_f64_vector(fbs::GEOM_VT_Z)?
        } else {
            None
        };
        let m = if self.has_m {
            t.get_f64_vector(fbs::GEOM_VT_M)?
        } else {
            None
        };

        let mut coords = Vec::with_capacity(n);
        for i in 0..n {
            let cz = if self.has_z {
                z.as_ref().and_then(|v| v.get(i).copied())
            } else {
                None
            };
            let cm = if self.has_m {
                m.as_ref().and_then(|v| v.get(i).copied())
            } else {
                None
            };
            coords.push(Coordinate {
                x: xy[2 * i],
                y: xy[2 * i + 1],
                z: cz,
                m: cm,
            });
        }
        Ok(coords)
    }

    /// Builds a `Geometry` table, optionally setting the `type` field
    /// (`set_type` is `true` for elements of a `GeometryCollection`).
    fn build_geom(
        &self,
        fbb: &mut FlatBufferBuilder<'_>,
        geometry: &Geometry,
        set_type: bool,
    ) -> Result<Offset> {
        let type_opt = set_type.then_some(geometry_type_of(geometry));
        match geometry {
            Geometry::Point(p) => self.build_simple(fbb, &[p.coord], &[], type_opt),
            Geometry::LineString(ls) => self.build_simple(fbb, &ls.coords, &[], type_opt),
            Geometry::MultiPoint(mp) => {
                let coords: Vec<Coordinate> = mp.points.iter().map(|p| p.coord).collect();
                self.build_simple(fbb, &coords, &[], type_opt)
            }
            Geometry::Polygon(poly) => self.build_polygon_geom(fbb, poly, type_opt),
            Geometry::MultiLineString(mls) => {
                let mut coords = Vec::new();
                let mut ends = Vec::new();
                if mls.line_strings.len() > 1 {
                    let mut acc = 0usize;
                    for ls in &mls.line_strings {
                        coords.extend_from_slice(&ls.coords);
                        acc += ls.coords.len();
                        ends.push(acc as u32);
                    }
                } else if let Some(ls) = mls.line_strings.first() {
                    coords.extend_from_slice(&ls.coords);
                }
                self.build_simple(fbb, &coords, &ends, type_opt)
            }
            Geometry::MultiPolygon(mp) => {
                let mut part_offs = Vec::with_capacity(mp.polygons.len());
                for poly in &mp.polygons {
                    part_offs.push(self.build_polygon_geom(fbb, poly, None)?);
                }
                Ok(self.build_parts(fbb, &part_offs, type_opt))
            }
            Geometry::GeometryCollection(gc) => {
                let mut part_offs = Vec::with_capacity(gc.geometries.len());
                for g in &gc.geometries {
                    part_offs.push(self.build_geom(fbb, g, true)?);
                }
                Ok(self.build_parts(fbb, &part_offs, type_opt))
            }
        }
    }

    /// Builds a `Geometry` table that only carries a `parts` vector (used for
    /// `MultiPolygon` and `GeometryCollection`).
    fn build_parts(
        &self,
        fbb: &mut FlatBufferBuilder<'_>,
        part_offs: &[Offset],
        type_opt: Option<GeometryType>,
    ) -> Offset {
        let parts_vec = if part_offs.is_empty() {
            None
        } else {
            Some(fbb.create_vector(part_offs))
        };
        let wip = fbb.start_table();
        if let Some(v) = parts_vec {
            fbb.push_slot_always(fbs::GEOM_VT_PARTS, v);
        }
        if let Some(gt) = type_opt {
            fbb.push_slot::<u8>(fbs::GEOM_VT_TYPE, gt as u8, 0);
        }
        fbb.end_table(wip)
    }

    /// Builds a `Geometry` table for a polygon (exterior ring plus holes),
    /// emitting cumulative `ends` when there is more than one ring.
    fn build_polygon_geom(
        &self,
        fbb: &mut FlatBufferBuilder<'_>,
        poly: &Polygon,
        type_opt: Option<GeometryType>,
    ) -> Result<Offset> {
        let mut coords = Vec::new();
        let mut ends = Vec::new();
        coords.extend_from_slice(&poly.exterior.coords);
        if !poly.interiors.is_empty() {
            let mut acc = poly.exterior.coords.len();
            ends.push(acc as u32);
            for ring in &poly.interiors {
                coords.extend_from_slice(&ring.coords);
                acc += ring.coords.len();
                ends.push(acc as u32);
            }
        }
        self.build_simple(fbb, &coords, &ends, type_opt)
    }

    /// Builds a `Geometry` table from a flat coordinate list plus `ends`.
    fn build_simple(
        &self,
        fbb: &mut FlatBufferBuilder<'_>,
        coords: &[Coordinate],
        ends: &[u32],
        type_opt: Option<GeometryType>,
    ) -> Result<Offset> {
        let mut xy = Vec::with_capacity(coords.len() * 2);
        for c in coords {
            xy.push(c.x);
            xy.push(c.y);
        }
        let xy_off = fbb.create_vector::<f64>(&xy);

        let z_off = if self.has_z {
            let z: Vec<f64> = coords.iter().map(|c| c.z.unwrap_or(0.0)).collect();
            Some(fbb.create_vector::<f64>(&z))
        } else {
            None
        };
        let m_off = if self.has_m {
            let m: Vec<f64> = coords.iter().map(|c| c.m.unwrap_or(0.0)).collect();
            Some(fbb.create_vector::<f64>(&m))
        } else {
            None
        };
        let ends_off = if ends.is_empty() {
            None
        } else {
            Some(fbb.create_vector::<u32>(ends))
        };

        let wip = fbb.start_table();
        if let Some(o) = ends_off {
            fbb.push_slot_always(fbs::GEOM_VT_ENDS, o);
        }
        fbb.push_slot_always(fbs::GEOM_VT_XY, xy_off);
        if let Some(o) = z_off {
            fbb.push_slot_always(fbs::GEOM_VT_Z, o);
        }
        if let Some(o) = m_off {
            fbb.push_slot_always(fbs::GEOM_VT_M, o);
        }
        if let Some(gt) = type_opt {
            fbb.push_slot::<u8>(fbs::GEOM_VT_TYPE, gt as u8, 0);
        }
        Ok(fbb.end_table(wip))
    }
}

/// Returns the `FlatGeobuf` geometry type of an `OxiGDAL` geometry.
const fn geometry_type_of(geometry: &Geometry) -> GeometryType {
    match geometry {
        Geometry::Point(_) => GeometryType::Point,
        Geometry::LineString(_) => GeometryType::LineString,
        Geometry::Polygon(_) => GeometryType::Polygon,
        Geometry::MultiPoint(_) => GeometryType::MultiPoint,
        Geometry::MultiLineString(_) => GeometryType::MultiLineString,
        Geometry::MultiPolygon(_) => GeometryType::MultiPolygon,
        Geometry::GeometryCollection(_) => GeometryType::GeometryCollection,
    }
}

/// Splits a flat coordinate list into parts using cumulative `ends` indices.
///
/// `ends` values are counts of coordinate pairs (not doubles). When `ends` is
/// absent or empty the whole coordinate list is returned as a single part.
fn split_by_ends(coords: &[Coordinate], ends: Option<&[u32]>) -> Result<Vec<Vec<Coordinate>>> {
    match ends {
        Some(ends) if !ends.is_empty() => {
            let mut parts = Vec::with_capacity(ends.len());
            let mut start = 0usize;
            for &e in ends {
                let end = e as usize;
                if end < start || end > coords.len() {
                    return Err(FlatGeobufError::InvalidGeometry(
                        "geometry ends index out of range".to_string(),
                    ));
                }
                parts.push(coords[start..end].to_vec());
                start = end;
            }
            Ok(parts)
        }
        _ => Ok(vec![coords.to_vec()]),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    /// Encodes a geometry into a standalone `Geometry` `FlatBuffers` message and
    /// reads it back through the codec.
    fn roundtrip(codec: &GeometryCodec, geometry: &Geometry, gt: GeometryType) -> Geometry {
        let mut fbb = FlatBufferBuilder::new();
        let off = codec.build(&mut fbb, geometry).expect("build geometry");
        fbb.finish(off, None);
        let data = fbb.finished_data().to_vec();
        let table = FbTable::root(&data).expect("root table");
        codec.read(&table, gt).expect("read geometry")
    }

    #[test]
    fn test_point_roundtrip() {
        let codec = GeometryCodec::new(false, false);
        let g = Geometry::Point(Point::new(10.0, 20.0));
        if let Geometry::Point(p) = roundtrip(&codec, &g, GeometryType::Point) {
            assert_eq!(p.coord.x, 10.0);
            assert_eq!(p.coord.y, 20.0);
        } else {
            panic!("expected point");
        }
    }

    #[test]
    fn test_point_3d_roundtrip() {
        let codec = GeometryCodec::new(true, false);
        let g = Geometry::Point(Point::new_3d(1.0, 2.0, 3.0));
        if let Geometry::Point(p) = roundtrip(&codec, &g, GeometryType::Point) {
            assert_eq!(p.coord.z, Some(3.0));
        } else {
            panic!("expected point");
        }
    }

    #[test]
    fn test_linestring_roundtrip() {
        let codec = GeometryCodec::new(false, false);
        let ls = LineString::new(vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(1.0, 1.0),
            Coordinate::new_2d(2.0, 0.0),
        ])
        .unwrap();
        let g = Geometry::LineString(ls);
        if let Geometry::LineString(l) = roundtrip(&codec, &g, GeometryType::LineString) {
            assert_eq!(l.coords.len(), 3);
            assert_eq!(l.coords[2].x, 2.0);
        } else {
            panic!("expected linestring");
        }
    }

    #[test]
    fn test_polygon_with_hole_roundtrip() {
        let codec = GeometryCodec::new(false, false);
        let exterior = LineString::new(vec![
            Coordinate::new_2d(0.0, 0.0),
            Coordinate::new_2d(10.0, 0.0),
            Coordinate::new_2d(10.0, 10.0),
            Coordinate::new_2d(0.0, 10.0),
            Coordinate::new_2d(0.0, 0.0),
        ])
        .unwrap();
        let hole = LineString::new(vec![
            Coordinate::new_2d(2.0, 2.0),
            Coordinate::new_2d(8.0, 2.0),
            Coordinate::new_2d(8.0, 8.0),
            Coordinate::new_2d(2.0, 8.0),
            Coordinate::new_2d(2.0, 2.0),
        ])
        .unwrap();
        let g = Geometry::Polygon(Polygon::new(exterior, vec![hole]).unwrap());
        if let Geometry::Polygon(p) = roundtrip(&codec, &g, GeometryType::Polygon) {
            assert_eq!(p.exterior.coords.len(), 5);
            assert_eq!(p.interiors.len(), 1);
            assert_eq!(p.interiors[0].coords.len(), 5);
        } else {
            panic!("expected polygon");
        }
    }

    #[test]
    fn test_multipolygon_roundtrip() {
        let codec = GeometryCodec::new(false, false);
        let mk = |x: f64| {
            Polygon::new(
                LineString::new(vec![
                    Coordinate::new_2d(x, 0.0),
                    Coordinate::new_2d(x + 1.0, 0.0),
                    Coordinate::new_2d(x + 1.0, 1.0),
                    Coordinate::new_2d(x, 1.0),
                    Coordinate::new_2d(x, 0.0),
                ])
                .unwrap(),
                vec![],
            )
            .unwrap()
        };
        let g = Geometry::MultiPolygon(MultiPolygon::new(vec![mk(0.0), mk(5.0)]));
        if let Geometry::MultiPolygon(mp) = roundtrip(&codec, &g, GeometryType::MultiPolygon) {
            assert_eq!(mp.polygons.len(), 2);
            assert_eq!(mp.polygons[1].exterior.coords[0].x, 5.0);
        } else {
            panic!("expected multipolygon");
        }
    }

    #[test]
    fn test_geometry_collection_roundtrip() {
        let codec = GeometryCodec::new(false, false);
        let point = Geometry::Point(Point::new(0.0, 0.0));
        let ls = Geometry::LineString(
            LineString::new(vec![
                Coordinate::new_2d(0.0, 0.0),
                Coordinate::new_2d(1.0, 1.0),
            ])
            .unwrap(),
        );
        let g = Geometry::GeometryCollection(GeometryCollection::new(vec![point, ls]));
        if let Geometry::GeometryCollection(gc) =
            roundtrip(&codec, &g, GeometryType::GeometryCollection)
        {
            assert_eq!(gc.geometries.len(), 2);
            assert!(matches!(gc.geometries[0], Geometry::Point(_)));
            assert!(matches!(gc.geometries[1], Geometry::LineString(_)));
        } else {
            panic!("expected geometry collection");
        }
    }
}
