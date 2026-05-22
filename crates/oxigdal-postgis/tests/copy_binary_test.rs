//! Integration tests for the PostgreSQL binary-COPY payload encoder.
//!
//! The [`CopyBinaryEncoder`] is fully testable without a database, so the byte
//! layout is exercised exhaustively here. The single live round-trip test is
//! marked `#[ignore]` because it needs a running PostgreSQL+PostGIS instance.

use oxigdal_postgis::copy_binary::{COPY_BINARY_SIGNATURE, CopyBinaryEncoder, ewkb_from_wkb};

/// Builds a minimal little-endian WKB `Point(x, y)` without an SRID.
///
/// Layout: `[0x01][type u32 LE = 1][x f64 LE][y f64 LE]` — 21 bytes total.
fn little_endian_wkb_point(x: f64, y: f64) -> Vec<u8> {
    let mut wkb = Vec::with_capacity(21);
    wkb.push(0x01); // little-endian byte order
    wkb.extend_from_slice(&1u32.to_le_bytes()); // geometry type: Point
    wkb.extend_from_slice(&x.to_le_bytes());
    wkb.extend_from_slice(&y.to_le_bytes());
    wkb
}

#[test]
fn test_copy_encoder_signature_bytes() {
    let encoder = CopyBinaryEncoder::new();
    let payload = encoder.finish();
    // The first 11 bytes must be the fixed PGCOPY signature.
    assert_eq!(&payload[..11], &COPY_BINARY_SIGNATURE);
    assert_eq!(&payload[..11], b"PGCOPY\n\xff\r\n\0");
}

#[test]
fn test_copy_encoder_header_flags_and_extension_zero() {
    let encoder = CopyBinaryEncoder::new();
    let payload = encoder.finish();
    // Bytes 11..19 are the i32 flags field followed by the i32 header
    // extension length — eight zero bytes in total.
    assert_eq!(&payload[11..19], &[0u8; 8]);
}

#[test]
fn test_copy_encoder_single_row_field_count() {
    let mut encoder = CopyBinaryEncoder::new();
    let header_len = encoder.len();
    encoder.begin_row(2);
    let payload = encoder.finish();
    // `begin_row(2)` appends the i16 BE field count `00 02`.
    assert_eq!(&payload[header_len..header_len + 2], &[0x00, 0x02]);
}

#[test]
fn test_copy_encoder_write_field_length_prefix_big_endian() {
    let mut encoder = CopyBinaryEncoder::new();
    let mark = encoder.len();
    encoder.write_field_bytes(&[0xAA, 0xBB]);
    let payload = encoder.finish();
    // i32 BE length prefix `00 00 00 02` followed by the raw bytes `AA BB`.
    assert_eq!(
        &payload[mark..mark + 6],
        &[0x00, 0x00, 0x00, 0x02, 0xAA, 0xBB]
    );
}

#[test]
fn test_copy_encoder_write_null_emits_minus_one() {
    let mut encoder = CopyBinaryEncoder::new();
    let mark = encoder.len();
    encoder.write_null();
    let payload = encoder.finish();
    // A NULL field is the i32 BE value -1 = `FF FF FF FF` with no body.
    assert_eq!(&payload[mark..mark + 4], &[0xFF, 0xFF, 0xFF, 0xFF]);
}

#[test]
fn test_copy_encoder_finish_appends_trailer() {
    let encoder = CopyBinaryEncoder::new();
    let payload = encoder.finish();
    // The stream trailer is the i16 BE value -1 = `FF FF`.
    assert_eq!(&payload[payload.len() - 2..], &[0xFF, 0xFF]);
}

#[test]
fn test_copy_encoder_multi_row_layout() {
    let mut encoder = CopyBinaryEncoder::new();
    let body_start = encoder.len();

    // Row 0: one field with a single byte 0x10.
    encoder.begin_row(1);
    encoder.write_field_bytes(&[0x10]);
    // Row 1: two fields — a NULL then a two-byte field.
    encoder.begin_row(2);
    encoder.write_null();
    encoder.write_field_bytes(&[0x20, 0x21]);

    let payload = encoder.finish();

    let expected_body: Vec<u8> = vec![
        // Row 0
        0x00, 0x01, // field count 1
        0x00, 0x00, 0x00, 0x01, // field length 1
        0x10, // field body
        // Row 1
        0x00, 0x02, // field count 2
        0xFF, 0xFF, 0xFF, 0xFF, // NULL field
        0x00, 0x00, 0x00, 0x02, // field length 2
        0x20, 0x21, // field body
        // Trailer
        0xFF, 0xFF,
    ];
    assert_eq!(&payload[body_start..], expected_body.as_slice());
}

