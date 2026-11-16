//! Robustheit gegen feindliche bzw. kaputte Eingaben (Plan §7.7, §8.4): deterministisches
//! Mutations-Fuzzing über den Korpus und gezielte Laufzeit-/Speicherfallen.
//!
//! Jede öffentliche Auswertung von [`SourcePdf`] muss für beliebige Bytes ohne Panic und in
//! begrenzter Zeit zurückkehren (Fehler sind erlaubt). Standardmäßig wenige Mutationen je Datei;
//! `PIDA_FUZZ_ITERS=<n>` erhöht die Zahl für längere Läufe.

mod common;

use std::io::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::{Duration, Instant};

use common::exercise::exercise;
use pida_pdf_cos::{CosError, Limits, SourcePdf};

/// xorshift64* – reproduzierbar, ohne Abhängigkeit.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
}

const TOKENS: &[&[u8]] = &[
    b"<<",
    b">>",
    b"[",
    b"]",
    b" R ",
    b" obj ",
    b"endobj",
    b"stream\n",
    b"endstream",
    b"%%EOF\n",
    b"startxref\n0\n",
    b"startxref\n9\n%%EOF",
    b"xref\n0 1\n",
    b"trailer",
    b" 1 0 R ",
    b" 4294967295 65535 R ",
    b"/Length 999999999",
    b"/Kids [1 0 R]",
    b"/Parent 1 0 R",
    b"/Filter /FlateDecode",
    b"/DecodeParms << /Predictor 15 /Columns 4294967295 /Colors 255 >>",
    b"/Width 65535 /Height 65535",
    b"/BitsPerComponent 16",
    b"/ColorSpace [/Indexed /DeviceRGB 255 <00>]",
    b"/Type /XRef /W [8 8 8] /Index [0 4294967295]",
    b"/XRefStm 0",
    b"/Prev 0",
    b"-99999999999999999999",
    b"1.#INF",
    b"BI ID EI",
    b"q q q Q cm Do",
    b"(\\",
    b"<FEFF",
];

fn mutate(rng: &mut Rng, data: &[u8]) -> Vec<u8> {
    let mut d = data.to_vec();
    let rounds = 1 + rng.below(4);
    for _ in 0..rounds {
        if d.is_empty() {
            d.extend_from_slice(b"%PDF-1.4\n");
        }
        match rng.below(7) {
            0 => {
                // Bits kippen
                for _ in 0..1 + rng.below(16) {
                    let i = rng.below(d.len());
                    d[i] ^= 1 << rng.below(8);
                }
            }
            1 => {
                // Bereich löschen
                let a = rng.below(d.len());
                let len = 1 + rng.below(d.len().min(4096));
                let b = (a + len).min(d.len());
                d.drain(a..b);
            }
            2 => {
                // Token einfügen
                let at = rng.below(d.len() + 1);
                let t = TOKENS[rng.below(TOKENS.len())];
                d.splice(at..at, t.iter().copied());
            }
            3 => {
                // abschneiden
                let n = rng.below(d.len());
                d.truncate(n.max(16).min(d.len()));
            }
            4 => {
                // Ziffern verändern (Offsets, Längen, Objektnummern)
                for _ in 0..1 + rng.below(8) {
                    let i = rng.below(d.len());
                    if d[i].is_ascii_digit() {
                        d[i] = b'0' + rng.below(10) as u8;
                    }
                }
            }
            5 => {
                // Bereich duplizieren
                let a = rng.below(d.len());
                let b = (a + 1 + rng.below(512)).min(d.len());
                let chunk = d[a..b].to_vec();
                let at = rng.below(d.len() + 1);
                d.splice(at..at, chunk);
            }
            _ => {
                // Zufallsbytes
                let at = rng.below(d.len() + 1);
                let n = 1 + rng.below(32);
                let bytes: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
                d.splice(at..at, bytes);
            }
        }
    }
    d
}

fn fuzz_limits() -> Limits {
    // Kleinere Grenzen: Bomben fallen schneller auf, Läufe bleiben kurz.
    Limits { max_decoded_stream: 64 * 1024 * 1024, ..Limits::default() }
}

