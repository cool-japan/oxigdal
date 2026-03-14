//! Tier-2 decoder — packet decoding, progression orders, rate control, and ROI
//!
//! JPEG2000 Tier-2 is responsible for organising compressed code-block data
//! (produced by Tier-1 EBCOT) into *packets*.  Each packet carries the data
//! for one (layer, resolution, component, precinct) tuple.  The order in which
//! packets appear in the codestream is governed by the *progression order*.
//!
//! # Submodules
//!
//! - [`packet`]: Packet-header and body decoding (tag trees, bit readers).
//! - [`progression`]: The five standard JPEG2000 progression order iterators.
//! - [`rate_control`]: Quality-layer configuration and PCRD-opt pass allocation.
//! - [`roi`]: Region-of-interest mask and MaxShift upshift/downshift helpers.
//!
//! # Backwards compatibility
//!
//! The types that existed in the old flat `tier2.rs` file are re-exported here
//! so that existing code that references `crate::tier2::PacketDecoder` etc.
//! continues to compile.

pub mod packet;
pub mod progression;
pub mod rate_control;
pub mod roi;

// ---------------------------------------------------------------------------
// Convenience re-exports
// ---------------------------------------------------------------------------

pub use packet::{BitReader, CodeBlockInclusion, Packet, PacketDecoder, PacketHeader, TagTree};
pub use progression::{CodeBlockAddress, ProgressionIterator};
pub use rate_control::{QualityLayer, RateController, SlopeEntry};
pub use roi::{RoiMap, RoiShift};

// ---------------------------------------------------------------------------
// Legacy types kept for backwards compatibility with the old tier2.rs API
// ---------------------------------------------------------------------------

use crate::codestream::ProgressionOrder;
use crate::error::{Jpeg2000Error, Result};
use byteorder::ReadBytesExt;
use std::io::Read;

/// Packet position in the codestream (legacy struct, kept for compatibility).
#[derive(Debug, Clone, Copy)]
pub struct PacketPosition {
    /// Quality layer
    pub layer: u16,
    /// Resolution level
    pub resolution: u8,
    /// Component index
    pub component: u16,
    /// Tile X coordinate
    pub tile_x: u32,
    /// Tile Y coordinate
    pub tile_y: u32,
}

/// Code-block contribution to a packet (legacy struct).
#[derive(Debug, Clone)]
pub struct CodeBlockContribution {
    /// Code-block index
    pub index: usize,
    /// Number of coding passes included
    pub num_passes: u8,
    /// Length of contribution data
    pub length: u32,
}

/// Legacy packet header (kept for backwards compatibility).
#[derive(Debug, Clone)]
pub struct LegacyPacketHeader {
    /// Is packet empty
    pub is_empty: bool,
    /// Code-block contributions
    pub contributions: Vec<CodeBlockContribution>,
}

/// Legacy packet decoder (kept for backwards compatibility).
///
/// New code should prefer [`PacketDecoder`] (from the `packet` submodule)
/// which implements proper tag-tree based decoding.
pub struct LegacyPacketDecoder {
    /// Progression order
    pub(crate) progression_order: ProgressionOrder,
    /// Number of layers
    pub(crate) num_layers: u16,
    /// Number of resolution levels
    pub(crate) num_resolutions: u8,
}

impl LegacyPacketDecoder {
    /// Create new legacy packet decoder.
    pub fn new(progression_order: ProgressionOrder, num_layers: u16, num_resolutions: u8) -> Self {
        Self {
            progression_order,
            num_layers,
            num_resolutions,
        }
    }

    /// Decode packet header (simplified legacy implementation).
    pub fn decode_header<R: Read>(&self, reader: &mut R) -> Result<LegacyPacketHeader> {
        let mut sync_byte = [0u8; 1];
        reader.read_exact(&mut sync_byte)?;

        let is_empty = (sync_byte[0] & 0x80) == 0;

        if is_empty {
            return Ok(LegacyPacketHeader {
                is_empty: true,
                contributions: Vec::new(),
            });
        }

        let mut contributions = Vec::new();
        let num_contributions = ((sync_byte[0] & 0x7F) as usize).min(16);

        for _ in 0..num_contributions {
            let index = reader.read_u8()? as usize;
            let num_passes = reader.read_u8()?;
            let length = u32::from(reader.read_u16::<byteorder::BigEndian>()?);

            contributions.push(CodeBlockContribution {
                index,
                num_passes,
                length,
            });
        }

        Ok(LegacyPacketHeader {
            is_empty: false,
            contributions,
        })
    }

    /// Decode packet data (legacy).
    pub fn decode_packet<R: Read>(
        &self,
        reader: &mut R,
        header: &LegacyPacketHeader,
    ) -> Result<Vec<Vec<u8>>> {
        if header.is_empty {
            return Ok(Vec::new());
        }

        let mut code_block_data = Vec::new();

        for contribution in &header.contributions {
            let mut data = vec![0u8; contribution.length as usize];
            reader.read_exact(&mut data)?;
            code_block_data.push(data);
        }

        Ok(code_block_data)
    }

