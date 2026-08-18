//! Auto-generated module
//!
//! 🤖 Generated with [SplitRS](https://github.com/cool-japan/splitrs)

use crate::fetch::FetchBackend;
use futures::stream::{self, StreamExt};
use oxigeo_core::error::{FormatError, OxiGeoError, Result};
use oxigeo_core::io::ByteRange;

use super::constants::{
    GEOKEY_GEOGRAPHIC_TYPE, GEOKEY_PROJECTED_CS_TYPE, MAX_CONCURRENT_TILE_FETCHES,
    TAG_GEO_DOUBLE_PARAMS, TAG_GEO_KEY_DIRECTORY, TAG_IMAGE_LENGTH, TAG_IMAGE_WIDTH,
    TAG_NEW_SUBFILE_TYPE, TAG_TILE_LENGTH, TAG_TILE_WIDTH,
};
use super::functions::{
    RangeSource, assemble_window, assemble_window_rgb8, bytes_to_u16, decompress_tile,
    expected_tile_byte_size, finish_tile_decode, is_mask_ifd, normalize_pixel_scale_y,
    tile_byte_range,
};
use super::types::{IfdMetadata, OverviewMetadata};
use super::types_3::{ByteOrder, CogMetadata, ParsedIfd};

/// WASM-compatible async COG reader
pub struct WasmCogReader {
    pub(super) backend: FetchBackend,
    pub(super) metadata: CogMetadata,
    #[allow(dead_code)]
    pub(super) byte_order: ByteOrder,
}
impl WasmCogReader {
    /// Open a COG file from a URL with async I/O
    pub async fn open(url: String) -> Result<Self> {
        let backend = FetchBackend::new(url.clone()).await?;
        let header_bytes = backend
            .read_range_async(ByteRange::from_offset_length(0, 16))
            .await?;
        if header_bytes.len() < 16 {
            return Err(OxiGeoError::Format(FormatError::InvalidHeader {
                message: format!(
                    "truncated TIFF header: expected 16 bytes, got {}",
                    header_bytes.len()
                ),
            }));
        }
        let byte_order = if &header_bytes[0..2] == b"II" {
            ByteOrder::LittleEndian
        } else if &header_bytes[0..2] == b"MM" {
            ByteOrder::BigEndian
        } else {
            return Err(OxiGeoError::Format(FormatError::InvalidHeader {
                message: "Invalid TIFF magic bytes".to_string(),
            }));
        };
        let ifd_offset = match byte_order {
            ByteOrder::LittleEndian => u32::from_le_bytes([
                header_bytes[4],
                header_bytes[5],
                header_bytes[6],
                header_bytes[7],
            ]) as u64,
            ByteOrder::BigEndian => u32::from_be_bytes([
                header_bytes[4],
                header_bytes[5],
                header_bytes[6],
                header_bytes[7],
            ]) as u64,
        };
        let metadata = Self::parse_ifd_chain(&backend, byte_order, ifd_offset).await?;
        Ok(Self {
            backend,
            metadata,
            byte_order,
        })
    }
    /// Walk the whole IFD chain from `first_ifd_offset` and build the complete
    /// [`CogMetadata`] (full-resolution tags plus `levels` / `overviews`).
    ///
    /// Split out of [`open`](Self::open) — and generic over [`RangeSource`] —
    /// so the chain walk, including the internal-mask skip below, is unit
    /// testable natively over an in-memory TIFF buffer.
    ///
    /// GDAL internal masks share this chain with the overviews but are *not*
    /// pyramid levels: they are single-bit alpha planes for the level that
    /// precedes them. They are walked through (their `next` link is followed,
    /// and they count against the runaway-chain cap) but never pushed as a
    /// level, so `overview_count` and every `levels[i]` index describe
    /// resolutions only.
    pub(super) async fn parse_ifd_chain<S: RangeSource>(
        source: &S,
        byte_order: ByteOrder,
        first_ifd_offset: u64,
    ) -> Result<CogMetadata> {
        let ifd_data = source
            .read_range(ByteRange::from_offset_length(first_ifd_offset, 4096))
            .await?;
        let primary = Self::parse_ifd(&ifd_data, byte_order, source, first_ifd_offset).await?;
        let metadata = primary.metadata;
        let mut levels: Vec<IfdMetadata> = vec![IfdMetadata::from_cog(&metadata)];
        let mut overviews = Vec::new();
        let mut ifd_offset = primary.next_ifd_offset;
        let mut ifd_count = 0;
        while ifd_offset != 0 && ifd_count < 100 {
            let ov_ifd_data = source
                .read_range(ByteRange::from_offset_length(ifd_offset, 4096))
                .await?;
            let parsed = Self::parse_ifd(&ov_ifd_data, byte_order, source, ifd_offset).await?;
            let ov_meta = parsed.metadata;
            if ov_meta.width > 0 && ov_meta.height > 0 && !parsed.is_mask {
                overviews.push(OverviewMetadata {
                    width: ov_meta.width,
                    height: ov_meta.height,
                    tile_width: ov_meta.tile_width,
                    tile_height: ov_meta.tile_height,
                });
                levels.push(IfdMetadata::from_cog(&ov_meta));
            }
            ifd_offset = parsed.next_ifd_offset;
            ifd_count += 1;
        }
        let mut final_metadata = metadata;
        final_metadata.overview_count = overviews.len();
        final_metadata.overviews = overviews;
        final_metadata.levels = levels;
        Ok(final_metadata)
    }
    /// Parse IFD to extract essential tags and return metadata with next IFD offset
    pub(super) async fn parse_ifd<S: RangeSource>(
        data: &[u8],
        byte_order: ByteOrder,
        backend: &S,
        _ifd_offset: u64,
    ) -> Result<ParsedIfd> {
        if data.len() < 2 {
            return Err(OxiGeoError::Format(FormatError::InvalidHeader {
                message: format!(
                    "truncated IFD: expected at least 2 bytes, got {}",
                    data.len()
                ),
            }));
        }
        let num_entries = match byte_order {
            ByteOrder::LittleEndian => u16::from_le_bytes([data[0], data[1]]),
            ByteOrder::BigEndian => u16::from_be_bytes([data[0], data[1]]),
        };
        let mut width = 0u64;
        let mut height = 0u64;
        let mut tile_width = 256u32;
        let mut tile_height = 256u32;
        let mut rows_per_strip = 0u32;
        let mut bits_per_sample = 8u16;
        let mut samples_per_pixel = 1u16;
        let mut sample_format = 1u16;
        let mut compression = 1u16;
        let mut predictor = 1u16;
        let mut photometric = 1u16;
        let mut subfile_type = 0u64;
        let mut tile_offsets = Vec::new();
        let mut tile_byte_counts = Vec::new();
        let mut pixel_scale_x: Option<f64> = None;
        let mut pixel_scale_y: Option<f64> = None;
        let mut tiepoint_pixel_x: Option<f64> = None;
        let mut tiepoint_pixel_y: Option<f64> = None;
        let mut tiepoint_geo_x: Option<f64> = None;
        let mut tiepoint_geo_y: Option<f64> = None;
        let mut geo_key_directory: Option<Vec<u16>> = None;
        let mut geo_double_params: Vec<f64> = Vec::new();
        for i in 0..num_entries {
            let offset = 2 + (i as usize * 12);
            if offset + 12 > data.len() {
                break;
            }
            let entry = &data[offset..offset + 12];
            let tag = match byte_order {
                ByteOrder::LittleEndian => u16::from_le_bytes([entry[0], entry[1]]),
                ByteOrder::BigEndian => u16::from_be_bytes([entry[0], entry[1]]),
            };
            let field_type = match byte_order {
                ByteOrder::LittleEndian => u16::from_le_bytes([entry[2], entry[3]]),
                ByteOrder::BigEndian => u16::from_be_bytes([entry[2], entry[3]]),
            };
            let count = match byte_order {
                ByteOrder::LittleEndian => {
                    u32::from_le_bytes([entry[4], entry[5], entry[6], entry[7]])
                }
                ByteOrder::BigEndian => {
                    u32::from_be_bytes([entry[4], entry[5], entry[6], entry[7]])
                }
            };
            let value_bytes = &entry[8..12];
            match tag {
                TAG_NEW_SUBFILE_TYPE => {
                    subfile_type = Self::read_value(value_bytes, field_type, byte_order);
                }
                TAG_IMAGE_WIDTH => {
                    width = Self::read_value(value_bytes, field_type, byte_order);
                }
                TAG_IMAGE_LENGTH => {
                    height = Self::read_value(value_bytes, field_type, byte_order);
                }
                258 => {
                    bits_per_sample = if count <= 1 {
                        Self::read_value(value_bytes, field_type, byte_order) as u16
                    } else {
                        Self::read_array(value_bytes, field_type, count, byte_order, backend)
                            .await?
                            .first()
                            .copied()
                            .unwrap_or(8) as u16
                    };
                }
                259 => {
                    compression = Self::read_value(value_bytes, field_type, byte_order) as u16;
                }
                262 => {
                    photometric = Self::read_value(value_bytes, field_type, byte_order) as u16;
                }
                317 => {
                    predictor = Self::read_value(value_bytes, field_type, byte_order) as u16;
                }
                277 => {
                    samples_per_pixel =
                        Self::read_value(value_bytes, field_type, byte_order) as u16;
                }
                339 => {
                    sample_format = if count <= 1 {
                        Self::read_value(value_bytes, field_type, byte_order) as u16
                    } else {
                        Self::read_array(value_bytes, field_type, count, byte_order, backend)
                            .await?
                            .first()
                            .copied()
                            .unwrap_or(1) as u16
                    };
                }
                278 => {
                    rows_per_strip = Self::read_value(value_bytes, field_type, byte_order) as u32;
                }
                TAG_TILE_WIDTH => {
                    tile_width = Self::read_value(value_bytes, field_type, byte_order) as u32;
                }
                TAG_TILE_LENGTH => {
                    tile_height = Self::read_value(value_bytes, field_type, byte_order) as u32;
                }
                273 => {
                    tile_offsets =
                        Self::read_array(value_bytes, field_type, count, byte_order, backend)
                            .await?;
                }
                279 => {
                    tile_byte_counts =
                        Self::read_array(value_bytes, field_type, count, byte_order, backend)
                            .await?;
                }
                324 => {
                    tile_offsets =
                        Self::read_array(value_bytes, field_type, count, byte_order, backend)
                            .await?;
                }
                325 => {
                    tile_byte_counts =
                        Self::read_array(value_bytes, field_type, count, byte_order, backend)
                            .await?;
                }
                33550 if count >= 2 => {
                    let doubles = Self::read_double_array(
                        value_bytes,
                        field_type,
                        count,
                        byte_order,
                        backend,
                    )
                    .await?;
                    if !doubles.is_empty() {
                        pixel_scale_x = Some(doubles[0]);
                    }
                    if doubles.len() > 1 {
                        pixel_scale_y = Some(normalize_pixel_scale_y(doubles[1]));
                    }
                }
                33550 => {}
                33922 if count >= 6 => {
                    let doubles = Self::read_double_array(
                        value_bytes,
                        field_type,
                        count,
                        byte_order,
                        backend,
                    )
                    .await?;
                    if doubles.len() >= 6 {
                        tiepoint_pixel_x = Some(doubles[0]);
                        tiepoint_pixel_y = Some(doubles[1]);
                        tiepoint_geo_x = Some(doubles[3]);
                        tiepoint_geo_y = Some(doubles[4]);
                    }
                }
                TAG_GEO_KEY_DIRECTORY => {
                    let values =
                        Self::read_array(value_bytes, field_type, count, byte_order, backend)
                            .await?;
                    geo_key_directory = Some(values.iter().map(|&v| v as u16).collect());
                }
                TAG_GEO_DOUBLE_PARAMS => {
                    geo_double_params = Self::read_double_array(
                        value_bytes,
                        field_type,
                        count,
                        byte_order,
                        backend,
                    )
                    .await?;
                }
                _ => {}
            }
        }
        if rows_per_strip > 0 && !tile_offsets.is_empty() {
            tile_width = width as u32;
            tile_height = rows_per_strip;
        }
        let next_ifd_offset_pos = 2 + (num_entries as usize * 12);
        let next_ifd_offset = if next_ifd_offset_pos + 4 <= data.len() {
            match byte_order {
                ByteOrder::LittleEndian => u32::from_le_bytes([
                    data[next_ifd_offset_pos],
                    data[next_ifd_offset_pos + 1],
                    data[next_ifd_offset_pos + 2],
                    data[next_ifd_offset_pos + 3],
                ]) as u64,
                ByteOrder::BigEndian => u32::from_be_bytes([
                    data[next_ifd_offset_pos],
                    data[next_ifd_offset_pos + 1],
                    data[next_ifd_offset_pos + 2],
                    data[next_ifd_offset_pos + 3],
                ]) as u64,
            }
        } else {
            0
        };
        let epsg_code = Self::parse_epsg_from_geokeys(&geo_key_directory, &geo_double_params);
        let metadata = CogMetadata {
            width,
            height,
            tile_width,
            tile_height,
            bits_per_sample,
            samples_per_pixel,
            sample_format,
            compression,
            photometric_interpretation: photometric,
            predictor,
            tile_offsets,
            tile_byte_counts,
            pixel_scale_x,
            pixel_scale_y,
            tiepoint_pixel_x,
            tiepoint_pixel_y,
            tiepoint_geo_x,
            tiepoint_geo_y,
            overview_count: 0,
            overviews: Vec::new(),
            epsg_code,
            levels: Vec::new(),
        };
        Ok(ParsedIfd {
            metadata,
            next_ifd_offset,
            is_mask: is_mask_ifd(subfile_type, photometric),
        })
    }
    /// Parse EPSG code from GeoKeyDirectory
    pub(super) fn parse_epsg_from_geokeys(
        geo_key_directory: &Option<Vec<u16>>,
        _geo_double_params: &[f64],
    ) -> Option<u32> {
        let directory = geo_key_directory.as_ref()?;
        if directory.len() < 4 {
            return None;
        }
        let key_count = directory[3] as usize;
        if directory.len() < 4 + key_count * 4 {
            return None;
        }
        for i in 0..key_count {
            let base = 4 + i * 4;
            let key_id = directory[base];
            let tiff_tag_location = directory[base + 1];
            let value_offset = directory[base + 3];
            if tiff_tag_location == 0 {
                if key_id == GEOKEY_PROJECTED_CS_TYPE && value_offset != 32767 {
                    return Some(u32::from(value_offset));
                }
                if key_id == GEOKEY_GEOGRAPHIC_TYPE && value_offset != 32767 {
                    return Some(u32::from(value_offset));
                }
            }
        }
        None
    }
    /// Read a single value from IFD entry
    pub(super) fn read_value(bytes: &[u8], field_type: u16, byte_order: ByteOrder) -> u64 {
        match field_type {
            3 => match byte_order {
                ByteOrder::LittleEndian => u16::from_le_bytes([bytes[0], bytes[1]]) as u64,
                ByteOrder::BigEndian => u16::from_be_bytes([bytes[0], bytes[1]]) as u64,
            },
            4 => match byte_order {
                ByteOrder::LittleEndian => {
                    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as u64
                }
                ByteOrder::BigEndian => {
                    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as u64
                }
            },
            _ => 0,
        }
    }
    /// Read an array of values (for tile offsets, byte counts, etc.)
    pub(super) async fn read_array<S: RangeSource>(
        bytes: &[u8],
        field_type: u16,
        count: u32,
        byte_order: ByteOrder,
        backend: &S,
    ) -> Result<Vec<u64>> {
        let value_size = match field_type {
            3 => 2,
            4 => 4,
            _ => return Ok(Vec::new()),
        };
        let total_size = count as usize * value_size;
        let data = if total_size <= 4 {
            bytes.to_vec()
        } else {
            let offset = match byte_order {
                ByteOrder::LittleEndian => {
                    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as u64
                }
                ByteOrder::BigEndian => {
                    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as u64
                }
            };
            backend
                .read_range(ByteRange::from_offset_length(offset, total_size as u64))
                .await?
        };
        let mut values = Vec::with_capacity(count as usize);
        for i in 0..count as usize {
            let offset = i * value_size;
            if offset + value_size > data.len() {
                break;
            }
            let value = match field_type {
                3 => match byte_order {
                    ByteOrder::LittleEndian => {
                        u16::from_le_bytes([data[offset], data[offset + 1]]) as u64
                    }
                    ByteOrder::BigEndian => {
                        u16::from_be_bytes([data[offset], data[offset + 1]]) as u64
                    }
                },
                4 => match byte_order {
                    ByteOrder::LittleEndian => u32::from_le_bytes([
                        data[offset],
                        data[offset + 1],
                        data[offset + 2],
                        data[offset + 3],
                    ]) as u64,
                    ByteOrder::BigEndian => u32::from_be_bytes([
                        data[offset],
                        data[offset + 1],
                        data[offset + 2],
                        data[offset + 3],
                    ]) as u64,
                },
                _ => 0,
            };
            values.push(value);
        }
        Ok(values)
    }
    /// Read an array of DOUBLE values (for GeoTIFF tags)
    pub(super) async fn read_double_array<S: RangeSource>(
        bytes: &[u8],
        field_type: u16,
        count: u32,
        byte_order: ByteOrder,
        backend: &S,
    ) -> Result<Vec<f64>> {
        if field_type != 12 {
            return Ok(Vec::new());
        }
        let value_size = 8;
        let total_size = count as usize * value_size;
        let data = if total_size <= 4 {
            bytes.to_vec()
        } else {
            let offset = match byte_order {
                ByteOrder::LittleEndian => {
                    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as u64
                }
                ByteOrder::BigEndian => {
                    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as u64
                }
            };
            backend
                .read_range(ByteRange::from_offset_length(offset, total_size as u64))
                .await?
        };
        let mut values = Vec::with_capacity(count as usize);
        for i in 0..count as usize {
            let offset = i * value_size;
            if offset + value_size > data.len() {
                break;
            }
            let bytes_array = [
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
                data[offset + 4],
                data[offset + 5],
                data[offset + 6],
                data[offset + 7],
            ];
            let value = match byte_order {
                ByteOrder::LittleEndian => f64::from_le_bytes(bytes_array),
                ByteOrder::BigEndian => f64::from_be_bytes(bytes_array),
            };
            values.push(value);
        }
        Ok(values)
    }
    /// Read a specific tile from the full-resolution image (level 0).
    ///
    /// Signature and behaviour are preserved for existing callers: the returned
    /// bytes are the fully decoded tile in raster order, in the **host's** byte
    /// order. This now routes through
    /// [`read_tile_level`](Self::read_tile_level) so the horizontal Predictor
    /// (TIFF tag 317) is undone when present. `predictor == 1` (or absent) is a
    /// no-op, so existing non-predicted COGs are unaffected.
    ///
    /// Nothing in the crate calls this any more: `WasmCogViewer::read_tile`
    /// used to, and that was the bug — it takes a `level` and this shortcut
    /// discards it. It is kept as the explicit level-0 convenience for
    /// single-resolution callers; anything level-aware must use
    /// [`read_tile_level`](Self::read_tile_level).
    #[allow(dead_code)]
    pub async fn read_tile(&self, tile_x: u32, tile_y: u32) -> Result<Vec<u8>> {
        self.read_tile_level(0, tile_x, tile_y).await
    }
    /// Read a single tile at a specific pyramid level (0 = full resolution).
    ///
    /// The tile is fetched from that level's own tile directory, decompressed,
    /// and — when the level declares `predictor == 2` — the horizontal
    /// differencing predictor is undone in place using the level's sample
    /// layout (bits/sample, samples/pixel) and the file's byte order. The
    /// returned bytes are the decoded tile in raster order (`tile_width ×
    /// tile_height × samples_per_pixel` samples).
    ///
    /// # Byte order
    ///
    /// The returned samples are in the **host's** byte order, whatever the
    /// file's `II`/`MM` header says. This is the same contract
    /// `oxigeo_geotiff`'s reader has (see its *Byte order of decoded samples*
    /// crate docs), which matters because [`crate::WasmCogViewer`] serves tiles
    /// from *either* this reader (URL path) or `oxigeo_geotiff::CogReader`
    /// (`openBytes` path) and must not have to know which. Before
    /// cool-japan/oxigeo#14 this reader returned file-order bytes and the viewer
    /// carried a `little_endian` flag to compensate — one flag that had to mean
    /// two different things depending on which reader had produced the tile.
    ///
    /// The two swaps in this function are ordered and both necessary: the
    /// predictor is defined on *file*-order samples (TIFF 6.0 §14), so it is
    /// undone first, and only then are the samples normalised to host order.
    pub async fn read_tile_level(&self, level: usize, tile_x: u32, tile_y: u32) -> Result<Vec<u8>> {
        let lvl = self
            .metadata
            .levels
            .get(level)
            .ok_or_else(|| OxiGeoError::OutOfBounds {
                message: format!("Overview level {} out of range", level),
            })?;
        let range = tile_byte_range(lvl, level, tile_x, tile_y)?;
        let compressed_data = self.backend.read_range_async(range).await?;
        let mut decompressed = decompress_tile(
            compressed_data,
            lvl.compression,
            expected_tile_byte_size(lvl),
        )?;
        finish_tile_decode(&mut decompressed, lvl, self.is_little_endian());
        Ok(decompressed)
    }
    /// Read a rectangular window of 16-bit samples at the given pyramid level.
    ///
    /// The window `[x0, x0+w) × [y0, y0+h)` is expressed in that level's pixel
    /// coordinates. Every tile intersecting the window is fetched, decoded
    /// (predictor-corrected) and cropped into a dense `w × h` row-major buffer.
    /// Tiles that fall entirely outside the level's tile grid (off-grid crops
    /// that overhang the image) contribute zeros rather than erroring. Intended
    /// for single-band 16-bit Sentinel-2 reflectance COGs.
    #[allow(dead_code)]
    pub async fn read_window_u16(
        &self,
        level: usize,
        x0: u64,
        y0: u64,
        w: u32,
        h: u32,
    ) -> Result<Vec<u16>> {
        let lvl = self
            .metadata
            .levels
            .get(level)
            .ok_or_else(|| OxiGeoError::OutOfBounds {
                message: format!("Overview level {} out of range", level),
            })?;
        if w == 0 || h == 0 {
            return Ok(Vec::new());
        }
        let tile_width = lvl.tile_width;
        let tile_height = lvl.tile_height;
        let tw = tile_width as u64;
        let th = tile_height as u64;
        let tiles_across = lvl.width.div_ceil(tw);
        let tiles_down = lvl.height.div_ceil(th);
        let tx0 = x0 / tw;
        let ty0 = y0 / th;
        let tx1 = (x0 + w as u64 - 1) / tw;
        let ty1 = (y0 + h as u64 - 1) / th;
        let mut coords: Vec<(u32, u32)> = Vec::new();
        for ty in ty0..=ty1 {
            if ty >= tiles_down {
                continue;
            }
            for tx in tx0..=tx1 {
                if tx >= tiles_across {
                    continue;
                }
                coords.push((tx as u32, ty as u32));
            }
        }
        let fetches = stream::iter(coords.into_iter().map(|(tx, ty)| async move {
            let bytes = self.read_tile_level(level, tx, ty).await?;
            Ok::<_, OxiGeoError>((tx, ty, bytes_to_u16(&bytes)))
        }))
        .buffer_unordered(MAX_CONCURRENT_TILE_FETCHES)
        .collect::<Vec<_>>()
        .await;
        let mut tiles: Vec<(u32, u32, Vec<u16>)> = Vec::with_capacity(fetches.len());
        for result in fetches {
            tiles.push(result?);
        }
        Ok(assemble_window(
            &tiles,
            tile_width,
            tile_height,
            x0,
            y0,
            w,
            h,
        ))
    }
    /// Read a rectangular window of 8-bit RGB (3 samples/pixel) at a level.
    ///
    /// Used for the Sentinel-2 True-Colour Image (TCI) asset. The output is a
    /// dense `w × h × 3` interleaved RGB byte buffer in row-major order.
    /// Off-grid tiles contribute zeros. The reader's own `samples_per_pixel`
    /// (nominally 3) is used to undo the predictor per channel.
    #[allow(dead_code)]
    pub async fn read_window_rgb8(
        &self,
        level: usize,
        x0: u64,
        y0: u64,
        w: u32,
        h: u32,
    ) -> Result<Vec<u8>> {
        let lvl = self
            .metadata
            .levels
            .get(level)
            .ok_or_else(|| OxiGeoError::OutOfBounds {
                message: format!("Overview level {} out of range", level),
            })?;
        if w == 0 || h == 0 {
            return Ok(Vec::new());
        }
        let tile_width = lvl.tile_width;
        let tile_height = lvl.tile_height;
        let tw = tile_width as u64;
        let th = tile_height as u64;
        let tiles_across = lvl.width.div_ceil(tw);
        let tiles_down = lvl.height.div_ceil(th);
        let tx0 = x0 / tw;
        let ty0 = y0 / th;
        let tx1 = (x0 + w as u64 - 1) / tw;
        let ty1 = (y0 + h as u64 - 1) / th;
        let mut coords: Vec<(u32, u32)> = Vec::new();
        for ty in ty0..=ty1 {
            if ty >= tiles_down {
                continue;
            }
            for tx in tx0..=tx1 {
                if tx >= tiles_across {
                    continue;
                }
                coords.push((tx as u32, ty as u32));
            }
        }
        let fetches = stream::iter(coords.into_iter().map(|(tx, ty)| async move {
            let bytes = self.read_tile_level(level, tx, ty).await?;
            Ok::<_, OxiGeoError>((tx, ty, bytes))
        }))
        .buffer_unordered(MAX_CONCURRENT_TILE_FETCHES)
        .collect::<Vec<_>>()
        .await;
        let mut tiles: Vec<(u32, u32, Vec<u8>)> = Vec::with_capacity(fetches.len());
        for result in fetches {
            tiles.push(result?);
        }
        Ok(assemble_window_rgb8(
            &tiles,
            tile_width,
            tile_height,
            x0,
            y0,
            w,
            h,
        ))
    }
    /// Get metadata
    pub fn metadata(&self) -> &CogMetadata {
        &self.metadata
    }
    /// Returns `true` if the underlying TIFF's *header* declares little-endian.
    ///
    /// Deliberately **not** `pub`: this describes the file on disk, never the
    /// samples this reader hands out, which are always host-native (see
    /// [`Self::read_tile_level`]). While it was public, `WasmCogViewer` used it
    /// to byte-swap tiles, which was correct only for as long as this reader
    /// returned file-order bytes. Keeping it module-private means no consumer
    /// can make that mistake again — the sample contract is now enforced by
    /// visibility rather than by a comment (cool-japan/oxigeo#14).
    pub(super) fn is_little_endian(&self) -> bool {
        matches!(self.byte_order, ByteOrder::LittleEndian)
    }
}