/// Einzelne Datei untersuchen: `PIDA_FUZZ_FILE=<pfad>` (z. B. eine Eingabe aus `fuzz-failures`).
#[test]
fn replay_single_file() {
    let Some(path) = std::env::var_os("PIDA_FUZZ_FILE") else { return };
    let data = std::fs::read(&path).unwrap();
    let start = Instant::now();
    let opened = exercise(data, &fuzz_limits());
    eprintln!("{}: geöffnet {opened}, {:?}", std::path::Path::new(&path).display(), start.elapsed());
}

#[test]
fn mutated_corpus_never_panics() {
    let iters: usize = std::env::var("PIDA_FUZZ_ITERS").ok().and_then(|v| v.parse().ok()).unwrap_or(6);
    let seed: u64 = std::env::var("PIDA_FUZZ_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(0x005E_EDA1_1CE5_u64);
    let limits = fuzz_limits();
    let files = common::corpus();
    assert!(!files.is_empty());
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8);
    let opened = std::sync::atomic::AtomicUsize::new(0);
    let failures: Vec<String> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|t| {
                let files = &files;
                let limits = &limits;
                let opened = &opened;
                scope.spawn(move || {
                    let mut out = Vec::new();
                    for (fi, path) in files.iter().enumerate().skip(t).step_by(threads) {
                        let data = std::fs::read(path).unwrap();
                        let mut rng = Rng(seed ^ (fi as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15));
                        for it in 0..iters {
                            let m = mutate(&mut rng, &data);
                            if let Some(dir) = std::env::var_os("PIDA_FUZZ_TRACE") {
                                // vor dem Lauf sichern: bricht der Prozess ab (Stack, Speicher),
                                // bleibt die auslösende Eingabe erhalten
                                let _ = std::fs::write(std::path::Path::new(&dir).join(format!("current-{t}.pdf")), &m);
                            }
                            let start = Instant::now();
                            let r = catch_unwind(AssertUnwindSafe(|| exercise(m.clone(), limits)));
                            let took = start.elapsed();
                            if matches!(r, Ok(true)) {
                                opened.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            }
                            let name = common::rel(path);
                            if r.is_err() || took > Duration::from_secs(20) {
                                let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("fuzz-failures");
                                let _ = std::fs::create_dir_all(&dir);
                                let f = dir.join(format!("{}-{it}.pdf", name.replace('/', "_")));
                                let _ = std::fs::write(&f, &m);
                                out.push(format!(
                                    "{name} #{it}: {} ({took:?}) → {}",
                                    if r.is_err() { "PANIC" } else { "zu langsam" },
                                    f.display()
                                ));
                            }
                        }
                    }
                    out
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
    });
    let opened = opened.into_inner();
    eprintln!("Fuzzing: {} Mutationen, {opened} davon öffnbar", files.len() * iters);
    assert!(failures.is_empty(), "{} Fehlschläge:\n{}", failures.len(), failures.join("\n"));
    // Die Mutationen sollen überwiegend öffnbare (lenient reparierte) Dokumente ergeben, sonst
    // prüft der Test nur den Fehlerpfad.
    assert!(opened * 3 >= files.len() * iters, "nur {opened} öffnbar");
}

// ------------------------------------------------------------------------------------------------
// Gezielte Fallen
// ------------------------------------------------------------------------------------------------

/// Minimaldokument mit frei wählbarem Inhalt von Objekt 4 ff. (klassische Tabelle).
fn build(objects: &[(u32, Vec<u8>)], trailer_extra: &str, tail: &[u8]) -> Vec<u8> {
    let mut buf = b"%PDF-1.5\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (n, body) in objects {
        offsets.push((*n, buf.len()));
        writeln!(buf, "{n} 0 obj").unwrap();
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\nendobj\n");
    }
    let xref = buf.len();
    let max = offsets.iter().map(|(n, _)| *n).max().unwrap_or(0);
    write!(buf, "xref\n0 {}\n0000000000 65535 f\r\n", max + 1).unwrap();
    for n in 1..=max {
        match offsets.iter().find(|(m, _)| *m == n) {
            Some((_, off)) => write!(buf, "{off:010} 00000 n\r\n").unwrap(),
            None => buf.extend_from_slice(b"0000000000 65535 f\r\n"),
        }
    }
    write!(buf, "trailer\n<< /Size {} /Root 1 0 R {trailer_extra} >>\nstartxref\n{xref}\n%%EOF\n", max + 1).unwrap();
    buf.extend_from_slice(tail);
    buf
}

