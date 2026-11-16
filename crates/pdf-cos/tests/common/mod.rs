//! Gemeinsame Test-Hilfen für `pida-pdf-cos`.
#![allow(dead_code)]

pub mod exercise;

use std::path::{Path, PathBuf};

/// Wurzel der gemeinsamen Testdaten.
pub fn testdata() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata"))
}

/// Pfad einer Datei unter `testdata/pdf/…`.
pub fn pdf(rel: &str) -> PathBuf {
    testdata().join("pdf").join(rel)
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
            out.push(p);
        }
    }
}

/// Alle PDFs des gemeinsamen Korpus (`testdata/pdf/`) und der eigenen Ordner von Paket A.
pub fn corpus() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for sub in ["pdf", "pdf-cos", "pdf-incremental"] {
        collect(&testdata().join(sub), &mut out);
    }
    out
}

/// Relativer Name für Meldungen.
pub fn rel(p: &Path) -> String {
    p.strip_prefix(testdata()).unwrap_or(p).to_string_lossy().replace('\\', "/")
}
