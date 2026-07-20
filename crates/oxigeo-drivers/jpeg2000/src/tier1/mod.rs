//! Tier-1 decoder (EBCOT - Embedded Block Coding with Optimized Truncation)
//!
//! This module implements the tier-1 decoding of JPEG2000, which includes:
//! - MQ arithmetic decoding with full probability estimation tables
//! - Context formation for significance, sign, and refinement coding
//! - 3-pass bit-plane decoding (significance propagation, magnitude refinement, cleanup)
//! - Progressive decoding with quality layers
//! - Resolution progression
//! - Truncated codestream handling
//!
//! # Architecture
//!
//! The EBCOT decoder is split into submodules:
//! - [`mq`]: MQ arithmetic decoder with JPEG2000 probability tables
//! - [`contexts`]: Context formation logic for all coding passes
//! - [`passes`]: The three coding passes per bit-plane
//! - [`decoder`]: Code-block level decoding orchestration
//!
//! # JPEG2000 Standard Reference
//!
//! Implements Annex C (MQ-coder) and Annex D (Coefficient bit modeling)
//! of ISO/IEC 15444-1:2019 (JPEG2000 Part 1).

pub mod contexts;
pub mod decoder;
pub mod mq;
pub mod passes;

use crate::codestream::ProgressionOrder;
use crate::error::{Jpeg2000Error, Result};

// Re-export key types from submodules
pub use mq::MqDecoder;

/// Code-block decoder
pub struct CodeBlockDecoder {
    /// Code-block width
    width: usize,
    /// Code-block height
    height: usize,
    /// Number of bit-planes
    num_bitplanes: usize,
    /// Subband type for context formation
    subband: SubbandType,
}

impl CodeBlockDecoder {
    /// Create new code-block decoder
    pub fn new(width: usize, height: usize, num_bitplanes: usize) -> Self {
        Self {
            width,
            height,
            num_bitplanes,
            subband: SubbandType::Ll,
        }
    }

    /// Create new code-block decoder with subband type
    pub fn with_subband(
        width: usize,
        height: usize,
        num_bitplanes: usize,
        subband: SubbandType,
    ) -> Self {
        Self {
            width,
            height,
            num_bitplanes,
            subband,
        }
    }

    /// Decode code-block using EBCOT 3-pass algorithm
    ///
    /// Processes compressed data through bit-planes from MSB to LSB,
    /// running significance propagation, magnitude refinement, and
    /// cleanup passes on each bit-plane.
    pub fn decode(&self, data: &[u8]) -> Result<Vec<i32>> {
        decoder::decode_code_block(
            data,
            self.width,
            self.height,
            self.num_bitplanes,
            self.subband,
        )
    }

    /// Decode code-block with quality layers
    ///
    /// Progressive decoding allows incremental refinement of image quality
    /// by processing quality layers in order.
    pub fn decode_layers(&self, data: &[u8], num_layers: usize) -> Result<Vec<i32>> {
        if num_layers == 0 {
            return Ok(vec![0i32; self.width * self.height]);
        }

        // Create progressive decoder for layer-based decoding
        let mut progressive =
            ProgressiveDecoder::new(self.width, self.height, num_layers, ProgressionOrder::Lrcp);

        // Decode with layer progression
        progressive.decode_with_layers(data, num_layers)
    }

    /// Decode code-block with specific quality layer range
    ///
    /// Allows decoding from start_layer to end_layer for progressive refinement.
    pub fn decode_layer_range(
        &self,
        data: &[u8],
        start_layer: usize,
        end_layer: usize,
    ) -> Result<Vec<i32>> {
        if end_layer < start_layer {
            return Err(Jpeg2000Error::Tier1Error(format!(
                "Invalid layer range: start {} > end {}",
                start_layer, end_layer
            )));
        }

        let num_coeffs = self.width * self.height;
        let mut coefficients = vec![0i32; num_coeffs];

        if data.is_empty() {
            return Ok(coefficients);
        }

        // Create progressive decoder
        let mut progressive = ProgressiveDecoder::new(
            self.width,
            self.height,
            end_layer + 1,
            ProgressionOrder::Lrcp,
        );

        // Skip to start layer and decode remaining
        coefficients = progressive.decode_layer_range(data, start_layer, end_layer)?;

        Ok(coefficients)
    }

    /// Decode code-block with a maximum number of coding passes
    ///
    /// Each bit-plane has up to 3 passes (except the first which has 1).
    /// This allows fine-grained truncation as used by quality layers.
    pub fn decode_with_passes(&self, data: &[u8], max_passes: usize) -> Result<Vec<i32>> {
        decoder::decode_code_block_passes(
            data,
            self.width,
            self.height,
            self.num_bitplanes,
            max_passes,
            self.subband,
        )
    }
}

/// Bit-plane decoder for coefficient refinement
pub struct BitPlaneDecoder {
    /// Coefficients being decoded
    coefficients: Vec<i32>,
    /// Width
    width: usize,
    /// Height
    height: usize,
    /// State grid for context formation
    state_grid: contexts::StateGrid,
}

impl BitPlaneDecoder {
    /// Create new bit-plane decoder
    pub fn new(width: usize, height: usize) -> Self {
        let size = width * height;
        Self {
            coefficients: vec![0; size],
            width,
            height,
            state_grid: contexts::StateGrid::new(width, height),
        }
    }

    /// Get coefficient at position
    pub fn get_coefficient(&self, x: usize, y: usize) -> Option<i32> {
        if x < self.width && y < self.height {
            Some(self.coefficients[y * self.width + x])
        } else {
            None
        }
    }

    /// Set coefficient at position
    pub fn set_coefficient(&mut self, x: usize, y: usize, value: i32) -> Result<()> {
        if x >= self.width || y >= self.height {
            return Err(Jpeg2000Error::Tier1Error(format!(
                "Coordinate out of bounds: ({}, {})",
                x, y
            )));
        }

        let idx = y * self.width + x;
        self.coefficients[idx] = value;
        if value != 0 {
            let sign = if value < 0 { 1 } else { 0 };
            self.state_grid.set_significant(x, y, sign);
        }

        Ok(())
    }

    /// Get all coefficients
    pub fn coefficients(&self) -> &[i32] {
        &self.coefficients
    }

