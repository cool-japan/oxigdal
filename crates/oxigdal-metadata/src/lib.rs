//! Comprehensive metadata standards support for OxiGDAL.
//!
//! This crate provides support for multiple geospatial and data catalog metadata standards:
//!
//! - **ISO 19115** - Geographic Information - Metadata
//! - **FGDC** - Federal Geographic Data Committee
//! - **INSPIRE** - EU INSPIRE Directive
//! - **DataCite** - DOI metadata for research data
//! - **DCAT** - W3C Data Catalog Vocabulary
//!
//! # Features
//!
//! - Metadata extraction from datasets (GeoTIFF, NetCDF, HDF5, STAC)
//! - Metadata validation and quality scoring
//! - Cross-standard transformation
//! - XML and JSON serialization
//!
//! # Examples
//!
//! ```no_run
//! use oxigdal_metadata::*;
//!
//! # fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! // Create ISO 19115 metadata
//! let iso = iso19115::Iso19115Metadata::builder()
//!     .title("Sentinel-2 Imagery")
//!     .abstract_text("Satellite imagery from Sentinel-2")
//!     .keywords(vec!["satellite", "sentinel-2"])
//!     .build()?;
//!
//! // Validate
//! let validation = validate::validate_iso19115(&iso)?;
//! if !validation.is_complete() {
//!     println!("Missing fields: {:?}", validation.missing_fields());
//! }
//!
//! # Ok(())
//! # }
//! ```

#![cfg_attr(not(feature = "std"), no_std)]
#![warn(missing_docs)]

#[cfg(feature = "std")]
extern crate std;

pub mod datacite;
pub mod dcat;
pub mod error;
pub mod extract;
pub mod fgdc;
pub mod inspire;
pub mod iso19115;
pub mod transform;
pub mod validate;

pub use error::{MetadataError, Result};

/// Common metadata types and utilities.
pub mod common {
    use serde::{Deserialize, Serialize};

    /// Contact information.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ContactInfo {
        /// Individual name
        pub individual_name: Option<String>,
        /// Organization name
        pub organization_name: Option<String>,
        /// Position name
        pub position_name: Option<String>,
        /// Email address
        pub email: Option<String>,
        /// Phone number
        pub phone: Option<String>,
        /// Address
        pub address: Option<Address>,
        /// Online resource
        pub online_resource: Option<String>,
    }

    /// Address information.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Address {
        /// Delivery point (street address)
        pub delivery_point: Option<String>,
        /// City
        pub city: Option<String>,
        /// Administrative area (state/province)
        pub administrative_area: Option<String>,
        /// Postal code
        pub postal_code: Option<String>,
        /// Country
        pub country: Option<String>,
    }

    /// Bounding box for geographic extent.
    #[derive(Debug, Clone, Copy, Serialize, Deserialize)]
    pub struct BoundingBox {
        /// West longitude
        pub west: f64,
        /// East longitude
        pub east: f64,
        /// South latitude
        pub south: f64,
        /// North latitude
        pub north: f64,
    }

    impl BoundingBox {
        /// Create a new bounding box.
        ///
        /// # Arguments
        ///
        /// * `west` - West longitude
        /// * `east` - East longitude
        /// * `south` - South latitude
        /// * `north` - North latitude
        pub fn new(west: f64, east: f64, south: f64, north: f64) -> Self {
            Self {
                west,
                east,
                south,
                north,
            }
        }

        /// Check if the bounding box is valid.
        pub fn is_valid(&self) -> bool {
            self.west <= self.east
                && self.south <= self.north
                && self.west >= -180.0
                && self.east <= 180.0
                && self.south >= -90.0
                && self.north <= 90.0
        }
    }

    /// Keyword with optional thesaurus.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Keyword {
        /// Keyword text
        pub keyword: String,
        /// Thesaurus name
        pub thesaurus: Option<String>,
    }

    /// Temporal extent.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct TemporalExtent {
        /// Start date/time
        pub start: Option<chrono::DateTime<chrono::Utc>>,
        /// End date/time
        pub end: Option<chrono::DateTime<chrono::Utc>>,
    }

    /// License or usage constraints.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct License {
        /// License name
        pub name: String,
        /// License URL
        pub url: Option<String>,
    }
}
