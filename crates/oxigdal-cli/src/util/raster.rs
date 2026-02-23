//! Raster utilities for CLI operations

use anyhow::{Context, Result};
use oxigdal_core::{
    buffer::RasterBuffer,
    io::FileDataSource,
    types::{GeoTransform, NoDataValue, RasterDataType},
};
use oxigdal_geotiff::{GeoTiffReader, GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};
use std::path::Path;

/// Raster metadata extracted from a file
#[derive(Debug, Clone)]
pub struct RasterInfo {
    pub width: u64,
    pub height: u64,
    pub bands: u32,
    pub data_type: RasterDataType,
    pub geo_transform: Option<GeoTransform>,
    pub epsg_code: Option<u32>,
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

/// Read all bands from a GeoTIFF file at the primary level
pub fn read_band(path: &Path, _band_index: u32) -> Result<RasterBuffer> {
    let source = FileDataSource::open(path)
        .with_context(|| format!("Failed to open file: {}", path.display()))?;

    let reader = GeoTiffReader::open(source)
        .with_context(|| format!("Failed to read GeoTIFF: {}", path.display()))?;

    let data = reader
        .read_band(0, 0)
        .with_context(|| "Failed to read band data")?;

    let width = reader.width();
    let height = reader.height();
    let data_type = reader
        .data_type()
        .ok_or_else(|| anyhow::anyhow!("Could not determine data type"))?;
    let nodata = reader.nodata();

    RasterBuffer::new(data, width, height, data_type, nodata)
        .with_context(|| "Failed to create RasterBuffer from band data")
}

/// Read a region from a specific band of a GeoTIFF file
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

    // Get tile/strip information
    let bytes_per_sample = data_type.size_bytes();
    let samples_per_pixel = reader.band_count() as usize;
    let bytes_per_pixel = bytes_per_sample * samples_per_pixel;

    // Check if this is a tiled layout
    let tile_size = reader.tile_size();

    if tile_size.is_none() {
        // Striped or non-tiled layout - read full band and subset
        let region = ImageRegion::new(
            img_width,
            img_height,
            x_offset,
            y_offset,
            actual_width,
            actual_height,
        );
        let config = RasterConfig::new(bytes_per_pixel, data_type, nodata);
        return read_and_subset_strip(&reader, band_index, region, config);
    }

    // Tiled layout - read only overlapping tiles
    let (tile_width, tile_height) =
        tile_size.ok_or_else(|| anyhow::anyhow!("Tile size not available"))?;
    let tile_width = tile_width as u64;
    let tile_height = tile_height as u64;

    let (tiles_x, tiles_y) = reader.tile_count();

    // Calculate tile range that overlaps with the region
    let tile_x_start = (x_offset / tile_width) as u32;
    let tile_y_start = (y_offset / tile_height) as u32;
    let tile_x_end = (x_offset + actual_width)
        .div_ceil(tile_width)
        .min(tiles_x as u64) as u32;
    let tile_y_end = (y_offset + actual_height)
        .div_ceil(tile_height)
        .min(tiles_y as u64) as u32;

    // Allocate output buffer
    let output_size = (actual_width * actual_height) as usize * bytes_per_pixel;
    let mut output = vec![0u8; output_size];

    // Read and assemble tiles
    for tile_y in tile_y_start..tile_y_end {
        for tile_x in tile_x_start..tile_x_end {
            let tile_data = reader
                .read_tile(0, tile_x, tile_y)
                .with_context(|| format!("Failed to read tile ({}, {})", tile_x, tile_y))?;

            // Calculate tile boundaries in image coordinates
            let tile_img_x = tile_x as u64 * tile_width;
            let tile_img_y = tile_y as u64 * tile_height;

            // Calculate intersection of tile with requested region
            let copy_x_start = x_offset.max(tile_img_x);
            let copy_y_start = y_offset.max(tile_img_y);
            let copy_x_end = (x_offset + actual_width).min(tile_img_x + tile_width);
            let copy_y_end = (y_offset + actual_height).min(tile_img_y + tile_height);

            // Copy data row by row
            for row in copy_y_start..copy_y_end {
                // Source position in tile
                let tile_row = (row - tile_img_y) as usize;
                let tile_col_start = (copy_x_start - tile_img_x) as usize;
                let tile_col_end = (copy_x_end - tile_img_x) as usize;

                // Destination position in output
                let out_row = (row - y_offset) as usize;
                let out_col_start = (copy_x_start - x_offset) as usize;

                let src_offset =
                    (tile_row * tile_width as usize + tile_col_start) * bytes_per_pixel;
                let dst_offset =
                    (out_row * actual_width as usize + out_col_start) * bytes_per_pixel;
                let copy_bytes = (tile_col_end - tile_col_start) * bytes_per_pixel;

                if src_offset + copy_bytes <= tile_data.len()
                    && dst_offset + copy_bytes <= output.len()
                {
                    output[dst_offset..dst_offset + copy_bytes]
                        .copy_from_slice(&tile_data[src_offset..src_offset + copy_bytes]);
                }
            }
        }
    }

    RasterBuffer::new(output, actual_width, actual_height, data_type, nodata)
        .with_context(|| "Failed to create RasterBuffer from region data")
}

/// Image dimensions and region configuration
#[derive(Debug, Clone, Copy)]
struct ImageRegion {
    img_width: u64,
    img_height: u64,
    x_offset: u64,
    y_offset: u64,
    width: u64,
    height: u64,
}

impl ImageRegion {
    fn new(
        img_width: u64,
        img_height: u64,
        x_offset: u64,
        y_offset: u64,
        width: u64,
        height: u64,
    ) -> Self {
        Self {
            img_width,
            img_height,
            x_offset,
            y_offset,
            width,
            height,
        }
    }
}

/// Raster data configuration
#[derive(Debug, Clone, Copy)]
struct RasterConfig {
    bytes_per_pixel: usize,
    data_type: RasterDataType,
    nodata: NoDataValue,
}

impl RasterConfig {
    fn new(bytes_per_pixel: usize, data_type: RasterDataType, nodata: NoDataValue) -> Self {
        Self {
            bytes_per_pixel,
            data_type,
            nodata,
        }
    }
}

/// Helper function for reading and subsetting strip-based (non-tiled) data
fn read_and_subset_strip(
    reader: &GeoTiffReader<FileDataSource>,
    band_index: u32,
    region: ImageRegion,
    config: RasterConfig,
) -> Result<RasterBuffer> {
    // Read full band
    let data = reader
        .read_band(0, band_index as usize)
        .with_context(|| "Failed to read band data")?;

    // Subset the data
    let output_size = (region.width * region.height) as usize * config.bytes_per_pixel;
    let mut output = vec![0u8; output_size];

    for row in 0..region.height {
        let src_row = region.y_offset + row;
        if src_row >= region.img_height {
            break;
        }

        let src_offset =
            (src_row * region.img_width + region.x_offset) as usize * config.bytes_per_pixel;
        let dst_offset = (row * region.width) as usize * config.bytes_per_pixel;
        let copy_bytes = region.width as usize * config.bytes_per_pixel;

        if src_offset + copy_bytes <= data.len() && dst_offset + copy_bytes <= output.len() {
            output[dst_offset..dst_offset + copy_bytes]
                .copy_from_slice(&data[src_offset..src_offset + copy_bytes]);
        }
    }

    RasterBuffer::new(
        output,
        region.width,
        region.height,
        config.data_type,
        config.nodata,
    )
    .with_context(|| "Failed to create RasterBuffer from subsetted data")
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
