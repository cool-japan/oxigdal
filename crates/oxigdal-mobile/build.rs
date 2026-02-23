//! Build script for oxigdal-mobile
//!
//! This script:
//! 1. Generates C header files using cbindgen
//! 2. Sets up platform-specific build configurations

// Build script allows - not subject to same restrictions as library code
#![allow(clippy::expect_used)]
#![allow(missing_docs)]

use std::env;
use std::path::PathBuf;

fn main() {
    // Get the output directory
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));
    let crate_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"));

    println!("cargo:rerun-if-changed=src/");
    println!("cargo:rerun-if-changed=cbindgen.toml");

    // Generate C header file
    #[cfg(feature = "std")]
    {
        let header_path = out_dir.join("oxigdal_mobile.h");

        match cbindgen::Builder::new()
            .with_crate(&crate_dir)
            .with_config(
                cbindgen::Config::from_file("cbindgen.toml")
                    .ok()
                    .unwrap_or_default(),
            )
            .with_language(cbindgen::Language::C)
            .with_pragma_once(true)
            .with_include_guard("OXIGDAL_MOBILE_H")
            .with_documentation(true)
            .generate()
        {
            Ok(bindings) => {
                if bindings.write_to_file(&header_path) {
                    println!(
                        "cargo:warning=Generated C header: {}",
                        header_path.display()
                    );
                } else {
                    eprintln!("Warning: Failed to write header file");
                }
            }
            Err(e) => {
                eprintln!("Warning: cbindgen failed: {}", e);
            }
        }
    }

    // Platform-specific configuration
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    match target_os.as_str() {
        "ios" => {
            println!("cargo:rustc-link-lib=framework=Foundation");
            println!("cargo:rustc-link-lib=framework=CoreGraphics");
            println!("cargo:rustc-link-lib=framework=UIKit");
        }
        "android" => {
            println!("cargo:rustc-link-lib=log");
            println!("cargo:rustc-link-lib=android");
        }
        _ => {}
    }

    // Note: rustc-link-lib=static=oxigdal_mobile is NOT needed here.
    // That directive tells the linker to find an external native static library,
    // which doesn't exist during normal Rust builds. The static library is only
    // produced as an output artifact (via crate-type=staticlib) for mobile
    // deployment, and linking it back into itself would be circular.
    // Mobile consumers (Xcode/Gradle) link the staticlib directly.
}
