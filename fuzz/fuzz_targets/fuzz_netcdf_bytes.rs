//! Fuzz target: NetCDF-4 (HDF5-backed) file parsing via the Pure-Rust
//! `oxinetcdf` engine that backs `oxigeo-netcdf`'s always-available NetCDF-4
//! reader.
//!
//! `oxinetcdf::NcFile::open_from_bytes` is explicitly documented upstream as
//! "for testing and fuzzing" - it parses the HDF5 superblock/object-header
//! chain via `oxih5` and then resolves the NetCDF-4 conventions on top
//! (dimension scales, `DIMENSION_LIST` axis linkage, recursive sub-groups).
//! Any `Err` is acceptable; panics and out-of-bounds reads are not.
#![no_main]
use libfuzzer_sys::fuzz_target;
use oxinetcdf::NcFile;

/// Recursively touch every variable in a resolved group tree, reading its
/// data with the read method matching its declared dtype. Bounded to a
/// modest number of variables/groups so one adversarial fuzz input can't
/// turn into an unbounded amount of work even though `resolve_group_deep`
/// already caps recursion depth on its own.
fn walk_group(nc: &NcFile, group: &oxinetcdf::NcGroup, budget: &mut usize) {
    for var in group.variables.iter().take(16) {
        if *budget == 0 {
            return;
        }
        *budget -= 1;

        // Exercise both the raw and fill-masked read paths, whichever type
        // matches - a mismatched read just returns an `Err`.
        let _ = var.read_f64(nc);
        let _ = var.read_i64(nc);
        let _ = var.read_strings(nc);
        let _ = var.read_f64_masked(nc);
        let _ = var.read_i64_masked(nc);
        let _ = var.units();
        let _ = var.fill_value();
        let _ = var.dim_names();
    }

    for child in group.children.iter().take(16) {
        if *budget == 0 {
            return;
        }
        walk_group(nc, child, budget);
    }
}

fuzz_target!(|data: &[u8]| {
    if let Ok(nc) = NcFile::open_from_bytes(data) {
        if let Ok(root) = nc.root_group() {
            let mut budget = 256usize;
            walk_group(&nc, &root, &mut budget);
        }
    }
});
