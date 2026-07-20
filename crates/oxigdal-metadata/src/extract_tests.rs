use super::*;

#[test]
fn test_extractor_builder() {
    let extractor = MetadataExtractor::new()
        .with_spatial(true)
        .with_temporal(false)
        .with_max_keywords(10);

    assert!(extractor.extract_spatial);
    assert!(!extractor.extract_temporal);
    assert_eq!(extractor.max_keywords, 10);
}

#[test]
fn test_extracted_metadata_default() {
    let metadata = ExtractedMetadata::default();
    assert!(metadata.title.is_none());
    assert!(metadata.bbox.is_none());
    assert!(metadata.keywords.is_empty());
}

#[test]
fn test_unsupported_extension() {
    let path = std::env::temp_dir().join("oxigdal_nonexistent_test_bx9f.xyz");
    let result = extract_metadata(path);
    assert!(result.is_err());
}

#[test]
fn test_no_extension() {
    let path = std::env::temp_dir().join("oxigdal_nonexistent_somefile_bx9f");
    let result = extract_metadata(path);
    assert!(result.is_err());
}

#[test]
fn test_geotiff_extraction_with_real_tiff() {
    // Create a minimal valid TIFF file with GeoTIFF tags
    let tiff_bytes = build_test_geotiff(
        256,
        256,
        8,
        1,
        Some(&[0.0, 0.0, 0.0, -120.0, 40.0, 0.0]), // tiepoint
        Some(&[0.01, 0.01, 0.0]),                  // pixel scale
        Some(&[
            1, 1, 0, 3, // GeoKey directory header: version=1, revision=1, minor=0, numKeys=3
            1024, 0, 1, 1, // GTModelTypeGeoKey = ModelTypeProjected
            1025, 0, 1, 1, // GTRasterTypeGeoKey = RasterPixelIsArea
            2048, 0, 1, 4326, // GeographicTypeGeoKey = EPSG:4326
        ]),
    );

    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let path = dir.path().join("test.tif");
    std::fs::write(&path, &tiff_bytes).expect("failed to write test TIFF");

    let metadata = extract_from_geotiff(&path).expect("extraction should succeed");
    assert_eq!(metadata.format.as_deref(), Some("GeoTIFF"));
    assert!(metadata.title.is_some());

    // Check CRS from GeoKeys
    assert_eq!(metadata.crs.as_deref(), Some("EPSG:4326"));

    // Check bounding box computed from tiepoint + pixel scale + dimensions
    let bbox = metadata.bbox.expect("should have bbox");
    assert!((bbox.west - (-120.0)).abs() < 1e-6);
    assert!((bbox.north - 40.0).abs() < 1e-6);
    assert!((bbox.east - (-120.0 + 0.01 * 256.0)).abs() < 1e-6);
    assert!((bbox.south - (40.0 - 0.01 * 256.0)).abs() < 1e-6);

    // Check spatial resolution
    assert!((metadata.spatial_resolution.expect("should have resolution") - 0.01).abs() < 1e-10);

    // Check attributes
    assert_eq!(
        metadata.attributes.get("width").map(|s| s.as_str()),
        Some("256")
    );
    assert_eq!(
        metadata.attributes.get("height").map(|s| s.as_str()),
        Some("256")
    );
    assert_eq!(
        metadata
            .attributes
            .get("bits_per_sample")
            .map(|s| s.as_str()),
        Some("8")
    );
    assert_eq!(
        metadata.attributes.get("compression").map(|s| s.as_str()),
        Some("None")
    );
}

#[test]
fn test_geotiff_extraction_projected_crs() {
    let tiff_bytes = build_test_geotiff(
        512,
        512,
        16,
        1,
        Some(&[0.0, 0.0, 0.0, 500000.0, 4500000.0, 0.0]),
        Some(&[10.0, 10.0, 0.0]),
        Some(&[
            1, 1, 0, 2, 1024, 0, 1, 1, // ModelTypeProjected
            3072, 0, 1, 32632, // ProjectedCSTypeGeoKey = EPSG:32632 (UTM 32N)
        ]),
    );

    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let path = dir.path().join("projected.tif");
    std::fs::write(&path, &tiff_bytes).expect("failed to write test TIFF");

    let metadata = extract_from_geotiff(&path).expect("extraction should succeed");
    assert_eq!(metadata.crs.as_deref(), Some("EPSG:32632"));
    assert_eq!(
        metadata
            .attributes
            .get("bits_per_sample")
            .map(|s| s.as_str()),
        Some("16")
    );
}

#[test]
fn test_geotiff_extraction_no_geokeys() {
    let tiff_bytes = build_test_geotiff(100, 100, 8, 3, None, None, None);

    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let path = dir.path().join("plain.tif");
    std::fs::write(&path, &tiff_bytes).expect("failed to write test TIFF");

    let metadata = extract_from_geotiff(&path).expect("extraction should succeed");
    assert_eq!(metadata.format.as_deref(), Some("GeoTIFF"));
    assert!(metadata.crs.is_none());
    assert!(metadata.bbox.is_none());
    assert_eq!(
        metadata
            .attributes
            .get("samples_per_pixel")
            .map(|s| s.as_str()),
        Some("3")
    );
}

