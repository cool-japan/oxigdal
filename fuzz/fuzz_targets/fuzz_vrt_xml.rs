//! Fuzz target: VRT (GDAL Virtual Raster) XML parser.
//!
//! `VrtXmlParser::parse` walks an attacker-controlled XML document (`quick-xml`
//! tokenizer output) and builds a `VrtDataset` - band definitions, sources
//! (simple/complex/kernel/averaged), a geo-transform, an SRS string, and a
//! mosaic list. Any `Err` is acceptable; panics and out-of-bounds reads
//! (e.g. from a malformed numeric attribute or an unterminated element) are
//! not.
//!
//! Fuzz input is treated as UTF-8 text via `from_utf8_lossy` rather than
//! rejected outright on invalid UTF-8: this keeps mutated-but-still-textual
//! inputs (the common case once libFuzzer starts from an XML seed) reaching
//! the parser instead of bailing out on a single stray byte.
#![no_main]
use libfuzzer_sys::fuzz_target;
use oxigeo_vrt::xml::{VrtXmlParser, VrtXmlWriter};

fuzz_target!(|data: &[u8]| {
    let xml = String::from_utf8_lossy(data);
    if let Ok(dataset) = VrtXmlParser::parse(&xml) {
        // Round-trip through the writer too - exercises serialization of
        // whatever attacker-influenced structure was actually accepted.
        let _ = VrtXmlWriter::write(&dataset);
        let _ = dataset.validate();
        let _ = dataset.extent();
        let _ = dataset.effective_block_size();
    }
});