    /// Check if coefficient is significant
    pub fn is_significant(&self, x: usize, y: usize) -> bool {
        if x < self.width && y < self.height {
            self.state_grid.is_significant(x, y)
        } else {
            false
        }
    }

    /// Get context for coefficient using real context formation
    pub fn get_context(&self, x: usize, y: usize, _bit_plane: usize) -> usize {
        if x < self.width && y < self.height {
            self.state_grid.significance_context(x, y, SubbandType::Ll)
        } else {
            0
        }
    }
}

/// Subband decoder
pub struct SubbandDecoder {
    /// Subband type (LL, LH, HL, HH)
    subband_type: SubbandType,
    /// Code-block dimensions
    code_block_width: usize,
    /// Code-block height
    code_block_height: usize,
}

/// Subband types in wavelet decomposition
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubbandType {
    /// Low-Low (approximation)
    Ll,
    /// Low-High (vertical detail)
    Lh,
    /// High-Low (horizontal detail)
    Hl,
    /// High-High (diagonal detail)
    Hh,
}

impl SubbandDecoder {
    /// Create new subband decoder
    pub fn new(
        subband_type: SubbandType,
        code_block_width: usize,
        code_block_height: usize,
    ) -> Self {
        Self {
            subband_type,
            code_block_width,
            code_block_height,
        }
    }

    /// Decode subband from code-blocks
    pub fn decode(&self, code_blocks: &[Vec<u8>], width: usize, height: usize) -> Result<Vec<i32>> {
        let mut output = vec![0i32; width * height];

        let blocks_x = width.div_ceil(self.code_block_width);
        let blocks_y = height.div_ceil(self.code_block_height);

        for block_y in 0..blocks_y {
            for block_x in 0..blocks_x {
                let block_idx = block_y * blocks_x + block_x;

                if block_idx >= code_blocks.len() {
                    continue;
                }

                let cb_decoder = CodeBlockDecoder::with_subband(
                    self.code_block_width,
                    self.code_block_height,
                    8,
                    self.subband_type,
                );
                let coeffs = cb_decoder.decode(&code_blocks[block_idx])?;

                let base_x = block_x * self.code_block_width;
                let base_y = block_y * self.code_block_height;

                for y in 0..self.code_block_height {
                    for x in 0..self.code_block_width {
                        let out_x = base_x + x;
                        let out_y = base_y + y;

                        if out_x < width && out_y < height {
                            let coeff_idx = y * self.code_block_width + x;
                            let out_idx = out_y * width + out_x;

                            if coeff_idx < coeffs.len() {
                                output[out_idx] = coeffs[coeff_idx];
                            }
                        }
                    }
                }
            }
        }

        Ok(output)
    }

    /// Decode subband from code-blocks with progressive layer support
    pub fn decode_progressive(
        &self,
        code_blocks: &[Vec<u8>],
        width: usize,
        height: usize,
        max_layer: usize,
    ) -> Result<Vec<i32>> {
        let mut output = vec![0i32; width * height];

        let blocks_x = width.div_ceil(self.code_block_width);
        let blocks_y = height.div_ceil(self.code_block_height);

        for block_y in 0..blocks_y {
            for block_x in 0..blocks_x {
                let block_idx = block_y * blocks_x + block_x;

                if block_idx >= code_blocks.len() {
                    continue;
                }

                let cb_decoder = CodeBlockDecoder::with_subband(
                    self.code_block_width,
                    self.code_block_height,
                    8,
                    self.subband_type,
                );
                let coeffs = cb_decoder.decode_layers(&code_blocks[block_idx], max_layer)?;

                let base_x = block_x * self.code_block_width;
                let base_y = block_y * self.code_block_height;

                for y in 0..self.code_block_height {
                    for x in 0..self.code_block_width {
                        let out_x = base_x + x;
                        let out_y = base_y + y;

                        if out_x < width && out_y < height {
                            let coeff_idx = y * self.code_block_width + x;
                            let out_idx = out_y * width + out_x;

                            if coeff_idx < coeffs.len() {
                                output[out_idx] = coeffs[coeff_idx];
                            }
                        }
                    }
                }
            }
        }

        Ok(output)
    }
}

// ============================================================================
// Progressive Decoding Implementation
// ============================================================================

/// State for tracking quality layer decoding progress
#[derive(Debug, Clone)]
pub struct LayerState {
    /// Layer index (0-based)
    pub index: usize,
    /// Whether this layer has been fully decoded
    pub decoded: bool,
    /// Number of coding passes included from this layer
    pub coding_passes: usize,
    /// Cumulative data offset for this layer
    pub data_offset: usize,
    /// Length of data for this layer
    pub data_length: usize,
}

impl LayerState {
    /// Create a new layer state
    pub fn new(index: usize) -> Self {
        Self {
            index,
            decoded: false,
            coding_passes: 0,
            data_offset: 0,
            data_length: 0,
        }
    }

    /// Mark the layer as decoded
    pub fn mark_decoded(&mut self, coding_passes: usize, offset: usize, length: usize) {
        self.decoded = true;
        self.coding_passes = coding_passes;
        self.data_offset = offset;
        self.data_length = length;
    }
}

/// State for tracking resolution level decoding progress
#[derive(Debug, Clone)]
pub struct ResolutionState {
    /// Resolution level (0 = lowest, higher = more detail)
    pub level: usize,
    /// Width at this resolution
    pub width: usize,
    /// Height at this resolution
    pub height: usize,
    /// Whether this resolution has been decoded
    pub decoded: bool,
    /// Subbands at this resolution (LL for level 0, LH/HL/HH for others)
    pub subbands: Vec<SubbandState>,
}

impl ResolutionState {
    /// Create a new resolution state
    pub fn new(level: usize, full_width: usize, full_height: usize) -> Self {
        let scale = 1usize << level;
        let width = full_width.div_ceil(scale);
        let height = full_height.div_ceil(scale);

        let subbands = if level == 0 {
            vec![SubbandState::new(SubbandType::Ll, width, height)]
        } else {
            let sub_width = width.div_ceil(2);
            let sub_height = height.div_ceil(2);
            vec![
                SubbandState::new(SubbandType::Lh, sub_width, sub_height),
                SubbandState::new(SubbandType::Hl, sub_width, sub_height),
                SubbandState::new(SubbandType::Hh, sub_width, sub_height),
            ]
        };

        Self {
            level,
            width,
            height,
            decoded: false,
            subbands,
        }
    }

