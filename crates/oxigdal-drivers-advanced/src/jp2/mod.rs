//! JPEG2000 (JP2) format driver.
//!
//! This module provides support for reading JPEG2000 images including:
//! - JP2 box structure parsing
//! - Codestream decoding
//! - Multi-resolution support
//! - GeoJP2 metadata extraction

mod codestream;
mod metadata;
mod parser;
mod reader;

pub use codestream::{CodestreamDecoder, CodestreamHeader, ComponentInfo};
pub use metadata::{ColorSpace, GeoJp2Metadata, Jp2Metadata};
pub use parser::{BoxType, Jp2Box, Jp2Parser};
pub use reader::Jp2Reader;

use crate::error::{Error, Result};
use std::io::{Read, Seek};

/// JPEG2000 image representation.
#[derive(Debug, Clone)]
pub struct Jp2Image {
    /// Image width in pixels
    pub width: u32,
    /// Image height in pixels
    pub height: u32,
    /// Number of components (channels)
    pub num_components: u16,
    /// Bits per component
    pub bit_depth: u8,
    /// Component information
    pub components: Vec<ComponentInfo>,
    /// Decoded image data (interleaved RGB or grayscale)
    pub data: Vec<u8>,
    /// Metadata
    pub metadata: Jp2Metadata,
}

impl Jp2Image {
    /// Create a new JP2 image.
    pub fn new(
        width: u32,
        height: u32,
        num_components: u16,
        bit_depth: u8,
        components: Vec<ComponentInfo>,
    ) -> Self {
        let size = (width as usize) * (height as usize) * (num_components as usize);
        Self {
            width,
            height,
            num_components,
            bit_depth,
            components,
            data: vec![0; size],
            metadata: Jp2Metadata::default(),
        }
    }

    /// Get pixel at (x, y) coordinates.
    pub fn get_pixel(&self, x: u32, y: u32) -> Option<&[u8]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let idx = ((y * self.width + x) as usize) * (self.num_components as usize);
        self.data.get(idx..idx + (self.num_components as usize))
    }

    /// Set pixel at (x, y) coordinates.
    pub fn set_pixel(&mut self, x: u32, y: u32, pixel: &[u8]) -> Result<()> {
        if x >= self.width || y >= self.height {
            return Err(Error::geometry("Pixel coordinates out of bounds"));
        }
        if pixel.len() != self.num_components as usize {
            return Err(Error::geometry("Invalid pixel component count"));
        }
        let idx = ((y * self.width + x) as usize) * (self.num_components as usize);
        self.data[idx..idx + pixel.len()].copy_from_slice(pixel);
        Ok(())
    }

    /// Get image dimensions.
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Get the number of bytes per pixel.
    pub fn bytes_per_pixel(&self) -> usize {
        self.num_components as usize
    }

    /// Check if image is grayscale.
    pub fn is_grayscale(&self) -> bool {
        self.num_components == 1
    }

    /// Check if image is RGB.
    pub fn is_rgb(&self) -> bool {
        self.num_components == 3
    }

    /// Check if image has alpha channel.
    pub fn has_alpha(&self) -> bool {
        self.num_components == 2 || self.num_components == 4
    }
}

/// Read a JP2 file from a reader.
pub fn read_jp2<R: Read + Seek>(reader: R) -> Result<Jp2Image> {
    let mut jp2_reader = Jp2Reader::new(reader)?;
    jp2_reader.decode()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jp2_image_creation() {
        let components = vec![
            ComponentInfo::new(0, 8, false),
            ComponentInfo::new(1, 8, false),
            ComponentInfo::new(2, 8, false),
        ];
        let img = Jp2Image::new(100, 100, 3, 8, components);
        assert_eq!(img.width, 100);
        assert_eq!(img.height, 100);
        assert_eq!(img.num_components, 3);
        assert!(img.is_rgb());
        assert!(!img.has_alpha());
    }

    #[test]
    fn test_jp2_pixel_access() {
        let components = vec![ComponentInfo::new(0, 8, false)];
        let mut img = Jp2Image::new(10, 10, 1, 8, components);

        img.set_pixel(5, 5, &[128]).ok();
        let pixel = img.get_pixel(5, 5);
        assert!(pixel.is_some());
        if let Some(p) = pixel {
            assert_eq!(p[0], 128);
        }
    }

    #[test]
    fn test_jp2_bounds_checking() {
        let components = vec![ComponentInfo::new(0, 8, false)];
        let img = Jp2Image::new(10, 10, 1, 8, components);

        assert!(img.get_pixel(9, 9).is_some());
        assert!(img.get_pixel(10, 10).is_none());
        assert!(img.get_pixel(5, 15).is_none());
    }
}