#[test]
fn test_geotiff_not_a_tiff() {
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let path = dir.path().join("bad.tif");
    std::fs::write(&path, b"not a tiff file").expect("failed to write");

    let result = extract_from_geotiff(&path);
    assert!(result.is_err());
}

#[test]
fn test_geotiff_too_small() {
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let path = dir.path().join("tiny.tif");
    std::fs::write(&path, b"II").expect("failed to write");

    let result = extract_from_geotiff(&path);
    assert!(result.is_err());
}

#[test]
fn test_geokeys_user_defined_projected() {
    let keys = vec![
        1u16, 1, 0, 1, 1024, 0, 1, 1, // ModelTypeProjected = user-defined
    ];
    let crs = parse_crs_from_geokeys(&keys, &None);
    assert_eq!(crs.as_deref(), Some("Projected CRS (user-defined)"));
}

#[test]
fn test_geokeys_user_defined_geographic() {
    let keys = vec![
        1u16, 1, 0, 1, 1024, 0, 1, 2, // ModelTypeGeographic
    ];
    let crs = parse_crs_from_geokeys(&keys, &None);
    assert_eq!(crs.as_deref(), Some("Geographic CRS (user-defined)"));
}

#[test]
fn test_geokeys_empty() {
    let crs = parse_crs_from_geokeys(&[], &None);
    assert!(crs.is_none());
}

#[test]
fn test_to_iso19115_conversion() {
    let metadata = ExtractedMetadata {
        title: Some("Test Dataset".to_string()),
        abstract_text: Some("A test dataset".to_string()),
        bbox: Some(BoundingBox::new(-180.0, -90.0, 180.0, 90.0)),
        crs: Some("EPSG:4326".to_string()),
        keywords: vec!["geospatial".to_string(), "test".to_string()],
        ..Default::default()
    };

    let iso = to_iso19115(&metadata).expect("conversion should succeed");
    assert_eq!(iso.identification_info.len(), 1);
    assert_eq!(iso.identification_info[0].citation.title, "Test Dataset");
    assert_eq!(iso.reference_system_info.len(), 1);
}

#[test]
fn test_to_fgdc_conversion() {
    let metadata = ExtractedMetadata {
        title: Some("FGDC Test".to_string()),
        abstract_text: Some("FGDC test dataset".to_string()),
        bbox: Some(BoundingBox::new(-100.0, 30.0, -90.0, 40.0)),
        ..Default::default()
    };

    let fgdc = to_fgdc(&metadata).expect("conversion should succeed");
    assert_eq!(fgdc.idinfo.citation.citeinfo.title, "FGDC Test");
}

#[test]
fn test_batch_extract_mixed() {
    let results = batch_extract(vec![
        std::path::PathBuf::from("/nonexistent/file.tif"),
        std::path::PathBuf::from("/nonexistent/file.xyz"),
    ]);
    assert_eq!(results.len(), 2);
    assert!(results[0].is_err()); // file doesn't exist
    assert!(results[1].is_err()); // unsupported format
}

#[test]
fn test_extractor_filters_spatial() {
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let path = dir.path().join("filter_test.tif");

    let tiff_bytes = build_test_geotiff(
        64,
        64,
        8,
        1,
        Some(&[0.0, 0.0, 0.0, 10.0, 50.0, 0.0]),
        Some(&[0.1, 0.1, 0.0]),
        Some(&[1, 1, 0, 1, 2048, 0, 1, 4326]),
    );
    std::fs::write(&path, &tiff_bytes).expect("failed to write test TIFF");

    let extractor = MetadataExtractor::new().with_spatial(false);
    let metadata = extractor.extract(&path).expect("extraction should succeed");
    assert!(metadata.bbox.is_none());
    assert!(metadata.crs.is_none());
    assert!(metadata.spatial_resolution.is_none());
}

