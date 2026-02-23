//! Variable chunking support for Zarr arrays
//!
//! This module provides advanced chunking strategies including:
//! - Variable chunk size support
//! - Chunk optimization algorithms
//! - Chunk boundary alignment
//! - Dimension-based chunking strategies
//! - Automatic chunk size calculation
//! - Chunk compression integration
//!
//! # Variable Chunking
//!
//! Unlike fixed chunk sizes, variable chunking allows different chunk sizes
//! along each dimension and can adapt chunk sizes based on data characteristics.
//!
//! # Example
//!
//! ```ignore
//! use oxigdal_zarr::chunking::{ChunkingStrategy, VariableChunkSpec};
//!
//! // Create a variable chunk specification
//! let spec = VariableChunkSpec::builder()
//!     .dimension(0, DimensionChunkSpec::fixed(100))
//!     .dimension(1, DimensionChunkSpec::variable(vec![50, 100, 150]))
//!     .dimension(2, DimensionChunkSpec::auto(32.0 * 1024.0 * 1024.0))
//!     .build()?;
//! ```

use crate::codecs::Codec;
use crate::dimension::Shape;
use crate::error::{ChunkError, MetadataError, Result, ZarrError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

// ============================================================================
// Constants for chunk size optimization
// ============================================================================

/// Default target chunk size in bytes (1 MB)
pub const DEFAULT_TARGET_CHUNK_SIZE: usize = 1024 * 1024;

/// Minimum chunk size in bytes (4 KB)
pub const MIN_CHUNK_SIZE: usize = 4 * 1024;

/// Maximum chunk size in bytes (256 MB)
pub const MAX_CHUNK_SIZE: usize = 256 * 1024 * 1024;

/// Default compression ratio estimate for optimization
pub const DEFAULT_COMPRESSION_RATIO: f64 = 0.5;

/// Default number of elements per chunk dimension
pub const DEFAULT_ELEMENTS_PER_DIM: usize = 128;

// ============================================================================
// Chunking Strategy Types
// ============================================================================

/// Strategy for determining chunk sizes
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ChunkingStrategy {
    /// Fixed chunk sizes for all dimensions
    Fixed(Vec<usize>),

    /// Variable chunk sizes per dimension
    Variable(VariableChunkSpec),

    /// Automatically optimize chunk sizes
    Auto(AutoChunkConfig),

    /// Aligned chunks for specific access patterns
    Aligned(AlignedChunkConfig),

    /// Adaptive chunks that vary based on data density
    Adaptive(AdaptiveChunkConfig),
}

impl ChunkingStrategy {
    /// Calculates chunk sizes for a given array shape and element size
    ///
    /// # Errors
    /// Returns error if the strategy cannot produce valid chunk sizes
    pub fn calculate_chunk_sizes(
        &self,
        array_shape: &Shape,
        element_size: usize,
    ) -> Result<Vec<usize>> {
        match self {
            Self::Fixed(sizes) => validate_fixed_chunks(sizes, array_shape),
            Self::Variable(spec) => spec.calculate_sizes(array_shape, element_size),
            Self::Auto(config) => config.calculate_sizes(array_shape, element_size),
            Self::Aligned(config) => config.calculate_sizes(array_shape, element_size),
            Self::Adaptive(config) => config.calculate_sizes(array_shape, element_size),
        }
    }

    /// Creates a fixed chunking strategy
    #[must_use]
    pub fn fixed(sizes: Vec<usize>) -> Self {
        Self::Fixed(sizes)
    }

    /// Creates an automatic chunking strategy with default settings
    #[must_use]
    pub fn auto() -> Self {
        Self::Auto(AutoChunkConfig::default())
    }

    /// Creates an automatic chunking strategy with target size
    #[must_use]
    pub fn auto_with_target(target_size_bytes: usize) -> Self {
        Self::Auto(AutoChunkConfig {
            target_chunk_size: target_size_bytes,
            ..Default::default()
        })
    }
}

impl Default for ChunkingStrategy {
    fn default() -> Self {
        Self::Auto(AutoChunkConfig::default())
    }
}

/// Validates fixed chunk sizes against array shape
fn validate_fixed_chunks(sizes: &[usize], array_shape: &Shape) -> Result<Vec<usize>> {
    if sizes.len() != array_shape.ndim() {
        return Err(ZarrError::Chunk(ChunkError::InvalidChunkShape {
            chunk_shape: sizes.to_vec(),
            array_shape: array_shape.to_vec(),
        }));
    }

    for (i, &size) in sizes.iter().enumerate() {
        if size == 0 {
            return Err(ZarrError::Metadata(MetadataError::InvalidChunkGrid {
                reason: format!("Chunk size at dimension {i} cannot be zero"),
            }));
        }
    }

    Ok(sizes.to_vec())
}

// ============================================================================
// Variable Chunk Specification
// ============================================================================

/// Specification for variable chunk sizes
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VariableChunkSpec {
    /// Chunk specification for each dimension
    dimensions: Vec<DimensionChunkSpec>,

    /// Optional constraints on total chunk size
    #[serde(skip_serializing_if = "Option::is_none")]
    max_chunk_bytes: Option<usize>,

    /// Whether to allow partial chunks at boundaries
    #[serde(default = "default_true")]
    allow_partial: bool,
}

fn default_true() -> bool {
    true
}

impl VariableChunkSpec {
    /// Creates a new variable chunk specification builder
    #[must_use]
    pub fn builder() -> VariableChunkSpecBuilder {
        VariableChunkSpecBuilder::new()
    }

    /// Creates a variable chunk specification from dimension specs
    ///
    /// # Errors
    /// Returns error if specs are empty
    pub fn new(dimensions: Vec<DimensionChunkSpec>) -> Result<Self> {
        if dimensions.is_empty() {
            return Err(ZarrError::InvalidDimension {
                message: "Variable chunk spec must have at least one dimension".to_string(),
            });
        }

        Ok(Self {
            dimensions,
            max_chunk_bytes: None,
            allow_partial: true,
        })
    }

    /// Sets the maximum chunk size in bytes
    #[must_use]
    pub fn with_max_bytes(mut self, max_bytes: usize) -> Self {
        self.max_chunk_bytes = Some(max_bytes);
        self
    }

    /// Calculates chunk sizes for a given array shape
    ///
    /// # Errors
    /// Returns error if dimensions don't match or specs are invalid
    pub fn calculate_sizes(&self, array_shape: &Shape, element_size: usize) -> Result<Vec<usize>> {
        if self.dimensions.len() != array_shape.ndim() {
            return Err(ZarrError::Chunk(ChunkError::InvalidChunkShape {
                chunk_shape: vec![0; self.dimensions.len()],
                array_shape: array_shape.to_vec(),
            }));
        }

        let mut sizes = Vec::with_capacity(self.dimensions.len());

        for (i, spec) in self.dimensions.iter().enumerate() {
            let dim_size = array_shape
                .dim(i)
                .ok_or_else(|| ZarrError::InvalidDimension {
                    message: format!("Dimension {i} out of bounds"),
                })?;

            let chunk_size = spec.calculate_for_dimension(dim_size, element_size)?;
            sizes.push(chunk_size);
        }

        // Apply max bytes constraint if set
        if let Some(max_bytes) = self.max_chunk_bytes {
            sizes = self.apply_byte_constraint(&sizes, element_size, max_bytes);
        }

        Ok(sizes)
    }

    /// Applies byte constraint by reducing chunk sizes proportionally
    fn apply_byte_constraint(
        &self,
        sizes: &[usize],
        element_size: usize,
        max_bytes: usize,
    ) -> Vec<usize> {
        let current_elements: usize = sizes.iter().product();
        let current_bytes = current_elements * element_size;

        if current_bytes <= max_bytes {
            return sizes.to_vec();
        }

        // Calculate reduction factor
        let reduction_factor = (max_bytes as f64 / current_bytes as f64).powf(1.0 / sizes.len() as f64);

        sizes
            .iter()
            .map(|&s| {
                let reduced = (s as f64 * reduction_factor) as usize;
                reduced.max(1)
            })
            .collect()
    }
}

/// Builder for variable chunk specifications
#[derive(Debug, Default)]
pub struct VariableChunkSpecBuilder {
    dimensions: Vec<Option<DimensionChunkSpec>>,
    max_chunk_bytes: Option<usize>,
    allow_partial: bool,
}

impl VariableChunkSpecBuilder {
    /// Creates a new builder
    #[must_use]
    pub fn new() -> Self {
        Self {
            dimensions: Vec::new(),
            max_chunk_bytes: None,
            allow_partial: true,
        }
    }

    /// Sets the chunk specification for a dimension
    #[must_use]
    pub fn dimension(mut self, index: usize, spec: DimensionChunkSpec) -> Self {
        while self.dimensions.len() <= index {
            self.dimensions.push(None);
        }
        self.dimensions[index] = Some(spec);
        self
    }

    /// Sets the maximum chunk size in bytes
    #[must_use]
    pub fn max_bytes(mut self, max_bytes: usize) -> Self {
        self.max_chunk_bytes = Some(max_bytes);
        self
    }

    /// Sets whether to allow partial chunks at boundaries
    #[must_use]
    pub fn allow_partial(mut self, allow: bool) -> Self {
        self.allow_partial = allow;
        self
    }

    /// Builds the variable chunk specification
    ///
    /// # Errors
    /// Returns error if any dimension is not specified
    pub fn build(self) -> Result<VariableChunkSpec> {
        let dimensions: Result<Vec<DimensionChunkSpec>> = self
            .dimensions
            .into_iter()
            .enumerate()
            .map(|(i, opt)| {
                opt.ok_or_else(|| ZarrError::Metadata(MetadataError::MissingField {
                    field: Box::leak(format!("dimension_{i}").into_boxed_str()),
                }))
            })
            .collect();

        Ok(VariableChunkSpec {
            dimensions: dimensions?,
            max_chunk_bytes: self.max_chunk_bytes,
            allow_partial: self.allow_partial,
        })
    }
}

// ============================================================================
// Dimension Chunk Specification
// ============================================================================

/// Chunk specification for a single dimension
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DimensionChunkSpec {
    /// Fixed chunk size
    Fixed(usize),

    /// Variable chunk sizes (boundaries)
    Variable(Vec<usize>),

    /// Automatic based on target size
    Auto {
        /// Target chunk size in bytes
        target_bytes: usize,
        /// Minimum chunk size
        min_size: usize,
        /// Maximum chunk size
        max_size: usize,
    },

    /// Full dimension (single chunk)
    Full,

    /// Aligned to specific boundary
    Aligned {
        /// Alignment boundary
        alignment: usize,
        /// Target size
        target: usize,
    },

    /// Ratio-based (fraction of dimension)
    Ratio(f64),
}

impl DimensionChunkSpec {
    /// Creates a fixed chunk specification
    #[must_use]
    pub const fn fixed(size: usize) -> Self {
        Self::Fixed(size)
    }

    /// Creates a variable chunk specification
    #[must_use]
    pub fn variable(sizes: Vec<usize>) -> Self {
        Self::Variable(sizes)
    }

    /// Creates an automatic chunk specification
    #[must_use]
    pub fn auto(target_bytes: usize) -> Self {
        Self::Auto {
            target_bytes,
            min_size: 1,
            max_size: usize::MAX,
        }
    }

    /// Creates a full-dimension chunk specification
    #[must_use]
    pub const fn full() -> Self {
        Self::Full
    }

    /// Creates an aligned chunk specification
    #[must_use]
    pub const fn aligned(alignment: usize, target: usize) -> Self {
        Self::Aligned { alignment, target }
    }

    /// Creates a ratio-based chunk specification
    #[must_use]
    pub const fn ratio(ratio: f64) -> Self {
        Self::Ratio(ratio)
    }

    /// Calculates the chunk size for this dimension
    ///
    /// # Errors
    /// Returns error if the specification is invalid
    pub fn calculate_for_dimension(
        &self,
        dim_size: usize,
        element_size: usize,
    ) -> Result<usize> {
        let size = match self {
            Self::Fixed(size) => *size,

            Self::Variable(sizes) => {
                // For variable, use the median size as the chunk size
                if sizes.is_empty() {
                    return Err(ZarrError::Metadata(MetadataError::InvalidChunkGrid {
                        reason: "Variable chunk sizes cannot be empty".to_string(),
                    }));
                }
                let mut sorted = sizes.clone();
                sorted.sort_unstable();
                sorted[sorted.len() / 2]
            }

            Self::Auto {
                target_bytes,
                min_size,
                max_size,
            } => {
                let target_elements = target_bytes / element_size.max(1);
                let size = target_elements.clamp(*min_size, *max_size);
                size.min(dim_size)
            }

            Self::Full => dim_size,

            Self::Aligned { alignment, target } => {
                // Round target to nearest alignment boundary
                let aligned = (*target / alignment) * alignment;
                let size = aligned.max(*alignment);
                size.min(dim_size)
            }

            Self::Ratio(ratio) => {
                let size = (dim_size as f64 * ratio) as usize;
                size.max(1).min(dim_size)
            }
        };

        if size == 0 {
            return Err(ZarrError::Metadata(MetadataError::InvalidChunkGrid {
                reason: "Calculated chunk size cannot be zero".to_string(),
            }));
        }

        Ok(size)
    }
}

// ============================================================================
// Auto Chunk Configuration
// ============================================================================

/// Configuration for automatic chunk size calculation
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutoChunkConfig {
    /// Target chunk size in bytes
    pub target_chunk_size: usize,

    /// Minimum chunk size in bytes
    pub min_chunk_size: usize,

    /// Maximum chunk size in bytes
    pub max_chunk_size: usize,

    /// Estimated compression ratio (0.0-1.0)
    pub compression_ratio: f64,

    /// Prefer square/cubic chunks
    pub prefer_balanced: bool,

    /// Priority weights for each dimension (higher = larger chunks)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimension_weights: Option<Vec<f64>>,

    /// Access pattern hint
    pub access_pattern: AccessPattern,
}

impl Default for AutoChunkConfig {
    fn default() -> Self {
        Self {
            target_chunk_size: DEFAULT_TARGET_CHUNK_SIZE,
            min_chunk_size: MIN_CHUNK_SIZE,
            max_chunk_size: MAX_CHUNK_SIZE,
            compression_ratio: DEFAULT_COMPRESSION_RATIO,
            prefer_balanced: true,
            dimension_weights: None,
            access_pattern: AccessPattern::Random,
        }
    }
}

impl AutoChunkConfig {
    /// Creates a new auto chunk configuration with target size
    #[must_use]
    pub fn with_target_size(target_bytes: usize) -> Self {
        Self {
            target_chunk_size: target_bytes,
            ..Default::default()
        }
    }

    /// Sets the compression ratio estimate
    #[must_use]
    pub fn with_compression_ratio(mut self, ratio: f64) -> Self {
        self.compression_ratio = ratio.clamp(0.01, 1.0);
        self
    }

    /// Sets the access pattern
    #[must_use]
    pub fn with_access_pattern(mut self, pattern: AccessPattern) -> Self {
        self.access_pattern = pattern;
        self
    }

    /// Sets dimension weights
    #[must_use]
    pub fn with_dimension_weights(mut self, weights: Vec<f64>) -> Self {
        self.dimension_weights = Some(weights);
        self
    }

    /// Calculates optimal chunk sizes for a given array shape
    ///
    /// # Errors
    /// Returns error if calculation fails
    pub fn calculate_sizes(&self, array_shape: &Shape, element_size: usize) -> Result<Vec<usize>> {
        let ndim = array_shape.ndim();

        if ndim == 0 {
            return Err(ZarrError::InvalidDimension {
                message: "Cannot calculate chunks for 0-dimensional array".to_string(),
            });
        }

        // Calculate target elements per chunk (accounting for compression)
        let effective_element_size =
            (element_size as f64 * self.compression_ratio).max(1.0) as usize;
        let target_elements = self.target_chunk_size / effective_element_size.max(1);

        // Get dimension weights
        let weights = self.get_dimension_weights(array_shape);

        // Calculate chunk sizes based on access pattern
        let sizes = match self.access_pattern {
            AccessPattern::Sequential => {
                self.calculate_sequential_chunks(array_shape, target_elements, &weights)
            }
            AccessPattern::Random => {
                self.calculate_balanced_chunks(array_shape, target_elements, &weights)
            }
            AccessPattern::ColumnMajor => {
                self.calculate_column_major_chunks(array_shape, target_elements, &weights)
            }
            AccessPattern::RowMajor => {
                self.calculate_row_major_chunks(array_shape, target_elements, &weights)
            }
            AccessPattern::Strided { stride } => {
                self.calculate_strided_chunks(array_shape, target_elements, stride)
            }
        };

        // Validate and adjust sizes
        self.validate_and_adjust(sizes, array_shape)
    }

    /// Gets dimension weights, using defaults if not specified
    fn get_dimension_weights(&self, array_shape: &Shape) -> Vec<f64> {
        self.dimension_weights
            .clone()
            .unwrap_or_else(|| vec![1.0; array_shape.ndim()])
    }

    /// Calculates balanced (square/cubic) chunks
    fn calculate_balanced_chunks(
        &self,
        array_shape: &Shape,
        target_elements: usize,
        weights: &[f64],
    ) -> Vec<usize> {
        let ndim = array_shape.ndim();

        if ndim == 0 {
            return Vec::new();
        }

        // Calculate weighted target per dimension
        let total_weight: f64 = weights.iter().sum();
        let base_size = (target_elements as f64).powf(1.0 / ndim as f64);

        array_shape
            .as_slice()
            .iter()
            .zip(weights)
            .map(|(&dim_size, &weight)| {
                let weighted_size = base_size * (weight / total_weight * ndim as f64);
                let size = weighted_size.round() as usize;
                size.max(1).min(dim_size)
            })
            .collect()
    }

    /// Calculates chunks optimized for sequential access
    fn calculate_sequential_chunks(
        &self,
        array_shape: &Shape,
        target_elements: usize,
        weights: &[f64],
    ) -> Vec<usize> {
        let ndim = array_shape.ndim();

        if ndim == 0 {
            return Vec::new();
        }

        let mut sizes = vec![1; ndim];
        let mut remaining_elements = target_elements;

        // Prioritize last dimensions (C-order contiguity)
        for i in (0..ndim).rev() {
            let dim_size = array_shape.as_slice()[i];
            let weight = weights.get(i).copied().unwrap_or(1.0);
            let max_for_dim = (remaining_elements as f64 * weight).round() as usize;
            let size = max_for_dim.min(dim_size).max(1);
            sizes[i] = size;

            if size > 0 {
                remaining_elements = remaining_elements.saturating_div(size);
            }
        }

        sizes
    }

    /// Calculates chunks optimized for row-major access
    fn calculate_row_major_chunks(
        &self,
        array_shape: &Shape,
        target_elements: usize,
        weights: &[f64],
    ) -> Vec<usize> {
        // Row-major: last dimension should be largest
        self.calculate_sequential_chunks(array_shape, target_elements, weights)
    }

    /// Calculates chunks optimized for column-major access
    fn calculate_column_major_chunks(
        &self,
        array_shape: &Shape,
        target_elements: usize,
        weights: &[f64],
    ) -> Vec<usize> {
        let ndim = array_shape.ndim();

        if ndim == 0 {
            return Vec::new();
        }

        let mut sizes = vec![1; ndim];
        let mut remaining_elements = target_elements;

        // Prioritize first dimensions (F-order contiguity)
        for i in 0..ndim {
            let dim_size = array_shape.as_slice()[i];
            let weight = weights.get(i).copied().unwrap_or(1.0);
            let max_for_dim = (remaining_elements as f64 * weight).round() as usize;
            let size = max_for_dim.min(dim_size).max(1);
            sizes[i] = size;

            if size > 0 {
                remaining_elements = remaining_elements.saturating_div(size);
            }
        }

        sizes
    }

    /// Calculates chunks for strided access patterns
    fn calculate_strided_chunks(
        &self,
        array_shape: &Shape,
        target_elements: usize,
        stride: usize,
    ) -> Vec<usize> {
        let ndim = array_shape.ndim();

        if ndim == 0 {
            return Vec::new();
        }

        // Align chunks to stride boundaries
        let base_size = (target_elements as f64).powf(1.0 / ndim as f64) as usize;
        let aligned_size = (base_size / stride.max(1)) * stride.max(1);
        let chunk_size = aligned_size.max(stride);

        array_shape
            .as_slice()
            .iter()
            .map(|&dim_size| chunk_size.min(dim_size).max(1))
            .collect()
    }

    /// Validates and adjusts chunk sizes to meet constraints
    fn validate_and_adjust(&self, sizes: Vec<usize>, array_shape: &Shape) -> Result<Vec<usize>> {
        let mut adjusted = sizes;

        for (i, size) in adjusted.iter_mut().enumerate() {
            let dim_size = array_shape
                .dim(i)
                .ok_or_else(|| ZarrError::InvalidDimension {
                    message: format!("Dimension {i} out of bounds"),
                })?;

            // Ensure chunk size doesn't exceed dimension size
            *size = (*size).min(dim_size);

            // Ensure minimum chunk size of 1
            *size = (*size).max(1);
        }

        // Check total chunk size
        let element_count: usize = adjusted.iter().product();
        let min_elements = self.min_chunk_size / 8; // Assume minimum 8 bytes per element
        let max_elements = self.max_chunk_size;

        if element_count < min_elements {
            // Scale up proportionally
            let scale = (min_elements as f64 / element_count as f64).powf(1.0 / adjusted.len() as f64);
            for (i, size) in adjusted.iter_mut().enumerate() {
                let dim_size = array_shape.dim(i).unwrap_or(1);
                *size = ((*size as f64 * scale) as usize).min(dim_size).max(1);
            }
        } else if element_count > max_elements {
            // Scale down proportionally
            let scale = (max_elements as f64 / element_count as f64).powf(1.0 / adjusted.len() as f64);
            for size in adjusted.iter_mut() {
                *size = ((*size as f64 * scale) as usize).max(1);
            }
        }

        Ok(adjusted)
    }
}

// ============================================================================
// Access Pattern Types
// ============================================================================

/// Access pattern hint for chunk optimization
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AccessPattern {
    /// Random access pattern
    #[default]
    Random,

    /// Sequential (streaming) access
    Sequential,

    /// Row-major (C-order) access
    RowMajor,

    /// Column-major (F-order) access
    ColumnMajor,

    /// Strided access with specific stride
    Strided {
        /// Stride value
        stride: usize,
    },
}

// ============================================================================
// Aligned Chunk Configuration
// ============================================================================

/// Configuration for aligned chunk sizes
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlignedChunkConfig {
    /// Alignment for each dimension
    pub alignments: Vec<usize>,

    /// Target chunk size in bytes
    pub target_size: usize,

    /// Whether to round up or down to alignment
    pub round_up: bool,
}

impl AlignedChunkConfig {
    /// Creates a new aligned chunk configuration
    #[must_use]
    pub fn new(alignments: Vec<usize>, target_size: usize) -> Self {
        Self {
            alignments,
            target_size,
            round_up: true,
        }
    }

    /// Calculates aligned chunk sizes
    ///
    /// # Errors
    /// Returns error if dimensions don't match
    pub fn calculate_sizes(&self, array_shape: &Shape, element_size: usize) -> Result<Vec<usize>> {
        let ndim = array_shape.ndim();

        if self.alignments.len() != ndim {
            return Err(ZarrError::Chunk(ChunkError::InvalidChunkShape {
                chunk_shape: self.alignments.clone(),
                array_shape: array_shape.to_vec(),
            }));
        }

        let target_elements = self.target_size / element_size.max(1);
        let base_size = (target_elements as f64).powf(1.0 / ndim as f64) as usize;

        let sizes: Vec<usize> = array_shape
            .as_slice()
            .iter()
            .zip(&self.alignments)
            .map(|(&dim_size, &alignment)| {
                let aligned = if self.round_up {
                    ((base_size + alignment - 1) / alignment.max(1)) * alignment.max(1)
                } else {
                    (base_size / alignment.max(1)) * alignment.max(1)
                };
                aligned.max(alignment).min(dim_size).max(1)
            })
            .collect();

        Ok(sizes)
    }
}

// ============================================================================
// Adaptive Chunk Configuration
// ============================================================================

/// Configuration for adaptive chunking based on data characteristics
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdaptiveChunkConfig {
    /// Base target chunk size
    pub base_target_size: usize,

    /// Minimum chunk size
    pub min_chunk_size: usize,

    /// Maximum chunk size
    pub max_chunk_size: usize,

    /// Data density regions (sparse regions get larger chunks)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub density_map: Option<DensityMap>,

    /// Compression characteristics
    pub compression_profile: CompressionProfile,
}

impl Default for AdaptiveChunkConfig {
    fn default() -> Self {
        Self {
            base_target_size: DEFAULT_TARGET_CHUNK_SIZE,
            min_chunk_size: MIN_CHUNK_SIZE,
            max_chunk_size: MAX_CHUNK_SIZE,
            density_map: None,
            compression_profile: CompressionProfile::default(),
        }
    }
}

impl AdaptiveChunkConfig {
    /// Calculates adaptive chunk sizes
    ///
    /// # Errors
    /// Returns error if calculation fails
    pub fn calculate_sizes(&self, array_shape: &Shape, element_size: usize) -> Result<Vec<usize>> {
        let ndim = array_shape.ndim();

        if ndim == 0 {
            return Err(ZarrError::InvalidDimension {
                message: "Cannot calculate chunks for 0-dimensional array".to_string(),
            });
        }

        // Adjust target size based on compression profile
        let effective_size = self.compression_profile.adjust_target_size(self.base_target_size);
        let target_elements = effective_size / element_size.max(1);

        // Calculate base chunk sizes
        let base_size = (target_elements as f64).powf(1.0 / ndim as f64) as usize;

        let sizes: Vec<usize> = array_shape
            .as_slice()
            .iter()
            .map(|&dim_size| base_size.min(dim_size).max(1))
            .collect();

        // Apply density adjustments if available
        let adjusted = if let Some(ref density_map) = self.density_map {
            density_map.adjust_chunk_sizes(&sizes, array_shape)
        } else {
            sizes
        };

        // Ensure sizes are within bounds
        Ok(adjusted
            .into_iter()
            .zip(array_shape.as_slice())
            .map(|(size, &dim_size)| {
                let min_elements = self.min_chunk_size / element_size.max(1);
                let max_elements = self.max_chunk_size / element_size.max(1);
                size.clamp(min_elements.max(1), max_elements.min(dim_size))
            })
            .collect())
    }
}

// ============================================================================
// Compression Profile
// ============================================================================

/// Profile describing data compression characteristics
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompressionProfile {
    /// Expected compression ratio (0.0-1.0, lower = better compression)
    pub expected_ratio: f64,

    /// Whether data is highly compressible
    pub is_highly_compressible: bool,

    /// Preferred codec for this data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_codec: Option<String>,

    /// Codec-specific parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codec_params: Option<HashMap<String, serde_json::Value>>,
}

impl Default for CompressionProfile {
    fn default() -> Self {
        Self {
            expected_ratio: DEFAULT_COMPRESSION_RATIO,
            is_highly_compressible: false,
            preferred_codec: None,
            codec_params: None,
        }
    }
}

impl CompressionProfile {
    /// Creates a compression profile for highly compressible data
    #[must_use]
    pub fn highly_compressible() -> Self {
        Self {
            expected_ratio: 0.2,
            is_highly_compressible: true,
            preferred_codec: Some("zstd".to_string()),
            codec_params: None,
        }
    }

    /// Creates a compression profile for incompressible data
    #[must_use]
    pub fn incompressible() -> Self {
        Self {
            expected_ratio: 1.0,
            is_highly_compressible: false,
            preferred_codec: Some("null".to_string()),
            codec_params: None,
        }
    }

    /// Adjusts target size based on compression characteristics
    fn adjust_target_size(&self, base_size: usize) -> usize {
        if self.is_highly_compressible {
            // Allow larger chunks for highly compressible data
            (base_size as f64 / self.expected_ratio.max(0.1)) as usize
        } else {
            (base_size as f64 * self.expected_ratio) as usize
        }
    }
}

// ============================================================================
// Density Map
// ============================================================================

/// Map of data density across the array
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DensityMap {
    /// Density values per region
    regions: Vec<DensityRegion>,

    /// Default density for unmapped regions
    default_density: f64,
}

impl DensityMap {
    /// Creates a new density map
    #[must_use]
    pub fn new(default_density: f64) -> Self {
        Self {
            regions: Vec::new(),
            default_density: default_density.clamp(0.0, 1.0),
        }
    }

    /// Adds a density region
    #[must_use]
    pub fn with_region(mut self, region: DensityRegion) -> Self {
        self.regions.push(region);
        self
    }

    /// Adjusts chunk sizes based on density
    fn adjust_chunk_sizes(&self, sizes: &[usize], _array_shape: &Shape) -> Vec<usize> {
        // For now, just return the original sizes
        // A more sophisticated implementation would adjust based on density
        sizes.to_vec()
    }
}

/// A region with specific data density
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DensityRegion {
    /// Start coordinates
    pub start: Vec<usize>,

    /// End coordinates
    pub end: Vec<usize>,

    /// Density value (0.0 = sparse, 1.0 = dense)
    pub density: f64,
}

// ============================================================================
// Chunk Boundary Alignment
// ============================================================================

/// Utilities for chunk boundary alignment
#[derive(Debug, Clone)]
pub struct ChunkBoundaryAligner {
    /// Page size for memory alignment
    page_size: usize,

    /// Cache line size for CPU optimization
    cache_line_size: usize,

    /// SIMD vector width for vectorization
    simd_width: usize,
}

impl Default for ChunkBoundaryAligner {
    fn default() -> Self {
        Self {
            page_size: 4096,
            cache_line_size: 64,
            simd_width: 32, // AVX2
        }
    }
}

impl ChunkBoundaryAligner {
    /// Creates a new aligner with custom settings
    #[must_use]
    pub fn new(page_size: usize, cache_line_size: usize, simd_width: usize) -> Self {
        Self {
            page_size,
            cache_line_size,
            simd_width,
        }
    }

    /// Aligns a chunk size to page boundaries
    #[must_use]
    pub fn align_to_page(&self, size: usize) -> usize {
        self.align_to(size, self.page_size)
    }

    /// Aligns a chunk size to cache line boundaries
    #[must_use]
    pub fn align_to_cache_line(&self, size: usize) -> usize {
        self.align_to(size, self.cache_line_size)
    }

    /// Aligns a chunk size to SIMD boundaries
    #[must_use]
    pub fn align_to_simd(&self, size: usize) -> usize {
        self.align_to(size, self.simd_width)
    }

    /// Aligns a size to a given boundary
    #[must_use]
    pub fn align_to(&self, size: usize, boundary: usize) -> usize {
        if boundary == 0 {
            return size;
        }
        ((size + boundary - 1) / boundary) * boundary
    }

    /// Aligns chunk sizes for optimal memory access
    pub fn align_chunk_sizes(
        &self,
        sizes: &[usize],
        element_size: usize,
        alignment: AlignmentStrategy,
    ) -> Vec<usize> {
        let boundary = match alignment {
            AlignmentStrategy::Page => self.page_size / element_size.max(1),
            AlignmentStrategy::CacheLine => self.cache_line_size / element_size.max(1),
            AlignmentStrategy::Simd => self.simd_width / element_size.max(1),
            AlignmentStrategy::Custom(b) => b / element_size.max(1),
            AlignmentStrategy::None => return sizes.to_vec(),
        };

        sizes
            .iter()
            .map(|&s| self.align_to(s, boundary.max(1)))
            .collect()
    }
}

/// Alignment strategy for chunk boundaries
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignmentStrategy {
    /// No alignment
    None,

    /// Align to page size
    Page,

    /// Align to cache line size
    CacheLine,

    /// Align to SIMD width
    Simd,

    /// Custom alignment boundary
    Custom(usize),
}

// ============================================================================
// Chunk Optimizer
// ============================================================================

/// Optimizer for chunk configuration
pub struct ChunkOptimizer {
    /// Target chunk size in bytes
    target_size: usize,

    /// Codec for compression estimation
    codec: Option<Arc<dyn Codec>>,

    /// Boundary aligner
    aligner: ChunkBoundaryAligner,

    /// Access pattern
    access_pattern: AccessPattern,
}

impl ChunkOptimizer {
    /// Creates a new chunk optimizer
    #[must_use]
    pub fn new(target_size: usize) -> Self {
        Self {
            target_size,
            codec: None,
            aligner: ChunkBoundaryAligner::default(),
            access_pattern: AccessPattern::Random,
        }
    }

    /// Sets the codec for compression estimation
    #[must_use]
    pub fn with_codec(mut self, codec: Arc<dyn Codec>) -> Self {
        self.codec = Some(codec);
        self
    }

    /// Sets the access pattern
    #[must_use]
    pub fn with_access_pattern(mut self, pattern: AccessPattern) -> Self {
        self.access_pattern = pattern;
        self
    }

    /// Sets a custom boundary aligner
    #[must_use]
    pub fn with_aligner(mut self, aligner: ChunkBoundaryAligner) -> Self {
        self.aligner = aligner;
        self
    }

    /// Optimizes chunk sizes for an array
    ///
    /// # Errors
    /// Returns error if optimization fails
    pub fn optimize(
        &self,
        array_shape: &Shape,
        element_size: usize,
    ) -> Result<OptimizedChunkConfig> {
        let config = AutoChunkConfig {
            target_chunk_size: self.target_size,
            access_pattern: self.access_pattern,
            ..Default::default()
        };

        let sizes = config.calculate_sizes(array_shape, element_size)?;

        // Align sizes
        let aligned_sizes = self.aligner.align_chunk_sizes(
            &sizes,
            element_size,
            AlignmentStrategy::CacheLine,
        );

        // Ensure aligned sizes don't exceed dimensions
        let final_sizes: Vec<usize> = aligned_sizes
            .iter()
            .zip(array_shape.as_slice())
            .map(|(&size, &dim)| size.min(dim))
            .collect();

        // Calculate estimated compressed size
        let uncompressed_size: usize = final_sizes.iter().product::<usize>() * element_size;
        let compressed_size = self.estimate_compressed_size(uncompressed_size);

        Ok(OptimizedChunkConfig {
            chunk_sizes: final_sizes,
            estimated_uncompressed_bytes: uncompressed_size,
            estimated_compressed_bytes: compressed_size,
            compression_ratio: compressed_size as f64 / uncompressed_size as f64,
        })
    }

    /// Estimates compressed size for given uncompressed size
    fn estimate_compressed_size(&self, uncompressed_size: usize) -> usize {
        if let Some(ref codec) = self.codec {
            // Use codec's maximum size as an upper bound estimate
            let max_size = codec.max_encoded_size(uncompressed_size);
            // Assume actual compression achieves ~50% of worst case
            (max_size + uncompressed_size) / 2
        } else {
            // Default compression ratio of 50%
            uncompressed_size / 2
        }
    }
}

/// Result of chunk optimization
#[derive(Debug, Clone)]
pub struct OptimizedChunkConfig {
    /// Optimized chunk sizes
    pub chunk_sizes: Vec<usize>,

    /// Estimated uncompressed size per chunk in bytes
    pub estimated_uncompressed_bytes: usize,

    /// Estimated compressed size per chunk in bytes
    pub estimated_compressed_bytes: usize,

    /// Estimated compression ratio
    pub compression_ratio: f64,
}

// ============================================================================
// Chunk Size Calculator
// ============================================================================

/// Calculator for automatic chunk size determination
pub struct ChunkSizeCalculator {
    /// Target memory usage per chunk
    target_memory: usize,

    /// Maximum number of chunks
    max_chunks: Option<usize>,

    /// Minimum elements per dimension
    min_elements_per_dim: usize,
}

impl Default for ChunkSizeCalculator {
    fn default() -> Self {
        Self {
            target_memory: DEFAULT_TARGET_CHUNK_SIZE,
            max_chunks: None,
            min_elements_per_dim: 1,
        }
    }
}

impl ChunkSizeCalculator {
    /// Creates a new calculator with target memory
    #[must_use]
    pub fn new(target_memory: usize) -> Self {
        Self {
            target_memory,
            ..Default::default()
        }
    }

    /// Sets the maximum number of chunks
    #[must_use]
    pub fn with_max_chunks(mut self, max_chunks: usize) -> Self {
        self.max_chunks = Some(max_chunks);
        self
    }

    /// Sets the minimum elements per dimension
    #[must_use]
    pub fn with_min_elements(mut self, min_elements: usize) -> Self {
        self.min_elements_per_dim = min_elements.max(1);
        self
    }

    /// Calculates optimal chunk sizes
    ///
    /// # Errors
    /// Returns error if calculation fails
    pub fn calculate(
        &self,
        array_shape: &Shape,
        element_size: usize,
    ) -> Result<Vec<usize>> {
        let ndim = array_shape.ndim();

        if ndim == 0 {
            return Err(ZarrError::InvalidDimension {
                message: "Cannot calculate chunks for 0-dimensional array".to_string(),
            });
        }

        let target_elements = self.target_memory / element_size.max(1);

        // If max_chunks is specified, adjust target
        let target_elements = if let Some(max_chunks) = self.max_chunks {
            let total_elements = array_shape.size();
            let min_chunk_elements = total_elements / max_chunks;
            target_elements.max(min_chunk_elements)
        } else {
            target_elements
        };

        // Calculate balanced chunk sizes
        let base_size = (target_elements as f64).powf(1.0 / ndim as f64) as usize;

        let sizes: Vec<usize> = array_shape
            .as_slice()
            .iter()
            .map(|&dim_size| {
                base_size
                    .max(self.min_elements_per_dim)
                    .min(dim_size)
            })
            .collect();

        Ok(sizes)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fixed_chunking_strategy() {
        let shape = Shape::new(vec![100, 200, 300]).expect("valid shape");
        let strategy = ChunkingStrategy::fixed(vec![10, 20, 30]);

        let sizes = strategy
            .calculate_chunk_sizes(&shape, 4)
            .expect("valid chunks");

        assert_eq!(sizes, vec![10, 20, 30]);
    }

    #[test]
    fn test_auto_chunking_strategy() {
        let shape = Shape::new(vec![1000, 1000]).expect("valid shape");
        let strategy = ChunkingStrategy::auto_with_target(1024 * 1024); // 1 MB

        let sizes = strategy
            .calculate_chunk_sizes(&shape, 8)
            .expect("valid chunks");

        // Should produce reasonable chunk sizes
        assert!(sizes[0] > 0);
        assert!(sizes[1] > 0);
        assert!(sizes[0] <= 1000);
        assert!(sizes[1] <= 1000);
    }

    #[test]
    fn test_variable_chunk_spec() {
        let spec = VariableChunkSpec::builder()
            .dimension(0, DimensionChunkSpec::fixed(50))
            .dimension(1, DimensionChunkSpec::ratio(0.5))
            .build()
            .expect("valid spec");

        let shape = Shape::new(vec![100, 200]).expect("valid shape");
        let sizes = spec.calculate_sizes(&shape, 4).expect("valid sizes");

        assert_eq!(sizes[0], 50);
        assert_eq!(sizes[1], 100); // 50% of 200
    }

    #[test]
    fn test_dimension_chunk_spec_fixed() {
        let spec = DimensionChunkSpec::fixed(100);
        let size = spec.calculate_for_dimension(1000, 4).expect("valid size");
        assert_eq!(size, 100);
    }

    #[test]
    fn test_dimension_chunk_spec_full() {
        let spec = DimensionChunkSpec::full();
        let size = spec.calculate_for_dimension(500, 4).expect("valid size");
        assert_eq!(size, 500);
    }

    #[test]
    fn test_dimension_chunk_spec_ratio() {
        let spec = DimensionChunkSpec::ratio(0.25);
        let size = spec.calculate_for_dimension(400, 4).expect("valid size");
        assert_eq!(size, 100);
    }

    #[test]
    fn test_auto_chunk_config_balanced() {
        let config = AutoChunkConfig::default();
        let shape = Shape::new(vec![512, 512, 512]).expect("valid shape");

        let sizes = config.calculate_sizes(&shape, 4).expect("valid sizes");

        // Sizes should be roughly equal for balanced config
        let ratio = sizes[0] as f64 / sizes[1] as f64;
        assert!(ratio > 0.5 && ratio < 2.0);
    }

    #[test]
    fn test_aligned_chunk_config() {
        let config = AlignedChunkConfig::new(vec![16, 16], 1024 * 1024);
        let shape = Shape::new(vec![1000, 1000]).expect("valid shape");

        let sizes = config.calculate_sizes(&shape, 4).expect("valid sizes");

        // Sizes should be aligned to 16
        assert_eq!(sizes[0] % 16, 0);
        assert_eq!(sizes[1] % 16, 0);
    }

    #[test]
    fn test_chunk_boundary_aligner() {
        let aligner = ChunkBoundaryAligner::default();

        assert_eq!(aligner.align_to_page(100), 4096);
        assert_eq!(aligner.align_to_page(4096), 4096);
        assert_eq!(aligner.align_to_page(4097), 8192);

        assert_eq!(aligner.align_to_cache_line(50), 64);
        assert_eq!(aligner.align_to_cache_line(64), 64);
        assert_eq!(aligner.align_to_cache_line(65), 128);
    }

    #[test]
    fn test_chunk_optimizer() {
        let optimizer = ChunkOptimizer::new(1024 * 1024);
        let shape = Shape::new(vec![1000, 1000]).expect("valid shape");

        let result = optimizer.optimize(&shape, 8).expect("optimization");

        assert!(result.chunk_sizes[0] > 0);
        assert!(result.chunk_sizes[1] > 0);
        assert!(result.estimated_uncompressed_bytes > 0);
        assert!(result.compression_ratio > 0.0);
        assert!(result.compression_ratio <= 1.0);
    }

    #[test]
    fn test_chunk_size_calculator() {
        let calculator = ChunkSizeCalculator::new(64 * 1024); // 64 KB
        let shape = Shape::new(vec![1000, 1000, 100]).expect("valid shape");

        let sizes = calculator.calculate(&shape, 4).expect("calculation");

        assert!(sizes.iter().all(|&s| s > 0));
        assert!(sizes[0] <= 1000);
        assert!(sizes[1] <= 1000);
        assert!(sizes[2] <= 100);
    }

    #[test]
    fn test_compression_profile() {
        let profile = CompressionProfile::highly_compressible();
        assert!(profile.expected_ratio < 0.5);
        assert!(profile.is_highly_compressible);

        let profile2 = CompressionProfile::incompressible();
        assert_eq!(profile2.expected_ratio, 1.0);
        assert!(!profile2.is_highly_compressible);
    }

    #[test]
    fn test_access_patterns() {
        let shape = Shape::new(vec![1000, 1000]).expect("valid shape");

        let config_random = AutoChunkConfig {
            access_pattern: AccessPattern::Random,
            ..Default::default()
        };
        let sizes_random = config_random
            .calculate_sizes(&shape, 4)
            .expect("random sizes");

        let config_sequential = AutoChunkConfig {
            access_pattern: AccessPattern::Sequential,
            ..Default::default()
        };
        let sizes_sequential = config_sequential
            .calculate_sizes(&shape, 4)
            .expect("sequential sizes");

        // Sequential should favor last dimension
        assert!(sizes_sequential[1] >= sizes_sequential[0]);

        // Both should produce valid sizes
        assert!(sizes_random.iter().all(|&s| s > 0 && s <= 1000));
        assert!(sizes_sequential.iter().all(|&s| s > 0 && s <= 1000));
    }

    #[test]
    fn test_adaptive_chunk_config() {
        let config = AdaptiveChunkConfig::default();
        let shape = Shape::new(vec![500, 500]).expect("valid shape");

        let sizes = config.calculate_sizes(&shape, 4).expect("adaptive sizes");

        assert!(sizes[0] > 0 && sizes[0] <= 500);
        assert!(sizes[1] > 0 && sizes[1] <= 500);
    }

    #[test]
    fn test_density_map() {
        let density_map = DensityMap::new(0.5)
            .with_region(DensityRegion {
                start: vec![0, 0],
                end: vec![100, 100],
                density: 0.9,
            })
            .with_region(DensityRegion {
                start: vec![100, 100],
                end: vec![200, 200],
                density: 0.1,
            });

        assert_eq!(density_map.regions.len(), 2);
        assert_eq!(density_map.default_density, 0.5);
    }

    #[test]
    fn test_invalid_fixed_chunks() {
        let shape = Shape::new(vec![100, 200]).expect("valid shape");
        let strategy = ChunkingStrategy::fixed(vec![10, 20, 30]); // Wrong number of dims

        let result = strategy.calculate_chunk_sizes(&shape, 4);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_variable_sizes() {
        let spec = DimensionChunkSpec::variable(vec![]);
        let result = spec.calculate_for_dimension(100, 4);
        assert!(result.is_err());
    }
}
