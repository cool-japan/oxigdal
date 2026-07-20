//! CRS reprojection for GeoPackage features using `oxigeo-proj`.
//!
//! Provides [`CrsReprojector`], a thin wrapper around an
//! [`oxigeo_proj::Transformer`] that knows how to walk every variant of
//! [`GpkgGeometry`] (including all Z, M, and ZM variants and nested
//! `GeometryCollection`s), substituting the source `(x, y)` pair with its
//! reprojected counterpart while preserving the optional Z and M ordinates.
//!
//! Each reprojection allocates fresh `Vec`s mirroring the structure of the
//! input — this is the safest behaviour for a library whose geometries are
//! exposed by value to downstream callers (no in-place mutation surprises).
//!
//! # Feature flag
//!
//! Compiled only when the `reproject` Cargo feature is enabled, which in turn
//! pulls in `oxigeo-proj` as a hard dependency.  The default build of
//! `oxigeo-gpkg` does not depend on `oxigeo-proj` at all.

use oxigeo_proj::{Coordinate, Transformer};

use crate::error::GpkgError;
use crate::vector::feature::{FeatureRow, FeatureTable};
use crate::vector::types::{GpkgGeometry, Point4D};

// ─────────────────────────────────────────────────────────────────────────────
// CrsReprojector
// ─────────────────────────────────────────────────────────────────────────────

/// Reprojects `GpkgGeometry`, `FeatureRow`, and `FeatureTable` values from a
/// source EPSG CRS to a destination EPSG CRS.
///
/// The reprojector holds an internal [`oxigeo_proj::Transformer`] constructed
/// via [`Transformer::from_epsg`].  Z and M ordinates are intentionally
/// preserved verbatim — only the planar `(x, y)` pair is transformed — since
/// the underlying transformer operates on 2D coordinates only.  This matches
/// the typical GeoPackage usage where elevation/measure are independent of
/// the horizontal CRS.
///
/// # Example
///
/// ```no_run
/// use oxigeo_gpkg::reproject::CrsReprojector;
/// use oxigeo_gpkg::vector::types::GpkgGeometry;
///
/// let reproj = CrsReprojector::new(4326, 3857).expect("ok");
/// let pt = GpkgGeometry::Point { x: 0.0, y: 0.0 };
/// let out = reproj.reproject_geometry(&pt).expect("ok");
/// ```
pub struct CrsReprojector {
    transformer: Transformer,
    src_epsg: u32,
    dst_epsg: u32,
}

impl core::fmt::Debug for CrsReprojector {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CrsReprojector")
            .field("src_epsg", &self.src_epsg)
            .field("dst_epsg", &self.dst_epsg)
            .finish_non_exhaustive()
    }
}

impl CrsReprojector {
    /// Construct a reprojector that maps coordinates from `src_epsg` to
    /// `dst_epsg`.
    ///
    /// # Errors
    ///
    /// Returns [`GpkgError::ReprojectionError`] when either EPSG code is
    /// unknown to the embedded oxigeo-proj database, or when the underlying
    /// transformer cannot be initialised (e.g. WKT-to-PROJ conversion is not
    /// implemented for the pair).
    pub fn new(src_epsg: u32, dst_epsg: u32) -> Result<Self, GpkgError> {
        let transformer = Transformer::from_epsg(src_epsg, dst_epsg)
            .map_err(|e| GpkgError::ReprojectionError(format!("{e}")))?;
        Ok(Self {
            transformer,
            src_epsg,
            dst_epsg,
        })
    }

    /// Source EPSG code recorded at construction time.
    pub fn src_epsg(&self) -> u32 {
        self.src_epsg
    }

    /// Destination EPSG code recorded at construction time.
    pub fn dst_epsg(&self) -> u32 {
        self.dst_epsg
    }

    /// Borrowed access to the wrapped transformer, for callers that need to
    /// invoke advanced operations (batch transforms, bbox transforms, …).
    pub fn transformer(&self) -> &Transformer {
        &self.transformer
    }

    /// Reproject a single 2D coordinate via the wrapped transformer.
    fn project_xy(&self, x: f64, y: f64) -> Result<(f64, f64), GpkgError> {
        let coord = Coordinate::new(x, y);
        let out = self
            .transformer
            .transform(&coord)
            .map_err(|e| GpkgError::ReprojectionError(format!("{e}")))?;
        Ok((out.x, out.y))
    }

    /// Reproject a contiguous (x, y) sequence into a new vector.
    fn project_xy_seq(&self, seq: &[(f64, f64)]) -> Result<Vec<(f64, f64)>, GpkgError> {
        let mut out = Vec::with_capacity(seq.len());
        for (x, y) in seq {
            out.push(self.project_xy(*x, *y)?);
        }
        Ok(out)
    }

    /// Reproject a contiguous (x, y, w) sequence (where the third ordinate is
    /// either Z or M and must pass through unchanged) into a new vector.
    fn project_xyw_seq(&self, seq: &[(f64, f64, f64)]) -> Result<Vec<(f64, f64, f64)>, GpkgError> {
        let mut out = Vec::with_capacity(seq.len());
        for (x, y, w) in seq {
            let (nx, ny) = self.project_xy(*x, *y)?;
            out.push((nx, ny, *w));
        }
        Ok(out)
    }

