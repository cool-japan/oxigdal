//! Format-conversion implementation for [`Dataset::convert`].
//!
//! Holds the dispatch logic, the GeoTIFF→GeoTIFF writer wiring (with optional
//! Cloud-Optimized GeoTIFF output), and the GeoJSON→Shapefile bridge.  The
//! free GeoJSON↔core helper functions live here so they stay co-located with
//! the only call-site that uses them.

use crate::{Compression, ConversionOptions, Dataset, DatasetFormat};
use oxigdal_core::error::OxiGdalError;
use oxigdal_core::error::Result;

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
    /// [`OxiGdalError::NotSupported`].
    ///
    /// # Errors
    ///
    /// - [`OxiGdalError::NotSupported`] — conversion pair is not supported.
    /// - [`OxiGdalError::Io`] — cannot write output file.
    /// - [`OxiGdalError::InvalidParameter`] — invalid output path.
    pub fn convert(
        &self,
        output_path: &std::path::Path,
        target_format: DatasetFormat,
        options: ConversionOptions,
    ) -> Result<Dataset> {
        // Validate the conversion pair via the planning module.
        if !crate::convert::can_convert(self.info().format, target_format) {
            return Err(OxiGdalError::NotSupported {
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
                        OxiGdalError::Io(oxigdal_core::error::IoError::Write {
                            message: format!("failed to copy GeoJSON: {e}"),
                        })
                    })?;
                }

                #[cfg(all(feature = "geojson", feature = "shapefile"))]
                (DatasetFormat::GeoJson, DatasetFormat::Shapefile) => {
                    self.convert_geojson_to_shapefile(output_path)?;
                }

                _ => {
                    return Err(OxiGdalError::NotSupported {
                        operation: format!(
                            "conversion from '{}' to '{}' is not yet implemented",
                            self.info().format.driver_name(),
                            target_format.driver_name(),
                        ),
                    });
                }
            }

            let output_str =
                output_path
                    .to_str()
                    .ok_or_else(|| OxiGdalError::InvalidParameter {
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
        use oxigdal_core::io::FileDataSource;
        use oxigdal_geotiff::{
            CogWriter, CogWriterOptions, GeoTiffReader, GeoTiffWriter, GeoTiffWriterOptions,
            WriterConfig,
        };

        let source = FileDataSource::open(self.path()).map_err(|e| {
            OxiGdalError::Io(oxigdal_core::error::IoError::Read {
                message: format!("failed to open source '{}': {e}", self.path()),
            })
        })?;
        let reader = GeoTiffReader::open(source)?;

        let width = reader.width();
        let height = reader.height();
        let band_count: u16 = u16::try_from(reader.band_count()).unwrap_or(1);
        let data_type = reader
            .data_type()
            .unwrap_or(oxigdal_core::types::RasterDataType::UInt8);

        use oxigdal_core::types::NoDataValue;
        use oxigdal_geotiff::tiff::{
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

        // Collect all band data into a single contiguous buffer (band-sequential).
        let mut all_band_data: Vec<u8> = Vec::new();
        for band_idx in 0..band_count {
            let raw = reader.read_band(0, band_idx as usize)?;
            all_band_data.extend_from_slice(&raw);
        }

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
                nodata: NoDataValue::None,
                use_bigtiff: false,
                generate_overviews: true,
                overview_resampling: oxigdal_geotiff::OverviewResampling::Average,
                overview_levels,
            };

            let mut cog_writer =
                CogWriter::create(output_path, config, CogWriterOptions::default()).map_err(
                    |e| {
                        OxiGdalError::Io(oxigdal_core::error::IoError::Write {
                            message: format!("failed to create COG output: {e}"),
                        })
                    },
                )?;

            cog_writer.write(&all_band_data).map_err(|e| {
                OxiGdalError::Io(oxigdal_core::error::IoError::Write {
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
                nodata: NoDataValue::None,
                use_bigtiff: false,
                generate_overviews,
                overview_resampling: oxigdal_geotiff::OverviewResampling::Average,
                overview_levels,
            };

            let mut writer =
                GeoTiffWriter::create(output_path, config, GeoTiffWriterOptions::default())
                    .map_err(|e| {
                        OxiGdalError::Io(oxigdal_core::error::IoError::Write {
                            message: format!("failed to create output TIFF: {e}"),
                        })
                    })?;

            writer.write(&all_band_data).map_err(|e| {
                OxiGdalError::Io(oxigdal_core::error::IoError::Write {
                    message: format!("failed to write TIFF data: {e}"),
                })
            })?;
        }

        Ok(())
    }

    /// GeoJSON → Shapefile conversion: reads the FeatureCollection and writes it.
    #[cfg(all(feature = "geojson", feature = "shapefile"))]
    fn convert_geojson_to_shapefile(&self, output_path: &std::path::Path) -> Result<()> {
        use oxigdal_geojson::GeoJsonReader;
        use oxigdal_shapefile::ShapefileWriter;

        let file = std::fs::File::open(self.path()).map_err(|e| {
            OxiGdalError::Io(oxigdal_core::error::IoError::Read {
                message: format!("cannot open source GeoJSON '{}': {e}", self.path()),
            })
        })?;

        let mut reader = GeoJsonReader::without_validation(std::io::BufReader::new(file));
        let fc = reader.read_feature_collection().map_err(|e| {
            OxiGdalError::Io(oxigdal_core::error::IoError::Read {
                message: format!("cannot parse GeoJSON FeatureCollection: {e}"),
            })
        })?;

        if fc.features.is_empty() {
            return Err(OxiGdalError::NotSupported {
                operation: "GeoJSON→Shapefile: source FeatureCollection has no features".into(),
            });
        }

        // Infer ShapeType and field schema from the GeoJSON features.
        let (shape_type, field_descriptors) = infer_shapefile_schema(&fc.features)?;

        // The base path for the Shapefile is the output path without extension.
        let base = output_path.with_extension("");
        let mut writer =
            ShapefileWriter::new(&base, shape_type, field_descriptors).map_err(|e| {
                OxiGdalError::Io(oxigdal_core::error::IoError::Write {
                    message: format!("cannot create Shapefile '{base:?}': {e}"),
                })
            })?;

        // Convert GeoJSON features to core features and write.
        let core_features: Vec<oxigdal_core::vector::Feature> = fc
            .features
            .iter()
            .map(geojson_feature_to_core)
            .collect::<Result<Vec<_>>>()?;

        writer.write_oxigdal_features(&core_features).map_err(|e| {
            OxiGdalError::Io(oxigdal_core::error::IoError::Write {
                message: format!("failed to write Shapefile features: {e}"),
            })
        })
    }
}

// ─── GeoJSON→Shapefile conversion helpers ───────────────────────────────────

/// Convert a `oxigdal_geojson::Feature` to a `oxigdal_core::vector::Feature`.
///
/// Properties with JSON value types that don't map 1-to-1 to
/// `oxigdal_core::vector::FieldValue` are coerced to their string representation.
#[cfg(all(feature = "geojson", feature = "shapefile"))]
fn geojson_feature_to_core(
    feature: &oxigdal_geojson::Feature,
) -> Result<oxigdal_core::vector::Feature> {
    use oxigdal_core::vector::Feature as CoreFeature;

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

/// Convert a `serde_json::Value` to a `oxigdal_core::vector::FieldValue`.
#[cfg(all(feature = "geojson", feature = "shapefile"))]
fn json_value_to_field_value(val: &serde_json::Value) -> oxigdal_core::vector::FieldValue {
    use oxigdal_core::vector::FieldValue;
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

/// Convert a `oxigdal_geojson::Geometry` to a `oxigdal_core::vector::Geometry`.
#[cfg(all(feature = "geojson", feature = "shapefile"))]
fn geojson_geom_to_core(
    geom: &oxigdal_geojson::Geometry,
) -> Result<oxigdal_core::vector::Geometry> {
    use oxigdal_core::vector::{
        Coordinate, Geometry as CoreGeom, GeometryCollection as CoreGC, LineString as CoreLS,
        MultiLineString as CoreMLS, MultiPoint as CoreMP, MultiPolygon as CoreMPoly,
        Point as CorePoint, Polygon as CorePoly,
    };
    use oxigdal_geojson::Geometry as GjGeom;

    let pos_to_coord = |pos: &[f64]| -> Result<Coordinate> {
        if pos.len() < 2 {
            return Err(OxiGdalError::InvalidParameter {
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
        let exterior_coords = iter.next().ok_or_else(|| OxiGdalError::InvalidParameter {
            parameter: "polygon",
            message: "polygon has no rings".to_string(),
        })?;
        let exterior =
            CoreLS::new(exterior_coords).map_err(|e| OxiGdalError::InvalidParameter {
                parameter: "exterior ring",
                message: e.to_string(),
            })?;
        let interiors = iter
            .map(|ring| {
                CoreLS::new(ring).map_err(|e| OxiGdalError::InvalidParameter {
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
                OxiGdalError::InvalidParameter {
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
                .map_err(|e| OxiGdalError::InvalidParameter {
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
                    CoreLS::new(coords).map_err(|e| OxiGdalError::InvalidParameter {
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
                    CorePoly::new(exterior, interiors).map_err(|e| OxiGdalError::InvalidParameter {
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
    features: &[oxigdal_geojson::Feature],
) -> Result<(
    oxigdal_shapefile::ShapeType,
    Vec<oxigdal_shapefile::dbf::FieldDescriptor>,
)> {
    use oxigdal_geojson::Geometry as GjGeom;
    use oxigdal_shapefile::{
        ShapeType,
        dbf::{FieldDescriptor, FieldType},
    };

    let shape_type = features
        .iter()
        .find_map(|f| f.geometry.as_ref())
        .map(|g| match g {
            GjGeom::Point(_) | GjGeom::MultiPoint(_) => ShapeType::Point,
            GjGeom::LineString(_) | GjGeom::MultiLineString(_) => ShapeType::PolyLine,
            GjGeom::Polygon(_) | GjGeom::MultiPolygon(_) => ShapeType::Polygon,
            GjGeom::GeometryCollection(_) => ShapeType::Point,
        })
        .unwrap_or(ShapeType::Point);

    // Scan properties for field names and widths; max field name is 10 chars.
    let mut widths: std::collections::HashMap<String, u8> = std::collections::HashMap::new();
    for feature in features {
        if let Some(props) = &feature.properties {
            for (key, val) in props {
                let short_key = if key.len() > 10 { &key[..10] } else { key };
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
                OxiGdalError::InvalidParameter {
                    parameter: "field descriptor",
                    message: e.to_string(),
                }
            })
        })
        .collect::<Result<Vec<_>>>()?;

    descriptors.sort_by(|a, b| a.name.cmp(&b.name));

    Ok((shape_type, descriptors))
}
