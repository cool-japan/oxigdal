//! Pure Rust PMTiles v3 reader and writer.
//!
//! Parses the 127-byte fixed header ([`header`]), varint-encoded directory
//! entries ([`directory`]), and provides a high-level reader ([`pmtiles`])
//! and writer ([`writer`]).  Tile IDs are computed via the Hilbert curve
//! ([`hilbert`]).  Structured JSON metadata access is provided via
//! [`metadata`].  Archive compaction (gap removal) is available via
//! [`compact`].  Sub-region extraction is available via [`extract`].
//! Re-compression between gzip / brotli / zstd is available via the
//! `transcode` module (feature-gated behind `compression`).  Tile-set
//! diffing between two archives is available via [`diff`].

pub mod compact;
pub mod diff;
pub mod directory;
pub mod error;
#[cfg(feature = "http-range")]
pub mod etag_cache;
pub mod extract;
pub mod header;
pub mod header_v2;
pub mod hilbert;
#[cfg(feature = "http-range")]
pub mod http_reader;
pub mod layout;
pub mod metadata;
pub mod pmtiles;
pub mod tile_detect;
#[cfg(feature = "compression")]
pub mod transcode;
pub mod v2_reader;
pub mod validate;
pub mod varint;
pub mod webmerc;
pub mod writer;

#[cfg(feature = "async")]
pub mod async_reader;

#[cfg(feature = "cloud-storage")]
pub mod cloud_reader;

#[cfg(feature = "mbtiles")]
pub mod mbtiles_export;

#[cfg(feature = "http-range")]
pub use etag_cache::EtagCache;
#[cfg(feature = "http-range")]
pub use http_reader::HttpPmTilesReader;

pub use compact::{
    CompactOptions, CompactStats, compact_archive, compact_archive_default,
    compact_archive_with_stats,
};
pub use diff::{DiffReport, DiffSummary, TileChange, diff_archives, diff_archives_summary};
pub use directory::{DirectoryEntry, decode_directory, decode_varint};
pub use error::PmTilesError;
pub use extract::{ExtractOptions, bbox_to_tile_range, extract_subregion};
pub use header::{Compression, PmTilesHeader, TileType};
pub use hilbert::{hilbert_to_xy, tile_id_to_zxy, xy_to_hilbert, zxy_to_tile_id};
pub use layout::{LayoutAnalysis, LayoutStrategy, analyze_tile_ordering, choose_strategy};
pub use metadata::PmTilesMetadata;
pub use pmtiles::{PmTilesReader, TileInfo};
pub use tile_detect::{DetectedTileFormat, detect_tile_format};
#[cfg(feature = "compression")]
pub use transcode::{
    TranscodeOptions, TranscodeStats, transcode_archive, transcode_archive_with_stats,
    transcode_tile,
};
pub use validate::{ValidationIssue, ValidationReport, validate_archive, validate_archive_strict};
pub use varint::encode_varint;
pub use webmerc::{lonlat_to_tile, tile_bounds_lonlat, tile_to_lonlat};
pub use writer::PmTilesBuilder;

#[cfg(feature = "async")]
pub use async_reader::AsyncPmTilesReader;

#[cfg(feature = "cloud-storage")]
pub use cloud_reader::{CloudCredentials, CloudObjectUri, CloudPmTilesReader, CloudProvider};

#[cfg(feature = "mbtiles")]
pub use mbtiles_export::{MbTilesConn, MbTilesExportStats, MbTilesExporter};

pub use header_v2::{
    PMTILES_V2_MAGIC, PmTilesV2Entry, PmTilesV2Header, parse_v2_header, read_v2_entry,
    zxy_to_v2_tile_id,
};
pub use v2_reader::{PmTilesV2Reader, V2Tile, detect_pmtiles_version};

#[cfg(feature = "parallel")]
pub mod parallel_encode;
#[cfg(all(feature = "parallel", feature = "compression"))]
pub use parallel_encode::compress_tile;
#[cfg(feature = "parallel")]
pub use parallel_encode::{ParallelBuildStats, ParallelEncodeConfig, RawTile};
#[cfg(feature = "parallel")]
pub use parallel_encode::{build_pmtiles_parallel, compress_tiles_parallel};
