//! Color table (palette) type for indexed-color raster support

#[cfg(not(feature = "std"))]
#[allow(unused_imports)]
use crate::compat::*;
use serde::{Deserialize, Serialize};

/// A single color entry in a palette
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColorEntry {
    /// Red channel
    pub r: u8,
    /// Green channel
    pub g: u8,
    /// Blue channel
    pub b: u8,
    /// Alpha channel (255 = fully opaque)
    pub a: u8,
}

impl From<[u8; 4]> for ColorEntry {
    fn from(bytes: [u8; 4]) -> Self {
        Self {
            r: bytes[0],
            g: bytes[1],
            b: bytes[2],
            a: bytes[3],
        }
    }
}

impl From<(u8, u8, u8)> for ColorEntry {
    fn from((r, g, b): (u8, u8, u8)) -> Self {
        Self { r, g, b, a: 255 }
    }
}

/// How to interpret a `ColorTable`'s entries
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ColorTableKind {
    /// Each entry is an RGBA color
    #[default]
    Rgba,
    /// Entries form a palette for indexed-color images
    Palette,
    /// Entries describe a grayscale ramp
    Grayscale,
}

/// Palette / color-table mapping pixel indices to RGBA colors
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColorTable {
    entries: Vec<ColorEntry>,
    kind: ColorTableKind,
}

impl Default for ColorTable {
    fn default() -> Self {
        Self::new(ColorTableKind::Rgba)
    }
}

impl ColorTable {
    /// Creates an empty color table with the given kind
    #[must_use]
    pub fn new(kind: ColorTableKind) -> Self {
        Self {
            entries: Vec::new(),
            kind,
        }
    }

    /// Creates an empty color table pre-allocated for `n` entries
    #[must_use]
    pub fn with_capacity(kind: ColorTableKind, n: usize) -> Self {
        Self {
            entries: Vec::with_capacity(n),
            kind,
        }
    }

    /// Appends an entry to the table
    pub fn push(&mut self, entry: ColorEntry) {
        self.entries.push(entry);
    }

    /// Returns the entry at the given index, or `None` if out of range
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&ColorEntry> {
        self.entries.get(index)
    }

    /// Returns the number of entries
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if there are no entries
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns a slice of all entries
    #[must_use]
    pub fn as_slice(&self) -> &[ColorEntry] {
        &self.entries
    }

    /// Returns the kind of this color table
    #[must_use]
    pub fn kind(&self) -> ColorTableKind {
        self.kind
    }

    /// Looks up the RGBA bytes for a `u8` pixel index.
    ///
    /// If the index is beyond the table, returns fully-transparent black.
    #[must_use]
    pub fn to_rgba_bytes(&self, pixel: u8) -> [u8; 4] {
        match self.entries.get(pixel as usize) {
            Some(e) => [e.r, e.g, e.b, e.a],
            None => [0, 0, 0, 0],
        }
    }

    /// Builds a `ColorTable` from a flat `Vec<ColorEntry>`
    #[must_use]
    pub fn from_rgba_vec(kind: ColorTableKind, vec: Vec<ColorEntry>) -> Self {
        Self { entries: vec, kind }
    }

    /// Constructs a linear grayscale ramp of `n` entries.
    ///
    /// Entry 0 is `(0, 0, 0, 255)` and entry `n-1` is `(255, 255, 255, 255)`,
    /// with linear interpolation in between.  Requires `n >= 1`.
    #[must_use]
    pub fn grayscale_ramp(n: usize) -> Self {
        let mut table = Self::with_capacity(ColorTableKind::Grayscale, n);
        if n == 0 {
            return table;
        }
        if n == 1 {
            table.push(ColorEntry {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            });
            return table;
        }
        for i in 0..n {
            let v = (i * 255 / (n - 1)) as u8;
            table.push(ColorEntry {
                r: v,
                g: v,
                b: v,
                a: 255,
            });
        }
        table
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn test_color_table_roundtrip() {
        let mut table = ColorTable::new(ColorTableKind::Rgba);
        for i in 0u8..=255 {
            table.push(ColorEntry {
                r: i,
                g: i,
                b: 0,
                a: 255,
            });
        }
        assert_eq!(table.len(), 256);
        let entry = table.get(42).expect("entry should exist");
        assert_eq!(entry.r, 42);
        assert_eq!(entry.g, 42);
        assert_eq!(entry.b, 0);
    }

    #[test]
    fn test_color_entry_from_tuples() {
        let from_array: ColorEntry = [10u8, 20, 30, 200].into();
        assert_eq!(from_array.r, 10);
        assert_eq!(from_array.g, 20);
        assert_eq!(from_array.b, 30);
        assert_eq!(from_array.a, 200);

        let from_tuple: ColorEntry = (100u8, 150, 200).into();
        assert_eq!(from_tuple.r, 100);
        assert_eq!(from_tuple.g, 150);
        assert_eq!(from_tuple.b, 200);
        assert_eq!(from_tuple.a, 255);
    }

    #[test]
    fn test_to_rgba_bytes_indexed_u8() {
        let mut table = ColorTable::new(ColorTableKind::Palette);
        for i in 0u8..=10 {
            table.push(ColorEntry {
                r: i * 10,
                g: i,
                b: 0,
                a: 255,
            });
        }
        let entry = table.get(5).expect("entry should exist");
        let bytes = table.to_rgba_bytes(5);
        assert_eq!(bytes, [entry.r, entry.g, entry.b, entry.a]);
    }

    #[test]
    fn test_grayscale_ramp_linear() {
        let ramp = ColorTable::grayscale_ramp(256);
        assert_eq!(ramp.len(), 256);
        assert_eq!(ramp.get(0).expect("first").r, 0);
        assert_eq!(ramp.get(255).expect("last").r, 255);
        assert_eq!(ramp.kind(), ColorTableKind::Grayscale);
    }

    #[test]
    fn test_color_table_serde_roundtrip() {
        let mut table = ColorTable::new(ColorTableKind::Palette);
        table.push(ColorEntry {
            r: 1,
            g: 2,
            b: 3,
            a: 4,
        });
        table.push(ColorEntry {
            r: 5,
            g: 6,
            b: 7,
            a: 8,
        });

        let json = serde_json::to_string(&table).expect("serialize");
        let recovered: ColorTable = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(table, recovered);
    }
}
