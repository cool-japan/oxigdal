//! Format-conversion implementation for [`Dataset::convert`].
//!
//! Holds the dispatch logic, the GeoTIFF→GeoTIFF writer wiring (with optional
//! Cloud-Optimized GeoTIFF output), and the GeoJSON→Shapefile bridge.  The
//! free GeoJSON↔core helper functions live here so they stay co-located with
//! the only call-site that uses them.

use crate::{ConversionOptions, Dataset, DatasetFormat};

// The umbrella compression enum is only ever mapped onto a driver's own
// compression codes, and the only such mapping lives in the GeoTIFF writer path.
#[cfg(feature = "geotiff")]
use crate::Compression;
use oxigeo_core::error::OxiGeoError;
use oxigeo_core::error::Result;

impl Dataset {
    /// Convert this dataset to a different format at `output_path`.
    ///
    /// Uses the [`crate::convert`] module's planning infrastructure to validate
    /// the conversion pair and then dispatches to the appropriate writer.
    ///
    /// # Format Support
    ///
    /// | From     | To       | Requires feature |
    /// |----------|----------|-----------------|
    /// | GeoTIFF  | GeoTIFF  | `geotiff`        |
    /// | GeoJSON  | GeoJSON  | `geojson`        |
    /// | GeoTIFF  | GeoJSON  | not supported    |
    ///
    /// Mixed raster-to-vector conversions are not supported and return
    /// [`OxiGeoError::NotSupported`].
    ///
    /// # Errors
    ///
    /// - [`OxiGeoError::NotSupported`] — conversion pair is not supported.
    /// - [`OxiGeoError::Io`] — cannot write output file.
    /// - [`OxiGeoError::InvalidParameter`] — invalid output path.
    pub fn convert(
        &self,
        output_path: &std::path::Path,
        target_format: DatasetFormat,
        options: ConversionOptions,
    ) -> Result<Dataset> {
        // Validate the conversion pair via the planning module.
        if !crate::convert::can_convert(self.info().format, target_format) {
            return Err(OxiGeoError::NotSupported {
                operation: format!(
                    "conversion from '{}' to '{}' is not supported",
                    self.info().format.driver_name(),
                    target_format.driver_name(),
                ),
            });
        }

        // Suppress unused-variable warnings when feature flags are all off.
        let _ = &output_path;
        let _ = &options;

        // Dispatch to the appropriate writer.
        #[allow(unreachable_code)]
        {
            match (self.info().format, target_format) {
                #[cfg(feature = "geotiff")]
                (DatasetFormat::GeoTiff, DatasetFormat::GeoTiff) => {
                    self.convert_geotiff_to_geotiff(output_path, &options)?;
                }

                #[cfg(feature = "geojson")]
                (DatasetFormat::GeoJson, DatasetFormat::GeoJson) => {
                    std::fs::copy(self.path(), output_path).map_err(|e| {
                        OxiGeoError::Io(oxigeo_core::error::IoError::Write {
                            message: format!("failed to copy GeoJSON: {e}"),
                        })
                    })?;
                }

                #[cfg(all(feature = "geojson", feature = "shapefile"))]
                (DatasetFormat::GeoJson, DatasetFormat::Shapefile) => {
                    self.convert_geojson_to_shapefile(output_path)?;
                }

                _ => {
                    return Err(OxiGeoError::NotSupported {
                        operation: format!(
                            "conversion from '{}' to '{}' is not yet implemented",
                            self.info().format.driver_name(),
                            target_format.driver_name(),
                        ),
                    });
                }
            }

            let output_str = output_path
                .to_str()
                .ok_or_else(|| OxiGeoError::InvalidParameter {
                    parameter: "output_path",
                    message: "output path contains non-UTF-8 characters".to_string(),
                })?;
            Dataset::open(output_str)
        }
    }

