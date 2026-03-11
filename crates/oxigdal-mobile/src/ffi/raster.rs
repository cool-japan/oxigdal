//! Raster operations FFI functions.
//!
//! Provides C-compatible functions for working with raster datasets.

use super::types::*;
use crate::{check_null, deref_ptr, deref_ptr_mut, ffi_result};
use oxigdal_core::buffer::RasterBuffer;
use oxigdal_core::io::FileDataSource;
use oxigdal_core::types::RasterDataType;
use oxigdal_geotiff::GeoTiffReader;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_double};
use std::path::Path;
use std::ptr;
use std::sync::Mutex;

/// Internal dataset handle.
///
/// This wraps the actual dataset object and provides a stable pointer
/// for FFI operations.
pub struct DatasetHandle {
    /// Path to the dataset file
    path: String,
    /// Cached metadata
    metadata: OxiGdalMetadata,
    /// Reader (if open for reading)
    reader: Option<Mutex<GeoTiffReader<FileDataSource>>>,
    /// Write buffer for datasets created for writing
    write_buffer: Option<Mutex<WriteBufferData>>,
}

/// Write buffer data for datasets opened for writing
struct WriteBufferData {
    /// Width of the dataset
    width: i32,
    /// Height of the dataset
    height: i32,
    /// Number of bands
    bands: i32,
    /// Data type
    data_type: OxiGdalDataType,
    /// Raster data buffers (one per band)
    band_data: Vec<Vec<u8>>,
    /// Geotransform
    geotransform: [f64; 6],
    /// EPSG code
    epsg_code: i32,
}

/// Tile handle for FFI
pub struct TileHandle {
    /// Tile data
    data: Vec<u8>,
    /// Tile width
    width: i32,
    /// Tile height
    height: i32,
    /// Number of channels
    channels: i32,
}

/// Converts OxiGdalDataType to RasterDataType
fn ffi_data_type_to_core(dt: OxiGdalDataType) -> RasterDataType {
    match dt {
        OxiGdalDataType::Byte => RasterDataType::UInt8,
        OxiGdalDataType::UInt16 => RasterDataType::UInt16,
        OxiGdalDataType::Int16 => RasterDataType::Int16,
        OxiGdalDataType::UInt32 => RasterDataType::UInt32,
        OxiGdalDataType::Int32 => RasterDataType::Int32,
        OxiGdalDataType::Float32 => RasterDataType::Float32,
        OxiGdalDataType::Float64 => RasterDataType::Float64,
    }
}

/// Converts RasterDataType to OxiGdalDataType
fn core_data_type_to_ffi(dt: RasterDataType) -> i32 {
    match dt {
        RasterDataType::UInt8 => OxiGdalDataType::Byte as i32,
        RasterDataType::Int8 => OxiGdalDataType::Byte as i32, // Map to Byte
        RasterDataType::UInt16 => OxiGdalDataType::UInt16 as i32,
        RasterDataType::Int16 => OxiGdalDataType::Int16 as i32,
        RasterDataType::UInt32 => OxiGdalDataType::UInt32 as i32,
        RasterDataType::Int32 => OxiGdalDataType::Int32 as i32,
        RasterDataType::UInt64 => OxiGdalDataType::Float64 as i32, // Closest type
        RasterDataType::Int64 => OxiGdalDataType::Float64 as i32,  // Closest type
        RasterDataType::Float32 => OxiGdalDataType::Float32 as i32,
        RasterDataType::Float64 => OxiGdalDataType::Float64 as i32,
        RasterDataType::CFloat32 => OxiGdalDataType::Float32 as i32,
        RasterDataType::CFloat64 => OxiGdalDataType::Float64 as i32,
    }
}

/// Opens a raster dataset from a file path.
///
/// # Parameters
/// - `path`: Null-terminated UTF-8 path to the dataset
/// - `out_dataset`: Output pointer to receive the dataset handle
///
/// # Returns
/// - `Success` on success
/// - Error code on failure (use `oxigdal_get_last_error` for details)
///
/// # Safety
/// - `path` must be a valid null-terminated string
/// - `out_dataset` must be a valid pointer
/// - Caller must call `oxigdal_dataset_close` when done
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_dataset_open(
    path: *const c_char,
    out_dataset: *mut *mut OxiGdalDataset,
) -> OxiGdalErrorCode {
    check_null!(path, "path");
    check_null!(out_dataset, "out_dataset");

    // Convert C string to Rust string
    let path_cstr =
        ffi_result!(unsafe { CStr::from_ptr(path).to_str().map_err(|e| e.to_string()) });
    let path_string = path_cstr.to_string();

    // Validate path exists
    if !Path::new(&path_string).exists() {
        crate::ffi::error::set_last_error(format!("File not found: {}", path_string));
        return OxiGdalErrorCode::FileNotFound;
    }

    // Open the file using FileDataSource
    let source = match FileDataSource::open(&path_string) {
        Ok(s) => s,
        Err(e) => {
            crate::ffi::error::set_last_error(format!("Failed to open file: {}", e));
            return OxiGdalErrorCode::IoError;
        }
    };

    // Create GeoTiffReader
    let reader = match GeoTiffReader::open(source) {
        Ok(r) => r,
        Err(e) => {
            crate::ffi::error::set_last_error(format!("Failed to parse GeoTIFF: {}", e));
            return OxiGdalErrorCode::UnsupportedFormat;
        }
    };

    // Extract metadata from the reader
    let raster_metadata = reader.metadata();
    let geo_transform = reader.geo_transform();
    let epsg_code = reader.epsg_code().unwrap_or(0) as i32;

    let mut geotransform = [0.0f64; 6];
    if let Some(gt) = geo_transform {
        geotransform[0] = gt.origin_x;
        geotransform[1] = gt.pixel_width;
        geotransform[2] = gt.row_rotation;
        geotransform[3] = gt.origin_y;
        geotransform[4] = gt.col_rotation;
        geotransform[5] = gt.pixel_height;
    }

    let metadata = OxiGdalMetadata {
        width: raster_metadata.width as i32,
        height: raster_metadata.height as i32,
        band_count: raster_metadata.band_count as i32,
        data_type: core_data_type_to_ffi(raster_metadata.data_type),
        epsg_code,
        geotransform,
    };

    let handle = Box::new(DatasetHandle {
        path: path_string,
        metadata,
        reader: Some(Mutex::new(reader)),
        write_buffer: None,
    });

    unsafe {
        *out_dataset = Box::into_raw(handle) as *mut OxiGdalDataset;
    }
    OxiGdalErrorCode::Success
}

