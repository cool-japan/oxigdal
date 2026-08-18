//! Regression tests for the `proj4rs-compat` feature.
//!
//! `proj4rs-compat` deliberately does **not** imply `std`: the only thing it
//! unlocks is `From<proj4rs::errors::Error> for Error`, which needs nothing
//! beyond `alloc`.  Historically that impl was compiled under `proj4rs-compat`
//! while the [`Error::Proj4rsError`] variant and the `Error::from_proj4rs`
//! constructor it calls were gated on `std`, so
//! `--no-default-features --features proj4rs-compat` never compiled.
//!
//! The mere existence of this file under
//! `--no-default-features --features proj4rs-compat` is the regression check:
//! it fails to build if `from_proj4rs` / `Proj4rsError` regress behind a
//! `std`-only gate again.
#![cfg(feature = "proj4rs-compat")]

use oxigeo_proj::Error;

#[test]
fn from_proj4rs_is_reachable_without_std() {
    let err = Error::from_proj4rs("garbage +proj string");
    assert!(matches!(err, Error::Proj4rsError(_)));
}

#[test]
fn from_proj4rs_preserves_the_message() {
    let err = Error::from_proj4rs("NoSuchProjection");
    match err {
        Error::Proj4rsError(message) => assert_eq!(message, "NoSuchProjection"),
        other => panic!("expected Error::Proj4rsError, got {other:?}"),
    }
}

#[test]
fn proj4rs_error_display_is_prefixed() {
    // `Display` comes from thiserror's `#[error("Proj4rs error: {0}")]`, which
    // must also work when `thiserror/std` is off.
    let err = Error::from_proj4rs("boom");
    assert_eq!(err.to_string(), "Proj4rs error: boom");
}
