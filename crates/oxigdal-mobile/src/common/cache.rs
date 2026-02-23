//! Mobile-optimized caching for tile and dataset data.
//!
//! Provides LRU caching with memory limits and automatic eviction
//! optimized for mobile device constraints.

use crate::ffi::types::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Cached tile data.
#[derive(Clone)]
struct CachedTile {
    /// Tile image data
    data: Vec<u8>,
    /// Width in pixels
    width: i32,
    /// Height in pixels
    height: i32,
    /// Number of channels
    channels: i32,
    /// Last access timestamp
    last_access: std::time::Instant,
    /// Size in bytes
    size_bytes: usize,
}

/// Mobile tile cache with LRU eviction.
struct TileCache {
    /// Cache entries by key
    entries: HashMap<String, CachedTile>,
    /// Maximum cache size in bytes
    max_size_bytes: usize,
    /// Current cache size in bytes
    current_size_bytes: usize,
}

impl TileCache {
    fn new(max_size_mb: usize) -> Self {
        Self {
            entries: HashMap::new(),
            max_size_bytes: max_size_mb * 1024 * 1024,
            current_size_bytes: 0,
        }
    }

    fn get(&mut self, key: &str) -> Option<CachedTile> {
        if let Some(tile) = self.entries.get_mut(key) {
            tile.last_access = std::time::Instant::now();
            super::record_cache_hit();
            Some(tile.clone())
        } else {
            super::record_cache_miss();
            None
        }
    }

    fn put(&mut self, key: String, tile: CachedTile) {
        // Evict old entries if needed
        while self.current_size_bytes + tile.size_bytes > self.max_size_bytes
            && !self.entries.is_empty()
        {
            self.evict_lru();
        }

        // Add new entry
        self.current_size_bytes += tile.size_bytes;
        self.entries.insert(key, tile);
        super::set_tiles_cached(self.entries.len());
    }

    fn evict_lru(&mut self) {
        // Find least recently used entry
        let oldest_key = self
            .entries
            .iter()
            .min_by_key(|(_, tile)| tile.last_access)
            .map(|(key, _)| key.clone());

        if let Some(key) = oldest_key {
            if let Some(tile) = self.entries.remove(&key) {
                self.current_size_bytes -= tile.size_bytes;
            }
        }
        super::set_tiles_cached(self.entries.len());
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.current_size_bytes = 0;
        super::set_tiles_cached(0);
    }

    fn resize(&mut self, max_size_mb: usize) -> Result<(), String> {
        self.max_size_bytes = max_size_mb * 1024 * 1024;

        // Evict entries if new size is smaller
        while self.current_size_bytes > self.max_size_bytes && !self.entries.is_empty() {
            self.evict_lru();
        }

        Ok(())
    }
}

/// Global tile cache instance.
static CACHE: Mutex<Option<TileCache>> = Mutex::new(None);

/// Initializes the tile cache with given size limit.
///
/// # Parameters
/// - `max_size_mb`: Maximum cache size in megabytes
pub fn init_cache(max_size_mb: usize) -> Result<(), String> {
    let mut cache = CACHE.lock().map_err(|e| e.to_string())?;
    *cache = Some(TileCache::new(max_size_mb));
    Ok(())
}

/// Sets the maximum cache size.
pub fn set_max_cache_size_mb(max_size_mb: usize) -> Result<(), String> {
    let mut cache = CACHE.lock().map_err(|e| e.to_string())?;
    if let Some(ref mut c) = *cache {
        c.resize(max_size_mb)?;
    } else {
        *cache = Some(TileCache::new(max_size_mb));
    }
    Ok(())
}

/// Gets a tile from cache.
pub fn get_cached_tile(key: &str) -> Option<(Vec<u8>, i32, i32, i32)> {
    let mut cache = CACHE.lock().ok()?;
    if let Some(ref mut c) = *cache {
        c.get(key).map(|tile| {
            // Record the bytes read from cache for statistics
            super::record_bytes_read(tile.data.len());
            (tile.data, tile.width, tile.height, tile.channels)
        })
    } else {
        None
    }
}

/// Puts a tile into cache.
pub fn put_cached_tile(key: String, data: Vec<u8>, width: i32, height: i32, channels: i32) {
    if let Ok(mut cache) = CACHE.lock() {
        if cache.is_none() {
            *cache = Some(TileCache::new(100)); // Default 100MB
        }

        if let Some(ref mut c) = *cache {
            let size_bytes = data.len();
            let tile = CachedTile {
                data,
                width,
                height,
                channels,
                last_access: std::time::Instant::now(),
                size_bytes,
            };
            c.put(key, tile);
        }
    }
}

