//! HDF5 file reader backed by the real Pure-Rust [`oxih5`] crate.
//!
//! [`Hdf5Reader::open`] opens a genuine HDF5 file, walks its group/dataset tree
//! via `oxih5`, and exposes real attributes, real datatypes, real shapes, and
//! real dataset values. The legacy `OXIGDAL_HDF5_METADATA_V1` JSON sidecar
//! reader (the 0.1.x-era on-disk format identifier, removed) has been retired —
//! this reader never fabricates zero-filled data.
//!
//! Numeric dataset bytes are normalized to little-endian at read time
//! (`build_dataset`'s big-endian byte-swap, driven by `is_big_endian`)
//! before being cached, so a real file whose Datatype message declares
//! big-endian order is read correctly rather than silently misinterpreted —
//! every downstream consumer of [`Dataset::data`] can keep assuming
//! little-endian.
//!
//! Chunk dimensions and filter pipelines are recovered directly from a
//! dataset's real (V1) object header via `object_header`, making
//! [`Hdf5Reader::decode_chunk`] usable against real chunked/filtered files.

use crate::attribute::{Attribute, Attributes};
use crate::convert;
use crate::dataset::{Dataset, DatasetProperties};
use crate::datatype::{Datatype, TypeConverter};
use crate::error::{Hdf5Error, Result};
use crate::group::{Group, PathUtils};
use crate::superblock_v2::{read_superblock_v2, validate_superblock_checksum};
use byteorder::{LittleEndian, ReadBytesExt};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// HDF5 file signature
const HDF5_SIGNATURE: &[u8] = b"\x89HDF\r\n\x1a\n";

/// HDF5 superblock version
#[derive(Debug, Clone, Copy)]
pub enum SuperblockVersion {
    /// Version 0 (HDF5 1.0)
    V0,
    /// Version 1 (HDF5 1.2)
    V1,
    /// Version 2 (HDF5 1.8)
    V2,
    /// Version 3 (HDF5 1.10)
    V3,
}

/// HDF5 superblock information
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Superblock {
    /// Superblock version
    version: SuperblockVersion,
    /// Size of offsets (in bytes)
    size_of_offsets: u8,
    /// Size of lengths (in bytes)
    size_of_lengths: u8,
    /// Base address
    base_address: u64,
    /// Root group object header address
    root_group_address: u64,
}

impl Superblock {
    /// Read superblock from file
    fn read<R: Read + Seek>(reader: &mut R) -> Result<Self> {
        // Read and verify signature
        let mut signature = vec![0u8; 8];
        reader.read_exact(&mut signature)?;

        if signature != HDF5_SIGNATURE {
            return Err(Hdf5Error::InvalidSignature(signature));
        }

        // Read superblock version
        let version_num = reader.read_u8()?;
        let version = match version_num {
            0 => SuperblockVersion::V0,
            1 => SuperblockVersion::V1,
            2 => SuperblockVersion::V2,
            3 => SuperblockVersion::V3,
            _ => return Err(Hdf5Error::UnsupportedSuperblockVersion(version_num)),
        };

        match version {
            SuperblockVersion::V0 | SuperblockVersion::V1 => {
                // Free-space storage version
                let _free_space_version = reader.read_u8()?;
                // Root group symbol table version
                let _root_group_version = reader.read_u8()?;
                // Reserved
                let _reserved1 = reader.read_u8()?;
                // Shared header message format version
                let _shared_header_version = reader.read_u8()?;
                // Size of offsets
                let size_of_offsets = reader.read_u8()?;
                // Size of lengths
                let size_of_lengths = reader.read_u8()?;
                // Reserved
                let _reserved2 = reader.read_u8()?;
                // Group leaf node K
                let _group_leaf_node_k = reader.read_u16::<LittleEndian>()?;
                // Group internal node K
                let _group_internal_node_k = reader.read_u16::<LittleEndian>()?;
                // File consistency flags
                let _file_consistency_flags = reader.read_u32::<LittleEndian>()?;

                // For version 1, read additional fields
                if matches!(version, SuperblockVersion::V1) {
                    let _indexed_storage_internal_node_k = reader.read_u16::<LittleEndian>()?;
                    let _reserved3 = reader.read_u16::<LittleEndian>()?;
                }

                // Base address
                let base_address = Self::read_offset(reader, size_of_offsets)?;
                // Address of file free space info
                let _free_space_address = Self::read_offset(reader, size_of_offsets)?;
                // End of file address
                let _end_of_file_address = Self::read_offset(reader, size_of_offsets)?;
                // Driver information block address
                let _driver_info_address = Self::read_offset(reader, size_of_offsets)?;
                // Root group symbol table entry
                let root_group_address = Self::read_offset(reader, size_of_offsets)?;

                Ok(Self {
                    version,
                    size_of_offsets,
                    size_of_lengths,
                    base_address,
                    root_group_address,
                })
            }
            SuperblockVersion::V2 | SuperblockVersion::V3 => {
                // Build a byte accumulator that already contains the bytes read
                // so far (signature + version) so that checksum validation can
                // cover the full superblock prefix.
                let mut header_bytes: Vec<u8> = Vec::with_capacity(64);
                header_bytes.extend_from_slice(HDF5_SIGNATURE);
                header_bytes.push(version_num);

                let v2 = read_superblock_v2(reader, &mut header_bytes)?;
                validate_superblock_checksum(&header_bytes)?;

                Ok(Self {
                    version,
                    size_of_offsets: v2.size_of_offsets,
                    size_of_lengths: v2.size_of_lengths,
                    base_address: v2.base_address,
                    root_group_address: v2.root_group_object_header_address,
                })
            }
        }
    }

