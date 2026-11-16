//! Fuzz-Ziel `open`: beliebige Bytes durch `SourcePdf::open` und alle lesenden Auswertungen
//! (Seiten, Felder, Inhaltsströme, Bilder, Filter, Verschlüsselung). Fehler sind erlaubt; Panics, Hänger und
//! übermäßiger Speicherverbrauch sind Funde.
#![no_main]

use libfuzzer_sys::fuzz_target;
use pida_pdf_cos::Limits;

#[path = "../../tests/common/exercise.rs"]
mod exercise;

/// Enge Grenzen: Bomben fallen schnell auf, ein Durchlauf bleibt kurz und im Speicherlimit von libFuzzer.
fn limits() -> Limits {
    Limits {
        max_file_size: 4 * 1024 * 1024,
        max_objects: 50_000,
        max_decoded_stream: 8 * 1024 * 1024,
        ..Limits::default()
    }
}

fuzz_target!(|data: &[u8]| {
    exercise::exercise(data.to_vec(), &limits());
});
