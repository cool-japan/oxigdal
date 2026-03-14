//! JPEG2000 Tier-2 rate control
//!
//! Implements a simplified version of the Post-Compression Rate-Distortion
//! Optimisation (PCRD-opt) algorithm described in ISO 15444-1:2019 §B.3.
//!
//! The algorithm allocates coding passes from code blocks into quality layers
//! by choosing the distortion-rate slope threshold that minimises distortion
//! for a given total byte budget.

use crate::error::{Jpeg2000Error, Result};

// ---------------------------------------------------------------------------
// Quality layer configuration
// ---------------------------------------------------------------------------

/// A single quality layer target.
///
/// Either a bit-rate target *or* a PSNR target may be specified; if neither
/// is set the layer is treated as lossless (all remaining passes go here).
#[derive(Debug, Clone)]
pub struct QualityLayer {
    /// Zero-based layer index.
    pub layer_index: u16,
    /// Target bit rate for this layer in bits-per-pixel, or `None`.
    pub target_rate: Option<f64>,
    /// Target PSNR in dB, or `None`.
    pub target_psnr: Option<f64>,
}

impl QualityLayer {
    /// Create a new quality layer with a bit-rate target.
    pub fn with_rate(layer_index: u16, target_rate: f64) -> Self {
        Self {
            layer_index,
            target_rate: Some(target_rate),
            target_psnr: None,
        }
    }

    /// Create a new quality layer with a PSNR target (dB).
    pub fn with_psnr(layer_index: u16, target_psnr: f64) -> Self {
        Self {
            layer_index,
            target_rate: None,
            target_psnr: Some(target_psnr),
        }
    }

    /// Create a lossless (no-target) quality layer.
    pub fn lossless(layer_index: u16) -> Self {
        Self {
            layer_index,
            target_rate: None,
            target_psnr: None,
        }
    }

    /// Return `true` if neither rate nor PSNR target is set (lossless layer).
    pub fn is_lossless(&self) -> bool {
        self.target_rate.is_none() && self.target_psnr.is_none()
    }
}

// ---------------------------------------------------------------------------
// Distortion-rate slope entry (per coding pass)
// ---------------------------------------------------------------------------

/// A (distortion-reduction, byte-cost) pair for a single coding pass.
///
/// The slope `Δ = distortion_reduction / byte_cost` is used to rank passes
/// across all code blocks for PCRD-opt allocation.
#[derive(Debug, Clone, PartialEq)]
pub struct SlopeEntry {
    /// Distortion reduction achieved by including this pass.
    pub distortion_reduction: f64,
    /// Compressed byte cost of this pass.
    pub byte_cost: u32,
}

impl SlopeEntry {
    /// Compute the distortion-rate slope.
    ///
    /// Returns `f64::INFINITY` when `byte_cost == 0`.
    pub fn slope(&self) -> f64 {
        if self.byte_cost == 0 {
            f64::INFINITY
        } else {
            self.distortion_reduction / self.byte_cost as f64
        }
    }
}

// ---------------------------------------------------------------------------
// Rate controller
// ---------------------------------------------------------------------------

/// Rate controller for multi-layer JPEG2000 encoding.
///
/// Given a set of quality layers with byte-budget targets, the controller
/// distributes coding passes from code blocks into layers using a simplified
/// PCRD-opt strategy: sort all passes by slope (distortion/byte), then greedily
/// assign them to layers in order of decreasing slope until each layer's byte
/// budget is exhausted.
#[derive(Debug, Clone)]
pub struct RateController {
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Number of image components.
    pub num_components: u16,
    /// Configured quality layers, in layer-index order.
    layers: Vec<QualityLayer>,
}

impl RateController {
    /// Create a new rate controller.
    pub fn new(width: u32, height: u32, num_components: u16) -> Self {
        Self {
            width,
            height,
            num_components,
            layers: Vec::new(),
        }
    }

    /// Add a quality layer.  Layers must be added in ascending `layer_index` order.
    pub fn add_layer(&mut self, layer: QualityLayer) -> Result<()> {
        if let Some(last) = self.layers.last() {
            if layer.layer_index <= last.layer_index {
                return Err(Jpeg2000Error::Tier2Error(format!(
                    "Quality layers must be added in ascending order; got {} after {}",
                    layer.layer_index, last.layer_index
                )));
            }
        }
        self.layers.push(layer);
        Ok(())
    }

