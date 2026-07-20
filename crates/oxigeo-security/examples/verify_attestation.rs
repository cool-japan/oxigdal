//! Independently verify an OxiGeo session attestation from the command line.
//!
//! This is a native re-implementation-free verifier "for skeptics": it reads an
//! attestation JSON file, rebuilds the hash chain, Merkle root, and seal bytes
//! from the JSON alone, checks the Ed25519 signature, and prints each property
//! independently.
//!
//! Run with a path to an attestation JSON file (e.g. one downloaded from the
//! GeoVault demo, or the bundled test fixture):
//!
//! ```text
//! cargo run -p oxigeo-security --features attestation \
//!     --example verify_attestation -- path/to/attestation.json
//! ```
//!
//! Exit status is `0` when all three checks pass, `1` otherwise (or on any I/O
//! or parse error), so it composes in scripts.

use std::process::ExitCode;

use oxigeo_security::attestation::verify_attestation;

fn main() -> ExitCode {
    let path = match std::env::args().nth(1) {
        Some(path) => path,
        None => {
            eprintln!("usage: verify_attestation <attestation.json>");
            return ExitCode::FAILURE;
        }
    };

    let json = match std::fs::read_to_string(&path) {
        Ok(json) => json,
        Err(err) => {
            eprintln!("error: failed to read {path}: {err}");
            return ExitCode::FAILURE;
        }
    };

    let report = match verify_attestation(&json) {
        Ok(report) => report,
        Err(err) => {
            eprintln!("error: malformed attestation: {err}");
            return ExitCode::FAILURE;
        }
    };

    let mark = |ok: bool| if ok { "PASS" } else { "FAIL" };

    println!("Attestation: {path}");
    println!("  session_id     : {}", report.session_id);
    println!("  public_key     : {}", report.public_key);
    println!("  operations     : {}", report.entry_count);
    println!("  bytes egressed : {}", report.bytes_egressed);
    println!();
    println!(
        "  [{}] hash chain complete and unaltered",
        mark(report.chain_ok)
    );
    println!(
        "  [{}] Merkle root commits to every operation",
        mark(report.merkle_ok)
    );
    println!(
        "  [{}] Ed25519 seal signed by the session key",
        mark(report.signature_ok)
    );
    println!();
    println!(
        "Note: this proves the record is intact and self-signed. It does NOT \n\
         prove that no other software on the machine sent data elsewhere."
    );

    if report.chain_ok && report.merkle_ok && report.signature_ok {
        println!("\nRESULT: verified");
        ExitCode::SUCCESS
    } else {
        println!("\nRESULT: verification FAILED");
        ExitCode::FAILURE
    }
}
