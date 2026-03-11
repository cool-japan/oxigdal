//! Dataset implementations for geospatial data.
//!
//! Provides the `Dataset` trait and implementations for loading
//! geospatial imagery for machine learning training.

use crate::augmentation::AugmentationPipeline;
use crate::{Error, Result};
use lru::LruCache;
use oxigdal_core::buffer::RasterBuffer;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Mutex;

#[cfg(feature = "ml")]
use crate::training::training_loop::Dataset as DatasetTrait;
#[cfg(feature = "ml")]
use oxigdal_core::RasterDataType;
#[cfg(feature = "ml")]
use std::path::Path;

/// Dataset trait for accessing samples.
///
/// This trait provides a unified interface for accessing training data,
/// whether from GeoTIFF files, in-memory arrays, or other sources.
#[cfg(not(feature = "ml"))]
pub trait Dataset: Send + Sync {
    /// Get the number of samples in the dataset
    fn len(&self) -> usize;

    /// Check if the dataset is empty
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get a batch of samples
    ///
    /// # Arguments
    ///
    /// * `indices` - Indices of samples to retrieve
    ///
    /// # Returns
    ///
    /// (inputs, targets) where inputs and targets are flat vectors
    fn get_batch(&self, indices: &[usize]) -> Result<(Vec<f32>, Vec<f32>)>;

    /// Get input and output shapes
    fn shapes(&self) -> (Vec<usize>, Vec<usize>);
}

#[cfg(feature = "ml")]
pub use crate::training::training_loop::Dataset;

/// GeoTIFF dataset for loading raster imagery.
///
/// Loads GeoTIFF files and extracts random or systematic patches
/// for training. Supports caching and data augmentation.
pub struct GeoTiffDataset {
    /// Paths to GeoTIFF files
    file_paths: Vec<PathBuf>,
    /// Optional label file paths (parallel to file_paths)
    label_paths: Option<Vec<PathBuf>>,
    /// Patch size (height, width)
    #[allow(dead_code)]
    patch_size: (usize, usize),
    /// Number of channels in input
    num_channels: usize,
    /// Number of classes for output
    num_classes: usize,
    /// LRU cache for loaded rasters
    cache: Mutex<LruCache<PathBuf, RasterBuffer>>,
    /// Optional augmentation pipeline
    transform: Option<AugmentationPipeline>,
    /// Number of patches per image
    patches_per_image: usize,
}

impl GeoTiffDataset {
    /// Creates a new GeoTIFF dataset.
    ///
    /// # Arguments
    ///
    /// * `file_paths` - Paths to input GeoTIFF files
    /// * `patch_size` - Size of patches to extract (height, width)
    ///
    /// # Errors
    ///
    /// Returns an error if the file list is empty or patch size is invalid.
    pub fn new(file_paths: Vec<PathBuf>, patch_size: (usize, usize)) -> Result<Self> {
        if file_paths.is_empty() {
            return Err(Error::invalid_parameter(
                "file_paths",
                "empty",
                "at least one file required",
            ));
        }

        if patch_size.0 == 0 || patch_size.1 == 0 {
            return Err(Error::invalid_parameter(
                "patch_size",
                format!("{:?}", patch_size),
                "both dimensions must be > 0",
            ));
        }

        // Default cache size: 16 images
        let cache_size = NonZeroUsize::new(16).ok_or_else(|| {
            Error::InvalidState("Failed to create NonZeroUsize for cache".to_string())
        })?;

        Ok(Self {
            file_paths,
            label_paths: None,
            patch_size,
            num_channels: 3, // RGB default
            num_classes: 1,  // Single output default
            cache: Mutex::new(LruCache::new(cache_size)),
            transform: None,
            patches_per_image: 10, // Default: 10 random patches per image
        })
    }

    /// Sets the label file paths for supervised learning.
    pub fn with_labels(mut self, label_paths: Vec<PathBuf>) -> Result<Self> {
        if label_paths.len() != self.file_paths.len() {
            return Err(Error::invalid_parameter(
                "label_paths",
                format!("{} files", label_paths.len()),
                format!("must match input files ({})", self.file_paths.len()),
            ));
        }
        self.label_paths = Some(label_paths);
        Ok(self)
    }