/// Closes a dataset and frees associated resources.
///
/// # Safety
/// - `dataset` must be a valid dataset handle from `oxigdal_dataset_open`
/// - Must not be used after this call
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_dataset_close(dataset: *mut OxiGdalDataset) -> OxiGdalErrorCode {
    check_null!(dataset, "dataset");

    // Convert back to Box and drop
    unsafe {
        drop(Box::from_raw(dataset as *mut DatasetHandle));
    }
    OxiGdalErrorCode::Success
}

/// Gets metadata for a dataset.
///
/// # Safety
/// - `dataset` must be a valid dataset handle
/// - `out_metadata` must be a valid pointer
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_dataset_get_metadata(
    dataset: *const OxiGdalDataset,
    out_metadata: *mut OxiGdalMetadata,
) -> OxiGdalErrorCode {
    check_null!(dataset, "dataset");
    check_null!(out_metadata, "out_metadata");

    unsafe {
        let handle = deref_ptr!(dataset, DatasetHandle, "dataset");
        *out_metadata = handle.metadata;
    }

    OxiGdalErrorCode::Success
}

/// Reads a rectangular region from a dataset into a buffer.
///
/// # Parameters
/// - `dataset`: Dataset handle
/// - `x_off`: X offset in pixels
/// - `y_off`: Y offset in pixels
/// - `x_size`: Width to read in pixels
/// - `y_size`: Height to read in pixels
/// - `band`: Band number (1-indexed)
/// - `buffer`: Output buffer (must be pre-allocated)
///
/// # Safety
/// - All pointers must be valid
/// - Buffer must have sufficient capacity: x_size * y_size * bytes_per_pixel
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_dataset_read_region(
    dataset: *const OxiGdalDataset,
    x_off: i32,
    y_off: i32,
    x_size: i32,
    y_size: i32,
    band: i32,
    buffer: *mut OxiGdalBuffer,
) -> OxiGdalErrorCode {
    check_null!(dataset, "dataset");
    check_null!(buffer, "buffer");

    if x_off < 0 || y_off < 0 || x_size <= 0 || y_size <= 0 {
        crate::ffi::error::set_last_error("Invalid region coordinates".to_string());
        return OxiGdalErrorCode::InvalidArgument;
    }

    if band < 1 {
        crate::ffi::error::set_last_error("Band index must be >= 1".to_string());
        return OxiGdalErrorCode::InvalidArgument;
    }

    let buf = deref_ptr_mut!(buffer, OxiGdalBuffer, "buffer");

    if buf.data.is_null() {
        crate::ffi::error::set_last_error("Buffer data is null".to_string());
        return OxiGdalErrorCode::NullPointer;
    }

    let handle = deref_ptr!(dataset, DatasetHandle, "dataset");

    // Validate bounds
    if x_off + x_size > handle.metadata.width || y_off + y_size > handle.metadata.height {
        crate::ffi::error::set_last_error("Region extends beyond dataset bounds".to_string());
        return OxiGdalErrorCode::OutOfBounds;
    }

    // Validate band
    if band > handle.metadata.band_count {
        crate::ffi::error::set_last_error(format!(
            "Band {} out of range (dataset has {} bands)",
            band, handle.metadata.band_count
        ));
        return OxiGdalErrorCode::InvalidArgument;
    }

    // Read data from the GeoTIFF reader
    let reader_mutex = match &handle.reader {
        Some(r) => r,
        None => {
            crate::ffi::error::set_last_error("Dataset not open for reading".to_string());
            return OxiGdalErrorCode::InvalidArgument;
        }
    };

    let reader = match reader_mutex.lock() {
        Ok(r) => r,
        Err(e) => {
            crate::ffi::error::set_last_error(format!("Failed to lock reader: {}", e));
            return OxiGdalErrorCode::Unknown;
        }
    };

    // Read the full band data (we'll need to extract the region)
    let band_data = match reader.read_band(0, (band - 1) as usize) {
        Ok(data) => data,
        Err(e) => {
            crate::ffi::error::set_last_error(format!("Failed to read band data: {}", e));
            return OxiGdalErrorCode::IoError;
        }
    };

    // Calculate bytes per pixel based on data type
    let bytes_per_pixel = match handle.metadata.data_type {
        0 => 1, // Byte
        1 => 2, // UInt16
        2 => 2, // Int16
        3 => 4, // UInt32
        4 => 4, // Int32
        5 => 4, // Float32
        6 => 8, // Float64
        _ => 1,
    };

    // Extract the region from the full band data
    let samples_per_pixel = handle.metadata.band_count as usize;
    let row_stride = handle.metadata.width as usize * bytes_per_pixel * samples_per_pixel;

    let expected_size = (x_size * y_size * buf.channels) as usize;
    if buf.length < expected_size {
        crate::ffi::error::set_last_error(format!(
            "Buffer too small: {} < {}",
            buf.length, expected_size
        ));
        return OxiGdalErrorCode::InvalidArgument;
    }

    // Copy the region data to the output buffer
    let dst_row_stride = x_size as usize * bytes_per_pixel * buf.channels as usize;

    for row in 0..y_size as usize {
        let src_row = y_off as usize + row;
        let src_offset =
            src_row * row_stride + x_off as usize * bytes_per_pixel * samples_per_pixel;

        // For band-interleaved data, we need to extract just the requested band
        // For now, copy all samples (channels) from the source
        let dst_offset = row * dst_row_stride;

        if src_offset + x_size as usize * bytes_per_pixel * samples_per_pixel <= band_data.len()
            && dst_offset + dst_row_stride <= buf.length
        {
            for col in 0..x_size as usize {
                let src_col_offset = src_offset + col * bytes_per_pixel * samples_per_pixel;
                let dst_col_offset = dst_offset + col * bytes_per_pixel * buf.channels as usize;

                // Copy data for each channel
                let channels_to_copy = samples_per_pixel.min(buf.channels as usize);
                for ch in 0..channels_to_copy {
                    let src_ch_offset = src_col_offset + ch * bytes_per_pixel;
                    let dst_ch_offset = dst_col_offset + ch * bytes_per_pixel;

                    if src_ch_offset + bytes_per_pixel <= band_data.len() {
                        unsafe {
                            std::ptr::copy_nonoverlapping(
                                band_data.as_ptr().add(src_ch_offset),
                                buf.data.add(dst_ch_offset),
                                bytes_per_pixel,
                            );
                        }
                    }
                }
            }
        }
    }

    buf.width = x_size;
    buf.height = y_size;

    OxiGdalErrorCode::Success
}

