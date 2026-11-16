//! Gemeinsame Test-Hilfen für `pida-pades`: Korpus, Keystore-Backend (Modell A), Fake-BKU
//! (Modell B), externe Orakel (qpdf, openssl, pyHanko, PDFium).
//!
//! Externe Werkzeuge sind optional: fehlen sie, wird die jeweilige Prüfung übersprungen (Meldung
//! auf stderr). Kein Netzverkehr (pyHanko ohne Fetcher). Testschlüssel: `testdata/keystores`
//! (selbstsigniert, Passwort `test`, NUR TEST).
#![allow(dead_code)]

pub mod cases;
pub mod fuzz_case;
pub mod oracles;
#[allow(unused_imports)]
pub use cases::*;
#[allow(unused_imports)]
pub use oracles::*;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};

use async_trait::async_trait;
use pida_backend_api::testing::ScriptedBroker;
use pida_backend_api::{
    BackendError, BackendId, BackendSession, CancelToken, Capabilities, CertChain, DigestJob, DocumentJob,
    DocumentSource, Environment, ExcludedByteRange, InteractionBroker, SessionContext, SignJob, SignResult,
    SignatureBackend,
};
use pida_backend_keystore::{KeystoreBackend, KeystoreConfig};
use pida_cades::{Certificate, CmsProfile, SignedAttrsBuilder, SigningTime, select_algorithm};
use pida_pades::{
    DocReport, FieldTarget, PlanOptions, PlannedSignature, Prepared, SignedPdf, cert_fields, plan_signature, prepare,
};
use pida_pdf_cos::{Limits, SourcePdf};
use pida_render::{PdfiumBackend, RenderBackend, Renderer};
use pida_sigblock::{FontProvider, ProfileStore, SigBlockInput};
use zeroize::Zeroizing;

/// Wurzel der gemeinsamen Testdaten.
pub fn testdata() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata"))
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

/// Alle PDFs unter `testdata/pdf/`.
pub fn corpus() -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect(&testdata().join("pdf"), &mut out);
    out
}

/// Relativer Name für Meldungen.
pub fn rel(p: &Path) -> String {
    p.strip_prefix(testdata()).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

/// Sucht ein Programm im `PATH`.
pub fn which(cmd: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for name in [cmd.to_string(), format!("{cmd}.exe")] {
            let p = dir.join(&name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Python mit pyHanko: `PIDA_PYTHON`, sonst `python3` neben `pyhanko`, sonst `python3`.
pub fn pyhanko_python() -> Option<PathBuf> {
    static PY: OnceLock<Option<PathBuf>> = OnceLock::new();
    PY.get_or_init(|| {
        let mut candidates = Vec::new();
        if let Some(p) = std::env::var_os("PIDA_PYTHON") {
            candidates.push(PathBuf::from(p));
        }
        if let Some(ph) = which("pyhanko")
            && let Some(dir) = ph.parent()
        {
            candidates.push(dir.join("python3"));
        }
        if let Some(p) = which("python3") {
            candidates.push(p);
        }
        candidates.into_iter().find(|p| {
            p.is_file()
                && Command::new(p)
                    .args(["-I", "-c", "import pyhanko.sign.validation"])
                    .output()
                    .is_ok_and(|o| o.status.success())
        })
    })
    .clone()
}

/// PDFium (nur mit `PIDA_PDFIUM_DIR`, sonst `None`).
pub fn pdfium() -> Option<PdfiumBackend> {
    static B: OnceLock<Option<PdfiumBackend>> = OnceLock::new();
    *B.get_or_init(|| {
        let dir = std::env::var_os("PIDA_PDFIUM_DIR")?;
        match PdfiumBackend::bind(Some(Path::new(&dir))) {
            Ok(b) => Some(b),
            Err(e) => {
                eprintln!("HINWEIS: PDFium nicht ladbar ({e}) – Prüfungen mit PDFium übersprungen");
                None
            }
        }
    })
}

/// Profile und Fonts (einmal geladen).
pub fn store() -> &'static ProfileStore {
    static S: OnceLock<ProfileStore> = OnceLock::new();
    S.get_or_init(|| ProfileStore::builtin().expect("eingebaute Profile"))
}

pub fn fonts() -> &'static FontProvider {
    static F: OnceLock<FontProvider> = OnceLock::new();
    F.get_or_init(FontProvider::builtin)
}

// ------------------------------------------------------------------------------------------------
// Externe Werkzeuge
// ------------------------------------------------------------------------------------------------

/// Ergebnis von `qpdf --check`.
#[derive(Debug, Clone)]
pub struct QpdfResult {
    pub code: i32,
    pub messages: Vec<String>,
}

pub fn qpdf_check(qpdf: &Path, file: &Path) -> QpdfResult {
    let out = Command::new(qpdf).arg("--check").arg(file).output().expect("qpdf startet");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let fname = file.to_string_lossy().to_string();
    let messages = text
        .lines()
        .filter(|l| {
            let l = l.to_ascii_lowercase();
            l.contains("warning") || l.contains("error")
        })
        .filter(|l| !l.contains("No syntax or stream encoding errors") && !l.contains("errors that qpdf cannot detect"))
        .map(|l| l.replace(&fname, "<datei>"))
        .map(|l| l.split(": ").last().unwrap_or(&l).to_string())
        .collect();
    QpdfResult { code: out.status.code().unwrap_or(-1), messages }
}

/// Ein Eintrag aus `pyhanko_report.py`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct PyhankoSig {
    pub field: Option<String>,
    pub intact: bool,
    pub valid: bool,
    pub trusted: bool,
    pub coverage: Option<String>,
    pub modification_level: Option<String>,
    pub docmdp_ok: Option<bool>,
    pub error: Option<String>,
}

/// pyHanko über mehrere Dateien (ein Python-Start).
pub fn pyhanko_batch(
    python: &Path,
    files: &[PathBuf],
    trust: &[PathBuf],
) -> Result<std::collections::HashMap<String, Vec<PyhankoSig>>, String> {
    let script = testdata().join("pades/pyhanko_report.py");
    let mut cmd = Command::new(python);
    cmd.arg("-I").arg(&script);
    for t in trust {
        cmd.arg("--trust").arg(t);
    }
    for f in files {
        cmd.arg(f);
    }
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("pyHanko-Skript fehlgeschlagen: {}", String::from_utf8_lossy(&out.stderr)));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout.lines().rev().find(|l| l.trim_start().starts_with('{')).ok_or("keine JSON-Ausgabe")?;
    serde_json::from_str(line).map_err(|e| e.to_string())
}