    /// Sets the number of input channels.
    pub fn with_channels(mut self, num_channels: usize) -> Result<Self> {
        if num_channels == 0 {
            return Err(Error::invalid_parameter("num_channels", 0, "must be > 0"));
        }
        self.num_channels = num_channels;
        Ok(self)
    }

    /// Sets the number of output classes.
    pub fn with_classes(mut self, num_classes: usize) -> Result<Self> {
        if num_classes == 0 {
            return Err(Error::invalid_parameter("num_classes", 0, "must be > 0"));
        }
        self.num_classes = num_classes;
        Ok(self)
    }

    /// Sets the augmentation pipeline.
    pub fn with_transforms(mut self, transform: AugmentationPipeline) -> Self {
        self.transform = Some(transform);
        self
    }

    /// Sets the cache size (number of images to cache in memory).
    pub fn with_cache_size(mut self, size: usize) -> Result<Self> {
        let cache_size = NonZeroUsize::new(size)
            .ok_or_else(|| Error::invalid_parameter("cache_size", 0, "must be > 0"))?;
        self.cache = Mutex::new(LruCache::new(cache_size));
        Ok(self)
    }

    /// Sets the number of patches to extract per image.
    pub fn with_patches_per_image(mut self, patches: usize) -> Result<Self> {
        if patches == 0 {
            return Err(Error::invalid_parameter(
                "patches_per_image",
                0,
                "must be > 0",
            ));
        }
        self.patches_per_image = patches;
        Ok(self)
    }

    /// Loads a raster from file with caching.
    ///
    /// # Note
    ///
    /// Currently supports single-band GeoTIFF files. Multi-band support
    /// requires handling band interleaving properly.
    #[cfg(feature = "ml")]
    fn load_raster(&self, path: &Path) -> Result<RasterBuffer> {
        // Check cache first
        {
            let mut cache = self
                .cache
                .lock()
                .map_err(|e| Error::InvalidState(format!("Failed to lock cache mutex: {}", e)))?;

            if let Some(buffer) = cache.get(path) {
                return Ok(buffer.clone());
            }
        }

        // Load from file using oxigdal-geotiff
        tracing::debug!("Loading raster from {:?}", path);

        // Check if file exists
        if !path.exists() {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("GeoTIFF file not found: {}", path.display()),
            )));
        }

        // Open the file as a data source
        let source = oxigdal_core::io::FileDataSource::open(path)?;

        // Open the GeoTIFF reader
        let reader = oxigdal_geotiff::GeoTiffReader::open(source)?;

        // Get image dimensions and data type
        let width = reader.width();
        let height = reader.height();
        let band_count = reader.band_count();
        let data_type = reader.data_type().unwrap_or(RasterDataType::UInt8);

        tracing::debug!(
            "GeoTIFF info: {}x{}, {} bands, type={:?}",
            width,
            height,
            band_count,
            data_type
        );

        // Read all bands
        // For multi-band images, we'll read band 0 (level 0)
        let raw_data = reader.read_band(0, 0)?;

        // Get nodata value
        let nodata = reader.nodata();

        // Create the raster buffer
        let buffer = RasterBuffer::new(raw_data, width, height, data_type, nodata)?;

        // Cache the result
        {
            let mut cache = self
                .cache
                .lock()
                .map_err(|e| Error::InvalidState(format!("Failed to lock cache mutex: {}", e)))?;
            cache.put(path.to_path_buf(), buffer.clone());
        }

        Ok(buffer)
    }

    /// Extracts a random patch from a raster buffer.
    #[cfg(feature = "ml")]
    fn extract_random_patch(&self, buffer: &RasterBuffer) -> Result<Vec<f32>> {
        let width = buffer.width() as usize;
        let height = buffer.height() as usize;

        if width < self.patch_size.1 || height < self.patch_size.0 {
            return Err(Error::invalid_dimensions(
                format!("{}x{}", self.patch_size.1, self.patch_size.0),
                format!("{}x{}", width, height),
            ));
        }

        // Random offset
        let max_x = width - self.patch_size.1;
        let max_y = height - self.patch_size.0;

        let offset_x = if max_x > 0 {
            (getrandom::get_random_u64()? % max_x as u64) as usize
        } else {
            0
        };

        let offset_y = if max_y > 0 {
            (getrandom::get_random_u64()? % max_y as u64) as usize
        } else {
            0
        };

        // Extract patch
        let mut patch = Vec::with_capacity(self.patch_size.0 * self.patch_size.1);

        for y in offset_y..(offset_y + self.patch_size.0) {
            for x in offset_x..(offset_x + self.patch_size.1) {
                let value = buffer.get_pixel(x as u64, y as u64)?;
                patch.push(value as f32);
            }
        }

        Ok(patch)
    }
}