/// Reads a map tile in XYZ tile scheme.
///
/// # Parameters
/// - `dataset`: Dataset handle
/// - `tile_coord`: Tile coordinates (z/x/y)
/// - `tile_size`: Size of tile in pixels (typically 256 or 512)
/// - `out_tile`: Output tile handle
///
/// # Safety
/// - All pointers must be valid
/// - Caller must call `oxigdal_tile_free` when done
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_dataset_read_tile(
    dataset: *const OxiGdalDataset,
    tile_coord: *const OxiGdalTileCoord,
    tile_size: i32,
    out_tile: *mut *mut OxiGdalTile,
) -> OxiGdalErrorCode {
    check_null!(dataset, "dataset");
    check_null!(tile_coord, "tile_coord");
    check_null!(out_tile, "out_tile");

    if tile_size <= 0 || tile_size > 4096 {
        crate::ffi::error::set_last_error("Invalid tile size".to_string());
        return OxiGdalErrorCode::InvalidArgument;
    }

    let coord = unsafe { deref_ptr!(tile_coord, OxiGdalTileCoord, "tile_coord") };

    if coord.z < 0 || coord.x < 0 || coord.y < 0 {
        crate::ffi::error::set_last_error("Invalid tile coordinates".to_string());
        return OxiGdalErrorCode::InvalidArgument;
    }

    let handle = deref_ptr!(dataset, DatasetHandle, "dataset");

    let reader_mutex = match &handle.reader {
        Some(r) => r,
        None => {
            crate::ffi::error::set_last_error("Dataset not open for reading".to_string());
            return OxiGdalErrorCode::InvalidArgument;
        }
    };

    let reader = match reader_mutex.lock() {
        Ok(r) => r,
        Err(e) => {
            crate::ffi::error::set_last_error(format!("Failed to lock reader: {}", e));
            return OxiGdalErrorCode::Unknown;
        }
    };

    // Calculate the appropriate overview level based on zoom
    // Higher zoom = less overview, zoom 0 = full resolution
    let overview_level = if coord.z < reader.overview_count() as i32 {
        (reader.overview_count() as i32 - 1 - coord.z).max(0) as usize
    } else {
        0
    };

    // Read the tile from the reader
    let tile_data = match reader.read_tile(overview_level, coord.x as u32, coord.y as u32) {
        Ok(data) => data,
        Err(e) => {
            crate::ffi::error::set_last_error(format!("Failed to read tile: {}", e));
            return OxiGdalErrorCode::IoError;
        }
    };

    let channels = handle.metadata.band_count;

    let tile_handle = Box::new(TileHandle {
        data: tile_data,
        width: tile_size,
        height: tile_size,
        channels,
    });

    unsafe {
        *out_tile = Box::into_raw(tile_handle) as *mut OxiGdalTile;
    }

    OxiGdalErrorCode::Success
}

/// Frees a tile allocated by `oxigdal_dataset_read_tile`.
///
/// # Safety
/// - `tile` must be a valid tile handle from `oxigdal_dataset_read_tile`
/// - Must not be used after this call
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_tile_free(tile: *mut OxiGdalTile) -> OxiGdalErrorCode {
    if tile.is_null() {
        return OxiGdalErrorCode::Success;
    }

    unsafe {
        drop(Box::from_raw(tile as *mut TileHandle));
    }
    OxiGdalErrorCode::Success
}

/// Gets data from a tile.
///
/// # Safety
/// - `tile` must be a valid tile handle
/// - `out_buffer` must be a valid pointer
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_tile_get_data(
    tile: *const OxiGdalTile,
    out_buffer: *mut OxiGdalBuffer,
) -> OxiGdalErrorCode {
    check_null!(tile, "tile");
    check_null!(out_buffer, "out_buffer");

    let tile_handle = deref_ptr!(tile, TileHandle, "tile");

    unsafe {
        (*out_buffer).width = tile_handle.width;
        (*out_buffer).height = tile_handle.height;
        (*out_buffer).channels = tile_handle.channels;
        (*out_buffer).length = tile_handle.data.len();
        // Note: We're giving a reference to internal data - caller should not free
        (*out_buffer).data = tile_handle.data.as_ptr() as *mut u8;
    }

    OxiGdalErrorCode::Success
}

