//! Clip command - Clip a raster or vector dataset to a bounding box
//!
//! For raster files: reads the GeoTransform, computes the pixel window that
//! intersects the supplied bbox, extracts that region from every band, and
//! writes a new GeoTIFF with an updated GeoTransform.
//!
//! For vector files: reads all features and keeps only those whose geometry
//! bounding envelope overlaps the requested bbox, then writes the result to
//! the output path.
//!
//! Examples:
//! ```bash
//! # Clip a raster to a bounding box
//! oxigeo clip input.tif output.tif --bbox 10.0,45.0,20.0,55.0
//!
//! # Clip a GeoJSON file
//! oxigeo clip input.geojson output.geojson --bbox 10.0,45.0,20.0,55.0
//!
//! # Overwrite existing output
//! oxigeo clip input.tif output.tif --bbox 0,0,100,100 --overwrite
//! ```

use crate::OutputFormat;
use crate::util::{progress, raster, vector};
use anyhow::{Context, Result};
use clap::Args;
use console::style;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Clip a raster or vector dataset to a bounding box
#[derive(Args, Debug)]
pub struct ClipArgs {
    /// Input file (GeoTIFF, GeoJSON, or Shapefile)
    #[arg(value_name = "INPUT")]
    input: PathBuf,

    /// Output file
    #[arg(value_name = "OUTPUT")]
    output: PathBuf,

    /// Bounding box: min_x,min_y,max_x,max_y (geographic/world coordinates)
    #[arg(long, value_name = "BBOX")]
    bbox: String,

    /// Overwrite output if it exists
    #[arg(long)]
    overwrite: bool,

    /// Show progress
    #[arg(long, short = 'p')]
    progress: bool,
}

/// Parsed bounding box in world (geographic) coordinates.
#[derive(Debug, Clone, Copy)]
pub struct BoundingBox {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl BoundingBox {
    /// Parse from a comma-separated string "min_x,min_y,max_x,max_y".
    pub fn parse(s: &str) -> Result<Self> {
        let parts: Vec<&str> = s.split(',').collect();
        if parts.len() != 4 {
            anyhow::bail!(
                "Bounding box must have exactly 4 comma-separated values \
                 (min_x,min_y,max_x,max_y), got: {}",
                s
            );
        }
        let min_x = parts[0]
            .trim()
            .parse::<f64>()
            .with_context(|| format!("Failed to parse min_x from '{}'", parts[0]))?;
        let min_y = parts[1]
            .trim()
            .parse::<f64>()
            .with_context(|| format!("Failed to parse min_y from '{}'", parts[1]))?;
        let max_x = parts[2]
            .trim()
            .parse::<f64>()
            .with_context(|| format!("Failed to parse max_x from '{}'", parts[2]))?;
        let max_y = parts[3]
            .trim()
            .parse::<f64>()
            .with_context(|| format!("Failed to parse max_y from '{}'", parts[3]))?;

        if min_x >= max_x {
            anyhow::bail!("min_x ({}) must be less than max_x ({})", min_x, max_x);
        }
        if min_y >= max_y {
            anyhow::bail!("min_y ({}) must be less than max_y ({})", min_y, max_y);
        }

        Ok(Self {
            min_x,
            min_y,
            max_x,
            max_y,
        })
    }

