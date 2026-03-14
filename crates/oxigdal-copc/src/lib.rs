//! Pure Rust COPC (Cloud Optimized Point Cloud) reader.
//!
//! Implements an ASPRS LAS 1.4 public header parser ([`las_header`]) and
//! COPC-specific VLR types ([`copc_vlr`]).

pub mod copc_vlr;
pub mod error;
pub mod las_header;

pub use copc_vlr::{CopcInfo, Vlr, VlrKey};
pub use error::CopcError;
pub use las_header::{LasHeader, LasVersion};
