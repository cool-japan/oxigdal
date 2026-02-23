//! Dictionary compression codec
//!
//! Dictionary compression is effective for data with repeated patterns,
//! particularly attribute data with limited unique values.

use crate::error::{CompressionError, Result};
use ahash::AHashMap;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::Cursor;

/// Dictionary codec configuration
#[derive(Debug, Clone)]
pub struct DictionaryConfig {
    /// Maximum dictionary size
    pub max_dict_size: usize,

    /// Symbol size in bytes
    pub symbol_size: usize,
}

impl Default for DictionaryConfig {
    fn default() -> Self {
        Self {
            max_dict_size: 65536,
            symbol_size: 4,
        }
    }
}

impl DictionaryConfig {
    /// Create configuration with symbol size
    pub fn with_symbol_size(symbol_size: usize) -> Self {
        Self {
            symbol_size,
            ..Default::default()
        }
    }

    /// Set maximum dictionary size
    pub fn with_max_dict_size(mut self, size: usize) -> Self {
        self.max_dict_size = size;
        self
    }
}

/// Dictionary compression codec
pub struct DictionaryCodec {
    config: DictionaryConfig,
}

impl DictionaryCodec {
    /// Create a new Dictionary codec with default configuration
    pub fn new() -> Self {
        Self {
            config: DictionaryConfig::default(),
        }
    }

    /// Create a new Dictionary codec with custom configuration
    pub fn with_config(config: DictionaryConfig) -> Self {
        Self { config }
    }

    /// Compress data using dictionary encoding
    pub fn compress(&self, input: &[u8]) -> Result<Vec<u8>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        let symbol_size = self.config.symbol_size;

        if input.len() % symbol_size != 0 {
            return Err(CompressionError::InvalidBufferSize(format!(
                "Input size {} must be multiple of symbol size {}",
                input.len(),
                symbol_size
            )));
        }

        let num_symbols = input.len() / symbol_size;

        // Build dictionary
        let mut dictionary = Vec::new();
        let mut dict_map: AHashMap<Vec<u8>, u32> = AHashMap::new();
        let mut indices = Vec::with_capacity(num_symbols);

        for i in 0..num_symbols {
            let start = i * symbol_size;
            let end = start + symbol_size;
            let symbol = &input[start..end];

            let index = if let Some(&idx) = dict_map.get(symbol) {
                idx
            } else {
                if dictionary.len() >= self.config.max_dict_size {
                    return Err(CompressionError::DictionaryError(
                        "Dictionary size exceeded".to_string(),
                    ));
                }

                let idx = (dictionary.len() / symbol_size) as u32;
                dictionary.extend_from_slice(symbol);
                dict_map.insert(symbol.to_vec(), idx);
                idx
            };

            indices.push(index);
        }

        // Encode output: dict_size (u32) + dictionary + indices
        let mut output = Vec::new();

        // Write dictionary size
        output.write_u32::<LittleEndian>(dictionary.len() as u32 / symbol_size as u32)?;

        // Write dictionary
        output.extend_from_slice(&dictionary);

        // Write indices (choose appropriate size based on dictionary size)
        if dict_map.len() <= 256 {
            // Use u8 indices
            output.push(1); // Index size marker
            for idx in indices {
                output.push(idx as u8);
            }
        } else if dict_map.len() <= 65536 {
            // Use u16 indices
            output.push(2); // Index size marker
            for idx in indices {
                output.write_u16::<LittleEndian>(idx as u16)?;
            }
        } else {
            // Use u32 indices
            output.push(4); // Index size marker
            for idx in indices {
                output.write_u32::<LittleEndian>(idx)?;
            }
        }