    /// Returns true if this bbox overlaps with another.
    pub fn overlaps(&self, other: &BoundingBox) -> bool {
        self.min_x < other.max_x
            && self.max_x > other.min_x
            && self.min_y < other.max_y
            && self.max_y > other.min_y
    }
}

#[derive(Serialize)]
struct ClipResult {
    input_file: String,
    output_file: String,
    format: String,
    width: Option<u64>,
    height: Option<u64>,
    feature_count: Option<usize>,
    processing_time_ms: u128,
}

pub fn execute(args: ClipArgs, format: OutputFormat) -> Result<()> {
    let start = std::time::Instant::now();

    if !args.input.exists() {
        anyhow::bail!("Input file not found: {}", args.input.display());
    }
    if args.output.exists() && !args.overwrite {
        anyhow::bail!(
            "Output file already exists: {}. Use --overwrite to replace.",
            args.output.display()
        );
    }

    let bbox = BoundingBox::parse(&args.bbox).context("Invalid --bbox")?;

    // Detect format from extension.
    let ext = args
        .input
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    let result = match ext.as_str() {
        "tif" | "tiff" => clip_raster(&args.input, &args.output, bbox, args.progress)?,
        "geojson" | "json" | "shp" => clip_vector(&args.input, &args.output, bbox, args.progress)?,
        other => anyhow::bail!(
            "Unsupported input format '{}'; clip supports .tif/.tiff, .geojson/.json, .shp",
            other
        ),
    };

    let elapsed = start.elapsed().as_millis();

    let final_result = ClipResult {
        input_file: args.input.display().to_string(),
        output_file: args.output.display().to_string(),
        format: result.0,
        width: result.1,
        height: result.2,
        feature_count: result.3,
        processing_time_ms: elapsed,
    };

    match format {
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&final_result)
                .context("Failed to serialize to JSON")?;
            println!("{}", json);
        }
        OutputFormat::Text => {
            println!(
                "{} Clipped: {} → {}",
                style("✓").green().bold(),
                args.input.display(),
                args.output.display()
            );
            if let (Some(w), Some(h)) = (final_result.width, final_result.height) {
                println!("   Output size : {}×{} pixels", w, h);
            }
            if let Some(fc) = final_result.feature_count {
                println!("   Features    : {}", fc);
            }
            println!("   Elapsed     : {} ms", elapsed);
        }
    }

    Ok(())
}

/// Returns (format_name, width, height, feature_count).
type ClipSummary = (String, Option<u64>, Option<u64>, Option<usize>);

// ── Raster clip ──────────────────────────────────────────────────────────────

fn clip_raster(
    input: &Path,
    output: &Path,
    bbox: BoundingBox,
    show_progress: bool,
) -> Result<ClipSummary> {
    let pb = if show_progress {
        Some(progress::create_spinner("Reading raster metadata"))
    } else {
        None
    };

    let info = raster::read_raster_info(input).context("Failed to read raster metadata")?;

    let geo_transform = info
        .geo_transform
        .ok_or_else(|| anyhow::anyhow!("Input raster has no GeoTransform; cannot clip by bbox"))?;

    if let Some(ref p) = pb {
        p.set_message("Computing pixel window");
    }

    let (x_off, y_off, width, height) = raster::geo_to_pixel_window(
        &geo_transform,
        bbox.min_x,
        bbox.min_y,
        bbox.max_x,
        bbox.max_y,
        info.width,
        info.height,
    )
    .context("Bounding box does not intersect the raster extent")?;

    // Compute the output GeoTransform based on the new pixel origin.
    let clipped_gt = raster::calculate_subset_geotransform(&geo_transform, x_off, y_off);

    // Read and write each band separately to avoid excessive memory usage.
    let bands = info.bands;
    let mut band_buffers = Vec::with_capacity(bands as usize);

    for band_idx in 0..bands {
        if let Some(ref p) = pb {
            p.set_message(format!("Reading band {}/{}", band_idx + 1, bands));
        }

        let buf = raster::read_band_region(input, band_idx, x_off, y_off, width, height)
            .with_context(|| format!("Failed to read band {}", band_idx))?;

        band_buffers.push(buf);
    }

    if let Some(ref p) = pb {
        p.set_message("Writing clipped raster");
    }

    raster::write_multi_band(
        output,
        &band_buffers,
        Some(clipped_gt),
        info.epsg_code,
        info.no_data_value,
    )
    .context("Failed to write clipped raster")?;

    if let Some(ref p) = pb {
        p.finish_and_clear();
    }

    Ok(("GeoTIFF".to_string(), Some(width), Some(height), None))
}

// ── Vector clip ──────────────────────────────────────────────────────────────

