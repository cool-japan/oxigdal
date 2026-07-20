//! Benchmarks for TIFF parsing operations
#![allow(missing_docs, clippy::expect_used)]
//!
//! This benchmark suite measures the performance of:
//! - Header parsing (Classic and BigTIFF)
//! - IFD entry parsing
//! - Tag value extraction
//! - Byte order conversions

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxigdal_geotiff::tiff::{ByteOrderType, FieldType, IfdEntry, TiffHeader, TiffVariant};
use std::hint::black_box;

fn bench_header_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("tiff/header_parse");

    // Classic TIFF little-endian
    let classic_le = [
        0x49, 0x49, // Little-endian
        0x2A, 0x00, // Version 42
        0x08, 0x00, 0x00, 0x00, // First IFD at offset 8
    ];

    // Classic TIFF big-endian
    let classic_be = [
        0x4D, 0x4D, // Big-endian
        0x00, 0x2A, // Version 42
        0x00, 0x00, 0x00, 0x08, // First IFD at offset 8
    ];

    // BigTIFF little-endian
    let bigtiff_le = [
        0x49, 0x49, // Little-endian
        0x2B, 0x00, // Version 43
        0x08, 0x00, // Offset byte size = 8
        0x00, 0x00, // Constant = 0
        0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // First IFD at offset 16
    ];

    group.bench_function("classic_le", |b| {
        b.iter(|| {
            black_box(TiffHeader::parse(black_box(&classic_le)).ok());
        });
    });

    group.bench_function("classic_be", |b| {
        b.iter(|| {
            black_box(TiffHeader::parse(black_box(&classic_be)).ok());
        });
    });

    group.bench_function("bigtiff_le", |b| {
        b.iter(|| {
            black_box(TiffHeader::parse(black_box(&bigtiff_le)).ok());
        });
    });

    group.finish();
}

fn bench_header_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("tiff/header_serialize");

    let classic = TiffHeader::classic(ByteOrderType::LittleEndian, 1024);
    let bigtiff = TiffHeader::bigtiff(ByteOrderType::LittleEndian, 0x1234_5678_9ABC_DEF0);

    group.bench_function("classic", |b| {
        b.iter(|| {
            black_box(classic.to_bytes());
        });
    });

    group.bench_function("bigtiff", |b| {
        b.iter(|| {
            black_box(bigtiff.to_bytes());
        });
    });

    group.finish();
}

fn bench_ifd_entry_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("tiff/ifd_entry_parse");

    // Classic TIFF entry (12 bytes)
    // ImageWidth tag (256), LONG (4), count=1, value=1024
    let classic_entry = [
        0x00, 0x01, // Tag: 256 (ImageWidth)
        0x04, 0x00, // Type: LONG (4)
        0x01, 0x00, 0x00, 0x00, // Count: 1
        0x00, 0x04, 0x00, 0x00, // Value: 1024 (inline)
    ];

    // BigTIFF entry (20 bytes)
    let bigtiff_entry = [
        0x00, 0x01, // Tag: 256 (ImageWidth)
        0x04, 0x00, // Type: LONG (4)
        0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Count: 1
        0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Value: 1024 (inline)
    ];

    group.bench_function("classic", |b| {
        b.iter(|| {
            black_box(
                IfdEntry::parse(
                    black_box(&classic_entry),
                    ByteOrderType::LittleEndian,
                    TiffVariant::Classic,
                )
                .ok(),
            );
        });
    });

    group.bench_function("bigtiff", |b| {
        b.iter(|| {
            black_box(
                IfdEntry::parse(
                    black_box(&bigtiff_entry),
                    ByteOrderType::LittleEndian,
                    TiffVariant::BigTiff,
                )
                .ok(),
            );
        });
    });

    group.finish();
}

fn bench_byte_order_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("tiff/byte_order_read");

    let data_u16 = [0x12, 0x34];
    let data_u32 = [0x12, 0x34, 0x56, 0x78];
    let data_u64 = [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0];

    group.bench_function("u16_le", |b| {
        b.iter(|| {
            black_box(ByteOrderType::LittleEndian.read_u16(black_box(&data_u16)));
        });
    });

    group.bench_function("u16_be", |b| {
        b.iter(|| {
            black_box(ByteOrderType::BigEndian.read_u16(black_box(&data_u16)));
        });
    });

    group.bench_function("u32_le", |b| {
        b.iter(|| {
            black_box(ByteOrderType::LittleEndian.read_u32(black_box(&data_u32)));
        });
    });

    group.bench_function("u32_be", |b| {
        b.iter(|| {
            black_box(ByteOrderType::BigEndian.read_u32(black_box(&data_u32)));
        });
    });

    group.bench_function("u64_le", |b| {
        b.iter(|| {
            black_box(ByteOrderType::LittleEndian.read_u64(black_box(&data_u64)));
        });
    });

    group.bench_function("u64_be", |b| {
        b.iter(|| {
            black_box(ByteOrderType::BigEndian.read_u64(black_box(&data_u64)));
        });
    });

    group.bench_function("f64_le", |b| {
        b.iter(|| {
            black_box(ByteOrderType::LittleEndian.read_f64(black_box(&data_u64)));
        });
    });

    group.finish();
}

