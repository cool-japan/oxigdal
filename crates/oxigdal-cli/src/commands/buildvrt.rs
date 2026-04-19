//! BuildVRT command - Create virtual raster (VRT) datasets
//!
//! Generates a valid GDAL VRT XML file from one or more input raster files.
//! The VRT mosaics inputs into a union extent, referencing each source by
//! relative path so the VRT is portable.
//!
//! Examples:
//! ```bash
//! # Create a VRT from two adjacent tiles
//! oxigdal buildvrt mosaic.vrt tile1.tif tile2.tif
//!
//! # Force a specific resolution
//! oxigdal buildvrt output.vrt --resolution 0.001 tile1.tif tile2.tif
//!
//! # Overwrite if it exists
//! oxigdal buildvrt output.vrt --overwrite tile1.tif tile2.tif
//! ```

use crate::OutputFormat;
use crate::util::raster;
use anyhow::{Context, Result};
use clap::Args;
use console::style;
use serde::Serialize;
use std::fmt::Write as FmtWrite;
use std::path::{Path, PathBuf};

/// Create virtual raster datasets
#[derive(Args, Debug)]
pub struct BuildVrtArgs {
    /// Output VRT file path
    #[arg(value_name = "OUTPUT")]
    output: PathBuf,

    /// Input raster files
    #[arg(value_name = "INPUT", required = true)]
    inputs: Vec<PathBuf>,

    /// Target resolution (pixel size); if omitted, uses the first input's resolution
    #[arg(long)]
    resolution: Option<f64>,

    /// Target EPSG code (informational only; no reprojection is performed)
    #[arg(long)]
    epsg: Option<u32>,

    /// Overwrite existing output file
    #[arg(long)]
    overwrite: bool,

    /// Creation options (KEY=VALUE, GDAL-compatible)
    #[arg(long = "co", value_parser = crate::util::creation_options::parse_key_value)]
    pub creation_options: Vec<(String, String)>,
}

#[derive(Serialize)]
struct BuildVrtResult {
    output_file: String,
    input_count: usize,
    width: u64,
    height: u64,
    status: String,
}

/// Map `RasterDataType` to the GDAL VRT dataType string.
fn gdal_dtype_str(dt: oxigdal_core::types::RasterDataType) -> &'static str {
    use oxigdal_core::types::RasterDataType;
    match dt {
        RasterDataType::UInt8 => "Byte",
        RasterDataType::UInt16 => "UInt16",
        RasterDataType::Int8 => "Int8",
        RasterDataType::Int16 => "Int16",
        RasterDataType::UInt32 => "UInt32",
        RasterDataType::Int32 => "Int32",
        RasterDataType::UInt64 => "UInt64",
        RasterDataType::Int64 => "Int64",
        RasterDataType::Float32 => "Float32",
        RasterDataType::Float64 => "Float64",
        RasterDataType::CFloat32 => "CFloat32",
        RasterDataType::CFloat64 => "CFloat64",
    }
}

