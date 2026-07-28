use oxih5::FileWriter;
use std::env;

fn main() {
    let n: usize = env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(2000);
    let path = env::args().nth(2).unwrap_or_else(|| "out.h5".to_string());
    // Same shape as scen_vlen_big: each string ~ 80+ chars
    let strings: Vec<String> = (0..n)
        .map(|i| format!("string_number_{i:06}_{}", "x".repeat(80)))
        .collect();
    let refs: Vec<&str> = strings.iter().map(|s| s.as_str()).collect();
    let mut w = FileWriter::new();
    w.create_vlen_string_dataset("big", &refs).unwrap();
    w.build(&path).unwrap();
    // Report the GCOL size by scanning for GCOL signature.
    let bytes = std::fs::read(&path).unwrap();
    if let Some(pos) = bytes.windows(4).position(|w| w == b"GCOL") {
        let sz = u64::from_le_bytes(bytes[pos+8..pos+16].try_into().unwrap());
        eprintln!("n={n} file={} gcol_at={pos} gcol_size={sz} last16={:02x?}",
            path, &bytes[pos + sz as usize - 16 .. pos + sz as usize]);
    }
    println!("wrote n={n} -> {path}");
}
