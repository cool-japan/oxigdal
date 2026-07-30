//! Dataset implementations for geospatial data.
//!
//! Provides the `Dataset` trait and implementations for loading
//! geospatial imagery for machine learning training.

use crate::augmentation::AugmentationPipeline;
use crate::{Error, Result};
use lru::LruCache;
use oxigeo_core::buffer::RasterBuffer;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Mutex;

#[cfg(feature = "ml")]
use crate::training::training_loop::Dataset as DatasetTrait;
#[cfg(feature = "ml")]
use oxigeo_core::RasterDataType;
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
    /// LRU cache for loaded rasters. Each entry holds every band of the file as
    /// a separate single-band [`RasterBuffer`], so multi-channel reads do not
    /// re-open the file per band.
    cache: Mutex<LruCache<PathBuf, Vec<RasterBuffer>>>,
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

    /// Loads every band of a raster from file, with caching.
    ///
    /// Returns one single-band [`RasterBuffer`] per band present in the file.
    /// Bands are read once and cached together so that a multi-channel dataset
    /// does not re-open the file for each channel.
    #[cfg(feature = "ml")]
    fn load_all_bands(&self, path: &Path) -> Result<Vec<RasterBuffer>> {
        // Check cache first
        {
            let mut cache = self
                .cache
                .lock()
                .map_err(|e| Error::InvalidState(format!("Failed to lock cache mutex: {}", e)))?;

            if let Some(bands) = cache.get(path) {
                return Ok(bands.clone());
            }
        }

        // Load from file using oxigeo-geotiff
        tracing::debug!("Loading raster from {:?}", path);

        // Check if file exists
        if !path.exists() {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("GeoTIFF file not found: {}", path.display()),
            )));
        }

        // Open the file as a data source
        let source = oxigeo_core::io::FileDataSource::open(path)?;

        // Open the GeoTIFF reader
        let reader = oxigeo_geotiff::GeoTiffReader::open(source)?;

        // Get image dimensions and data type
        let width = reader.width();
        let height = reader.height();
        let band_count = reader.band_count();
        let data_type = reader.data_type().unwrap_or(RasterDataType::UInt8);
        let nodata = reader.nodata();

        tracing::debug!(
            "GeoTIFF info: {}x{}, {} bands, type={:?}",
            width,
            height,
            band_count,
            data_type
        );

        if band_count == 0 {
            return Err(Error::invalid_dimensions(
                "at least 1 band",
                format!("{} bands", band_count),
            ));
        }

        // `GeoTiffReader::read_band(level, band)` returns exactly one
        // de-interleaved band plane -- `width * height * bytes_per_sample`
        // bytes, row-major, with the driver handling both chunky
        // (`PlanarConfiguration = 1`) and planar (`= 2`) storage. So we ask for
        // each band in turn and wrap it directly; de-interleaving here would
        // re-slice an already-single-band plane and read the wrong pixels.
        // See <https://github.com/cool-japan/oxigeo/issues/14>.
        let band_count = band_count as usize;
        let bytes_per_sample = data_type.size_bytes();
        let pixel_count = (width * height) as usize;
        let expected_len = pixel_count * bytes_per_sample;

        let mut bands = Vec::with_capacity(band_count);
        for band_idx in 0..band_count {
            let band_bytes = reader.read_band(0, band_idx)?;
            if band_bytes.len() != expected_len {
                return Err(Error::invalid_dimensions(
                    format!(
                        "{} bytes ({}x{} x {} byte(s)) for band {}",
                        expected_len, width, height, bytes_per_sample, band_idx
                    ),
                    format!("{} bytes", band_bytes.len()),
                ));
            }
            let buffer = RasterBuffer::new(band_bytes, width, height, data_type, nodata)?;
            bands.push(buffer);
        }

        // Cache the result
        {
            let mut cache = self
                .cache
                .lock()
                .map_err(|e| Error::InvalidState(format!("Failed to lock cache mutex: {}", e)))?;
            cache.put(path.to_path_buf(), bands.clone());
        }

        Ok(bands)
    }

    /// Loads a raster from file with caching, returning the first band only.
    ///
    /// Retained as a single-band convenience for tests; multi-channel training
    /// uses [`Self::load_bands`] / [`Self::load_all_bands`].
    #[cfg(all(feature = "ml", test))]
    fn load_raster(&self, path: &Path) -> Result<RasterBuffer> {
        let bands = self.load_all_bands(path)?;
        bands
            .into_iter()
            .next()
            .ok_or_else(|| Error::invalid_dimensions("at least 1 band", "0 bands".to_string()))
    }

    /// Loads the first `num_bands` bands of a file, erroring if the file does
    /// not contain enough bands to satisfy the request.
    #[cfg(feature = "ml")]
    fn load_bands(&self, path: &Path, num_bands: usize) -> Result<Vec<RasterBuffer>> {
        let mut bands = self.load_all_bands(path)?;
        if bands.len() < num_bands {
            return Err(Error::invalid_parameter(
                "num_channels",
                num_bands,
                format!("file {} only has {} band(s)", path.display(), bands.len()),
            ));
        }
        bands.truncate(num_bands);
        Ok(bands)
    }

    /// Validates that a patch of the configured size fits inside `buffer` and
    /// returns the usable maximum offsets `(max_x, max_y)` (inclusive upper
    /// bounds for the top-left corner).
    #[cfg(feature = "ml")]
    fn patch_bounds(&self, buffer: &RasterBuffer) -> Result<(usize, usize)> {
        let width = buffer.width() as usize;
        let height = buffer.height() as usize;

        if width < self.patch_size.1 || height < self.patch_size.0 {
            return Err(Error::invalid_dimensions(
                format!("{}x{}", self.patch_size.1, self.patch_size.0),
                format!("{}x{}", width, height),
            ));
        }

        Ok((width - self.patch_size.1, height - self.patch_size.0))
    }

    /// Extracts a patch at a fixed top-left offset from a single-band buffer.
    #[cfg(feature = "ml")]
    fn extract_patch_at(
        &self,
        buffer: &RasterBuffer,
        offset_x: usize,
        offset_y: usize,
    ) -> Result<Vec<f32>> {
        let mut patch = Vec::with_capacity(self.patch_size.0 * self.patch_size.1);

        for y in offset_y..(offset_y + self.patch_size.0) {
            for x in offset_x..(offset_x + self.patch_size.1) {
                let value = buffer.get_pixel(x as u64, y as u64)?;
                patch.push(value as f32);
            }
        }

        Ok(patch)
    }

    /// Extracts a random patch from a raster buffer (single band).
    ///
    /// Uses OS entropy for the offset. For reproducible sampling used by
    /// training, [`Self::get_batch`] derives a deterministic offset from the
    /// sample index instead, so this random sampler is exercised only in tests.
    #[cfg(all(feature = "ml", test))]
    fn extract_random_patch(&self, buffer: &RasterBuffer) -> Result<Vec<f32>> {
        let (max_x, max_y) = self.patch_bounds(buffer)?;

        let offset_x = if max_x > 0 {
            (getrandom::get_random_u64()? % (max_x as u64 + 1)) as usize
        } else {
            0
        };

        let offset_y = if max_y > 0 {
            (getrandom::get_random_u64()? % (max_y as u64 + 1)) as usize
        } else {
            0
        };

        self.extract_patch_at(buffer, offset_x, offset_y)
    }
}

