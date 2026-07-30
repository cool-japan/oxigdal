//! Raster utilities for CLI operations

use anyhow::{Context, Result};
use oxigeo_core::{
    buffer::RasterBuffer,
    io::FileDataSource,
    types::{GeoTransform, NoDataValue, RasterDataType},
};
use oxigeo_geotiff::{
    CogWriter, CogWriterOptions, Compression, GeoTiffReader, GeoTiffWriter, GeoTiffWriterOptions,
    WriterConfig,
};
use std::path::Path;

/// Raster metadata extracted from a file
#[derive(Debug, Clone)]
pub struct RasterInfo {
    /// Image width in pixels
    pub width: u64,
    /// Image height in pixels
    pub height: u64,
    /// Number of bands (samples per pixel)
    pub bands: u32,
    /// Data type of raster samples
    pub data_type: RasterDataType,
    /// Geographic transform (origin, pixel size, rotation)
    pub geo_transform: Option<GeoTransform>,
    /// EPSG CRS code, if any
    pub epsg_code: Option<u32>,
    /// NoData value, if any
    pub no_data_value: Option<f64>,
}

/// Read raster metadata from a GeoTIFF file
pub fn read_raster_info(path: &Path) -> Result<RasterInfo> {
    let source = FileDataSource::open(path)
        .with_context(|| format!("Failed to open file: {}", path.display()))?;

    let reader = GeoTiffReader::open(source)
        .with_context(|| format!("Failed to read GeoTIFF: {}", path.display()))?;

    let width = reader.width();
    let height = reader.height();
    let bands = reader.band_count();
    let data_type = reader
        .data_type()
        .ok_or_else(|| anyhow::anyhow!("Could not determine data type"))?;
    let geo_transform = reader.geo_transform().copied();
    let epsg_code = reader.epsg_code();
    let nodata = reader.nodata();
    let no_data_value = nodata.as_f64();

    Ok(RasterInfo {
        width,
        height,
        bands,
        data_type,
        geo_transform,
        epsg_code,
        no_data_value,
    })
}

/// Read a single band from a GeoTIFF file at the primary level
///
/// `band_index` is zero-based. The returned [`RasterBuffer`] contains only the
/// requested band's samples: `GeoTiffReader::read_band` de-interleaves chunky
/// (`PlanarConfiguration = 1`) storage and selects out of planar (`= 2`)
/// storage on our behalf.
///
/// This used to normalise the driver's output through an `extract_single_band`
/// helper, because `read_band` ignored its band argument and returned the whole
/// interleaved image. That is no longer so; see
/// <https://github.com/cool-japan/oxigeo/issues/14>.
pub fn read_band(path: &Path, band_index: u32) -> Result<RasterBuffer> {
    let source = FileDataSource::open(path)
        .with_context(|| format!("Failed to open file: {}", path.display()))?;

    let reader = GeoTiffReader::open(source)
        .with_context(|| format!("Failed to read GeoTIFF: {}", path.display()))?;

    let width = reader.width();
    let height = reader.height();
    let data_type = reader
        .data_type()
        .ok_or_else(|| anyhow::anyhow!("Could not determine data type"))?;
    let nodata = reader.nodata();
    let samples_per_pixel = reader.band_count();

    if band_index >= samples_per_pixel {
        anyhow::bail!(
            "Band index {} out of range (file has {} band(s))",
            band_index,
            samples_per_pixel
        );
    }

    let data = reader
        .read_band(0, band_index as usize)
        .with_context(|| "Failed to read band data")?;

    check_plane_len(
        data.len(),
        width,
        height,
        band_index,
        data_type.size_bytes(),
    )?;

    RasterBuffer::new(data, width, height, data_type, nodata)
        .with_context(|| "Failed to create RasterBuffer from band data")
}

/// Rejects a band plane that is not `width * height * bytes_per_sample` bytes.
///
/// `RasterBuffer::new` would reject it too, but with a message that does not
/// mention the band; this turns a driver-side regression into a clear report.
fn check_plane_len(
    got: usize,
    width: u64,
    height: u64,
    band_index: u32,
    bytes_per_sample: usize,
) -> Result<()> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|px| px.checked_mul(bytes_per_sample))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Raster dimensions {}x{} ({} bytes/sample) overflow usize",
                width,
                height,
                bytes_per_sample
            )
        })?;

    if got != expected {
        anyhow::bail!(
            "Unexpected band {} data size: got {} bytes, expected {} ({}x{} x {} byte(s))",
            band_index,
            got,
            expected,
            width,
            height,
            bytes_per_sample
        );
    }
    Ok(())
}