#[test]
fn test_ewkb_from_wkb_sets_srid_flag_and_appends_srid() {
    // Plain little-endian Point WKB; type word currently 0x00000001.
    let wkb = little_endian_wkb_point(1.0, 2.0);
    let ewkb = ewkb_from_wkb(&wkb, 4326).expect("ewkb conversion failed");

    // EWKB is 4 bytes longer than the plain WKB (the inserted SRID).
    assert_eq!(ewkb.len(), wkb.len() + 4);

    // Byte order byte is preserved.
    assert_eq!(ewkb[0], 0x01);

    // The type word now carries the SRID flag bit 0x20000000.
    let type_word = u32::from_le_bytes([ewkb[1], ewkb[2], ewkb[3], ewkb[4]]);
    assert_eq!(type_word & 0x2000_0000, 0x2000_0000);
    // Base geometry type is still Point (1).
    assert_eq!(type_word & 0x0000_00FF, 1);

    // The i32 SRID is spliced in immediately after the type word.
    let srid = i32::from_le_bytes([ewkb[5], ewkb[6], ewkb[7], ewkb[8]]);
    assert_eq!(srid, 4326);
}

#[test]
fn test_ewkb_from_wkb_point_round_trip_byte_layout() {
    // Full byte-for-byte assertion of an EWKB Point(1.0, 2.0) with SRID 4326.
    let wkb = little_endian_wkb_point(1.0, 2.0);
    let ewkb = ewkb_from_wkb(&wkb, 4326).expect("ewkb conversion failed");

    let mut expected = Vec::with_capacity(25);
    expected.push(0x01); // little-endian
    expected.extend_from_slice(&0x2000_0001u32.to_le_bytes()); // Point | SRID flag
    expected.extend_from_slice(&4326i32.to_le_bytes()); // SRID
    expected.extend_from_slice(&1.0f64.to_le_bytes()); // x
    expected.extend_from_slice(&2.0f64.to_le_bytes()); // y

    assert_eq!(ewkb, expected);
}

/// Live round-trip test: encodes 100 features into a binary-COPY payload, sends
/// them through a real `COPY ... FROM STDIN WITH (FORMAT binary)` and reads the
/// row count back.
///
/// This test is `#[ignore]` because it requires a running PostgreSQL instance
/// with the PostGIS extension. Run it explicitly with:
///
/// ```text
/// PG_TEST_URL='host=localhost user=postgres dbname=gis' \
///     cargo nextest run -p oxigdal-postgis --run-ignored all \
///     -E 'binary(copy_binary_test)'
/// ```
///
/// (or `cargo test -p oxigdal-postgis -- --ignored`). The connection string is
/// read from the `PG_TEST_URL` environment variable.
#[tokio::test]
#[ignore = "requires a running PostgreSQL+PostGIS instance (set PG_TEST_URL)"]
async fn test_copy_in_binary_roundtrip_100_features() {
    use oxigdal_core::vector::feature::Feature;
    use oxigdal_core::vector::geometry::{Geometry, Point};
    use oxigdal_postgis::{ConnectionConfig, ConnectionPool, PostGisWriter};

    let conn_str = std::env::var("PG_TEST_URL")
        .expect("PG_TEST_URL must be set for the live binary-COPY round-trip test");

    // `ConnectionPool` does not implement `Clone`; build one pool for the
    // writer and a second from the same config for the verification query.
    let writer_config = ConnectionConfig::from_connection_string(&conn_str)
        .expect("invalid PG_TEST_URL connection string");
    let writer_pool =
        ConnectionPool::new(writer_config).expect("failed to build writer connection pool");
    let verify_config = ConnectionConfig::from_connection_string(&conn_str)
        .expect("invalid PG_TEST_URL connection string");
    let verify_pool =
        ConnectionPool::new(verify_config).expect("failed to build verify connection pool");

    let table = "oxigdal_copy_binary_roundtrip_test";
    let mut writer = PostGisWriter::new(writer_pool, table)
        .srid(4326)
        .create_table(true);

    // Start from a clean table.
    writer.truncate().await.expect("truncate failed");

    // Encode 100 distinct point features.
    for i in 0..100i32 {
        let point = Point::new(f64::from(i), f64::from(i) * 2.0);
        let mut feature = Feature::new(Geometry::Point(point));
        feature.set_property(
            "idx",
            oxigdal_core::vector::feature::FieldValue::Integer(i64::from(i)),
        );
        writer.add_to_batch(feature);
    }

    let written = writer.flush().await.expect("binary COPY flush failed");
    assert_eq!(written, 100);

    // Verify the row count landed in the database.
    let client = verify_pool.get().await.expect("failed to get connection");
    let sql = format!("SELECT COUNT(*) FROM \"{table}\"");
    let row = client
        .query_one(&sql, &[])
        .await
        .expect("count query failed");
    let count: i64 = row.get(0);
    assert_eq!(count, 100);

    // Clean up.
    writer.truncate().await.expect("cleanup truncate failed");
}