    /// GeoTIFF → GeoTIFF conversion, applying `options` (compression, tiling, COG).
    ///
    /// When `options.cog` is `true`, the output is written using [`CogWriter`] which
    /// enforces COG-compliant IFD ordering (all IFDs before tile data), power-of-2
    /// tiling, and overview embedding.  The default tile size when none is specified is
    /// 256 × 256 pixels.
    #[cfg(feature = "geotiff")]
    fn convert_geotiff_to_geotiff(
        &self,
        output_path: &std::path::Path,
        options: &ConversionOptions,
    ) -> Result<()> {
        use oxigeo_core::io::FileDataSource;
        use oxigeo_geotiff::{
            CogWriter, CogWriterOptions, GeoTiffReader, GeoTiffWriter, GeoTiffWriterOptions,
            WriterConfig,
        };

        let source = FileDataSource::open(self.path()).map_err(|e| {
            OxiGeoError::Io(oxigeo_core::error::IoError::Read {
                message: format!("failed to open source '{}': {e}", self.path()),
            })
        })?;
        let reader = GeoTiffReader::open(source)?;

        let src_width = reader.width();
        let src_height = reader.height();
        let band_count: u16 = u16::try_from(reader.band_count()).unwrap_or(1);
        let data_type = reader
            .data_type()
            .unwrap_or(oxigeo_core::types::RasterDataType::UInt8);
        // Preserve the source's NoData value across the round-trip so downstream
        // consumers (stats, hillshade, colorizers) keep masking former nodata pixels.
        let nodata = reader.nodata();

        use oxigeo_geotiff::tiff::{
            Compression as TiffCompression, PhotometricInterpretation, Predictor,
        };

        // Map umbrella Compression enum to GeoTIFF tiff::Compression.
        // Deflate maps to AdobeDeflate (code 8) — the standard TIFF deflate.
        let tiff_compression = match options.compression {
            Some(Compression::Lzw) => TiffCompression::Lzw,
            Some(Compression::Deflate) => TiffCompression::AdobeDeflate,
            Some(Compression::PackBits) => TiffCompression::Packbits,
            Some(Compression::Zstd) => TiffCompression::Zstd,
            Some(Compression::None) | None => TiffCompression::None,
        };

        // `GeoTiffReader::read_band(level, band)` returns ONE de-interleaved band
        // plane — `width × height × bytes_per_sample` bytes, row-major. The
        // GeoTIFF/COG writers, in contrast, consume a chunky (pixel-interleaved)
        // buffer of `width × height × band_count × bytes_per_sample` bytes, so
        // every band is read and the planes are woven back together below.
        //
        // A single-band raster is already in the writers' layout, so its plane
        // moves straight through without an interleave pass or an extra copy.
        let bands_to_read = usize::from(band_count.max(1));
        let bytes_per_sample = data_type.size_bytes();
        let full_band_data: Vec<u8> = if bands_to_read == 1 {
            reader.read_band(0, 0)?
        } else {
            let pixel_count = src_width
                .checked_mul(src_height)
                .and_then(|px| usize::try_from(px).ok())
                .ok_or_else(|| OxiGeoError::Internal {
                    message: format!(
                        "raster extent {src_width}×{src_height} overflows the address space"
                    ),
                })?;
            let mut planes: Vec<Vec<u8>> = Vec::with_capacity(bands_to_read);
            for band in 0..bands_to_read {
                planes.push(reader.read_band(0, band)?);
            }
            interleave_band_planes(&planes, pixel_count, bytes_per_sample)?
        };

        // Honour any clip window recorded by `Dataset::clip`: crop the pixel
        // buffer to the window and use the clipped dimensions + geo-transform
        // (already adjusted on `self.info()`), so `clip().convert()` writes the
        // cropped raster instead of silently emitting the full original.
        let (all_band_data, width, height) = match self.clip_window() {
            Some(window) => {
                let full_w = u32::try_from(src_width).map_err(|_| OxiGeoError::Internal {
                    message: format!("raster width {src_width} exceeds u32 for clip"),
                })?;
                let full_h = u32::try_from(src_height).map_err(|_| OxiGeoError::Internal {
                    message: format!("raster height {src_height} exceeds u32 for clip"),
                })?;
                let cropped =
                    crate::crop_interleaved(&full_band_data, full_w, full_h, window).ok_or_else(
                        || OxiGeoError::Internal {
                            message: format!(
                                "clip window [{},{} {}×{}] does not fit source raster {full_w}×{full_h}",
                                window.col, window.row, window.width, window.height
                            ),
                        },
                    )?;
                (cropped, u64::from(window.width), u64::from(window.height))
            }
            None => (full_band_data, src_width, src_height),
        };

        if options.cog {
            // COG path: tiling is mandatory; default to 256 × 256 when not specified.
            // The tile size must be a power of two (validated by CogWriter).
            let cog_tile = options.tile_size.unwrap_or(256);
            let overview_levels = if options.overviews.is_empty() {
                vec![2u32, 4, 8, 16]
            } else {
                options.overviews.clone()
            };

            let config = WriterConfig {
                width,
                height,
                band_count,
                data_type,
                compression: tiff_compression,
                predictor: Predictor::None,
                tile_width: Some(cog_tile),
                tile_height: Some(cog_tile),
                photometric: PhotometricInterpretation::BlackIsZero,
                geo_transform: self.info().geotransform,
                epsg_code: self
                    .info()
                    .crs
                    .as_deref()
                    .and_then(crate::extract_epsg_from_crs_string),
                nodata,
                use_bigtiff: false,
                generate_overviews: true,
                overview_resampling: oxigeo_geotiff::OverviewResampling::Average,
                overview_levels,
            };

            let mut cog_writer =
                CogWriter::create(output_path, config, CogWriterOptions::default()).map_err(
                    |e| {
                        OxiGeoError::Io(oxigeo_core::error::IoError::Write {
                            message: format!("failed to create COG output: {e}"),
                        })
                    },
                )?;

            cog_writer.write(&all_band_data).map_err(|e| {
                OxiGeoError::Io(oxigeo_core::error::IoError::Write {
                    message: format!("failed to write COG data: {e}"),
                })
            })?;
        } else {
            // Standard GeoTIFF path.
            let tile_size = options.tile_size;
            let generate_overviews = !options.overviews.is_empty();
            let overview_levels = options.overviews.clone();

            let config = WriterConfig {
                width,
                height,
                band_count,
                data_type,
                compression: tiff_compression,
                predictor: Predictor::None,
                tile_width: tile_size,
                tile_height: tile_size,
                photometric: PhotometricInterpretation::BlackIsZero,
                geo_transform: self.info().geotransform,
                epsg_code: self
                    .info()
                    .crs
                    .as_deref()
                    .and_then(crate::extract_epsg_from_crs_string),
                nodata,
                use_bigtiff: false,
                generate_overviews,
                overview_resampling: oxigeo_geotiff::OverviewResampling::Average,
                overview_levels,
            };

            let mut writer =
                GeoTiffWriter::create(output_path, config, GeoTiffWriterOptions::default())
                    .map_err(|e| {
                        OxiGeoError::Io(oxigeo_core::error::IoError::Write {
                            message: format!("failed to create output TIFF: {e}"),
                        })
                    })?;

            writer.write(&all_band_data).map_err(|e| {
                OxiGeoError::Io(oxigeo_core::error::IoError::Write {
                    message: format!("failed to write TIFF data: {e}"),
                })
            })?;
        }

        Ok(())
    }