/// Read a region from a specific band of a GeoTIFF file
///
/// The region is clamped to the image extent. `GeoTiffReader::read_window`
/// touches only the tiles or strips that overlap it and returns just this
/// band's samples, so this no longer stitches tiles and de-interleaves by hand.
/// The hand-rolled version could not read `PlanarConfiguration = 2` files
/// correctly; see <https://github.com/cool-japan/oxigeo/issues/14>.
pub fn read_band_region(
    path: &Path,
    band_index: u32,
    x_offset: u64,
    y_offset: u64,
    width: u64,
    height: u64,
) -> Result<RasterBuffer> {
    let source = FileDataSource::open(path)
        .with_context(|| format!("Failed to open file: {}", path.display()))?;

    let reader = GeoTiffReader::open(source)
        .with_context(|| format!("Failed to read GeoTIFF: {}", path.display()))?;

    // Validate region bounds
    let img_width = reader.width();
    let img_height = reader.height();

    if x_offset >= img_width || y_offset >= img_height {
        anyhow::bail!(
            "Region offset ({}, {}) is outside image bounds ({}x{})",
            x_offset,
            y_offset,
            img_width,
            img_height
        );
    }

    // Clamp region to image bounds
    let actual_width = width.min(img_width.saturating_sub(x_offset));
    let actual_height = height.min(img_height.saturating_sub(y_offset));

    if actual_width == 0 || actual_height == 0 {
        anyhow::bail!("Invalid region dimensions");
    }

    let data_type = reader
        .data_type()
        .ok_or_else(|| anyhow::anyhow!("Could not determine data type"))?;
    let nodata = reader.nodata();
    let samples_per_pixel = reader.band_count();

    if band_index >= samples_per_pixel {
        anyhow::bail!(
            "Band index {} out of range (file has {} band(s))",
            band_index,
            samples_per_pixel
        );
    }

    let output = reader
        .read_window(
            0,
            band_index as usize,
            x_offset,
            y_offset,
            actual_width,
            actual_height,
        )
        .with_context(|| {
            format!(
                "Failed to read region ({}, {}) {}x{} of band {}",
                x_offset, y_offset, actual_width, actual_height, band_index
            )
        })?;

    check_plane_len(
        output.len(),
        actual_width,
        actual_height,
        band_index,
        data_type.size_bytes(),
    )?;

    RasterBuffer::new(output, actual_width, actual_height, data_type, nodata)
        .with_context(|| "Failed to create RasterBuffer from region data")
}

/// Write a single band to a GeoTIFF file
pub fn write_single_band(
    path: &Path,
    buffer: &RasterBuffer,
    geo_transform: Option<GeoTransform>,
    epsg_code: Option<u32>,
    no_data_value: Option<f64>,
) -> Result<()> {
    // Create writer configuration
    let mut config = WriterConfig::new(buffer.width(), buffer.height(), 1, buffer.data_type());

    // Set geo_transform if provided
    if let Some(gt) = geo_transform {
        config = config.with_geo_transform(gt);
    }

    // Set EPSG code if provided
    if let Some(epsg) = epsg_code {
        config = config.with_epsg_code(epsg);
    }

    // Set NoData value if provided
    if let Some(no_data) = no_data_value {
        let nodata_val = match buffer.data_type() {
            RasterDataType::Int8
            | RasterDataType::Int16
            | RasterDataType::Int32
            | RasterDataType::Int64
            | RasterDataType::UInt8
            | RasterDataType::UInt16
            | RasterDataType::UInt32
            | RasterDataType::UInt64 => NoDataValue::Integer(no_data as i64),
            _ => NoDataValue::Float(no_data),
        };
        config = config.with_nodata(nodata_val);
    }

    // Create writer with config and options
    let mut writer = GeoTiffWriter::create(path, config, GeoTiffWriterOptions::default())
        .with_context(|| format!("Failed to create GeoTIFF: {}", path.display()))?;

    // Write the band data
    writer
        .write(buffer.as_bytes())
        .with_context(|| format!("Failed to write band to {}", path.display()))?;

    Ok(())
}