/// Rangfolge der pyHanko-Änderungsstufen.
pub fn modification_rank(level: Option<&str>) -> u8 {
    match level {
        Some("NONE") => 0,
        Some("LTA_UPDATES") => 1,
        Some("FORM_FILLING") => 2,
        Some("ANNOTATIONS") => 3,
        _ => 4,
    }
}

/// `openssl cms -verify -binary -noverify` über die ByteRange-Daten der neuen Signatur.
pub fn openssl_verify(openssl: &Path, dir: &Path, signed: &SignedPdf) -> bool {
    let bytes = signed.bytes();
    let [_, a, b, c] = signed.byte_range().0;
    let hex = &bytes[a as usize + 1..b as usize - 1];
    let mut der = Vec::with_capacity(hex.len() / 2);
    for pair in hex.chunks(2) {
        der.push(u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap());
    }
    let der = pida_cades::trim_contents_padding(&der).unwrap().to_vec();
    let mut data = bytes[..a as usize].to_vec();
    data.extend_from_slice(&bytes[b as usize..(b + c) as usize]);
    let cms = dir.join("verify-cms.der");
    let content = dir.join("verify-data.bin");
    std::fs::write(&cms, &der).unwrap();
    std::fs::write(&content, &data).unwrap();
    Command::new(openssl)
        .args(["cms", "-verify", "-binary", "-inform", "DER", "-noverify", "-in"])
        .arg(&cms)
        .arg("-content")
        .arg(&content)
        .arg("-out")
        .arg(dir.join("verify.out"))
        .output()
        .is_ok_and(|o| o.status.success())
}

// ------------------------------------------------------------------------------------------------
// Schlüssel
// ------------------------------------------------------------------------------------------------

/// Test-Keystores (Anzeige, Dateistamm).
pub const KEYS: [(&str, &str); 3] =
    [("RSA-2048", "test-rsa2048"), ("P-256", "test-ecp256"), ("P-384", "test-ecp384-title")];

pub fn key_p12(stem: &str) -> PathBuf {
    testdata().join(format!("keystores/{stem}.p12"))
}

pub fn key_cert_pem(stem: &str) -> PathBuf {
    testdata().join(format!("keystores/{stem}.crt.pem"))
}

/// Tokio-Laufzeit für die asynchronen Backends.
pub fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

fn session_context() -> SessionContext {
    SessionContext { language: Default::default(), batch_size: 1, cancel: CancelToken::new(), headless: false }
}

/// Offene Keystore-Sitzung (Modell A).
pub struct KeystoreSigner {
    rt: tokio::runtime::Runtime,
    session: Box<dyn BackendSession>,
    pub chain: CertChain,
}