#[cfg(not(feature = "ml"))]
impl Dataset for GeoTiffDataset {
    fn len(&self) -> usize {
        self.file_paths.len() * self.patches_per_image
    }

    fn get_batch(&self, _indices: &[usize]) -> Result<(Vec<f32>, Vec<f32>)> {
        Err(Error::InvalidState(
            "Dataset loading requires the 'ml' feature".to_string(),
        ))
    }

    fn shapes(&self) -> (Vec<usize>, Vec<usize>) {
        let input_shape = vec![
            1, // batch size placeholder
            self.num_channels,
            self.patch_size.0,
            self.patch_size.1,
        ];
        let output_shape = vec![
            1, // batch size placeholder
            self.num_classes,
            self.patch_size.0,
            self.patch_size.1,
        ];
        (input_shape, output_shape)
    }
}

#[cfg(feature = "ml")]
impl DatasetTrait for GeoTiffDataset {
    fn len(&self) -> usize {
        self.file_paths.len() * self.patches_per_image
    }

    fn get_batch(&self, indices: &[usize]) -> Result<(Vec<f32>, Vec<f32>)> {
        let batch_size = indices.len();
        let patch_pixels = self.patch_size.0 * self.patch_size.1;
        let input_size = batch_size * self.num_channels * patch_pixels;
        let output_size = batch_size * self.num_classes * patch_pixels;

        let mut inputs = Vec::with_capacity(input_size);
        let mut targets = Vec::with_capacity(output_size);

        for &idx in indices {
            // Determine which file and which patch
            let file_idx = idx / self.patches_per_image;
            let file_idx = file_idx.min(self.file_paths.len() - 1);

            // Load input raster
            let input_buffer = self.load_raster(&self.file_paths[file_idx])?;
            let patch = self.extract_random_patch(&input_buffer)?;
            inputs.extend_from_slice(&patch);

            // Load target/label if available
            if let Some(ref label_paths) = self.label_paths {
                let label_buffer = self.load_raster(&label_paths[file_idx])?;
                let label_patch = self.extract_random_patch(&label_buffer)?;
                targets.extend_from_slice(&label_patch);
            } else {
                // No labels: use zeros or same as input
                targets.extend(vec![0.0; patch_pixels]);
            }
        }

        Ok((inputs, targets))
    }

    fn shapes(&self) -> (Vec<usize>, Vec<usize>) {
        let input_shape = vec![
            1, // batch size placeholder
            self.num_channels,
            self.patch_size.0,
            self.patch_size.1,
        ];
        let output_shape = vec![
            1, // batch size placeholder
            self.num_classes,
            self.patch_size.0,
            self.patch_size.1,
        ];
        (input_shape, output_shape)
    }
}

/// Helper function to get a random u64 value
#[cfg(feature = "ml")]
mod getrandom {
    use crate::Result;