/// Write multiple bands to a GeoTIFF file
pub fn write_multi_band(
    path: &Path,
    buffers: &[RasterBuffer],
    geo_transform: Option<GeoTransform>,
    epsg_code: Option<u32>,
    no_data_value: Option<f64>,
) -> Result<()> {
    if buffers.is_empty() {
        anyhow::bail!("No bands provided");
    }

    // Verify all bands have the same dimensions and data type
    let first_width = buffers[0].width();
    let first_height = buffers[0].height();
    let first_data_type = buffers[0].data_type();
    for (i, buffer) in buffers.iter().enumerate().skip(1) {
        if buffer.width() != first_width || buffer.height() != first_height {
            anyhow::bail!(
                "Band {} has different dimensions ({} x {}) than first band ({} x {})",
                i,
                buffer.width(),
                buffer.height(),
                first_width,
                first_height
            );
        }
        if buffer.data_type() != first_data_type {
            anyhow::bail!(
                "Band {} has different data type ({:?}) than first band ({:?})",
                i,
                buffer.data_type(),
                first_data_type
            );
        }
    }

    // Interleave band data (pixel-by-pixel, all bands per pixel)
    let bytes_per_pixel = first_data_type.size_bytes() as u64;
    let pixel_count = first_width * first_height;
    let total_bytes = (pixel_count * bytes_per_pixel * buffers.len() as u64) as usize;
    let mut interleaved_data = vec![0u8; total_bytes];

    for pixel_idx in 0..pixel_count {
        for (band_idx, buffer) in buffers.iter().enumerate() {
            let src_offset = (pixel_idx * bytes_per_pixel) as usize;
            let dst_offset = ((pixel_idx * bytes_per_pixel) * buffers.len() as u64
                + band_idx as u64 * bytes_per_pixel) as usize;
            let src_end = src_offset + (bytes_per_pixel as usize);
            let dst_end = dst_offset + (bytes_per_pixel as usize);
            interleaved_data[dst_offset..dst_end]
                .copy_from_slice(&buffer.as_bytes()[src_offset..src_end]);
        }
    }

    // Create writer configuration
    let mut config = WriterConfig::new(
        first_width,
        first_height,
        buffers.len() as u16,
        first_data_type,
    );

    // Set geo_transform if provided
    if let Some(gt) = geo_transform {
        config = config.with_geo_transform(gt);
    }

    // Set EPSG code if provided
    if let Some(epsg) = epsg_code {
        config = config.with_epsg_code(epsg);
    }

    // Set NoData value if provided
    if let Some(no_data) = no_data_value {
        let nodata_val = match first_data_type {
            RasterDataType::Int8
            | RasterDataType::Int16
            | RasterDataType::Int32
            | RasterDataType::Int64
            | RasterDataType::UInt8
            | RasterDataType::UInt16
            | RasterDataType::UInt32
            | RasterDataType::UInt64 => NoDataValue::Integer(no_data as i64),
            _ => NoDataValue::Float(no_data),
        };
        config = config.with_nodata(nodata_val);
    }

    // Create writer with config and options
    let mut writer = GeoTiffWriter::create(path, config, GeoTiffWriterOptions::default())
        .with_context(|| format!("Failed to create GeoTIFF: {}", path.display()))?;

    // Write the interleaved band data
    writer
        .write(&interleaved_data)
        .with_context(|| format!("Failed to write bands to {}", path.display()))?;

    Ok(())
}

/// Options for writing a Cloud-Optimized GeoTIFF.
#[derive(Debug, Clone)]
pub struct CogWriteOptions {
    /// Geographic transform (origin, pixel size, rotation)
    pub geo_transform: Option<GeoTransform>,
    /// EPSG CRS code
    pub epsg_code: Option<u32>,
    /// NoData fill value
    pub no_data_value: Option<f64>,
    /// Overview downsampling factors (e.g., `[2, 4, 8, 16]`).
    /// An empty `Vec` means no overviews.
    pub overview_levels: Vec<u32>,
    /// COG tile size in pixels (must be a power of 2)
    pub tile_size: u32,
    /// Compression scheme
    pub compression: Compression,
}

impl Default for CogWriteOptions {
    fn default() -> Self {
        Self {
            geo_transform: None,
            epsg_code: None,
            no_data_value: None,
            overview_levels: vec![2, 4, 8, 16],
            tile_size: 256,
            compression: Compression::Lzw,
        }
    }
}

