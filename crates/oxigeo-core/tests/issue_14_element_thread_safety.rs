//! Regression tests for cool-japan/oxigeo#14 — `RasterElement` is thread-safe.
//!
//! The typed read path (`GeoTiffReader::read_band_into_typed`) has to split a
//! caller-owned `&mut [T]` across rayon workers. That needs `T: Send`, and until
//! this bound landed the trait only promised `Copy + Default + 'static`, so the
//! typed API and the parallel decode were mutually exclusive — a user reading a
//! `Float32` DEM into a `Vec<f64>` (the issue's own use case) had to give up one
//! of them.
//!
//! `RasterElement` now declares `Send + Sync`. These tests prove the bound is
//! visible **from outside the crate**, i.e. that downstream generic code bounded
//! only on `RasterElement` may move `T` and `&mut [T]` across threads.
//!
//! The bound is not a breaking change: `RasterElement` is sealed by a supertrait
//! living in a *private* module (`oxigeo_core::buffer::element`'s `mod private`,
//! declared without `pub`), so the ten primitive implementors in this crate are
//! the only implementors that can ever exist. Strengthening a sealed trait's
//! supertraits can therefore only relax the obligations of *callers*; there is no
//! out-of-crate `impl` that could fail to satisfy it. A `use
//! oxigeo_core::buffer::element::private::Sealed;` here would not compile, which
//! is the seal working as designed and why that line is prose, not code.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::thread;

use oxigeo_core::buffer::{RasterElement, convert_raw_into};
use oxigeo_core::error::Result;
use oxigeo_core::types::RasterDataType;

/// Compile-time proof that the bound is reachable through `RasterElement` alone.
const fn assert_send_sync<T: Send + Sync>() {}
const fn assert_element_is_thread_safe<T: RasterElement>() {
    assert_send_sync::<T>();
}

#[test]
fn test_issue_14_every_raster_element_is_send_and_sync() {
    assert_element_is_thread_safe::<u8>();
    assert_element_is_thread_safe::<i8>();
    assert_element_is_thread_safe::<u16>();
    assert_element_is_thread_safe::<i16>();
    assert_element_is_thread_safe::<u32>();
    assert_element_is_thread_safe::<i32>();
    assert_element_is_thread_safe::<u64>();
    assert_element_is_thread_safe::<i64>();
    assert_element_is_thread_safe::<f32>();
    assert_element_is_thread_safe::<f64>();
}

/// The shape the driver actually needs: a generic function that knows nothing
/// about `T` beyond `RasterElement`, splitting a caller-owned `&mut [T]` into
/// disjoint chunks and converting into them from several threads at once.
///
/// This is a compile-time claim as much as a runtime one — before the bound
/// existed this function could not be written without `T: Send`.
fn convert_in_parallel<T: RasterElement>(
    src: &[u8],
    src_type: RasterDataType,
    dst: &mut [T],
    chunks: usize,
) -> Result<()> {
    let stride = src_type.size_bytes();
    let per_chunk = dst.len().div_ceil(chunks.max(1));
    thread::scope(|scope| -> Result<()> {
        let mut handles = Vec::new();
        let mut rest_dst = dst;
        let mut rest_src = src;
        while !rest_dst.is_empty() {
            let take = per_chunk.min(rest_dst.len());
            let (head_dst, tail_dst) = rest_dst.split_at_mut(take);
            let (head_src, tail_src) = rest_src.split_at(take * stride);
            rest_dst = tail_dst;
            rest_src = tail_src;
            handles.push(scope.spawn(move || convert_raw_into(head_src, src_type, head_dst)));
        }
        for handle in handles {
            match handle.join() {
                Ok(result) => result?,
                Err(_) => panic!("worker panicked"),
            }
        }
        Ok(())
    })
}

#[test]
fn test_issue_14_typed_conversion_across_threads_matches_serial() {
    let count = 10_000usize;
    let raw: Vec<u8> = (0..count)
        .flat_map(|i| ((i as f32) * 0.25 - 1000.0).to_ne_bytes())
        .collect();

    let mut serial = vec![0.0f64; count];
    convert_raw_into(&raw, RasterDataType::Float32, &mut serial).expect("serial convert");

    for chunks in [1usize, 2, 3, 7, 16] {
        let mut parallel = vec![f64::NAN; count];
        convert_in_parallel(&raw, RasterDataType::Float32, &mut parallel, chunks)
            .expect("threaded convert");
        // Bit-for-bit, not approximately: the conversion is deterministic and the
        // chunk boundaries must not perturb a single sample.
        for (i, (a, b)) in serial.iter().zip(parallel.iter()).enumerate() {
            assert_eq!(a.to_bits(), b.to_bits(), "chunks={chunks}, sample {i}");
        }
    }
}

/// The same, for an integer destination, so the `i128` bridge is exercised on
/// worker threads too.
#[test]
fn test_issue_14_integer_conversion_across_threads_matches_serial() {
    let count = 8_192usize;
    let raw: Vec<u8> = (0..count)
        .flat_map(|i| ((i as u32) * 97).to_ne_bytes())
        .collect();

    let mut serial = vec![0i16; count];
    convert_raw_into(&raw, RasterDataType::UInt32, &mut serial).expect("serial convert");

    let mut parallel = vec![0i16; count];
    convert_in_parallel(&raw, RasterDataType::UInt32, &mut parallel, 5).expect("threaded convert");
    assert_eq!(serial, parallel);
    // Saturation must survive the split: 97 * 8191 far exceeds i16::MAX.
    assert!(serial.contains(&i16::MAX));
}

/// A `Vec<T>` built by generic code must be movable into another thread, which is
/// what makes `read_band_into_typed` usable from a rayon/tokio worker at all.
#[test]
fn test_issue_14_typed_vec_moves_across_thread_boundary() {
    fn build_and_send<T: RasterElement>(len: usize) -> Vec<T> {
        let values = vec![T::default(); len];
        match thread::spawn(move || values).join() {
            Ok(moved) => moved,
            Err(_) => panic!("worker panicked"),
        }
    }

    assert_eq!(build_and_send::<f64>(4).len(), 4);
    assert_eq!(build_and_send::<u16>(4).len(), 4);
}