/// Derives a deterministic patch offset from a sample index.
///
/// Uses a SplitMix64 hash of the index so that the same `idx` always maps to
/// the same `(offset_x, offset_y)`, keeping validation / early-stopping /
/// checkpoint comparisons reproducible across calls and epochs while still
/// spreading patches across the image.
#[cfg(feature = "ml")]
fn deterministic_offset(idx: usize, max_x: usize, max_y: usize) -> (usize, usize) {
    // SplitMix64: two successive draws from the seeded state give two
    // independent, well-distributed 64-bit values.
    fn splitmix64(state: &mut u64) -> u64 {
        *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = *state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    let mut state = (idx as u64).wrapping_add(0x1234_5678_9ABC_DEF0);
    let rx = splitmix64(&mut state);
    let ry = splitmix64(&mut state);

    let offset_x = if max_x > 0 {
        (rx % (max_x as u64 + 1)) as usize
    } else {
        0
    };
    let offset_y = if max_y > 0 {
        (ry % (max_y as u64 + 1)) as usize
    } else {
        0
    };

    (offset_x, offset_y)
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
        // Supervised training requires ground-truth labels. Returning fabricated
        // all-zero targets would silently train the model against a fake
        // supervision signal, so refuse instead.
        let label_paths = self.label_paths.as_ref().ok_or_else(|| {
            Error::invalid_parameter(
                "label_paths",
                "None",
                "supervised get_batch requires labels; call with_labels() before training",
            )
        })?;

        let batch_size = indices.len();
        let patch_pixels = self.patch_size.0 * self.patch_size.1;
        let input_size = batch_size * self.num_channels * patch_pixels;
        let output_size = batch_size * self.num_classes * patch_pixels;

        let mut inputs = Vec::with_capacity(input_size);
        let mut targets = Vec::with_capacity(output_size);

        for &idx in indices {
            // Determine which file and which patch.
            let file_idx = (idx / self.patches_per_image).min(self.file_paths.len() - 1);

            // Load the requested number of input channels (bands). This errors
            // if the file has fewer bands than `num_channels`.
            let input_bands = self.load_bands(&self.file_paths[file_idx], self.num_channels)?;

            // Derive a single deterministic offset per sample index so the same
            // index always yields the same patch, and so the input patch and its
            // label patch are spatially aligned.
            let (max_x, max_y) = self.patch_bounds(&input_bands[0])?;
            let (offset_x, offset_y) = deterministic_offset(idx, max_x, max_y);

            // Channel-major layout (all of channel 0, then channel 1, ...) to
            // match the NCHW input shape advertised by `shapes()`.
            for band in &input_bands {
                let patch = self.extract_patch_at(band, offset_x, offset_y)?;
                inputs.extend_from_slice(&patch);
            }

            // Labels use `num_classes` bands and are sampled at the same offset.
            let label_bands = self.load_bands(&label_paths[file_idx], self.num_classes)?;
            let (lmax_x, lmax_y) = self.patch_bounds(&label_bands[0])?;
            // Clamp the shared offset into the label's valid range in case the
            // label raster is smaller than the input.
            let loffset_x = offset_x.min(lmax_x);
            let loffset_y = offset_y.min(lmax_y);
            for band in &label_bands {
                let label_patch = self.extract_patch_at(band, loffset_x, loffset_y)?;
                targets.extend_from_slice(&label_patch);
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

/// Helper function to get a random u64 value (used by the test-only random
/// patch sampler).
#[cfg(all(feature = "ml", test))]
mod getrandom {
    use crate::Result;

    pub fn get_random_u64() -> Result<u64> {
        let mut buf = [0u8; 8];
        getrandom::fill(&mut buf).map_err(|e| {
            crate::Error::Numerical(format!("Failed to generate random number: {}", e))
        })?;
        Ok(u64::from_ne_bytes(buf))
    }
}

#[cfg(test)]
mod tests {
    // Channel/class counts are written out in full (e.g. `2 * 1 * patch_pixels`)
    // to document the tensor layout, so the `* 1` factors are intentional.
    #![allow(clippy::identity_op)]
    use super::*;
    #[cfg(feature = "ml")]
    use std::env;

    /// A fixture path that is unique per process and per call, and that removes
    /// the file when it goes out of scope -- including on a panicking test,
    /// where a trailing `remove_file` would be skipped.
    #[cfg(feature = "ml")]
    struct TempPath(PathBuf);

    #[cfg(feature = "ml")]
    impl std::ops::Deref for TempPath {
        type Target = std::path::Path;

        fn deref(&self) -> &std::path::Path {
            &self.0
        }
    }

    #[cfg(feature = "ml")]
    impl AsRef<std::path::Path> for TempPath {
        fn as_ref(&self) -> &std::path::Path {
            &self.0
        }
    }

    #[cfg(feature = "ml")]
    impl Drop for TempPath {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    /// Helper to create a test GeoTIFF file
    #[cfg(feature = "ml")]
    fn create_test_geotiff(width: u32, height: u32, bands: u16) -> Result<TempPath> {
        // Fill with the historical test pattern: a running `i % 256` over the
        // pixel-interleaved byte stream.
        let size = (width as usize) * (height as usize) * (bands as usize);
        let data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
        create_test_geotiff_with_data(width, height, bands, &data)
    }

    /// Helper to create a test GeoTIFF file from caller-supplied,
    /// pixel-interleaved (`[b0,b1,..,b0,b1,..]`) UInt8 samples.
    #[cfg(feature = "ml")]
    fn create_test_geotiff_with_data(
        width: u32,
        height: u32,
        bands: u16,
        data: &[u8],
    ) -> Result<TempPath> {
        use oxigeo_core::RasterDataType;
        use oxigeo_core::types::{GeoTransform, NoDataValue};
        use oxigeo_geotiff::{GeoTiffWriter, GeoTiffWriterOptions, WriterConfig};

        // Fixtures live directly in the temp directory: a shared subdirectory
        // would outlive every run, since no single test can know when the last
        // one has finished with it.
        let temp_dir = env::temp_dir();

        // Generate unique filename
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| Error::InvalidState(format!("Failed to get timestamp: {}", e)))?
            .as_nanos();
        // The leaf name embeds the process id and a monotonic counter on top of
        // the timestamp, so no two test binaries -- nor two concurrent runs of
        // this one -- can ever land on the same fixture file.
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let filename = temp_dir.join(format!(
            "oxigeo_ml_test_{}_{}_{}x{}_b{}_t{}.tif",
            std::process::id(),
            seq,
            width,
            height,
            bands,
            timestamp
        ));

        let data_type = RasterDataType::UInt8;
        let size =
            (width as u64) * (height as u64) * (bands as u64) * (data_type.size_bytes() as u64);
        if data.len() as u64 != size {
            return Err(Error::invalid_dimensions(
                format!("{} sample bytes", size),
                format!("{} sample bytes", data.len()),
            ));
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
        config.compression = oxigeo_geotiff::Compression::None;
        config.tile_width = Some(256);
        config.tile_height = Some(256);
        config.photometric = oxigeo_geotiff::PhotometricInterpretation::BlackIsZero;
        config.geo_transform = Some(geo_transform);
        config.epsg_code = Some(4326);
        config.nodata = NoDataValue::None;
        config.generate_overviews = false;

        let options = GeoTiffWriterOptions::default();

        let mut writer = GeoTiffWriter::create(&filename, config, options)?;

        writer.write(data)?;

        // Writer automatically closes when dropped
        drop(writer);

        Ok(TempPath(filename))
    }

    /// Per-band, per-pixel value used by the issue-14 regression fixture.
    ///
    /// Each band occupies its own disjoint value range (band 0 -> `0..=16`,
    /// band 1 -> `80..=96`, band 2 -> `160..=176`), so "band 0 repeated",
    /// "bands swapped" and "interleaved garbage" are all individually
    /// distinguishable from the correct answer.
    #[cfg(feature = "ml")]
    fn issue_14_band_value(band: usize, pixel: usize) -> u8 {
        (band as u8) * 80 + (pixel % 17) as u8
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

        let dataset = GeoTiffDataset::new(vec![test_file.to_path_buf()], (64, 64))
            .expect("Failed to create dataset");

        let result = dataset.load_raster(&test_file);
        assert!(result.is_ok());

        let buffer = result.expect("Failed to load raster");
        assert_eq!(buffer.width(), 128);
        assert_eq!(buffer.height(), 128);

        // Cleanup
    }

    #[test]
    #[cfg(feature = "ml")]
    fn test_load_raster_multi_band() {
        // The GeoTIFF reader returns every band pixel-interleaved in a single
        // call; `load_all_bands` must de-interleave that into one single-band
        // buffer per band, each of full spatial size.
        let test_file = create_test_geotiff(32, 16, 3).expect("Failed to create test file");

        let dataset = GeoTiffDataset::new(vec![test_file.to_path_buf()], (8, 8))
            .and_then(|d| d.with_channels(3))
            .expect("Failed to create dataset");

        let bands = dataset
            .load_all_bands(&test_file)
            .expect("Failed to load bands");
        assert_eq!(bands.len(), 3, "expected 3 de-interleaved bands");
        for band in &bands {
            assert_eq!(band.width(), 32);
            assert_eq!(band.height(), 16);
        }

        // create_test_geotiff fills bytes with `i % 256` over the interleaved
        // [b0,b1,b2, b0,b1,b2, ...] stream (UInt8), so for pixel 0 the three
        // bands are 0,1,2 and for pixel 1 they are 3,4,5. Verify the split put
        // the right sample in the right band.
        let b0 = bands[0].get_pixel(0, 0).expect("pixel");
        let b1 = bands[1].get_pixel(0, 0).expect("pixel");
        let b2 = bands[2].get_pixel(0, 0).expect("pixel");
        assert_eq!((b0, b1, b2), (0.0, 1.0, 2.0));
        // Pixel (1,0) is the second pixel -> byte indices 3,4,5.
        let n0 = bands[0].get_pixel(1, 0).expect("pixel");
        let n1 = bands[1].get_pixel(1, 0).expect("pixel");
        let n2 = bands[2].get_pixel(1, 0).expect("pixel");
        assert_eq!((n0, n1, n2), (3.0, 4.0, 5.0));

        // Cleanup
    }

    /// Regression test for <https://github.com/cool-japan/oxigeo/issues/14>.
    ///
    /// `load_all_bands` used to issue a single `read_band(0, 0)`, assert the
    /// result was `width * height * band_count * bytes_per_sample` bytes and
    /// de-interleave it by hand. Once `read_band` started returning one
    /// de-interleaved band plane (`width * height * bytes_per_sample`), that
    /// length assertion failed for every multi-band file, so loading any
    /// RGB/multispectral GeoTIFF errored out and ML training on multi-channel
    /// data was dead. It must now read each band separately and hand back one
    /// full-size buffer per band, holding that band's own samples.
    #[test]
    #[cfg(feature = "ml")]
    fn test_issue_14_load_all_bands_multiband_returns_distinct_planes() {
        let width = 12u32;
        let height = 5u32;
        let band_count = 3usize;
        let pixel_count = (width as usize) * (height as usize);

        // Pixel-interleaved source: [b0,b1,b2, b0,b1,b2, ...].
        let mut interleaved = Vec::with_capacity(pixel_count * band_count);
        for pixel in 0..pixel_count {
            for band in 0..band_count {
                interleaved.push(issue_14_band_value(band, pixel));
            }
        }

        let test_file =
            create_test_geotiff_with_data(width, height, band_count as u16, &interleaved)
                .expect("Failed to create 3-band test file");

        let dataset = GeoTiffDataset::new(vec![test_file.to_path_buf()], (4, 4))
            .and_then(|d| d.with_channels(3))
            .expect("Failed to create dataset");

        let bands = dataset
            .load_all_bands(&test_file)
            .expect("load_all_bands must succeed for a multi-band GeoTIFF (issue #14)");

        assert_eq!(
            bands.len(),
            band_count,
            "expected one RasterBuffer per band: expected {}, got {}",
            band_count,
            bands.len()
        );

        for (band, buffer) in bands.iter().enumerate() {
            assert_eq!(
                buffer.width(),
                width as u64,
                "band {}: width expected {}, got {}",
                band,
                width,
                buffer.width()
            );
            assert_eq!(
                buffer.height(),
                height as u64,
                "band {}: height expected {}, got {}",
                band,
                height,
                buffer.height()
            );

            for y in 0..height as u64 {
                for x in 0..width as u64 {
                    let pixel = (y as usize) * (width as usize) + (x as usize);
                    let expected = f64::from(issue_14_band_value(band, pixel));
                    let actual = buffer
                        .get_pixel(x, y)
                        .expect("get_pixel inside the raster extent");
                    assert!(
                        (actual - expected).abs() < f64::EPSILON,
                        "band {} pixel ({}, {}): expected {}, got {}",
                        band,
                        x,
                        y,
                        expected,
                        actual
                    );
                }
            }
        }

        // Explicitly rule out the "every band is band 0" failure mode: each
        // band lives in a disjoint value range, so pixel (0, 0) must differ.
        for band in 1..band_count {
            let first = bands[0]
                .get_pixel(0, 0)
                .expect("get_pixel inside the raster extent");
            let other = bands[band]
                .get_pixel(0, 0)
                .expect("get_pixel inside the raster extent");
            assert!(
                (first - other).abs() > f64::EPSILON,
                "band {} pixel (0, 0): expected a value distinct from band 0's {}, got {}",
                band,
                first,
                other
            );
        }

        // Cleanup
    }

    #[test]
    #[cfg(feature = "ml")]
    fn test_raster_caching() {
        // Create a test GeoTIFF file
        let test_file = create_test_geotiff(64, 64, 1).expect("Failed to create test file");

        let dataset = GeoTiffDataset::new(vec![test_file.to_path_buf()], (32, 32))
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
    }

    #[test]
    #[cfg(feature = "ml")]
    fn test_extract_random_patch() {
        // Create a test GeoTIFF file
        let test_file = create_test_geotiff(512, 512, 1).expect("Failed to create test file");

        let patch_size = (128, 128);
        let dataset = GeoTiffDataset::new(vec![test_file.to_path_buf()], patch_size)
            .expect("Failed to create dataset");

        let buffer = dataset
            .load_raster(&test_file)
            .expect("Failed to load raster");
        let patch = dataset.extract_random_patch(&buffer);

        assert!(patch.is_ok());
        let patch_data = patch.expect("Failed to extract patch");
        assert_eq!(patch_data.len(), patch_size.0 * patch_size.1);

        // Cleanup
    }

    #[test]
    #[cfg(feature = "ml")]
    fn test_get_batch_requires_labels() {
        // A dataset created without labels must refuse supervised get_batch
        // rather than fabricating all-zero targets.
        let test_file = create_test_geotiff(64, 64, 1).expect("Failed to create test file");

        let dataset = GeoTiffDataset::new(vec![test_file.to_path_buf()], (16, 16))
            .expect("Failed to create dataset");

        let result = dataset.get_batch(&[0]);
        assert!(result.is_err(), "get_batch without labels must error");
    }

    #[test]
    #[cfg(feature = "ml")]
    fn test_get_batch_deterministic() {
        // The same sample index must always yield the same patch data.
        let input_file = create_test_geotiff(128, 128, 1).expect("Failed to create input file");
        let label_file = create_test_geotiff(128, 128, 1).expect("Failed to create label file");

        let dataset = GeoTiffDataset::new(vec![input_file.to_path_buf()], (32, 32))
            .and_then(|d| d.with_channels(1))
            .and_then(|d| d.with_labels(vec![label_file.to_path_buf()]))
            .expect("Failed to create dataset");

        let (in1, tg1) = dataset.get_batch(&[5]).expect("first get_batch failed");
        let (in2, tg2) = dataset.get_batch(&[5]).expect("second get_batch failed");

        assert_eq!(in1, in2, "input patch for index 5 must be reproducible");
        assert_eq!(tg1, tg2, "target patch for index 5 must be reproducible");

        // Different indices should (generally) draw different offsets.
        let (in_other, _) = dataset.get_batch(&[7]).expect("get_batch failed");
        assert_eq!(in_other.len(), in1.len());
    }

    #[test]
    #[cfg(feature = "ml")]
    fn test_get_batch_multichannel_length() {
        // A 3-channel dataset must return inputs sized for all 3 channels.
        let input_file = create_test_geotiff(64, 64, 3).expect("Failed to create input file");
        let label_file = create_test_geotiff(64, 64, 1).expect("Failed to create label file");

        let dataset = GeoTiffDataset::new(vec![input_file.to_path_buf()], (16, 16))
            .and_then(|d| d.with_channels(3))
            .and_then(|d| d.with_classes(1))
            .and_then(|d| d.with_labels(vec![label_file.to_path_buf()]))
            .expect("Failed to create dataset");

        let (inputs, targets) = dataset.get_batch(&[0, 1]).expect("get_batch failed");

        let patch_pixels = 16 * 16;
        assert_eq!(
            inputs.len(),
            2 * 3 * patch_pixels,
            "inputs must cover 3 channels"
        );
        assert_eq!(
            targets.len(),
            2 * 1 * patch_pixels,
            "targets must cover 1 class"
        );
    }

    #[test]
    #[cfg(feature = "ml")]
    fn test_get_batch_too_many_channels_errors() {
        // Requesting more channels than the file provides must error, not
        // silently return a short buffer.
        let input_file = create_test_geotiff(64, 64, 1).expect("Failed to create input file");
        let label_file = create_test_geotiff(64, 64, 1).expect("Failed to create label file");

        let dataset = GeoTiffDataset::new(vec![input_file.to_path_buf()], (16, 16))
            .and_then(|d| d.with_channels(4))
            .and_then(|d| d.with_labels(vec![label_file.to_path_buf()]))
            .expect("Failed to create dataset");

        let result = dataset.get_batch(&[0]);
        assert!(
            result.is_err(),
            "requesting 4 channels from a 1-band file must error"
        );
    }

    #[test]
    #[cfg(feature = "ml")]
    fn test_extract_patch_too_small_image() {
        // Create a small test GeoTIFF file
        let test_file = create_test_geotiff(32, 32, 1).expect("Failed to create test file");

        // Try to extract a patch larger than the image
        let patch_size = (128, 128);
        let dataset = GeoTiffDataset::new(vec![test_file.to_path_buf()], patch_size)
            .expect("Failed to create dataset");

        let buffer = dataset
            .load_raster(&test_file)
            .expect("Failed to load raster");
        let patch = dataset.extract_random_patch(&buffer);

        // Should return an error for dimensions
        assert!(patch.is_err());

        // Cleanup
    }
}
