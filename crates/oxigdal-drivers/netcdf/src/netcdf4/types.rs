//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use crate::attribute::{Attribute, AttributeValue, Attributes};
use crate::dimension::{Dimension, Dimensions};
use crate::error::{NetCdfError, Result};
use crate::variable::{DataType, Variable, Variables};
use byteorder::{BigEndian, ByteOrder, LittleEndian, ReadBytesExt, WriteBytesExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use super::functions::{
    HDF5_SIGNATURE, compress_deflate, decompress_deflate, fletcher32, shuffle_data, unshuffle_data,
};

/// HDF5 superblock version for format detection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hdf5SuperblockVersion {
    /// Version 0 (HDF5 1.0)
    V0,
    /// Version 1 (HDF5 1.2)
    V1,
    /// Version 2 (HDF5 1.8)
    V2,
    /// Version 3 (HDF5 1.10+)
    V3,
}
impl Hdf5SuperblockVersion {
    /// Create from version byte
    pub(crate) fn from_byte(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::V0),
            1 => Ok(Self::V1),
            2 => Ok(Self::V2),
            3 => Ok(Self::V3),
            _ => Err(NetCdfError::UnsupportedVersion {
                version: value,
                message: "Unsupported HDF5 superblock version".to_string(),
            }),
        }
    }
}
/// Chunk information for chunked datasets
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkInfo {
    /// Chunk dimensions
    pub dims: Vec<usize>,
    /// Filter pipeline
    pub filters: Vec<CompressionFilter>,
    /// Chunk offsets in file (address map)
    pub chunk_offsets: HashMap<Vec<usize>, u64>,
}
impl ChunkInfo {
    /// Create new chunk info
    #[must_use]
    pub fn new(dims: Vec<usize>) -> Self {
        Self {
            dims,
            filters: Vec::new(),
            chunk_offsets: HashMap::new(),
        }
    }
    /// Add a filter to the pipeline
    pub fn add_filter(&mut self, filter: CompressionFilter) {
        self.filters.push(filter);
    }
    /// Check if compression is enabled
    #[must_use]
    pub fn is_compressed(&self) -> bool {
        self.filters
            .iter()
            .any(|f| matches!(f, CompressionFilter::Deflate(_)))
    }
    /// Calculate total number of chunks
    #[must_use]
    pub fn total_chunks(&self, dataset_dims: &[usize]) -> usize {
        if self.dims.is_empty() || dataset_dims.is_empty() {
            return 0;
        }
        self.dims
            .iter()
            .zip(dataset_dims.iter())
            .map(|(chunk_dim, data_dim)| {
                if *chunk_dim == 0 {
                    0
                } else {
                    (data_dim + chunk_dim - 1) / chunk_dim
                }
            })
            .product()
    }
    /// Get chunk indices for a given element position
    #[must_use]
    pub fn get_chunk_indices(&self, position: &[usize]) -> Vec<usize> {
        position
            .iter()
            .zip(self.dims.iter())
            .map(
                |(pos, chunk_dim)| {
                    if *chunk_dim == 0 { 0 } else { pos / chunk_dim }
                },
            )
            .collect()
    }
}
/// HDF5 object header message types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Hdf5MessageType {
    /// NIL message
    Nil = 0x0000,
    /// Dataspace message
    Dataspace = 0x0001,
    /// Link info message
    LinkInfo = 0x0002,
    /// Datatype message
    Datatype = 0x0003,
    /// Fill value (old) message
    FillValueOld = 0x0004,
    /// Fill value message
    FillValue = 0x0005,
    /// Link message
    Link = 0x0006,
    /// External file list message
    ExternalFileList = 0x0007,
    /// Data layout message
    DataLayout = 0x0008,
    /// Bogus message
    Bogus = 0x0009,
    /// Group info message
    GroupInfo = 0x000A,
    /// Filter pipeline message
    FilterPipeline = 0x000B,
    /// Attribute message
    Attribute = 0x000C,
    /// Object comment message
    ObjectComment = 0x000D,
    /// Object modification time (old) message
    ObjectModTimeOld = 0x000E,
    /// Shared message table message
    SharedMessageTable = 0x000F,
    /// Object header continuation message
    ObjectHeaderContinuation = 0x0010,
    /// Symbol table message
    SymbolTable = 0x0011,
    /// Object modification time message
    ObjectModTime = 0x0012,
    /// B-tree K values message
    BTreeKValues = 0x0013,
    /// Driver info message
    DriverInfo = 0x0014,
    /// Attribute info message
    AttributeInfo = 0x0015,
    /// Object reference count message
    ObjectRefCount = 0x0016,
}
impl Hdf5MessageType {
    /// Create from u16 value
    pub(crate) fn from_u16(value: u16) -> Option<Self> {
        match value {
            0x0000 => Some(Self::Nil),
            0x0001 => Some(Self::Dataspace),
            0x0002 => Some(Self::LinkInfo),
            0x0003 => Some(Self::Datatype),
            0x0004 => Some(Self::FillValueOld),
            0x0005 => Some(Self::FillValue),
            0x0006 => Some(Self::Link),
            0x0007 => Some(Self::ExternalFileList),
            0x0008 => Some(Self::DataLayout),
            0x0009 => Some(Self::Bogus),
            0x000A => Some(Self::GroupInfo),
            0x000B => Some(Self::FilterPipeline),
            0x000C => Some(Self::Attribute),
            0x000D => Some(Self::ObjectComment),
            0x000E => Some(Self::ObjectModTimeOld),
            0x000F => Some(Self::SharedMessageTable),
            0x0010 => Some(Self::ObjectHeaderContinuation),
            0x0011 => Some(Self::SymbolTable),
            0x0012 => Some(Self::ObjectModTime),
            0x0013 => Some(Self::BTreeKValues),
            0x0014 => Some(Self::DriverInfo),
            0x0015 => Some(Self::AttributeInfo),
            0x0016 => Some(Self::ObjectRefCount),
            _ => None,
        }
    }
}
/// Extended variable information for NetCDF-4
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Nc4VariableInfo {
    /// Base variable
    pub variable: Variable,
    /// Chunk info (if chunked)
    pub chunk_info: Option<ChunkInfo>,
    /// Fill value
    pub fill_value: Option<AttributeValue>,
    /// Data offset in file
    pub data_offset: u64,
    /// Data size in bytes
    pub data_size: u64,
    /// Is compressed
    pub is_compressed: bool,
}
impl Nc4VariableInfo {
    /// Create new variable info
    pub fn new(variable: Variable) -> Self {
        Self {
            variable,
            chunk_info: None,
            fill_value: None,
            data_offset: 0,
            data_size: 0,
            is_compressed: false,
        }
    }
    /// Get variable name
    #[must_use]
    pub fn name(&self) -> &str {
        self.variable.name()
    }
    /// Get data type
    #[must_use]
    pub fn data_type(&self) -> DataType {
        self.variable.data_type()
    }
    /// Check if this is chunked storage
    #[must_use]
    pub fn is_chunked(&self) -> bool {
        self.chunk_info.is_some()
    }
}
/// Byte order for HDF5 data
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Hdf5ByteOrder {
    /// Little-endian
    #[default]
    LittleEndian,
    /// Big-endian
    BigEndian,
}
/// HDF5 superblock information
#[derive(Debug, Clone)]
pub struct Hdf5Superblock {
    /// Superblock version
    pub version: Hdf5SuperblockVersion,
    /// Size of offsets in bytes
    pub offset_size: u8,
    /// Size of lengths in bytes
    pub length_size: u8,
    /// Group leaf node K
    pub group_leaf_k: u16,
    /// Group internal node K
    pub group_internal_k: u16,
    /// Base address
    pub base_address: u64,
    /// Root group object header address
    pub root_group_address: u64,
    /// End of file address
    pub eof_address: u64,
}
impl Hdf5Superblock {
    /// Parse superblock from reader
    pub fn parse<R: Read + Seek>(reader: &mut R) -> Result<Self> {
        let mut signature = [0u8; 8];
        reader
            .read_exact(&mut signature)
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        if signature != HDF5_SIGNATURE {
            return Err(NetCdfError::InvalidFormat(
                "Not a valid HDF5/NetCDF-4 file: invalid signature".to_string(),
            ));
        }
        let version_byte = reader
            .read_u8()
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        let version = Hdf5SuperblockVersion::from_byte(version_byte)?;
        match version {
            Hdf5SuperblockVersion::V0 | Hdf5SuperblockVersion::V1 => {
                Self::parse_v0_v1(reader, version)
            }
            Hdf5SuperblockVersion::V2 | Hdf5SuperblockVersion::V3 => {
                Self::parse_v2_v3(reader, version)
            }
        }
    }
    /// Parse superblock version 0 or 1
    fn parse_v0_v1<R: Read + Seek>(reader: &mut R, version: Hdf5SuperblockVersion) -> Result<Self> {
        let _free_space_version = reader
            .read_u8()
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        let _root_group_version = reader
            .read_u8()
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        let _reserved1 = reader
            .read_u8()
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        let _shared_header_version = reader
            .read_u8()
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        let offset_size = reader
            .read_u8()
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        let length_size = reader
            .read_u8()
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        let _reserved2 = reader
            .read_u8()
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        let group_leaf_k = reader
            .read_u16::<LittleEndian>()
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        let group_internal_k = reader
            .read_u16::<LittleEndian>()
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        let _file_consistency_flags = reader
            .read_u32::<LittleEndian>()
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        if matches!(version, Hdf5SuperblockVersion::V1) {
            let _indexed_storage_k = reader
                .read_u16::<LittleEndian>()
                .map_err(|e| NetCdfError::Io(e.to_string()))?;
            let _reserved3 = reader
                .read_u16::<LittleEndian>()
                .map_err(|e| NetCdfError::Io(e.to_string()))?;
        }
        let base_address = Self::read_offset(reader, offset_size)?;
        let _free_space_address = Self::read_offset(reader, offset_size)?;
        let eof_address = Self::read_offset(reader, offset_size)?;
        let _driver_info_address = Self::read_offset(reader, offset_size)?;
        let _link_name_offset = Self::read_offset(reader, offset_size)?;
        let root_group_address = Self::read_offset(reader, offset_size)?;
        Ok(Self {
            version,
            offset_size,
            length_size,
            group_leaf_k,
            group_internal_k,
            base_address,
            root_group_address,
            eof_address,
        })
    }
    /// Parse superblock version 2 or 3
    fn parse_v2_v3<R: Read + Seek>(reader: &mut R, version: Hdf5SuperblockVersion) -> Result<Self> {
        let offset_size = reader
            .read_u8()
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        let length_size = reader
            .read_u8()
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        let _file_consistency_flags = reader
            .read_u8()
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        let base_address = Self::read_offset(reader, offset_size)?;
        let _sb_ext_address = Self::read_offset(reader, offset_size)?;
        let eof_address = Self::read_offset(reader, offset_size)?;
        let root_group_address = Self::read_offset(reader, offset_size)?;
        let _checksum = reader
            .read_u32::<LittleEndian>()
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        Ok(Self {
            version,
            offset_size,
            length_size,
            group_leaf_k: 4,
            group_internal_k: 16,
            base_address,
            root_group_address,
            eof_address,
        })
    }
    /// Read offset value
    fn read_offset<R: Read>(reader: &mut R, size: u8) -> Result<u64> {
        match size {
            1 => Ok(u64::from(
                reader
                    .read_u8()
                    .map_err(|e| NetCdfError::Io(e.to_string()))?,
            )),
            2 => Ok(u64::from(
                reader
                    .read_u16::<LittleEndian>()
                    .map_err(|e| NetCdfError::Io(e.to_string()))?,
            )),
            4 => Ok(u64::from(
                reader
                    .read_u32::<LittleEndian>()
                    .map_err(|e| NetCdfError::Io(e.to_string()))?,
            )),
            8 => reader
                .read_u64::<LittleEndian>()
                .map_err(|e| NetCdfError::Io(e.to_string())),
            _ => Err(NetCdfError::InvalidFormat(format!(
                "Invalid offset size: {}",
                size
            ))),
        }
    }
    /// Read length value
    fn read_length<R: Read>(reader: &mut R, size: u8) -> Result<u64> {
        Self::read_offset(reader, size)
    }
}
/// A group in a NetCDF-4 file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Nc4Group {
    /// Group name
    name: String,
    /// Full path
    path: String,
    /// Dimensions defined in this group
    dimensions: Dimensions,
    /// Variables defined in this group
    variables: Variables,
    /// Attributes
    attributes: Attributes,
    /// Child groups
    groups: Vec<Nc4Group>,
}
impl Nc4Group {
    /// Create a new root group
    #[must_use]
    pub fn root() -> Self {
        Self {
            name: String::new(),
            path: "/".to_string(),
            dimensions: Dimensions::new(),
            variables: Variables::new(),
            attributes: Attributes::new(),
            groups: Vec::new(),
        }
    }
    /// Create a new named group
    pub fn new(name: impl Into<String>, parent_path: &str) -> Result<Self> {
        let name = name.into();
        if name.is_empty() {
            return Err(NetCdfError::InvalidFormat(
                "Group name cannot be empty".to_string(),
            ));
        }
        let path = if parent_path == "/" {
            format!("/{}", name)
        } else {
            format!("{}/{}", parent_path, name)
        };
        Ok(Self {
            name,
            path,
            dimensions: Dimensions::new(),
            variables: Variables::new(),
            attributes: Attributes::new(),
            groups: Vec::new(),
        })
    }
    /// Get group name
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Get full path
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
    /// Get dimensions
    #[must_use]
    pub const fn dimensions(&self) -> &Dimensions {
        &self.dimensions
    }
    /// Get mutable dimensions
    pub fn dimensions_mut(&mut self) -> &mut Dimensions {
        &mut self.dimensions
    }
    /// Get variables
    #[must_use]
    pub const fn variables(&self) -> &Variables {
        &self.variables
    }
    /// Get mutable variables
    pub fn variables_mut(&mut self) -> &mut Variables {
        &mut self.variables
    }
    /// Get attributes
    #[must_use]
    pub const fn attributes(&self) -> &Attributes {
        &self.attributes
    }
    /// Get mutable attributes
    pub fn attributes_mut(&mut self) -> &mut Attributes {
        &mut self.attributes
    }
    /// Get child groups
    #[must_use]
    pub fn groups(&self) -> &[Nc4Group] {
        &self.groups
    }
    /// Add a child group
    pub fn add_group(&mut self, group: Nc4Group) {
        self.groups.push(group);
    }
    /// Find a group by path (relative to this group)
    #[must_use]
    pub fn find_group(&self, path: &str) -> Option<&Nc4Group> {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        self.find_group_recursive(&parts)
    }
    fn find_group_recursive(&self, parts: &[&str]) -> Option<&Nc4Group> {
        if parts.is_empty() {
            return Some(self);
        }
        let first = parts[0];
        for group in &self.groups {
            if group.name == first {
                return group.find_group_recursive(&parts[1..]);
            }
        }
        None
    }
    /// Get all dimension names (including parent groups)
    #[must_use]
    pub fn all_dimension_names(&self) -> Vec<String> {
        self.dimensions
            .names()
            .iter()
            .map(|s| (*s).to_string())
            .collect()
    }
}
/// HDF5 datatype class
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Hdf5DatatypeClass {
    /// Fixed-point (integer)
    FixedPoint = 0,
    /// Floating-point
    FloatingPoint = 1,
    /// Time
    Time = 2,
    /// String
    String = 3,
    /// Bitfield
    Bitfield = 4,
    /// Opaque
    Opaque = 5,
    /// Compound
    Compound = 6,
    /// Reference
    Reference = 7,
    /// Enumeration
    Enum = 8,
    /// Variable-length
    VarLen = 9,
    /// Array
    Array = 10,
}
impl Hdf5DatatypeClass {
    /// Create from byte value
    pub(crate) fn from_byte(value: u8) -> Result<Self> {
        match value & 0x0F {
            0 => Ok(Self::FixedPoint),
            1 => Ok(Self::FloatingPoint),
            2 => Ok(Self::Time),
            3 => Ok(Self::String),
            4 => Ok(Self::Bitfield),
            5 => Ok(Self::Opaque),
            6 => Ok(Self::Compound),
            7 => Ok(Self::Reference),
            8 => Ok(Self::Enum),
            9 => Ok(Self::VarLen),
            10 => Ok(Self::Array),
            _ => Err(NetCdfError::InvalidFormat(format!(
                "Unknown HDF5 datatype class: {}",
                value
            ))),
        }
    }
}
/// Pure Rust NetCDF-4 file reader
pub struct Nc4Reader {
    /// File reader
    reader: BufReader<File>,
    /// HDF5 superblock
    superblock: Hdf5Superblock,
    /// Root group
    root_group: Nc4Group,
    /// Variable info cache
    variable_info: HashMap<String, Nc4VariableInfo>,
}
impl Nc4Reader {
    /// Open a NetCDF-4 file for reading
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(path.as_ref()).map_err(|_| NetCdfError::FileNotFound {
            path: path.as_ref().to_string_lossy().to_string(),
        })?;
        let mut reader = BufReader::new(file);
        let superblock = Hdf5Superblock::parse(&mut reader)?;
        let mut nc4_reader = Self {
            reader,
            superblock,
            root_group: Nc4Group::root(),
            variable_info: HashMap::new(),
        };
        nc4_reader.parse_root_group()?;
        Ok(nc4_reader)
    }
    /// Parse the root group structure
    fn parse_root_group(&mut self) -> Result<()> {
        self.reader
            .seek(SeekFrom::Start(self.superblock.root_group_address))
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        self.root_group = Nc4Group::root();
        Ok(())
    }
    /// Get the root group
    #[must_use]
    pub fn root_group(&self) -> &Nc4Group {
        &self.root_group
    }
    /// Get dimensions from root group
    #[must_use]
    pub fn dimensions(&self) -> &Dimensions {
        self.root_group.dimensions()
    }
    /// Get variables from root group
    #[must_use]
    pub fn variables(&self) -> &Variables {
        self.root_group.variables()
    }
    /// Get global attributes
    #[must_use]
    pub fn global_attributes(&self) -> &Attributes {
        self.root_group.attributes()
    }
    /// Get a group by path
    #[must_use]
    pub fn group(&self, path: &str) -> Option<&Nc4Group> {
        self.root_group.find_group(path)
    }
    /// Read variable data as f32
    pub fn read_f32(&mut self, var_name: &str) -> Result<Vec<f32>> {
        let info = self
            .variable_info
            .get(var_name)
            .ok_or_else(|| NetCdfError::VariableNotFound {
                name: var_name.to_string(),
            })?
            .clone();
        self.reader
            .seek(SeekFrom::Start(info.data_offset))
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        let mut raw_data = vec![0u8; info.data_size as usize];
        self.reader
            .read_exact(&mut raw_data)
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        let data = if let Some(ref chunk_info) = info.chunk_info {
            self.decompress_data(&raw_data, chunk_info)?
        } else {
            raw_data
        };
        let n_elements = data.len() / 4;
        let mut result = Vec::with_capacity(n_elements);
        for chunk in data.chunks_exact(4) {
            result.push(LittleEndian::read_f32(chunk));
        }
        Ok(result)
    }
    /// Read variable data as f64
    pub fn read_f64(&mut self, var_name: &str) -> Result<Vec<f64>> {
        let info = self
            .variable_info
            .get(var_name)
            .ok_or_else(|| NetCdfError::VariableNotFound {
                name: var_name.to_string(),
            })?
            .clone();
        self.reader
            .seek(SeekFrom::Start(info.data_offset))
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        let mut raw_data = vec![0u8; info.data_size as usize];
        self.reader
            .read_exact(&mut raw_data)
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        let data = if let Some(ref chunk_info) = info.chunk_info {
            self.decompress_data(&raw_data, chunk_info)?
        } else {
            raw_data
        };
        let n_elements = data.len() / 8;
        let mut result = Vec::with_capacity(n_elements);
        for chunk in data.chunks_exact(8) {
            result.push(LittleEndian::read_f64(chunk));
        }
        Ok(result)
    }
    /// Read variable data as i32
    pub fn read_i32(&mut self, var_name: &str) -> Result<Vec<i32>> {
        let info = self
            .variable_info
            .get(var_name)
            .ok_or_else(|| NetCdfError::VariableNotFound {
                name: var_name.to_string(),
            })?
            .clone();
        self.reader
            .seek(SeekFrom::Start(info.data_offset))
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        let mut raw_data = vec![0u8; info.data_size as usize];
        self.reader
            .read_exact(&mut raw_data)
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        let data = if let Some(ref chunk_info) = info.chunk_info {
            self.decompress_data(&raw_data, chunk_info)?
        } else {
            raw_data
        };
        let n_elements = data.len() / 4;
        let mut result = Vec::with_capacity(n_elements);
        for chunk in data.chunks_exact(4) {
            result.push(LittleEndian::read_i32(chunk));
        }
        Ok(result)
    }
    /// Read a chunk of data
    pub fn read_chunk(&mut self, var_name: &str, chunk_indices: &[usize]) -> Result<Vec<u8>> {
        let info = self
            .variable_info
            .get(var_name)
            .ok_or_else(|| NetCdfError::VariableNotFound {
                name: var_name.to_string(),
            })?
            .clone();
        let chunk_info = info
            .chunk_info
            .as_ref()
            .ok_or_else(|| NetCdfError::InvalidFormat("Variable is not chunked".to_string()))?;
        let chunk_key = chunk_indices.to_vec();
        let chunk_offset =
            chunk_info
                .chunk_offsets
                .get(&chunk_key)
                .ok_or_else(|| NetCdfError::InvalidShape {
                    message: format!("Chunk not found: {:?}", chunk_indices),
                })?;
        self.reader
            .seek(SeekFrom::Start(*chunk_offset))
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        let element_size = info.data_type().size();
        let chunk_elements: usize = chunk_info.dims.iter().product();
        let chunk_size = chunk_elements * element_size;
        let mut data = vec![0u8; chunk_size];
        self.reader
            .read_exact(&mut data)
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        self.decompress_data(&data, chunk_info)
    }
    /// Decompress data based on filter pipeline
    fn decompress_data(&self, data: &[u8], chunk_info: &ChunkInfo) -> Result<Vec<u8>> {
        let mut result = data.to_vec();
        for filter in chunk_info.filters.iter().rev() {
            result = match filter {
                CompressionFilter::Deflate(_) => {
                    let uncompressed_size = chunk_info.dims.iter().product::<usize>() * 8;
                    decompress_deflate(&result, uncompressed_size)?
                }
                CompressionFilter::Shuffle => unshuffle_data(&result, 4),
                CompressionFilter::Fletcher32 => {
                    if result.len() >= 4 {
                        result.truncate(result.len() - 4);
                    }
                    result
                }
                _ => result,
            };
        }
        Ok(result)
    }
    /// Check if file is a valid NetCDF-4 file
    pub fn is_netcdf4<P: AsRef<Path>>(path: P) -> bool {
        if let Ok(file) = File::open(path) {
            let mut reader = BufReader::new(file);
            Hdf5Superblock::parse(&mut reader).is_ok()
        } else {
            false
        }
    }
}
/// Pure Rust NetCDF-4 file writer
pub struct Nc4Writer {
    /// File writer
    writer: BufWriter<File>,
    /// Superblock version to use
    superblock_version: Hdf5SuperblockVersion,
    /// Offset size
    offset_size: u8,
    /// Length size
    length_size: u8,
    /// Root group
    root_group: Nc4Group,
    /// Variable info
    variable_info: HashMap<String, Nc4VariableInfo>,
    /// Current file offset
    current_offset: u64,
    /// Is in define mode
    in_define_mode: bool,
}
impl Nc4Writer {
    /// Create a new NetCDF-4 file
    pub fn create<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::create(path.as_ref())
            .map_err(|e| NetCdfError::Io(format!("Failed to create file: {}", e)))?;
        let writer = BufWriter::new(file);
        Ok(Self {
            writer,
            superblock_version: Hdf5SuperblockVersion::V2,
            offset_size: 8,
            length_size: 8,
            root_group: Nc4Group::root(),
            variable_info: HashMap::new(),
            current_offset: 0,
            in_define_mode: true,
        })
    }
    /// Add a dimension
    pub fn add_dimension(&mut self, dim: Dimension) -> Result<()> {
        if !self.in_define_mode {
            return Err(NetCdfError::InvalidFormat(
                "Cannot add dimension in data mode".to_string(),
            ));
        }
        self.root_group.dimensions_mut().add(dim)
    }
    /// Add a variable
    pub fn add_variable(&mut self, var: Variable) -> Result<()> {
        if !self.in_define_mode {
            return Err(NetCdfError::InvalidFormat(
                "Cannot add variable in data mode".to_string(),
            ));
        }
        let var_info = Nc4VariableInfo::new(var.clone());
        self.variable_info.insert(var.name().to_string(), var_info);
        self.root_group.variables_mut().add(var)
    }
    /// Add a global attribute
    pub fn add_global_attribute(&mut self, attr: Attribute) -> Result<()> {
        self.root_group.attributes_mut().add(attr)
    }
    /// Add a variable attribute
    pub fn add_variable_attribute(&mut self, var_name: &str, attr: Attribute) -> Result<()> {
        let var = self
            .root_group
            .variables_mut()
            .get_mut(var_name)
            .ok_or_else(|| NetCdfError::VariableNotFound {
                name: var_name.to_string(),
            })?;
        var.attributes_mut().add(attr)
    }
    /// Set chunking for a variable
    pub fn set_chunking(&mut self, var_name: &str, chunk_dims: Vec<usize>) -> Result<()> {
        if !self.in_define_mode {
            return Err(NetCdfError::InvalidFormat(
                "Cannot set chunking in data mode".to_string(),
            ));
        }
        let info =
            self.variable_info
                .get_mut(var_name)
                .ok_or_else(|| NetCdfError::VariableNotFound {
                    name: var_name.to_string(),
                })?;
        info.chunk_info = Some(ChunkInfo::new(chunk_dims));
        Ok(())
    }
    /// Set compression for a variable
    pub fn set_compression(&mut self, var_name: &str, level: u8) -> Result<()> {
        if !self.in_define_mode {
            return Err(NetCdfError::InvalidFormat(
                "Cannot set compression in data mode".to_string(),
            ));
        }
        let info =
            self.variable_info
                .get_mut(var_name)
                .ok_or_else(|| NetCdfError::VariableNotFound {
                    name: var_name.to_string(),
                })?;
        if let Some(ref mut chunk_info) = info.chunk_info {
            chunk_info.add_filter(CompressionFilter::Deflate(level));
            info.is_compressed = true;
        } else {
            return Err(NetCdfError::InvalidFormat(
                "Must set chunking before compression".to_string(),
            ));
        }
        Ok(())
    }
    /// Enable shuffle filter for a variable
    pub fn set_shuffle(&mut self, var_name: &str) -> Result<()> {
        if !self.in_define_mode {
            return Err(NetCdfError::InvalidFormat(
                "Cannot set shuffle in data mode".to_string(),
            ));
        }
        let info =
            self.variable_info
                .get_mut(var_name)
                .ok_or_else(|| NetCdfError::VariableNotFound {
                    name: var_name.to_string(),
                })?;
        if let Some(ref mut chunk_info) = info.chunk_info {
            chunk_info.add_filter(CompressionFilter::Shuffle);
        } else {
            return Err(NetCdfError::InvalidFormat(
                "Must set chunking before shuffle".to_string(),
            ));
        }
        Ok(())
    }
    /// End define mode and write header
    pub fn end_define_mode(&mut self) -> Result<()> {
        if !self.in_define_mode {
            return Ok(());
        }
        self.write_superblock()?;
        self.write_root_group()?;
        self.in_define_mode = false;
        Ok(())
    }
    /// Write HDF5 superblock
    fn write_superblock(&mut self) -> Result<()> {
        self.writer
            .write_all(&HDF5_SIGNATURE)
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        self.current_offset += 8;
        match self.superblock_version {
            Hdf5SuperblockVersion::V2 | Hdf5SuperblockVersion::V3 => {
                self.writer
                    .write_u8(2)
                    .map_err(|e| NetCdfError::Io(e.to_string()))?;
                self.writer
                    .write_u8(self.offset_size)
                    .map_err(|e| NetCdfError::Io(e.to_string()))?;
                self.writer
                    .write_u8(self.length_size)
                    .map_err(|e| NetCdfError::Io(e.to_string()))?;
                self.writer
                    .write_u8(0)
                    .map_err(|e| NetCdfError::Io(e.to_string()))?;
                self.write_offset(0)?;
                self.write_offset(u64::MAX)?;
                self.write_offset(0)?;
                let root_group_offset = 48u64;
                self.write_offset(root_group_offset)?;
                self.writer
                    .write_u32::<LittleEndian>(0)
                    .map_err(|e| NetCdfError::Io(e.to_string()))?;
                self.current_offset = 48;
            }
            _ => {
                return Err(NetCdfError::InvalidFormat(
                    "Only superblock version 2/3 supported for writing".to_string(),
                ));
            }
        }
        Ok(())
    }
    /// Write offset value
    fn write_offset(&mut self, value: u64) -> Result<()> {
        match self.offset_size {
            1 => self
                .writer
                .write_u8(value as u8)
                .map_err(|e| NetCdfError::Io(e.to_string()))?,
            2 => self
                .writer
                .write_u16::<LittleEndian>(value as u16)
                .map_err(|e| NetCdfError::Io(e.to_string()))?,
            4 => self
                .writer
                .write_u32::<LittleEndian>(value as u32)
                .map_err(|e| NetCdfError::Io(e.to_string()))?,
            8 => self
                .writer
                .write_u64::<LittleEndian>(value)
                .map_err(|e| NetCdfError::Io(e.to_string()))?,
            _ => {
                return Err(NetCdfError::InvalidFormat(format!(
                    "Invalid offset size: {}",
                    self.offset_size
                )));
            }
        }
        self.current_offset += u64::from(self.offset_size);
        Ok(())
    }
    /// Write root group structure
    fn write_root_group(&mut self) -> Result<()> {
        self.writer
            .write_all(b"OHDR")
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        self.writer
            .write_u8(2)
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        self.writer
            .write_u8(0)
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        self.writer
            .write_u16::<LittleEndian>(0)
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        self.writer
            .write_u16::<LittleEndian>(0)
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        let chunk_size = 64u32;
        self.writer
            .write_u32::<LittleEndian>(chunk_size)
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        self.current_offset += 16;
        Ok(())
    }
    /// Write f32 data to a variable
    pub fn write_f32(&mut self, var_name: &str, data: &[f32]) -> Result<()> {
        if self.in_define_mode {
            self.end_define_mode()?;
        }
        let info = self
            .variable_info
            .get(var_name)
            .ok_or_else(|| NetCdfError::VariableNotFound {
                name: var_name.to_string(),
            })?
            .clone();
        let mut raw_data = Vec::with_capacity(data.len() * 4);
        for &value in data {
            raw_data
                .write_f32::<LittleEndian>(value)
                .map_err(|e| NetCdfError::Io(e.to_string()))?;
        }
        let write_data = if let Some(ref chunk_info) = info.chunk_info {
            self.compress_data(&raw_data, chunk_info, 4)?
        } else {
            raw_data
        };
        self.writer
            .write_all(&write_data)
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        if let Some(var_info) = self.variable_info.get_mut(var_name) {
            var_info.data_offset = self.current_offset;
            var_info.data_size = write_data.len() as u64;
        }
        self.current_offset += write_data.len() as u64;
        Ok(())
    }
    /// Write f64 data to a variable
    pub fn write_f64(&mut self, var_name: &str, data: &[f64]) -> Result<()> {
        if self.in_define_mode {
            self.end_define_mode()?;
        }
        let info = self
            .variable_info
            .get(var_name)
            .ok_or_else(|| NetCdfError::VariableNotFound {
                name: var_name.to_string(),
            })?
            .clone();
        let mut raw_data = Vec::with_capacity(data.len() * 8);
        for &value in data {
            raw_data
                .write_f64::<LittleEndian>(value)
                .map_err(|e| NetCdfError::Io(e.to_string()))?;
        }
        let write_data = if let Some(ref chunk_info) = info.chunk_info {
            self.compress_data(&raw_data, chunk_info, 8)?
        } else {
            raw_data
        };
        self.writer
            .write_all(&write_data)
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        if let Some(var_info) = self.variable_info.get_mut(var_name) {
            var_info.data_offset = self.current_offset;
            var_info.data_size = write_data.len() as u64;
        }
        self.current_offset += write_data.len() as u64;
        Ok(())
    }
    /// Write i32 data to a variable
    pub fn write_i32(&mut self, var_name: &str, data: &[i32]) -> Result<()> {
        if self.in_define_mode {
            self.end_define_mode()?;
        }
        let info = self
            .variable_info
            .get(var_name)
            .ok_or_else(|| NetCdfError::VariableNotFound {
                name: var_name.to_string(),
            })?
            .clone();
        let mut raw_data = Vec::with_capacity(data.len() * 4);
        for &value in data {
            raw_data
                .write_i32::<LittleEndian>(value)
                .map_err(|e| NetCdfError::Io(e.to_string()))?;
        }
        let write_data = if let Some(ref chunk_info) = info.chunk_info {
            self.compress_data(&raw_data, chunk_info, 4)?
        } else {
            raw_data
        };
        self.writer
            .write_all(&write_data)
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        if let Some(var_info) = self.variable_info.get_mut(var_name) {
            var_info.data_offset = self.current_offset;
            var_info.data_size = write_data.len() as u64;
        }
        self.current_offset += write_data.len() as u64;
        Ok(())
    }
    /// Compress data based on filter pipeline
    fn compress_data(
        &self,
        data: &[u8],
        chunk_info: &ChunkInfo,
        element_size: usize,
    ) -> Result<Vec<u8>> {
        let mut result = data.to_vec();
        for filter in &chunk_info.filters {
            result = match filter {
                CompressionFilter::Shuffle => shuffle_data(&result, element_size),
                CompressionFilter::Deflate(level) => compress_deflate(&result, *level)?,
                CompressionFilter::Fletcher32 => {
                    let checksum = fletcher32(&result);
                    result.extend_from_slice(&checksum.to_le_bytes());
                    result
                }
                _ => result,
            };
        }
        Ok(result)
    }
    /// Close the file
    pub fn close(mut self) -> Result<()> {
        if self.in_define_mode {
            self.end_define_mode()?;
        }
        self.writer
            .flush()
            .map_err(|e| NetCdfError::Io(e.to_string()))?;
        Ok(())
    }
    /// Add a new group
    pub fn add_group(&mut self, path: &str) -> Result<()> {
        if !self.in_define_mode {
            return Err(NetCdfError::InvalidFormat(
                "Cannot add group in data mode".to_string(),
            ));
        }
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() {
            return Err(NetCdfError::InvalidFormat("Invalid group path".to_string()));
        }
        let mut current_path = String::new();
        for part in parts {
            let parent_path = if current_path.is_empty() {
                "/".to_string()
            } else {
                current_path.clone()
            };
            let new_group = Nc4Group::new(part, &parent_path)?;
            current_path = new_group.path().to_string();
            self.root_group.add_group(new_group);
        }
        Ok(())
    }
}
/// Compression filter type for NetCDF-4 data
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionFilter {
    /// No compression
    None,
    /// Deflate (zlib) compression with level 0-9
    Deflate(u8),
    /// Shuffle filter (byte reordering)
    Shuffle,
    /// Fletcher32 checksum
    Fletcher32,
    /// SZIP compression
    Szip,
    /// LZF compression
    Lzf,
    /// Blosc compression
    Blosc,
}
impl CompressionFilter {
    /// Get the HDF5 filter ID
    #[must_use]
    pub const fn filter_id(&self) -> u16 {
        match self {
            Self::None => 0,
            Self::Deflate(_) => 1,
            Self::Shuffle => 2,
            Self::Fletcher32 => 3,
            Self::Szip => 4,
            Self::Lzf => 32000,
            Self::Blosc => 32001,
        }
    }
}
