//! Tile provider implementations.

use super::cache::TileCache;
use super::protocol::{
    FileSystemTileProtocol, TileCoordinate, TileProtocol, TileRequest, TileResponse,
};
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

/// User-supplied in-memory tile generator.
///
/// Backs [`TileSource::Generator`]: the provider calls [`Self::generate`] to
/// synthesize a tile for a coordinate (e.g. procedurally rendered debug tiles,
/// on-the-fly rasterization, or a test fixture) rather than fetching it over a
/// network or reading it from disk.
#[async_trait]
pub trait TileGenerator: Send + Sync {
    /// Generate a tile for the given coordinate.
    async fn generate(&self, coord: &TileCoordinate) -> Result<TileResponse>;
}

/// Tile source configuration.
#[derive(Debug, Clone)]
pub enum TileSource {
    /// HTTP/HTTPS URL template
    Http {
        /// URL template with `{z}`, `{x}`, `{y}` placeholders
        url_template: String,
        /// Minimum supported zoom level
        min_zoom: u8,
        /// Maximum supported zoom level
        max_zoom: u8,
    },

    /// Local file system
    FileSystem {
        /// Root directory containing tile files
        base_path: std::path::PathBuf,
        /// Tile image format
        format: super::protocol::TileFormat,
    },

    /// In-memory tile generator
    Generator {
        /// Minimum supported zoom level
        min_zoom: u8,
        /// Maximum supported zoom level
        max_zoom: u8,
    },
}

/// Standard tile provider with caching.
///
/// `fetch_tile` routes on the configured [`TileSource`]:
/// - [`TileSource::Http`] delegates to the supplied [`TileProtocol`].
/// - [`TileSource::FileSystem`] reads tiles from disk via an internal
///   [`FileSystemTileProtocol`] (the supplied protocol is not used).
/// - [`TileSource::Generator`] calls the [`TileGenerator`] registered with
///   [`Self::with_generator`]; if none is set, an honest typed error is
///   returned rather than silently falling back to the supplied protocol.
pub struct StandardTileProvider {
    /// Source configuration — actively drives routing in `fetch_tile`.
    source: TileSource,
    cache: Option<Arc<TileCache>>,
    /// Protocol used for `Http` sources.
    protocol: Arc<dyn TileProtocol>,
    /// Backing disk protocol built for `FileSystem` sources.
    fs_protocol: Option<FileSystemTileProtocol>,
    /// Generator used for `Generator` sources.
    generator: Option<Arc<dyn TileGenerator>>,
}

impl StandardTileProvider {
    /// Create a new tile provider.
    ///
    /// For [`TileSource::FileSystem`] the `protocol` argument is unused (disk
    /// reads are handled internally); pass any protocol (e.g. a placeholder) or
    /// prefer routing through the source directly. For [`TileSource::Generator`]
    /// register a generator with [`Self::with_generator`].
    pub fn new(source: TileSource, protocol: Arc<dyn TileProtocol>) -> Self {
        let fs_protocol = match &source {
            TileSource::FileSystem { base_path, format } => {
                Some(FileSystemTileProtocol::new(base_path.clone(), *format))
            }
            _ => None,
        };
        Self {
            source,
            cache: None,
            protocol,
            fs_protocol,
            generator: None,
        }
    }

    /// Enable caching.
    pub fn with_cache(mut self, cache: Arc<TileCache>) -> Self {
        self.cache = Some(cache);
        self
    }

    /// Register an in-memory tile generator, used when the source is
    /// [`TileSource::Generator`].
    pub fn with_generator(mut self, generator: Arc<dyn TileGenerator>) -> Self {
        self.generator = Some(generator);
        self
    }

    /// Fetch a tile from the configured source.
    async fn fetch_tile(&self, request: &TileRequest) -> Result<TileResponse> {
        match &self.source {
            TileSource::Http {
                min_zoom, max_zoom, ..
            } => {
                if request.coord.z < *min_zoom || request.coord.z > *max_zoom {
                    return Err(StreamingError::InvalidOperation(format!(
                        "Zoom level {} out of range [{}, {}]",
                        request.coord.z, min_zoom, max_zoom
                    )));
                }
                self.protocol.get_tile(request).await
            }
            TileSource::FileSystem { .. } => {
                let fs = self.fs_protocol.as_ref().ok_or_else(|| {
                    StreamingError::InvalidState(
                        "FileSystem source without an initialized disk protocol".to_string(),
                    )
                })?;
                fs.get_tile(request).await
            }
            TileSource::Generator { min_zoom, max_zoom } => {
                if request.coord.z < *min_zoom || request.coord.z > *max_zoom {
                    return Err(StreamingError::InvalidOperation(format!(
                        "Zoom level {} out of range [{}, {}]",
                        request.coord.z, min_zoom, max_zoom
                    )));
                }
                match &self.generator {
                    Some(generator) => generator.generate(&request.coord).await,
                    None => Err(StreamingError::InvalidOperation(
                        "Generator source requires a generator; register one via \
                         StandardTileProvider::with_generator"
                            .to_string(),
                    )),
                }
            }
        }
    }
}