fn bench_byte_order_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("tiff/byte_order_write");

    let mut buf_u16 = [0u8; 2];
    let mut buf_u32 = [0u8; 4];
    let mut buf_u64 = [0u8; 8];

    group.bench_function("u16_le", |b| {
        b.iter(|| {
            ByteOrderType::LittleEndian.write_u16(black_box(&mut buf_u16), black_box(0x1234));
        });
    });

    group.bench_function("u16_be", |b| {
        b.iter(|| {
            ByteOrderType::BigEndian.write_u16(black_box(&mut buf_u16), black_box(0x1234));
        });
    });

    group.bench_function("u32_le", |b| {
        b.iter(|| {
            ByteOrderType::LittleEndian.write_u32(black_box(&mut buf_u32), black_box(0x12345678));
        });
    });

    group.bench_function("u32_be", |b| {
        b.iter(|| {
            ByteOrderType::BigEndian.write_u32(black_box(&mut buf_u32), black_box(0x12345678));
        });
    });

    group.bench_function("u64_le", |b| {
        b.iter(|| {
            ByteOrderType::LittleEndian
                .write_u64(black_box(&mut buf_u64), black_box(0x123456789ABCDEF0));
        });
    });

    group.bench_function("u64_be", |b| {
        b.iter(|| {
            ByteOrderType::BigEndian
                .write_u64(black_box(&mut buf_u64), black_box(0x123456789ABCDEF0));
        });
    });

    group.finish();
}

fn bench_field_type_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("tiff/field_type");

    let field_types = vec![
        FieldType::Byte,
        FieldType::Short,
        FieldType::Long,
        FieldType::Float,
        FieldType::Double,
        FieldType::Long8,
    ];

    group.bench_function("element_size", |b| {
        b.iter(|| {
            for ft in &field_types {
                black_box(ft.element_size());
            }
        });
    });

    group.bench_function("is_signed", |b| {
        b.iter(|| {
            for ft in &field_types {
                black_box(ft.is_signed());
            }
        });
    });

    group.bench_function("is_floating_point", |b| {
        b.iter(|| {
            for ft in &field_types {
                black_box(ft.is_floating_point());
            }
        });
    });

    group.finish();
}

fn bench_ifd_entry_inline_check(c: &mut Criterion) {
    let mut group = c.benchmark_group("tiff/ifd_entry_inline");

    // Create entries with different sizes
    let entries = vec![
        ("inline_u16", 256, FieldType::Short, 1),
        ("inline_u32", 256, FieldType::Long, 1),
        ("not_inline_u32", 256, FieldType::Long, 2), // 8 bytes, won't fit in classic
        ("inline_u64", 256, FieldType::Long8, 1),
    ];

    for (name, tag, field_type, count) in entries {
        let entry = IfdEntry {
            tag,
            field_type,
            count,
            value_offset: 0,
            inline_value: Some(vec![0u8; 8]),
        };

        group.bench_with_input(BenchmarkId::new("classic", name), &entry, |b, entry| {
            b.iter(|| {
                black_box(entry.is_inline(TiffVariant::Classic));
            });
        });

        group.bench_with_input(BenchmarkId::new("bigtiff", name), &entry, |b, entry| {
            b.iter(|| {
                black_box(entry.is_inline(TiffVariant::BigTiff));
            });
        });
    }

    group.finish();
}

fn bench_batch_ifd_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("tiff/batch_ifd_parse");

    // Create a batch of IFD entries
    let entry_template = [
        0x00, 0x01, // Tag
        0x04, 0x00, // Type: LONG
        0x01, 0x00, 0x00, 0x00, // Count: 1
        0x00, 0x00, 0x00, 0x00, // Value
    ];

    let entry_counts = vec![10, 50, 100, 200];

    for count in entry_counts {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, count| {
            b.iter(|| {
                for _ in 0..*count {
                    black_box(
                        IfdEntry::parse(
                            &entry_template,
                            ByteOrderType::LittleEndian,
                            TiffVariant::Classic,
                        )
                        .ok(),
                    );
                }
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_header_parsing,
    bench_header_serialization,
    bench_ifd_entry_parsing,
    bench_byte_order_read,
    bench_byte_order_write,
    bench_field_type_operations,
    bench_ifd_entry_inline_check,
    bench_batch_ifd_parsing
);
criterion_main!(benches);