/// Computes statistics for a raster band.
///
/// # Safety
/// - All pointers must be valid
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_dataset_compute_stats(
    dataset: *const OxiGdalDataset,
    band: i32,
    approx_ok: i32,
    out_stats: *mut OxiGdalStats,
) -> OxiGdalErrorCode {
    check_null!(dataset, "dataset");
    check_null!(out_stats, "out_stats");

    if band < 1 {
        crate::ffi::error::set_last_error("Band index must be >= 1".to_string());
        return OxiGdalErrorCode::InvalidArgument;
    }

    let handle = deref_ptr!(dataset, DatasetHandle, "dataset");

    // Validate band index
    if band > handle.metadata.band_count {
        crate::ffi::error::set_last_error(format!(
            "Band {} out of range (dataset has {} bands)",
            band, handle.metadata.band_count
        ));
        return OxiGdalErrorCode::InvalidArgument;
    }

    let reader_mutex = match &handle.reader {
        Some(r) => r,
        None => {
            crate::ffi::error::set_last_error("Dataset not open for reading".to_string());
            return OxiGdalErrorCode::InvalidArgument;
        }
    };

    let reader = match reader_mutex.lock() {
        Ok(r) => r,
        Err(e) => {
            crate::ffi::error::set_last_error(format!("Failed to lock reader: {}", e));
            return OxiGdalErrorCode::Unknown;
        }
    };

    // For approximate stats, we might read from an overview level
    let level = if approx_ok != 0 && reader.overview_count() > 0 {
        reader.overview_count() - 1
    } else {
        0
    };

    // Read band data
    let band_data = match reader.read_band(level, (band - 1) as usize) {
        Ok(data) => data,
        Err(e) => {
            crate::ffi::error::set_last_error(format!("Failed to read band data: {}", e));
            return OxiGdalErrorCode::IoError;
        }
    };

    // Determine data type from metadata
    let data_type = match handle.metadata.data_type {
        0 => RasterDataType::UInt8,
        1 => RasterDataType::UInt16,
        2 => RasterDataType::Int16,
        3 => RasterDataType::UInt32,
        4 => RasterDataType::Int32,
        5 => RasterDataType::Float32,
        6 => RasterDataType::Float64,
        _ => RasterDataType::UInt8,
    };

    // Create a RasterBuffer to compute statistics
    let width = handle.metadata.width as u64;
    let height = handle.metadata.height as u64;

    // For overview levels, adjust dimensions
    let (actual_width, actual_height) = if level > 0 {
        let scale = 1u64 << level;
        ((width + scale - 1) / scale, (height + scale - 1) / scale)
    } else {
        (width, height)
    };

    let buffer = match RasterBuffer::new(
        band_data,
        actual_width,
        actual_height,
        data_type,
        oxigdal_core::types::NoDataValue::None,
    ) {
        Ok(b) => b,
        Err(e) => {
            crate::ffi::error::set_last_error(format!("Failed to create buffer: {}", e));
            return OxiGdalErrorCode::Unknown;
        }
    };

    // Compute statistics
    let stats = match buffer.compute_statistics() {
        Ok(s) => s,
        Err(e) => {
            crate::ffi::error::set_last_error(format!("Failed to compute statistics: {}", e));
            return OxiGdalErrorCode::Unknown;
        }
    };

    unsafe {
        *out_stats = OxiGdalStats {
            min: stats.min,
            max: stats.max,
            mean: stats.mean,
            stddev: stats.std_dev,
            valid_count: stats.valid_count,
        };
    }

    OxiGdalErrorCode::Success
}

/// Creates a new raster dataset.
///
/// # Parameters
/// - `path`: Output file path
/// - `width`: Width in pixels
/// - `height`: Height in pixels
/// - `bands`: Number of bands
/// - `data_type`: Data type code
/// - `out_dataset`: Output dataset handle
///
/// # Safety
/// - All pointers must be valid
/// - Caller must call `oxigdal_dataset_close` when done
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_dataset_create(
    path: *const c_char,
    width: i32,
    height: i32,
    bands: i32,
    data_type: OxiGdalDataType,
    out_dataset: *mut *mut OxiGdalDataset,
) -> OxiGdalErrorCode {
    check_null!(path, "path");
    check_null!(out_dataset, "out_dataset");

    if width <= 0 || height <= 0 || bands <= 0 {
        crate::ffi::error::set_last_error("Invalid dimensions".to_string());
        return OxiGdalErrorCode::InvalidArgument;
    }

    let path_cstr =
        ffi_result!(unsafe { CStr::from_ptr(path).to_str().map_err(|e| e.to_string()) });
    let path_string = path_cstr.to_string();

    let metadata = OxiGdalMetadata {
        width,
        height,
        band_count: bands,
        data_type: data_type as i32,
        epsg_code: 0,
        geotransform: [0.0; 6],
    };

    // Calculate buffer size per band
    let core_data_type = ffi_data_type_to_core(data_type);
    let bytes_per_pixel = core_data_type.size_bytes();
    let band_size = width as usize * height as usize * bytes_per_pixel;

    // Initialize band data buffers
    let mut band_data = Vec::with_capacity(bands as usize);
    for _ in 0..bands {
        band_data.push(vec![0u8; band_size]);
    }

    let write_buffer = WriteBufferData {
        width,
        height,
        bands,
        data_type,
        band_data,
        geotransform: [0.0; 6],
        epsg_code: 0,
    };

    let handle = Box::new(DatasetHandle {
        path: path_string,
        metadata,
        reader: None,
        write_buffer: Some(Mutex::new(write_buffer)),
    });

    unsafe {
        *out_dataset = Box::into_raw(handle) as *mut OxiGdalDataset;
    }
    OxiGdalErrorCode::Success
}

/// Writes data to a raster band.
///
/// # Safety
/// - All pointers must be valid
/// - Buffer must contain sufficient data
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_dataset_write_region(
    dataset: *mut OxiGdalDataset,
    x_off: i32,
    y_off: i32,
    x_size: i32,
    y_size: i32,
    band: i32,
    buffer: *const OxiGdalBuffer,
) -> OxiGdalErrorCode {
    check_null!(dataset, "dataset");
    check_null!(buffer, "buffer");

    if x_off < 0 || y_off < 0 || x_size <= 0 || y_size <= 0 {
        crate::ffi::error::set_last_error("Invalid region coordinates".to_string());
        return OxiGdalErrorCode::InvalidArgument;
    }

    if band < 1 {
        crate::ffi::error::set_last_error("Band index must be >= 1".to_string());
        return OxiGdalErrorCode::InvalidArgument;
    }

    let buf = deref_ptr!(buffer, OxiGdalBuffer, "buffer");

    if buf.data.is_null() {
        crate::ffi::error::set_last_error("Buffer data is null".to_string());
        return OxiGdalErrorCode::NullPointer;
    }

    let handle = deref_ptr_mut!(dataset, DatasetHandle, "dataset");

    // Get write buffer
    let write_buffer_mutex = match &handle.write_buffer {
        Some(wb) => wb,
        None => {
            crate::ffi::error::set_last_error("Dataset not open for writing".to_string());
            return OxiGdalErrorCode::InvalidArgument;
        }
    };

    let mut write_buffer = match write_buffer_mutex.lock() {
        Ok(wb) => wb,
        Err(e) => {
            crate::ffi::error::set_last_error(format!("Failed to lock write buffer: {}", e));
            return OxiGdalErrorCode::Unknown;
        }
    };

    // Validate bounds
    if x_off + x_size > write_buffer.width || y_off + y_size > write_buffer.height {
        crate::ffi::error::set_last_error("Region extends beyond dataset bounds".to_string());
        return OxiGdalErrorCode::OutOfBounds;
    }

    // Validate band
    if band > write_buffer.bands {
        crate::ffi::error::set_last_error(format!(
            "Band {} out of range (dataset has {} bands)",
            band, write_buffer.bands
        ));
        return OxiGdalErrorCode::InvalidArgument;
    }

    let band_idx = (band - 1) as usize;

    // Calculate bytes per pixel
    let core_data_type = ffi_data_type_to_core(write_buffer.data_type);
    let bytes_per_pixel = core_data_type.size_bytes();

    // Copy the buffer data to the band data
    let row_stride = write_buffer.width as usize * bytes_per_pixel;

    for row in 0..y_size as usize {
        let src_offset = row * x_size as usize * bytes_per_pixel;
        let dst_row = y_off as usize + row;
        let dst_offset = dst_row * row_stride + x_off as usize * bytes_per_pixel;
        let copy_len = x_size as usize * bytes_per_pixel;

        if src_offset + copy_len <= buf.length
            && dst_offset + copy_len <= write_buffer.band_data[band_idx].len()
        {
            unsafe {
                std::ptr::copy_nonoverlapping(
                    buf.data.add(src_offset),
                    write_buffer.band_data[band_idx]
                        .as_mut_ptr()
                        .add(dst_offset),
                    copy_len,
                );
            }
        }
    }

    OxiGdalErrorCode::Success
}