fn flate(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut v = format!("<< /Length {} {dict} >>\nstream\n", data.len()).into_bytes();
    v.extend_from_slice(data);
    v.extend_from_slice(b"\nendstream");
    v
}

fn timed<T>(what: &str, max: Duration, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let r = f();
    let took = start.elapsed();
    assert!(took < max, "{what}: {took:?} (Grenze {max:?})");
    r
}

/// Viele `%%EOF`-Marken, die alle auf dieselbe große xref-Tabelle zeigen: die Revisionsanalyse
/// darf die Tabelle nicht für jede Marke neu einlesen (quadratische Laufzeit).
#[test]
fn many_eof_markers_are_linear() {
    let mut objs = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (3, b"<< /Type /Page /Parent 2 0 R >>".to_vec()),
    ];
    // große Tabelle: 60 000 Objekte
    for n in 4..60_000u32 {
        objs.push((n, b"null".to_vec()));
    }
    let mut pdf = build(&objs, "", b"");
    let xref = {
        let p = pdf.windows(10).rposition(|w| w == b"startxref\n").unwrap() + 10;
        let e = p + pdf[p..].iter().position(|&b| b == b'\n').unwrap();
        String::from_utf8(pdf[p..e].to_vec()).unwrap()
    };
    // 50 000 weitere Marken mit gültigem startxref auf dieselbe Tabelle
    let marker = format!("\nstartxref\n{xref}\n%%EOF\n");
    for _ in 0..50_000 {
        pdf.extend_from_slice(marker.as_bytes());
    }
    let s = timed("viele %%EOF", Duration::from_secs(20), || SourcePdf::open(pdf, &Limits::default()));
    let s = s.unwrap();
    assert_eq!(s.page_count(), 1);
}

/// `/Contents` mit vielen Verweisen auf denselben großen Stream: die Summe ist begrenzt.
#[test]
fn page_content_total_is_bounded() {
    let big = flate(&vec![b' '; 4 * 1024 * 1024]);
    let refs = "4 0 R ".repeat(64);
    let objs = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (3, format!("<< /Type /Page /Parent 2 0 R /Contents [{refs}] >>").into_bytes()),
        (4, stream("/Filter /FlateDecode", &big)),
    ];
    let pdf = build(&objs, "", b"");
    let limits = Limits { max_decoded_stream: 32 * 1024 * 1024, ..Limits::default() };
    let s = SourcePdf::open(pdf, &limits).unwrap();
    let page = s.pages()[0].object_id;
    match s.page_content(page) {
        Err(CosError::LimitExceeded { .. }) => {}
        Ok(c) => panic!("unbegrenzter Seiteninhalt: {} Bytes", c.len()),
        Err(e) => panic!("{e}"),
    }
    assert!(matches!(s.image_invocations(0, false), Err(CosError::LimitExceeded { .. })));
}

/// Form-XObjects mit Auffächerung (jede Form ruft die nächste zweimal auf): ohne Budget wäre die
/// Laufzeit exponentiell in der Tiefe.
#[test]
fn form_fan_out_is_bounded() {
    let mut objs = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (3, b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /XObject << /F 10 0 R >> >> >>".to_vec()),
        (4, stream("", b"/F Do")),
        (
            9,
            stream(
                "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8",
                &[0],
            ),
        ),
    ];
    let depth = 40u32;
    for i in 0..depth {
        let n = 10 + i;
        let next = if i + 1 == depth { "/Im 9 0 R".to_string() } else { format!("/F {} 0 R", n + 1) };
        let body = if i + 1 == depth { b"/Im Do /Im Do".to_vec() } else { b"/F Do /F Do /F Do".to_vec() };
        objs.push((
            n,
            stream(
                &format!("/Type /XObject /Subtype /Form /BBox [0 0 1 1] /Resources << /XObject << {next} >> >>"),
                &body,
            ),
        ));
    }
    let pdf = build(&objs, "", b"");
    let s = SourcePdf::open(pdf, &Limits::default()).unwrap();
    let r = timed("Form-Auffächerung", Duration::from_secs(20), || s.image_invocations(0, true));
    assert!(matches!(r, Err(CosError::LimitExceeded { .. })), "{r:?}");
    // ohne Abstieg: kein Bild auf der Seite selbst
    assert!(s.image_invocations(0, false).unwrap().is_empty());
}

