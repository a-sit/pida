//! Fuzz-Ziel `xmp`: beliebige Bytes als XMP-Paket durch die PDF/A-Erkennung (`parse_pdfa_identification`).
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = pida_pdf_cos::parse_pdfa_identification(data);
});
