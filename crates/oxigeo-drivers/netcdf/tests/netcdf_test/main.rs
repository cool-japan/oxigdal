//! Integration-test harness for the `oxigeo-netcdf` driver.
//!
//! Cargo auto-discovers `tests/<dir>/main.rs` as an integration-test target, so
//! this file is what turns the sibling `functions*.rs` modules into the
//! `netcdf_test` binary. It only declares the modules — no glob re-exports,
//! which would collide across the five modules.
//!
//! The shared on-disk fixture helper (`functions::temp_file_path`, returning an
//! RAII `TempPath` guard) lives in [`functions`] and is reached from the other
//! modules via `crate::functions::temp_file_path`. It is `netcdf3`-gated,
//! because only the NetCDF-3 suites touch the filesystem.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod functions;
mod functions_2;
mod functions_3;
mod functions_4;
mod functions_5;