fn clip_vector(
    input: &Path,
    output: &Path,
    bbox: BoundingBox,
    show_progress: bool,
) -> Result<ClipSummary> {
    let fmt = vector::VectorFormat::from_path(input)
        .ok_or_else(|| anyhow::anyhow!("Cannot detect input vector format"))?;

    let out_fmt = vector::VectorFormat::from_path(output)
        .ok_or_else(|| anyhow::anyhow!("Cannot detect output vector format"))?;

    let pb = if show_progress {
        Some(progress::create_spinner("Reading vector features"))
    } else {
        None
    };

    let count = match (fmt, out_fmt) {
        (vector::VectorFormat::GeoJson, vector::VectorFormat::GeoJson) => {
            clip_geojson_to_geojson(input, output, bbox, pb.as_ref())?
        }
        (vector::VectorFormat::GeoJson, vector::VectorFormat::Shapefile) => {
            clip_geojson_to_shapefile(input, output, bbox, pb.as_ref())?
        }
        (vector::VectorFormat::Shapefile, vector::VectorFormat::GeoJson) => {
            clip_shapefile_to_geojson(input, output, bbox, pb.as_ref())?
        }
        (vector::VectorFormat::Shapefile, vector::VectorFormat::Shapefile) => {
            clip_shapefile_to_shapefile(input, output, bbox, pb.as_ref())?
        }
        _ => anyhow::bail!(
            "Unsupported vector clip combination: {:?} → {:?}",
            fmt,
            out_fmt
        ),
    };

    if let Some(ref p) = pb {
        p.finish_and_clear();
    }

    let fmt_str = match fmt {
        vector::VectorFormat::GeoJson => "GeoJSON",
        vector::VectorFormat::Shapefile => "Shapefile",
        vector::VectorFormat::FlatGeobuf => "FlatGeobuf",
    };

    Ok((fmt_str.to_string(), None, None, Some(count)))
}

/// Compute the bounding envelope of a GeoJSON geometry.
fn geojson_geom_bbox(geom: &oxigeo_geojson::Geometry) -> Option<BoundingBox> {
    // Collect all x/y positions.
    let mut positions: Vec<&[f64]> = Vec::new();
    collect_geojson_positions(geom, &mut positions);

    if positions.is_empty() {
        return None;
    }

    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for pos in &positions {
        if pos.len() >= 2 {
            min_x = min_x.min(pos[0]);
            max_x = max_x.max(pos[0]);
            min_y = min_y.min(pos[1]);
            max_y = max_y.max(pos[1]);
        }
    }

    if min_x > max_x || min_y > max_y {
        return None;
    }

    // For point geometries where min==max, expand slightly so `overlaps` works.
    let (min_x, max_x) = if min_x == max_x {
        (min_x - f64::EPSILON, max_x + f64::EPSILON)
    } else {
        (min_x, max_x)
    };
    let (min_y, max_y) = if min_y == max_y {
        (min_y - f64::EPSILON, max_y + f64::EPSILON)
    } else {
        (min_y, max_y)
    };

    Some(BoundingBox {
        min_x,
        min_y,
        max_x,
        max_y,
    })
}

fn collect_geojson_positions<'a>(geom: &'a oxigeo_geojson::Geometry, out: &mut Vec<&'a [f64]>) {
    use oxigeo_geojson::Geometry;
    match geom {
        Geometry::Point(p) => out.push(&p.coordinates),
        Geometry::LineString(ls) => {
            for pos in &ls.coordinates {
                out.push(pos.as_slice());
            }
        }
        Geometry::Polygon(p) => {
            for ring in &p.coordinates {
                for pos in ring {
                    out.push(pos.as_slice());
                }
            }
        }
        Geometry::MultiPoint(mp) => {
            for pos in &mp.coordinates {
                out.push(pos.as_slice());
            }
        }
        Geometry::MultiLineString(mls) => {
            for ls in &mls.coordinates {
                for pos in ls {
                    out.push(pos.as_slice());
                }
            }
        }
        Geometry::MultiPolygon(mpoly) => {
            for poly in &mpoly.coordinates {
                for ring in poly {
                    for pos in ring {
                        out.push(pos.as_slice());
                    }
                }
            }
        }
        Geometry::GeometryCollection(gc) => {
            for g in &gc.geometries {
                collect_geojson_positions(g, out);
            }
        }
    }
}

fn clip_geojson_to_geojson(
    input: &Path,
    output: &Path,
    bbox: BoundingBox,
    pb: Option<&indicatif::ProgressBar>,
) -> Result<usize> {
    use oxigeo_geojson::{FeatureCollection, GeoJsonReader, GeoJsonWriter};
    use std::fs::File;
    use std::io::{BufReader, BufWriter};

    let file = File::open(input).with_context(|| format!("Failed to open {}", input.display()))?;
    let mut reader = GeoJsonReader::new(BufReader::new(file));
    let fc = reader
        .read_feature_collection()
        .context("Failed to read GeoJSON")?;

    if let Some(p) = pb {
        p.set_message(format!("Filtering {} features", fc.features.len()));
    }

    let filtered: Vec<_> = fc
        .features
        .into_iter()
        .filter(|f| match &f.geometry {
            None => false,
            Some(geom) => geojson_geom_bbox(geom)
                .map(|gb| bbox.overlaps(&gb))
                .unwrap_or(false),
        })
        .collect();

    let count = filtered.len();
    let out_fc = FeatureCollection::new(filtered);

    let out_file =
        File::create(output).with_context(|| format!("Failed to create {}", output.display()))?;
    let mut writer = GeoJsonWriter::pretty(BufWriter::new(out_file));
    writer
        .write_feature_collection(&out_fc)
        .context("Failed to write GeoJSON")?;

    Ok(count)
}

