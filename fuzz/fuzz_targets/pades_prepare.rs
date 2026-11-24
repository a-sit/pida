//! Fuzz-Ziel `pades_prepare`: beliebige Bytes durch Öffnen, Analyse, Planung und Vorbereitung der Signatur
//! (unsichtbar und mit sichtbarem Block) bis zum Hash über die signierten Bereiche. Signiert wird mit dem
//! Testschlüssel aus `testdata/keystores` (nur Zertifikat und Verfahren; kein Netz, kein Renderer).
#![no_main]

use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use pida_backend_keystore::LoadedKey;
use pida_pdf_cos::Limits;

#[path = "../../crates/pades/tests/common/fuzz_case.rs"]
mod fuzz_case;

/// Testschlüssel (selbstsigniert, Passwort `test`, NUR TEST).
const KEY_P12: &[u8] = include_bytes!("../../testdata/keystores/test-ecp256.p12");

fn key() -> &'static LoadedKey {
    static K: OnceLock<LoadedKey> = OnceLock::new();
    K.get_or_init(|| LoadedKey::open_pkcs12(KEY_P12, "test", None).expect("Testschlüssel"))
}

/// Enge Grenzen wie beim Ziel `pdf_open`.
fn limits() -> Limits {
    Limits {
        max_file_size: 4 * 1024 * 1024,
        max_objects: 50_000,
        max_decoded_stream: 8 * 1024 * 1024,
        ..Limits::default()
    }
}

fuzz_target!(|data: &[u8]| {
    fuzz_case::run_case(data.to_vec(), key(), &limits());
});