/// Sehr viele Bild-`Do`-Aufrufe: die Ergebnisliste ist begrenzt (Speicher).
#[test]
fn image_invocation_count_is_bounded() {
    let content = flate(&b"/Im Do ".repeat(3_000_000));
    let objs = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (3, b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /XObject << /Im 5 0 R >> >> >>".to_vec()),
        (4, stream("/Filter /FlateDecode", &content)),
        (
            5,
            stream(
                "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8",
                &[0],
            ),
        ),
    ];
    let pdf = build(&objs, "", b"");
    let s = SourcePdf::open(pdf, &Limits::default()).unwrap();
    let r = timed("viele Do", Duration::from_secs(30), || s.image_invocations(0, false));
    assert!(matches!(r, Err(CosError::LimitExceeded { .. })), "{:?}", r.map(|v| v.len()));
}

/// Viele Streams ohne `endstream` (falsche `/Length`): die Suche nach `endstream` darf nicht für
/// jedes Objekt den Rest der Datei durchlaufen.
#[test]
fn many_streams_without_endstream_are_not_quadratic() {
    let mut objs = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (3, b"<< /Type /Page /Parent 2 0 R >>".to_vec()),
    ];
    for n in 4..40_000u32 {
        objs.push((n, b"<< /Length 999999999 >>\nstream\nabc".to_vec()));
    }
    let pdf = build(&objs, "", b"");
    let s = timed("Streams ohne endstream", Duration::from_secs(30), || SourcePdf::open(pdf, &Limits::default()));
    if let Ok(s) = s {
        assert_eq!(s.page_count(), 1);
    }
}

/// JPEG-Bild mit `/SMask`, deren Pixelzahl nicht zu den Maßen passt, darf nicht paniken.
#[test]
fn smask_with_mismatching_sizes() {
    let objs = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (3, b"<< /Type /Page /Parent 2 0 R >>".to_vec()),
        (
            4,
            stream(
                "/Type /XObject /Subtype /Image /Width 3 /Height 3 /ColorSpace /DeviceRGB /BitsPerComponent 8 /SMask 5 0 R",
                &[0u8; 5],
            ),
        ),
        (
            5,
            stream(
                "/Type /XObject /Subtype /Image /Width 7 /Height 2 /ColorSpace /DeviceGray /BitsPerComponent 8",
                &[1],
            ),
        ),
    ];
    let pdf = build(&objs, "", b"");
    let s = SourcePdf::open(pdf, &Limits::default()).unwrap();
    let img = s.decode_image((4, 0)).unwrap();
    assert_eq!((img.width, img.height, img.data.len()), (3, 3, 36));
}

// ------------------------------------------------------------------------------------------------
// Predictor-Parameter (lopdf reserviert die Zeile ungeprüft: Columns · Colors · Bits / 8)
// ------------------------------------------------------------------------------------------------

const INSANE_PARMS: &str = "/DecodeParms << /Predictor 15 /Columns 4294967295 /Colors 255 >>";

