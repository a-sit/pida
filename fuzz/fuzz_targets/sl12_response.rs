//! Fuzz-Ziel `sl12_response`: beliebige Bytes als Antwort der Signaturstelle durch den Security-Layer-Parser
//! (`is_sl_response`, `parse_response`) und die Auswertung der gelesenen Antwort.
#![no_main]

use libfuzzer_sys::fuzz_target;
use pida_sl12::{SlResponse, is_sl_response, parse_response};

fuzz_target!(|data: &[u8]| {
    let _ = is_sl_response(data);
    match parse_response(data) {
        Ok(SlResponse::InfoboxRead(r)) => {
            let _ = r.signature_certificate();
        }
        Ok(SlResponse::Error(e)) => {
            let _ = e.is_user_cancel();
        }
        Ok(_) | Err(_) => {}
    }
});