/// Flushes and writes the dataset to disk.
///
/// # Safety
/// - `dataset` must be a valid dataset handle
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_dataset_flush(dataset: *mut OxiGdalDataset) -> OxiGdalErrorCode {
    check_null!(dataset, "dataset");

    let handle = deref_ptr!(dataset, DatasetHandle, "dataset");

    let write_buffer_mutex = match &handle.write_buffer {
        Some(wb) => wb,
        None => {
            // Nothing to flush for read-only datasets
            return OxiGdalErrorCode::Success;
        }
    };

    let write_buffer = match write_buffer_mutex.lock() {
        Ok(wb) => wb,
        Err(e) => {
            crate::ffi::error::set_last_error(format!("Failed to lock write buffer: {}", e));
            return OxiGdalErrorCode::Unknown;
        }
    };

    // Use GeoTiffWriter to write the data
    use oxigdal_core::types::GeoTransform;
    use oxigdal_geotiff::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};

    let core_data_type = ffi_data_type_to_core(write_buffer.data_type);

    // Create geo_transform from array if any values are non-zero
    let geo_transform = if write_buffer.geotransform.iter().any(|&x| x != 0.0) {
        Some(GeoTransform::from_gdal_array(write_buffer.geotransform))
    } else {
        None
    };

    let mut config = WriterConfig::new(
        write_buffer.width as u64,
        write_buffer.height as u64,
        write_buffer.bands as u16,
        core_data_type,
    );

    config.tile_width = Some(256);
    config.tile_height = Some(256);
    config.geo_transform = geo_transform;
    config.epsg_code = if write_buffer.epsg_code > 0 {
        Some(write_buffer.epsg_code as u32)
    } else {
        None
    };

    let options = GeoTiffWriterOptions::default();

    let mut writer = match GeoTiffWriter::create(&handle.path, config, options) {
        Ok(w) => w,
        Err(e) => {
            crate::ffi::error::set_last_error(format!("Failed to create writer: {}", e));
            return OxiGdalErrorCode::IoError;
        }
    };

    // Combine all bands into a single interleaved buffer
    let bytes_per_pixel = core_data_type.size_bytes();
    let pixels = write_buffer.width as usize * write_buffer.height as usize;
    let mut combined_data = vec![0u8; pixels * bytes_per_pixel * write_buffer.bands as usize];

    // Interleave band data
    for pixel in 0..pixels {
        for band_idx in 0..write_buffer.bands as usize {
            let src_offset = pixel * bytes_per_pixel;
            let dst_offset =
                pixel * bytes_per_pixel * write_buffer.bands as usize + band_idx * bytes_per_pixel;

            if src_offset + bytes_per_pixel <= write_buffer.band_data[band_idx].len() {
                combined_data[dst_offset..dst_offset + bytes_per_pixel].copy_from_slice(
                    &write_buffer.band_data[band_idx][src_offset..src_offset + bytes_per_pixel],
                );
            }
        }
    }

    if let Err(e) = writer.write(&combined_data) {
        crate::ffi::error::set_last_error(format!("Failed to write data: {}", e));
        return OxiGdalErrorCode::IoError;
    }

    OxiGdalErrorCode::Success
}

/// Sets the geotransform for a dataset.
///
/// # Safety
/// - All pointers must be valid
/// - geotransform must point to an array of 6 doubles
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_dataset_set_geotransform(
    dataset: *mut OxiGdalDataset,
    geotransform: *const f64,
) -> OxiGdalErrorCode {
    check_null!(dataset, "dataset");
    check_null!(geotransform, "geotransform");

    unsafe {
        let handle = deref_ptr_mut!(dataset, DatasetHandle, "dataset");
        std::ptr::copy_nonoverlapping(geotransform, handle.metadata.geotransform.as_mut_ptr(), 6);

        // Also update write buffer if present
        if let Some(wb_mutex) = &handle.write_buffer {
            if let Ok(mut wb) = wb_mutex.lock() {
                std::ptr::copy_nonoverlapping(geotransform, wb.geotransform.as_mut_ptr(), 6);
            }
        }
    }

    OxiGdalErrorCode::Success
}

/// Sets the projection for a dataset using an EPSG code.
///
/// # Safety
/// - dataset must be a valid handle
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_dataset_set_projection_epsg(
    dataset: *mut OxiGdalDataset,
    epsg_code: i32,
) -> OxiGdalErrorCode {
    check_null!(dataset, "dataset");

    if epsg_code <= 0 {
        crate::ffi::error::set_last_error("Invalid EPSG code".to_string());
        return OxiGdalErrorCode::InvalidArgument;
    }

    unsafe {
        let handle = deref_ptr_mut!(dataset, DatasetHandle, "dataset");
        handle.metadata.epsg_code = epsg_code;

        // Also update write buffer if present
        if let Some(wb_mutex) = &handle.write_buffer {
            if let Ok(mut wb) = wb_mutex.lock() {
                wb.epsg_code = epsg_code;
            }
        }
    }

    OxiGdalErrorCode::Success
}