fn clip_geojson_to_shapefile(
    input: &Path,
    output: &Path,
    bbox: BoundingBox,
    pb: Option<&indicatif::ProgressBar>,
) -> Result<usize> {
    use oxigeo_geojson::GeoJsonReader;
    use oxigeo_shapefile::ShapefileWriter;
    use std::fs::File;
    use std::io::BufReader;

    let file = File::open(input).with_context(|| format!("Failed to open {}", input.display()))?;
    let mut reader = GeoJsonReader::new(BufReader::new(file));
    let fc = reader
        .read_feature_collection()
        .context("Failed to read GeoJSON")?;

    if let Some(p) = pb {
        p.set_message(format!("Filtering {} features", fc.features.len()));
    }

    let filtered: Vec<_> = fc
        .features
        .into_iter()
        .filter(|f| match &f.geometry {
            None => false,
            Some(geom) => geojson_geom_bbox(geom)
                .map(|gb| bbox.overlaps(&gb))
                .unwrap_or(false),
        })
        .collect();

    if filtered.is_empty() {
        anyhow::bail!("No features overlap the bounding box; cannot write empty Shapefile");
    }

    let count = filtered.len();
    let (shape_type, field_descriptors) = vector::infer_shapefile_schema_from_geojson(&filtered)?;
    let field_names: Vec<String> = field_descriptors.iter().map(|d| d.name.clone()).collect();

    let base_path = output.with_extension("");
    let mut writer = ShapefileWriter::new(&base_path, shape_type, field_descriptors)
        .context("Failed to create Shapefile writer")?;

    let sf_features: Vec<_> = filtered
        .iter()
        .enumerate()
        .map(|(i, f)| {
            // Reuse the existing geojson→shapefile conversion in util/vector.
            let geom = match &f.geometry {
                Some(g) => Some(vector::geojson_geom_to_core(g)?),
                None => None,
            };
            let mut attrs = std::collections::HashMap::new();
            if let Some(props) = &f.properties {
                for name in &field_names {
                    let original = props
                        .keys()
                        .find(|k| k.chars().take(10).collect::<String>() == *name)
                        .cloned();
                    if let Some(key) = original {
                        let val = props.get(&key).cloned().unwrap_or(serde_json::Value::Null);
                        attrs.insert(name.clone(), json_to_field_value(&val));
                    } else {
                        attrs.insert(name.clone(), oxigeo_core::vector::FieldValue::Null);
                    }
                }
            }
            Ok(oxigeo_shapefile::reader::ShapefileFeature::new(
                (i + 1) as i32,
                geom,
                attrs,
            ))
        })
        .collect::<Result<Vec<_>>>()?;

    writer
        .write_features(&sf_features)
        .context("Failed to write Shapefile")?;

    Ok(count)
}

fn clip_shapefile_to_geojson(
    input: &Path,
    output: &Path,
    bbox: BoundingBox,
    pb: Option<&indicatif::ProgressBar>,
) -> Result<usize> {
    use oxigeo_geojson::{FeatureCollection, GeoJsonWriter};
    use oxigeo_shapefile::ShapefileReader;
    use std::fs::File;
    use std::io::BufWriter;

    let base_path = input.with_extension("");
    let reader = ShapefileReader::open(&base_path)
        .with_context(|| format!("Failed to open Shapefile: {}", input.display()))?;
    let sf_features = reader.read_features().context("Failed to read Shapefile")?;

    if let Some(p) = pb {
        p.set_message(format!("Filtering {} features", sf_features.len()));
    }

    // Filter by bbox using the core geometry envelope.
    let filtered: Vec<_> = sf_features
        .into_iter()
        .filter(|f| match &f.geometry {
            None => false,
            Some(geom) => {
                let gb = core_geom_bbox(geom);
                gb.map(|b| bbox.overlaps(&b)).unwrap_or(false)
            }
        })
        .collect();

    let count = filtered.len();

    let gj_features = filtered
        .iter()
        .map(|sf| {
            let geom = match &sf.geometry {
                Some(g) => Some(vector::core_geom_to_geojson(g)?),
                None => None,
            };
            let mut props = serde_json::Map::new();
            for (k, v) in &sf.attributes {
                props.insert(k.clone(), v.to_json_value());
            }
            Ok(oxigeo_geojson::Feature::new(geom, Some(props)))
        })
        .collect::<Result<Vec<_>>>()?;

    let out_fc = FeatureCollection::new(gj_features);
    let out_file =
        File::create(output).with_context(|| format!("Failed to create {}", output.display()))?;
    let mut writer = GeoJsonWriter::pretty(BufWriter::new(out_file));
    writer
        .write_feature_collection(&out_fc)
        .context("Failed to write GeoJSON")?;

    Ok(count)
}

