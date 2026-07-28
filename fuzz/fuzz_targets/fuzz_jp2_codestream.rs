//! Fuzz target: raw JPEG2000 codestream (J2K) marker/segment parsing and the
//! Tier-1 EBCOT code-block decoder - the layer beneath `fuzz_jpeg2000_boxes`
//! (which only exercises the outer JP2 *box* structure).
//!
//! Covers three independent, bounded entry points over the same bytes:
//!
//! 1. `CodestreamParser` marker walk: reads markers one at a time and
//!    dispatches SIZ/COD/QCD to their dedicated segment parsers, skipping
//!    every other marker's segment body via its declared length. Bounded
//!    naturally (each iteration consumes at least 2 bytes from a
//!    fixed-size buffer) plus an explicit iteration cap.
//! 2. `tier1::decode_code_block` / `decode_code_block_passes`: the real
//!    MQ-arithmetic-coded EBCOT bit-plane decoder, fed attacker bytes
//!    directly as the code-block bitstream. Width/height are derived from
//!    the input but capped small so this stays cheap regardless of what
//!    the fuzzer picks.
//! 3. `tier1::MqDecoder::decode`: the raw MQ arithmetic decoder driven
//!    directly across all 19 contexts.
//!
//! Any `Err` is acceptable; panics and out-of-bounds reads are not.
#![no_main]
use libfuzzer_sys::fuzz_target;
use oxigeo_jpeg2000::codestream::{CodestreamParser, Marker};
use oxigeo_jpeg2000::tier1::{
    self,
    decoder::{decode_code_block, decode_code_block_passes},
    MqDecoder,
};
use std::io::Cursor;

/// Cap on code-block dimensions used for Tier-1 fuzzing: real code-blocks
/// are at most 64x64 per the JPEG2000 spec (`xcb`/`ycb` in COD, max 6 bits
/// each => 2^6 = 64), so this stays realistic while remaining cheap.
const MAX_CB_DIM: usize = 64;

fn subband_for(byte: u8) -> tier1::SubbandType {
    match byte % 4 {
        0 => tier1::SubbandType::Ll,
        1 => tier1::SubbandType::Lh,
        2 => tier1::SubbandType::Hl,
        _ => tier1::SubbandType::Hh,
    }
}

fuzz_target!(|data: &[u8]| {
    // (1) Marker/segment walk over the raw codestream.
    let mut parser = CodestreamParser::new(Cursor::new(data));
    for _ in 0..4096 {
        match parser.read_marker() {
            Ok(Some(marker)) => match marker {
                // SOC/SOD/EOC carry no length-prefixed segment.
                Marker::Soc | Marker::Sod | Marker::Eoc => {}
                Marker::Siz => {
                    let _ = parser.parse_siz();
                }
                Marker::Cod => {
                    let _ = parser.parse_cod();
                }
                Marker::Qcd => {
                    let _ = parser.parse_qcd();
                }
                _ => {
                    if let Ok(len) = parser.read_segment_length() {
                        let _ = parser.skip_segment(len);
                    }
                }
            },
            Ok(None) => break,
            Err(_) => break,
        }
    }

    if data.len() < 3 {
        return;
    }

    // (2) Tier-1 EBCOT code-block decoder, fed the rest of the input as the
    // compressed bitstream with small attacker-influenced (but capped)
    // dimensions and bit-plane count.
    let width = 1 + (data[0] as usize % MAX_CB_DIM);
    let height = 1 + (data[1] as usize % MAX_CB_DIM);
    let num_bitplanes = 1 + (data[2] as usize % 16);
    let subband = subband_for(data[0]);
    let payload = &data[3..];

    let _ = decode_code_block(payload, width, height, num_bitplanes, subband);
    let max_passes = 1 + (data[2] as usize % 32);
    let _ = decode_code_block_passes(payload, width, height, num_bitplanes, max_passes, subband);

    // (3) Raw MQ arithmetic decoder, driven across every context directly.
    let mut mq = MqDecoder::new(payload.to_vec());
    for ctx in 0..tier1::mq::NUM_CONTEXTS {
        if mq.is_exhausted() {
            break;
        }
        let _ = mq.decode(ctx);
    }
});