    /// GeoJSON → Shapefile conversion: reads the FeatureCollection and writes it.
    #[cfg(all(feature = "geojson", feature = "shapefile"))]
    fn convert_geojson_to_shapefile(&self, output_path: &std::path::Path) -> Result<()> {
        use oxigeo_geojson::GeoJsonReader;
        use oxigeo_shapefile::ShapefileWriter;

        let file = std::fs::File::open(self.path()).map_err(|e| {
            OxiGeoError::Io(oxigeo_core::error::IoError::Read {
                message: format!("cannot open source GeoJSON '{}': {e}", self.path()),
            })
        })?;

        let mut reader = GeoJsonReader::without_validation(std::io::BufReader::new(file));
        let fc = reader.read_feature_collection().map_err(|e| {
            OxiGeoError::Io(oxigeo_core::error::IoError::Read {
                message: format!("cannot parse GeoJSON FeatureCollection: {e}"),
            })
        })?;

        if fc.features.is_empty() {
            return Err(OxiGeoError::NotSupported {
                operation: "GeoJSON→Shapefile: source FeatureCollection has no features".into(),
            });
        }

        // Infer ShapeType and field schema from the GeoJSON features.
        let (shape_type, field_descriptors) = infer_shapefile_schema(&fc.features)?;

        // The base path for the Shapefile is the output path without extension.
        let base = output_path.with_extension("");
        let mut writer =
            ShapefileWriter::new(&base, shape_type, field_descriptors).map_err(|e| {
                OxiGeoError::Io(oxigeo_core::error::IoError::Write {
                    message: format!("cannot create Shapefile '{base:?}': {e}"),
                })
            })?;

        // Convert GeoJSON features to core features and write.
        let core_features: Vec<oxigeo_core::vector::Feature> = fc
            .features
            .iter()
            .map(geojson_feature_to_core)
            .collect::<Result<Vec<_>>>()?;

        writer.write_oxigeo_features(&core_features).map_err(|e| {
            OxiGeoError::Io(oxigeo_core::error::IoError::Write {
                message: format!("failed to write Shapefile features: {e}"),
            })
        })
    }
}