/// Gets the version of OxiGDAL.
///
/// # Safety
/// - out_version must be a valid pointer
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_get_version(out_version: *mut OxiGdalVersion) -> OxiGdalErrorCode {
    check_null!(out_version, "out_version");

    let version_str = env!("CARGO_PKG_VERSION");
    let mut parts = version_str.splitn(3, '.');
    let major = parts
        .next()
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(0);
    let minor = parts
        .next()
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(0);
    let patch = parts
        .next()
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(0);

    unsafe {
        *out_version = OxiGdalVersion {
            major,
            minor,
            patch,
        };
    }

    OxiGdalErrorCode::Success
}

/// Performs histogram equalization on an image buffer.
///
/// Histogram equalization redistributes pixel intensities to enhance contrast.
/// Works on grayscale and RGB images (per-channel for RGB).
///
/// # Parameters
/// - `buffer`: Image buffer to equalize (modified in-place)
///
/// # Returns
/// - `Success` on success
/// - Error code on failure
///
/// # Safety
/// - `buffer` must be a valid pointer to OxiGdalBuffer
/// - Buffer data must be mutable
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_buffer_histogram_equalize(
    buffer: *mut OxiGdalBuffer,
) -> OxiGdalErrorCode {
    check_null!(buffer, "buffer");

    let buf = deref_ptr_mut!(buffer, OxiGdalBuffer, "buffer");

    if buf.data.is_null() || buf.length == 0 {
        crate::ffi::error::set_last_error("Invalid buffer".to_string());
        return OxiGdalErrorCode::InvalidArgument;
    }

    if buf.width <= 0 || buf.height <= 0 || buf.channels <= 0 {
        crate::ffi::error::set_last_error("Invalid buffer dimensions".to_string());
        return OxiGdalErrorCode::InvalidArgument;
    }

    let pixel_count = (buf.width * buf.height) as usize;
    let data_slice = unsafe { std::slice::from_raw_parts_mut(buf.data, buf.length) };

    // Perform histogram equalization per channel
    for ch in 0..buf.channels as usize {
        // Step 1: Compute histogram (256 bins for 8-bit data)
        let mut histogram = [0u32; 256];

        for i in 0..pixel_count {
            let offset = i * buf.channels as usize + ch;
            if offset < data_slice.len() {
                let pixel_value = data_slice[offset] as usize;
                histogram[pixel_value] += 1;
            }
        }

        // Step 2: Compute cumulative distribution function (CDF)
        let mut cdf = [0u32; 256];
        cdf[0] = histogram[0];
        for i in 1..256 {
            cdf[i] = cdf[i - 1] + histogram[i];
        }

        // Find minimum non-zero CDF value
        let cdf_min = match cdf.iter().find(|&&v| v > 0) {
            Some(&v) => v,
            None => {
                // All pixels are zero, nothing to equalize
                continue;
            }
        };

        // Step 3: Normalize and create lookup table
        let mut lut = [0u8; 256];
        let range = (pixel_count as u32).saturating_sub(cdf_min);

        if range == 0 {
            // All pixels have the same value
            continue;
        }

        for i in 0..256 {
            if cdf[i] > 0 {
                let normalized = ((cdf[i] - cdf_min) as f64 / range as f64 * 255.0).round();
                lut[i] = normalized.clamp(0.0, 255.0) as u8;
            }
        }

        // Step 4: Apply equalization using lookup table
        for i in 0..pixel_count {
            let offset = i * buf.channels as usize + ch;
            if offset < data_slice.len() {
                let pixel_value = data_slice[offset] as usize;
                data_slice[offset] = lut[pixel_value];
            }
        }
    }

    OxiGdalErrorCode::Success
}

/// Performs color balance adjustment on an image buffer.
///
/// Multiplies each RGB channel by its corresponding factor to adjust color balance.
///
/// # Parameters
/// - `buffer`: Image buffer to adjust (modified in-place)
/// - `red_factor`: Red channel multiplier (1.0 = no change, range [0.0, 2.0])
/// - `green_factor`: Green channel multiplier (1.0 = no change, range [0.0, 2.0])
/// - `blue_factor`: Blue channel multiplier (1.0 = no change, range [0.0, 2.0])
///
/// # Returns
/// - `Success` on success
/// - Error code on failure
///
/// # Safety
/// - `buffer` must be a valid pointer to OxiGdalBuffer
/// - Buffer data must be mutable
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_buffer_color_balance(
    buffer: *mut OxiGdalBuffer,
    red_factor: c_double,
    green_factor: c_double,
    blue_factor: c_double,
) -> OxiGdalErrorCode {
    check_null!(buffer, "buffer");

    // Validate factors
    if !(0.0..=2.0).contains(&red_factor)
        || !(0.0..=2.0).contains(&green_factor)
        || !(0.0..=2.0).contains(&blue_factor)
    {
        crate::ffi::error::set_last_error(
            "Color balance factors must be in range [0.0, 2.0]".to_string(),
        );
        return OxiGdalErrorCode::InvalidArgument;
    }

    let buf = deref_ptr_mut!(buffer, OxiGdalBuffer, "buffer");

    if buf.data.is_null() || buf.length == 0 {
        crate::ffi::error::set_last_error("Invalid buffer".to_string());
        return OxiGdalErrorCode::InvalidArgument;
    }

    if buf.width <= 0 || buf.height <= 0 || buf.channels <= 0 {
        crate::ffi::error::set_last_error("Invalid buffer dimensions".to_string());
        return OxiGdalErrorCode::InvalidArgument;
    }

    let pixel_count = (buf.width * buf.height) as usize;
    let data_slice = unsafe { std::slice::from_raw_parts_mut(buf.data, buf.length) };

    if buf.channels == 1 {
        // Grayscale: apply average of factors
        let avg_factor = (red_factor + green_factor + blue_factor) / 3.0;
        for i in 0..pixel_count {
            if i < data_slice.len() {
                let value = data_slice[i] as f64 * avg_factor;
                data_slice[i] = value.clamp(0.0, 255.0) as u8;
            }
        }
    } else if buf.channels >= 3 {
        // RGB or RGBA: apply per-channel factors
        let factors = [red_factor, green_factor, blue_factor];

        for i in 0..pixel_count {
            for (ch, &factor) in factors
                .iter()
                .enumerate()
                .take(3.min(buf.channels as usize))
            {
                let offset = i * buf.channels as usize + ch;
                if offset < data_slice.len() {
                    let value = data_slice[offset] as f64 * factor;
                    data_slice[offset] = value.clamp(0.0, 255.0) as u8;
                }
            }
            // Leave alpha channel unchanged if present
        }
    }

    OxiGdalErrorCode::Success
}

