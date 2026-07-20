//! Dataset format detection and identification.
//!
//! Exposes [`DatasetFormat`], the enum that tags every supported geospatial
//! format together with its detection helpers (extension-based, magic-byte,
//! and on-disk variants) and a stable human-readable driver name.

/// Detected format of a geospatial dataset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatasetFormat {
    /// GeoTIFF / Cloud-Optimized GeoTIFF (.tif, .tiff)
    GeoTiff,
    /// GeoJSON (.geojson, .json)
    GeoJson,
    /// ESRI Shapefile (.shp)
    Shapefile,
    /// GeoParquet (.parquet, .geoparquet)
    GeoParquet,
    /// NetCDF (.nc, .nc4)
    NetCdf,
    /// HDF5 (.h5, .hdf5, .he5)
    Hdf5,
    /// Zarr (.zarr directory)
    Zarr,
    /// GRIB/GRIB2 (.grib, .grib2, .grb, .grb2)
    Grib,
    /// STAC catalog (.json with STAC metadata)
    Stac,
    /// Terrain formats
    Terrain,
    /// Virtual Raster Tiles (.vrt)
    Vrt,
    /// FlatGeobuf (.fgb)
    FlatGeobuf,
    /// JPEG2000 (.jp2, .j2k)
    Jpeg2000,
    /// GeoPackage (.gpkg, SQLite-based)
    GeoPackage,
    /// PMTiles v3 single-file tile archive (.pmtiles)
    PMTiles,
    /// MBTiles SQLite tile archive (.mbtiles)
    MBTiles,
    /// Cloud Optimized Point Cloud (.copc.laz)
    Copc,
    /// Unknown / user-specified
    Unknown,
}

impl DatasetFormat {
    /// Detect format from file extension.
    ///
    /// Returns `DatasetFormat::Unknown` if the extension is not recognized.
    /// For `.copc.laz` files, the compound extension is checked first.
    pub fn from_extension(path: &str) -> Self {
        // Check compound extensions first (e.g. .copc.laz)
        let lower = path.to_lowercase();
        if lower.ends_with(".copc.laz") {
            return Self::Copc;
        }

        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();

        match ext.as_str() {
            "tif" | "tiff" => Self::GeoTiff,
            "geojson" => Self::GeoJson,
            "shp" => Self::Shapefile,
            "parquet" | "geoparquet" => Self::GeoParquet,
            "nc" | "nc4" => Self::NetCdf,
            "h5" | "hdf5" | "he5" => Self::Hdf5,
            "zarr" => Self::Zarr,
            "grib" | "grib2" | "grb" | "grb2" => Self::Grib,
            "vrt" => Self::Vrt,
            "fgb" => Self::FlatGeobuf,
            "jp2" | "j2k" => Self::Jpeg2000,
            "gpkg" => Self::GeoPackage,
            "pmtiles" => Self::PMTiles,
            "mbtiles" => Self::MBTiles,
            "laz" | "las" => Self::Copc,
            _ => Self::Unknown,
        }
    }