    /// Return the number of configured quality layers.
    pub fn num_layers(&self) -> usize {
        self.layers.len()
    }

    /// Return a reference to a layer by index, or `None` if out of range.
    pub fn get_layer(&self, idx: usize) -> Option<&QualityLayer> {
        self.layers.get(idx)
    }

    /// Compute the cumulative byte budget for quality layer `layer_idx`.
    ///
    /// Returns `None` if the layer is lossless (no rate target) or if
    /// `layer_idx` is out of bounds.
    pub fn layer_byte_budget(&self, layer_idx: u16) -> Option<u64> {
        let layer = self.layers.get(layer_idx as usize)?;
        if let Some(bpp) = layer.target_rate {
            let total_pixels = self.width as u64 * self.height as u64 * self.num_components as u64;
            // bpp is bits per pixel → convert to bytes
            let bytes = (bpp * total_pixels as f64 / 8.0).ceil() as u64;
            Some(bytes)
        } else {
            None
        }
    }

    /// Allocate coding passes into quality layers using simplified PCRD-opt.
    ///
    /// # Parameters
    /// - `slopes`: Slice of `(distortion_reduction, byte_cost)` pairs, one per
    ///   coding pass, ordered by code block and then by pass index within that block.
    ///
    /// # Returns
    /// A `Vec<u8>` of the same length as `slopes`, where each entry is the
    /// layer index (0-based) into which that coding pass is allocated.
    /// Passes that cannot fit in any layer are assigned to the last layer.
    pub fn allocate_passes(&self, slopes: &[SlopeEntry]) -> Result<Vec<u16>> {
        if self.layers.is_empty() {
            return Err(Jpeg2000Error::Tier2Error(
                "No quality layers configured".to_string(),
            ));
        }

        let num_passes = slopes.len();
        let num_layers = self.layers.len();

        // Build sorted index list: highest slope first
        let mut order: Vec<usize> = (0..num_passes).collect();
        order.sort_by(|&a, &b| {
            slopes[b]
                .slope()
                .partial_cmp(&slopes[a].slope())
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut assignments = vec![0u16; num_passes];
        let mut cumulative_bytes = 0u64;
        let mut current_layer = 0usize;

        for &pass_idx in &order {
            let cost = slopes[pass_idx].byte_cost as u64;

            // Advance to the next layer whose budget is not yet exceeded
            while current_layer + 1 < num_layers {
                if let Some(budget) = self.layer_byte_budget(current_layer as u16) {
                    if cumulative_bytes + cost > budget {
                        current_layer += 1;
                        continue;
                    }
                }
                break;
            }

            assignments[pass_idx] = current_layer as u16;
            cumulative_bytes += cost;
        }

        Ok(assignments)
    }

    /// Total pixel count (width × height × components).
    pub fn total_pixels(&self) -> u64 {
        self.width as u64 * self.height as u64 * self.num_components as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quality_layer_with_rate() {
        let layer = QualityLayer::with_rate(0, 1.5);
        assert_eq!(layer.layer_index, 0);
        assert_eq!(layer.target_rate, Some(1.5));
        assert!(layer.target_psnr.is_none());
        assert!(!layer.is_lossless());
    }

    #[test]
    fn test_quality_layer_with_psnr() {
        let layer = QualityLayer::with_psnr(1, 40.0);
        assert_eq!(layer.layer_index, 1);
        assert_eq!(layer.target_psnr, Some(40.0));
        assert!(layer.target_rate.is_none());
        assert!(!layer.is_lossless());
    }

    #[test]
    fn test_quality_layer_lossless() {
        let layer = QualityLayer::lossless(2);
        assert!(layer.is_lossless());
    }

    #[test]
    fn test_slope_entry_slope() {
        let entry = SlopeEntry {
            distortion_reduction: 100.0,
            byte_cost: 25,
        };
        assert!((entry.slope() - 4.0).abs() < 1e-10);
    }

    #[test]
    fn test_slope_entry_zero_cost() {
        let entry = SlopeEntry {
            distortion_reduction: 10.0,
            byte_cost: 0,
        };
        assert_eq!(entry.slope(), f64::INFINITY);
    }

    #[test]
    fn test_rate_controller_new() {
        let rc = RateController::new(512, 512, 3);
        assert_eq!(rc.width, 512);
        assert_eq!(rc.height, 512);
        assert_eq!(rc.num_components, 3);
        assert_eq!(rc.num_layers(), 0);
    }

    #[test]
    fn test_add_layers_in_order() {
        let mut rc = RateController::new(256, 256, 1);
        assert!(rc.add_layer(QualityLayer::with_rate(0, 0.5)).is_ok());
        assert!(rc.add_layer(QualityLayer::with_rate(1, 1.0)).is_ok());
        assert_eq!(rc.num_layers(), 2);
    }

    #[test]
    fn test_add_layers_out_of_order_fails() {
        let mut rc = RateController::new(256, 256, 1);
        rc.add_layer(QualityLayer::with_rate(1, 1.0)).unwrap();
        let err = rc.add_layer(QualityLayer::with_rate(0, 0.5));
        assert!(err.is_err());
    }

    #[test]
    fn test_layer_byte_budget_basic() {
        let mut rc = RateController::new(256, 256, 1);
        rc.add_layer(QualityLayer::with_rate(0, 1.0)).unwrap(); // 1 bpp
        // 256*256*1 pixels * 1 bit/pixel / 8 = 8192 bytes
        let budget = rc.layer_byte_budget(0);
        assert_eq!(budget, Some(8192));
    }

    #[test]
    fn test_layer_byte_budget_multi_component() {
        let mut rc = RateController::new(64, 64, 3);
        rc.add_layer(QualityLayer::with_rate(0, 8.0)).unwrap(); // 8 bpp
        // 64*64*3 * 8 / 8 = 12288 bytes
        let budget = rc.layer_byte_budget(0);
        assert_eq!(budget, Some(12288));
    }

    #[test]
    fn test_layer_byte_budget_lossless_returns_none() {
        let mut rc = RateController::new(256, 256, 1);
        rc.add_layer(QualityLayer::lossless(0)).unwrap();
        assert_eq!(rc.layer_byte_budget(0), None);
    }

    #[test]
    fn test_layer_byte_budget_out_of_range() {
        let rc = RateController::new(256, 256, 1);
        assert_eq!(rc.layer_byte_budget(99), None);
    }

    #[test]
    fn test_allocate_passes_basic() {
        let mut rc = RateController::new(64, 64, 1);
        // Budget: 64*64*2/8 = 1024 bytes for layer 0
        rc.add_layer(QualityLayer::with_rate(0, 2.0)).unwrap();
        rc.add_layer(QualityLayer::lossless(1)).unwrap();

        // Two passes: one cheap low-slope, one expensive high-slope
        let slopes = vec![
            SlopeEntry {
                distortion_reduction: 10.0,
                byte_cost: 100,
            },
            SlopeEntry {
                distortion_reduction: 100.0,
                byte_cost: 100,
            },
        ];
        let assignments = rc.allocate_passes(&slopes).unwrap();
        assert_eq!(assignments.len(), 2);
        // Higher slope pass should be in an earlier (or equal) layer
        assert!(assignments[1] <= assignments[0]);
    }

    #[test]
    fn test_allocate_passes_no_layers_fails() {
        let rc = RateController::new(64, 64, 1);
        let slopes = vec![SlopeEntry {
            distortion_reduction: 1.0,
            byte_cost: 10,
        }];
        assert!(rc.allocate_passes(&slopes).is_err());
    }

    #[test]
    fn test_total_pixels() {
        let rc = RateController::new(100, 200, 3);
        assert_eq!(rc.total_pixels(), 60000);
    }

    #[test]
    fn test_get_layer() {
        let mut rc = RateController::new(256, 256, 1);
        rc.add_layer(QualityLayer::with_rate(0, 1.0)).unwrap();
        assert!(rc.get_layer(0).is_some());
        assert!(rc.get_layer(1).is_none());
    }
}
