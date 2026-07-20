//! Virtual Raster (VRT) construction from multiple source datasets.
//!
//! This module provides `build_vrt`, which generates a GDAL-compatible VRT
//! XML file describing a mosaic of multiple raster datasets. The resulting
//! file can be opened with [`crate::Dataset::open`] or any GDAL-compatible
//! reader.
//!
//! # Design
//!
//! VRT files are pure XML — no pixel data is copied. Each source dataset
//! contributes a `<VRTRasterBand>` → `<SimpleSource>` stanza that tells
//! readers where to fetch pixel data at runtime.
//!
//! # Examples
//!
//! ```rust,no_run
//! use std::path::Path;
//! use oxigdal::vrt_builder::{build_vrt, VrtOptions, VrtResolution};
//!
//! # fn main() -> oxigdal::Result<()> {
//! let sources = [Path::new("tile_a.tif"), Path::new("tile_b.tif")];
//! let output = Path::new("mosaic.vrt");
//! let options = VrtOptions::default();
//! let ds = build_vrt(&sources, output, options)?;
//! println!("VRT extent: {}×{}", ds.width(), ds.height());
//! # Ok(())
//! # }
//! ```

use std::path::Path;

use crate::{Dataset, DatasetFormat, DatasetInfo, GeoTransform, OxiGdalError, Result};

// ─── Public types ─────────────────────────────────────────────────────────────

/// Resolution rule for combining multiple source rasters.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum VrtResolution {
    /// Use the arithmetic average of all source pixel sizes (default).
    #[default]
    Average,
    /// Use the finest (smallest pixel) resolution.
    Highest,
    /// Use the coarsest (largest pixel) resolution.
    Lowest,
    /// Use a user-specified pixel size.
    ///
    /// The value is the pixel width (and height, for square pixels).
    User(f64),
}

/// Options controlling [`build_vrt`].
#[derive(Debug, Clone, Default)]
pub struct VrtOptions {
    /// Resolution rule for the output VRT.
    pub resolution: VrtResolution,
    /// NoData value for the output bands (applied to all sources).
    pub no_data: Option<f64>,
    /// When `true`, each source file contributes a *separate* band rather than
    /// overlapping bands being composited by first-come-first-served order.
    pub separate_bands: bool,
    /// Source NoData value to mask before compositing.
    pub srcnodata: Option<f64>,
}

// ─── Main entry point ─────────────────────────────────────────────────────────

