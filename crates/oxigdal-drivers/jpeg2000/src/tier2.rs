//! Tier-2 decoder (Packet decoding)
//!
//! This module handles the tier-2 decoding of JPEG2000, which includes:
//! - Packet header decoding
//! - Packet body decoding
//! - Layer progression
//! - Quality layer management

use crate::codestream::ProgressionOrder;
use crate::error::{Jpeg2000Error, Result};
use byteorder::ReadBytesExt;
use std::io::Read;

/// Packet header information
#[derive(Debug, Clone)]
pub struct PacketHeader {
    /// Is packet empty
    pub is_empty: bool,
    /// Code-block contributions
    pub contributions: Vec<CodeBlockContribution>,
}

/// Code-block contribution to a packet
#[derive(Debug, Clone)]
pub struct CodeBlockContribution {
    /// Code-block index
    pub index: usize,
    /// Number of coding passes included
    pub num_passes: u8,
    /// Length of contribution data
    pub length: u32,
}

/// Packet decoder
pub struct PacketDecoder {
    /// Progression order
    progression_order: ProgressionOrder,
    /// Number of layers
    num_layers: u16,
    /// Number of resolution levels
    num_resolutions: u8,
}

impl PacketDecoder {
    /// Create new packet decoder
    pub fn new(progression_order: ProgressionOrder, num_layers: u16, num_resolutions: u8) -> Self {
        Self {
            progression_order,
            num_layers,
            num_resolutions,
        }
    }

    /// Decode packet header
    pub fn decode_header<R: Read>(&self, reader: &mut R) -> Result<PacketHeader> {
        // Read packet header synchronization bit
        let mut sync_byte = [0u8; 1];
        reader.read_exact(&mut sync_byte)?;

        let is_empty = (sync_byte[0] & 0x80) == 0;

        if is_empty {
            return Ok(PacketHeader {
                is_empty: true,
                contributions: Vec::new(),
            });
        }

        // Decode contributions (simplified)
        // In practice, this requires bit-level reading and variable-length codes
        let mut contributions = Vec::new();

        // Simplified: read a fixed number of contributions
        // Real implementation would use tag trees and variable-length encoding
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

        Ok(PacketHeader {
            is_empty: false,
            contributions,
        })
    }

    /// Decode packet data
    pub fn decode_packet<R: Read>(
        &self,
        reader: &mut R,
        header: &PacketHeader,
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

    /// Calculate packet sequence for given progression order
    pub fn packet_sequence(
        &self,
        num_components: u16,
        tiles_x: u32,
        tiles_y: u32,
    ) -> Vec<PacketPosition> {
        let mut sequence = Vec::new();

        match self.progression_order {
            ProgressionOrder::Lrcp => {
                // Layer-Resolution-Component-Position
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
                // Resolution-Layer-Component-Position
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
                // Resolution-Position-Component-Layer
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
                // Position-Component-Resolution-Layer
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
                // Component-Position-Resolution-Layer
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

/// Packet position in the codestream
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

/// Quality layer manager
pub struct QualityLayerManager {
    /// Number of layers
    num_layers: u16,
    /// Layer data
    layers: Vec<QualityLayer>,
}

/// Quality layer data
#[derive(Debug, Clone)]
pub struct QualityLayer {
    /// Layer index
    pub index: u16,
    /// Packets in this layer
    pub packets: Vec<PacketData>,
    /// Cumulative data rate (bytes per pixel)
    pub data_rate: f32,
}

/// Packet data
#[derive(Debug, Clone)]
pub struct PacketData {
    /// Component index
    pub component: u16,
    /// Resolution level
    pub resolution: u8,
    /// Compressed data
    pub data: Vec<u8>,
}

impl QualityLayerManager {
    /// Create new quality layer manager
    pub fn new(num_layers: u16) -> Self {
        let layers = (0..num_layers)
            .map(|i| QualityLayer {
                index: i,
                packets: Vec::new(),
                data_rate: 0.0,
            })
            .collect();

        Self { num_layers, layers }
    }

    /// Add packet to layer
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

    /// Get layer
    pub fn get_layer(&self, index: u16) -> Option<&QualityLayer> {
        if index < self.num_layers {
            Some(&self.layers[index as usize])
        } else {
            None
        }
    }

    /// Decode up to specified layer
    pub fn decode_layers(&self, max_layer: u16) -> Result<Vec<Vec<u8>>> {
        let end_layer = max_layer.min(self.num_layers - 1);

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
    fn test_packet_decoder_creation() {
        let decoder = PacketDecoder::new(ProgressionOrder::Lrcp, 10, 5);
        assert_eq!(decoder.num_layers, 10);
        assert_eq!(decoder.num_resolutions, 5);
    }

    #[test]
    fn test_packet_sequence_lrcp() {
        let decoder = PacketDecoder::new(ProgressionOrder::Lrcp, 2, 3);
        let sequence = decoder.packet_sequence(1, 1, 1);

        // Should have layer * resolution * component * tiles packets
        assert_eq!(sequence.len(), 2 * 3);

        // First packet should be layer 0, resolution 0
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