/// Build a minimal valid TIFF file with optional GeoTIFF tags for testing.
fn build_test_geotiff(
    width: u32,
    height: u32,
    bits_per_sample: u16,
    samples_per_pixel: u16,
    tiepoint: Option<&[f64]>,
    pixel_scale: Option<&[f64]>,
    geokeys: Option<&[u16]>,
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(4096);

    // TIFF header: little-endian, magic 42
    buf.extend_from_slice(b"II");
    buf.extend_from_slice(&42u16.to_le_bytes());

    // IFD offset at byte 8
    let ifd_offset = 8u32;
    buf.extend_from_slice(&ifd_offset.to_le_bytes());

    // Count how many IFD entries we need
    let mut entry_count: u16 = 5; // width, height, bits_per_sample, compression, samples_per_pixel
    if tiepoint.is_some() {
        entry_count += 1;
    }
    if pixel_scale.is_some() {
        entry_count += 1;
    }
    if geokeys.is_some() {
        entry_count += 1;
    }

    // IFD entry count
    buf.extend_from_slice(&entry_count.to_le_bytes());

    // Helper: write an IFD entry (tag, type, count, value/offset)
    let write_entry_long = |buf: &mut Vec<u8>, tag: u16, value: u32| {
        buf.extend_from_slice(&tag.to_le_bytes());
        buf.extend_from_slice(&3u16.to_le_bytes()); // SHORT type (for width/height we use LONG)
        buf.extend_from_slice(&1u32.to_le_bytes()); // count
        // For LONG values that should be tag type 4:
        // Actually let's use SHORT for small values, LONG for larger
        if value <= u32::from(u16::MAX) {
            buf.extend_from_slice(&(value as u16).to_le_bytes());
            buf.extend_from_slice(&0u16.to_le_bytes()); // pad
        } else {
            // Re-write type as LONG
            let type_off = buf.len() - 8;
            buf[type_off] = 4; // LONG type
            buf[type_off + 1] = 0;
            buf.extend_from_slice(&value.to_le_bytes());
        }
    };

    let write_entry_short = |buf: &mut Vec<u8>, tag: u16, value: u16| {
        buf.extend_from_slice(&tag.to_le_bytes());
        buf.extend_from_slice(&3u16.to_le_bytes()); // SHORT type
        buf.extend_from_slice(&1u32.to_le_bytes()); // count
        buf.extend_from_slice(&value.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes()); // pad
    };

    // IFD entries (must be sorted by tag)
    write_entry_long(&mut buf, 256, width); // ImageWidth
    write_entry_long(&mut buf, 257, height); // ImageLength
    write_entry_short(&mut buf, 258, bits_per_sample); // BitsPerSample
    write_entry_short(&mut buf, 259, 1); // Compression = None
    write_entry_short(&mut buf, 277, samples_per_pixel); // SamplesPerPixel

    // Compute offset for external data (after all IFD entries + next IFD offset)
    let entries_so_far = 5;
    let remaining_entries = entry_count - entries_so_far as u16;
    let _ifd_end_offset = buf.len() + (remaining_entries as usize * 12) + 4; // +4 for next IFD pointer

    // We need to plan offsets for data sections that come after the IFD
    struct DeferredData {
        entry_offset: usize, // where the offset field is in the IFD entry
        data: Vec<u8>,
    }
    let mut deferred: Vec<DeferredData> = Vec::new();

    // Write GeoTIFF tag entries with placeholder offsets
    if let Some(ps) = pixel_scale {
        let entry_offset = buf.len() + 8; // offset of the value/offset field
        buf.extend_from_slice(&33550u16.to_le_bytes()); // ModelPixelScaleTag
        buf.extend_from_slice(&12u16.to_le_bytes()); // DOUBLE type
        buf.extend_from_slice(&(ps.len() as u32).to_le_bytes()); // count
        buf.extend_from_slice(&0u32.to_le_bytes()); // placeholder offset
        let data: Vec<u8> = ps.iter().flat_map(|v| v.to_le_bytes()).collect();
        deferred.push(DeferredData { entry_offset, data });
    }

    if let Some(tp) = tiepoint {
        let entry_offset = buf.len() + 8;
        buf.extend_from_slice(&33922u16.to_le_bytes()); // ModelTiepointTag
        buf.extend_from_slice(&12u16.to_le_bytes()); // DOUBLE type
        buf.extend_from_slice(&(tp.len() as u32).to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        let data: Vec<u8> = tp.iter().flat_map(|v| v.to_le_bytes()).collect();
        deferred.push(DeferredData { entry_offset, data });
    }

    if let Some(gk) = geokeys {
        let entry_offset = buf.len() + 8;
        buf.extend_from_slice(&34735u16.to_le_bytes()); // GeoKeyDirectoryTag
        buf.extend_from_slice(&3u16.to_le_bytes()); // SHORT type
        buf.extend_from_slice(&(gk.len() as u32).to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        let data: Vec<u8> = gk.iter().flat_map(|v| v.to_le_bytes()).collect();
        deferred.push(DeferredData { entry_offset, data });
    }

    // Next IFD offset = 0 (no more IFDs)
    buf.extend_from_slice(&0u32.to_le_bytes());

    // Now write deferred data and patch offsets
    for def in &deferred {
        let data_offset = buf.len() as u32;
        // Patch the offset in the IFD entry
        buf[def.entry_offset..def.entry_offset + 4].copy_from_slice(&data_offset.to_le_bytes());
        buf.extend_from_slice(&def.data);
    }

    buf
}

/// Build a minimal synthetic BigTIFF (version 43) file with GeoTIFF tags,
/// matching the classic-TIFF layout produced by `build_test_geotiff` but
/// using BigTIFF's wide fields: an 8-byte first-IFD offset, an 8-byte IFD
/// entry count, and 20-byte entries (tag2 + type2 + count8 + value8).
///
/// `little_endian` selects "II" (Intel) vs "MM" (Motorola) byte order so
/// both orderings can be regression-tested.
#[allow(clippy::too_many_arguments)]
fn build_test_bigtiff(
    little_endian: bool,
    width: u32,
    height: u32,
    bits_per_sample: u16,
    samples_per_pixel: u16,
    tiepoint: Option<&[f64]>,
    pixel_scale: Option<&[f64]>,
    geokeys: Option<&[u16]>,
) -> Vec<u8> {
    // Endianness-aware byte-writer helpers, so the same construction
    // logic can emit either "II" or "MM" BigTIFF files.
    let w16 = |v: u16| -> [u8; 2] {
        if little_endian {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    };
    let w32 = |v: u32| -> [u8; 4] {
        if little_endian {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    };
    let w64 = |v: u64| -> [u8; 8] {
        if little_endian {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    };
    let wf64 = |v: f64| -> [u8; 8] {
        if little_endian {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    };

    let mut buf = Vec::with_capacity(4096);

    // BigTIFF header: byte-order mark, version=43, offset-bytesize=8,
    // reserved=0, then the 8-byte offset to the first IFD.
    buf.extend_from_slice(if little_endian { b"II" } else { b"MM" });
    buf.extend_from_slice(&w16(43));
    buf.extend_from_slice(&w16(8)); // bytesize of offsets (always 8)
    buf.extend_from_slice(&w16(0)); // reserved
    let ifd_offset: u64 = 16;
    buf.extend_from_slice(&w64(ifd_offset));

    let mut entry_count: u64 = 5; // width, height, bits_per_sample, compression, samples_per_pixel
    if tiepoint.is_some() {
        entry_count += 1;
    }
    if pixel_scale.is_some() {
        entry_count += 1;
    }
    if geokeys.is_some() {
        entry_count += 1;
    }
    buf.extend_from_slice(&w64(entry_count));

    // Write a BigTIFF IFD entry with an inline LONG (4-byte) value,
    // padded to the full 8-byte value/offset field.
    let write_entry_long = |buf: &mut Vec<u8>, tag: u16, value: u32| {
        buf.extend_from_slice(&w16(tag));
        buf.extend_from_slice(&w16(4)); // LONG type
        buf.extend_from_slice(&w64(1)); // count
        buf.extend_from_slice(&w32(value));
        buf.extend_from_slice(&[0u8; 4]); // pad value field to 8 bytes
    };

    // Write a BigTIFF IFD entry with an inline SHORT (2-byte) value,
    // padded to the full 8-byte value/offset field.
    let write_entry_short = |buf: &mut Vec<u8>, tag: u16, value: u16| {
        buf.extend_from_slice(&w16(tag));
        buf.extend_from_slice(&w16(3)); // SHORT type
        buf.extend_from_slice(&w64(1)); // count
        buf.extend_from_slice(&w16(value));
        buf.extend_from_slice(&[0u8; 6]); // pad value field to 8 bytes
    };

    // IFD entries (tag order doesn't matter for this parser).
    write_entry_long(&mut buf, 256, width); // ImageWidth
    write_entry_long(&mut buf, 257, height); // ImageLength
    write_entry_short(&mut buf, 258, bits_per_sample); // BitsPerSample
    write_entry_short(&mut buf, 259, 1); // Compression = None
    write_entry_short(&mut buf, 277, samples_per_pixel); // SamplesPerPixel

    struct DeferredData {
        entry_offset: usize, // position of the 8-byte value/offset field
        data: Vec<u8>,
    }
    let mut deferred: Vec<DeferredData> = Vec::new();

    if let Some(ps) = pixel_scale {
        let entry_offset = buf.len() + 12; // tag(2)+type(2)+count(8)
        buf.extend_from_slice(&w16(33550)); // ModelPixelScaleTag
        buf.extend_from_slice(&w16(12)); // DOUBLE type
        buf.extend_from_slice(&w64(ps.len() as u64));
        buf.extend_from_slice(&[0u8; 8]); // placeholder offset
        let data: Vec<u8> = ps.iter().flat_map(|v| wf64(*v)).collect();
        deferred.push(DeferredData { entry_offset, data });
    }

    if let Some(tp) = tiepoint {
        let entry_offset = buf.len() + 12;
        buf.extend_from_slice(&w16(33922)); // ModelTiepointTag
        buf.extend_from_slice(&w16(12)); // DOUBLE type
        buf.extend_from_slice(&w64(tp.len() as u64));
        buf.extend_from_slice(&[0u8; 8]);
        let data: Vec<u8> = tp.iter().flat_map(|v| wf64(*v)).collect();
        deferred.push(DeferredData { entry_offset, data });
    }

    if let Some(gk) = geokeys {
        let entry_offset = buf.len() + 12;
        buf.extend_from_slice(&w16(34735)); // GeoKeyDirectoryTag
        buf.extend_from_slice(&w16(3)); // SHORT type
        buf.extend_from_slice(&w64(gk.len() as u64));
        buf.extend_from_slice(&[0u8; 8]);
        let data: Vec<u8> = gk.iter().flat_map(|v| w16(*v)).collect();
        deferred.push(DeferredData { entry_offset, data });
    }

    // Next IFD offset = 0 (no more IFDs), 8 bytes wide in BigTIFF.
    buf.extend_from_slice(&w64(0));

    // Write deferred data and patch each entry's offset field (8 bytes).
    for def in &deferred {
        let data_offset = buf.len() as u64;
        buf[def.entry_offset..def.entry_offset + 8].copy_from_slice(&w64(data_offset));
        buf.extend_from_slice(&def.data);
    }

    buf
}

#[test]
fn test_bigtiff_extraction_little_endian() {
    let tiff_bytes = build_test_bigtiff(
        true,
        256,
        256,
        8,
        1,
        Some(&[0.0, 0.0, 0.0, -120.0, 40.0, 0.0]), // tiepoint
        Some(&[0.01, 0.01, 0.0]),                  // pixel scale
        Some(&[
            1, 1, 0, 3, // GeoKey directory header: version=1, revision=1, minor=0, numKeys=3
            1024, 0, 1, 1, // GTModelTypeGeoKey = ModelTypeProjected
            1025, 0, 1, 1, // GTRasterTypeGeoKey = RasterPixelIsArea
            2048, 0, 1, 4326, // GeographicTypeGeoKey = EPSG:4326
        ]),
    );

    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let path = dir.path().join("bigtiff_le.tif");
    std::fs::write(&path, &tiff_bytes).expect("failed to write test BigTIFF");

    let metadata = extract_from_geotiff(&path).expect("extraction should succeed");
    assert_eq!(metadata.format.as_deref(), Some("GeoTIFF"));
    assert_eq!(
        metadata.attributes.get("bigtiff").map(|s| s.as_str()),
        Some("true")
    );

    assert_eq!(
        metadata.attributes.get("width").map(|s| s.as_str()),
        Some("256")
    );
    assert_eq!(
        metadata.attributes.get("height").map(|s| s.as_str()),
        Some("256")
    );
    assert_eq!(
        metadata
            .attributes
            .get("bits_per_sample")
            .map(|s| s.as_str()),
        Some("8")
    );
    assert_eq!(
        metadata.attributes.get("compression").map(|s| s.as_str()),
        Some("None")
    );

    assert_eq!(metadata.crs.as_deref(), Some("EPSG:4326"));

    let bbox = metadata
        .bbox
        .expect("should have bbox parsed from BigTIFF ModelTiepoint/ModelPixelScale");
    assert!((bbox.west - (-120.0)).abs() < 1e-6);
    assert!((bbox.north - 40.0).abs() < 1e-6);
    assert!((bbox.east - (-120.0 + 0.01 * 256.0)).abs() < 1e-6);
    assert!((bbox.south - (40.0 - 0.01 * 256.0)).abs() < 1e-6);

    assert!((metadata.spatial_resolution.expect("should have resolution") - 0.01).abs() < 1e-10);
}

#[test]
fn test_bigtiff_extraction_big_endian() {
    let tiff_bytes = build_test_bigtiff(
        false,
        512,
        512,
        16,
        1,
        Some(&[0.0, 0.0, 0.0, 500000.0, 4500000.0, 0.0]),
        Some(&[10.0, 10.0, 0.0]),
        Some(&[
            1, 1, 0, 2, 1024, 0, 1, 1, // ModelTypeProjected
            3072, 0, 1, 32632, // ProjectedCSTypeGeoKey = EPSG:32632 (UTM 32N)
        ]),
    );

    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let path = dir.path().join("bigtiff_be.tif");
    std::fs::write(&path, &tiff_bytes).expect("failed to write test BigTIFF");

    let metadata = extract_from_geotiff(&path).expect("extraction should succeed");
    assert_eq!(
        metadata.attributes.get("bigtiff").map(|s| s.as_str()),
        Some("true")
    );
    assert_eq!(metadata.crs.as_deref(), Some("EPSG:32632"));
    assert_eq!(
        metadata
            .attributes
            .get("bits_per_sample")
            .map(|s| s.as_str()),
        Some("16")
    );
    assert_eq!(
        metadata.attributes.get("width").map(|s| s.as_str()),
        Some("512")
    );
    assert_eq!(
        metadata.attributes.get("height").map(|s| s.as_str()),
        Some("512")
    );

    let bbox = metadata
        .bbox
        .expect("should have bbox parsed from big-endian BigTIFF");
    assert!((bbox.west - 500000.0).abs() < 1e-6);
    assert!((bbox.north - 4500000.0).abs() < 1e-6);
}

#[test]
fn test_bigtiff_extraction_no_geokeys() {
    let tiff_bytes = build_test_bigtiff(true, 100, 100, 8, 3, None, None, None);

    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let path = dir.path().join("bigtiff_plain.tif");
    std::fs::write(&path, &tiff_bytes).expect("failed to write test BigTIFF");

    let metadata = extract_from_geotiff(&path).expect("extraction should succeed");
    assert_eq!(
        metadata.attributes.get("bigtiff").map(|s| s.as_str()),
        Some("true")
    );
    assert!(metadata.crs.is_none());
    assert!(metadata.bbox.is_none());
    assert_eq!(
        metadata
            .attributes
            .get("samples_per_pixel")
            .map(|s| s.as_str()),
        Some("3")
    );
}

#[cfg(feature = "stac")]
#[test]
fn test_stac_extraction_full_item() {
    let json = r#"{
            "type": "Feature",
            "stac_version": "1.0.0",
            "stac_extensions": [
                "https://stac-extensions.github.io/projection/v1.1.0/schema.json"
            ],
            "id": "test-item-001",
            "geometry": {
                "type": "Polygon",
                "coordinates": [[[-122.5, 37.5], [-122.0, 37.5], [-122.0, 38.0], [-122.5, 38.0], [-122.5, 37.5]]]
            },
            "bbox": [-122.5, 37.5, -122.0, 38.0],
            "properties": {
                "datetime": "2024-01-15T10:30:00Z",
                "title": "San Francisco Bay Area",
                "description": "Sentinel-2 L2A imagery over SF Bay",
                "proj:epsg": 32610,
                "gsd": 10.0,
                "platform": "sentinel-2a",
                "constellation": "sentinel-2",
                "keywords": ["sentinel", "optical", "bay-area"]
            },
            "links": [],
            "assets": {
                "visual": {
                    "href": "https://example.com/visual.tif",
                    "type": "image/tiff; application=geotiff"
                },
                "thumbnail": {
                    "href": "https://example.com/thumb.png",
                    "type": "image/png"
                }
            },
            "collection": "sentinel-2-l2a"
        }"#;

    let metadata = extract_from_stac_json(json).expect("extraction should succeed");

    assert_eq!(metadata.title.as_deref(), Some("San Francisco Bay Area"));
    assert_eq!(
        metadata.abstract_text.as_deref(),
        Some("Sentinel-2 L2A imagery over SF Bay")
    );
    assert_eq!(metadata.format.as_deref(), Some("STAC"));
    assert_eq!(metadata.crs.as_deref(), Some("EPSG:32610"));
    assert!((metadata.spatial_resolution.expect("should have gsd") - 10.0).abs() < 1e-10);

    let bbox = metadata.bbox.expect("should have bbox");
    assert!((bbox.west - (-122.5)).abs() < 1e-6);
    assert!((bbox.south - 37.5).abs() < 1e-6);
    assert!((bbox.east - (-122.0)).abs() < 1e-6);
    assert!((bbox.north - 38.0).abs() < 1e-6);

    assert!(
        metadata
            .temporal_extent
            .as_ref()
            .expect("should have temporal")
            .start
            .is_some()
    );

    assert_eq!(metadata.keywords, vec!["sentinel", "optical", "bay-area"]);
    assert_eq!(
        metadata.attributes.get("id").map(|s| s.as_str()),
        Some("test-item-001")
    );
    assert_eq!(
        metadata.attributes.get("collection").map(|s| s.as_str()),
        Some("sentinel-2-l2a")
    );
    assert_eq!(
        metadata.attributes.get("platform").map(|s| s.as_str()),
        Some("sentinel-2a")
    );
    assert!(metadata.attributes.contains_key("asset_keys"));
}

#[cfg(feature = "stac")]
#[test]
fn test_stac_extraction_minimal_item() {
    let json = r#"{
            "type": "Feature",
            "stac_version": "1.0.0",
            "id": "minimal-item",
            "geometry": null,
            "bbox": null,
            "properties": {
                "datetime": null
            },
            "links": [],
            "assets": {}
        }"#;

    let metadata = extract_from_stac_json(json).expect("extraction should succeed");

    assert!(metadata.title.is_none());
    assert!(metadata.abstract_text.is_none());
    assert!(metadata.bbox.is_none());
    assert!(metadata.temporal_extent.is_none());
    assert!(metadata.crs.is_none());
    assert_eq!(metadata.format.as_deref(), Some("STAC"));
    assert_eq!(
        metadata.attributes.get("id").map(|s| s.as_str()),
        Some("minimal-item")
    );
}

#[cfg(feature = "stac")]
#[test]
fn test_stac_extraction_with_temporal_range() {
    let json = r#"{
            "type": "Feature",
            "stac_version": "1.0.0",
            "id": "temporal-range",
            "geometry": null,
            "bbox": null,
            "properties": {
                "datetime": null,
                "start_datetime": "2024-01-01T00:00:00Z",
                "end_datetime": "2024-01-31T23:59:59Z"
            },
            "links": [],
            "assets": {}
        }"#;

    let metadata = extract_from_stac_json(json).expect("extraction should succeed");
    let extent = metadata
        .temporal_extent
        .expect("should have temporal extent");
    assert!(extent.start.is_some());
    assert!(extent.end.is_some());
}

/// A `proj:wkt2` string with a multi-byte UTF-8 character straddling the
/// 64-character truncation boundary must not panic on a non-char-boundary
/// byte slice.
#[cfg(feature = "stac")]
#[test]
fn test_stac_extraction_wkt2_multibyte_truncation_no_panic() {
    // 63 ASCII chars followed by a 2-byte UTF-8 character ('°', U+00B0)
    // straddling byte offset 64, then more content past the cutoff.
    let prefix = "A".repeat(63);
    let wkt2 = format!("{prefix}°REMAINDER_OF_WKT2_STRING_THAT_SHOULD_BE_TRUNCATED_AWAY");

    let json = format!(
        r#"{{
                "type": "Feature",
                "stac_version": "1.0.0",
                "id": "wkt2-multibyte-test",
                "geometry": null,
                "bbox": null,
                "properties": {{
                    "datetime": null,
                    "proj:wkt2": "{wkt2}"
                }},
                "links": [],
                "assets": {{}}
            }}"#
    );

    let metadata = extract_from_stac_json(&json).expect("extraction should not panic");
    let crs = metadata
        .crs
        .expect("crs should be populated from proj:wkt2");
    assert!(crs.starts_with("WKT2:"));
    // Exactly 64 characters were kept (not 64 bytes).
    assert_eq!(crs.trim_start_matches("WKT2:").chars().count(), 64);
}

#[cfg(feature = "stac")]
#[test]
fn test_stac_extraction_invalid_json() {
    let result = extract_from_stac_json("not valid json");
    assert!(result.is_err());
}

#[cfg(feature = "stac")]
#[test]
fn test_stac_extraction_from_file() {
    let json = r#"{
            "type": "Feature",
            "stac_version": "1.0.0",
            "id": "file-test",
            "geometry": null,
            "bbox": [-10.0, -20.0, 10.0, 20.0],
            "properties": {
                "datetime": "2024-06-01T12:00:00Z",
                "title": "File Test Item"
            },
            "links": [],
            "assets": {}
        }"#;

    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let path = dir.path().join("item.json");
    std::fs::write(&path, json).expect("failed to write");

    let metadata = extract_metadata(&path).expect("extraction should succeed");
    assert_eq!(metadata.title.as_deref(), Some("File Test Item"));
    assert_eq!(metadata.format.as_deref(), Some("STAC"));
    let bbox = metadata.bbox.expect("should have bbox");
    assert!((bbox.west - (-10.0)).abs() < 1e-6);
}