/// Build a GDAL-compatible VRT mosaic from a list of source raster files.
///
/// The function:
/// 1. Opens each source (or extension fallback) to read width, height, and geotransform.
///    fallback) to read width, height, and geotransform.
/// 2. Computes the union bounding box and output pixel dimensions.
/// 3. Writes GDAL VRT XML to `output_path`.
/// 4. Returns a [`Dataset`] opened from the newly written VRT.
///
/// # Errors
///
/// - [`OxiGdalError::InvalidParameter`] — `sources` is empty or any source
///   cannot be opened.
/// - [`OxiGdalError::Io`] — cannot write the output VRT file.
pub fn build_vrt(sources: &[&Path], output_path: &Path, options: VrtOptions) -> Result<Dataset> {
    if sources.is_empty() {
        return Err(OxiGdalError::InvalidParameter {
            parameter: "sources",
            message: "at least one source file is required to build a VRT".to_string(),
        });
    }

    // Gather metadata for each source.
    let source_metas: Vec<SourceMeta> = sources
        .iter()
        .enumerate()
        .map(|(idx, &src)| {
            read_source_meta(src).map_err(|e| OxiGdalError::InvalidParameter {
                parameter: "sources",
                message: format!(
                    "failed to read metadata for source[{}] '{}': {e}",
                    idx,
                    src.display()
                ),
            })
        })
        .collect::<Result<Vec<_>>>()?;

    // Compute output resolution.
    let pixel_size = resolve_pixel_size(&source_metas, &options.resolution);

    // Compute union bounding box.
    let union_bbox = compute_union_bbox(&source_metas)?;

    // Output dimensions.
    let total_width = ((union_bbox.max_x - union_bbox.min_x) / pixel_size).ceil() as u32;
    let total_height = ((union_bbox.max_y - union_bbox.min_y) / pixel_size).ceil() as u32;

    let band_count: u32 = source_metas.iter().map(|m| m.band_count).max().unwrap_or(1);

    // Write the VRT XML.
    let xml = generate_vrt_xml(
        &source_metas,
        sources,
        &union_bbox,
        total_width,
        total_height,
        pixel_size,
        band_count,
        &options,
    );

    let output_str = output_path
        .to_str()
        .ok_or_else(|| OxiGdalError::InvalidParameter {
            parameter: "output_path",
            message: "output path contains non-UTF-8 characters".to_string(),
        })?;

    std::fs::write(output_path, &xml).map_err(|e| {
        OxiGdalError::Io(oxigdal_core::error::IoError::Write {
            message: format!("failed to write VRT file '{}': {e}", output_str),
        })
    })?;

    // Build a Dataset metadata descriptor for the newly written VRT.
    let vrt_gt = GeoTransform::north_up(union_bbox.min_x, union_bbox.max_y, pixel_size, pixel_size);

    let info = DatasetInfo {
        format: DatasetFormat::Vrt,
        path: Some(output_str.to_string()),
        width: Some(total_width),
        height: Some(total_height),
        band_count,
        layer_count: 0,
        crs: source_metas.first().and_then(|m| m.crs.clone()),
        geotransform: Some(vrt_gt),
        feature_count: None,
        bounds: None,
    };

    Ok(Dataset::from_info(output_str.to_string(), info))
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

/// Lightweight metadata extracted from a source file for VRT construction.
#[derive(Debug, Clone)]
struct SourceMeta {
    /// Pixel width.
    pixel_width: f64,
    /// Pixel height (absolute value, always positive).
    pixel_height: f64,
    /// Raster width in pixels.
    width: u32,
    /// Raster height in pixels.
    height: u32,
    /// Number of bands.
    band_count: u32,
    /// Geotransform origin X (top-left corner).
    origin_x: f64,
    /// Geotransform origin Y (top-left corner).
    origin_y: f64,
    /// GDAL data type string (e.g. "Float32").
    data_type_str: String,
    /// CRS string (optional).
    crs: Option<String>,
}

impl SourceMeta {
    /// Left-most X coordinate.
    fn min_x(&self) -> f64 {
        self.origin_x
    }

    /// Right-most X coordinate.
    fn max_x(&self) -> f64 {
        self.origin_x + self.width as f64 * self.pixel_width
    }

    /// Bottom-most Y coordinate (north-up: origin_y − height × pixel_height).
    fn min_y(&self) -> f64 {
        self.origin_y - self.height as f64 * self.pixel_height
    }

    /// Top-most Y coordinate.
    fn max_y(&self) -> f64 {
        self.origin_y
    }
}

/// Bounding box used internally for VRT construction.
#[derive(Debug, Clone)]
struct Bbox {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

/// Read lightweight raster metadata from a source file.
fn read_source_meta(path: &Path) -> Result<SourceMeta> {
    // Attempt IFD-level parsing for GeoTIFF; fall back to placeholders.
    let info_opt = crate::open::extract_tiff_info(path);

    match info_opt {
        Some(info) => {
            let gt = info
                .geotransform
                .unwrap_or_else(|| GeoTransform::north_up(0.0, 0.0, 1.0, 1.0));
            let width = info.width.unwrap_or(1);
            let height = info.height.unwrap_or(1);
            let pixel_height = gt.pixel_height.abs();
            // Parse the real pixel type (BitsPerSample / SampleFormat) rather
            // than assuming Float32 — UInt8/UInt16 imagery would otherwise be
            // misinterpreted as 4-byte floats by VRT readers.
            let data_type_str = crate::open::extract_tiff_data_type(path)
                .map(|dt| dt.name().to_string())
                .unwrap_or_else(|| "Float32".to_string());
            Ok(SourceMeta {
                pixel_width: gt.pixel_width,
                pixel_height,
                width,
                height,
                band_count: info.band_count.max(1),
                origin_x: gt.origin_x,
                origin_y: gt.origin_y,
                data_type_str,
                crs: info.crs,
            })
        }
        None => Err(OxiGdalError::InvalidParameter {
            parameter: "source",
            message: format!(
                "cannot read raster metadata from '{}' — only GeoTIFF sources are supported",
                path.display()
            ),
        }),
    }
}

/// Compute the output pixel size from source metadata and the resolution rule.
fn resolve_pixel_size(metas: &[SourceMeta], rule: &VrtResolution) -> f64 {
    match rule {
        VrtResolution::User(px) => *px,
        VrtResolution::Average => {
            let sum: f64 = metas.iter().map(|m| m.pixel_width).sum();
            sum / metas.len() as f64
        }
        VrtResolution::Highest => metas
            .iter()
            .map(|m| m.pixel_width)
            .fold(f64::INFINITY, f64::min),
        VrtResolution::Lowest => metas.iter().map(|m| m.pixel_width).fold(0.0f64, f64::max),
    }
}

/// Compute the union bounding box over all sources.
fn compute_union_bbox(metas: &[SourceMeta]) -> Result<Bbox> {
    let first = metas
        .first()
        .ok_or_else(|| OxiGdalError::InvalidParameter {
            parameter: "sources",
            message: "no sources to compute bbox from".to_string(),
        })?;

    let mut min_x = first.min_x();
    let mut min_y = first.min_y();
    let mut max_x = first.max_x();
    let mut max_y = first.max_y();

    for m in metas.iter().skip(1) {
        if m.min_x() < min_x {
            min_x = m.min_x();
        }
        if m.min_y() < min_y {
            min_y = m.min_y();
        }
        if m.max_x() > max_x {
            max_x = m.max_x();
        }
        if m.max_y() > max_y {
            max_y = m.max_y();
        }
    }

    Ok(Bbox {
        min_x,
        min_y,
        max_x,
        max_y,
    })
}

/// Map a GDAL data type string to the VRT-compatible token.
fn gdal_dtype_str(dt_str: &str) -> &str {
    match dt_str {
        "UInt8" => "Byte",
        "UInt16" => "UInt16",
        "Int16" => "Int16",
        "UInt32" => "UInt32",
        "Int32" => "Int32",
        "Float32" => "Float32",
        "Float64" => "Float64",
        other => other,
    }
}

/// Generate the GDAL VRT XML string for the mosaic.
#[allow(clippy::too_many_arguments)]
fn generate_vrt_xml(
    metas: &[SourceMeta],
    sources: &[&Path],
    bbox: &Bbox,
    total_width: u32,
    total_height: u32,
    pixel_size: f64,
    band_count: u32,
    options: &VrtOptions,
) -> String {
    let mut xml = String::with_capacity(4096);

    // VRT header
    xml.push_str("<VRTDataset rasterXSize=\"");
    xml.push_str(&total_width.to_string());
    xml.push_str("\" rasterYSize=\"");
    xml.push_str(&total_height.to_string());
    xml.push_str("\">\n");

    // GeoTransform: origin_x, pixel_width, 0, origin_y, 0, -pixel_height
    xml.push_str("  <GeoTransform>");
    xml.push_str(&format!(
        "{:.10}, {:.10}, 0.0, {:.10}, 0.0, -{:.10}",
        bbox.min_x, pixel_size, bbox.max_y, pixel_size
    ));
    xml.push_str("</GeoTransform>\n");

    // SRS from first source
    if let Some(crs) = metas.first().and_then(|m| m.crs.as_ref()) {
        xml.push_str("  <SRS>");
        xml.push_str(crs);
        xml.push_str("</SRS>\n");
    }

    // Emit VRTRasterBand elements
    for band_idx in 1..=band_count {
        let dt_str = metas
            .first()
            .map(|m| gdal_dtype_str(&m.data_type_str))
            .unwrap_or("Float32");

        xml.push_str("  <VRTRasterBand dataType=\"");
        xml.push_str(dt_str);
        xml.push_str("\" band=\"");
        xml.push_str(&band_idx.to_string());
        xml.push_str("\">\n");

        // NoData
        if let Some(nd) = options.no_data {
            xml.push_str("    <NoDataValue>");
            xml.push_str(&nd.to_string());
            xml.push_str("</NoDataValue>\n");
        }

        // Simple sources
        for (meta, src_path) in metas.iter().zip(sources.iter()) {
            // Offset in output pixels for this source.
            let dst_off_x = ((meta.origin_x - bbox.min_x) / pixel_size).round() as i64;
            let dst_off_y = ((bbox.max_y - meta.max_y()) / pixel_size).round() as i64;
            let dst_w = (meta.width as f64 * meta.pixel_width / pixel_size).ceil() as u32;
            let dst_h = (meta.height as f64 * meta.pixel_height / pixel_size).ceil() as u32;

            let src_path_str = src_path.to_string_lossy();

            xml.push_str("    <SimpleSource>\n");
            xml.push_str("      <SourceFilename relativeToVRT=\"1\">");
            xml.push_str(&src_path_str);
            xml.push_str("</SourceFilename>\n");
            xml.push_str("      <SourceBand>");
            xml.push_str(&band_idx.to_string());
            xml.push_str("</SourceBand>\n");

            // SrcRect: full source extent
            xml.push_str("      <SrcRect xOff=\"0\" yOff=\"0\" xSize=\"");
            xml.push_str(&meta.width.to_string());
            xml.push_str("\" ySize=\"");
            xml.push_str(&meta.height.to_string());
            xml.push_str("\"/>\n");

            // DstRect: where this source maps in the output
            xml.push_str("      <DstRect xOff=\"");
            xml.push_str(&dst_off_x.to_string());
            xml.push_str("\" yOff=\"");
            xml.push_str(&dst_off_y.to_string());
            xml.push_str("\" xSize=\"");
            xml.push_str(&dst_w.to_string());
            xml.push_str("\" ySize=\"");
            xml.push_str(&dst_h.to_string());
            xml.push_str("\"/>\n");

            if let Some(nd) = options.srcnodata {
                xml.push_str("      <NODATA>");
                xml.push_str(&nd.to_string());
                xml.push_str("</NODATA>\n");
            }

            xml.push_str("    </SimpleSource>\n");
        }

        xml.push_str("  </VRTRasterBand>\n");
    }

    xml.push_str("</VRTDataset>\n");
    xml
}

// ─── Dataset convenience wrapper ─────────────────────────────────────────────

impl Dataset {
    /// Build a GDAL-compatible Virtual Raster (VRT) mosaic from multiple source files.
    ///
    /// Delegates to [`build_vrt`].  See its documentation for full details.
    ///
    /// # Errors
    ///
    /// - [`OxiGdalError::InvalidParameter`] — `sources` is empty or a source cannot be opened.
    /// - [`OxiGdalError::Io`] — cannot write the output VRT file.
    pub fn build_vrt(
        sources: &[&Path],
        output_path: &Path,
        options: VrtOptions,
    ) -> Result<Dataset> {
        build_vrt(sources, output_path, options)
    }
}