fn clip_shapefile_to_shapefile(
    input: &Path,
    output: &Path,
    bbox: BoundingBox,
    pb: Option<&indicatif::ProgressBar>,
) -> Result<usize> {
    use oxigeo_shapefile::{ShapefileReader, ShapefileWriter};

    let base_in = input.with_extension("");
    let reader = ShapefileReader::open(&base_in)
        .with_context(|| format!("Failed to open Shapefile: {}", input.display()))?;
    let sf_features = reader.read_features().context("Failed to read Shapefile")?;

    if let Some(p) = pb {
        p.set_message(format!("Filtering {} features", sf_features.len()));
    }

    let filtered: Vec<_> = sf_features
        .into_iter()
        .filter(|f| match &f.geometry {
            None => false,
            Some(geom) => {
                let gb = core_geom_bbox(geom);
                gb.map(|b| bbox.overlaps(&b)).unwrap_or(false)
            }
        })
        .collect();

    if filtered.is_empty() {
        anyhow::bail!("No features overlap the bounding box; cannot write empty Shapefile");
    }

    let count = filtered.len();
    let (shape_type, field_descriptors) =
        vector::infer_shapefile_schema_from_shapefiles(&filtered)?;
    let base_out = output.with_extension("");
    let mut writer = ShapefileWriter::new(&base_out, shape_type, field_descriptors)
        .context("Failed to create Shapefile writer")?;
    writer
        .write_features(&filtered)
        .context("Failed to write Shapefile")?;

    Ok(count)
}

/// Compute the bounding envelope of a core geometry.
fn core_geom_bbox(geom: &oxigeo_core::vector::Geometry) -> Option<BoundingBox> {
    let mut coords: Vec<(f64, f64)> = Vec::new();
    collect_core_coords(geom, &mut coords);

    if coords.is_empty() {
        return None;
    }

    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;

    for (x, y) in &coords {
        min_x = min_x.min(*x);
        max_x = max_x.max(*x);
        min_y = min_y.min(*y);
        max_y = max_y.max(*y);
    }

    // Expand degenerate envelopes slightly.
    let (min_x, max_x) = if min_x == max_x {
        (min_x - f64::EPSILON, max_x + f64::EPSILON)
    } else {
        (min_x, max_x)
    };
    let (min_y, max_y) = if min_y == max_y {
        (min_y - f64::EPSILON, max_y + f64::EPSILON)
    } else {
        (min_y, max_y)
    };

    Some(BoundingBox {
        min_x,
        min_y,
        max_x,
        max_y,
    })
}

fn collect_core_coords(geom: &oxigeo_core::vector::Geometry, out: &mut Vec<(f64, f64)>) {
    use oxigeo_core::vector::Geometry;
    match geom {
        Geometry::Point(p) => out.push((p.coord.x, p.coord.y)),
        Geometry::LineString(ls) => {
            for c in &ls.coords {
                out.push((c.x, c.y));
            }
        }
        Geometry::Polygon(p) => {
            for c in &p.exterior.coords {
                out.push((c.x, c.y));
            }
            for ring in &p.interiors {
                for c in &ring.coords {
                    out.push((c.x, c.y));
                }
            }
        }
        Geometry::MultiPoint(mp) => {
            for pt in &mp.points {
                out.push((pt.coord.x, pt.coord.y));
            }
        }
        Geometry::MultiLineString(mls) => {
            for ls in &mls.line_strings {
                for c in &ls.coords {
                    out.push((c.x, c.y));
                }
            }
        }
        Geometry::MultiPolygon(mpoly) => {
            for poly in &mpoly.polygons {
                for c in &poly.exterior.coords {
                    out.push((c.x, c.y));
                }
            }
        }
        Geometry::GeometryCollection(gc) => {
            for g in &gc.geometries {
                collect_core_coords(g, out);
            }
        }
    }
}