impl KeystoreSigner {
    pub fn open(stem: &str) -> KeystoreSigner {
        Self::open_p12(&key_p12(stem))
    }

    /// Wie [`Self::open`] für eine PKCS#12-Datei außerhalb von `testdata/keystores` (Passwort `test`).
    pub fn open_p12(p12: &Path) -> KeystoreSigner {
        let rt = runtime();
        let mut cfg = KeystoreConfig::new(p12.to_path_buf());
        cfg.store_password = Some(Zeroizing::new("test".into()));
        let backend = KeystoreBackend::new(cfg);
        let broker: Arc<dyn InteractionBroker> = Arc::new(ScriptedBroker::new([]));
        let mut session = rt.block_on(backend.open(session_context(), broker)).expect("Keystore öffnen");
        let chain = rt.block_on(session.certificate()).expect("Zertifikat");
        KeystoreSigner { rt, session, chain }
    }

    /// Modell A nach ADR 0001 §8.3: `digest(alg)` → SignedAttrs → Hash → Backend → `finish_raw`.
    pub fn sign(&mut self, prepared: Prepared) -> Result<SignedPdf, String> {
        let alg = select_algorithm(&self.chain.signer.key_type().unwrap()).unwrap().digest();
        let digest = prepared.digest(alg);
        let attrs = SignedAttrsBuilder::new(
            prepared.cms_profile(),
            &digest,
            &self.chain.signer,
            SigningTime(prepared.signing_time()),
        )
        .build()
        .map_err(|e| e.to_string())?;
        let job = SignJob::Digest(DigestJob { display_name: "test.pdf".into(), alg, digest: attrs.digest(alg).value });
        let mut results = self.rt.block_on(self.session.sign(vec![job])).map_err(|e| e.to_string())?;
        let SignResult::Raw { signature, chain } = results.remove(0) else {
            return Err("Keystore lieferte keine Rohsignatur".into());
        };
        prepared.finish_raw(&attrs, &signature, &chain).map_err(|e| format!("finish_raw ({}): {e}", e.code()))
    }

    /// Fertiges CMS dieses Schlüssels über die ByteRange-Daten von `prepared` (ohne `finish_*`) –
    /// für Negativtests (fremder Signer, Reservegrenzen).
    pub fn cms_for(&mut self, prepared: &Prepared) -> Vec<u8> {
        let alg = select_algorithm(&self.chain.signer.key_type().unwrap()).unwrap().digest();
        let digest = prepared.digest(alg);
        let attrs = SignedAttrsBuilder::new(
            prepared.cms_profile(),
            &digest,
            &self.chain.signer,
            SigningTime(prepared.signing_time()),
        )
        .build()
        .unwrap();
        let job = SignJob::Digest(DigestJob { display_name: "test.pdf".into(), alg, digest: attrs.digest(alg).value });
        let mut results = self.rt.block_on(self.session.sign(vec![job])).unwrap();
        let SignResult::Raw { signature, chain } = results.remove(0) else { panic!("keine Rohsignatur") };
        let opts = pida_cades::AssembleOptions { profile: prepared.cms_profile(), include_intermediates: false };
        pida_cades::assemble(&attrs, &signature, &chain, &opts).unwrap()
    }
}

/// Plan für ein Profil (Builtin-Store) mit Zertifikat; `renderer` = PDFium falls vorhanden.
pub fn plan_for(
    src: &SourcePdf,
    report: &DocReport,
    profile_id: &str,
    signer: &Certificate,
    opts: &PlanOptions,
    renderer: Option<&dyn Renderer>,
) -> Result<PlannedSignature, pida_pades::PadesError> {
    let base = store().profile(profile_id).expect("Profil");
    // BASE_LOGO: Laufzeit-Overlay wie PDF-Over (ohne eigenes Emblem 1×1 px, 23 mm)
    let profile = if profile_id == "BASE_LOGO" {
        base.with_overrides(&pida_sigblock::ProfileOverrides::base_logo(
            None,
            (1, 1),
            pida_sigblock::profile::DEFAULT_LOGO_SIZE_MM,
        ))
    } else {
        base.clone()
    };
    let input = SigBlockInput::new(cert_fields(signer), opts.signing_time);
    plan_signature(src, report, &profile, &input, opts, fonts(), &**store().resources(), renderer)
}

/// Öffnet ein Dokument für die Planung mit PDFium (falls vorhanden).
pub fn open_renderer(src: &SourcePdf) -> Option<Box<dyn Renderer>> {
    pdfium().and_then(|b| b.open(src.shared_bytes(), None).ok())
}

