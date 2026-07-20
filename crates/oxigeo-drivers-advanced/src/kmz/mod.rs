//! KMZ (Zipped KML) format driver.

use super::kml::{KmlDocument, read_kml, write_kml};
use crate::error::{Error, Result};
use oxiarc_archive::zip::{ZipCompressionLevel, ZipReader, ZipWriter};
use std::io::{Cursor, Read, Write};
use std::path::Path;

/// Read KMZ file.
pub fn read_kmz<R: Read + std::io::Seek>(reader: R) -> Result<KmzArchive> {
    let mut archive = ZipReader::new(reader)?;
    let mut documents = Vec::new();
    let mut images = Vec::new();

    let entries = archive.entries().to_vec();
    for entry in entries {
        let name = entry.name.clone();

        if name.ends_with(".kml") {
            let data = archive.extract(&entry)?;
            let content = String::from_utf8(data)?;
            let doc = read_kml(Cursor::new(content.as_bytes()))?;
            documents.push((name, doc));
        } else if is_image_file(&name) {
            let data = archive.extract(&entry)?;
            images.push((name, data));
        }
    }

    if documents.is_empty() {
        return Err(Error::kmz("No KML files found in KMZ archive"));
    }

    Ok(KmzArchive { documents, images })
}

/// Write KMZ file.
pub fn write_kmz<W: Write>(
    writer: W,
    doc: &KmlDocument,
    images: &[(String, Vec<u8>)],
) -> Result<()> {
    let mut zip = ZipWriter::new(writer);
    zip.set_compression(ZipCompressionLevel::Normal);

    // Write main KML file
    let mut kml_buf = Vec::new();
    write_kml(&mut kml_buf, doc)?;
    zip.add_file("doc.kml", &kml_buf)?;

    // Write images
    for (name, data) in images {
        zip.add_file(name, data)?;
    }

    zip.finish()?;
    Ok(())
}

/// Read KMZ from file path.
pub fn read_kmz_file<P: AsRef<Path>>(path: P) -> Result<KmzArchive> {
    let file = std::fs::File::open(path)?;
    read_kmz(file)
}

/// Write KMZ to file path.
pub fn write_kmz_file<P: AsRef<Path>>(
    path: P,
    doc: &KmlDocument,
    images: &[(String, Vec<u8>)],
) -> Result<()> {
    let file = std::fs::File::create(path)?;
    write_kmz(file, doc, images)
}

/// KMZ archive contents.
#[derive(Debug, Clone)]
pub struct KmzArchive {
    /// KML documents (filename, document)
    pub documents: Vec<(String, KmlDocument)>,
    /// Images (filename, data)
    pub images: Vec<(String, Vec<u8>)>,
}

impl KmzArchive {
    /// Create new empty archive.
    pub fn new() -> Self {
        Self {
            documents: Vec::new(),
            images: Vec::new(),
        }
    }

    /// Add KML document.
    pub fn add_document<S: Into<String>>(&mut self, name: S, doc: KmlDocument) {
        self.documents.push((name.into(), doc));
    }

    /// Add image.
    pub fn add_image<S: Into<String>>(&mut self, name: S, data: Vec<u8>) {
        self.images.push((name.into(), data));
    }

    /// Get main document (first KML file).
    pub fn main_document(&self) -> Option<&KmlDocument> {
        self.documents.first().map(|(_, doc)| doc)
    }

    /// Get document by name.
    pub fn get_document(&self, name: &str) -> Option<&KmlDocument> {
        self.documents
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, doc)| doc)
    }

    /// Get image by name.
    pub fn get_image(&self, name: &str) -> Option<&[u8]> {
        self.images
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, data)| data.as_slice())
    }

    /// Get document count.
    pub fn document_count(&self) -> usize {
        self.documents.len()
    }

    /// Get image count.
    pub fn image_count(&self) -> usize {
        self.images.len()
    }
}

impl Default for KmzArchive {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if file is an image.
fn is_image_file(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".gif")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_is_image_file() {
        assert!(is_image_file("test.png"));
        assert!(is_image_file("test.PNG"));
        assert!(is_image_file("test.jpg"));
        assert!(is_image_file("test.jpeg"));
        assert!(!is_image_file("test.kml"));
        assert!(!is_image_file("test.txt"));
    }

    #[test]
    fn test_kmz_archive_creation() {
        let mut archive = KmzArchive::new();
        assert_eq!(archive.document_count(), 0);
        assert_eq!(archive.image_count(), 0);

        let doc = KmlDocument::new();
        archive.add_document("doc.kml", doc);
        assert_eq!(archive.document_count(), 1);

        archive.add_image("icon.png", vec![0, 1, 2, 3]);
        assert_eq!(archive.image_count(), 1);
    }

    #[test]
    fn test_kmz_write_read_roundtrip() -> Result<()> {
        let doc = KmlDocument::new().with_name("Test KMZ");
        let images = vec![("test.png".to_string(), vec![0u8; 100])];

        let mut buffer = Cursor::new(Vec::new());
        write_kmz(&mut buffer, &doc, &images)?;

        buffer.set_position(0);
        let archive = read_kmz(buffer)?;

        assert!(archive.document_count() >= 1);
        assert_eq!(archive.image_count(), 1);

        Ok(())
    }
}