#[async_trait]
impl TileProvider for StandardTileProvider {
    async fn get_tile(&self, request: &TileRequest) -> Result<TileResponse> {
        // Check cache first
        if let Some(cache) = &self.cache
            && let Some(response) = cache.get(&request.coord).await
        {
            debug!("Cache hit for tile {}", request.coord);
            return Ok(response);
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

        Err(StreamingError::Other(format!(
            "All providers failed for tile {}",
            request.coord
        )))
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
#[allow(clippy::panic)]
mod tests {
    use super::super::protocol::{TileCoordinate, TileFormat, TileMetadata};
    use super::*;
    use bytes::Bytes;

    /// A protocol that always yields a distinctive marker body, used to prove
    /// that FileSystem/Generator sources do NOT delegate to it.
    struct MarkerProtocol;

    #[async_trait]
    impl TileProtocol for MarkerProtocol {
        async fn get_tile(&self, request: &TileRequest) -> Result<TileResponse> {
            Ok(TileResponse::new(
                request.coord,
                Bytes::from_static(b"FROM_HTTP_PROTOCOL"),
                "image/png".to_string(),
            ))
        }
        async fn has_tile(&self, _coord: &TileCoordinate) -> Result<bool> {
            Ok(true)
        }
        async fn get_tile_metadata(&self, coord: &TileCoordinate) -> Result<TileMetadata> {
            Ok(TileMetadata {
                coord: *coord,
                size_bytes: 0,
                format: TileFormat::Png,
                created_at: None,
                modified_at: None,
                bbox: None,
                metadata: std::collections::HashMap::new(),
            })
        }
        fn zoom_levels(&self) -> (u8, u8) {
            (0, 31)
        }
        fn tile_size(&self) -> (u32, u32) {
            (256, 256)
        }
    }

    struct SolidGenerator;

    #[async_trait]
    impl TileGenerator for SolidGenerator {
        async fn generate(&self, coord: &TileCoordinate) -> Result<TileResponse> {
            Ok(TileResponse::new(
                *coord,
                Bytes::from(format!("GEN:{}/{}/{}", coord.z, coord.x, coord.y).into_bytes()),
                "image/png".to_string(),
            ))
        }
    }

    #[tokio::test]
    async fn test_filesystem_source_reads_disk_not_protocol() {
        let dir = tempfile::tempdir().expect("temp dir");
        let coord = TileCoordinate::new(4, 2, 3);
        let tile_file = dir.path().join("4/2/3.png");
        tokio::fs::create_dir_all(tile_file.parent().expect("parent"))
            .await
            .expect("mkdir");
        tokio::fs::write(&tile_file, b"FROM_DISK")
            .await
            .expect("write");

        let source = TileSource::FileSystem {
            base_path: dir.path().to_path_buf(),
            format: TileFormat::Png,
        };
        // Supplied protocol would return a different marker if (incorrectly) used.
        let provider = StandardTileProvider::new(source, Arc::new(MarkerProtocol));
        let request = TileRequest::new(coord, TileFormat::Png);
        let resp = provider.get_tile(&request).await.expect("disk tile");
        assert_eq!(&resp.data[..], b"FROM_DISK");
    }

    #[tokio::test]
    async fn test_generator_source_uses_generator() {
        let source = TileSource::Generator {
            min_zoom: 0,
            max_zoom: 20,
        };
        let coord = TileCoordinate::new(6, 1, 2);
        let provider = StandardTileProvider::new(source, Arc::new(MarkerProtocol))
            .with_generator(Arc::new(SolidGenerator));
        let request = TileRequest::new(coord, TileFormat::Png);
        let resp = provider.get_tile(&request).await.expect("generated tile");
        assert_eq!(&resp.data[..], b"GEN:6/1/2");
    }

    #[tokio::test]
    async fn test_generator_source_without_generator_errors() {
        let source = TileSource::Generator {
            min_zoom: 0,
            max_zoom: 20,
        };
        let provider = StandardTileProvider::new(source, Arc::new(MarkerProtocol));
        let request = TileRequest::new(TileCoordinate::new(6, 1, 2), TileFormat::Png);
        let err = provider
            .get_tile(&request)
            .await
            .expect_err("should error without generator");
        assert!(matches!(err, StreamingError::InvalidOperation(_)));
    }

    #[tokio::test]
    async fn test_http_source_uses_protocol() {
        let source = TileSource::Http {
            url_template: "https://example.com/{z}/{x}/{y}.png".to_string(),
            min_zoom: 0,
            max_zoom: 18,
        };
        let provider = StandardTileProvider::new(source, Arc::new(MarkerProtocol));
        let request = TileRequest::new(TileCoordinate::new(5, 1, 1), TileFormat::Png);
        let resp = provider.get_tile(&request).await.expect("http tile");
        assert_eq!(&resp.data[..], b"FROM_HTTP_PROTOCOL");
    }

    #[test]
    fn test_tile_source() {
        let source = TileSource::Http {
            url_template: "https://tile.openstreetmap.org/{z}/{x}/{y}.png".to_string(),
            min_zoom: 0,
            max_zoom: 18,
        };

        match source {
            TileSource::Http {
                min_zoom, max_zoom, ..
            } => {
                assert_eq!(min_zoom, 0);
                assert_eq!(max_zoom, 18);
            }
            _ => panic!("Wrong variant"),
        }
    }
}