// ─── Band interleaving ──────────────────────────────────────────────────────

/// Weave per-band plane buffers into a single chunky (pixel-interleaved) buffer.
///
/// Input is one buffer per band, each `pixel_count × bytes_per_sample` bytes in
/// row-major order — the layout [`oxigeo_geotiff::GeoTiffReader::read_band`]
/// produces.  Output is the layout the GeoTIFF/COG writers consume: for every
/// pixel, all bands consecutively (`b0 b1 b2 b0 b1 b2 …`), each sample copied
/// whole so its byte order is preserved verbatim.
///
/// Every plane length is validated against `pixel_count × bytes_per_sample`
/// first: a mismatch means the file's real sample size disagrees with the
/// declared [`RasterDataType`](oxigeo_core::types::RasterDataType), and writing
/// that buffer would silently emit shifted pixel values.
///
/// # Errors
///
/// Returns [`OxiGeoError::Internal`] when `planes` is empty, when
/// `bytes_per_sample` is zero, when any plane has the wrong length, or when the
/// interleaved size overflows `usize`.
#[cfg(feature = "geotiff")]
fn interleave_band_planes(
    planes: &[Vec<u8>],
    pixel_count: usize,
    bytes_per_sample: usize,
) -> Result<Vec<u8>> {
    let band_count = planes.len();
    if band_count == 0 {
        return Err(OxiGeoError::Internal {
            message: "cannot interleave zero band planes".to_string(),
        });
    }
    if bytes_per_sample == 0 {
        return Err(OxiGeoError::Internal {
            message: "bytes per sample must be non-zero to interleave band planes".to_string(),
        });
    }

    let plane_len =
        pixel_count
            .checked_mul(bytes_per_sample)
            .ok_or_else(|| OxiGeoError::Internal {
                message: format!(
                    "band plane size {pixel_count} × {bytes_per_sample} overflows the address space"
                ),
            })?;
    for (index, plane) in planes.iter().enumerate() {
        if plane.len() != plane_len {
            return Err(OxiGeoError::Internal {
                message: format!(
                    "band {index} returned {} bytes, expected {plane_len} \
                     ({pixel_count} pixels × {bytes_per_sample} bytes/sample)",
                    plane.len()
                ),
            });
        }
    }

    let total = plane_len
        .checked_mul(band_count)
        .ok_or_else(|| OxiGeoError::Internal {
            message: format!(
                "interleaved buffer {plane_len} × {band_count} overflows the address space"
            ),
        })?;

    let mut out = vec![0u8; total];
    for (band, plane) in planes.iter().enumerate() {
        for pixel in 0..pixel_count {
            let src = pixel * bytes_per_sample;
            let dst = (pixel * band_count + band) * bytes_per_sample;
            out[dst..dst + bytes_per_sample].copy_from_slice(&plane[src..src + bytes_per_sample]);
        }
    }
    Ok(out)
}