    /// Detect format purely from a byte slice — no file I/O.
    ///
    /// Pass the first 72 bytes of a file (`MAGIC_READ_SIZE`).
    /// Returns `None` when no known magic signature is matched.
    ///
    /// Note: ZIP magic (`PK\x03\x04`) and SQLite magic are mapped to
    /// [`DatasetFormat::GeoPackage`] as a conservative default; callers
    /// can refine the choice with the file extension when needed.
    pub fn detect_from_magic_bytes(bytes: &[u8]) -> Option<Self> {
        use crate::magic::*;

        if bytes.len() < 2 {
            return None;
        }

        // TIFF / BigTIFF — little-endian or big-endian
        if bytes.starts_with(&TIFF_LE_MAGIC) || bytes.starts_with(&TIFF_BE_MAGIC) {
            if bytes.len() >= 4 {
                let version = if bytes[0] == 0x49 {
                    u16::from_le_bytes([bytes[2], bytes[3]])
                } else {
                    u16::from_be_bytes([bytes[2], bytes[3]])
                };
                if version == TIFF_VERSION || version == BIGTIFF_VERSION {
                    return Some(Self::GeoTiff);
                }
            }
            return Some(Self::GeoTiff);
        }

        // JPEG 2000
        if bytes.len() >= 12 && bytes[..12] == JP2_MAGIC {
            return Some(Self::Jpeg2000);
        }

        // HDF5
        if bytes.len() >= 8 && bytes[..8] == HDF5_MAGIC {
            return Some(Self::Hdf5);
        }

        // NetCDF (CDF\x01, CDF\x02, or CDF\x05 for NetCDF-4)
        if bytes.len() >= 4 && bytes[..3] == NETCDF_MAGIC && matches!(bytes[3], 0x01 | 0x02 | 0x05)
        {
            return Some(Self::NetCdf);
        }

        // FlatGeobuf — checked before ZIP to avoid mis-classification
        if bytes.len() >= 8 && bytes[..8] == FLATGEOBUF_MAGIC {
            return Some(Self::FlatGeobuf);
        }

        // PMTiles v3
        if bytes.len() >= 7 && bytes[..7] == PMTILES_MAGIC {
            return Some(Self::PMTiles);
        }

        // LAS / LAZ → COPC
        if bytes.len() >= 4 && bytes[..4] == LAS_MAGIC {
            return Some(Self::Copc);
        }

        // GRIB / GRIB2
        if bytes.len() >= 4 && bytes[..4] == GRIB_MAGIC {
            return Some(Self::Grib);
        }

        // GeoParquet (Parquet PAR1)
        if bytes.len() >= 4 && bytes[..4] == GEOPARQUET_MAGIC {
            return Some(Self::GeoParquet);
        }

        // SQLite database (full 16-byte header)
        if bytes.len() >= 16 && bytes[..16] == SQLITE_MAGIC {
            return Some(Self::GeoPackage);
        }

        // ZIP local-file header (PK\x03\x04) — conservative: assume GeoPackage
        if bytes.len() >= 4 && bytes[..4] == ZIP_MAGIC {
            return Some(Self::GeoPackage);
        }

        None
    }

    /// Detect format by reading magic bytes from a file on disk.
    ///
    /// Opens the file, reads 72 bytes (`MAGIC_READ_SIZE`), then calls
    /// [`DatasetFormat::detect_from_magic_bytes`].  When the magic check yields
    /// [`DatasetFormat::GeoPackage`] the file extension is used to disambiguate
    /// between GeoPackage (`.gpkg`), MBTiles (`.mbtiles`), and generic SQLite.
    ///
    /// # Errors
    ///
    /// Returns `std::io::Error` if the file cannot be opened or read.
    pub fn detect(path: &std::path::Path) -> std::io::Result<Self> {
        use crate::magic::MAGIC_READ_SIZE;
        use std::io::Read as _;

        let mut file = std::fs::File::open(path)?;
        let mut buf = vec![0u8; MAGIC_READ_SIZE];
        let n = file.read(&mut buf)?;
        buf.truncate(n);

        let magic_fmt = Self::detect_from_magic_bytes(&buf);

        let resolved = match magic_fmt {
            // ZIP / SQLite: cross-check extension to pick the right variant
            Some(Self::GeoPackage) => {
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(str::to_lowercase)
                    .unwrap_or_default();
                match ext.as_str() {
                    "mbtiles" => Self::MBTiles,
                    "gpkg" => Self::GeoPackage,
                    _ => Self::GeoPackage,
                }
            }
            Some(fmt) => fmt,
            None => {
                // Fall back to extension
                let path_str = path.to_str().unwrap_or("");
                Self::from_extension(path_str)
            }
        };

        Ok(resolved)
    }

    /// Human-readable driver name (matches GDAL naming convention).
    pub fn driver_name(&self) -> &'static str {
        match self {
            Self::GeoTiff => "GTiff",
            Self::GeoJson => "GeoJSON",
            Self::Shapefile => "ESRI Shapefile",
            Self::GeoParquet => "GeoParquet",
            Self::NetCdf => "netCDF",
            Self::Hdf5 => "HDF5",
            Self::Zarr => "Zarr",
            Self::Grib => "GRIB",
            Self::Stac => "STAC",
            Self::Terrain => "Terrain",
            Self::Vrt => "VRT",
            Self::FlatGeobuf => "FlatGeobuf",
            Self::Jpeg2000 => "JPEG2000",
            Self::GeoPackage => "GPKG",
            Self::PMTiles => "PMTiles",
            Self::MBTiles => "MBTiles",
            Self::Copc => "COPC",
            Self::Unknown => "Unknown",
        }
    }
}

impl core::fmt::Display for DatasetFormat {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.driver_name())
    }
}
