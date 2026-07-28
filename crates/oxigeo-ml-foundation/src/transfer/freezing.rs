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

    /// Builds a per-layer learning-rate vector: `base_learning_rate` for
    /// trainable layers and `0.0` for frozen layers.
    ///
    /// The result has one entry per layer (`0..total_layers`) and is intended
    /// to be passed to a backend's layer-wise optimizer step
    /// (`MLBackend::optimizer_step_layerwise`), so freezing is applied to a real
    /// model's weights (frozen layers receive a zero step and do not change).
    ///
    /// # Errors
    ///
    /// Returns an error if `num_layers` does not match the freezer's configured
    /// layer count.
    pub fn layer_learning_rates(
        &self,
        base_learning_rate: f32,
        num_layers: usize,
    ) -> Result<Vec<f32>> {
        if num_layers != self.total_layers {
            return Err(Error::invalid_parameter(
                "num_layers",
                num_layers,
                format!("freezer was built for {} layers", self.total_layers),
            ));
        }
        Ok((0..self.total_layers)
            .map(|i| {
                if self.is_layer_frozen(i) {
                    0.0
                } else {
                    base_learning_rate
                }
            })
            .collect())
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
    fn test_layer_learning_rates_bridge() {
        let config = FreezingConfig::freeze_first_n(3);
        let freezer = LayerFreezer::new(config, 5).expect("Failed to create layer freezer");

        let lrs = freezer
            .layer_learning_rates(0.01, 5)
            .expect("layer lrs failed");
        // Layers 0..3 frozen -> 0.0; layers 3,4 trainable -> base lr.
        assert_eq!(lrs, vec![0.0, 0.0, 0.0, 0.01, 0.01]);

        // Mismatched layer count must error.
        assert!(freezer.layer_learning_rates(0.01, 4).is_err());
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
