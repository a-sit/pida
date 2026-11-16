//! Fuzz-Lauf (Welle 3, T2 Punkt 7): mutierte Dateien des **gesamten** Korpus (`testdata/pdf`) durch
//! `pdf-cos::SourcePdf::open` → `pades::analyze` → `plan_signature` (ohne Renderer) → `prepare`
//! (unsichtbar und sichtbar). Erwartet: kein Panic und kein Hänger (ein Fall länger als
//! [`HANG_LIMIT`] gilt als Hänger); Fehler sind erlaubt.
//!
//! Standard: 4 Mutationen je Datei (schnell). Langer Lauf in Release mit mehreren Threads:
//! `PIDA_FUZZ_PREPARE_ITERS=3300 cargo test --release -p pida-pades --test fuzz_prepare -- --nocapture`
//! (61 Dateien × 3300 ≈ 200 000 Mutationen). `PIDA_FUZZ_SEED=<u64>` ändert den Startwert,
//! `PIDA_FUZZ_DUMP=<dir>` legt die Eingabe eines Funds ab, `PIDA_FUZZ_THREADS=<n>` setzt die
//! Anzahl der Threads.

mod common;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::fuzz_case::run_case;
use common::*;
use pida_backend_keystore::LoadedKey;
use pida_pdf_cos::Limits;

/// Ein einzelner Fall darf (Release) höchstens so lange dauern.
const HANG_LIMIT: Duration = Duration::from_secs(30);

#[derive(Clone)]
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

const KEYWORDS: [&[u8]; 16] = [
    b"/Annots",
    b"/Kids",
    b"/Fields",
    b"/AcroForm",
    b"/Count",
    b"/Rotate",
    b"/StructTreeRoot",
    b"/Length",
    b"/Filter",
    b"/Prev",
    b"/XRefStm",
    b"/Encrypt",
    b"/Parent",
    b"/ObjStm",
    b"/MediaBox",
    b"/Resources",
];

/// Mutationen: Bytes, Ziffern, Abschneiden, Müll einfügen, Schlüsselwörter verfälschen, Bereiche
/// kopieren/löschen, Zahlen durch Extremwerte ersetzen, Strukturzeichen tauschen.
fn mutate(rng: &mut Rng, data: &[u8]) -> Vec<u8> {
    let mut d = data.to_vec();
    if d.is_empty() {
        return d;
    }
    for _ in 0..1 + rng.below(3) {
        let len = d.len().max(1);
        match rng.below(9) {
            0 => {
                for _ in 0..1 + rng.below(8) {
                    let i = rng.below(d.len());
                    d[i] = rng.next() as u8;
                }
            }
            1 => {
                for _ in 0..1 + rng.below(4) {
                    let i = rng.below(d.len());
                    if let Some(j) = d[i..].iter().position(u8::is_ascii_digit) {
                        d[i + j] = b'0' + (rng.next() % 10) as u8;
                    }
                }
            }
            2 => d.truncate(len * (30 + rng.below(70)) / 100),
            3 => {
                let i = rng.below(d.len());
                let junk: Vec<u8> = (0..1 + rng.below(64)).map(|_| rng.next() as u8).collect();
                d.splice(i..i, junk);
            }
            4 => {
                for kw in KEYWORDS {
                    if rng.below(4) == 0
                        && let Some(p) = d.windows(kw.len()).position(|w| w == kw)
                    {
                        d[p + 1 + rng.below(kw.len() - 1)] = b'X';
                    }
                }
            }
            5 => {
                // Bereich an andere Stelle kopieren (Objekte doppelt, verschobene Offsets)
                let a = rng.below(d.len());
                let n = (1 + rng.below(512)).min(d.len() - a);
                let chunk = d[a..a + n].to_vec();
                let at = rng.below(d.len());
                d.splice(at..at, chunk);
            }
            6 => {
                let a = rng.below(d.len());
                let n = (1 + rng.below(256)).min(d.len() - a);
                d.drain(a..a + n);
            }
            7 => {
                // eine Zahl durch einen Extremwert ersetzen
                let i = rng.below(d.len());
                if let Some(j) = d[i..].iter().position(u8::is_ascii_digit) {
                    let start = i + j;
                    let end = start + d[start..].iter().take_while(|b| b.is_ascii_digit()).count();
                    let v: &[u8] = match rng.below(5) {
                        0 => b"0",
                        1 => b"-1",
                        2 => b"2147483647",
                        3 => b"99999999999999999999",
                        _ => b"4294967296",
                    };
                    d.splice(start..end, v.iter().copied());
                }
            }
            _ => {
                // Strukturzeichen tauschen
                let i = rng.below(d.len());
                if let Some(j) = d[i..].iter().position(|b| b"<>[]()/".contains(b)) {
                    d[i + j] = b"<>[]()/ "[rng.below(8)];
                }
            }
        }
        if d.is_empty() {
            break;
        }
    }
    d
}