// ─── GeoJSON→Shapefile conversion helpers ───────────────────────────────────

/// Convert a `oxigeo_geojson::Feature` to a `oxigeo_core::vector::Feature`.
///
/// Properties with JSON value types that don't map 1-to-1 to
/// `oxigeo_core::vector::FieldValue` are coerced to their string representation.
#[cfg(all(feature = "geojson", feature = "shapefile"))]
fn geojson_feature_to_core(
    feature: &oxigeo_geojson::Feature,
) -> Result<oxigeo_core::vector::Feature> {
    use oxigeo_core::vector::Feature as CoreFeature;

    let geometry = feature
        .geometry
        .as_ref()
        .map(geojson_geom_to_core)
        .transpose()?;

    let mut core_feature = CoreFeature {
        id: None,
        geometry,
        properties: std::collections::HashMap::new(),
    };

    if let Some(props) = &feature.properties {
        for (key, val) in props {
            let fv = json_value_to_field_value(val);
            core_feature.properties.insert(key.clone(), fv);
        }
    }

    Ok(core_feature)
}

/// Convert a `serde_json::Value` to a `oxigeo_core::vector::FieldValue`.
#[cfg(all(feature = "geojson", feature = "shapefile"))]
fn json_value_to_field_value(val: &serde_json::Value) -> oxigeo_core::vector::FieldValue {
    use oxigeo_core::vector::FieldValue;
    match val {
        serde_json::Value::Null => FieldValue::Null,
        serde_json::Value::Bool(b) => FieldValue::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                FieldValue::Integer(i)
            } else if let Some(f) = n.as_f64() {
                FieldValue::Float(f)
            } else {
                FieldValue::String(n.to_string())
            }
        }
        serde_json::Value::String(s) => FieldValue::String(s.clone()),
        other => FieldValue::String(other.to_string()),
    }
}