    /// Reproject a contiguous [`Point4D`] sequence, preserving Z and M.
    fn project_point4d_seq(&self, seq: &[Point4D]) -> Result<Vec<Point4D>, GpkgError> {
        let mut out = Vec::with_capacity(seq.len());
        for p in seq {
            let (nx, ny) = self.project_xy(p.x, p.y)?;
            out.push(Point4D {
                x: nx,
                y: ny,
                z: p.z,
                m: p.m,
            });
        }
        Ok(out)
    }

    /// Reproject every coordinate of a [`GpkgGeometry`], yielding a fresh
    /// geometry value with the same variant and identical structure but
    /// transformed `(x, y)` coordinates.
    ///
    /// Z and M ordinates are preserved exactly.  `GeometryCollection*`
    /// variants are recursively reprojected member-by-member.
    ///
    /// # Errors
    ///
    /// Returns [`GpkgError::ReprojectionError`] if any single point fails to
    /// project (e.g. lies outside the source CRS area-of-use under strict
    /// mode).  The first failure aborts the operation; partially-projected
    /// geometries are never returned.
    pub fn reproject_geometry(&self, g: &GpkgGeometry) -> Result<GpkgGeometry, GpkgError> {
        use GpkgGeometry as G;
        match g {
            // ── 2D variants ───────────────────────────────────────────────
            G::Point { x, y } => {
                let (nx, ny) = self.project_xy(*x, *y)?;
                Ok(G::Point { x: nx, y: ny })
            }
            G::LineString { coords } => Ok(G::LineString {
                coords: self.project_xy_seq(coords)?,
            }),
            G::Polygon { rings } => {
                let mut new_rings = Vec::with_capacity(rings.len());
                for r in rings {
                    new_rings.push(self.project_xy_seq(r)?);
                }
                Ok(G::Polygon { rings: new_rings })
            }
            G::MultiPoint { points } => Ok(G::MultiPoint {
                points: self.project_xy_seq(points)?,
            }),
            G::MultiLineString { lines } => {
                let mut new_lines = Vec::with_capacity(lines.len());
                for l in lines {
                    new_lines.push(self.project_xy_seq(l)?);
                }
                Ok(G::MultiLineString { lines: new_lines })
            }
            G::MultiPolygon { polygons } => {
                let mut new_polys = Vec::with_capacity(polygons.len());
                for poly in polygons {
                    let mut new_rings = Vec::with_capacity(poly.len());
                    for r in poly {
                        new_rings.push(self.project_xy_seq(r)?);
                    }
                    new_polys.push(new_rings);
                }
                Ok(G::MultiPolygon {
                    polygons: new_polys,
                })
            }
            G::GeometryCollection(geoms) => {
                let mut new_geoms = Vec::with_capacity(geoms.len());
                for inner in geoms {
                    new_geoms.push(self.reproject_geometry(inner)?);
                }
                Ok(G::GeometryCollection(new_geoms))
            }

            // ── Z variants ────────────────────────────────────────────────
            G::PointZ { x, y, z } => {
                let (nx, ny) = self.project_xy(*x, *y)?;
                Ok(G::PointZ {
                    x: nx,
                    y: ny,
                    z: *z,
                })
            }
            G::LineStringZ { coords } => Ok(G::LineStringZ {
                coords: self.project_xyw_seq(coords)?,
            }),
            G::PolygonZ { rings } => {
                let mut new_rings = Vec::with_capacity(rings.len());
                for r in rings {
                    new_rings.push(self.project_xyw_seq(r)?);
                }
                Ok(G::PolygonZ { rings: new_rings })
            }
            G::MultiPointZ { points } => Ok(G::MultiPointZ {
                points: self.project_xyw_seq(points)?,
            }),
            G::MultiLineStringZ { lines } => {
                let mut new_lines = Vec::with_capacity(lines.len());
                for l in lines {
                    new_lines.push(self.project_xyw_seq(l)?);
                }
                Ok(G::MultiLineStringZ { lines: new_lines })
            }
            G::MultiPolygonZ { polygons } => {
                let mut new_polys = Vec::with_capacity(polygons.len());
                for poly in polygons {
                    let mut new_rings = Vec::with_capacity(poly.len());
                    for r in poly {
                        new_rings.push(self.project_xyw_seq(r)?);
                    }
                    new_polys.push(new_rings);
                }
                Ok(G::MultiPolygonZ {
                    polygons: new_polys,
                })
            }
            G::GeometryCollectionZ(geoms) => {
                let mut new_geoms = Vec::with_capacity(geoms.len());
                for inner in geoms {
                    new_geoms.push(self.reproject_geometry(inner)?);
                }
                Ok(G::GeometryCollectionZ(new_geoms))
            }

            // ── M variants ────────────────────────────────────────────────
            G::PointM { x, y, m } => {
                let (nx, ny) = self.project_xy(*x, *y)?;
                Ok(G::PointM {
                    x: nx,
                    y: ny,
                    m: *m,
                })
            }
            G::LineStringM { coords } => Ok(G::LineStringM {
                coords: self.project_xyw_seq(coords)?,
            }),
            G::PolygonM { rings } => {
                let mut new_rings = Vec::with_capacity(rings.len());
                for r in rings {
                    new_rings.push(self.project_xyw_seq(r)?);
                }
                Ok(G::PolygonM { rings: new_rings })
            }
            G::MultiPointM { points } => Ok(G::MultiPointM {
                points: self.project_xyw_seq(points)?,
            }),
            G::MultiLineStringM { lines } => {
                let mut new_lines = Vec::with_capacity(lines.len());
                for l in lines {
                    new_lines.push(self.project_xyw_seq(l)?);
                }
                Ok(G::MultiLineStringM { lines: new_lines })
            }
            G::MultiPolygonM { polygons } => {
                let mut new_polys = Vec::with_capacity(polygons.len());
                for poly in polygons {
                    let mut new_rings = Vec::with_capacity(poly.len());
                    for r in poly {
                        new_rings.push(self.project_xyw_seq(r)?);
                    }
                    new_polys.push(new_rings);
                }
                Ok(G::MultiPolygonM {
                    polygons: new_polys,
                })
            }
            G::GeometryCollectionM(geoms) => {
                let mut new_geoms = Vec::with_capacity(geoms.len());
                for inner in geoms {
                    new_geoms.push(self.reproject_geometry(inner)?);
                }
                Ok(G::GeometryCollectionM(new_geoms))
            }

            // ── ZM variants (Point4D carriers) ────────────────────────────
            G::PointZM(p) => {
                let (nx, ny) = self.project_xy(p.x, p.y)?;
                Ok(G::PointZM(Point4D {
                    x: nx,
                    y: ny,
                    z: p.z,
                    m: p.m,
                }))
            }
            G::LineStringZM { coords } => Ok(G::LineStringZM {
                coords: self.project_point4d_seq(coords)?,
            }),
            G::PolygonZM { rings } => {
                let mut new_rings = Vec::with_capacity(rings.len());
                for r in rings {
                    new_rings.push(self.project_point4d_seq(r)?);
                }
                Ok(G::PolygonZM { rings: new_rings })
            }
            G::MultiPointZM { points } => Ok(G::MultiPointZM {
                points: self.project_point4d_seq(points)?,
            }),
            G::MultiLineStringZM { lines } => {
                let mut new_lines = Vec::with_capacity(lines.len());
                for l in lines {
                    new_lines.push(self.project_point4d_seq(l)?);
                }
                Ok(G::MultiLineStringZM { lines: new_lines })
            }
            G::MultiPolygonZM { polygons } => {
                let mut new_polys = Vec::with_capacity(polygons.len());
                for poly in polygons {
                    let mut new_rings = Vec::with_capacity(poly.len());
                    for r in poly {
                        new_rings.push(self.project_point4d_seq(r)?);
                    }
                    new_polys.push(new_rings);
                }
                Ok(G::MultiPolygonZM {
                    polygons: new_polys,
                })
            }
            G::GeometryCollectionZM(geoms) => {
                let mut new_geoms = Vec::with_capacity(geoms.len());
                for inner in geoms {
                    new_geoms.push(self.reproject_geometry(inner)?);
                }
                Ok(G::GeometryCollectionZM(new_geoms))
            }

            // ── Sentinel ──────────────────────────────────────────────────
            G::Empty => Ok(G::Empty),
        }
    }