/// Laufender Fall eines Threads: Start, Bezeichnung, Eingabe.
type Running = (Instant, String, Arc<Vec<u8>>);

fn dump(data: &[u8], what: &str) {
    if let Some(dir) = std::env::var_os("PIDA_FUZZ_DUMP") {
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let _ = std::fs::write(std::path::PathBuf::from(dir).join(format!("{n}-{what}.pdf")), data);
    }
}

#[test]
fn fuzz_open_analyze_prepare() {
    let iters: usize = std::env::var("PIDA_FUZZ_PREPARE_ITERS").ok().and_then(|v| v.parse().ok()).unwrap_or(4);
    let seed = std::env::var("PIDA_FUZZ_SEED").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0x0f0f_2026_1015);
    let threads = std::env::var("PIDA_FUZZ_THREADS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(2, |n| n.get()).min(8))
        .max(1);
    let p12 = std::fs::read(key_p12("test-ecp256")).unwrap();
    let key = LoadedKey::open_pkcs12(&p12, "test", None).unwrap();
    let files: Vec<(String, Vec<u8>)> = corpus().iter().map(|p| (rel(p), std::fs::read(p).unwrap())).collect();
    assert!(files.len() > 40, "Korpus unvollständig: {}", files.len());
    // Panics nicht auf stderr ausgeben (der Fund wird gesammelt und am Ende gemeldet)
    let quiet = std::env::var_os("PIDA_FUZZ_VERBOSE").is_none();
    if quiet {
        std::panic::set_hook(Box::new(|_| {}));
    }

    let total = files.len() * iters;
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let panics: Mutex<Vec<String>> = Mutex::new(Vec::new());
    // je Thread: (Start, Bezeichnung, Eingabe) des laufenden Falls
    let running: Vec<Mutex<Option<Running>>> = (0..threads).map(|_| Mutex::new(None)).collect();
    let started = Instant::now();

    std::thread::scope(|scope| {
        for slot in &running {
            let (files, key, next, done, stop, panics) = (&files, &key, &next, &done, &stop, &panics);
            scope.spawn(move || {
                loop {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let n = next.fetch_add(1, Ordering::Relaxed);
                    if n >= total {
                        break;
                    }
                    let (rel, data) = &files[n % files.len()];
                    let iteration = n / files.len();
                    // reproduzierbar je (Datei, Iteration), unabhängig von der Thread-Verteilung
                    let mut rng = Rng((seed ^ (n as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)).max(1));
                    for _ in 0..4 {
                        rng.next();
                    }
                    let m = Arc::new(mutate(&mut rng, data));
                    let label = format!("{rel} Mutation {iteration}");
                    *slot.lock().unwrap() = Some((Instant::now(), label.clone(), Arc::clone(&m)));
                    let input = (*m).clone();
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        run_case(input, key, &Limits::default())
                    }));
                    *slot.lock().unwrap() = None;
                    if let Err(payload) = r {
                        dump(&m, "panic");
                        let msg = payload
                            .downcast_ref::<String>()
                            .cloned()
                            .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                            .unwrap_or_default();
                        panics.lock().unwrap().push(format!("{label}: {msg}"));
                    }
                    done.fetch_add(1, Ordering::Relaxed);
                }
            });
        }
        // Wächter: meldet Hänger und den Fortschritt
        let mut last_report = Instant::now();
        while done.load(Ordering::Relaxed) < total && !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(200));
            for slot in &running {
                if let Some((t, label, data)) = &*slot.lock().unwrap()
                    && t.elapsed() > HANG_LIMIT
                {
                    dump(data, "hang");
                    panics.lock().unwrap().push(format!("Hänger (> {HANG_LIMIT:?}): {label}"));
                    stop.store(true, Ordering::Relaxed);
                }
            }
            if last_report.elapsed() > Duration::from_secs(30) {
                last_report = Instant::now();
                eprintln!("{} / {total} Mutationen nach {:?}", done.load(Ordering::Relaxed), started.elapsed());
            }
            if stop.load(Ordering::Relaxed) {
                // Hänger: Threads nicht abwarten (der hängende Fall endet nie; `scope` würde sie
                // auch bei einem Panic joinen), sondern den Testprozess mit Fehler beenden
                let found = panics.lock().unwrap().clone();
                eprintln!("Fuzz-Fund: {found:?}");
                std::process::exit(101);
            }
        }
    });
    if quiet {
        let _ = std::panic::take_hook();
    }
    let found = panics.into_inner().unwrap();
    eprintln!("{total} Mutationen in {:?} ({threads} Threads), {} Funde", started.elapsed(), found.len());
    assert!(found.is_empty(), "Panics: {found:?}");
}
