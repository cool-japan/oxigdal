//! EPSG definition types and database structure.

use crate::error::{Error, Result};
#[cfg(not(feature = "std"))]
use alloc::collections::BTreeMap as HashMap;
use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};
#[cfg(feature = "std")]
use std::collections::HashMap;

/// EPSG code definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EpsgDefinition {
    /// EPSG code
    pub code: u32,
    /// Human-readable name
    pub name: String,
    /// PROJ string representation
    pub proj_string: String,
    /// WKT (Well-Known Text) representation
    pub wkt: Option<String>,
    /// CRS type (geographic, projected, etc.)
    pub crs_type: CrsType,
    /// Area of use
    pub area_of_use: String,
    /// Unit of measurement
    pub unit: String,
    /// Datum name
    pub datum: String,
}

/// Type of coordinate reference system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CrsType {
    /// Geographic CRS (latitude/longitude)
    Geographic,
    /// Projected CRS (planar coordinates)
    Projected,
    /// Geocentric CRS (3D Cartesian)
    Geocentric,
    /// Vertical CRS (heights/depths)
    Vertical,
    /// Compound CRS (combination of horizontal + vertical)
    Compound,
    /// Engineering CRS (local coordinate systems)
    Engineering,
}

// Include build-generated EPSG registrations. `build.rs` always emits this file
// (an empty `register_generated_epsg` when the snapshot is absent), and the
// generated body only needs `String` plus `Map::entry`, both of which `alloc`
// provides — so the snapshot is available in `no_std` builds too.
include!(concat!(env!("OUT_DIR"), "/generated_epsg.rs"));

/// EPSG database containing common coordinate reference systems.
pub struct EpsgDatabase {
    pub(crate) definitions: HashMap<u32, EpsgDefinition>,
}

impl EpsgDatabase {
    /// Creates a new EPSG database with built-in definitions.
    pub fn new() -> Self {
        let mut db = Self {
            definitions: HashMap::new(),
        };
        db.initialize_builtin_codes();
        db
    }

    /// Looks up an EPSG code in the database.
    pub fn lookup(&self, code: u32) -> Result<&EpsgDefinition> {
        self.definitions
            .get(&code)
            .ok_or_else(|| Error::epsg_not_found(code))
    }

    /// Checks if an EPSG code exists in the database.
    pub fn contains(&self, code: u32) -> bool {
        self.definitions.contains_key(&code)
    }

    /// Returns all available EPSG codes.
    pub fn codes(&self) -> Vec<u32> {
        let mut codes: Vec<u32> = self.definitions.keys().copied().collect();
        codes.sort_unstable();
        codes
    }

    /// Returns the number of EPSG codes in the database.
    pub fn len(&self) -> usize {
        self.definitions.len()
    }

    /// Checks if the database is empty.
    pub fn is_empty(&self) -> bool {
        self.definitions.is_empty()
    }

    /// Adds a custom EPSG definition to the database.
    pub fn add_definition(&mut self, definition: EpsgDefinition) {
        self.definitions.insert(definition.code, definition);
    }

    /// Removes an EPSG code from the database, if present.
    ///
    /// Returns the removed definition, or `None` when the code was not in the
    /// database.  Primarily useful for testing and dynamic management.
    pub fn remove_definition(&mut self, code: u32) -> Option<EpsgDefinition> {
        self.definitions.remove(&code)
    }

    /// Initializes the database with built-in EPSG codes.
    fn initialize_builtin_codes(&mut self) {
        super::geographic::register_geographic_crs(self);
        super::utm::register_utm_zones(self);
        super::projected::register_projected_crs(self);
        super::extended::register_extended_crs(self);
        register_generated_epsg(self);
    }
}

impl Default for EpsgDatabase {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "proj-db")]
impl EpsgDatabase {
    /// Attempts to open the first available system PROJ.db and populate this
    /// database with any EPSG codes not already present.
    ///
    /// Built-in entries are **never** overwritten (they win on collision).
    ///
    /// Returns:
    /// - `Ok(None)` — no PROJ.db was found on the system.
    /// - `Ok(Some(n))` — `n` new entries were inserted.
    /// - `Err(_)` — a database that was found could not be read.
    pub fn populate_from_system_proj_db(&mut self) -> crate::error::Result<Option<usize>> {
        use crate::epsg::proj_db::ProjDb;
        match ProjDb::open_first_available()? {
            None => Ok(None),
            Some(proj_db) => Ok(Some(super::proj_db::populate_from_proj_db(self, &proj_db)?)),
        }
    }
}

/// Global EPSG database instance (std only — once_cell::sync::Lazy requires std).
#[cfg(feature = "std")]
static EPSG_DB: once_cell::sync::Lazy<EpsgDatabase> = once_cell::sync::Lazy::new(EpsgDatabase::new);

/// Looks up an EPSG code in the global database.
#[cfg(feature = "std")]
pub fn lookup_epsg(code: u32) -> Result<&'static EpsgDefinition> {
    EPSG_DB.lookup(code)
}

/// Checks if an EPSG code exists in the global database.
#[cfg(feature = "std")]
pub fn contains_epsg(code: u32) -> bool {
    EPSG_DB.contains(code)
}

/// Returns all available EPSG codes from the global database.
#[cfg(feature = "std")]
pub fn available_epsg_codes() -> Vec<u32> {
    EPSG_DB.codes()
}
