//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

/// Upper bound on simultaneously in-flight tile range-fetches for a single
/// windowed read. wasm32 has no OS threads, but browsers happily pipeline
/// several concurrent `fetch()` calls to the same origin; capping keeps a
/// large (400+ tile) window from saturating the browser's per-origin
/// connection pool while still avoiding the fully-serial one-at-a-time cost.
pub(super) const MAX_CONCURRENT_TILE_FETCHES: usize = 8;

pub(super) const TAG_NEW_SUBFILE_TYPE: u16 = 254;

pub(super) const TAG_IMAGE_WIDTH: u16 = 256;

pub(super) const TAG_IMAGE_LENGTH: u16 = 257;

pub(super) const TAG_TILE_WIDTH: u16 = 322;

pub(super) const TAG_TILE_LENGTH: u16 = 323;

pub(super) const TAG_GEO_KEY_DIRECTORY: u16 = 34735;

pub(super) const TAG_GEO_DOUBLE_PARAMS: u16 = 34736;

/// `NewSubfileType` (tag 254) bit 2: this IFD is a transparency mask for
/// another IFD in the file. GDAL sets it on every internal-mask IFD it writes.
pub(super) const SUBFILE_TYPE_TRANSPARENCY_MASK: u64 = 0x4;

/// `PhotometricInterpretation` (tag 262) value 4: transparency mask. GDAL's
/// internal masks carry this too, and some writers set *only* this.
pub(super) const PHOTOMETRIC_TRANSPARENCY_MASK: u16 = 4;

pub(super) const GEOKEY_PROJECTED_CS_TYPE: u16 = 3072;

pub(super) const GEOKEY_GEOGRAPHIC_TYPE: u16 = 2048;
