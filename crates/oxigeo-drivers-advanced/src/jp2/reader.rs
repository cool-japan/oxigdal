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
        // Surface JPEG2000 Part-2 (JPX) structures we cannot interpret as a
        // typed error, rather than silently decoding only the Part-1 subset and
        // dropping compositing layers / associations / animation.
        if let Some(b) = self
            .parser
            .boxes()
            .iter()
            .find(|b| b.box_type.is_jpx_part2())
        {
            return Err(Error::jpeg2000(format!(
                "JPEG2000 Part-2 (JPX) box {:?} is not supported; only Part-1 JP2 is decodable",
                b.box_type
            )));
        }

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

    /// Minimal, structurally valid raw J2K codestream: 4×4, 1 component, no
    /// wavelet levels, empty SOD (decodes to all-zero coefficients).
    fn minimal_codestream_4x4_gray() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&[0xFF, 0x4F]); // SOC
        // SIZ (Lsiz = 41)
        out.extend_from_slice(&[0xFF, 0x51]);
        out.extend_from_slice(&41u16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes()); // Rsiz
        out.extend_from_slice(&4u32.to_be_bytes()); // Xsiz
        out.extend_from_slice(&4u32.to_be_bytes()); // Ysiz
        out.extend_from_slice(&0u32.to_be_bytes()); // XOsiz
        out.extend_from_slice(&0u32.to_be_bytes()); // YOsiz
        out.extend_from_slice(&4u32.to_be_bytes()); // XTsiz
        out.extend_from_slice(&4u32.to_be_bytes()); // YTsiz
        out.extend_from_slice(&0u32.to_be_bytes()); // XTOsiz
        out.extend_from_slice(&0u32.to_be_bytes()); // YTOsiz
        out.extend_from_slice(&1u16.to_be_bytes()); // Csiz
        out.push(0x07); // Ssiz
        out.push(0x01); // XRsiz
        out.push(0x01); // YRsiz
        // COD (Lcod = 12)
        out.extend_from_slice(&[0xFF, 0x52]);
        out.extend_from_slice(&12u16.to_be_bytes());
        out.push(0x00);
        out.push(0x00);
        out.extend_from_slice(&1u16.to_be_bytes());
        out.push(0x00);
        out.push(0x00);
        out.push(0x00);
        out.push(0x00);
        out.push(0x00);
        out.push(0x01);
        // QCD (Lqcd = 4)
        out.extend_from_slice(&[0xFF, 0x5C]);
        out.extend_from_slice(&4u16.to_be_bytes());
        out.push(0x00);
        out.push(0x00);
        // SOT
        out.extend_from_slice(&[0xFF, 0x90]);
        out.extend_from_slice(&10u16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&0u32.to_be_bytes());
        out.push(0x00);
        out.push(0x01);
        // SOD + EOC
        out.extend_from_slice(&[0xFF, 0x93]);
        out.extend_from_slice(&[0xFF, 0xD9]);
        out
    }

    /// Wrap a box body in a JP2 box (`length | type | body`).
    fn jp2_box(box_type: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&((body.len() + 8) as u32).to_be_bytes());
        b.extend_from_slice(box_type);
        b.extend_from_slice(body);
        b
    }

    /// Build a complete, spec-conformant JP2 with `ihdr` and `colr` correctly
    /// nested inside the `jp2h` superbox plus a `jp2c` codestream.
    fn create_full_jp2() -> Vec<u8> {
        let mut data = Vec::new();
        // Signature box
        data.extend_from_slice(&12u32.to_be_bytes());
        data.extend_from_slice(b"jP  ");
        data.extend_from_slice(&0x0D0A870Au32.to_be_bytes());
        // File Type box
        data.extend(jp2_box(b"ftyp", b"jp2 \x00\x00\x00\x00jp2 "));

        // ihdr (14 bytes): height, width, NC, BPC, C, UnkC, IPR
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&4u32.to_be_bytes()); // height
        ihdr.extend_from_slice(&4u32.to_be_bytes()); // width
        ihdr.extend_from_slice(&1u16.to_be_bytes()); // num components
        ihdr.push(0x07); // bits per component (8-bit unsigned)
        ihdr.push(0x07); // compression type = 7 (JPEG2000)
        ihdr.push(0x00); // colorspace unknown
        ihdr.push(0x00); // IPR
        // colr (enumerated, grayscale = 17)
        let mut colr = Vec::new();
        colr.push(0x01); // METH = enumerated
        colr.push(0x00); // PREC
        colr.push(0x00); // APPROX
        colr.extend_from_slice(&17u32.to_be_bytes());

        let mut jp2h_body = Vec::new();
        jp2h_body.extend(jp2_box(b"ihdr", &ihdr));
        jp2h_body.extend(jp2_box(b"colr", &colr));
        data.extend(jp2_box(b"jp2h", &jp2h_body));

        // jp2c codestream box
        data.extend(jp2_box(b"jp2c", &minimal_codestream_4x4_gray()));
        data
    }

    #[test]
    fn test_jp2_reader_decodes_nested_boxes() {
        // Regression: the parser must recurse into jp2h and locate the nested
        // ihdr/colr, so a real spec-conformant JP2 decodes with correct
        // dimensions instead of failing with "Missing image header box".
        let data = create_full_jp2();
        let mut reader = Jp2Reader::new(Cursor::new(data)).expect("reader creation");
        let image = reader.decode().expect("decode of nested-box JP2");
        assert_eq!(image.width, 4);
        assert_eq!(image.height, 4);
        assert_eq!(image.num_components, 1);
        assert_eq!(image.data.len(), 4 * 4);
    }

    #[test]
    fn test_parser_finds_nested_ihdr() {
        let data = create_full_jp2();
        let mut parser = Jp2Parser::new(Cursor::new(data)).expect("parser");
        parser.parse().expect("parse");
        // ihdr and colr live inside jp2h yet must be discoverable via find_box.
        assert!(parser.find_box(BoxType::Jp2Header).is_some());
        assert!(parser.find_box(BoxType::ImageHeader).is_some());
        assert!(parser.find_box(BoxType::ColorSpec).is_some());
        assert!(parser.find_box(BoxType::CodeStream).is_some());
        let ihdr = parser.read_image_header().expect("ihdr");
        assert_eq!(ihdr.width, 4);
        assert_eq!(ihdr.height, 4);
    }

    #[test]
    fn test_jpx_part2_box_rejected() {
        // A file carrying a Part-2 (JPX) box must fail with a typed error, not
        // be silently decoded as plain Part-1.
        let mut data = Vec::new();
        data.extend_from_slice(&12u32.to_be_bytes());
        data.extend_from_slice(b"jP  ");
        data.extend_from_slice(&0x0D0A870Au32.to_be_bytes());
        data.extend(jp2_box(b"ftyp", b"jpx \x00\x00\x00\x00jpx "));
        data.extend(jp2_box(b"jpch", &[0u8; 4])); // JPX codestream header
        let mut reader = Jp2Reader::new(Cursor::new(data)).expect("reader creation");
        assert!(reader.decode().is_err());
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