    /// Read offset value
    fn read_offset<R: Read>(reader: &mut R, size: u8) -> Result<u64> {
        match size {
            4 => Ok(reader.read_u32::<LittleEndian>()? as u64),
            8 => Ok(reader.read_u64::<LittleEndian>()?),
            _ => Err(Hdf5Error::invalid_format(format!(
                "Invalid offset size: {}",
                size
            ))),
        }
    }

    /// Read length value
    #[allow(dead_code)]
    fn read_length<R: Read>(reader: &mut R, size: u8) -> Result<u64> {
        Self::read_offset(reader, size)
    }
}

/// HDF5 file reader (real HDF5, backed by `oxih5`).
pub struct Hdf5Reader {
    /// File handle (kept open for `file_size`).
    file: File,
    /// Superblock
    superblock: Superblock,
    /// Groups cache (path -> Group), populated from the real file.
    groups: HashMap<String, Group>,
    /// Datasets cache (path -> Dataset), populated from the real file.
    datasets: HashMap<String, Dataset>,
}

impl Hdf5Reader {
    /// Open an HDF5 file for reading.
    ///
    /// The superblock is parsed to determine the format version, then the
    /// group/dataset tree, attributes, and dataset values are read through the
    /// real Pure-Rust `oxih5` reader.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let mut file = File::open(path)?;

        // Parse the superblock (validates the signature and records the version).
        let superblock = Superblock::read(&mut file)?;

        let mut reader = Self {
            file,
            superblock,
            groups: HashMap::new(),
            datasets: HashMap::new(),
        };

        reader.populate(path);