    /// Reproject a single [`FeatureRow`]: returns a new row with the same FID
    /// and field map but with the geometry (if any) reprojected.
    ///
    /// # Errors
    ///
    /// Propagates [`GpkgError::ReprojectionError`] from
    /// [`reproject_geometry`](Self::reproject_geometry).
    pub fn reproject_feature_row(&self, row: &FeatureRow) -> Result<FeatureRow, GpkgError> {
        let new_geometry = match &row.geometry {
            Some(g) => Some(self.reproject_geometry(g)?),
            None => None,
        };
        Ok(FeatureRow {
            fid: row.fid,
            geometry: new_geometry,
            fields: row.fields.clone(),
        })
    }

    /// Reproject every feature in `table` and return a new [`FeatureTable`]
    /// whose `srs_id` reflects the destination EPSG code.
    ///
    /// Schema, geometry-column name, and table name are preserved unchanged.
    ///
    /// # Errors
    ///
    /// Propagates the first per-feature [`GpkgError::ReprojectionError`]
    /// encountered; partially-reprojected tables are never returned.
    pub fn reproject_feature_table(&self, table: &FeatureTable) -> Result<FeatureTable, GpkgError> {
        let mut new_features = Vec::with_capacity(table.features.len());
        for row in &table.features {
            new_features.push(self.reproject_feature_row(row)?);
        }
        let mut new_table = table.clone();
        new_table.features = new_features;
        new_table.srs_id = Some(self.dst_epsg as i32);
        Ok(new_table)
    }
}