// ── NetCDF / STAC feature-gating tests ─────────────────────────────────

/// Without the `netcdf` feature, extract_from_netcdf should return
/// Unsupported rather than a hollow "successful" result, mirroring the
/// hdf5 behavior below.
#[cfg(not(feature = "netcdf"))]
#[test]
fn test_netcdf_without_feature_returns_unsupported() {
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let path = dir.path().join("dummy.nc");
    let result = extract_from_netcdf(&path);
    assert!(result.is_err());
    match result.unwrap_err() {
        MetadataError::Unsupported(_) => {}
        other => panic!("expected Unsupported, got {:?}", other),
    }
}

/// The `extract_metadata` dispatcher should surface the same Unsupported
/// error for `.nc` files when the `netcdf` feature is off.
#[cfg(not(feature = "netcdf"))]
#[test]
fn test_extract_metadata_nc_extension_unsupported_without_feature() {
    let path = std::env::temp_dir().join("oxigdal_nc_test_bx9f.nc");
    let result = extract_metadata(&path);
    assert!(result.is_err());
    match result.unwrap_err() {
        MetadataError::Unsupported(_) => {}
        other => panic!("expected Unsupported, got {:?}", other),
    }
}

/// Without the `stac` feature, extract_from_stac should return
/// Unsupported rather than a hollow "successful" result.
#[cfg(not(feature = "stac"))]
#[test]
fn test_stac_without_feature_returns_unsupported() {
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let path = dir.path().join("dummy.json");
    let result = extract_from_stac(&path);
    assert!(result.is_err());
    match result.unwrap_err() {
        MetadataError::Unsupported(_) => {}
        other => panic!("expected Unsupported, got {:?}", other),
    }
}