fn json_to_field_value(v: &serde_json::Value) -> oxigeo_core::vector::FieldValue {
    use oxigeo_core::vector::FieldValue;
    match v {
        serde_json::Value::Null => FieldValue::Null,
        serde_json::Value::Bool(b) => FieldValue::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                FieldValue::Integer(i)
            } else if let Some(u) = n.as_u64() {
                FieldValue::UInteger(u)
            } else {
                FieldValue::Float(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => FieldValue::String(s.clone()),
        other => FieldValue::String(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::raster;
    use oxigeo_core::types::{GeoTransform, NoDataValue, RasterDataType};
    use std::env;

    #[test]
    fn test_clip_bbox_parse_valid() {
        let bb = BoundingBox::parse("1.0,2.0,3.0,4.0").expect("should parse");
        assert!((bb.min_x - 1.0).abs() < f64::EPSILON);
        assert!((bb.min_y - 2.0).abs() < f64::EPSILON);
        assert!((bb.max_x - 3.0).abs() < f64::EPSILON);
        assert!((bb.max_y - 4.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_clip_bbox_parse_negative() {
        let bb = BoundingBox::parse("-180.0,-90.0,180.0,90.0").expect("should parse");
        assert!((bb.min_x - (-180.0)).abs() < f64::EPSILON);
        assert!((bb.max_y - 90.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_clip_bbox_parse_wrong_count() {
        assert!(BoundingBox::parse("1.0,2.0,3.0").is_err());
        assert!(BoundingBox::parse("1.0,2.0,3.0,4.0,5.0").is_err());
    }

    #[test]
    fn test_clip_bbox_parse_inverted() {
        // max < min should fail
        assert!(BoundingBox::parse("5.0,2.0,3.0,4.0").is_err());
        assert!(BoundingBox::parse("1.0,9.0,3.0,4.0").is_err());
    }

    #[test]
    fn test_clip_bbox_overlaps() {
        let a = BoundingBox {
            min_x: 0.0,
            min_y: 0.0,
            max_x: 10.0,
            max_y: 10.0,
        };
        let b = BoundingBox {
            min_x: 5.0,
            min_y: 5.0,
            max_x: 15.0,
            max_y: 15.0,
        };
        let c = BoundingBox {
            min_x: 20.0,
            min_y: 20.0,
            max_x: 30.0,
            max_y: 30.0,
        };
        assert!(a.overlaps(&b));
        assert!(!a.overlaps(&c));
    }

    /// Write a minimal 16×16 Float32 GeoTIFF for integration tests.
    fn write_test_tiff_16x16(path: &Path) -> Result<()> {
        let pixel_count = 16_usize * 16;
        let float_values: Vec<f32> = (0..pixel_count).map(|i| i as f32).collect();
        let mut bytes = Vec::with_capacity(pixel_count * 4);
        for v in &float_values {
            bytes.extend_from_slice(&v.to_ne_bytes());
        }
        let nodata = NoDataValue::None;
        let buf =
            oxigeo_core::buffer::RasterBuffer::new(bytes, 16, 16, RasterDataType::Float32, nodata)
                .map_err(|e| anyhow::anyhow!("{e}"))?;

        let gt = GeoTransform {
            origin_x: 0.0,
            origin_y: 16.0,
            pixel_width: 1.0,
            pixel_height: -1.0,
            row_rotation: 0.0,
            col_rotation: 0.0,
        };
        raster::write_single_band(path, &buf, Some(gt), None, None)
    }

    #[test]
    fn test_clip_raster_integration() -> Result<()> {
        let tmp = env::temp_dir().join(format!("oxigeo_clip_raster_{}", std::process::id()));
        std::fs::create_dir_all(&tmp)?;

        let input = tmp.join("input.tif");
        let output = tmp.join("output.tif");

        write_test_tiff_16x16(&input)?;

        // Clip to the left half (x: 0..8, y: 0..16 in world coords)
        let bbox = BoundingBox::parse("0.0,0.0,8.0,16.0")?;
        clip_raster(&input, &output, bbox, false)?;

        let out_info = raster::read_raster_info(&output)?;
        assert_eq!(out_info.width, 8, "clipped width should be 8");
        assert_eq!(out_info.height, 16, "clipped height should be 16");

        let _ = std::fs::remove_dir_all(&tmp);
        Ok(())
    }
}
