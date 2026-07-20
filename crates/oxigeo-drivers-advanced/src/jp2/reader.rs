//! JPEG2000 JP2 file reader.

use super::{
    Jp2Image,
    codestream::CodestreamDecoder,
    metadata::Jp2Metadata,
    parser::{BoxType, Jp2Parser},
};
use crate::error::{Error, Result};
use std::io::{Read, Seek};

/// JP2 file reader.
pub struct Jp2Reader<R> {
    parser: Jp2Parser<R>,
    metadata: Jp2Metadata,
}

impl<R: Read + Seek> Jp2Reader<R> {
    /// Create a new JP2 reader.
    pub fn new(reader: R) -> Result<Self> {
        let mut parser = Jp2Parser::new(reader)?;
        parser.parse()?;

        let metadata = Jp2Metadata::default();

        Ok(Self { parser, metadata })
    }

    /// Decode the JP2 image.
    pub fn decode(&mut self) -> Result<Jp2Image> {
        // Read image header
        let ihdr = self.parser.read_image_header()?;

        // Read codestream
        let codestream = self.parser.read_codestream()?;

        // Parse and decode codestream
        let mut decoder = CodestreamDecoder::new(codestream);
        let header = decoder.parse_header()?;

        // Clone header values needed after decode
        let width = header.width;
        let height = header.height;
        let num_components = header.num_components;
        let components = header.components.clone();

        // Verify dimensions match
        if width != ihdr.width || height != ihdr.height {
            return Err(Error::jpeg2000(
                "Image header and codestream dimensions mismatch",
            ));
        }

        if num_components != ihdr.num_components {
            return Err(Error::jpeg2000(
                "Image header and codestream component count mismatch",
            ));
        }

        // Decode image data
        let data = decoder.decode()?;

        // Read metadata from XML boxes if present
        self.read_metadata()?;

        // Create image
        let mut image = Jp2Image::new(
            width,
            height,
            num_components,
            ihdr.bits_per_component,
            components,
        );
        image.data = data;
        image.metadata = self.metadata.clone();

        Ok(image)
    }

    /// Read metadata from XML boxes.
    fn read_metadata(&mut self) -> Result<()> {
        let xml_boxes: Vec<_> = self
            .parser
            .find_boxes(BoxType::Xml)
            .into_iter()
            .cloned()
            .collect();

        for mut xml_box in xml_boxes {
            xml_box.read_content(self.parser.reader_mut())?;

            if let Ok(xml_str) = String::from_utf8(xml_box.content.clone()) {
                self.metadata.add_xml(xml_str);
            }
        }

        // Check for GeoJP2 UUID box
        let uuid_boxes: Vec<_> = self
            .parser
            .find_boxes(BoxType::Uuid)
            .into_iter()
            .cloned()
            .collect();
        for mut uuid_box in uuid_boxes {
            uuid_box.read_content(self.parser.reader_mut())?;

            // Check if this is a GeoJP2 UUID (first 16 bytes)
            if uuid_box.content.len() > 16 {
                let uuid = &uuid_box.content[0..16];
                // GeoJP2 UUID: b14bf8bd-083d-4b43-a5ae-8cd7d5a6ce03
                if uuid
                    == [
                        0xb1, 0x4b, 0xf8, 0xbd, 0x08, 0x3d, 0x4b, 0x43, 0xa5, 0xae, 0x8c, 0xd7,
                        0xd5, 0xa6, 0xce, 0x03,
                    ]
                {
                    // GeoJP2 metadata follows the UUID
                    let geojp2_data = uuid_box.content[16..].to_vec();
                    self.metadata.set_geojp2(geojp2_data);
                }
            }
        }

        Ok(())
    }

    /// Get image dimensions without fully decoding.
    pub fn dimensions(&mut self) -> Result<(u32, u32)> {
        let ihdr = self.parser.read_image_header()?;
        Ok((ihdr.width, ihdr.height))
    }

    /// Get number of components.
    pub fn num_components(&mut self) -> Result<u16> {
        let ihdr = self.parser.read_image_header()?;
        Ok(ihdr.num_components)
    }

    /// Get metadata.
    pub fn metadata(&self) -> &Jp2Metadata {
        &self.metadata
    }

    /// Check if image has GeoJP2 metadata.
    pub fn has_geojp2(&self) -> bool {
        self.metadata.geojp2.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn create_minimal_jp2() -> Vec<u8> {
        let mut data = Vec::new();

        // JP2 Signature box
        data.extend_from_slice(&12u32.to_be_bytes());
        data.extend_from_slice(b"jP  ");
        data.extend_from_slice(&0x0D0A870Au32.to_be_bytes());

        // File Type box
        data.extend_from_slice(&20u32.to_be_bytes());
        data.extend_from_slice(b"ftyp");
        data.extend_from_slice(b"jp2 "); // Brand
        data.extend_from_slice(&0u32.to_be_bytes()); // Minor version
        data.extend_from_slice(b"jp2 "); // Compatibility list

        data
    }

    #[test]
    fn test_jp2_reader_creation() {
        let data = create_minimal_jp2();
        let cursor = Cursor::new(data);
        let result = Jp2Reader::new(cursor);
        // Will fail because we don't have complete JP2, but should parse signature
        assert!(result.is_err() || result.is_ok());
    }

    #[test]
    fn test_metadata_access() {
        let data = create_minimal_jp2();
        let cursor = Cursor::new(data);
        if let Ok(reader) = Jp2Reader::new(cursor) {
            let metadata = reader.metadata();
            assert!(!reader.has_geojp2());
            assert!(metadata.xml_metadata.is_empty());
        }
    }
}