// ── HDF5 tests ──────────────────────────────────────────────────────────

/// Without the `hdf5` feature, extract_from_hdf5 should return Unsupported.
#[cfg(not(feature = "hdf5"))]
#[test]
fn test_hdf5_without_feature_returns_unsupported() {
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let path = dir.path().join("dummy.h5");
    // File does not need to exist — error is feature-level before any I/O.
    let result = extract_from_hdf5(&path);
    assert!(result.is_err());
    match result.unwrap_err() {
        MetadataError::Unsupported(_) => {}
        other => panic!("expected Unsupported, got {:?}", other),
    }
}

/// Non-HDF5 file content should produce an ExtractionError when the `hdf5`
/// feature is enabled.
#[cfg(feature = "hdf5")]
#[test]
fn test_hdf5_extraction_invalid_content() {
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let path = dir.path().join("not_hdf5.h5");
    std::fs::write(&path, b"this is not an HDF5 file").expect("write failed");
    let result = extract_from_hdf5(&path);
    assert!(result.is_err(), "Expected error for non-HDF5 input, got Ok");
}

/// A non-existent file should return an ExtractionError.
#[cfg(feature = "hdf5")]
#[test]
fn test_hdf5_extraction_missing_file() {
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let path = dir.path().join("nonexistent_oxigdal_test.h5");
    let result = extract_from_hdf5(&path);
    assert!(result.is_err(), "Expected error for missing file");
}