/// Bequemlichkeit: plant und bereitet vor (Modell A, Keystore-Zertifikat).
pub fn prepare_with(
    src: &SourcePdf,
    report: &DocReport,
    profile_id: &str,
    signer: &Certificate,
    chain: &CertChain,
    cms_profile: CmsProfile,
    field: FieldTarget,
) -> Result<(Prepared, PlannedSignature), pida_pades::PadesError> {
    let mut opts = PlanOptions::new(pida_pades::local_signing_time());
    opts.cms_profile = cms_profile;
    opts.field = field;
    let renderer = open_renderer(src);
    let planned = plan_for(src, report, profile_id, signer, &opts, renderer.as_deref())?;
    let prepared = prepare(src, &planned.plan, Some(chain))?;
    Ok((prepared, planned))
}

pub fn open(path: &Path) -> SourcePdf {
    SourcePdf::open_file(path, &Limits::default()).unwrap_or_else(|e| panic!("{}: öffnen: {e}", rel(path)))
}

// ------------------------------------------------------------------------------------------------
// Fake-BKU (Modell B): nimmt Blackout-PDF + ExcludedByteRange und liefert ein CMS per openssl
// ------------------------------------------------------------------------------------------------

/// `DocumentSource` um ein `Prepared` (wie der Workflow-Adapter).
pub struct PreparedSource(pub Arc<Mutex<Option<Prepared>>>);

impl std::fmt::Debug for PreparedSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PreparedSource")
    }
}

impl DocumentSource for PreparedSource {
    fn display_name(&self) -> &str {
        "test.pdf"
    }
    fn len(&self) -> u64 {
        self.0.lock().unwrap().as_ref().map_or(0, |p| p.total_len())
    }
    fn excluded_range(&self) -> ExcludedByteRange {
        let g = self.0.lock().unwrap();
        let e = g.as_ref().unwrap().excluded_range();
        ExcludedByteRange { from: e.from, to: e.to }
    }
    fn open_blacked_out(&self) -> std::io::Result<Box<dyn Read + Send + '_>> {
        let mut out = Vec::new();
        self.0.lock().unwrap().as_ref().unwrap().blacked_out().read_to_end(&mut out)?;
        Ok(Box::new(std::io::Cursor::new(out)))
    }
}

/// Signaturgerät wie eine BKU: Schlüssel/Zertifikat als PEM, `openssl cms -sign -cades` über die
/// Daten **außerhalb** der ExcludedByteRange (From/To inklusive).
#[derive(Debug, Clone)]
pub struct FakeBku {
    pub openssl: PathBuf,
    pub key_pem: PathBuf,
    pub cert_pem: PathBuf,
    /// Zertifikat, das `certificate()` meldet (darf vom signierenden abweichen – Negativtest)
    pub reported_cert: PathBuf,
    pub work: PathBuf,
}

impl FakeBku {
    /// Exportiert Schlüssel und Zertifikat aus einem Test-PKCS#12 (`None` ohne openssl).
    pub fn from_p12(stem: &str, work: &Path) -> Option<FakeBku> {
        let openssl = which("openssl")?;
        let p12 = key_p12(stem);
        let key = work.join(format!("{stem}.key.pem"));
        let cert = work.join(format!("{stem}.cert.pem"));
        let run = |args: &[&str], out: &Path| {
            Command::new(&openssl)
                .args(["pkcs12", "-in"])
                .arg(&p12)
                .args(["-passin", "pass:test"])
                .args(args)
                .arg("-out")
                .arg(out)
                .output()
                .is_ok_and(|o| o.status.success())
        };
        if !run(&["-nodes", "-nocerts"], &key) || !run(&["-nokeys", "-clcerts"], &cert) {
            eprintln!("HINWEIS: Test-PKCS#12 mit openssl nicht lesbar – Fake-BKU übersprungen");
            return None;
        }
        let bku =
            FakeBku { openssl, key_pem: key, cert_pem: cert.clone(), reported_cert: cert, work: work.to_path_buf() };
        // `cms -sign -cades` braucht OpenSSL ≥ 3.0 (LibreSSL kann es nicht)
        if let Err(e) = bku.sign_data(b"Probe") {
            eprintln!("HINWEIS: openssl kann kein CAdES-CMS erzeugen ({}) – Fake-BKU übersprungen", e.trim());
            return None;
        }
        Some(bku)
    }

