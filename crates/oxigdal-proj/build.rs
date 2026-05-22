//! Build script for oxigdal-proj: generates EPSG registration code from
//! `epsg-snapshot/minimal.json` at compile time.
#![allow(clippy::expect_used)]

use serde::Deserialize;
use std::{env, fs, io::Write, path::PathBuf};

#[derive(Debug, Deserialize)]
struct EpsgRecord {
    code: u32,
    name: String,
    proj_string: String,
    wkt: Option<String>,
    crs_type: String,
    area_of_use: String,
    unit: String,
    datum: String,
}

fn main() {
    println!("cargo:rerun-if-changed=epsg-snapshot/minimal.json");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));
    let out_path = out_dir.join("generated_epsg.rs");

    // Path to the snapshot relative to CARGO_MANIFEST_DIR
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"));
    let json_path = manifest_dir.join("epsg-snapshot").join("minimal.json");

    if !json_path.exists() {
        // Emit an empty function — graceful no-op when file is absent
        let mut f = fs::File::create(&out_path).expect("cannot create generated_epsg.rs");
        writeln!(
            f,
            "pub(crate) fn register_generated_epsg(_db: &mut EpsgDatabase) {{}}"
        )
        .expect("cannot write fallback generated_epsg.rs");
        return;
    }

    let content = fs::read_to_string(&json_path).expect("cannot read minimal.json");
    let records: Vec<EpsgRecord> =
        serde_json::from_str(&content).expect("invalid JSON in minimal.json");

    let mut out = String::new();
    out.push_str("pub(crate) fn register_generated_epsg(db: &mut EpsgDatabase) {\n");

    for rec in &records {
        let wkt = match &rec.wkt {
            Some(w) => format!("Some(String::from({:?}))", w),
            None => "None".to_string(),
        };
        let crs_type = map_crs_type(&rec.crs_type);
        let entry = format!(
            "    db.definitions.entry({code}).or_insert_with(|| EpsgDefinition {{\n        code: {code},\n        name: String::from({name:?}),\n        proj_string: String::from({proj:?}),\n        wkt: {wkt},\n        crs_type: {crs_type},\n        area_of_use: String::from({area:?}),\n        unit: String::from({unit:?}),\n        datum: String::from({datum:?}),\n    }});\n",
            code = rec.code,
            name = rec.name,
            proj = rec.proj_string,
            wkt = wkt,
            crs_type = crs_type,
            area = rec.area_of_use,
            unit = rec.unit,
            datum = rec.datum,
        );
        out.push_str(&entry);
    }

    out.push_str("}\n");

    let mut f = fs::File::create(&out_path).expect("cannot create generated_epsg.rs");
    f.write_all(out.as_bytes())
        .expect("cannot write generated_epsg.rs");
}

fn map_crs_type(s: &str) -> &'static str {
    match s {
        "Geographic" => "CrsType::Geographic",
        "Projected" => "CrsType::Projected",
        "Geocentric" => "CrsType::Geocentric",
        "Vertical" => "CrsType::Vertical",
        "Compound" => "CrsType::Compound",
        "Engineering" => "CrsType::Engineering",
        _ => "CrsType::Geographic", // default
    }
}
