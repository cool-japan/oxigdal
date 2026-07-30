//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use crate::attribute::{Attribute, AttributeValue, Attributes};
use crate::dimension::{Dimension, Dimensions};
use crate::error::{NetCdfError, Result};
use crate::variable::{DataType, Variable, Variables};
use byteorder::{BigEndian, ByteOrder, LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};

use super::types::{
    ChunkInfo, CompressionFilter, Hdf5ByteOrder, Hdf5DatatypeClass, Hdf5MessageType,
    Hdf5Superblock, Hdf5SuperblockVersion, Nc4Group, Nc4VariableInfo, Nc4Writer,
};

/// HDF5 signature (magic bytes)
pub(crate) const HDF5_SIGNATURE: [u8; 8] = [0x89, 0x48, 0x44, 0x46, 0x0d, 0x0a, 0x1a, 0x0a];
/// NetCDF-4 convention attribute name
const NETCDF4_CONVENTIONS: &str = "_NCProperties";
/// Dimension scale class attribute
const DIMENSION_SCALE_CLASS: &str = "CLASS";
/// Dimension scale name attribute
const DIMENSION_SCALE_NAME: &str = "NAME";
/// HDF5 reference dimension list
const DIMENSION_LIST: &str = "DIMENSION_LIST";
/// Fill value attribute
const FILL_VALUE_ATTR: &str = "_FillValue";
/// Chunk sizes attribute (internal)
const CHUNK_SIZES_ATTR: &str = "_ChunkSizes";
/// Parse HDF5 datatype to NetCDF DataType
fn parse_hdf5_datatype<R: Read>(
    reader: &mut R,
    _byte_order: Hdf5ByteOrder,
) -> Result<(DataType, usize)> {
    let class_and_version = reader
        .read_u8()
        .map_err(|e| NetCdfError::Io(e.to_string()))?;
    let class = Hdf5DatatypeClass::from_byte(class_and_version)?;
    let _version = (class_and_version >> 4) & 0x0F;
    let mut class_bits = [0u8; 3];
    reader
        .read_exact(&mut class_bits)
        .map_err(|e| NetCdfError::Io(e.to_string()))?;
    let size = reader
        .read_u32::<LittleEndian>()
        .map_err(|e| NetCdfError::Io(e.to_string()))? as usize;
    let data_type = match class {
        Hdf5DatatypeClass::FixedPoint => {
            let is_signed = (class_bits[0] & 0x08) != 0;
            let _bit_offset = reader
                .read_u16::<LittleEndian>()
                .map_err(|e| NetCdfError::Io(e.to_string()))?;
            let _bit_precision = reader
                .read_u16::<LittleEndian>()
                .map_err(|e| NetCdfError::Io(e.to_string()))?;
            match (size, is_signed) {
                (1, true) => DataType::I8,
                (1, false) => DataType::U8,
                (2, true) => DataType::I16,
                (2, false) => DataType::U16,
                (4, true) => DataType::I32,
                (4, false) => DataType::U32,
                (8, true) => DataType::I64,
                (8, false) => DataType::U64,
                _ => {
                    return Err(NetCdfError::InvalidFormat(format!(
                        "Unsupported fixed-point size: {} bytes, signed: {}",
                        size, is_signed
                    )));
                }
            }
        }
        Hdf5DatatypeClass::FloatingPoint => {
            let _bit_offset = reader
                .read_u16::<LittleEndian>()
                .map_err(|e| NetCdfError::Io(e.to_string()))?;
            let _bit_precision = reader
                .read_u16::<LittleEndian>()
                .map_err(|e| NetCdfError::Io(e.to_string()))?;
            let _exponent_location = reader
                .read_u8()
                .map_err(|e| NetCdfError::Io(e.to_string()))?;
            let _exponent_size = reader
                .read_u8()
                .map_err(|e| NetCdfError::Io(e.to_string()))?;
            let _mantissa_location = reader
                .read_u8()
                .map_err(|e| NetCdfError::Io(e.to_string()))?;
            let _mantissa_size = reader
                .read_u8()
                .map_err(|e| NetCdfError::Io(e.to_string()))?;
            let _exponent_bias = reader
                .read_u32::<LittleEndian>()
                .map_err(|e| NetCdfError::Io(e.to_string()))?;
            match size {
                4 => DataType::F32,
                8 => DataType::F64,
                _ => {
                    return Err(NetCdfError::InvalidFormat(format!(
                        "Unsupported floating-point size: {} bytes",
                        size
                    )));
                }
            }
        }
        Hdf5DatatypeClass::String => DataType::String,
        _ => {
            return Err(NetCdfError::InvalidFormat(format!(
                "Unsupported HDF5 datatype class: {:?}",
                class
            )));
        }
    };
    Ok((data_type, size))
}
/// Decompress deflate-compressed data
pub(crate) fn decompress_deflate(data: &[u8], _uncompressed_size: usize) -> Result<Vec<u8>> {
    oxiarc_deflate::zlib_decompress(data)
        .map_err(|e| NetCdfError::InvalidCompressionParams(e.to_string()))
}
/// Compress data with deflate
pub(crate) fn compress_deflate(data: &[u8], level: u8) -> Result<Vec<u8>> {
    oxiarc_deflate::zlib_compress(data, level.min(9))
        .map_err(|e| NetCdfError::InvalidCompressionParams(e.to_string()))
}
/// Apply shuffle filter (rearrange bytes for better compression)
pub(crate) fn shuffle_data(data: &[u8], element_size: usize) -> Vec<u8> {
    if element_size <= 1 || data.len() < element_size {
        return data.to_vec();
    }
    let n_elements = data.len() / element_size;
    let mut result = vec![0u8; data.len()];
    for i in 0..n_elements {
        for j in 0..element_size {
            result[j * n_elements + i] = data[i * element_size + j];
        }
    }
    result
}
/// Apply unshuffle filter (reverse shuffle)
pub(crate) fn unshuffle_data(data: &[u8], element_size: usize) -> Vec<u8> {
    if element_size <= 1 || data.len() < element_size {
        return data.to_vec();
    }
    let n_elements = data.len() / element_size;
    let mut result = vec![0u8; data.len()];
    for i in 0..n_elements {
        for j in 0..element_size {
            result[i * element_size + j] = data[j * n_elements + i];
        }
    }
    result
}
/// Calculate Fletcher32 checksum
pub(crate) fn fletcher32(data: &[u8]) -> u32 {
    let mut sum1: u32 = 0;
    let mut sum2: u32 = 0;
    for chunk in data.chunks(2) {
        let word = if chunk.len() == 2 {
            u32::from(chunk[0]) | (u32::from(chunk[1]) << 8)
        } else {
            u32::from(chunk[0])
        };
        sum1 = (sum1 + word) % 65535;
        sum2 = (sum2 + sum1) % 65535;
    }
    (sum2 << 16) | sum1
}