    pub fn sign_data(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        let input = self.work.join("bku-data.bin");
        let output = self.work.join("bku-cms.der");
        std::fs::write(&input, data).map_err(|e| e.to_string())?;
        let o = Command::new(&self.openssl)
            .args(["cms", "-sign", "-binary", "-outform", "DER", "-nosmimecap", "-cades", "-md", "sha256", "-in"])
            .arg(&input)
            .arg("-signer")
            .arg(&self.cert_pem)
            .arg("-inkey")
            .arg(&self.key_pem)
            .arg("-out")
            .arg(&output)
            .output()
            .map_err(|e| e.to_string())?;
        if !o.status.success() {
            return Err(String::from_utf8_lossy(&o.stderr).into_owned());
        }
        std::fs::read(&output).map_err(|e| e.to_string())
    }
}

#[async_trait]
impl SignatureBackend for FakeBku {
    fn id(&self) -> BackendId {
        BackendId::LocalBku
    }
    fn capabilities(&self, _env: &Environment) -> Option<Capabilities> {
        None
    }
    async fn open(
        &self,
        _ctx: SessionContext,
        _ui: Arc<dyn InteractionBroker>,
    ) -> Result<Box<dyn BackendSession>, BackendError> {
        Ok(Box::new(FakeBkuSession(self.clone())))
    }
}

pub struct FakeBkuSession(FakeBku);

#[async_trait]
impl BackendSession for FakeBkuSession {
    async fn certificate(&mut self) -> Result<CertChain, BackendError> {
        let pem = std::fs::read_to_string(&self.0.reported_cert).map_err(|e| BackendError::Internal(e.to_string()))?;
        let cert = Certificate::from_pem(&pem).map_err(|e| BackendError::Internal(e.to_string()))?;
        Ok(CertChain::new(cert))
    }

    async fn sign(&mut self, jobs: Vec<SignJob>) -> Result<Vec<SignResult>, BackendError> {
        let mut out = Vec::new();
        for job in jobs {
            let SignJob::Document(DocumentJob { source, .. }) = job else {
                return Err(BackendError::Internal("nur Dokumente".into()));
            };
            let ex = source.excluded_range();
            let mut blacked = Vec::new();
            source
                .open_blacked_out()
                .and_then(|mut r| r.read_to_end(&mut blacked))
                .map_err(|e| BackendError::Internal(e.to_string()))?;
            // die BKU prüft, dass die Lücke genullt ist
            if blacked[ex.from as usize..=ex.to as usize].iter().any(|b| *b != 0) {
                return Err(BackendError::Protocol("ExcludedByteRange nicht genullt".into()));
            }
            let mut data = blacked[..ex.from as usize].to_vec();
            data.extend_from_slice(&blacked[ex.to as usize + 1..]);
            let cms = self.0.sign_data(&data).map_err(BackendError::Internal)?;
            out.push(SignResult::Cms(cms));
        }
        Ok(out)
    }

    async fn cancel(&mut self) {}
}

/// Modell B über die Fake-BKU: `certificate()` → Plan/`prepare` (Reserve 16 KiB) → `sign` →
/// `finish_cms` mit dem gemeldeten Zertifikat.
pub fn sign_model_b(
    bku: &FakeBku,
    src: &SourcePdf,
    report: &DocReport,
    profile_id: &str,
    reserve: pida_pades::Reserve,
) -> Result<SignedPdf, String> {
    let rt = runtime();
    let broker: Arc<dyn InteractionBroker> = Arc::new(ScriptedBroker::new([]));
    let mut session = rt.block_on(bku.open(session_context(), broker)).map_err(|e| e.to_string())?;
    let chain = rt.block_on(session.certificate()).map_err(|e| e.to_string())?;
    let mut opts = PlanOptions::new(pida_pades::local_signing_time());
    opts.reserve = reserve;
    let renderer = open_renderer(src);
    let planned =
        plan_for(src, report, profile_id, &chain.signer, &opts, renderer.as_deref()).map_err(|e| e.to_string())?;
    let prepared = prepare(src, &planned.plan, Some(&chain)).map_err(|e| e.to_string())?;
    let shared = Arc::new(Mutex::new(Some(prepared)));
    let job = SignJob::Document(DocumentJob {
        source: Arc::new(PreparedSource(Arc::clone(&shared))),
        expected_signer: Some(chain.signer.clone()),
    });
    let mut results = rt.block_on(session.sign(vec![job])).map_err(|e| e.to_string())?;
    let SignResult::Cms(cms) = results.remove(0) else { return Err("kein CMS".into()) };
    let prepared = shared.lock().unwrap().take().unwrap();
    let expect = pida_cades::Expect {
        signer: pida_cades::ExpectedSigner::Certificate(chain.signer.clone()),
        ..Default::default()
    };
    prepared.finish_cms(&cms, &expect).map_err(|e| format!("{}: {e}", e.code()))
}