/// Compute the relative path from `base_dir` to `target`, falling back to the
/// absolute path if `target` is not under `base_dir`.
fn relative_path(base_dir: &Path, target: &Path) -> String {
    // Canonicalise both paths if possible; fall back to the raw path on error.
    let base = base_dir
        .canonicalize()
        .unwrap_or_else(|_| base_dir.to_path_buf());
    let tgt = target
        .canonicalize()
        .unwrap_or_else(|_| target.to_path_buf());

    // Walk up from `tgt` to find the common ancestor.
    let base_components: Vec<_> = base.components().collect();
    let tgt_components: Vec<_> = tgt.components().collect();

    let common_len = base_components
        .iter()
        .zip(tgt_components.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let ups = base_components.len() - common_len;
    let mut rel = PathBuf::new();
    for _ in 0..ups {
        rel.push("..");
    }
    for c in &tgt_components[common_len..] {
        rel.push(c);
    }

    rel.to_string_lossy().into_owned()
}

/// Build the VRT XML for a given set of rasters sharing the same mosaic extent.
fn build_vrt_xml(inputs: &[PathBuf], vrt_out: &Path) -> Result<(String, u64, u64)> {
    // Read metadata for every input.
    let infos: Vec<raster::RasterInfo> = inputs
        .iter()
        .map(|p| {
            raster::read_raster_info(p)
                .with_context(|| format!("Failed to read metadata from {}", p.display()))
        })
        .collect::<Result<Vec<_>>>()?;

    // All inputs must have a GeoTransform (otherwise we cannot compute extents).
    for (i, info) in infos.iter().enumerate() {
        if info.geo_transform.is_none() {
            anyhow::bail!(
                "Input {} ({}) has no GeoTransform; cannot build VRT",
                i,
                inputs[i].display()
            );
        }
    }

    // Verify all inputs share the same pixel size (within a small tolerance).
    // SAFETY: geo_transform presence was validated above for all infos.
    let ref_gt = infos[0]
        .geo_transform
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("First input has no GeoTransform"))?;
    let ref_pw = ref_gt.pixel_width;
    let ref_ph = ref_gt.pixel_height;
    let ref_dt = infos[0].data_type;

    for (i, info) in infos.iter().enumerate().skip(1) {
        let gt = info
            .geo_transform
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Input {} has no GeoTransform", i))?;
        let tol = ref_pw.abs() * 1e-6;
        if (gt.pixel_width - ref_pw).abs() > tol || (gt.pixel_height - ref_ph).abs() > tol {
            anyhow::bail!(
                "Input {} ({}) has a different pixel size ({},{}) \
                 than the first input ({},{}); cannot build aligned VRT without resampling",
                i,
                inputs[i].display(),
                gt.pixel_width,
                gt.pixel_height,
                ref_pw,
                ref_ph
            );
        }
        if info.data_type != ref_dt {
            eprintln!(
                "{}",
                style(format!(
                    "Warning: input {} has data type {:?} but first input is {:?}; \
                     VRT will use first input's type",
                    inputs[i].display(),
                    info.data_type,
                    ref_dt
                ))
                .yellow()
            );
        }
    }

    // Compute the union extent in world coordinates.
    let mut union_min_x = f64::INFINITY;
    let mut union_max_x = f64::NEG_INFINITY;
    let mut union_min_y = f64::INFINITY;
    let mut union_max_y = f64::NEG_INFINITY;

    for (idx, info) in infos.iter().enumerate() {
        let gt = info
            .geo_transform
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Input {} has no GeoTransform", idx))?;
        // origin is the top-left corner; pixel_height is negative for north-up images.
        let left = gt.origin_x;
        let right = gt.origin_x + info.width as f64 * gt.pixel_width;
        let top = gt.origin_y;
        let bottom = gt.origin_y + info.height as f64 * gt.pixel_height;

        union_min_x = union_min_x.min(left.min(right));
        union_max_x = union_max_x.max(left.max(right));
        union_min_y = union_min_y.min(top.min(bottom));
        union_max_y = union_max_y.max(top.max(bottom));
    }

    // Mosaic dimensions in pixels (using the first input's pixel size).
    let pw = ref_pw.abs();
    let ph = ref_ph.abs();
    let mosaic_width = ((union_max_x - union_min_x) / pw).round() as u64;
    let mosaic_height = ((union_max_y - union_min_y) / ph).round() as u64;

    if mosaic_width == 0 || mosaic_height == 0 {
        anyhow::bail!("Computed VRT mosaic has zero dimensions");
    }

    // Determine the number of bands (use the maximum across inputs so all sources
    // can contribute at least partially; typically all should match).
    let band_count = infos.iter().map(|i| i.bands).max().unwrap_or(1);

    // Origin for the VRT (top-left corner, north-up convention).
    let vrt_origin_x = union_min_x;
    let vrt_origin_y = union_max_y; // north-up: y-axis flipped

    let dtype_str = gdal_dtype_str(ref_dt);

    // Directory containing the VRT output — used for relative path computation.
    let vrt_dir = vrt_out.parent().unwrap_or_else(|| Path::new("."));

    let mut xml = String::new();

    writeln!(
        xml,
        r#"<VRTDataset rasterXSize="{}" rasterYSize="{}">"#,
        mosaic_width, mosaic_height
    )
    .map_err(|e| anyhow::anyhow!("fmt error: {e}"))?;

    // GeoTransform line
    writeln!(
        xml,
        "  <GeoTransform>{:.15}, {:.15}, 0, {:.15}, 0, {:.15}</GeoTransform>",
        vrt_origin_x, ref_pw, vrt_origin_y, ref_ph,
    )
    .map_err(|e| anyhow::anyhow!("fmt error: {e}"))?;

    // Build each band
    for band_n in 1..=band_count {
        writeln!(
            xml,
            r#"  <VRTRasterBand dataType="{}" band="{}">"#,
            dtype_str, band_n
        )
        .map_err(|e| anyhow::anyhow!("fmt error: {e}"))?;

        // Add a SimpleSource for each input that has at least this many bands.
        for (input_path, info) in inputs.iter().zip(infos.iter()) {
            if info.bands < band_n {
                continue;
            }
            let gt = info
                .geo_transform
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Input has no GeoTransform (band loop)"))?;

            // Offset of this source relative to the VRT mosaic origin.
            let src_left = gt.origin_x;
            let src_top = gt.origin_y;

            // Pixel offset in the destination (VRT) coordinate space.
            let dst_x_off = ((src_left - vrt_origin_x) / ref_pw).round() as i64;
            // vrt_origin_y is the north edge; src_top is also north edge (for north-up images).
            // Positive pixel offset goes downward.
            let dst_y_off = ((vrt_origin_y - src_top) / ph).round() as i64;

            let rel = relative_path(vrt_dir, input_path);

            writeln!(xml, "    <SimpleSource>").map_err(|e| anyhow::anyhow!("fmt: {e}"))?;
            writeln!(
                xml,
                r#"      <SourceFilename relativeToVRT="1">{}</SourceFilename>"#,
                rel
            )
            .map_err(|e| anyhow::anyhow!("fmt: {e}"))?;
            writeln!(xml, "      <SourceBand>{}</SourceBand>", band_n)
                .map_err(|e| anyhow::anyhow!("fmt: {e}"))?;
            writeln!(
                xml,
                r#"      <SourceProperties RasterXSize="{}" RasterYSize="{}" DataType="{}" BlockXSize="256" BlockYSize="256"/>"#,
                info.width, info.height, dtype_str
            )
            .map_err(|e| anyhow::anyhow!("fmt: {e}"))?;
            writeln!(
                xml,
                r#"      <SrcRect xOff="0" yOff="0" xSize="{}" ySize="{}"/>"#,
                info.width, info.height
            )
            .map_err(|e| anyhow::anyhow!("fmt: {e}"))?;
            writeln!(
                xml,
                r#"      <DstRect xOff="{}" yOff="{}" xSize="{}" ySize="{}"/>"#,
                dst_x_off, dst_y_off, info.width, info.height
            )
            .map_err(|e| anyhow::anyhow!("fmt: {e}"))?;
            writeln!(xml, "    </SimpleSource>").map_err(|e| anyhow::anyhow!("fmt: {e}"))?;
        }

        writeln!(xml, "  </VRTRasterBand>").map_err(|e| anyhow::anyhow!("fmt error: {e}"))?;
    }

    writeln!(xml, "</VRTDataset>").map_err(|e| anyhow::anyhow!("fmt error: {e}"))?;

    Ok((xml, mosaic_width, mosaic_height))
}