/// Convert a `oxigeo_geojson::Geometry` to a `oxigeo_core::vector::Geometry`.
#[cfg(all(feature = "geojson", feature = "shapefile"))]
fn geojson_geom_to_core(geom: &oxigeo_geojson::Geometry) -> Result<oxigeo_core::vector::Geometry> {
    use oxigeo_core::vector::{
        Coordinate, Geometry as CoreGeom, GeometryCollection as CoreGC, LineString as CoreLS,
        MultiLineString as CoreMLS, MultiPoint as CoreMP, MultiPolygon as CoreMPoly,
        Point as CorePoint, Polygon as CorePoly,
    };
    use oxigeo_geojson::Geometry as GjGeom;

    let pos_to_coord = |pos: &[f64]| -> Result<Coordinate> {
        if pos.len() < 2 {
            return Err(OxiGeoError::InvalidParameter {
                parameter: "coordinates",
                message: format!("position needs at least 2 elements, got {}", pos.len()),
            });
        }
        Ok(Coordinate {
            x: pos[0],
            y: pos[1],
            z: pos.get(2).copied(),
            m: None,
        })
    };

    let positions_to_coords = |positions: &[Vec<f64>]| -> Result<Vec<Coordinate>> {
        positions.iter().map(|p| pos_to_coord(p)).collect()
    };

    let rings_to_linestrings = |rings: Vec<Vec<Coordinate>>| -> Result<(CoreLS, Vec<CoreLS>)> {
        let mut iter = rings.into_iter();
        let exterior_coords = iter.next().ok_or_else(|| OxiGeoError::InvalidParameter {
            parameter: "polygon",
            message: "polygon has no rings".to_string(),
        })?;
        let exterior = CoreLS::new(exterior_coords).map_err(|e| OxiGeoError::InvalidParameter {
            parameter: "exterior ring",
            message: e.to_string(),
        })?;
        let interiors = iter
            .map(|ring| {
                CoreLS::new(ring).map_err(|e| OxiGeoError::InvalidParameter {
                    parameter: "interior ring",
                    message: e.to_string(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok((exterior, interiors))
    };

    match geom {
        GjGeom::Point(p) => {
            let coord = pos_to_coord(&p.coordinates)?;
            Ok(CoreGeom::Point(CorePoint::from_coord(coord)))
        }
        GjGeom::LineString(ls) => {
            let coords = positions_to_coords(&ls.coordinates)?;
            CoreLS::new(coords).map(CoreGeom::LineString).map_err(|e| {
                OxiGeoError::InvalidParameter {
                    parameter: "linestring",
                    message: e.to_string(),
                }
            })
        }
        GjGeom::Polygon(p) => {
            let rings = p
                .coordinates
                .iter()
                .map(|ring| positions_to_coords(ring))
                .collect::<Result<Vec<_>>>()?;
            let (exterior, interiors) = rings_to_linestrings(rings)?;
            CorePoly::new(exterior, interiors)
                .map(CoreGeom::Polygon)
                .map_err(|e| OxiGeoError::InvalidParameter {
                    parameter: "polygon",
                    message: e.to_string(),
                })
        }
        GjGeom::MultiPoint(mp) => {
            let points = mp
                .coordinates
                .iter()
                .map(|pos| pos_to_coord(pos).map(CorePoint::from_coord))
                .collect::<Result<Vec<_>>>()?;
            Ok(CoreGeom::MultiPoint(CoreMP::new(points)))
        }
        GjGeom::MultiLineString(mls) => {
            let lines = mls
                .coordinates
                .iter()
                .map(|line| {
                    let coords = positions_to_coords(line)?;
                    CoreLS::new(coords).map_err(|e| OxiGeoError::InvalidParameter {
                        parameter: "multilinestring segment",
                        message: e.to_string(),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(CoreGeom::MultiLineString(CoreMLS {
                line_strings: lines,
            }))
        }
        GjGeom::MultiPolygon(mpoly) => {
            let polygons = mpoly
                .coordinates
                .iter()
                .map(|rings| {
                    let coord_rings = rings
                        .iter()
                        .map(|ring| positions_to_coords(ring))
                        .collect::<Result<Vec<_>>>()?;
                    let (exterior, interiors) = rings_to_linestrings(coord_rings)?;
                    CorePoly::new(exterior, interiors).map_err(|e| OxiGeoError::InvalidParameter {
                        parameter: "multipolygon ring",
                        message: e.to_string(),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(CoreGeom::MultiPolygon(CoreMPoly { polygons }))
        }
        GjGeom::GeometryCollection(gc) => {
            let geoms = gc
                .geometries
                .iter()
                .map(geojson_geom_to_core)
                .collect::<Result<Vec<_>>>()?;
            Ok(CoreGeom::GeometryCollection(CoreGC { geometries: geoms }))
        }
    }
}

/// Infer the Shapefile `ShapeType` and `FieldDescriptor`s from GeoJSON features.
#[cfg(all(feature = "geojson", feature = "shapefile"))]
fn infer_shapefile_schema(
    features: &[oxigeo_geojson::Feature],
) -> Result<(
    oxigeo_shapefile::ShapeType,
    Vec<oxigeo_shapefile::dbf::FieldDescriptor>,
)> {
    use oxigeo_geojson::Geometry as GjGeom;
    use oxigeo_shapefile::{
        ShapeType,
        dbf::{FieldDescriptor, FieldType},
    };

    // Map a GeoJSON geometry to the Shapefile ShapeType it belongs to.
    let geom_shape_type = |g: &GjGeom| match g {
        GjGeom::Point(_) | GjGeom::MultiPoint(_) => ShapeType::Point,
        GjGeom::LineString(_) | GjGeom::MultiLineString(_) => ShapeType::PolyLine,
        GjGeom::Polygon(_) | GjGeom::MultiPolygon(_) => ShapeType::Polygon,
        GjGeom::GeometryCollection(_) => ShapeType::Point,
    };

    // A Shapefile stores exactly ONE geometry type. Validate that every feature
    // with a geometry maps to the same ShapeType up front; a mixed-geometry
    // FeatureCollection (which GeoJSON permits but Shapefile does not) must be
    // rejected rather than silently written into a structurally inconsistent
    // .shp file under the first feature's type.
    let mut shape_type: Option<ShapeType> = None;
    for feature in features {
        if let Some(g) = feature.geometry.as_ref() {
            let st = geom_shape_type(g);
            match shape_type {
                None => shape_type = Some(st),
                Some(existing) if existing != st => {
                    return Err(OxiGeoError::NotSupported {
                        operation: format!(
                            "GeoJSON→Shapefile: mixed geometry types in one FeatureCollection \
                             ({existing:?} and {st:?}); a Shapefile holds a single geometry type"
                        ),
                    });
                }
                Some(_) => {}
            }
        }
    }
    let shape_type = shape_type.unwrap_or(ShapeType::Point);

    // Scan properties for field names and widths; max field name is 10 chars.
    let mut widths: std::collections::HashMap<String, u8> = std::collections::HashMap::new();
    for feature in features {
        if let Some(props) = &feature.properties {
            for (key, val) in props {
                // DBF field names are capped at 10 bytes. `key.len()` is a byte
                // length, so slicing `&key[..10]` would panic when byte 10 lands
                // mid-character (Japanese, emoji, accented Latin, …). Back off to
                // the nearest UTF-8 char boundary at or below 10 bytes.
                let short_key = if key.len() > 10 {
                    let mut end = 10;
                    while end > 0 && !key.is_char_boundary(end) {
                        end -= 1;
                    }
                    &key[..end]
                } else {
                    key.as_str()
                };
                let width = match val {
                    serde_json::Value::String(s) => u8::try_from(s.len().min(254)).unwrap_or(254),
                    other => u8::try_from(other.to_string().len().min(254)).unwrap_or(254),
                };
                let entry = widths.entry(short_key.to_string()).or_insert(1);
                if width > *entry {
                    *entry = width;
                }
            }
        }
    }

    let mut descriptors: Vec<FieldDescriptor> = widths
        .into_iter()
        .map(|(name, width)| {
            FieldDescriptor::new(name.clone(), FieldType::Character, width.max(1), 0).map_err(|e| {
                OxiGeoError::InvalidParameter {
                    parameter: "field descriptor",
                    message: e.to_string(),
                }
            })
        })
        .collect::<Result<Vec<_>>>()?;

    descriptors.sort_by(|a, b| a.name.cmp(&b.name));

    Ok((shape_type, descriptors))
}

#[cfg(all(test, feature = "geotiff"))]
#[allow(clippy::expect_used)]
mod interleave_tests {
    use super::interleave_band_planes;

    /// Three 2-byte-per-sample planes must weave into `b0 b1 b2 b0 b1 b2 …`
    /// with every sample's byte order preserved verbatim.
    #[test]
    fn test_interleave_three_uint16_planes() {
        // 2 pixels per band, values chosen so both bytes of each sample differ
        // between bands — a byte-order or stride slip cannot cancel out.
        let b0: Vec<u8> = vec![0xE8, 0x03, 0xE9, 0x03]; // 1000, 1001 (LE)
        let b1: Vec<u8> = vec![0xD0, 0x07, 0xD1, 0x07]; // 2000, 2001 (LE)
        let b2: Vec<u8> = vec![0xB8, 0x0B, 0xB9, 0x0B]; // 3000, 3001 (LE)
        let out = interleave_band_planes(&[b0, b1, b2], 2, 2).expect("interleave");
        assert_eq!(
            out,
            vec![
                0xE8, 0x03, 0xD0, 0x07, 0xB8, 0x0B, // pixel 0: 1000, 2000, 3000
                0xE9, 0x03, 0xD1, 0x07, 0xB9, 0x0B, // pixel 1: 1001, 2001, 3001
            ]
        );
    }

    /// Single-byte samples: the interleave degenerates to a plain transpose.
    #[test]
    fn test_interleave_two_uint8_planes() {
        let b0: Vec<u8> = vec![1, 2, 3];
        let b1: Vec<u8> = vec![10, 20, 30];
        let out = interleave_band_planes(&[b0, b1], 3, 1).expect("interleave");
        assert_eq!(out, vec![1, 10, 2, 20, 3, 30]);
    }

    /// Eight-byte samples (Float64 / Int64) keep whole-sample granularity.
    #[test]
    fn test_interleave_eight_byte_samples() {
        let b0: Vec<u8> = (0u8..8).collect();
        let b1: Vec<u8> = (100u8..108).collect();
        let out = interleave_band_planes(&[b0.clone(), b1.clone()], 1, 8).expect("interleave");
        assert_eq!(out[..8], b0[..]);
        assert_eq!(out[8..], b1[..]);
    }

    /// A single plane round-trips unchanged (the caller short-circuits this
    /// case, but the helper must still be correct for it).
    #[test]
    fn test_interleave_single_plane_is_identity() {
        let b0: Vec<u8> = vec![9, 8, 7, 6];
        let out = interleave_band_planes(std::slice::from_ref(&b0), 2, 2).expect("interleave");
        assert_eq!(out, b0);
    }

    /// A plane whose length disagrees with `pixel_count × bytes_per_sample`
    /// means the declared data type does not describe the file — that must be
    /// an error, never a silently shifted buffer.
    #[test]
    fn test_interleave_rejects_short_plane() {
        let b0: Vec<u8> = vec![1, 2, 3, 4];
        let b1: Vec<u8> = vec![5, 6];
        let err = interleave_band_planes(&[b0, b1], 2, 2);
        assert!(err.is_err(), "mismatched plane length must be rejected");
    }

    #[test]
    fn test_interleave_rejects_degenerate_inputs() {
        assert!(interleave_band_planes(&[], 2, 2).is_err(), "no planes");
        assert!(
            interleave_band_planes(&[vec![1, 2]], 2, 0).is_err(),
            "zero bytes per sample"
        );
    }
}

#[cfg(all(test, feature = "geojson", feature = "shapefile"))]
#[allow(clippy::expect_used)]
mod tests {
    use crate::{ConversionOptions, Dataset, DatasetFormat};
    use std::io::Write;

    fn write_temp(name: &str, content: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(name);
        let mut f = std::fs::File::create(&path).expect("create");
        f.write_all(content).expect("write");
        path
    }

    #[test]
    fn test_mixed_geometry_geojson_to_shapefile_errors() {
        // A FeatureCollection mixing Point and Polygon — legal GeoJSON, illegal
        // Shapefile. Conversion must error instead of silently writing a
        // structurally inconsistent .shp.
        let content = br#"{"type":"FeatureCollection","features":[
            {"type":"Feature","geometry":{"type":"Point","coordinates":[0,0]},"properties":{}},
            {"type":"Feature","geometry":{"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[0,1],[0,0]]]},"properties":{}}
        ]}"#;
        let path = write_temp("convert_mixed_geom.geojson", content);
        let ds = Dataset::open(path.to_str().expect("path")).expect("open");
        let out = std::env::temp_dir().join("convert_mixed_geom_out.shp");
        let result = ds.convert(&out, DatasetFormat::Shapefile, ConversionOptions::default());
        assert!(
            result.is_err(),
            "mixed-geometry FeatureCollection must not convert to a Shapefile"
        );
    }

    #[test]
    fn test_uniform_geometry_geojson_to_shapefile_ok() {
        let content = br#"{"type":"FeatureCollection","features":[
            {"type":"Feature","geometry":{"type":"Point","coordinates":[0,0]},"properties":{"n":"a"}},
            {"type":"Feature","geometry":{"type":"Point","coordinates":[1,1]},"properties":{"n":"b"}}
        ]}"#;
        let path = write_temp("convert_uniform_geom.geojson", content);
        let ds = Dataset::open(path.to_str().expect("path")).expect("open");
        let out = std::env::temp_dir().join("convert_uniform_geom_out.shp");
        let result = ds.convert(&out, DatasetFormat::Shapefile, ConversionOptions::default());
        assert!(
            result.is_ok(),
            "uniform Point features should convert: {result:?}"
        );
    }
}
