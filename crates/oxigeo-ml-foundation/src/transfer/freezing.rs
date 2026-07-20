//! Layer freezing strategies for transfer learning.

use crate::transfer::FreezingConfig;
use crate::{Error, Result};

/// Layer freezing manager.
#[derive(Debug)]
pub struct LayerFreezer {
    /// Freezing configuration
    config: FreezingConfig,
    /// Total number of layers
    total_layers: usize,
}

impl LayerFreezer {
    /// Creates a new layer freezer.
    pub fn new(config: FreezingConfig, total_layers: usize) -> Result<Self> {
        config.validate()?;

        if total_layers == 0 {
            return Err(Error::invalid_parameter(
                "total_layers",
                total_layers,
                "must be positive",
            ));
        }

        Ok(Self {
            config,
            total_layers,
        })
    }

    /// Checks if a layer should be frozen.
    pub fn is_layer_frozen(&self, layer_idx: usize) -> bool {
        if layer_idx >= self.total_layers {
            return false;
        }
        self.config.is_frozen(layer_idx)
    }

    /// Gets the list of frozen layer indices.
    pub fn frozen_layer_indices(&self) -> Vec<usize> {
        (0..self.total_layers)
            .filter(|&i| self.is_layer_frozen(i))
            .collect()
    }

    /// Gets the list of trainable layer indices.
    pub fn trainable_layer_indices(&self) -> Vec<usize> {
        (0..self.total_layers)
            .filter(|&i| !self.is_layer_frozen(i))
            .collect()
    }

    /// Unfreezes all layers.
    pub fn unfreeze_all(&mut self) {
        self.config.frozen_layers = Some(Vec::new());
    }

    /// Freezes all layers.
    pub fn freeze_all(&mut self) {
        self.config.frozen_layers = None;
    }

    /// Unfreezes the top N layers.
    pub fn unfreeze_top_n(&mut self, n: usize) {
        let start_idx = self.total_layers.saturating_sub(n);
        let frozen: Vec<usize> = (0..start_idx).collect();
        self.config.frozen_layers = Some(frozen);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layer_freezer() {
        let config = FreezingConfig::freeze_first_n(3);
        let freezer = LayerFreezer::new(config, 10).expect("Failed to create layer freezer");

        assert!(freezer.is_layer_frozen(0));
        assert!(freezer.is_layer_frozen(2));
        assert!(!freezer.is_layer_frozen(3));

        let frozen = freezer.frozen_layer_indices();
        assert_eq!(frozen, vec![0, 1, 2]);

        let trainable = freezer.trainable_layer_indices();
        assert_eq!(trainable, vec![3, 4, 5, 6, 7, 8, 9]);
    }

    #[test]
    fn test_unfreeze_operations() {
        let config = FreezingConfig::freeze_all();
        let mut freezer = LayerFreezer::new(config, 5).expect("Failed to create layer freezer");

        assert!(freezer.is_layer_frozen(0));
        assert!(freezer.is_layer_frozen(4));

        freezer.unfreeze_all();
        assert!(!freezer.is_layer_frozen(0));
        assert!(!freezer.is_layer_frozen(4));

        freezer.unfreeze_top_n(2);
        assert!(freezer.is_layer_frozen(0));
        assert!(!freezer.is_layer_frozen(3));
        assert!(!freezer.is_layer_frozen(4));
    }
}
