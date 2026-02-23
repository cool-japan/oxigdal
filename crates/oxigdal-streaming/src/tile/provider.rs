//! Tile provider implementations.

use super::cache::TileCache;
use super::protocol::{TileCoordinate, TileProtocol, TileRequest, TileResponse};
use crate::error::{Result, StreamingError};
use async_trait::async_trait;
use std::sync::Arc;
use tracing::{debug, info};

/// Tile provider trait.
#[async_trait]
pub trait TileProvider: Send + Sync {
    /// Get a tile.
    async fn get_tile(&self, request: &TileRequest) -> Result<TileResponse>;

    /// Prefetch multiple tiles.
    async fn prefetch_tiles(&self, requests: Vec<TileRequest>) -> Result<Vec<TileResponse>>;
}

/// Tile source configuration.
#[derive(Debug, Clone)]
pub enum TileSource {
    /// HTTP/HTTPS URL template
    Http {
        url_template: String,
        min_zoom: u8,
        max_zoom: u8,
    },

    /// Local file system
    FileSystem {
        base_path: std::path::PathBuf,
        format: super::protocol::TileFormat,
    },

    /// In-memory tile generator
    Generator {
        min_zoom: u8,
        max_zoom: u8,
    },
}

/// Standard tile provider with caching.
pub struct StandardTileProvider {
    source: TileSource,
    cache: Option<Arc<TileCache>>,
    protocol: Arc<dyn TileProtocol>,
}

impl StandardTileProvider {
    /// Create a new tile provider.
    pub fn new(source: TileSource, protocol: Arc<dyn TileProtocol>) -> Self {
        Self {
            source,
            cache: None,
            protocol,
        }
    }

    /// Enable caching.
    pub fn with_cache(mut self, cache: Arc<TileCache>) -> Self {
        self.cache = Some(cache);
        self
    }

    /// Fetch a tile from the source.
    async fn fetch_tile(&self, request: &TileRequest) -> Result<TileResponse> {
        self.protocol.get_tile(request).await
    }
}

#[async_trait]
impl TileProvider for StandardTileProvider {
    async fn get_tile(&self, request: &TileRequest) -> Result<TileResponse> {
        // Check cache first
        if let Some(cache) = &self.cache {
            if let Some(response) = cache.get(&request.coord).await {
                debug!("Cache hit for tile {}", request.coord);
                return Ok(response);
            }
        }

        // Fetch from source
        let response = self.fetch_tile(request).await?;

        // Store in cache
        if let Some(cache) = &self.cache {
            cache.put(response.clone()).await.ok();
        }

        Ok(response)
    }

    async fn prefetch_tiles(&self, requests: Vec<TileRequest>) -> Result<Vec<TileResponse>> {
        let mut responses = Vec::with_capacity(requests.len());

        for request in requests {
            match self.get_tile(&request).await {
                Ok(response) => responses.push(response),
                Err(e) => {
                    debug!("Failed to prefetch tile {}: {}", request.coord, e);
                }
            }
        }

        Ok(responses)
    }
}

/// Multi-source tile provider with fallback.
pub struct MultiSourceTileProvider {
    providers: Vec<Arc<dyn TileProvider>>,
}

impl MultiSourceTileProvider {
    /// Create a new multi-source provider.
    pub fn new(providers: Vec<Arc<dyn TileProvider>>) -> Self {
        Self { providers }
    }

    /// Add a provider.
    pub fn add_provider(&mut self, provider: Arc<dyn TileProvider>) {
        self.providers.push(provider);
    }
}

#[async_trait]
impl TileProvider for MultiSourceTileProvider {
    async fn get_tile(&self, request: &TileRequest) -> Result<TileResponse> {
        for (i, provider) in self.providers.iter().enumerate() {
            match provider.get_tile(request).await {
                Ok(response) => {
                    if i > 0 {
                        info!("Fallback to provider {} for tile {}", i, request.coord);
                    }
                    return Ok(response);
                }
                Err(e) => {
                    debug!("Provider {} failed for tile {}: {}", i, request.coord, e);
                    continue;
                }
            }
        }

        Err(StreamingError::Other(
            format!("All providers failed for tile {}", request.coord)
        ))
    }

    async fn prefetch_tiles(&self, requests: Vec<TileRequest>) -> Result<Vec<TileResponse>> {
        // Use first provider for prefetch
        if let Some(provider) = self.providers.first() {
            provider.prefetch_tiles(requests).await
        } else {
            Ok(Vec::new())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::protocol::{TileFormat, XyzProtocol};

    #[test]
    fn test_tile_source() {
        let source = TileSource::Http {
            url_template: "https://tile.openstreetmap.org/{z}/{x}/{y}.png".to_string(),
            min_zoom: 0,
            max_zoom: 18,
        };

        match source {
            TileSource::Http { min_zoom, max_zoom, .. } => {
                assert_eq!(min_zoom, 0);
                assert_eq!(max_zoom, 18);
            }
            _ => panic!("Wrong variant"),
        }
    }
}