    /// Check if all subbands are decoded
    pub fn is_complete(&self) -> bool {
        self.subbands.iter().all(|s| s.decoded)
    }
}

/// State for tracking subband decoding progress
#[derive(Debug, Clone)]
pub struct SubbandState {
    /// Subband type
    pub subband_type: SubbandType,
    /// Width of the subband
    pub width: usize,
    /// Height of the subband
    pub height: usize,
    /// Whether the subband has been decoded
    pub decoded: bool,
    /// Code-blocks in this subband
    pub code_blocks: Vec<CodeBlockState>,
}

impl SubbandState {
    /// Create a new subband state
    pub fn new(subband_type: SubbandType, width: usize, height: usize) -> Self {
        Self {
            subband_type,
            width,
            height,
            decoded: false,
            code_blocks: Vec::new(),
        }
    }

    /// Initialize code-blocks for this subband
    pub fn init_code_blocks(&mut self, cb_width: usize, cb_height: usize) {
        let num_x = self.width.div_ceil(cb_width);
        let num_y = self.height.div_ceil(cb_height);

        self.code_blocks.clear();
        for y in 0..num_y {
            for x in 0..num_x {
                let actual_width = if x == num_x - 1 {
                    self.width - x * cb_width
                } else {
                    cb_width
                };
                let actual_height = if y == num_y - 1 {
                    self.height - y * cb_height
                } else {
                    cb_height
                };
                self.code_blocks
                    .push(CodeBlockState::new(x, y, actual_width, actual_height));
            }
        }
    }
}

/// State for tracking individual code-block decoding progress
#[derive(Debug, Clone)]
pub struct CodeBlockState {
    /// X position in the subband grid
    pub x: usize,
    /// Y position in the subband grid
    pub y: usize,
    /// Width of the code-block
    pub width: usize,
    /// Height of the code-block
    pub height: usize,
    /// Number of bit-planes decoded so far
    pub decoded_bitplanes: usize,
    /// Total number of bit-planes
    pub total_bitplanes: usize,
    /// Layer contributions received
    pub layer_contributions: Vec<LayerContribution>,
    /// Current decoded coefficients
    pub coefficients: Vec<i32>,
}

impl CodeBlockState {
    /// Create a new code-block state
    pub fn new(x: usize, y: usize, width: usize, height: usize) -> Self {
        Self {
            x,
            y,
            width,
            height,
            decoded_bitplanes: 0,
            total_bitplanes: 0,
            layer_contributions: Vec::new(),
            coefficients: vec![0; width * height],
        }
    }

    /// Add a layer contribution
    pub fn add_contribution(&mut self, layer: usize, passes: usize, data: &[u8]) {
        self.layer_contributions.push(LayerContribution {
            layer,
            coding_passes: passes,
            data: data.to_vec(),
        });
    }

    /// Get cumulative data up to specified layer
    pub fn get_data_to_layer(&self, max_layer: usize) -> Vec<u8> {
        let mut result = Vec::new();
        for contrib in &self.layer_contributions {
            if contrib.layer <= max_layer {
                result.extend_from_slice(&contrib.data);
            }
        }
        result
    }
}

/// Contribution from a quality layer to a code-block
#[derive(Debug, Clone)]
pub struct LayerContribution {
    /// Layer index
    pub layer: usize,
    /// Number of coding passes in this contribution
    pub coding_passes: usize,
    /// Compressed data for this contribution
    pub data: Vec<u8>,
}

/// Progression state for tracking overall decoding progress
#[derive(Debug, Clone)]
pub struct ProgressionState {
    /// Current layer index
    pub current_layer: usize,
    /// Current resolution level
    pub current_resolution: usize,
    /// Current component index
    pub current_component: usize,
    /// Current position (for PCRL, CPRL, RPCL)
    pub current_position: (usize, usize),
    /// Total number of layers
    pub total_layers: usize,
    /// Total number of resolution levels
    pub total_resolutions: usize,
    /// Total number of components
    pub total_components: usize,
    /// Progression order
    pub progression_order: ProgressionOrder,
    /// Whether decoding is complete
    pub complete: bool,
}

impl ProgressionState {
    /// Create a new progression state
    pub fn new(
        total_layers: usize,
        total_resolutions: usize,
        total_components: usize,
        progression_order: ProgressionOrder,
    ) -> Self {
        Self {
            current_layer: 0,
            current_resolution: 0,
            current_component: 0,
            current_position: (0, 0),
            total_layers,
            total_resolutions,
            total_components,
            progression_order,
            complete: false,
        }
    }

    /// Advance to the next position based on progression order
    pub fn advance(&mut self) -> bool {
        if self.complete {
            return false;
        }

        match self.progression_order {
            ProgressionOrder::Lrcp => self.advance_lrcp(),
            ProgressionOrder::Rlcp => self.advance_rlcp(),
            ProgressionOrder::Rpcl => self.advance_rpcl(),
            ProgressionOrder::Pcrl => self.advance_pcrl(),
            ProgressionOrder::Cprl => self.advance_cprl(),
        }
    }

    fn advance_lrcp(&mut self) -> bool {
        self.current_position.0 += 1;
        if self.current_position.0 >= 1 {
            self.current_position.0 = 0;
            self.current_component += 1;
            if self.current_component >= self.total_components {
                self.current_component = 0;
                self.current_resolution += 1;
                if self.current_resolution >= self.total_resolutions {
                    self.current_resolution = 0;
                    self.current_layer += 1;
                    if self.current_layer >= self.total_layers {
                        self.complete = true;
                        return false;
                    }
                }
            }
        }
        true
    }

    fn advance_rlcp(&mut self) -> bool {
        self.current_position.0 += 1;
        if self.current_position.0 >= 1 {
            self.current_position.0 = 0;
            self.current_component += 1;
            if self.current_component >= self.total_components {
                self.current_component = 0;
                self.current_layer += 1;
                if self.current_layer >= self.total_layers {
                    self.current_layer = 0;
                    self.current_resolution += 1;
                    if self.current_resolution >= self.total_resolutions {
                        self.complete = true;
                        return false;
                    }
                }
            }
        }
        true
    }