        Ok(reader)
    }

    /// Populate the group/dataset caches from the real file via `oxih5`.
    ///
    /// Best-effort: if `oxih5` cannot parse the body (e.g. an as-yet-unsupported
    /// v2/v3 superblock) the reader is still usable with an empty root group,
    /// matching the historical lenient-open behaviour — but no data is faked.
    fn populate(&mut self, path: &Path) {
        // Always have a root group so the reader is usable even when the body
        // cannot be parsed.
        self.groups
            .entry("/".to_string())
            .or_insert_with(Group::root);

        let h5 = match oxih5::open(path) {
            Ok(f) => f,
            Err(_) => return,
        };

        // Root-group attributes.
        if let Ok(root_group) = h5.root() {
            let attrs = read_group_attributes(&root_group);
            if let Some(root) = self.groups.get_mut("/") {
                *root.attributes_mut() = attrs;
            }
        }

        // Enumerate the entire tree (groups + datasets) by full path.
        let mut group_paths: Vec<String> = Vec::new();
        let mut dataset_paths: Vec<String> = Vec::new();
        let _ = h5.walk(&mut |p, is_group| {
            if is_group {
                group_paths.push(p.to_string());
            } else {
                dataset_paths.push(p.to_string());
            }
        });

        for gp in group_paths {
            if self.groups.contains_key(&gp) {
                continue;
            }
            if let Ok(g) = h5.group(&gp) {
                let name = gp.rsplit('/').next().unwrap_or(gp.as_str()).to_string();
                let mut grp = Group::new(name, gp.clone());
                *grp.attributes_mut() = read_group_attributes(&g);
                self.groups.insert(gp, grp);
            }
        }

        let size_of_offsets = self.superblock.size_of_offsets;
        let size_of_lengths = self.superblock.size_of_lengths;
        for dp in dataset_paths {
            if let Some(ds) =
                build_dataset(&h5, &dp, &mut self.file, size_of_offsets, size_of_lengths)
            {
                self.datasets.insert(dp, ds);
            }
        }
    }

    /// Get the root group
    pub fn root(&self) -> Result<&Group> {
        self.groups
            .get("/")
            .ok_or_else(|| Hdf5Error::internal("Root group not found"))
    }

    /// Get a group by path
    pub fn group(&self, path: &str) -> Result<&Group> {
        let normalized = PathUtils::normalize(path)?;
        self.groups
            .get(&normalized)
            .ok_or_else(|| Hdf5Error::group_not_found(path))
    }

    /// Get a dataset by path
    pub fn dataset(&self, path: &str) -> Result<&Dataset> {
        let normalized = PathUtils::normalize(path)?;
        self.datasets
            .get(&normalized)
            .ok_or_else(|| Hdf5Error::dataset_not_found(path))
    }

    /// Check if a path exists
    pub fn exists(&self, path: &str) -> bool {
        let normalized = PathUtils::normalize(path).ok();
        if let Some(path) = normalized {
            self.groups.contains_key(&path) || self.datasets.contains_key(&path)
        } else {
            false
        }
    }

    /// Check if a path is a group
    pub fn is_group(&self, path: &str) -> bool {
        let normalized = PathUtils::normalize(path).ok();
        if let Some(path) = normalized {
            self.groups.contains_key(&path)
        } else {
            false
        }
    }

    /// Check if a path is a dataset
    pub fn is_dataset(&self, path: &str) -> bool {
        let normalized = PathUtils::normalize(path).ok();
        if let Some(path) = normalized {
            self.datasets.contains_key(&path)
        } else {
            false
        }
    }

    /// List all groups
    pub fn list_groups(&self) -> Vec<&str> {
        self.groups.keys().map(|s| s.as_str()).collect()
    }

    /// List all datasets
    pub fn list_datasets(&self) -> Vec<&str> {
        self.datasets.keys().map(|s| s.as_str()).collect()
    }

    /// Read a dataset's decoded element bytes (little-endian, as stored).
    ///
    /// Returns the real dataset values read from the file. Datatypes that are
    /// not decoded to a plain byte buffer (e.g. variable-length or compound
    /// types) return a typed error rather than fabricated zeros.
    pub fn read_dataset_raw(&mut self, path: &str) -> Result<Vec<u8>> {
        let dataset = self.dataset(path)?;
        match dataset.data() {
            Some(data) => Ok(data.to_vec()),
            None => Err(Hdf5Error::feature_not_available(format!(
                "raw data for dataset '{}' (datatype {} is not decoded to a byte buffer)",
                path,
                dataset.datatype().name()
            ))),
        }
    }

    /// Read dataset data as i32 array
    pub fn read_i32(&mut self, path: &str) -> Result<Vec<i32>> {
        let len = {
            let dataset = self.dataset(path)?;
            if !matches!(dataset.datatype(), Datatype::Int32) {
                return Err(Hdf5Error::type_conversion(dataset.datatype().name(), "i32"));
            }
            dataset.len()
        };

        let raw_data = self.read_dataset_raw(path)?;
        // `len` is the header-declared element count; the loop only produces as
        // many elements as `raw_data` actually holds, so cap the preallocation
        // hint to avoid an oversized allocation from an untrusted dimension.
        let mut result = Vec::with_capacity(len.min(raw_data.len() / 4));

        for chunk in raw_data.chunks_exact(4) {
            result.push(TypeConverter::read_i32_le(chunk)?);
        }

        Ok(result)
    }

    /// Read dataset data as f32 array
    pub fn read_f32(&mut self, path: &str) -> Result<Vec<f32>> {
        let len = {
            let dataset = self.dataset(path)?;
            if !matches!(dataset.datatype(), Datatype::Float32) {
                return Err(Hdf5Error::type_conversion(dataset.datatype().name(), "f32"));
            }
            dataset.len()
        };

        let raw_data = self.read_dataset_raw(path)?;
        // Cap the hint to the bytes actually read (see `read_i32`).
        let mut result = Vec::with_capacity(len.min(raw_data.len() / 4));

        for chunk in raw_data.chunks_exact(4) {
            result.push(TypeConverter::read_f32_le(chunk)?);
        }

        Ok(result)
    }

    /// Read dataset data as f64 array
    pub fn read_f64(&mut self, path: &str) -> Result<Vec<f64>> {
        let len = {
            let dataset = self.dataset(path)?;
            if !matches!(dataset.datatype(), Datatype::Float64) {
                return Err(Hdf5Error::type_conversion(dataset.datatype().name(), "f64"));
            }
            dataset.len()
        };

        let raw_data = self.read_dataset_raw(path)?;
        // Cap the hint to the bytes actually read (see `read_i32`).
        let mut result = Vec::with_capacity(len.min(raw_data.len() / 8));

        for chunk in raw_data.chunks_exact(8) {
            result.push(TypeConverter::read_f64_le(chunk)?);
        }

        Ok(result)
    }

    /// Decode a single raw (still-filtered) chunk of a dataset into unfiltered
    /// element bytes.
    ///
    /// This applies the dataset's filter pipeline in reverse (back-to-front, as
    /// libhdf5 does). When the dataset carries no filter pipeline the chunk
    /// bytes are returned unchanged. A filter whose identifier has no decoder
    /// produces a typed error (via
    /// [`crate::filters::FilterPipeline::apply_reverse`]) rather than garbage.
    ///
    /// The dataset must use a chunked layout (its properties must supply chunk
    /// dimensions); a contiguous/compact dataset yields a [`Hdf5Error::Layout`]
    /// error.
    pub fn decode_chunk(&self, path: &str, raw_chunk: &[u8]) -> Result<Vec<u8>> {
        let dataset = self.dataset(path)?;
        let datatype = dataset.datatype().clone();
        let chunk_dims = dataset
            .properties()
            .chunk_dims()
            .ok_or_else(|| {
                Hdf5Error::Layout(format!(
                    "dataset '{}' is not chunked; decode_chunk requires a chunked layout",
                    path
                ))
            })?
            .to_vec();

        match dataset.properties().filter_pipeline() {
            Some(pipeline) if !pipeline.is_empty() => {
                pipeline.apply_reverse(raw_chunk, &datatype, &chunk_dims)
            }
            _ => Ok(raw_chunk.to_vec()),
        }
    }

    /// Read a sub-region (hyperslab) of a dataset's real values.
    ///
    /// `start` and `count` give the per-dimension offset and extent. The slice
    /// is gathered from the real (row-major) dataset bytes. Datatypes that are
    /// not decoded to a byte buffer return a typed error.
    pub fn read_slice(&mut self, path: &str, start: &[usize], count: &[usize]) -> Result<Vec<u8>> {
        let dataset = self.dataset(path)?;
        dataset.validate_slice(start, count)?;

        let elem = dataset.datatype().size();
        let dims = dataset.dims().to_vec();

        match dataset.data() {
            Some(data) => Ok(gather_contiguous_slice(data, &dims, start, count, elem)),
            None => Err(Hdf5Error::feature_not_available(format!(
                "slicing dataset '{}' whose datatype {} is not decoded to a byte buffer",
                path,
                dataset.datatype().name()
            ))),
        }
    }

    /// Get file size
    pub fn file_size(&mut self) -> Result<u64> {
        let size = self.file.seek(SeekFrom::End(0))?;
        self.file.seek(SeekFrom::Start(0))?;
        Ok(size)
    }

    /// Get superblock version
    pub fn superblock_version(&self) -> SuperblockVersion {
        self.superblock.version
    }
}

