// build.rs for oxigdal-python
//
// When the `extension-module` feature is enabled AND we are building a test
// binary (rather than the cdylib extension), the linker needs to resolve
// Python symbols explicitly.  Without this, `cargo nextest --all-features`
// fails because pyo3's extension-module mode suppresses the `-lpython3.x`
// linker argument (by design: the .so is loaded by an already-running
// Python interpreter that supplies those symbols at runtime).
//
// This build script detects when `extension-module` is active and emits
// the correct linker flags to link against Python.

use std::process::Command;

fn main() {
    // Feature flags in build scripts are available as CARGO_FEATURE_<NAME>.
    let extension_module = std::env::var("CARGO_FEATURE_EXTENSION_MODULE").is_ok();

    if !extension_module {
        // pyo3 handles linking automatically when extension-module is not active.
        return;
    }

    // When extension-module is active, pyo3 intentionally omits `-lpython3.x`
    // so the .so can be loaded by an existing Python process.  But when building
    // as a test binary or rlib (cargo nextest --all-features, cargo test), we
    // need Python symbols resolved at link time.
    //
    // We emit the Python library link directives here.  For true cdylib builds
    // on macOS, the linker normally uses `-undefined dynamic_lookup`, making
    // extra link-lib directives either harmless or helpful.

    let python = std::env::var("PYO3_PYTHON").unwrap_or_else(|_| "python3".to_string());

    // Query Python for its configuration
    let output = Command::new(&python)
        .args([
            "-c",
            concat!(
                "import sysconfig, os; ",
                "cfg = sysconfig.get_config_vars(); ",
                "libdir = cfg.get('LIBDIR', '') or ''; ",
                "ldlib = cfg.get('LDLIBRARY', '') or ''; ",
                "framework = cfg.get('PYTHONFRAMEWORK', '') or ''; ",
                "frameworkdir = cfg.get('PYTHONFRAMEWORKPREFIX', '') or ''; ",
                "ver = cfg.get('LDVERSION', '') or cfg.get('py_version_short', ''); ",
                "print(libdir); print(ldlib); print(framework); print(frameworkdir); print(ver)"
            ),
        ])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            let mut lines = text.lines();
            let libdir = lines.next().unwrap_or("").trim().to_string();
            let ldlib = lines.next().unwrap_or("").trim().to_string();
            let framework = lines.next().unwrap_or("").trim().to_string();
            let frameworkdir = lines.next().unwrap_or("").trim().to_string();
            let ver = lines.next().unwrap_or("").trim().to_string();

            if !libdir.is_empty() {
                println!("cargo:rustc-link-search=native={libdir}");
            }

            if !framework.is_empty() {
                // macOS Framework build — link with -framework Python
                if !frameworkdir.is_empty() {
                    println!("cargo:rustc-link-search=framework={frameworkdir}");
                } else if !libdir.is_empty() {
                    // Try to find the Frameworks dir from libdir
                    // e.g. .../Python.framework/Versions/3.14/lib -> .../
                    if let Some(idx) = libdir.find("Python.framework") {
                        let base = &libdir[..idx];
                        println!("cargo:rustc-link-search=framework={base}");
                    }
                }
                println!("cargo:rustc-link-lib=framework={framework}");
            } else if !ldlib.is_empty() {
                // Unix-style shared library
                let name = if let Some(stripped) = ldlib.strip_prefix("lib") {
                    stripped
                        .trim_end_matches(".dylib")
                        .trim_end_matches(".so")
                        .trim_end_matches(".a")
                        .to_string()
                } else {
                    // Fallback: use version to construct name
                    if !ver.is_empty() {
                        format!("python{ver}")
                    } else {
                        "python3".to_string()
                    }
                };
                if !name.is_empty() {
                    println!("cargo:rustc-link-lib=dylib={name}");
                }
            } else if !ver.is_empty() {
                // Last resort: construct the library name from version
                println!("cargo:rustc-link-lib=dylib=python{ver}");
            }
        }
        _ => {
            // Fallback if python3 is not available
            println!("cargo:rustc-link-lib=dylib=python3");
        }
    }
}