/// Clears all cached tiles.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_cache_clear() -> OxiGdalErrorCode {
    match CACHE.lock() {
        Ok(mut cache) => {
            if let Some(ref mut c) = *cache {
                c.clear();
            }
            OxiGdalErrorCode::Success
        }
        Err(e) => {
            crate::ffi::error::set_last_error(e.to_string());
            OxiGdalErrorCode::Unknown
        }
    }
}

/// Sets the cache size limit.
///
/// # Parameters
/// - `max_size_mb`: Maximum size in megabytes
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_cache_set_size(
    max_size_mb: std::os::raw::c_int,
) -> OxiGdalErrorCode {
    if max_size_mb <= 0 {
        crate::ffi::error::set_last_error("Invalid cache size".to_string());
        return OxiGdalErrorCode::InvalidArgument;
    }

    match set_max_cache_size_mb(max_size_mb as usize) {
        Ok(()) => OxiGdalErrorCode::Success,
        Err(e) => {
            crate::ffi::error::set_last_error(e);
            OxiGdalErrorCode::AllocationFailed
        }
    }
}

/// Gets the current cache statistics.
///
/// # Parameters
/// - `out_size_mb`: Current cache size in MB
/// - `out_max_mb`: Maximum cache size in MB
/// - `out_entries`: Number of cached entries
#[unsafe(no_mangle)]
pub unsafe extern "C" fn oxigdal_cache_get_info(
    out_size_mb: *mut std::os::raw::c_int,
    out_max_mb: *mut std::os::raw::c_int,
    out_entries: *mut std::os::raw::c_int,
) -> OxiGdalErrorCode {
    if out_size_mb.is_null() || out_max_mb.is_null() || out_entries.is_null() {
        crate::ffi::error::set_last_error("Null pointer in cache info".to_string());
        return OxiGdalErrorCode::NullPointer;
    }

    match CACHE.lock() {
        Ok(cache) => {
            if let Some(ref c) = *cache {
                unsafe {
                    *out_size_mb = (c.current_size_bytes / 1024 / 1024) as i32;
                    *out_max_mb = (c.max_size_bytes / 1024 / 1024) as i32;
                    *out_entries = c.entries.len() as i32;
                }
                OxiGdalErrorCode::Success
            } else {
                unsafe {
                    *out_size_mb = 0;
                    *out_max_mb = 0;
                    *out_entries = 0;
                }
                OxiGdalErrorCode::Success
            }
        }
        Err(e) => {
            crate::ffi::error::set_last_error(e.to_string());
            OxiGdalErrorCode::Unknown
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_initialization() {
        let result = init_cache(50);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cache_put_get() {
        init_cache(100).ok();

        let data = vec![1, 2, 3, 4];
        put_cached_tile("test_key".to_string(), data.clone(), 2, 2, 1);

        let cached = get_cached_tile("test_key");
        assert!(cached.is_some());

        let (cached_data, width, height, channels) = cached.expect("cached tile");
        assert_eq!(cached_data, data);
        assert_eq!(width, 2);
        assert_eq!(height, 2);
        assert_eq!(channels, 1);
    }

    #[test]
    fn test_cache_eviction() {
        // Create small cache
        init_cache(1).ok(); // 1MB

        // Add tiles until eviction happens
        for i in 0..100 {
            let data = vec![0u8; 100_000]; // 100KB each
            put_cached_tile(format!("tile_{}", i), data, 100, 100, 1);
        }

        // Verify cache didn't exceed limit
        let mut size_mb = 0;
        let mut max_mb = 0;
        let mut entries = 0;
        let result = unsafe { oxigdal_cache_get_info(&mut size_mb, &mut max_mb, &mut entries) };
        assert_eq!(result, OxiGdalErrorCode::Success);
        assert!(size_mb <= max_mb);
    }

    #[test]
    fn test_cache_clear() {
        init_cache(100).ok();
        put_cached_tile("test".to_string(), vec![1, 2, 3], 1, 1, 3);

        let result = unsafe { oxigdal_cache_clear() };
        assert_eq!(result, OxiGdalErrorCode::Success);

        let cached = get_cached_tile("test");
        assert!(cached.is_none());
    }

    #[test]
    fn test_cache_resize() {
        init_cache(100).ok();

        let result = unsafe { oxigdal_cache_set_size(50) };
        assert_eq!(result, OxiGdalErrorCode::Success);

        let result = unsafe { oxigdal_cache_set_size(-1) };
        assert_eq!(result, OxiGdalErrorCode::InvalidArgument);
    }
}