pub fn execute(args: BuildVrtArgs, format: OutputFormat) -> Result<()> {
    let _co = crate::util::creation_options::map_creation_options(&args.creation_options);

    if args.output.exists() && !args.overwrite {
        anyhow::bail!(
            "Output file already exists: {}. Use --overwrite to replace.",
            args.output.display()
        );
    }

    // Ensure output has .vrt extension (warn if not).
    if args
        .output
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .as_deref()
        != Some("vrt")
    {
        eprintln!(
            "{}",
            style("Warning: output file does not have a .vrt extension").yellow()
        );
    }

    // Validate all inputs exist.
    for input in &args.inputs {
        if !input.exists() {
            anyhow::bail!("Input file not found: {}", input.display());
        }
    }

    let (xml, mosaic_width, mosaic_height) = build_vrt_xml(&args.inputs, &args.output)?;

    // Write the VRT file.
    std::fs::write(&args.output, &xml)
        .with_context(|| format!("Failed to write VRT to {}", args.output.display()))?;

    let result = BuildVrtResult {
        output_file: args.output.display().to_string(),
        input_count: args.inputs.len(),
        width: mosaic_width,
        height: mosaic_height,
        status: "success".to_string(),
    };

    match format {
        OutputFormat::Json => {
            let json =
                serde_json::to_string_pretty(&result).context("Failed to serialize to JSON")?;
            println!("{}", json);
        }
        OutputFormat::Text => {
            println!(
                "{} Created VRT: {}",
                style("✓").green().bold(),
                args.output.display()
            );
            println!("   Mosaic size : {}×{} pixels", mosaic_width, mosaic_height);
            println!("   Input files : {}", args.inputs.len());
            for input in &args.inputs {
                println!("     - {}", input.display());
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::raster;
    use oxigdal_core::types::{GeoTransform, RasterDataType};
    use std::env;

    /// Write a minimal 8×8 Float32 GeoTIFF for testing.
    fn write_test_tiff(
        path: &Path,
        origin_x: f64,
        origin_y: f64,
        pixel_size: f64,
        width: u64,
        height: u64,
    ) -> Result<()> {
        let pixel_count = (width * height) as usize;
        let float_values: Vec<f32> = (0..pixel_count).map(|i| i as f32).collect();

        let mut bytes = Vec::with_capacity(pixel_count * 4);
        for v in &float_values {
            bytes.extend_from_slice(&v.to_ne_bytes());
        }

        let nodata = oxigdal_core::types::NoDataValue::None;
        let buf = oxigdal_core::buffer::RasterBuffer::new(
            bytes,
            width,
            height,
            RasterDataType::Float32,
            nodata,
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;

        let gt = GeoTransform {
            origin_x,
            origin_y,
            pixel_width: pixel_size,
            pixel_height: -pixel_size,
            row_rotation: 0.0,
            col_rotation: 0.0,
        };

        raster::write_single_band(path, &buf, Some(gt), None, None)
    }

    #[test]
    fn test_buildvrt_xml_generation() -> Result<()> {
        let tmp = env::temp_dir().join(format!("oxigdal_vrt_test_{}", std::process::id()));
        std::fs::create_dir_all(&tmp)?;

        let tile1 = tmp.join("tile1.tif");
        let tile2 = tmp.join("tile2.tif");
        let vrt_out = tmp.join("mosaic.vrt");

        // Two 8×8 tiles side by side (second tile starts where first ends).
        write_test_tiff(&tile1, 0.0, 8.0, 1.0, 8, 8)?;
        write_test_tiff(&tile2, 8.0, 8.0, 1.0, 8, 8)?;

        let (xml, w, h) = build_vrt_xml(&[tile1, tile2], &vrt_out)?;

        assert!(xml.contains("<VRTDataset"), "missing VRTDataset element");
        assert!(
            xml.contains("<VRTRasterBand"),
            "missing VRTRasterBand element"
        );
        assert!(xml.contains("<SimpleSource>"), "missing SimpleSource");
        assert!(xml.contains("<GeoTransform>"), "missing GeoTransform");
        assert_eq!(w, 16, "mosaic width should be 16 (two 8-pixel tiles)");
        assert_eq!(h, 8, "mosaic height should be 8");
        assert!(xml.contains("Float32"), "data type should be Float32");

        // Cleanup
        let _ = std::fs::remove_dir_all(&tmp);
        Ok(())
    }

    #[test]
    fn test_buildvrt_single_input() -> Result<()> {
        let tmp = env::temp_dir().join(format!("oxigdal_vrt_single_{}", std::process::id()));
        std::fs::create_dir_all(&tmp)?;

        let tile = tmp.join("single.tif");
        let vrt_out = tmp.join("single.vrt");

        write_test_tiff(&tile, 10.0, 20.0, 0.5, 4, 4)?;

        let (xml, w, h) = build_vrt_xml(&[tile], &vrt_out)?;

        assert_eq!(w, 4);
        assert_eq!(h, 4);
        assert!(xml.contains(r#"relativeToVRT="1""#));

        let _ = std::fs::remove_dir_all(&tmp);
        Ok(())
    }

    #[test]
    fn test_gdal_dtype_str() {
        use oxigdal_core::types::RasterDataType;
        assert_eq!(gdal_dtype_str(RasterDataType::UInt8), "Byte");
        assert_eq!(gdal_dtype_str(RasterDataType::Float32), "Float32");
        assert_eq!(gdal_dtype_str(RasterDataType::Float64), "Float64");
        assert_eq!(gdal_dtype_str(RasterDataType::Int16), "Int16");
        assert_eq!(gdal_dtype_str(RasterDataType::UInt16), "UInt16");
    }
}
