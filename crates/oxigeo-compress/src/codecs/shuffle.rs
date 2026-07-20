//! Byte-shuffle pre-filter for chunked numeric arrays
//!
//! The byte-shuffle filter is a permutation that improves the compressibility
//! of numeric arrays by grouping bytes of the same significance from successive
//! elements. For an array of `typesize`-byte elements, the filter reorganises
//! the buffer into `typesize` "bands", where band `i` contains the `i`-th byte
//! of every element.
//!
//! This is the same transform used in c-blosc2 and provides much better
//! compression ratios for codecs such as LZ4, Zstd, or Snappy when applied to
//! IEEE-754 floats, signed integers, or any data with structure on the
//! element-byte level.
//!
//! # Layout
//!
//! For `data.len() = N * typesize + tail`, the shuffled output has the form:
//!
//! ```text
//! [ band_0 || band_1 || ... || band_(typesize-1) || tail ]
//! ```
//!
//! where `band_i` contains `N` bytes (`data[i], data[i + typesize], ...`)
//! and `tail` is the trailing `data.len() % typesize` bytes that do not form
//! a complete element. The tail is preserved verbatim at the end of the
//! shuffled buffer so the transform is lossless for any input length.
//!
//! # Examples
//!
//! ```
//! use oxigeo_compress::codecs::shuffle::{byte_shuffle, byte_unshuffle};
//!
//! // A 2-element f32 array `[a0 a1 a2 a3 | b0 b1 b2 b3]` shuffles to
//! // `[a0 b0 | a1 b1 | a2 b2 | a3 b3]`.
//! let data = [1u8, 2, 3, 4, 5, 6, 7, 8];
//! let shuffled = byte_shuffle(&data, 4);
//! assert_eq!(shuffled, vec![1, 5, 2, 6, 3, 7, 4, 8]);
//!
//! let restored = byte_unshuffle(&shuffled, 4);
//! assert_eq!(restored, data);
//! ```

/// Apply the byte-shuffle filter and return a new buffer.
///
/// `typesize` is the size in bytes of one element. Common values are 1, 2, 4,
/// 8, or 16. A `typesize` of 1 acts as the identity transform, and any input
/// shorter than `typesize` is copied through unchanged.
///
/// The output always has the same length as the input.
pub fn byte_shuffle(data: &[u8], typesize: usize) -> Vec<u8> {
    if typesize <= 1 || data.len() < typesize {
        return data.to_vec();
    }
    let mut out = vec![0u8; data.len()];
    shuffle_into(data, &mut out, typesize);
    out
}

/// Apply the inverse byte-shuffle filter and return a new buffer.
///
/// This is the exact inverse of [`byte_shuffle`] for any input that was
/// produced by `byte_shuffle` with the same `typesize`.
pub fn byte_unshuffle(data: &[u8], typesize: usize) -> Vec<u8> {
    if typesize <= 1 || data.len() < typesize {
        return data.to_vec();
    }
    let mut out = vec![0u8; data.len()];
    unshuffle_into(data, &mut out, typesize);
    out
}

/// Apply the byte-shuffle filter in-place using a scratch buffer.
///
/// This allocates an internal scratch `Vec<u8>` of length `buf.len()` and
/// copies the result back. Use [`byte_shuffle`] when you already have an
/// input buffer to avoid the double-copy.
pub fn byte_shuffle_in_place(buf: &mut [u8], typesize: usize) {
    if typesize <= 1 || buf.len() < typesize {
        return;
    }
    let scratch = byte_shuffle(buf, typesize);
    buf.copy_from_slice(&scratch);
}

/// Apply the inverse byte-shuffle filter in-place using a scratch buffer.
pub fn byte_unshuffle_in_place(buf: &mut [u8], typesize: usize) {
    if typesize <= 1 || buf.len() < typesize {
        return;
    }
    let scratch = byte_unshuffle(buf, typesize);
    buf.copy_from_slice(&scratch);
}

/// Write the byte-shuffled representation of `data` into `out`.
///
/// `out.len()` must equal `data.len()` and `typesize >= 2`.
fn shuffle_into(data: &[u8], out: &mut [u8], typesize: usize) {
    let n_elements = data.len() / typesize;
    let tail_len = data.len() - n_elements * typesize;

    // Reorder element bytes into typesize bands.
    for byte_idx in 0..typesize {
        let band_offset = byte_idx * n_elements;
        for elem_idx in 0..n_elements {
            out[band_offset + elem_idx] = data[elem_idx * typesize + byte_idx];
        }
    }

    // Preserve any trailing bytes that do not form a full element.
    if tail_len > 0 {
        let tail_src_start = n_elements * typesize;
        let tail_dst_start = typesize * n_elements;
        out[tail_dst_start..tail_dst_start + tail_len]
            .copy_from_slice(&data[tail_src_start..tail_src_start + tail_len]);
    }
}

/// Write the unshuffled representation of `data` into `out`.
///
/// This is the inverse of [`shuffle_into`].
fn unshuffle_into(data: &[u8], out: &mut [u8], typesize: usize) {
    let n_elements = data.len() / typesize;
    let tail_len = data.len() - n_elements * typesize;

    for byte_idx in 0..typesize {
        let band_offset = byte_idx * n_elements;
        for elem_idx in 0..n_elements {
            out[elem_idx * typesize + byte_idx] = data[band_offset + elem_idx];
        }
    }

    if tail_len > 0 {
        let tail_src_start = typesize * n_elements;
        let tail_dst_start = n_elements * typesize;
        out[tail_dst_start..tail_dst_start + tail_len]
            .copy_from_slice(&data[tail_src_start..tail_src_start + tail_len]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shuffle_typesize_one_is_identity() {
        let data: Vec<u8> = (0..32).collect();
        assert_eq!(byte_shuffle(&data, 1), data);
        assert_eq!(byte_unshuffle(&data, 1), data);
    }

    #[test]
    fn shuffle_typesize_zero_is_identity() {
        let data: Vec<u8> = (0..16).collect();
        assert_eq!(byte_shuffle(&data, 0), data);
    }

    #[test]
    fn shuffle_short_input_passthrough() {
        let data = [1u8, 2, 3];
        // typesize > data.len()
        assert_eq!(byte_shuffle(&data, 8), data.to_vec());
    }

    #[test]
    fn shuffle_f32_two_elements_canonical_example() {
        // [a0 a1 a2 a3 | b0 b1 b2 b3] -> [a0 b0 | a1 b1 | a2 b2 | a3 b3]
        let data = [0u8, 1, 2, 3, 4, 5, 6, 7];
        let shuffled = byte_shuffle(&data, 4);
        assert_eq!(shuffled, vec![0, 4, 1, 5, 2, 6, 3, 7]);
        let restored = byte_unshuffle(&shuffled, 4);
        assert_eq!(restored, data);
    }

    #[test]
    fn shuffle_in_place_round_trip() {
        let original: Vec<u8> = (0..64).collect();
        let mut buf = original.clone();
        byte_shuffle_in_place(&mut buf, 4);
        assert_ne!(buf, original);
        byte_unshuffle_in_place(&mut buf, 4);
        assert_eq!(buf, original);
    }
}