    /// Calculate packet sequence for given progression order (legacy).
    pub fn packet_sequence(
        &self,
        num_components: u16,
        tiles_x: u32,
        tiles_y: u32,
    ) -> Vec<PacketPosition> {
        let mut sequence = Vec::new();

        match self.progression_order {
            ProgressionOrder::Lrcp => {
                for layer in 0..self.num_layers {
                    for resolution in 0..self.num_resolutions {
                        for component in 0..num_components {
                            for tile_y in 0..tiles_y {
                                for tile_x in 0..tiles_x {
                                    sequence.push(PacketPosition {
                                        layer,
                                        resolution,
                                        component,
                                        tile_x,
                                        tile_y,
                                    });
                                }
                            }
                        }
                    }
                }
            }
            ProgressionOrder::Rlcp => {
                for resolution in 0..self.num_resolutions {
                    for layer in 0..self.num_layers {
                        for component in 0..num_components {
                            for tile_y in 0..tiles_y {
                                for tile_x in 0..tiles_x {
                                    sequence.push(PacketPosition {
                                        layer,
                                        resolution,
                                        component,
                                        tile_x,
                                        tile_y,
                                    });
                                }
                            }
                        }
                    }
                }
            }
            ProgressionOrder::Rpcl => {
                for resolution in 0..self.num_resolutions {
                    for tile_y in 0..tiles_y {
                        for tile_x in 0..tiles_x {
                            for component in 0..num_components {
                                for layer in 0..self.num_layers {
                                    sequence.push(PacketPosition {
                                        layer,
                                        resolution,
                                        component,
                                        tile_x,
                                        tile_y,
                                    });
                                }
                            }
                        }
                    }
                }
            }
            ProgressionOrder::Pcrl => {
                for tile_y in 0..tiles_y {
                    for tile_x in 0..tiles_x {
                        for component in 0..num_components {
                            for resolution in 0..self.num_resolutions {
                                for layer in 0..self.num_layers {
                                    sequence.push(PacketPosition {
                                        layer,
                                        resolution,
                                        component,
                                        tile_x,
                                        tile_y,
                                    });
                                }
                            }
                        }
                    }
                }
            }
            ProgressionOrder::Cprl => {
                for component in 0..num_components {
                    for tile_y in 0..tiles_y {
                        for tile_x in 0..tiles_x {
                            for resolution in 0..self.num_resolutions {
                                for layer in 0..self.num_layers {
                                    sequence.push(PacketPosition {
                                        layer,
                                        resolution,
                                        component,
                                        tile_x,
                                        tile_y,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        sequence
    }
}

/// Packet data (legacy).
#[derive(Debug, Clone)]
pub struct PacketData {
    /// Component index
    pub component: u16,
    /// Resolution level
    pub resolution: u8,
    /// Compressed data
    pub data: Vec<u8>,
}

/// Quality layer manager (legacy).
pub struct QualityLayerManager {
    num_layers: u16,
    layers: Vec<QualityLayerData>,
}

/// Quality layer data (legacy).
#[derive(Debug, Clone)]
pub struct QualityLayerData {
    /// Layer index
    pub index: u16,
    /// Packets in this layer
    pub packets: Vec<PacketData>,
    /// Cumulative data rate (bytes per pixel)
    pub data_rate: f32,
}

impl QualityLayerManager {
    /// Create new quality layer manager.
    pub fn new(num_layers: u16) -> Self {
        let layers = (0..num_layers)
            .map(|i| QualityLayerData {
                index: i,
                packets: Vec::new(),
                data_rate: 0.0,
            })
            .collect();

        Self { num_layers, layers }
    }

    /// Add packet to layer.
    pub fn add_packet(&mut self, layer: u16, packet: PacketData) -> Result<()> {
        if layer >= self.num_layers {
            return Err(Jpeg2000Error::Tier2Error(format!(
                "Invalid layer index: {}",
                layer
            )));
        }

        self.layers[layer as usize].packets.push(packet);
        Ok(())
    }

    /// Get layer.
    pub fn get_layer(&self, index: u16) -> Option<&QualityLayerData> {
        if index < self.num_layers {
            Some(&self.layers[index as usize])
        } else {
            None
        }
    }

    /// Decode up to specified layer.
    pub fn decode_layers(&self, max_layer: u16) -> Result<Vec<Vec<u8>>> {
        let end_layer = max_layer.min(self.num_layers.saturating_sub(1));

        let mut all_data = Vec::new();

        for layer_idx in 0..=end_layer {
            if let Some(layer) = self.get_layer(layer_idx) {
                for packet in &layer.packets {
                    all_data.push(packet.data.clone());
                }
            }
        }

        Ok(all_data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_legacy_packet_decoder_creation() {
        let decoder = LegacyPacketDecoder::new(ProgressionOrder::Lrcp, 10, 5);
        assert_eq!(decoder.num_layers, 10);
        assert_eq!(decoder.num_resolutions, 5);
    }

    #[test]
    fn test_legacy_packet_sequence_lrcp() {
        let decoder = LegacyPacketDecoder::new(ProgressionOrder::Lrcp, 2, 3);
        let sequence = decoder.packet_sequence(1, 1, 1);
        assert_eq!(sequence.len(), 2 * 3);
        assert_eq!(sequence[0].layer, 0);
        assert_eq!(sequence[0].resolution, 0);
    }

    #[test]
    fn test_quality_layer_manager() {
        let mut manager = QualityLayerManager::new(5);

        let packet = PacketData {
            component: 0,
            resolution: 0,
            data: vec![1, 2, 3, 4],
        };

        assert!(manager.add_packet(0, packet).is_ok());
        assert!(manager.get_layer(0).is_some());
        assert_eq!(manager.get_layer(0).map(|l| l.packets.len()), Some(1));
    }

    #[test]
    fn test_invalid_layer_index() {
        let mut manager = QualityLayerManager::new(5);

        let packet = PacketData {
            component: 0,
            resolution: 0,
            data: vec![1, 2, 3, 4],
        };

        assert!(manager.add_packet(10, packet).is_err());
    }
}