/// Object Stream mit unzulässigen Parametern: lopdf darf ihn beim Laden nicht dekodieren.
#[test]
fn insane_predictor_in_object_stream_is_never_decoded() {
    let objstm = flate(b"5 0 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
    // a) nur als Objekt an einem Offset (lopdf dekodiert jeden Stream mit /Type /ObjStm)
    let objs = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (3, b"<< /Type /Page /Parent 2 0 R >>".to_vec()),
        (6, stream(&format!("/Type /ObjStm /N 1 /First 4 /Filter /FlateDecode {INSANE_PARMS}"), &objstm)),
    ];
    let pdf = build(&objs, "", b"");
    let s = timed("ObjStm am Offset", Duration::from_secs(10), || SourcePdf::open(pdf, &Limits::default()));
    let s = s.unwrap();
    assert_eq!(s.page_count(), 1);
    assert!(matches!(s.decoded_stream((6, 0)), Err(CosError::LimitExceeded { .. })));

    // b) als Container komprimierter Objekte (Hybrid: Tabelle + xref-Stream mit Typ-2-Eintrag)
    let mut objs = objs;
    objs.push((7, Vec::new())); // Platzhalter, unten ersetzt
    let mut pdf = build(&objs[..4], "", b"");
    // xref-Stream (unkomprimiert, W [1 2 1]): Objekt 5 → Typ 2 in Container 6
    pdf.truncate(pdf.windows(4).rposition(|w| w == b"xref").unwrap());
    let objstm_off = pdf.windows(8).rposition(|w| w == b"6 0 obj\n").unwrap();
    let xs_off = pdf.len();
    let rows = [2u8, 0, 6, 0];
    pdf.extend_from_slice(
        format!("7 0 obj\n<< /Type /XRef /Size 8 /W [1 2 1] /Index [5 1] /Length {} >>\nstream\n", rows.len())
            .as_bytes(),
    );
    pdf.extend_from_slice(&rows);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");
    let offsets: Vec<usize> =
        (1..=3u32).map(|n| pdf.windows(8).position(|w| w == format!("{n} 0 obj\n").as_bytes()).unwrap()).collect();
    let xref = pdf.len();
    pdf.extend_from_slice(b"xref\n0 4\n0000000000 65535 f\r\n");
    for o in offsets {
        pdf.extend_from_slice(format!("{o:010} 00000 n\r\n").as_bytes());
    }
    pdf.extend_from_slice(format!("6 2\n{objstm_off:010} 00000 n\r\n{xs_off:010} 00000 n\r\n").as_bytes());
    pdf.extend_from_slice(
        format!("trailer\n<< /Size 8 /Root 1 0 R /XRefStm {xs_off} >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    let r = timed("ObjStm-Container", Duration::from_secs(10), || SourcePdf::open(pdf, &Limits::default()));
    assert!(matches!(r, Err(CosError::LimitExceeded { .. })), "{r:?}");
}

/// xref-Stream mit unzulässigen Parametern (unverschlüsselt und AES-256): Ablehnung vor lopdf.
#[test]
fn insane_predictor_in_xref_stream_is_rejected() {
    for rel in ["generated/simple-xref-stream.pdf", "generated/simple-aes256-owner-xref-stream.pdf"] {
        let mut data = std::fs::read(common::pdf(rel)).unwrap();
        let at = data.windows(10).rposition(|w| w == b"/Columns 4").unwrap();
        data.splice(at..at + 10, b"/Columns 4294967295 /Colors 255".iter().copied());
        let r = timed(rel, Duration::from_secs(10), || SourcePdf::open(data, &Limits::default()));
        assert!(matches!(r, Err(CosError::LimitExceeded { .. })), "{rel}: {r:?}");
    }
}

/// Seiteninhalt bzw. Bild mit unzulässigen Parametern: Fehler statt Speicheranforderung.
#[test]
fn insane_predictor_in_content_and_image() {
    let objs = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec()),
        (3, b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /XObject << /Im 5 0 R >> >> >>".to_vec()),
        (4, stream(&format!("/Filter /FlateDecode {INSANE_PARMS}"), &flate(b"\x00/Im Do"))),
        (
            5,
            stream(
                &format!(
                    "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode {INSANE_PARMS}"
                ),
                &flate(&[0, 0]),
            ),
        ),
    ];
    let s = SourcePdf::open(build(&objs, "", b""), &Limits::default()).unwrap();
    let page = s.pages()[0].object_id;
    assert!(matches!(s.page_content(page), Err(CosError::LimitExceeded { .. })));
    assert!(matches!(s.image_invocations(0, true), Err(CosError::LimitExceeded { .. })));
    assert!(matches!(s.decode_image((5, 0)), Err(CosError::LimitExceeded { .. })));
    assert!(matches!(s.decoded_stream((4, 0)), Err(CosError::LimitExceeded { .. })));
}