/// Writes raster bands to a Cloud-Optimized GeoTIFF (COG).
///
/// `options.overview_levels` is a list of downsampling factors (e.g., `[2, 4, 8, 16]`).
/// An empty `Vec` means "no overviews".
pub fn write_raster_cog(
    path: &Path,
    buffers: &[RasterBuffer],
    options: CogWriteOptions,
) -> Result<()> {
    let CogWriteOptions {
        geo_transform,
        epsg_code,
        no_data_value,
        overview_levels,
        tile_size,
        compression,
    } = options;
    if buffers.is_empty() {
        anyhow::bail!("No bands provided for COG write");
    }

    let first_width = buffers[0].width();
    let first_height = buffers[0].height();
    let first_data_type = buffers[0].data_type();

    for (i, buffer) in buffers.iter().enumerate().skip(1) {
        if buffer.width() != first_width || buffer.height() != first_height {
            anyhow::bail!(
                "Band {} has different dimensions than the first band ({} x {} vs {} x {})",
                i,
                buffer.width(),
                buffer.height(),
                first_width,
                first_height
            );
        }
        if buffer.data_type() != first_data_type {
            anyhow::bail!(
                "Band {} has different data type ({:?}) than first band ({:?})",
                i,
                buffer.data_type(),
                first_data_type
            );
        }
    }

    // Interleave band data exactly as write_multi_band does
    let bytes_per_pixel = first_data_type.size_bytes() as u64;
    let pixel_count = first_width * first_height;
    let total_bytes = (pixel_count * bytes_per_pixel * buffers.len() as u64) as usize;
    let mut interleaved_data = vec![0u8; total_bytes];

    for pixel_idx in 0..pixel_count {
        for (band_idx, buffer) in buffers.iter().enumerate() {
            let src_offset = (pixel_idx * bytes_per_pixel) as usize;
            let dst_offset = ((pixel_idx * bytes_per_pixel) * buffers.len() as u64
                + band_idx as u64 * bytes_per_pixel) as usize;
            let src_end = src_offset + bytes_per_pixel as usize;
            let dst_end = dst_offset + bytes_per_pixel as usize;
            interleaved_data[dst_offset..dst_end]
                .copy_from_slice(&buffer.as_bytes()[src_offset..src_end]);
        }
    }

    let generate_overviews = !overview_levels.is_empty();

    let mut config = WriterConfig::new(
        first_width,
        first_height,
        buffers.len() as u16,
        first_data_type,
    )
    .with_compression(compression)
    .with_tile_size(tile_size, tile_size);

    if let Some(gt) = geo_transform {
        config = config.with_geo_transform(gt);
    }
    if let Some(epsg) = epsg_code {
        config = config.with_epsg_code(epsg);
    }
    if let Some(no_data) = no_data_value {
        let nodata_val = match first_data_type {
            RasterDataType::Int8
            | RasterDataType::Int16
            | RasterDataType::Int32
            | RasterDataType::Int64
            | RasterDataType::UInt8
            | RasterDataType::UInt16
            | RasterDataType::UInt32
            | RasterDataType::UInt64 => NoDataValue::Integer(no_data as i64),
            _ => NoDataValue::Float(no_data),
        };
        config = config.with_nodata(nodata_val);
    }

    use oxigeo_geotiff::OverviewResampling;
    config = config.with_overviews(generate_overviews, OverviewResampling::Average);
    if generate_overviews {
        config = config.with_overview_levels(overview_levels);
    }

    let mut writer = CogWriter::create(path, config, CogWriterOptions::default())
        .with_context(|| format!("Failed to create COG: {}", path.display()))?;

    writer
        .write(&interleaved_data)
        .with_context(|| format!("Failed to write COG data to {}", path.display()))?;

    Ok(())
}

/// Reads raster info from a URI or bare file path.
///
/// Cloud URIs (`s3://`, `gs://`, `az://`) and `file://` URIs give a clear error
/// directing the user to use local paths until GeoTiffReader is wired to accept
/// arbitrary DataSource objects.
pub fn read_raster_info_uri(uri: &str) -> Result<RasterInfo> {
    if crate::util::cloud::is_cloud_uri(uri) || uri.starts_with("file://") {
        // Opening via the cloud/URI datasource path is not yet wired to
        // GeoTiffReader<T: DataSource> in this crate. Give a helpful error.
        anyhow::bail!(
            "cloud URI reading for raster requires GeoTiffReader<DataSource>; \
             use a local file path for now (got: {})",
            uri
        );
    }
    read_raster_info(Path::new(uri))
}