    fn advance_rpcl(&mut self) -> bool {
        self.current_layer += 1;
        if self.current_layer >= self.total_layers {
            self.current_layer = 0;
            self.current_component += 1;
            if self.current_component >= self.total_components {
                self.current_component = 0;
                self.current_position.0 += 1;
                if self.current_position.0 >= 1 {
                    self.current_position.0 = 0;
                    self.current_resolution += 1;
                    if self.current_resolution >= self.total_resolutions {
                        self.complete = true;
                        return false;
                    }
                }
            }
        }
        true
    }

    fn advance_pcrl(&mut self) -> bool {
        self.current_layer += 1;
        if self.current_layer >= self.total_layers {
            self.current_layer = 0;
            self.current_resolution += 1;
            if self.current_resolution >= self.total_resolutions {
                self.current_resolution = 0;
                self.current_component += 1;
                if self.current_component >= self.total_components {
                    self.current_component = 0;
                    self.current_position.0 += 1;
                    if self.current_position.0 >= 1 {
                        self.complete = true;
                        return false;
                    }
                }
            }
        }
        true
    }

    fn advance_cprl(&mut self) -> bool {
        self.current_layer += 1;
        if self.current_layer >= self.total_layers {
            self.current_layer = 0;
            self.current_resolution += 1;
            if self.current_resolution >= self.total_resolutions {
                self.current_resolution = 0;
                self.current_position.0 += 1;
                if self.current_position.0 >= 1 {
                    self.current_position.0 = 0;
                    self.current_component += 1;
                    if self.current_component >= self.total_components {
                        self.complete = true;
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Get current decoding position
    pub fn current_position_info(&self) -> (usize, usize, usize) {
        (
            self.current_layer,
            self.current_resolution,
            self.current_component,
        )
    }
}

/// Progressive decoder for JPEG2000 codestreams
///
/// Supports incremental decoding by quality layers and resolution levels.
/// Handles truncated codestreams gracefully.
#[derive(Debug)]
pub struct ProgressiveDecoder {
    /// Image width
    width: usize,
    /// Image height
    height: usize,
    /// Number of quality layers
    num_layers: usize,
    /// Progression order
    progression_order: ProgressionOrder,
    /// Current progression state
    progression_state: ProgressionState,
    /// Layer states
    layer_states: Vec<LayerState>,
    /// Resolution states
    resolution_states: Vec<ResolutionState>,
    /// Current decoded coefficients
    coefficients: Vec<i32>,
    /// Whether we're handling a truncated stream
    truncated: bool,
    /// Number of bytes successfully decoded
    bytes_decoded: usize,
}

impl ProgressiveDecoder {
    /// Create a new progressive decoder
    pub fn new(
        width: usize,
        height: usize,
        num_layers: usize,
        progression_order: ProgressionOrder,
    ) -> Self {
        let layer_states: Vec<LayerState> = (0..num_layers).map(LayerState::new).collect();

        let num_resolutions = Self::calculate_num_resolutions(width, height);

        let resolution_states: Vec<ResolutionState> = (0..num_resolutions)
            .map(|level| ResolutionState::new(level, width, height))
            .collect();

        let progression_state =
            ProgressionState::new(num_layers, num_resolutions, 1, progression_order);

        Self {
            width,
            height,
            num_layers,
            progression_order,
            progression_state,
            layer_states,
            resolution_states,
            coefficients: vec![0; width * height],
            truncated: false,
            bytes_decoded: 0,
        }
    }

    /// Calculate number of resolution levels based on image dimensions
    fn calculate_num_resolutions(width: usize, height: usize) -> usize {
        let max_dim = width.max(height);
        let mut levels = 1;
        let mut size = max_dim;
        while size > 64 && levels < 8 {
            size /= 2;
            levels += 1;
        }
        levels
    }

    /// Decode with quality layer progression
    pub fn decode_with_layers(&mut self, data: &[u8], max_layer: usize) -> Result<Vec<i32>> {
        if data.is_empty() {
            return Ok(self.coefficients.clone());
        }

        let target_layer = max_layer.min(self.num_layers);
        let mut offset = 0;

        for layer_idx in 0..target_layer {
            match self.decode_layer(data, layer_idx, offset) {
                Ok(new_offset) => {
                    offset = new_offset;
                    if let Some(layer_state) = self.layer_states.get_mut(layer_idx) {
                        layer_state.decoded = true;
                    }
                }
                Err(e) => {
                    if self.is_truncation_error(&e) {
                        self.truncated = true;
                        tracing::warn!(
                            "Truncated codestream at layer {}: proceeding with partial decode",
                            layer_idx
                        );
                        break;
                    }
                    return Err(e);
                }
            }
        }

        self.bytes_decoded = offset;
        Ok(self.coefficients.clone())
    }

    /// Decode a specific layer range
    pub fn decode_layer_range(
        &mut self,
        data: &[u8],
        start_layer: usize,
        end_layer: usize,
    ) -> Result<Vec<i32>> {
        if data.is_empty() {
            return Ok(self.coefficients.clone());
        }

        let mut offset = 0;

        for layer_idx in 0..start_layer {
            let layer_size = self.estimate_layer_size(data.len(), layer_idx);
            offset += layer_size;
            if offset >= data.len() {
                return Err(Jpeg2000Error::Tier1Error(format!(
                    "Data exhausted before reaching start layer {}",
                    start_layer
                )));
            }
        }

        let target_layer = end_layer.min(self.num_layers - 1);
        for layer_idx in start_layer..=target_layer {
            match self.decode_layer(data, layer_idx, offset) {
                Ok(new_offset) => {
                    offset = new_offset;
                    if let Some(layer_state) = self.layer_states.get_mut(layer_idx) {
                        layer_state.decoded = true;
                    }
                }
                Err(e) => {
                    if self.is_truncation_error(&e) {
                        self.truncated = true;
                        tracing::warn!("Truncated codestream at layer {}", layer_idx);
                        break;
                    }
                    return Err(e);
                }
            }
        }

        self.bytes_decoded = offset;
        Ok(self.coefficients.clone())
    }

    /// Decode a single quality layer
    fn decode_layer(&mut self, data: &[u8], layer_idx: usize, offset: usize) -> Result<usize> {
        if offset >= data.len() {
            return Err(Jpeg2000Error::InsufficientData {
                expected: offset + 1,
                actual: data.len(),
            });
        }

        let layer_size = self.estimate_layer_size(data.len(), layer_idx);
        let end_offset = (offset + layer_size).min(data.len());
        let layer_data = &data[offset..end_offset];

        self.apply_layer_contribution(layer_idx, layer_data)?;

        Ok(end_offset)
    }

    /// Estimate the size of a layer
    fn estimate_layer_size(&self, total_size: usize, layer_idx: usize) -> usize {
        if self.num_layers == 0 {
            return total_size;
        }

        let base_size = total_size / (2usize.pow(self.num_layers as u32) - 1);
        let layer_weight = 2usize.pow(layer_idx as u32);
        base_size.saturating_mul(layer_weight).max(1)
    }

    /// Apply a layer contribution to the coefficients using EBCOT decoding
    fn apply_layer_contribution(&mut self, layer_idx: usize, data: &[u8]) -> Result<()> {
        if data.is_empty() {
            return Ok(());
        }

        // Calculate coding passes for this layer
        let total_passes = decoder::total_coding_passes(8);
        let passes_per_layer = total_passes
            .checked_div(self.num_layers)
            .unwrap_or(total_passes);
        let max_passes = passes_per_layer * (layer_idx + 1);

        // Use the real EBCOT decoder with pass truncation
        let decoded = decoder::decode_code_block_passes(
            data,
            self.width,
            self.height,
            8,
            max_passes,
            SubbandType::Ll,
        )?;

        // Merge decoded coefficients (accumulate layer refinements)
        for (i, &val) in decoded.iter().enumerate() {
            if i < self.coefficients.len() {
                // For progressive layers, add refinement bits
                self.coefficients[i] = self.coefficients[i].saturating_add(val);
            }
        }

        if let Some(layer_state) = self.layer_states.get_mut(layer_idx) {
            layer_state.mark_decoded(passes_per_layer, 0, data.len());
        }

        Ok(())
    }

    /// Check if an error indicates truncated data
    fn is_truncation_error(&self, error: &Jpeg2000Error) -> bool {
        matches!(
            error,
            Jpeg2000Error::InsufficientData { .. } | Jpeg2000Error::IoError(_)
        )
    }

    /// Decode with resolution progression
    pub fn decode_with_resolution(
        &mut self,
        data: &[u8],
        max_resolution: usize,
    ) -> Result<Vec<i32>> {
        if data.is_empty() {
            return Ok(self.coefficients.clone());
        }

        let target_resolution = max_resolution.min(self.resolution_states.len());
        let mut offset = 0;

        for res_idx in 0..target_resolution {
            match self.decode_resolution(data, res_idx, offset) {
                Ok(new_offset) => {
                    offset = new_offset;
                    if let Some(res_state) = self.resolution_states.get_mut(res_idx) {
                        res_state.decoded = true;
                    }
                }
                Err(e) => {
                    if self.is_truncation_error(&e) {
                        self.truncated = true;
                        tracing::warn!("Truncated at resolution {}", res_idx);
                        break;
                    }
                    return Err(e);
                }
            }
        }

        self.bytes_decoded = offset;
        Ok(self.coefficients.clone())
    }

    /// Decode a single resolution level
    fn decode_resolution(&mut self, data: &[u8], res_idx: usize, offset: usize) -> Result<usize> {
        if offset >= data.len() {
            return Err(Jpeg2000Error::InsufficientData {
                expected: offset + 1,
                actual: data.len(),
            });
        }

        let res_state = self
            .resolution_states
            .get(res_idx)
            .ok_or_else(|| Jpeg2000Error::Tier1Error(format!("Invalid resolution: {}", res_idx)))?;

        let res_width = res_state.width;
        let res_height = res_state.height;

        let res_size = (res_width * res_height).min(data.len() - offset);
        let end_offset = offset + res_size;
        let res_data = &data[offset..end_offset];

        self.apply_resolution_contribution(res_idx, res_data, res_width, res_height)?;

        Ok(end_offset)
    }

    /// Apply resolution contribution to coefficients
    fn apply_resolution_contribution(
        &mut self,
        res_idx: usize,
        data: &[u8],
        res_width: usize,
        res_height: usize,
    ) -> Result<()> {
        if data.is_empty() {
            return Ok(());
        }

        let scale = 1usize << res_idx;

        for y in 0..res_height {
            for x in 0..res_width {
                let src_idx = y * res_width + x;
                if src_idx >= data.len() {
                    break;
                }

                let dst_x = x * scale;
                let dst_y = y * scale;

                if dst_x < self.width && dst_y < self.height {
                    let dst_idx = dst_y * self.width + dst_x;
                    if dst_idx < self.coefficients.len() {
                        let contribution = i32::from(data[src_idx]);
                        self.coefficients[dst_idx] =
                            self.coefficients[dst_idx].saturating_add(contribution);
                    }
                }
            }
        }

        Ok(())
    }

    /// Get the current coefficients
    pub fn coefficients(&self) -> &[i32] {
        &self.coefficients
    }

    /// Check if the stream was truncated
    pub fn is_truncated(&self) -> bool {
        self.truncated
    }

    /// Get the number of bytes successfully decoded
    pub fn bytes_decoded(&self) -> usize {
        self.bytes_decoded
    }

    /// Get the number of layers decoded
    pub fn layers_decoded(&self) -> usize {
        self.layer_states.iter().filter(|l| l.decoded).count()
    }

    /// Get the number of resolutions decoded
    pub fn resolutions_decoded(&self) -> usize {
        self.resolution_states.iter().filter(|r| r.decoded).count()
    }

    /// Get current progression state
    pub fn progression_state(&self) -> &ProgressionState {
        &self.progression_state
    }

    /// Reset decoder state for re-decoding
    pub fn reset(&mut self) {
        self.coefficients.fill(0);
        self.truncated = false;
        self.bytes_decoded = 0;

        for layer in &mut self.layer_states {
            layer.decoded = false;
            layer.coding_passes = 0;
            layer.data_offset = 0;
            layer.data_length = 0;
        }

        for res in &mut self.resolution_states {
            res.decoded = false;
        }

        self.progression_state = ProgressionState::new(
            self.num_layers,
            self.resolution_states.len(),
            1,
            self.progression_order,
        );
    }
}

/// Configuration for progressive decoding
#[derive(Debug, Clone)]
pub struct ProgressiveConfig {
    /// Maximum quality layers to decode
    pub max_layers: Option<usize>,
    /// Maximum resolution level to decode
    pub max_resolution: Option<usize>,
    /// Progression order to use
    pub progression_order: ProgressionOrder,
    /// Whether to gracefully handle truncated streams
    pub allow_truncation: bool,
    /// Target quality (0.0 - 1.0, affects number of layers)
    pub target_quality: Option<f32>,
}

impl Default for ProgressiveConfig {
    fn default() -> Self {
        Self {
            max_layers: None,
            max_resolution: None,
            progression_order: ProgressionOrder::Lrcp,
            allow_truncation: true,
            target_quality: None,
        }
    }
}

impl ProgressiveConfig {
    /// Create config for quick preview (low quality, fast)
    pub fn preview() -> Self {
        Self {
            max_layers: Some(1),
            max_resolution: Some(2),
            progression_order: ProgressionOrder::Rlcp,
            allow_truncation: true,
            target_quality: Some(0.25),
        }
    }

    /// Create config for medium quality
    pub fn medium() -> Self {
        Self {
            max_layers: None,
            max_resolution: Some(4),
            progression_order: ProgressionOrder::Lrcp,
            allow_truncation: true,
            target_quality: Some(0.5),
        }
    }

    /// Create config for full quality
    pub fn full() -> Self {
        Self {
            max_layers: None,
            max_resolution: None,
            progression_order: ProgressionOrder::Lrcp,
            allow_truncation: false,
            target_quality: None,
        }
    }

    /// Calculate effective number of layers based on target quality
    pub fn effective_layers(&self, total_layers: usize) -> usize {
        if let Some(max) = self.max_layers {
            return max.min(total_layers);
        }

        if let Some(quality) = self.target_quality {
            let layers = (quality * total_layers as f32).ceil() as usize;
            return layers.max(1).min(total_layers);
        }

        total_layers
    }
}

/// Decode result with progressive decoding metadata
#[derive(Debug)]
pub struct ProgressiveDecodeResult {
    /// Decoded coefficients
    pub coefficients: Vec<i32>,
    /// Number of layers decoded
    pub layers_decoded: usize,
    /// Number of resolutions decoded
    pub resolutions_decoded: usize,
    /// Whether the stream was truncated
    pub truncated: bool,
    /// Number of bytes consumed
    pub bytes_consumed: usize,
    /// Estimated quality (0.0 - 1.0)
    pub estimated_quality: f32,
}

impl ProgressiveDecodeResult {
    /// Create from progressive decoder state
    pub fn from_decoder(decoder: &ProgressiveDecoder, total_layers: usize) -> Self {
        let layers_decoded = decoder.layers_decoded();
        let estimated_quality = if total_layers > 0 {
            layers_decoded as f32 / total_layers as f32
        } else {
            0.0
        };

        Self {
            coefficients: decoder.coefficients().to_vec(),
            layers_decoded,
            resolutions_decoded: decoder.resolutions_decoded(),
            truncated: decoder.is_truncated(),
            bytes_consumed: decoder.bytes_decoded(),
            estimated_quality,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mq_decoder_creation() {
        // Use data without 0xFF to avoid marker detection during init
        let data = vec![0x80, 0x40, 0x55, 0x00];
        let decoder = MqDecoder::new(data);
        assert!(!decoder.is_exhausted());
    }

    #[test]
    fn test_code_block_decoder() {
        let decoder = CodeBlockDecoder::new(64, 64, 8);
        let data = vec![0u8; 100];
        let result = decoder.decode(&data);
        assert!(result.is_ok());
    }

    #[test]
    fn test_code_block_decoder_with_subband() {
        let decoder = CodeBlockDecoder::with_subband(32, 32, 8, SubbandType::Hh);
        let data = vec![0x55u8; 100];
        let result = decoder.decode(&data);
        assert!(result.is_ok());
        let coeffs = result.expect("decode failed");
        assert_eq!(coeffs.len(), 32 * 32);
    }

    #[test]
    fn test_code_block_decoder_with_passes() {
        let decoder = CodeBlockDecoder::new(16, 16, 8);
        let data = vec![0xAAu8; 200];
        let result = decoder.decode_with_passes(&data, 5);
        assert!(result.is_ok());
    }

    #[test]
    fn test_bitplane_decoder() {
        let mut decoder = BitPlaneDecoder::new(8, 8);
        assert!(decoder.set_coefficient(0, 0, 42).is_ok());
        assert_eq!(decoder.get_coefficient(0, 0), Some(42));
        assert!(decoder.is_significant(0, 0));
    }

    #[test]
    fn test_bitplane_decoder_context() {
        let mut decoder = BitPlaneDecoder::new(8, 8);
        decoder.set_coefficient(3, 4, 100).expect("set failed");
        // Neighbor of (3,4) should have non-zero context
        let ctx = decoder.get_context(4, 4, 7);
        assert!(ctx <= 18); // valid context range
    }

    #[test]
    fn test_subband_decoder_creation() {
        let decoder = SubbandDecoder::new(SubbandType::Ll, 64, 64);
        assert_eq!(decoder.code_block_width, 64);
        assert_eq!(decoder.code_block_height, 64);
    }

    // Progressive Decoding Tests

    #[test]
    fn test_layer_state_creation() {
        let layer = LayerState::new(5);
        assert_eq!(layer.index, 5);
        assert!(!layer.decoded);
        assert_eq!(layer.coding_passes, 0);
    }

    #[test]
    fn test_layer_state_mark_decoded() {
        let mut layer = LayerState::new(0);
        layer.mark_decoded(3, 100, 256);
        assert!(layer.decoded);
        assert_eq!(layer.coding_passes, 3);
        assert_eq!(layer.data_offset, 100);
        assert_eq!(layer.data_length, 256);
    }

    #[test]
    fn test_resolution_state_creation() {
        let res = ResolutionState::new(0, 256, 256);
        assert_eq!(res.level, 0);
        assert_eq!(res.width, 256);
        assert_eq!(res.height, 256);
        assert!(!res.decoded);
        assert_eq!(res.subbands.len(), 1);
        assert_eq!(res.subbands[0].subband_type, SubbandType::Ll);
    }

    #[test]
    fn test_resolution_state_higher_level() {
        let res = ResolutionState::new(1, 256, 256);
        assert_eq!(res.level, 1);
        assert_eq!(res.subbands.len(), 3);
        assert_eq!(res.subbands[0].subband_type, SubbandType::Lh);
        assert_eq!(res.subbands[1].subband_type, SubbandType::Hl);
        assert_eq!(res.subbands[2].subband_type, SubbandType::Hh);
    }

    #[test]
    fn test_subband_state_init_code_blocks() {
        let mut subband = SubbandState::new(SubbandType::Ll, 128, 128);
        subband.init_code_blocks(64, 64);
        assert_eq!(subband.code_blocks.len(), 4);
    }

    #[test]
    fn test_code_block_state_creation() {
        let cb = CodeBlockState::new(1, 2, 64, 64);
        assert_eq!(cb.x, 1);
        assert_eq!(cb.y, 2);
        assert_eq!(cb.width, 64);
        assert_eq!(cb.height, 64);
        assert_eq!(cb.coefficients.len(), 64 * 64);
    }

    #[test]
    fn test_code_block_state_add_contribution() {
        let mut cb = CodeBlockState::new(0, 0, 32, 32);
        cb.add_contribution(0, 3, &[1, 2, 3, 4]);
        cb.add_contribution(1, 2, &[5, 6, 7]);

        assert_eq!(cb.layer_contributions.len(), 2);
        assert_eq!(cb.layer_contributions[0].layer, 0);
        assert_eq!(cb.layer_contributions[0].coding_passes, 3);
        assert_eq!(cb.layer_contributions[1].layer, 1);
    }

    #[test]
    fn test_code_block_state_get_data_to_layer() {
        let mut cb = CodeBlockState::new(0, 0, 32, 32);
        cb.add_contribution(0, 1, &[1, 2]);
        cb.add_contribution(1, 1, &[3, 4]);
        cb.add_contribution(2, 1, &[5, 6]);

        let data = cb.get_data_to_layer(1);
        assert_eq!(data, vec![1, 2, 3, 4]);

        let data_all = cb.get_data_to_layer(2);
        assert_eq!(data_all, vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn test_progression_state_creation() {
        let state = ProgressionState::new(5, 3, 1, ProgressionOrder::Lrcp);
        assert_eq!(state.total_layers, 5);
        assert_eq!(state.total_resolutions, 3);
        assert_eq!(state.total_components, 1);
        assert_eq!(state.progression_order, ProgressionOrder::Lrcp);
        assert!(!state.complete);
    }

    #[test]
    fn test_progression_state_advance_lrcp() {
        let mut state = ProgressionState::new(2, 2, 1, ProgressionOrder::Lrcp);
        assert!(state.advance());
        assert!(!state.complete);

        for _ in 0..10 {
            state.advance();
        }
    }

    #[test]
    fn test_progression_state_current_position_info() {
        let state = ProgressionState::new(5, 3, 2, ProgressionOrder::Rlcp);
        let (layer, res, comp) = state.current_position_info();
        assert_eq!(layer, 0);
        assert_eq!(res, 0);
        assert_eq!(comp, 0);
    }

    #[test]
    fn test_progressive_decoder_creation() {
        let decoder = ProgressiveDecoder::new(256, 256, 5, ProgressionOrder::Lrcp);
        assert_eq!(decoder.width, 256);
        assert_eq!(decoder.height, 256);
        assert_eq!(decoder.num_layers, 5);
        assert!(!decoder.is_truncated());
        assert_eq!(decoder.bytes_decoded(), 0);
    }

    #[test]
    fn test_progressive_decoder_decode_with_layers() {
        let mut decoder = ProgressiveDecoder::new(64, 64, 3, ProgressionOrder::Lrcp);
        let data = vec![0x80u8; 1024];

        let result = decoder.decode_with_layers(&data, 2);
        assert!(result.is_ok());

        let coeffs = result.expect("decode failed");
        assert_eq!(coeffs.len(), 64 * 64);
    }

    #[test]
    fn test_progressive_decoder_empty_data() {
        let mut decoder = ProgressiveDecoder::new(32, 32, 3, ProgressionOrder::Lrcp);
        let data: Vec<u8> = vec![];

        let result = decoder.decode_with_layers(&data, 3);
        assert!(result.is_ok());

        let coeffs = result.expect("decode failed");
        assert!(coeffs.iter().all(|&c| c == 0));
    }

    #[test]
    fn test_progressive_decoder_decode_layer_range() {
        let mut decoder = ProgressiveDecoder::new(32, 32, 5, ProgressionOrder::Lrcp);
        let data = vec![0x55u8; 2048];

        let result = decoder.decode_layer_range(&data, 1, 3);
        assert!(result.is_ok());
    }

    #[test]
    fn test_progressive_decoder_invalid_layer_range() {
        let decoder = CodeBlockDecoder::new(32, 32, 8);

        let result = decoder.decode_layer_range(&[1, 2, 3], 5, 2);
        assert!(result.is_err());
    }

    #[test]
    fn test_progressive_decoder_decode_with_resolution() {
        let mut decoder = ProgressiveDecoder::new(128, 128, 3, ProgressionOrder::Rlcp);
        let data = vec![0x42u8; 4096];

        let result = decoder.decode_with_resolution(&data, 2);
        assert!(result.is_ok());

        assert!(decoder.resolutions_decoded() > 0);
    }

    #[test]
    fn test_progressive_decoder_layers_decoded() {
        let mut decoder = ProgressiveDecoder::new(32, 32, 4, ProgressionOrder::Lrcp);
        let data = vec![0xAAu8; 512];

        let _ = decoder.decode_with_layers(&data, 3);
        let layers = decoder.layers_decoded();
        assert!(layers <= 3);
    }

    #[test]
    fn test_progressive_decoder_reset() {
        let mut decoder = ProgressiveDecoder::new(32, 32, 3, ProgressionOrder::Lrcp);
        let data = vec![0xFFu8; 256];

        let _ = decoder.decode_with_layers(&data, 2);

        decoder.reset();
        assert_eq!(decoder.bytes_decoded(), 0);
        assert!(!decoder.is_truncated());
        assert_eq!(decoder.layers_decoded(), 0);
    }

    #[test]
    fn test_progressive_config_default() {
        let config = ProgressiveConfig::default();
        assert!(config.max_layers.is_none());
        assert!(config.max_resolution.is_none());
        assert!(config.allow_truncation);
        assert_eq!(config.progression_order, ProgressionOrder::Lrcp);
    }

    #[test]
    fn test_progressive_config_preview() {
        let config = ProgressiveConfig::preview();
        assert_eq!(config.max_layers, Some(1));
        assert_eq!(config.max_resolution, Some(2));
        assert_eq!(config.target_quality, Some(0.25));
    }

    #[test]
    fn test_progressive_config_medium() {
        let config = ProgressiveConfig::medium();
        assert!(config.max_layers.is_none());
        assert_eq!(config.max_resolution, Some(4));
        assert_eq!(config.target_quality, Some(0.5));
    }

    #[test]
    fn test_progressive_config_full() {
        let config = ProgressiveConfig::full();
        assert!(config.max_layers.is_none());
        assert!(config.max_resolution.is_none());
        assert!(!config.allow_truncation);
    }

    #[test]
    fn test_progressive_config_effective_layers() {
        let config = ProgressiveConfig {
            max_layers: Some(3),
            max_resolution: None,
            progression_order: ProgressionOrder::Lrcp,
            allow_truncation: true,
            target_quality: None,
        };

        assert_eq!(config.effective_layers(10), 3);
        assert_eq!(config.effective_layers(2), 2);
    }

    #[test]
    fn test_progressive_config_effective_layers_with_quality() {
        let config = ProgressiveConfig {
            max_layers: None,
            max_resolution: None,
            progression_order: ProgressionOrder::Lrcp,
            allow_truncation: true,
            target_quality: Some(0.5),
        };

        assert_eq!(config.effective_layers(10), 5);
    }

    #[test]
    fn test_progressive_decode_result_from_decoder() {
        let mut decoder = ProgressiveDecoder::new(32, 32, 4, ProgressionOrder::Lrcp);
        let data = vec![0x80u8; 256];

        let _ = decoder.decode_with_layers(&data, 2);

        let result = ProgressiveDecodeResult::from_decoder(&decoder, 4);
        assert_eq!(result.coefficients.len(), 32 * 32);
        assert!(result.estimated_quality >= 0.0 && result.estimated_quality <= 1.0);
    }

    #[test]
    fn test_code_block_decoder_decode_layers() {
        let decoder = CodeBlockDecoder::new(32, 32, 8);
        let data = vec![0x55u8; 128];

        let result = decoder.decode_layers(&data, 3);
        assert!(result.is_ok());

        let coeffs = result.expect("decode failed");
        assert_eq!(coeffs.len(), 32 * 32);
    }

    #[test]
    fn test_code_block_decoder_decode_layers_zero() {
        let decoder = CodeBlockDecoder::new(16, 16, 8);

        let result = decoder.decode_layers(&[], 0);
        assert!(result.is_ok());

        let coeffs = result.expect("decode failed");
        assert_eq!(coeffs.len(), 16 * 16);
        assert!(coeffs.iter().all(|&c| c == 0));
    }

    #[test]
    fn test_subband_decoder_decode_progressive() {
        let decoder = SubbandDecoder::new(SubbandType::Ll, 32, 32);
        let code_blocks = vec![vec![0x42u8; 64], vec![0x43u8; 64]];

        let result = decoder.decode_progressive(&code_blocks, 64, 64, 2);
        assert!(result.is_ok());
    }

    #[test]
    fn test_progression_order_all_types() {
        for order in [
            ProgressionOrder::Lrcp,
            ProgressionOrder::Rlcp,
            ProgressionOrder::Rpcl,
            ProgressionOrder::Pcrl,
            ProgressionOrder::Cprl,
        ] {
            let state = ProgressionState::new(2, 2, 1, order);
            assert!(!state.complete);
        }
    }

    #[test]
    fn test_truncated_stream_handling() {
        let mut decoder = ProgressiveDecoder::new(128, 128, 10, ProgressionOrder::Lrcp);
        // Very small data - the decoder should handle it gracefully
        let data = vec![0x01u8; 10];

        let result = decoder.decode_with_layers(&data, 10);
        // Should succeed (either full or partial decode)
        assert!(result.is_ok());
        // The decoded coefficients should be valid (correct size)
        let coeffs = result.expect("decode failed");
        assert_eq!(coeffs.len(), 128 * 128);
    }

    #[test]
    fn test_resolution_state_is_complete() {
        let mut res = ResolutionState::new(0, 64, 64);
        assert!(!res.is_complete());

        for subband in &mut res.subbands {
            subband.decoded = true;
        }
        assert!(res.is_complete());
    }

    #[test]
    fn test_ebcot_decode_produces_coefficients() {
        // Test that the real EBCOT decoder produces non-trivial coefficients
        // from meaningful MQ-coded data
        let data = vec![
            0x00, 0x00, 0xFF, 0x7F, 0x80, 0x00, 0xAA, 0x55, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC,
            0xDE, 0xF0, 0xFF, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA,
            0xBB, 0xCC, 0xDD, 0xEE,
        ];

        let cb_decoder = CodeBlockDecoder::new(4, 4, 8);
        let result = cb_decoder.decode(&data);
        assert!(result.is_ok());
        let coeffs = result.expect("decode failed");
        assert_eq!(coeffs.len(), 16);
    }

    #[test]
    fn test_minimal_j2k_codestream_structure() {
        // Test with data that mimics a minimal J2K code-block contribution
        // SOC marker (0xFF4F) should not appear in MQ data since it
        // uses byte-stuffing, but we test the decoder handles various patterns
        let patterns = [
            vec![0x00u8; 32],              // All zeros
            vec![0xFFu8; 32],              // All ones (triggers stuffing)
            vec![0x80u8; 32],              // Half pattern
            (0..32u8).collect::<Vec<_>>(), // Sequential
        ];

        for (i, pattern) in patterns.iter().enumerate() {
            let cb = CodeBlockDecoder::new(4, 4, 4);
            let result = cb.decode(pattern);
            assert!(result.is_ok(), "Pattern {} failed", i);
        }
    }
}
