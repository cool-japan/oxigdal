//! Fuzz target: COPC/LAS public header, VLR chain, and COPC-info VLR body
//! parsers.
//!
//! Tests that `LasHeader::parse`, `parse_vlrs` (walks `number_of_vlrs`
//! records starting at `header_size`), and `CopcInfo::parse` never panic on
//! arbitrary input - including a `LasHeader` with an attacker-controlled
//! `number_of_vlrs` / `header_size` that could otherwise drive an
//! out-of-bounds read while iterating the VLR chain. Any `Err` is
//! acceptable; panics are not.
#![no_main]
use libfuzzer_sys::fuzz_target;
use oxigdal_copc::{CopcInfo, LasHeader};
use oxigdal_copc::vlr_chain::{find_copc_hierarchy_vlr, find_copc_info, parse_vlrs};

fuzz_target!(|data: &[u8]| {
    // Raw 160-byte COPC info VLR body decode, independent of any header.
    let _ = CopcInfo::parse(data);

    if let Ok(header) = LasHeader::parse(data) {
        if let Ok(vlrs) = parse_vlrs(data, &header) {
            let _ = find_copc_info(&vlrs);
            let _ = find_copc_hierarchy_vlr(&vlrs);
        }
    }
});