/// Write a minimal valid HDF5 file using Hdf5Writer and verify round-trip
/// metadata extraction with the `hdf5` feature.
#[cfg(feature = "hdf5")]
#[test]
fn test_hdf5_extraction_roundtrip() {
    use oxigdal_hdf5::{Attribute, Datatype, Hdf5Version, Hdf5Writer};

    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let path = dir.path().join("test_meta.h5");

    // Build a minimal HDF5 file with known attributes and a dataset.
    {
        let mut writer =
            Hdf5Writer::create(&path, Hdf5Version::V10).expect("Hdf5Writer::create failed");

        // Root group attributes
        writer
            .add_group_attribute("/", Attribute::string("title", "Test HDF5 Dataset"))
            .expect("add title attr");
        writer
            .add_group_attribute(
                "/",
                Attribute::string("comment", "Created for oxigdal-metadata test"),
            )
            .expect("add comment attr");
        writer
            .add_group_attribute("/", Attribute::string("Conventions", "CF-1.8"))
            .expect("add Conventions attr");
        writer
            .add_group_attribute("/", Attribute::string("keywords", "hdf5,test,geospatial"))
            .expect("add keywords attr");
        // Geospatial bounds. The real Pure-Rust HDF5 writer (oxih5) supports
        // only string root attributes; the extractor parses these back to f64
        // for the bounding box, so string form round-trips the same values.
        writer
            .add_group_attribute("/", Attribute::string("geospatial_lon_min", "-120.5"))
            .expect("add lon_min attr");
        writer
            .add_group_attribute("/", Attribute::string("geospatial_lon_max", "-119.0"))
            .expect("add lon_max attr");
        writer
            .add_group_attribute("/", Attribute::string("geospatial_lat_min", "35.0"))
            .expect("add lat_min attr");
        writer
            .add_group_attribute("/", Attribute::string("geospatial_lat_max", "36.5"))
            .expect("add lat_max attr");

        // Subgroup. NOTE: the real Pure-Rust HDF5 writer (oxih5) does not
        // support attributes on a sub-group, so we do not attach one here — the
        // group is still created and listed on read-back.
        writer.create_group("/measurements").expect("create group");

        // Dataset. The real writer supports f64/i32 datasets inside a
        // single-level sub-group and requires their data to be written
        // (zero-filled sub-group datasets are rejected, fail-loud).
        writer
            .create_dataset(
                "/measurements/temperature",
                Datatype::Float64,
                vec![10, 20],
                Default::default(),
            )
            .expect("create dataset");
        let temperature: Vec<f64> = (0..10 * 20).map(|i| 20.0 + f64::from(i)).collect();
        writer
            .write_f64("/measurements/temperature", &temperature)
            .expect("write dataset data");
        writer
            .add_dataset_attribute(
                "/measurements/temperature",
                Attribute::string("units", "celsius"),
            )
            .expect("add dataset attr");

        writer.finalize().expect("finalize");
    }

    // Now extract metadata.
    let meta = extract_from_hdf5(&path).expect("extract_from_hdf5 should succeed");

    assert_eq!(meta.format.as_deref(), Some("HDF5"));

    // Title should be populated from the "title" attribute.
    assert_eq!(
        meta.title.as_deref(),
        Some("Test HDF5 Dataset"),
        "title should come from hdf5_attr_title"
    );

    // Abstract from "comment".
    assert_eq!(
        meta.abstract_text.as_deref(),
        Some("Created for oxigdal-metadata test"),
        "abstract_text should come from hdf5_attr_comment"
    );

    // Keywords parsed from comma-separated "keywords" attribute.
    assert!(
        meta.keywords.contains(&"hdf5".to_string()),
        "keywords should contain 'hdf5'"
    );
    assert!(
        meta.keywords.contains(&"test".to_string()),
        "keywords should contain 'test'"
    );
    assert!(
        meta.keywords.contains(&"geospatial".to_string()),
        "keywords should contain 'geospatial'"
    );

    // Bounding box from geospatial_lon/lat_min/max attributes.
    let bbox = meta
        .bbox
        .expect("bbox should be populated from geospatial_lon/lat attrs");
    assert!((bbox.west - (-120.5)).abs() < 1e-9, "bbox.west mismatch");
    assert!((bbox.east - (-119.0)).abs() < 1e-9, "bbox.east mismatch");
    assert!((bbox.south - 35.0).abs() < 1e-9, "bbox.south mismatch");
    assert!((bbox.north - 36.5).abs() < 1e-9, "bbox.north mismatch");

    // Superblock version should be recorded.
    assert!(
        meta.attributes.contains_key("hdf5_superblock_version"),
        "superblock version attribute expected"
    );

    // Conventions attribute should be visible.
    assert_eq!(
        meta.attributes
            .get("hdf5_attr_Conventions")
            .map(|s| s.as_str()),
        Some("CF-1.8")
    );

    // Subgroup should be listed.
    if let Some(groups) = meta.attributes.get("hdf5_groups") {
        assert!(
            groups.contains("/measurements"),
            "hdf5_groups should contain /measurements, got: {groups}"
        );
    }

    // Dataset shape and dtype should be recorded.
    assert_eq!(
        meta.attributes
            .get("hdf5_ds_measurements_temperature_shape")
            .map(|s| s.as_str()),
        Some("10x20"),
        "dataset shape should be recorded"
    );
    assert!(
        meta.attributes
            .contains_key("hdf5_ds_measurements_temperature_dtype"),
        "dataset dtype should be recorded"
    );
}

/// `extract_metadata` dispatcher routes .h5 extension to HDF5 extractor.
#[cfg(feature = "hdf5")]
#[test]
#[allow(clippy::panic)]
fn test_extract_metadata_routes_h5_extension() {
    let dir = tempfile::tempdir().expect("failed to create tempdir");
    let path = dir.path().join("bad_content.h5");
    // Write non-HDF5 bytes — we just want to confirm the routing, not a
    // successful parse.
    std::fs::write(&path, b"not hdf5").expect("write failed");
    let result = extract_metadata(&path);
    // Should fail, but with ExtractionError (not Unsupported), meaning it
    // reached the HDF5 extractor.
    match result {
        Err(MetadataError::ExtractionError(_)) => {}
        Err(other) => panic!("expected ExtractionError, got {:?}", other),
        Ok(_) => panic!("expected error for non-HDF5 data"),
    }
}