/// Read all mappable attributes from an `oxih5` group into an [`Attributes`].
fn read_group_attributes(g: &oxih5::Group) -> Attributes {
    let mut attrs = Attributes::new();
    if let Ok(views) = g.attr_views() {
        for v in &views {
            if let Some(val) = convert::decode_attr(v) {
                attrs.add(Attribute::new(v.name().to_string(), val));
            }
        }
    }
    attrs
}

/// Read a single dataset (shape, datatype, attributes, and — where the datatype
/// is a plain byte buffer — its real values) from an `oxih5` file.
///
/// Also recovers the dataset's real chunk dimensions and filter pipeline (if
/// any) by walking its object header directly via [`crate::object_header`],
/// since `oxih5`'s own decoded [`oxih5::Dataset`] view doesn't expose that
/// metadata. This is what makes [`Hdf5Reader::decode_chunk`] usable against
/// real, chunked/filtered files rather than always erroring.
fn build_dataset(
    h5: &oxih5::File,
    path: &str,
    file: &mut File,
    size_of_offsets: u8,
    size_of_lengths: u8,
) -> Option<Dataset> {
    let ohd = h5.dataset(path).ok()?;
    let name = path.rsplit('/').next().unwrap_or(path).to_string();
    let dtype = convert::map_dtype(&ohd.dtype);
    let dims = ohd.shape.clone();

    let mut properties = DatasetProperties::new();
    if let Ok(header_addr) = h5.header_addr_of(path)
        && let Ok(raw_layout) = crate::object_header::read_dataset_layout(
            file,
            header_addr,
            size_of_offsets,
            size_of_lengths,
        )
    {
        // Only trust chunk dims whose rank matches the dataset's real
        // rank — a mismatch means our best-effort object-header walk
        // mis-parsed something, and Dataset::new would otherwise reject
        // the whole dataset outright (validate_chunks) rather than just
        // this one property.
        if let Some(chunk_dims) = raw_layout.chunk_dims
            && chunk_dims.len() == dims.len()
            && chunk_dims
                .iter()
                .zip(dims.iter())
                .all(|(&c, &d)| c > 0 && c <= d)
        {
            properties = properties.with_chunks(chunk_dims);
        }
        if let Some(pipeline) = raw_layout.filter_pipeline {
            properties = properties.with_filter_pipeline(pipeline);
        }
    }

    let mut ds = Dataset::new(name, path.to_string(), dtype.clone(), dims, properties).ok()?;

    if let Ok(views) = h5.attr_views(path) {
        for v in &views {
            if let Some(val) = convert::decode_attr(v) {
                ds.attributes_mut()
                    .add(Attribute::new(v.name().to_string(), val));
            }
        }
    }

    if convert::is_storable(&dtype) && ohd.data.len() == ds.size_in_bytes() {
        // `Hdf5Reader::read_f32`/`read_f64`/`read_i32` (and `read_slice`)
        // decode this crate's own stored bytes assuming little-endian
        // (`TypeConverter::read_*_le`) — oxigeo's [`Datatype`] enum, unlike
        // `oxih5::Dtype`, does not carry a byte-order flag. A real HDF5 file
        // whose numeric datatype message declares big-endian order (common
        // for files written on/for big-endian platforms, or by tools that
        // default to network byte order) would otherwise be silently
        // misread element-by-element. Normalize to little-endian here, once,
        // at read time, so every downstream consumer of `ds.data()` can keep
        // assuming little-endian.
        let mut data = ohd.data;
        if convert::is_big_endian(&ohd.dtype) {
            byte_swap_elements(&mut data, dtype.size());
        }
        let _ = ds.set_data(data);
    }

    Some(ds)
}