/// Bob Jenkins' lookup3 `hashlittle` hash function (2006) — the little-endian
/// variant HDF5 uses for Superblock V2/V3 checksums (`H5checksum.c` in the
/// official HDF5 source; HDF5 always calls it with `initval = 0`).
///
/// Used by [`super::types::Nc4Writer::write_superblock`] to compute a real
/// superblock checksum rather than a hardcoded placeholder, so a written v2/v3
/// superblock is at least internally consistent (`checksum ==
/// jenkins_lookup3_hashlittle(everything_before_it, 0)`), matching the
/// algorithm oxigeo-hdf5's `superblock_v2::validate_superblock_checksum` (the
/// real reader-side counterpart) checks against.
///
/// # Reference
///
/// - <http://burtleburtle.net/bob/hash/doobs.html>
pub(crate) fn jenkins_lookup3_hashlittle(data: &[u8], initval: u32) -> u32 {
    let len = data.len();

    let init = 0xdeadbeef_u32
        .wrapping_add(len as u32)
        .wrapping_add(initval);
    let mut a: u32 = init;
    let mut b: u32 = init;
    let mut c: u32 = init;

    let mut i = 0usize;
    while i + 12 <= len {
        a = a.wrapping_add(u32::from_le_bytes([
            data[i],
            data[i + 1],
            data[i + 2],
            data[i + 3],
        ]));
        b = b.wrapping_add(u32::from_le_bytes([
            data[i + 4],
            data[i + 5],
            data[i + 6],
            data[i + 7],
        ]));
        c = c.wrapping_add(u32::from_le_bytes([
            data[i + 8],
            data[i + 9],
            data[i + 10],
            data[i + 11],
        ]));

        a = a.wrapping_sub(c);
        a ^= c.rotate_left(4);
        c = c.wrapping_add(b);
        b = b.wrapping_sub(a);
        b ^= a.rotate_left(6);
        a = a.wrapping_add(c);
        c = c.wrapping_sub(b);
        c ^= b.rotate_left(8);
        b = b.wrapping_add(a);
        a = a.wrapping_sub(c);
        a ^= c.rotate_left(16);
        c = c.wrapping_add(b);
        b = b.wrapping_sub(a);
        b ^= a.rotate_left(19);
        a = a.wrapping_add(c);
        c = c.wrapping_sub(b);
        c ^= b.rotate_left(4);
        b = b.wrapping_add(a);

        i += 12;
    }

    let tail = &data[i..];
    let remaining = tail.len();

    if remaining > 0 {
        match remaining {
            11 => {
                c = c.wrapping_add((tail[10] as u32) << 24);
                c = c.wrapping_add((tail[9] as u32) << 16);
                c = c.wrapping_add((tail[8] as u32) << 8);
                b = b.wrapping_add((tail[7] as u32) << 24);
                b = b.wrapping_add((tail[6] as u32) << 16);
                b = b.wrapping_add((tail[5] as u32) << 8);
                b = b.wrapping_add(tail[4] as u32);
                a = a.wrapping_add((tail[3] as u32) << 24);
                a = a.wrapping_add((tail[2] as u32) << 16);
                a = a.wrapping_add((tail[1] as u32) << 8);
                a = a.wrapping_add(tail[0] as u32);
            }
            10 => {
                c = c.wrapping_add((tail[9] as u32) << 16);
                c = c.wrapping_add((tail[8] as u32) << 8);
                b = b.wrapping_add((tail[7] as u32) << 24);
                b = b.wrapping_add((tail[6] as u32) << 16);
                b = b.wrapping_add((tail[5] as u32) << 8);
                b = b.wrapping_add(tail[4] as u32);
                a = a.wrapping_add((tail[3] as u32) << 24);
                a = a.wrapping_add((tail[2] as u32) << 16);
                a = a.wrapping_add((tail[1] as u32) << 8);
                a = a.wrapping_add(tail[0] as u32);
            }
            9 => {
                c = c.wrapping_add((tail[8] as u32) << 8);
                b = b.wrapping_add((tail[7] as u32) << 24);
                b = b.wrapping_add((tail[6] as u32) << 16);
                b = b.wrapping_add((tail[5] as u32) << 8);
                b = b.wrapping_add(tail[4] as u32);
                a = a.wrapping_add((tail[3] as u32) << 24);
                a = a.wrapping_add((tail[2] as u32) << 16);
                a = a.wrapping_add((tail[1] as u32) << 8);
                a = a.wrapping_add(tail[0] as u32);
            }
            8 => {
                b = b.wrapping_add((tail[7] as u32) << 24);
                b = b.wrapping_add((tail[6] as u32) << 16);
                b = b.wrapping_add((tail[5] as u32) << 8);
                b = b.wrapping_add(tail[4] as u32);
                a = a.wrapping_add((tail[3] as u32) << 24);
                a = a.wrapping_add((tail[2] as u32) << 16);
                a = a.wrapping_add((tail[1] as u32) << 8);
                a = a.wrapping_add(tail[0] as u32);
            }
            7 => {
                b = b.wrapping_add((tail[6] as u32) << 16);
                b = b.wrapping_add((tail[5] as u32) << 8);
                b = b.wrapping_add(tail[4] as u32);
                a = a.wrapping_add((tail[3] as u32) << 24);
                a = a.wrapping_add((tail[2] as u32) << 16);
                a = a.wrapping_add((tail[1] as u32) << 8);
                a = a.wrapping_add(tail[0] as u32);
            }
            6 => {
                b = b.wrapping_add((tail[5] as u32) << 8);
                b = b.wrapping_add(tail[4] as u32);
                a = a.wrapping_add((tail[3] as u32) << 24);
                a = a.wrapping_add((tail[2] as u32) << 16);
                a = a.wrapping_add((tail[1] as u32) << 8);
                a = a.wrapping_add(tail[0] as u32);
            }
            5 => {
                b = b.wrapping_add(tail[4] as u32);
                a = a.wrapping_add((tail[3] as u32) << 24);
                a = a.wrapping_add((tail[2] as u32) << 16);
                a = a.wrapping_add((tail[1] as u32) << 8);
                a = a.wrapping_add(tail[0] as u32);
            }
            4 => {
                a = a.wrapping_add((tail[3] as u32) << 24);
                a = a.wrapping_add((tail[2] as u32) << 16);
                a = a.wrapping_add((tail[1] as u32) << 8);
                a = a.wrapping_add(tail[0] as u32);
            }
            3 => {
                a = a.wrapping_add((tail[2] as u32) << 16);
                a = a.wrapping_add((tail[1] as u32) << 8);
                a = a.wrapping_add(tail[0] as u32);
            }
            2 => {
                a = a.wrapping_add((tail[1] as u32) << 8);
                a = a.wrapping_add(tail[0] as u32);
            }
            1 => {
                a = a.wrapping_add(tail[0] as u32);
            }
            _ => {}
        }
    }

    c ^= b;
    c = c.wrapping_sub(b.rotate_left(14));
    a ^= c;
    a = a.wrapping_sub(c.rotate_left(11));
    b ^= a;
    b = b.wrapping_sub(a.rotate_left(25));
    c ^= b;
    c = c.wrapping_sub(b.rotate_left(16));
    a ^= c;
    a = a.wrapping_sub(c.rotate_left(4));
    b ^= a;
    b = b.wrapping_sub(a.rotate_left(14));
    c ^= b;
    c = c.wrapping_sub(b.rotate_left(24));

    c
}