/// Gets the version as a string.
///
/// # Returns
/// Pointer to version string (caller must free with oxigdal_string_free)
#[unsafe(no_mangle)]
pub extern "C" fn oxigdal_get_version_string() -> *mut c_char {
    match CString::new(env!("CARGO_PKG_VERSION")) {
        Ok(s) => s.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::oxigdal_string_free;

    #[test]
    fn test_version() {
        let mut version = OxiGdalVersion {
            major: 0,
            minor: 0,
            patch: 0,
        };

        unsafe {
            let result = oxigdal_get_version(&mut version);
            assert_eq!(result, OxiGdalErrorCode::Success);
            let version_str = env!("CARGO_PKG_VERSION");
            let mut parts = version_str.splitn(3, '.');
            let expected_major = parts
                .next()
                .and_then(|s| s.parse::<i32>().ok())
                .unwrap_or(0);
            let expected_minor = parts
                .next()
                .and_then(|s| s.parse::<i32>().ok())
                .unwrap_or(0);
            let expected_patch = parts
                .next()
                .and_then(|s| s.parse::<i32>().ok())
                .unwrap_or(0);
            assert_eq!(version.major, expected_major);
            assert_eq!(version.minor, expected_minor);
            assert_eq!(version.patch, expected_patch);
        }
    }

    #[test]
    fn test_version_string() {
        let version_ptr = oxigdal_get_version_string();
        assert!(!version_ptr.is_null());

        unsafe {
            let version_cstr = CStr::from_ptr(version_ptr);
            let version_str = version_cstr.to_str().expect("valid UTF-8");
            assert_eq!(version_str, env!("CARGO_PKG_VERSION"));
            oxigdal_string_free(version_ptr);
        }
    }

    #[test]
    fn test_data_type_conversion() {
        assert_eq!(
            ffi_data_type_to_core(OxiGdalDataType::Byte),
            RasterDataType::UInt8
        );
        assert_eq!(
            ffi_data_type_to_core(OxiGdalDataType::Float32),
            RasterDataType::Float32
        );
        assert_eq!(
            ffi_data_type_to_core(OxiGdalDataType::Float64),
            RasterDataType::Float64
        );
    }

    #[test]
    fn test_core_data_type_to_ffi() {
        assert_eq!(
            core_data_type_to_ffi(RasterDataType::UInt8),
            OxiGdalDataType::Byte as i32
        );
        assert_eq!(
            core_data_type_to_ffi(RasterDataType::Float32),
            OxiGdalDataType::Float32 as i32
        );
    }

    #[test]
    fn test_create_dataset() {
        use std::ffi::CString;

        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join("test_create_dataset.tif");
        let path_cstring =
            CString::new(temp_path.to_str().expect("valid path")).expect("valid cstring");

        let mut dataset_ptr: *mut OxiGdalDataset = ptr::null_mut();

        unsafe {
            let result = oxigdal_dataset_create(
                path_cstring.as_ptr(),
                100,
                100,
                3,
                OxiGdalDataType::Byte,
                &mut dataset_ptr,
            );
            assert_eq!(result, OxiGdalErrorCode::Success);
            assert!(!dataset_ptr.is_null());

            // Get metadata
            let mut metadata = OxiGdalMetadata {
                width: 0,
                height: 0,
                band_count: 0,
                data_type: 0,
                epsg_code: 0,
                geotransform: [0.0; 6],
            };

            let result = oxigdal_dataset_get_metadata(dataset_ptr, &mut metadata);
            assert_eq!(result, OxiGdalErrorCode::Success);
            assert_eq!(metadata.width, 100);
            assert_eq!(metadata.height, 100);
            assert_eq!(metadata.band_count, 3);

            // Close dataset
            let result = oxigdal_dataset_close(dataset_ptr);
            assert_eq!(result, OxiGdalErrorCode::Success);
        }
    }

    #[test]
    fn test_null_pointer_checks() {
        unsafe {
            // Test null path
            let mut dataset_ptr: *mut OxiGdalDataset = ptr::null_mut();
            let result = oxigdal_dataset_open(ptr::null(), &mut dataset_ptr);
            assert_eq!(result, OxiGdalErrorCode::NullPointer);

            // Test null out_dataset
            let path = CString::new("/nonexistent/path.tif").expect("valid cstring");
            let result = oxigdal_dataset_open(path.as_ptr(), ptr::null_mut());
            assert_eq!(result, OxiGdalErrorCode::NullPointer);

            // Test null dataset for close
            let result = oxigdal_dataset_close(ptr::null_mut());
            assert_eq!(result, OxiGdalErrorCode::NullPointer);
        }
    }

    #[test]
    fn test_file_not_found() {
        use std::ffi::CString;

        let path = CString::new("/nonexistent/path/to/file.tif").expect("valid cstring");
        let mut dataset_ptr: *mut OxiGdalDataset = ptr::null_mut();

        unsafe {
            let result = oxigdal_dataset_open(path.as_ptr(), &mut dataset_ptr);
            assert_eq!(result, OxiGdalErrorCode::FileNotFound);
        }
    }

    #[test]
    fn test_invalid_region_coords() {
        use std::ffi::CString;

        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join("test_invalid_region.tif");
        let path_cstring =
            CString::new(temp_path.to_str().expect("valid path")).expect("valid cstring");

        let mut dataset_ptr: *mut OxiGdalDataset = ptr::null_mut();

        unsafe {
            let result = oxigdal_dataset_create(
                path_cstring.as_ptr(),
                100,
                100,
                1,
                OxiGdalDataType::Byte,
                &mut dataset_ptr,
            );
            assert_eq!(result, OxiGdalErrorCode::Success);

            // Create a buffer
            let mut buffer_data = vec![0u8; 100];
            let mut buffer = OxiGdalBuffer {
                data: buffer_data.as_mut_ptr(),
                length: 100,
                width: 10,
                height: 10,
                channels: 1,
            };

            // Test negative coordinates
            let result = oxigdal_dataset_read_region(dataset_ptr, -1, 0, 10, 10, 1, &mut buffer);
            assert_eq!(result, OxiGdalErrorCode::InvalidArgument);

            // Test zero size
            let result = oxigdal_dataset_read_region(dataset_ptr, 0, 0, 0, 10, 1, &mut buffer);
            assert_eq!(result, OxiGdalErrorCode::InvalidArgument);

            // Test invalid band
            let result = oxigdal_dataset_read_region(dataset_ptr, 0, 0, 10, 10, 0, &mut buffer);
            assert_eq!(result, OxiGdalErrorCode::InvalidArgument);

            let result = oxigdal_dataset_close(dataset_ptr);
            assert_eq!(result, OxiGdalErrorCode::Success);
        }
    }

    #[test]
    fn test_histogram_equalize_grayscale() {
        // Create a test grayscale image with varying intensities
        let mut data = vec![
            0u8, 50, 100, 150, 200, 255, 0, 50, 100, 150, 200, 255, 0, 50, 100, 150, 200, 255,
        ];
        let mut buffer = OxiGdalBuffer {
            data: data.as_mut_ptr(),
            length: data.len(),
            width: 6,
            height: 3,
            channels: 1,
        };

        unsafe {
            let result = oxigdal_buffer_histogram_equalize(&mut buffer);
            assert_eq!(result, OxiGdalErrorCode::Success);

            // Verify data was modified (histogram equalization should spread values)
            // We don't check exact values as they depend on the algorithm,
            // but we verify it runs successfully
        }
    }

    #[test]
    fn test_histogram_equalize_rgb() {
        // Create a test RGB image
        let mut data = vec![
            255u8, 0, 0, 0, 255, 0, 0, 0, 255, 128, 128, 128, 64, 64, 64, 192, 192, 192,
        ];
        let mut buffer = OxiGdalBuffer {
            data: data.as_mut_ptr(),
            length: data.len(),
            width: 3,
            height: 2,
            channels: 3,
        };

        unsafe {
            let result = oxigdal_buffer_histogram_equalize(&mut buffer);
            assert_eq!(result, OxiGdalErrorCode::Success);
        }
    }

    #[test]
    fn test_histogram_equalize_null_buffer() {
        unsafe {
            let result = oxigdal_buffer_histogram_equalize(ptr::null_mut());
            assert_eq!(result, OxiGdalErrorCode::NullPointer);
        }
    }

    #[test]
    fn test_histogram_equalize_invalid_buffer() {
        let mut buffer = OxiGdalBuffer {
            data: ptr::null_mut(),
            length: 0,
            width: 10,
            height: 10,
            channels: 1,
        };

        unsafe {
            let result = oxigdal_buffer_histogram_equalize(&mut buffer);
            assert_eq!(result, OxiGdalErrorCode::InvalidArgument);
        }
    }

    #[test]
    fn test_color_balance_rgb() {
        // Create a test RGB image
        let mut data = vec![
            128u8, 128, 128, // Gray pixel
            255, 0, 0, // Red pixel
            0, 255, 0, // Green pixel
            0, 0, 255, // Blue pixel
        ];
        let mut buffer = OxiGdalBuffer {
            data: data.as_mut_ptr(),
            length: data.len(),
            width: 4,
            height: 1,
            channels: 3,
        };

        unsafe {
            // Apply color balance: boost red, reduce green, normal blue
            let result = oxigdal_buffer_color_balance(&mut buffer, 1.5, 0.5, 1.0);
            assert_eq!(result, OxiGdalErrorCode::Success);

            // Check that red channel was boosted
            assert!(data[3] > 255 / 2); // Red pixel's red channel should be high
            // Green channel should be reduced
            assert!(data[7] < 200); // Green pixel's green channel should be reduced
        }
    }

    #[test]
    fn test_color_balance_grayscale() {
        let mut data = vec![100u8, 150, 200];
        let mut buffer = OxiGdalBuffer {
            data: data.as_mut_ptr(),
            length: data.len(),
            width: 3,
            height: 1,
            channels: 1,
        };

        unsafe {
            // For grayscale, average of factors should be applied
            let result = oxigdal_buffer_color_balance(&mut buffer, 1.2, 1.0, 0.8);
            assert_eq!(result, OxiGdalErrorCode::Success);

            // Values should be modified by average factor (1.2 + 1.0 + 0.8) / 3 = 1.0
            // So they should be approximately the same
        }
    }

    #[test]
    fn test_color_balance_null_buffer() {
        unsafe {
            let result = oxigdal_buffer_color_balance(ptr::null_mut(), 1.0, 1.0, 1.0);
            assert_eq!(result, OxiGdalErrorCode::NullPointer);
        }
    }

    #[test]
    fn test_color_balance_invalid_factors() {
        let mut data = vec![128u8; 12];
        let mut buffer = OxiGdalBuffer {
            data: data.as_mut_ptr(),
            length: data.len(),
            width: 2,
            height: 2,
            channels: 3,
        };

        unsafe {
            // Test negative factor
            let result = oxigdal_buffer_color_balance(&mut buffer, -0.5, 1.0, 1.0);
            assert_eq!(result, OxiGdalErrorCode::InvalidArgument);

            // Test factor > 2.0
            let result = oxigdal_buffer_color_balance(&mut buffer, 2.5, 1.0, 1.0);
            assert_eq!(result, OxiGdalErrorCode::InvalidArgument);
        }
    }

    #[test]
    fn test_color_balance_clamping() {
        // Test that values are properly clamped to [0, 255]
        let mut data = vec![200u8, 200, 200];
        let mut buffer = OxiGdalBuffer {
            data: data.as_mut_ptr(),
            length: data.len(),
            width: 1,
            height: 1,
            channels: 3,
        };

        unsafe {
            // Apply high factors that would overflow
            let result = oxigdal_buffer_color_balance(&mut buffer, 2.0, 2.0, 2.0);
            assert_eq!(result, OxiGdalErrorCode::Success);

            // Values should be clamped to 255
            assert_eq!(data[0], 255);
            assert_eq!(data[1], 255);
            assert_eq!(data[2], 255);
        }
    }
}