/// Reverse the byte order of every fixed-size `elem_size`-byte element in
/// `data` in place (a no-op for `elem_size <= 1`, or when `data.len()` isn't
/// an exact multiple of `elem_size`, since that indicates the caller
/// mismeasured the element size and swapping partial elements would corrupt
/// the buffer worse than leaving it alone).
fn byte_swap_elements(data: &mut [u8], elem_size: usize) {
    if elem_size <= 1 || data.is_empty() || !data.len().is_multiple_of(elem_size) {
        return;
    }
    for chunk in data.chunks_exact_mut(elem_size) {
        chunk.reverse();
    }
}

/// Gather a contiguous (row-major) hyperslab from `data`.
fn gather_contiguous_slice(
    data: &[u8],
    dims: &[usize],
    start: &[usize],
    count: &[usize],
    elem: usize,
) -> Vec<u8> {
    let out_elems: usize = count.iter().product();
    let mut out = vec![0u8; out_elems * elem];
    if out_elems == 0 || dims.is_empty() || elem == 0 {
        return out;
    }

    let ndims = dims.len();
    // Row-major strides.
    let mut strides = vec![1usize; ndims];
    for d in (0..ndims.saturating_sub(1)).rev() {
        strides[d] = strides[d + 1] * dims[d + 1];
    }

    let mut coords = vec![0usize; ndims];
    let mut dst = 0usize;
    loop {
        let mut src_flat = 0usize;
        for d in 0..ndims {
            src_flat += (start[d] + coords[d]) * strides[d];
        }
        let src_off = src_flat * elem;
        let dst_off = dst * elem;
        if src_off + elem <= data.len() && dst_off + elem <= out.len() {
            out[dst_off..dst_off + elem].copy_from_slice(&data[src_off..src_off + elem]);
        }
        dst += 1;

        // Odometer increment over `count`.
        let mut carry = true;
        for d in (0..ndims).rev() {
            if carry {
                coords[d] += 1;
                if coords[d] >= count[d] {
                    coords[d] = 0;
                } else {
                    carry = false;
                }
            }
        }
        if carry {
            break;
        }
    }

    out
}