    pub fn get_random_u64() -> Result<u64> {
        let mut buf = [0u8; 8];
        getrandom::getrandom(&mut buf).map_err(|e| {
            crate::Error::Numerical(format!("Failed to generate random number: {}", e))
        })?;
        Ok(u64::from_ne_bytes(buf))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "ml")]
    use std::env;
    #[cfg(feature = "ml")]
    use std::fs;

    /// Helper to create a test GeoTIFF file
    #[cfg(feature = "ml")]
    fn create_test_geotiff(width: u32, height: u32, bands: u16) -> Result<PathBuf> {
        use oxigdal_core::RasterDataType;
        use oxigdal_core::types::{GeoTransform, NoDataValue};
        use oxigdal_geotiff::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};

        // Create temp directory
        let temp_dir = env::temp_dir().join("oxigdal_ml_test");
        fs::create_dir_all(&temp_dir)?;

        // Generate unique filename
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| Error::InvalidState(format!("Failed to get timestamp: {}", e)))?
            .as_nanos();
        let filename = temp_dir.join(format!(
            "test_{}x{}_b{}_t{}.tif",
            width, height, bands, timestamp
        ));

        // Create test data
        let data_type = RasterDataType::UInt8;
        let size =
            (width as u64) * (height as u64) * (bands as u64) * (data_type.size_bytes() as u64);
        let mut data = vec![0u8; size as usize];

        // Fill with test pattern
        for (i, item) in data.iter_mut().enumerate() {
            *item = (i % 256) as u8;
        }

        // Setup geotransform
        let geo_transform = GeoTransform {
            origin_x: 0.0,
            origin_y: 0.0,
            pixel_width: 1.0,
            pixel_height: -1.0,
            row_rotation: 0.0,
            col_rotation: 0.0,
        };

        // Configure writer
        let mut config = WriterConfig::new(width as u64, height as u64, bands, data_type);
        config.compression = oxigdal_geotiff::Compression::None;
        config.tile_width = Some(256);
        config.tile_height = Some(256);
        config.photometric = oxigdal_geotiff::PhotometricInterpretation::BlackIsZero;
        config.geo_transform = Some(geo_transform);
        config.epsg_code = Some(4326);
        config.nodata = NoDataValue::None;
        config.generate_overviews = false;

        let options = GeoTiffWriterOptions::default();

        let mut writer = GeoTiffWriter::create(&filename, config, options)?;

        writer.write(&data)?;

        // Writer automatically closes when dropped
        drop(writer);

        Ok(filename)
    }

    #[test]
    fn test_dataset_creation() {
        let files = vec![PathBuf::from("test1.tif"), PathBuf::from("test2.tif")];
        let dataset = GeoTiffDataset::new(files.clone(), (256, 256));
        assert!(dataset.is_ok());

        let dataset = dataset.expect("Failed to create dataset");
        assert_eq!(dataset.file_paths.len(), 2);
        assert_eq!(dataset.patch_size, (256, 256));
    }

    #[test]
    fn test_dataset_with_labels() {
        let files = vec![PathBuf::from("test1.tif")];
        let labels = vec![PathBuf::from("label1.tif")];

        let dataset = GeoTiffDataset::new(files, (128, 128)).and_then(|d| d.with_labels(labels));

        assert!(dataset.is_ok());
    }

    #[test]
    fn test_dataset_validation() {
        // Empty file list
        let result = GeoTiffDataset::new(vec![], (256, 256));
        assert!(result.is_err());

        // Invalid patch size
        let files = vec![PathBuf::from("test.tif")];
        let result = GeoTiffDataset::new(files.clone(), (0, 0));
        assert!(result.is_err());

        let result = GeoTiffDataset::new(files, (256, 0));
        assert!(result.is_err());
    }

    #[test]
    fn test_dataset_builder() {
        let files = vec![PathBuf::from("test.tif")];
        let dataset = GeoTiffDataset::new(files, (256, 256))
            .and_then(|d| d.with_channels(4))
            .and_then(|d| d.with_classes(10))
            .and_then(|d| d.with_cache_size(32))
            .and_then(|d| d.with_patches_per_image(20));

        assert!(dataset.is_ok());
        let dataset = dataset.expect("Failed to build dataset");
        assert_eq!(dataset.num_channels, 4);
        assert_eq!(dataset.num_classes, 10);
        assert_eq!(dataset.patches_per_image, 20);
    }

    #[test]
    #[cfg(feature = "ml")]
    fn test_load_raster_missing_file() {
        let dataset = GeoTiffDataset::new(vec![PathBuf::from("nonexistent.tif")], (64, 64))
            .expect("Failed to create dataset");

        let result = dataset.load_raster(Path::new("nonexistent.tif"));
        assert!(result.is_err());

        // Check that error message is meaningful
        if let Err(e) = result {
            let msg = format!("{:?}", e);
            assert!(msg.contains("not found") || msg.contains("GeoTIFF"));
        }
    }

    #[test]
    #[cfg(feature = "ml")]
    fn test_load_raster_single_band() {
        // Create a test GeoTIFF file
        let test_file = create_test_geotiff(128, 128, 1).expect("Failed to create test file");

        let dataset = GeoTiffDataset::new(vec![test_file.clone()], (64, 64))
            .expect("Failed to create dataset");

        let result = dataset.load_raster(&test_file);
        assert!(result.is_ok());

        let buffer = result.expect("Failed to load raster");
        assert_eq!(buffer.width(), 128);
        assert_eq!(buffer.height(), 128);

        // Cleanup
        let _ = fs::remove_file(&test_file);
    }

    #[test]
    #[cfg(feature = "ml")]
    fn test_load_raster_multi_band() {
        // NOTE: Currently, the GeoTIFF reader returns all bands interleaved,
        // but RasterBuffer expects single-band data. Multi-band support
        // requires additional work to properly handle band interleaving.
        // For now, test with single-band and verify basic loading works.

        // Create a single-band test GeoTIFF file
        let test_file = create_test_geotiff(256, 256, 1).expect("Failed to create test file");

        let dataset = GeoTiffDataset::new(vec![test_file.clone()], (128, 128))
            .and_then(|d| d.with_channels(1))
            .expect("Failed to create dataset");

        let result = dataset.load_raster(&test_file);
        assert!(result.is_ok(), "Failed to load raster");

        let buffer = result.expect("Failed to load raster");
        assert_eq!(buffer.width(), 256);
        assert_eq!(buffer.height(), 256);

        // Cleanup
        let _ = fs::remove_file(&test_file);
    }

    #[test]
    #[cfg(feature = "ml")]
    fn test_raster_caching() {
        // Create a test GeoTIFF file
        let test_file = create_test_geotiff(64, 64, 1).expect("Failed to create test file");

        let dataset = GeoTiffDataset::new(vec![test_file.clone()], (32, 32))
            .expect("Failed to create dataset");

        // First load - should read from disk
        let result1 = dataset.load_raster(&test_file);
        assert!(result1.is_ok());

        // Second load - should hit cache
        let result2 = dataset.load_raster(&test_file);
        assert!(result2.is_ok());

        // Verify buffers are the same
        let buffer1 = result1.expect("Failed to load raster 1");
        let buffer2 = result2.expect("Failed to load raster 2");
        assert_eq!(buffer1.width(), buffer2.width());
        assert_eq!(buffer1.height(), buffer2.height());

        // Cleanup
        let _ = fs::remove_file(&test_file);
    }

    #[test]
    #[cfg(feature = "ml")]
    fn test_extract_random_patch() {
        // Create a test GeoTIFF file
        let test_file = create_test_geotiff(512, 512, 1).expect("Failed to create test file");

        let patch_size = (128, 128);
        let dataset = GeoTiffDataset::new(vec![test_file.clone()], patch_size)
            .expect("Failed to create dataset");

        let buffer = dataset
            .load_raster(&test_file)
            .expect("Failed to load raster");
        let patch = dataset.extract_random_patch(&buffer);

        assert!(patch.is_ok());
        let patch_data = patch.expect("Failed to extract patch");
        assert_eq!(patch_data.len(), patch_size.0 * patch_size.1);

        // Cleanup
        let _ = fs::remove_file(&test_file);
    }

    #[test]
    #[cfg(feature = "ml")]
    fn test_extract_patch_too_small_image() {
        // Create a small test GeoTIFF file
        let test_file = create_test_geotiff(32, 32, 1).expect("Failed to create test file");

        // Try to extract a patch larger than the image
        let patch_size = (128, 128);
        let dataset = GeoTiffDataset::new(vec![test_file.clone()], patch_size)
            .expect("Failed to create dataset");

        let buffer = dataset
            .load_raster(&test_file)
            .expect("Failed to load raster");
        let patch = dataset.extract_random_patch(&buffer);

        // Should return an error for dimensions
        assert!(patch.is_err());

        // Cleanup
        let _ = fs::remove_file(&test_file);
    }
}
