//! Example: Inspect TIFF file structure
//!
//! This example demonstrates low-level TIFF structure inspection,
//! showing IFDs, tags, and internal organization.
//!
//! Usage:
//!   cargo run --example list_tiff_structure -- `<path-to-tiff>`

use oxigdal_core::io::FileDataSource;
use oxigdal_geotiff::cog::validate_cog;
use oxigdal_geotiff::tiff::{TiffFile, TiffTag};

fn main() -> oxigdal_core::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <tiff-file>", args[0]);
        std::process::exit(1);
    }

    let path = &args[1];

    println!("Parsing TIFF: {}", path);
    println!("{}", "=".repeat(70));

    // Open and parse TIFF
    let source = FileDataSource::open(path)?;
    let tiff = TiffFile::parse(&source)?;

    // Print header information
    println!("\n📋 TIFF Header:");
    println!("  Byte Order: {:?}", tiff.header.byte_order);
    println!(
        "  Variant: {} ({})",
        if tiff.is_bigtiff() {
            "BigTIFF"
        } else {
            "Classic TIFF"
        },
        if tiff.is_bigtiff() {
            "64-bit offsets"
        } else {
            "32-bit offsets"
        }
    );
    println!("  First IFD Offset: 0x{:08X}", tiff.header.first_ifd_offset);

    // Print IFD information
    println!("\n📂 Image File Directories (IFDs): {}", tiff.image_count());

    for (ifd_idx, ifd) in tiff.ifds.iter().enumerate() {
        println!("\n  IFD #{} ({} entries):", ifd_idx, ifd.entries.len());

        // Get essential tags
        let width = ifd
            .get_entry(TiffTag::ImageWidth)
            .and_then(|e| e.get_u64(tiff.byte_order()).ok());
        let height = ifd
            .get_entry(TiffTag::ImageLength)
            .and_then(|e| e.get_u64(tiff.byte_order()).ok());
        let compression = ifd
            .get_entry(TiffTag::Compression)
            .and_then(|e| e.get_u64(tiff.byte_order()).ok());
        let tile_width = ifd
            .get_entry(TiffTag::TileWidth)
            .and_then(|e| e.get_u64(tiff.byte_order()).ok());
        let tile_height = ifd
            .get_entry(TiffTag::TileLength)
            .and_then(|e| e.get_u64(tiff.byte_order()).ok());
        let bits_per_sample = ifd
            .get_entry(TiffTag::BitsPerSample)
            .and_then(|e| e.get_u64(tiff.byte_order()).ok());
        let samples_per_pixel = ifd
            .get_entry(TiffTag::SamplesPerPixel)
            .and_then(|e| e.get_u64(tiff.byte_order()).ok());

        if let (Some(w), Some(h)) = (width, height) {
            println!("    Dimensions: {}x{} pixels", w, h);

            if let (Some(tw), Some(th)) = (tile_width, tile_height) {
                let tiles_x = w.div_ceil(tw);
                let tiles_y = h.div_ceil(th);
                println!("    Tiled: {}x{} ({}x{} tiles)", tw, th, tiles_x, tiles_y);
            } else {
                println!("    Striped or Scanline");
            }
        }

        if let Some(comp) = compression {
            let comp_name = match comp {
                1 => "None",
                5 => "LZW",
                7 => "JPEG",
                8 | 32946 => "DEFLATE",
                50000 => "ZSTD",
                _ => "Other",
            };
            println!("    Compression: {} ({})", comp_name, comp);
        }

        if let Some(bps) = bits_per_sample {
            if let Some(spp) = samples_per_pixel {
                println!("    Samples: {} @ {} bits/sample", spp, bps);
            }
        }

        // Check for GeoTIFF tags
        let has_model_pixel_scale = ifd.get_entry(TiffTag::ModelPixelScale).is_some();
        let has_model_tiepoint = ifd.get_entry(TiffTag::ModelTiepoint).is_some();
        let has_geo_key_dir = ifd.get_entry(TiffTag::GeoKeyDirectory).is_some();

        if has_model_pixel_scale || has_model_tiepoint || has_geo_key_dir {
            println!("    GeoTIFF Tags: ");
            if has_model_pixel_scale {
                println!("      ✓ ModelPixelScale");
            }
            if has_model_tiepoint {
                println!("      ✓ ModelTiepoint");
            }
            if has_geo_key_dir {
                println!("      ✓ GeoKeyDirectory");
            }
        }

        if ifd.has_next() {
            println!("    Next IFD: 0x{:08X}", ifd.next_ifd_offset);
        }
    }

    // Validate COG compliance
    println!("\n🔍 COG Validation:");
    let validation = validate_cog(&tiff, &source);
    println!(
        "  Is Valid COG: {}",
        if validation.is_valid {
            "✅ Yes"
        } else {
            "❌ No"
        }
    );
    println!(
        "  Has Overviews: {}",
        if validation.has_overviews {
            "✅ Yes"
        } else {
            "❌ No"
        }
    );

    if !validation.messages.is_empty() {
        println!("\n  Messages:");
        for msg in &validation.messages {
            println!("    • {}", msg);
        }
    }

    println!("\n{}", "=".repeat(70));

    Ok(())
}