/// Builder for Hdf5Reader with configuration options
pub struct Hdf5ReaderBuilder {
    /// Cache size
    cache_size: Option<usize>,
}

impl Hdf5ReaderBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self { cache_size: None }
    }

    /// Set cache size
    pub fn cache_size(mut self, size: usize) -> Self {
        self.cache_size = Some(size);
        self
    }

    /// Build the reader
    pub fn open<P: AsRef<Path>>(self, path: P) -> Result<Hdf5Reader> {
        // Cache options are accepted for API compatibility; `oxih5` manages its
        // own chunk-index cache internally.
        Hdf5Reader::open(path)
    }
}

impl Default for Hdf5ReaderBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer::{Hdf5Version, Hdf5Writer};
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    use tempfile::NamedTempFile;

    /// Per-test scratch fixture inside the system temp dir (house policy: no
    /// hardcoded absolute paths).
    ///
    /// The leaf name embeds the process id and a monotonic counter, so no two test
    /// binaries — nor two concurrent runs of this one — can ever land on the same
    /// file.  Dropping the guard removes the fixture, so a panicking test leaks
    /// nothing.
    struct TempPath(std::path::PathBuf);

    impl TempPath {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
            Self(std::env::temp_dir().join(format!(
                "oxigeo_hdf5_reader_{}_{seq}_{name}",
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
        assert_eq!(HDF5_SIGNATURE, b"\x89HDF\r\n\x1a\n");
    }

    #[test]
    fn test_byte_swap_elements_reverses_each_element_independently() {
        // Two 4-byte elements: swapping must reverse each independently, not
        // the whole buffer.
        let mut data = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
        byte_swap_elements(&mut data, 4);
        assert_eq!(data, vec![4, 3, 2, 1, 8, 7, 6, 5]);

        // f64 round-trip: BE bytes of a value, swapped, must decode as LE to
        // the same value.
        let v = 12345.6789_f64;
        let mut be_bytes = v.to_be_bytes().to_vec();
        byte_swap_elements(&mut be_bytes, 8);
        assert_eq!(f64::from_le_bytes(be_bytes.try_into().expect("8 bytes")), v);
    }

    #[test]
    fn test_byte_swap_elements_noop_cases() {
        // elem_size <= 1: no-op.
        let mut single = vec![1u8, 2, 3];
        byte_swap_elements(&mut single, 1);
        assert_eq!(single, vec![1, 2, 3]);

        // Length not a multiple of elem_size: left untouched (never panics).
        let mut mismatched = vec![1u8, 2, 3, 4, 5];
        byte_swap_elements(&mut mismatched, 4);
        assert_eq!(mismatched, vec![1, 2, 3, 4, 5]);

        // Empty buffer: no-op, no panic.
        let mut empty: Vec<u8> = Vec::new();
        byte_swap_elements(&mut empty, 4);
        assert!(empty.is_empty());
    }

    #[test]
    fn test_invalid_signature() {
        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        temp_file.write_all(b"INVALID\n").expect("Failed to write");
        temp_file.flush().expect("Failed to flush");

        let result = Hdf5Reader::open(temp_file.path());
        assert!(result.is_err());
        assert!(matches!(result, Err(Hdf5Error::InvalidSignature(_))));
    }

    #[test]
    fn test_builder() {
        let builder = Hdf5ReaderBuilder::new().cache_size(1024);
        assert!(builder.cache_size.is_some());
    }

    /// Round-trip through the REAL writer and reader: write an i32 dataset with
    /// data, then read the genuine `.h5` back and assert the decoded values.
    #[test]
    fn test_real_roundtrip_i32_values() {
        let path = TempPath::new("roundtrip_i32.h5");

        {
            let mut writer = Hdf5Writer::create(&path, Hdf5Version::V10).expect("create writer");
            writer
                .create_dataset(
                    "/counts",
                    Datatype::Int32,
                    vec![5],
                    DatasetProperties::new(),
                )
                .expect("create dataset");
            writer
                .write_i32("/counts", &[10, 20, 30, 40, 50])
                .expect("write i32");
            writer.finalize().expect("finalize");
        }

        let mut reader = Hdf5Reader::open(&path).expect("open real file");
        assert!(reader.is_dataset("/counts"));
        {
            let ds = reader.dataset("/counts").expect("dataset");
            assert_eq!(ds.dims(), &[5]);
            assert_eq!(ds.datatype(), &Datatype::Int32);
        }
        let values = reader.read_i32("/counts").expect("read i32");
        assert_eq!(values, vec![10, 20, 30, 40, 50]);
    }

    /// Read a genuine `.h5` produced directly by `oxih5::FileWriter`, asserting
    /// real dataset values and a real dataset attribute survive the round-trip.
    #[test]
    fn test_read_real_oxih5_fixture() {
        let path = TempPath::new("oxih5_fixture.h5");

        {
            let mut w = oxih5::FileWriter::new();
            w.write_dataset_f64("temperature", &[20.5, 21.0, 19.75], &[3])
                .expect("write f64 dataset");
            w.write_string_attr("temperature", "units", "celsius")
                .expect("write string attr");
            w.write_root_str_attr("title", "Real HDF5 Fixture");
            w.build(&path).expect("build real hdf5");
        }

        let mut reader = Hdf5Reader::open(&path).expect("open real file");

        // Real dataset values.
        let values = reader.read_f64("/temperature").expect("read f64");
        assert_eq!(values, vec![20.5, 21.0, 19.75]);

        // Real dataset attribute.
        let ds = reader.dataset("/temperature").expect("dataset");
        let units = ds.attributes().get("units").expect("units attr");
        assert_eq!(units.as_string().ok(), Some("celsius".to_string()));

        // Real root attribute.
        let root = reader.root().expect("root");
        let title = root.attributes().get("title").expect("title attr");
        assert_eq!(
            title.as_string().ok(),
            Some("Real HDF5 Fixture".to_string())
        );
    }

    /// A real HDF5 dataset whose Datatype message declares **big-endian**
    /// byte order must still decode to the correct values through
    /// `read_f64` — not the silently-corrupted little-endian misread of the
    /// same bytes.
    ///
    /// `oxih5::FileWriter` only ever writes little-endian, so this test
    /// builds a genuine little-endian file with it and then, byte-for-byte,
    /// (a) flips the Datatype message's byte-order bit and (b) reverses the
    /// on-disk data bytes for each element — producing exactly the file a
    /// real big-endian-authored HDF5 file would contain for the same
    /// logical values. Both patches target fixed, known byte patterns
    /// (`write_datatype_body`'s hardcoded F64 datatype-message bytes, and the
    /// values' own little-endian encoding), so this genuinely exercises the
    /// real object-header datatype parsing + byte-swap path end to end.
    #[test]
    fn test_read_f64_normalizes_big_endian_source_bytes() {
        let path = TempPath::new("big_endian.h5");

        // The exact 24-byte F64 (little-endian) datatype-message body oxih5's
        // writer always emits (see `oxih5::write::messages::write_datatype_body`).
        // Byte [1] = 0x20 encodes byte-order bit 0 = 0 (little-endian); flipping
        // it to 0x21 marks the datatype big-endian without touching anything
        // else about its meaning (size, precision, exponent/mantissa layout).
        const F64_DTYPE_LE: [u8; 24] = [
            0x11, 0x20, 0x3f, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00, 0x34, 0x0b,
            0x00, 0x34, 0xff, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];

        let values = [1.5_f64, -2.5_f64];

        let mut bytes = {
            let mut w = oxih5::FileWriter::new();
            w.write_dataset_f64("x", &values, &[2]).expect("write f64");
            w.build_to_vec().expect("build to vec")
        };

        // Sanity-check the file really is little-endian before patching, and
        // decodes to the expected values through oxih5's own reader.
        {
            let f = oxih5::File::open_from_bytes(&bytes).expect("open LE bytes");
            let ds = f.dataset("x").expect("dataset x");
            assert_eq!(ds.as_f64().expect("as_f64"), values.to_vec());
        }

        // Patch 1: flip the F64 datatype message's byte-order bit.
        let dtype_pos = bytes
            .windows(F64_DTYPE_LE.len())
            .position(|w| w == F64_DTYPE_LE)
            .expect("F64 datatype message bytes must be present");
        bytes[dtype_pos + 1] = 0x21; // byte-order bit -> big-endian

        // Patch 2: reverse each element's on-disk bytes (LE encoding -> BE
        // encoding of the same logical value).
        let mut le_data = Vec::with_capacity(values.len() * 8);
        for v in &values {
            le_data.extend_from_slice(&v.to_le_bytes());
        }
        let data_pos = bytes
            .windows(le_data.len())
            .position(|w| w == le_data.as_slice())
            .expect("little-endian data bytes must be present");
        for chunk in bytes[data_pos..data_pos + le_data.len()].chunks_exact_mut(8) {
            chunk.reverse();
        }

        // Confirm the patched file really is big-endian from oxih5's own
        // point of view (as_f64 would misread it without a BE-aware caller).
        {
            let f = oxih5::File::open_from_bytes(&bytes).expect("open patched bytes");
            let ds = f.dataset("x").expect("dataset x");
            assert_eq!(
                ds.dtype,
                oxih5::Dtype::Float {
                    size: 8,
                    order: oxih5::ByteOrder::Big
                }
            );
            assert_eq!(ds.as_f64().expect("as_f64 on BE dataset"), values.to_vec());
        }

        std::fs::write(&path, &bytes).expect("write patched file");

        // The real assertion: oxigeo-hdf5's own reader must normalize the
        // big-endian bytes and return the correct logical values.
        let mut reader = Hdf5Reader::open(&path).expect("open big-endian file");
        let read_values = reader.read_f64("/x").expect("read_f64");
        assert_eq!(read_values, values.to_vec());
    }

    /// A dataset written with a real chunked layout must have its chunk
    /// dimensions recovered on read (via the object-header walk), making
    /// `decode_chunk` usable rather than always erroring with `Layout`.
    #[test]
    fn test_chunked_dataset_populates_chunk_dims_on_read() {
        let path = TempPath::new("chunked_roundtrip.h5");

        {
            let mut writer = Hdf5Writer::create(&path, Hdf5Version::V10).expect("create writer");
            let props = DatasetProperties::new().with_chunks(vec![4, 2]);
            writer
                .create_dataset("/chunked", Datatype::Float64, vec![4, 2], props)
                .expect("create chunked dataset");
            writer
                .write_f64("/chunked", &(0..8).map(|i| i as f64).collect::<Vec<_>>())
                .expect("write chunked data");
            writer.finalize().expect("finalize");
        }

        let mut reader = Hdf5Reader::open(&path).expect("open real file");

        // Chunk metadata recovered from the real object header.
        {
            let ds = reader.dataset("/chunked").expect("dataset");
            assert_eq!(
                ds.properties().layout(),
                crate::dataset::LayoutType::Chunked
            );
            assert_eq!(ds.properties().chunk_dims(), Some(&[4, 2][..]));
        }

        // decode_chunk is now live: no filter pipeline was written, so the
        // raw bytes pass through unchanged.
        let raw_chunk: Vec<u8> = (0..8i64)
            .map(|i| i as f64)
            .flat_map(|v: f64| v.to_le_bytes())
            .collect();
        let decoded = reader
            .decode_chunk("/chunked", &raw_chunk)
            .expect("decode_chunk on a real chunked dataset");
        assert_eq!(decoded, raw_chunk);

        // Real values still read correctly through the normal path too.
        let values = reader.read_f64("/chunked").expect("read f64");
        assert_eq!(values, (0..8).map(|i| i as f64).collect::<Vec<_>>());
    }
}
