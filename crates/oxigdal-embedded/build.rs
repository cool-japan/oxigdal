//! Build script for oxigdal-embedded
//!
//! Declares custom cfg conditions for ESP32 variants

fn main() {
    // Declare custom cfg conditions for ESP32 variants
    // These are set by esp-hal/esp-idf toolchains when targeting ESP32 chips
    println!("cargo::rustc-check-cfg=cfg(esp32)");
    println!("cargo::rustc-check-cfg=cfg(esp32s2)");
    println!("cargo::rustc-check-cfg=cfg(esp32s3)");
    println!("cargo::rustc-check-cfg=cfg(esp32c3)");
    println!("cargo::rustc-check-cfg=cfg(esp32c6)");
    println!("cargo::rustc-check-cfg=cfg(esp32h2)");
}
