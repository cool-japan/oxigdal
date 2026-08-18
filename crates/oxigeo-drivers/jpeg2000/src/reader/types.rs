//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use crate::error::{Jpeg2000Error, Result};
use std::io::{Read, Seek};

use super::types_2::Jpeg2000Reader;

/// Progressive decoder iterator
///
/// Yields increasingly refined image data as quality layers are decoded.
pub struct ProgressiveDecoder<'a, R> {
    pub(super) reader: &'a mut Jpeg2000Reader<R>,
    pub(super) current_layer: u16,
    pub(super) max_layers: u16,
}
impl<'a, R: Read + Seek> ProgressiveDecoder<'a, R> {
    /// Get next quality layer
    pub fn next_layer(&mut self) -> Result<Option<Vec<u8>>> {
        if self.current_layer >= self.max_layers {
            return Ok(None);
        }
        let data = self.reader.decode_quality_layers(self.current_layer)?;
        self.current_layer += 1;
        Ok(Some(data))
    }
    /// Get current layer index
    pub fn current_layer(&self) -> u16 {
        self.current_layer
    }
    /// Get total number of layers
    pub fn total_layers(&self) -> u16 {
        self.max_layers
    }
    /// Get progress as percentage (0.0 - 1.0)
    pub fn progress(&self) -> f64 {
        if self.max_layers == 0 {
            1.0
        } else {
            f64::from(self.current_layer) / f64::from(self.max_layers)
        }
    }
    /// Check if decoding is complete
    pub fn is_complete(&self) -> bool {
        self.current_layer >= self.max_layers
    }
    /// Skip to specific layer
    pub fn skip_to_layer(&mut self, layer: u16) -> Result<Vec<u8>> {
        if layer >= self.max_layers {
            return Err(Jpeg2000Error::Tier2Error(format!(
                "Layer {} exceeds maximum {}",
                layer, self.max_layers
            )));
        }
        self.current_layer = layer;
        self.reader.decode_quality_layers(layer)
    }
}