/// Convert NetCDF DataType to bytes per element
fn data_type_size(dtype: DataType) -> usize {
    match dtype {
        DataType::I8 | DataType::U8 | DataType::Char => 1,
        DataType::I16 | DataType::U16 => 2,
        DataType::I32 | DataType::U32 | DataType::F32 => 4,
        DataType::I64 | DataType::U64 | DataType::F64 => 8,
        DataType::String => 0,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Per-test scratch fixture inside the system temp dir (house policy: no
    /// hardcoded absolute paths).
    ///
    /// The leaf name embeds the process id and a monotonic counter, so no two
    /// test binaries — nor two concurrent runs of this one — can ever land on
    /// the same file.  Dropping the guard removes the fixture, so a panicking
    /// test leaks nothing.
    struct TempPath(std::path::PathBuf);

    impl TempPath {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
            Self(std::env::temp_dir().join(format!(
                "oxigeo_nc4_functions_{}_{seq}_{name}",
                std::process::id()
            )))
        }
    }

    impl std::ops::Deref for TempPath {
        type Target = std::path::Path;

        fn deref(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl AsRef<std::path::Path> for TempPath {
        fn as_ref(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TempPath {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn test_hdf5_signature() {
        assert_eq!(
            HDF5_SIGNATURE,
            [0x89, 0x48, 0x44, 0x46, 0x0d, 0x0a, 0x1a, 0x0a]
        );
    }
    #[test]
    fn test_superblock_version_from_byte() {
        assert!(matches!(
            Hdf5SuperblockVersion::from_byte(0),
            Ok(Hdf5SuperblockVersion::V0)
        ));
        assert!(matches!(
            Hdf5SuperblockVersion::from_byte(1),
            Ok(Hdf5SuperblockVersion::V1)
        ));
        assert!(matches!(
            Hdf5SuperblockVersion::from_byte(2),
            Ok(Hdf5SuperblockVersion::V2)
        ));
        assert!(matches!(
            Hdf5SuperblockVersion::from_byte(3),
            Ok(Hdf5SuperblockVersion::V3)
        ));
        assert!(Hdf5SuperblockVersion::from_byte(4).is_err());
    }
    #[test]
    fn test_datatype_class_from_byte() {
        assert!(matches!(
            Hdf5DatatypeClass::from_byte(0),
            Ok(Hdf5DatatypeClass::FixedPoint)
        ));
        assert!(matches!(
            Hdf5DatatypeClass::from_byte(1),
            Ok(Hdf5DatatypeClass::FloatingPoint)
        ));
        assert!(matches!(
            Hdf5DatatypeClass::from_byte(3),
            Ok(Hdf5DatatypeClass::String)
        ));
    }
    #[test]
    fn test_compression_filter_id() {
        assert_eq!(CompressionFilter::None.filter_id(), 0);
        assert_eq!(CompressionFilter::Deflate(6).filter_id(), 1);
        assert_eq!(CompressionFilter::Shuffle.filter_id(), 2);
        assert_eq!(CompressionFilter::Fletcher32.filter_id(), 3);
    }
    #[test]
    fn test_chunk_info() {
        let mut chunk_info = ChunkInfo::new(vec![10, 10]);
        assert!(!chunk_info.is_compressed());
        chunk_info.add_filter(CompressionFilter::Deflate(6));
        assert!(chunk_info.is_compressed());
        let total = chunk_info.total_chunks(&[100, 100]);
        assert_eq!(total, 100);
    }
    #[test]
    fn test_nc4_group() {
        let mut root = Nc4Group::root();
        assert_eq!(root.path(), "/");
        assert!(root.name().is_empty());
        let child = Nc4Group::new("data", "/").expect("Failed to create group");
        assert_eq!(child.path(), "/data");
        assert_eq!(child.name(), "data");
        root.add_group(child);
        assert_eq!(root.groups().len(), 1);
        let found = root.find_group("data");
        assert!(found.is_some());
        assert_eq!(found.map(|g| g.name()), Some("data"));
    }
    #[test]
    fn test_shuffle_unshuffle() {
        let data = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
        let shuffled = shuffle_data(&data, 4);
        let unshuffled = unshuffle_data(&shuffled, 4);
        assert_eq!(data, unshuffled);
    }
    #[test]
    fn test_fletcher32() {
        let data = b"Hello, World!";
        let checksum = fletcher32(data);
        assert!(checksum > 0);
    }
    #[test]
    fn test_jenkins_lookup3_deterministic_and_input_sensitive() {
        let h1 = jenkins_lookup3_hashlittle(b"hello", 0);
        let h2 = jenkins_lookup3_hashlittle(b"hello", 0);
        assert_eq!(h1, h2, "same input must hash identically");

        let h3 = jenkins_lookup3_hashlittle(b"world", 0);
        assert_ne!(
            h1, h3,
            "different input must (overwhelmingly likely) hash differently"
        );

        // Different initval must also change the hash.
        let h4 = jenkins_lookup3_hashlittle(b"hello", 1);
        assert_ne!(h1, h4);
    }
    #[test]
    fn test_jenkins_lookup3_empty_input() {
        // Must not panic on empty input, and must be deterministic.
        let h1 = jenkins_lookup3_hashlittle(&[], 0);
        let h2 = jenkins_lookup3_hashlittle(&[], 0);
        assert_eq!(h1, h2);
    }
    #[test]
    fn test_jenkins_lookup3_all_tail_lengths() {
        // Exercise every tail-length match arm (1..=11 remaining bytes after
        // the last full 12-byte block) without panicking.
        for len in 0..40usize {
            let data: Vec<u8> = (0..len as u8).collect();
            let _ = jenkins_lookup3_hashlittle(&data, 0);
        }
    }
    #[test]
    fn test_data_type_size() {
        assert_eq!(data_type_size(DataType::I8), 1);
        assert_eq!(data_type_size(DataType::I16), 2);
        assert_eq!(data_type_size(DataType::I32), 4);
        assert_eq!(data_type_size(DataType::I64), 8);
        assert_eq!(data_type_size(DataType::F32), 4);
        assert_eq!(data_type_size(DataType::F64), 8);
    }
    #[test]
    fn test_nc4_variable_info() {
        let var = Variable::new("temperature", DataType::F32, vec!["time".to_string()])
            .expect("Failed to create variable");
        let mut info = Nc4VariableInfo::new(var);
        assert_eq!(info.name(), "temperature");
        assert!(!info.is_chunked());
        assert!(!info.is_compressed);
        info.chunk_info = Some(ChunkInfo::new(vec![10]));
        assert!(info.is_chunked());
    }
    #[test]
    fn test_compress_decompress_deflate() {
        let data = b"Hello, World! This is some test data for compression.";
        let compressed = compress_deflate(data, 6).expect("Compression failed");
        let decompressed =
            decompress_deflate(&compressed, data.len()).expect("Decompression failed");
        assert_eq!(data.as_slice(), decompressed.as_slice());
    }
    #[test]
    fn test_hdf5_superblock_parse_invalid() {
        let invalid_data = b"INVALID!";
        let mut cursor = Cursor::new(invalid_data);
        let result = Hdf5Superblock::parse(&mut cursor);
        assert!(result.is_err());
    }
    #[test]
    fn test_chunk_indices() {
        let chunk_info = ChunkInfo::new(vec![10, 20]);
        let indices = chunk_info.get_chunk_indices(&[5, 15]);
        assert_eq!(indices, vec![0, 0]);
        let indices = chunk_info.get_chunk_indices(&[15, 25]);
        assert_eq!(indices, vec![1, 1]);
        let indices = chunk_info.get_chunk_indices(&[95, 195]);
        assert_eq!(indices, vec![9, 9]);
    }
    #[test]
    fn test_hdf5_message_type() {
        assert!(matches!(
            Hdf5MessageType::from_u16(0x0001),
            Some(Hdf5MessageType::Dataspace)
        ));
        assert!(matches!(
            Hdf5MessageType::from_u16(0x0003),
            Some(Hdf5MessageType::Datatype)
        ));
        assert!(matches!(
            Hdf5MessageType::from_u16(0x0008),
            Some(Hdf5MessageType::DataLayout)
        ));
        assert!(matches!(
            Hdf5MessageType::from_u16(0x000C),
            Some(Hdf5MessageType::Attribute)
        ));
        assert!(Hdf5MessageType::from_u16(0xFFFF).is_none());
    }
    #[test]
    fn test_nc4_writer_define_mode() {
        let temp_file = TempPath::new("define.nc4");
        {
            let mut writer = Nc4Writer::create(&temp_file).expect("Failed to create writer");
            let dim = Dimension::new("time", 10).expect("Failed to create dimension");
            writer.add_dimension(dim).expect("Failed to add dimension");
            let var = Variable::new("temperature", DataType::F32, vec!["time".to_string()])
                .expect("Failed to create variable");
            writer.add_variable(var).expect("Failed to add variable");
            let attr = Attribute::new("title", AttributeValue::text("Test Data"))
                .expect("Failed to create attribute");
            writer
                .add_global_attribute(attr)
                .expect("Failed to add attribute");
            writer
                .set_chunking("temperature", vec![5])
                .expect("Failed to set chunking");
            writer
                .set_compression("temperature", 6)
                .expect("Failed to set compression");
            writer.close().expect("Failed to close writer");
        }
    }
    #[test]
    fn test_byte_order_default() {
        let byte_order = Hdf5ByteOrder::default();
        assert!(matches!(byte_order, Hdf5ByteOrder::LittleEndian));
    }
}