        Ok(output)
    }

    /// Decompress dictionary encoded data
    pub fn decompress(&self, input: &[u8]) -> Result<Vec<u8>> {
        if input.is_empty() {
            return Ok(Vec::new());
        }

        let mut cursor = Cursor::new(input);
        let symbol_size = self.config.symbol_size;

        // Read dictionary size
        let dict_size = cursor.read_u32::<LittleEndian>()? as usize;

        // Read dictionary
        let dict_bytes = dict_size * symbol_size;
        let pos = cursor.position() as usize;

        if pos + dict_bytes > input.len() {
            return Err(CompressionError::DictionaryError(
                "Invalid dictionary size".to_string(),
            ));
        }

        let dictionary = &input[pos..pos + dict_bytes];
        cursor.set_position((pos + dict_bytes) as u64);

        // Read index size marker
        let index_size = cursor.read_u8()? as usize;

        // Read indices and reconstruct data
        let mut output = Vec::new();

        match index_size {
            1 => {
                while cursor.position() < input.len() as u64 {
                    let idx = cursor.read_u8()? as usize;
                    let start = idx * symbol_size;
                    let end = start + symbol_size;

                    if end > dictionary.len() {
                        return Err(CompressionError::DictionaryError(
                            "Invalid dictionary index".to_string(),
                        ));
                    }

                    output.extend_from_slice(&dictionary[start..end]);
                }
            }
            2 => {
                while cursor.position() < input.len() as u64 {
                    let idx = cursor.read_u16::<LittleEndian>()? as usize;
                    let start = idx * symbol_size;
                    let end = start + symbol_size;

                    if end > dictionary.len() {
                        return Err(CompressionError::DictionaryError(
                            "Invalid dictionary index".to_string(),
                        ));
                    }

                    output.extend_from_slice(&dictionary[start..end]);
                }
            }
            4 => {
                while cursor.position() < input.len() as u64 {
                    let idx = cursor.read_u32::<LittleEndian>()? as usize;
                    let start = idx * symbol_size;
                    let end = start + symbol_size;

                    if end > dictionary.len() {
                        return Err(CompressionError::DictionaryError(
                            "Invalid dictionary index".to_string(),
                        ));
                    }

                    output.extend_from_slice(&dictionary[start..end]);
                }
            }
            _ => {
                return Err(CompressionError::DictionaryError(format!(
                    "Invalid index size: {}",
                    index_size
                )));
            }
        }

        Ok(output)
    }

    /// Estimate unique values in data
    pub fn count_unique(input: &[u8], symbol_size: usize) -> usize {
        if input.is_empty() || input.len() % symbol_size != 0 {
            return 0;
        }

        let mut unique = std::collections::HashSet::new();

        for chunk in input.chunks_exact(symbol_size) {
            unique.insert(chunk);
        }

        unique.len()
    }
}

impl Default for DictionaryCodec {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dictionary_compress_decompress() {
        let config = DictionaryConfig::with_symbol_size(4);
        let codec = DictionaryCodec::with_config(config);

        let mut data = Vec::new();
        for _ in 0..10 {
            data.extend_from_slice(&[1u8, 2, 3, 4]);
        }
        for _ in 0..10 {
            data.extend_from_slice(&[5u8, 6, 7, 8]);
        }

        let compressed = codec.compress(&data).expect("Compression failed");
        assert!(compressed.len() < data.len());

        let decompressed = codec.decompress(&compressed).expect("Decompression failed");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_dictionary_empty_data() {
        let codec = DictionaryCodec::new();
        let data: &[u8] = b"";

        let compressed = codec.compress(data).expect("Compression failed");
        assert_eq!(compressed.len(), 0);

        let decompressed = codec.decompress(&compressed).expect("Decompression failed");
        assert_eq!(decompressed.len(), 0);
    }

    #[test]
    fn test_dictionary_count_unique() {
        let mut data = Vec::new();
        data.extend_from_slice(&[1u8, 2, 3, 4]);
        data.extend_from_slice(&[1u8, 2, 3, 4]);
        data.extend_from_slice(&[5u8, 6, 7, 8]);

        let unique = DictionaryCodec::count_unique(&data, 4);
        assert_eq!(unique, 2);
    }
}