/// Calculate output geotransform for a subset operation
pub fn calculate_subset_geotransform(
    original: &GeoTransform,
    x_offset: u64,
    y_offset: u64,
) -> GeoTransform {
    let new_origin_x = original.origin_x + (x_offset as f64 * original.pixel_width);
    let new_origin_y = original.origin_y + (y_offset as f64 * original.pixel_height);

    GeoTransform {
        origin_x: new_origin_x,
        origin_y: new_origin_y,
        pixel_width: original.pixel_width,
        pixel_height: original.pixel_height,
        row_rotation: original.row_rotation,
        col_rotation: original.col_rotation,
    }
}

/// Calculate pixel window from geographic bounding box
pub fn geo_to_pixel_window(
    geo_transform: &GeoTransform,
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
    raster_width: u64,
    raster_height: u64,
) -> Result<(u64, u64, u64, u64)> {
    // Calculate inverse geotransform
    let det = geo_transform.pixel_width * geo_transform.pixel_height
        - geo_transform.row_rotation * geo_transform.col_rotation;

    if det.abs() < 1e-10 {
        anyhow::bail!("Invalid geotransform: determinant is zero");
    }

    // Convert corner coordinates to pixel space using inverse geotransform
    // Inverse formulas: pixel_x = (pixel_height * (geo_x - origin_x) - col_rotation * (geo_y - origin_y)) / det
    //                   pixel_y = (-row_rotation * (geo_x - origin_x) + pixel_width * (geo_y - origin_y)) / det
    let calc_pixel_x = |geo_x: f64, geo_y: f64| -> f64 {
        (geo_transform.pixel_height * (geo_x - geo_transform.origin_x)
            - geo_transform.col_rotation * (geo_y - geo_transform.origin_y))
            / det
    };

    let calc_pixel_y = |geo_x: f64, geo_y: f64| -> f64 {
        (-geo_transform.row_rotation * (geo_x - geo_transform.origin_x)
            + geo_transform.pixel_width * (geo_y - geo_transform.origin_y))
            / det
    };

    let px_min_x = calc_pixel_x(min_x, max_y);
    let px_max_x = calc_pixel_x(max_x, min_y);
    let px_min_y = calc_pixel_y(min_x, max_y);
    let px_max_y = calc_pixel_y(max_x, min_y);

    // Clamp to raster bounds
    let x_off = px_min_x.max(0.0).floor() as u64;
    let y_off = px_min_y.max(0.0).floor() as u64;
    let x_max = px_max_x.min(raster_width as f64).ceil() as u64;
    let y_max = px_max_y.min(raster_height as f64).ceil() as u64;

    let width = x_max.saturating_sub(x_off);
    let height = y_max.saturating_sub(y_off);

    if width == 0 || height == 0 {
        anyhow::bail!("Bounding box does not intersect raster");
    }

    Ok((x_off, y_off, width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_subset_geotransform() {
        let original = GeoTransform {
            origin_x: 0.0,
            origin_y: 100.0,
            pixel_width: 1.0,
            pixel_height: -1.0,
            row_rotation: 0.0,
            col_rotation: 0.0,
        };

        let subset = calculate_subset_geotransform(&original, 10, 5);
        assert_eq!(subset.origin_x, 10.0);
        assert_eq!(subset.origin_y, 95.0);
        assert_eq!(subset.pixel_width, 1.0);
        assert_eq!(subset.pixel_height, -1.0);
    }

    #[test]
    fn test_geo_to_pixel_window() {
        let geo_transform = GeoTransform {
            origin_x: 0.0,
            origin_y: 100.0,
            pixel_width: 1.0,
            pixel_height: -1.0,
            row_rotation: 0.0,
            col_rotation: 0.0,
        };

        let result = geo_to_pixel_window(&geo_transform, 10.0, 80.0, 20.0, 90.0, 100, 100);
        assert!(result.is_ok());

        let (x_off, y_off, width, height) = result.expect("should succeed");
        assert_eq!(x_off, 10);
        assert_eq!(y_off, 10);
        assert_eq!(width, 10);
        assert_eq!(height, 10);
    }
}
