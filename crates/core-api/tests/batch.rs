//! Stapel gegen den echten Kern (ADR 0004 §3.8, Paket K2; Abnahme E2, Kriterien [K] 1–7 und 9 – Punkt 8 in
//! `tests/mobile.rs`): Übersicht mit Voranalyse (Miniatur, Seiten, Größe, Status inkl. abgelehnt und
//! QR-Platzhalter), Schätzung, „Alle signieren“ mit **einer** Sitzung (eine Anmeldung, höchstens ein
//! Zertifikat, je Dokument eine Bestätigung mit eigenem Vergleichswert), Überspringen ≠ Abbrechen, Pausen je
//! Fehlerart, Zusammenfassung, Einzeldokument öffnen/zurück, vorhandene Zieldateien, D-06 ohne Übersicht.
//!
//! Das A-Trust-Verhalten bildet ein Fake-Backend nach (eine Anmeldung je Sitzung, Zugangsdaten nur in der
//! Sitzung, Abbruch der Sitzung löscht sie, je Dokument eine Bestätigung mit Vergleichswert, Fehler je Auftrag
//! nach Skript). Mit dem Keystore läuft die echte Engine; pyHanko prüft die Ergebnisse. PDFium-Fälle
//! (Miniatur, QR-Platzhalter) laufen nur mit `PIDA_PDFIUM_DIR`.

mod common;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use common::*;
use pida_backend_api::{
    BackendError, BackendEvent, BackendId, BackendSession, Capabilities, CertChain, Environment, FormAnswer,
    FormContext, InteractionBroker, InteractionError, NativeForm, Notice, NoticeAnswer, NoticeButtons, NoticeCode,
    NoticeKind, SessionContext, SignJob, SignResult, SignatureBackend,
};
use pida_backend_keystore::LoadedKey;
use pida_cades::{CmsProfile, DigestAlg, RawSigner, SigningTime};
use pida_config::Overlay;
use pida_core_api::{
    BatchItemStatusView, BatchView, CertificateSource, Core, CoreOptions, FormAnswerView, InteractionView,
    OutputModeView, PauseScope, PositionSummaryView, SigningProgressView, SigningStage, StepView, UiEvent, ViewState,
};
use pida_workflow::{DocId, ExistingPolicy, ExitCode};
use zeroize::Zeroizing;

const PDF: &str = "pdf/generated/simple-xref-table.pdf";
const TEXT_PDF: &str = "pdf/pdfas/pdf-as-legacy/simple.pdf";
const PASSWORD_PDF: &str = "pdf/generated/simple-aes256-userpw.pdf";
const QR_PDF: &str = "pdf/pdfas/pdf-as-pdfbox-2/new_qr_2-2.pdf";

fn pdfium_dir() -> Option<PathBuf> {
    std::env::var_os("PIDA_PDFIUM_DIR").filter(|d| !d.is_empty()).map(Into::into)
}

fn tool(name: &str) -> bool {
    Command::new(name).arg("--version").output().is_ok_and(|o| o.status.success())
}

// ------------------------------------------------------------------------------------------------
// ID-Austria-artiges Fake-Backend mit Skript
// ------------------------------------------------------------------------------------------------

#[derive(Debug)]
struct Atrust {
    key: Arc<LoadedKey>,
    opened: AtomicUsize,
    /// `cancel()` der Sitzung (Abbruch, F-72: Zugangsdaten gelöscht)
    cancelled: AtomicUsize,
    /// `certificate()` (InfoboxRead)
    certificates: AtomicUsize,
    /// gezeigte Login-Masken
    logins: AtomicUsize,
    /// Aufträge (`sign`, über die Standard-Implementierung von `sign_one`)
    signs: AtomicUsize,
    /// Fehler des n-ten Auftrags (1-basiert)
    failures: Mutex<HashMap<usize, BackendError>>,
    /// je Auftrag: Dokumentname und Vergleichswert der Bestätigung
    confirmations: Mutex<Vec<(String, String)>>,
    /// Meldung mit „Wiederholen“/„Abbrechen“ im nächsten Auftrag („Abbrechen“ → `Cancelled`)
    notice: Mutex<Option<NoticeCode>>,
}

impl Atrust {
    fn new(failures: Vec<(usize, BackendError)>) -> Arc<Atrust> {
        Arc::new(Atrust {
            key: test_key(),
            opened: AtomicUsize::new(0),
            cancelled: AtomicUsize::new(0),
            certificates: AtomicUsize::new(0),
            logins: AtomicUsize::new(0),
            signs: AtomicUsize::new(0),
            failures: Mutex::new(failures.into_iter().collect()),
            confirmations: Mutex::new(Vec::new()),
            notice: Mutex::new(None),
        })
    }

    /// Wie am Desktop: ID Austria und Chipkarte (nie benutzt) – mit nur einem Signiermittel gäbe es keine
    /// Signiermittel-Auswahl und kein „Jedes Mal fragen“ (ADR 0004 Nachtrag 20).
    fn backends(self: &Arc<Self>) -> Option<Vec<Arc<dyn SignatureBackend>>> {
        Some(vec![Arc::new(AtrustBackend(self.clone())), IdleBackend::card()])
    }

    fn n(&self, c: &AtomicUsize) -> usize {
        c.load(Ordering::SeqCst)
    }
}

#[derive(Debug)]
struct AtrustBackend(Arc<Atrust>);

#[async_trait]
impl SignatureBackend for AtrustBackend {
    fn id(&self) -> BackendId {
        BackendId::AtrustNative
    }
    fn capabilities(&self, env: &Environment) -> Option<Capabilities> {
        (!env.headless).then(atrust_caps)
    }
    async fn open(
        &self,
        _ctx: SessionContext,
        ui: Arc<dyn InteractionBroker>,
    ) -> Result<Box<dyn BackendSession>, BackendError> {
        self.0.opened.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(AtrustFakeSession { backend: self.0.clone(), ui, logged_in: false }))
    }
}

struct AtrustFakeSession {
    backend: Arc<Atrust>,
    ui: Arc<dyn InteractionBroker>,
    /// Zugangsdaten der Sitzung (wie `AtrustSession::credentials`)
    logged_in: bool,
}

fn interaction(e: InteractionError) -> BackendError {
    match e {
        InteractionError::Cancelled => BackendError::Cancelled,
        InteractionError::Unsupported => BackendError::Unavailable("keine Oberfläche".into()),
    }
}

impl AtrustFakeSession {
    /// eine Anmeldung je Sitzung; jede weitere Anfrage meldet automatisch an
    async fn ensure_login(&mut self) -> Result<(), BackendError> {
        if self.logged_in {
            return Ok(());
        }
        self.backend.logins.fetch_add(1, Ordering::SeqCst);
        let form = NativeForm::Login { prefill_username: None, remember: false, ctx: FormContext::default() };
        match self.ui.native_form(form).await.map_err(interaction)? {
            FormAnswer::Login { username, remember, .. } => {
                self.ui.event(BackendEvent::LoginSucceeded { username, remember });
                self.logged_in = true;
                Ok(())
            }
            _ => Err(BackendError::Protocol("unerwartet".into())),
        }
    }
}

#[async_trait]
impl BackendSession for AtrustFakeSession {
    async fn certificate(&mut self) -> Result<CertChain, BackendError> {
        self.backend.certificates.fetch_add(1, Ordering::SeqCst);
        self.ensure_login().await?;
        Ok(self.backend.key.chain().clone())
    }

    async fn sign(&mut self, jobs: Vec<SignJob>) -> Result<Vec<SignResult>, BackendError> {
        let n = self.backend.signs.fetch_add(1, Ordering::SeqCst) + 1;
        self.ensure_login().await?;
        let failure = self.backend.failures.lock().unwrap().remove(&n);
        if let Some(e) = failure {
            return Err(e);
        }
        let notice = self.backend.notice.lock().unwrap().take();
        if let Some(code) = notice {
            let notice = Notice { kind: NoticeKind::Error, code, text: None, buttons: NoticeButtons::RetryCancel };
            if self.ui.notice(notice).await.map_err(interaction)? == NoticeAnswer::Cancel {
                return Err(BackendError::Cancelled);
            }
        }
        let mut out = Vec::new();
        for job in jobs {
            let SignJob::Document(d) = job else { return Err(BackendError::Internal("nur Modell B".into())) };
            // je Dokument eine eigene Bestätigung mit eigenem Vergleichswert
            let reference = format!("V{n}");
            self.backend.confirmations.lock().unwrap().push((d.source.display_name().to_owned(), reference.clone()));
            let form = NativeForm::SmsTan { reference_value: reference, ctx: FormContext::default() };
            match self.ui.native_form(form).await.map_err(interaction)? {
                FormAnswer::Tan(t) if t.as_str() == "123456" => {}
                _ => return Err(BackendError::Protocol("TAN falsch".into())),
            }
            let bytes = d.source.blacked_out_bytes().map_err(|e| BackendError::Internal(e.to_string()))?;
            let r = d.source.excluded_range();
            let mut content = bytes[..r.from as usize].to_vec();
            content.extend_from_slice(&bytes[r.to as usize + 1..]);
            let digest = DigestAlg::Sha256.digest(&content);
            let cms = pida_cades::sign_detached(
                self.backend.key.as_ref() as &dyn RawSigner,
                &digest,
                CmsProfile::PdfasCompat,
                SigningTime(time::OffsetDateTime::now_utc()),
            )
            .map_err(|e| BackendError::Internal(e.to_string()))?;
            out.push(SignResult::Cms(cms));
        }
        Ok(out)
    }

    async fn cancel(&mut self) {
        // F-72: Passwort gelöscht – die nächste Sitzung fragt erneut
        self.logged_in = false;
        self.backend.cancelled.fetch_add(1, Ordering::SeqCst);
    }
}

// ------------------------------------------------------------------------------------------------
// Kern, Warten, Bedienen
// ------------------------------------------------------------------------------------------------

struct Opts {
    backends: Option<Vec<Arc<dyn SignatureBackend>>>,
    engine: Option<Arc<FakeEngine>>,
    pdfium: bool,
}

fn start(fx: &Fixture, overlay: Overlay, o: Opts) -> Option<Core> {
    let mut cfg = fx.config(overlay, false);
    if o.pdfium {
        cfg.pdfium_dir = Some(pdfium_dir()?);
    }
    let opts = CoreOptions {
        workspace_ui: Some(true),
        batch_overview: Some(true),
        backends: o.backends,
        engine: o.engine.map(|e| e as Arc<dyn pida_core_api::engine::PadesEngine>),
        use_pdfium: o.pdfium,
        ..CoreOptions::isolated()
    };
    Some(Core::new_with(cfg, opts).expect("Kern startet"))
}

fn fake(backends: Option<Vec<Arc<dyn SignatureBackend>>>) -> Opts {
    Opts { backends, engine: Some(FakeEngine::new()), pdfium: false }
}

fn overlay(inputs: Vec<PathBuf>, out: &Path) -> Overlay {
    Overlay { input_files: inputs, output: Some(out.to_path_buf()), ..Overlay::default() }
}

fn keystore_overlay(inputs: Vec<PathBuf>, out: &Path) -> Overlay {
    Overlay {
        keystore_file: Some(testdata("keystores/test-rsa2048.p12")),
        keystore_store_password: Some(Zeroizing::new("test".into())),
        ..overlay(inputs, out)
    }
}

/// Dateien mit unterschiedlichem Inhalt (sonst gleiche Signatur-Eingabe) im Ordner `in`.
fn inputs(fx: &Fixture, names: &[&str]) -> Vec<PathBuf> {
    let dir = fx.dir("in");
    names
        .iter()
        .map(|n| {
            let p = copy_pdf(PDF, &dir, n);
            std::fs::write(&p, [std::fs::read(&p).unwrap().as_slice(), format!("\n%{n}\n").as_bytes()].concat())
                .unwrap();
            p
        })
        .collect()
}

fn batch(v: &ViewState) -> &BatchView {
    v.batch.as_ref().expect("Stapel")
}

fn item_status(v: &ViewState, name: &str) -> BatchItemStatusView {
    batch(v).items.iter().find(|i| i.name == name).map(|i| i.status.clone()).expect(name)
}

fn doc_of(v: &ViewState, name: &str) -> DocId {
    batch(v).items.iter().find(|i| i.name == name).map(|i| i.doc).expect(name)
}

/// Übersicht mit abgeschlossener Voranalyse.
async fn overview_ready(core: &Core, n: usize) -> ViewState {
    wait_for(core, "Übersicht voranalysiert", |v| {
        matches!(v.step, StepView::Batch)
            && v.batch.as_ref().is_some_and(|b| {
                b.items.len() == n && b.items.iter().all(|i| !matches!(i.status, BatchItemStatusView::Analyzing))
            })
    })
    .await
}

fn start_batch(
    core: &Core,
    v: &ViewState,
    backend: BackendId,
    on_existing: ExistingPolicy,
    output_dir: Option<PathBuf>,
) {
    let docs = batch(v).items.iter().map(|i| i.doc).collect();
    core.dispatch(UiEvent::StartBatch { backend, docs, output_dir, on_existing });
}

fn progress(v: &ViewState) -> Option<&SigningProgressView> {
    match &v.step {
        StepView::Signing { progress, .. } if progress.batch => Some(progress),
        _ => None,
    }
}

fn form(v: &ViewState) -> Option<(u64, &NativeForm)> {
    match &v.step {
        StepView::Signing { interaction: InteractionView::Form { request, form }, .. } => Some((*request, form)),
        _ => None,
    }
}

async fn login(core: &Core) {
    let v = wait_for(core, "Anmeldung", |v| form(v).is_some_and(|(_, f)| matches!(f, NativeForm::Login { .. }))).await;
    let (request, _) = form(&v).unwrap();
    let answer =
        FormAnswerView::Login { username: "+436641234567".into(), password: "geheim1".into(), remember: false };
    core.dispatch(UiEvent::SubmitForm { request, answer });
}

/// Wartet auf die Bestätigung (SMS-TAN) des Dokuments `index` und liefert Zustand und Vergleichswert.
async fn confirmation(core: &Core, index: u32) -> (ViewState, String) {
    let v = wait_for(core, &format!("Bestätigung {index}"), |v| {
        form(v).is_some_and(|(_, f)| matches!(f, NativeForm::SmsTan { .. }))
            && progress(v).is_some_and(|p| p.index == index)
    })
    .await;
    let Some((_, NativeForm::SmsTan { reference_value, .. })) = form(&v) else { unreachable!() };
    let r = reference_value.clone();
    (v, r)
}

fn tan(core: &Core, v: &ViewState) {
    let (request, _) = form(v).unwrap();
    core.dispatch(UiEvent::SubmitForm { request, answer: FormAnswerView::Tan { tan: "123456".into() } });
}

/// Übersicht nach dem Lauf (mit Zusammenfassung).
async fn summary(core: &Core) -> ViewState {
    wait_for(core, "Zusammenfassung", |v| {
        matches!(v.step, StepView::Batch) && v.batch.as_ref().is_some_and(|b| b.summary.is_some() && !b.running)
    })
    .await
}

async fn paused(core: &Core, scope: PauseScope) -> ViewState {
    wait_for(core, &format!("Pause {scope:?}"), |v| {
        progress(v).and_then(|p| p.pause.as_ref()).is_some_and(|p| p.scope == scope)
    })
    .await
}

fn select(core: &Core, backend: BackendId) {
    core.dispatch(UiEvent::SelectSigner { backend: Some(backend), make_default: false });
}

fn png_width(png: &[u8]) -> u32 {
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    u32::from_be_bytes(png[16..20].try_into().unwrap())
}

// ------------------------------------------------------------------------------------------------
// Punkt 1: Übersicht und Schätzung
// ------------------------------------------------------------------------------------------------

/// Vier Dateien (eine kennwortgeschützt, eine mit QR-Platzhalter, Platzhaltersuche an): Miniatur, Seiten,
/// Größe, Status; Schätzung „3 Dokumente · 1 Anmeldung · 3 Bestätigungen (ggf. +1 für das Zertifikat)“.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn overview_with_preanalysis_thumbnails_and_estimate() {
    let fx = Fixture::new();
    fx.write_settings("[placeholder]\nsearch = true\n");
    let input = fx.dir("in");
    let a = copy_pdf(TEXT_PDF, &input, "Vertrag.pdf");
    let locked = copy_pdf(PASSWORD_PDF, &input, "Geheim.pdf");
    let qr = copy_pdf(QR_PDF, &input, "Antrag.pdf");
    let d = copy_pdf(PDF, &input, "Beilage.pdf");
    let pdfium = pdfium_dir().is_some();
    let atrust = Atrust::new(vec![]);
    let o = Opts { backends: atrust.backends(), engine: None, pdfium };
    let core = start(&fx, overlay(vec![a, locked, qr, d], &fx.dir("out")), o).expect("Kern");
    let v = overview_ready(&core, 4).await;
    assert!(v.document.is_none(), "Übersicht ohne Einzeldokument");
    assert_eq!(item_status(&v, "Geheim.pdf"), BatchItemStatusView::Rejected { code: "error-PDFPwdProtected".into() });
    for name in ["Vertrag.pdf", "Beilage.pdf", "Antrag.pdf"] {
        let i = batch(&v).items.iter().find(|i| i.name == name).unwrap();
        assert!(i.page_count.is_some_and(|n| n > 0) && i.size_bytes.is_some_and(|n| n > 0), "{name}");
        assert_eq!(i.target_name.as_deref(), Some(format!("{}_signed.pdf", name.trim_end_matches(".pdf")).as_str()));
        assert!(!i.target_exists);
        assert_eq!(i.thumbnail, pdfium, "{name}: Miniatur nur mit PDFium");
        if pdfium {
            let png = core.thumbnail(i.doc).expect("Miniatur");
            assert_eq!(png_width(&png), 96, "Breite 96 px");
        }
    }
    if pdfium {
        assert_eq!(item_status(&v, "Antrag.pdf"), BatchItemStatusView::Positioned, "QR-Platzhalter");
        let i = batch(&v).items.iter().find(|i| i.name == "Antrag.pdf").unwrap();
        assert_eq!(i.position, PositionSummaryView::Qr);
    } else {
        eprintln!("PIDA_PDFIUM_DIR nicht gesetzt – QR-Platzhalter und Miniaturen übersprungen");
    }
    assert_eq!(item_status(&v, "Vertrag.pdf"), BatchItemStatusView::Ready);
    assert!(core.thumbnail(doc_of(&v, "Geheim.pdf")).is_err());
    assert_eq!(batch(&v).output.mode, OutputModeView::Folder);
    assert!(!batch(&v).output.needs_folder);
    assert!(v.actions.start_batch && v.actions.open_files && !v.actions.sign);
    // ohne gewähltes Signiermittel keine Schätzung
    assert!(batch(&v).estimate.is_none());
    select(&core, BackendId::AtrustNative);
    let v = wait_for(&core, "Schätzung", |v| v.batch.as_ref().is_some_and(|b| b.estimate.is_some())).await;
    let e = batch(&v).estimate.clone().unwrap();
    assert_eq!((e.documents, e.logins, e.confirmations), (3, 1, 3), "{e:?}");
    assert!(e.certificate_confirmation_possible && !e.large);
    assert_eq!(atrust.n(&atrust.opened), 0, "die Übersicht öffnet keine Sitzung (A1)");
    drop(core);

    // gültiger Eintrag im Zertifikats-Cache: ohne „+1“
    let fx = Fixture::new();
    fx.write_settings(
        "[certificate_cache]\nenabled = true\n[id_austria]\nusername = \"+436641234567\"\nremember = true\n",
    );
    let settings = pida_config::CertificateCacheSettings { enabled: true };
    let cache = pida_config::CertificateCache::new(&fx.paths, &settings);
    let key = pida_config::CertCacheKey::new(BackendId::AtrustNative.as_str(), "+436641234567");
    cache.put(&key, test_key().chain().signer.as_der()).unwrap();
    let files = inputs(&fx, &["a.pdf", "b.pdf"]);
    let core = start(&fx, overlay(files, &fx.dir("out")), fake(Atrust::new(vec![]).backends())).unwrap();
    overview_ready(&core, 2).await;
    select(&core, BackendId::AtrustNative);
    let v = wait_for(&core, "Schätzung", |v| v.batch.as_ref().is_some_and(|b| b.estimate.is_some())).await;
    let e = batch(&v).estimate.clone().unwrap();
    if test_key().chain().signer.is_valid_at(time::OffsetDateTime::now_utc()) {
        assert!(!e.certificate_confirmation_possible, "gültiger Cache-Eintrag: keine Bestätigung für das Zertifikat");
    }
    assert_eq!(e.documents, 2);
    drop(core);

    // Passwort im „Merken“-Speicher (echtes ID-Austria-Backend, ohne Sitzung): „0 Anmeldungen“
    let fx = Fixture::new();
    let files = inputs(&fx, &["a.pdf", "b.pdf"]);
    let mut o = overlay(files, &fx.dir("out"));
    o.username = Some("+436641234567".into());
    o.password = Some(Zeroizing::new("geheim1".into()));
    let core = start(&fx, o, Opts { backends: None, engine: Some(FakeEngine::new()), pdfium: false }).unwrap();
    overview_ready(&core, 2).await;
    select(&core, BackendId::AtrustNative);
    let v = wait_for(&core, "Schätzung", |v| v.batch.as_ref().is_some_and(|b| b.estimate.is_some())).await;
    assert_eq!(batch(&v).estimate.as_ref().unwrap().logins, 0, "gemerktes Passwort");
    // Keystore mit Passwort aus dem Overlay: ebenfalls ohne Anmeldung, keine Bestätigung je Dokument
    drop(core);
    let fx = Fixture::new();
    let files = inputs(&fx, &["a.pdf", "b.pdf"]);
    let core = start(&fx, keystore_overlay(files, &fx.dir("out")), fake(None)).unwrap();
    overview_ready(&core, 2).await;
    select(&core, BackendId::Keystore);
    let v = wait_for(&core, "Schätzung", |v| v.batch.as_ref().is_some_and(|b| b.estimate.is_some())).await;
    let e = batch(&v).estimate.clone().unwrap();
    assert_eq!((e.logins, e.confirmations, e.certificate_confirmation_possible), (0, 0, false), "{e:?}");
}

// ------------------------------------------------------------------------------------------------
// Punkt 2: eine Anmeldung, höchstens ein Zertifikat, je Dokument eine Bestätigung
// ------------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn run_with_one_login_and_one_confirmation_per_document() {
    let fx = Fixture::new();
    let out = fx.dir("out");
    let names = ["Vertrag.pdf", "Antrag.pdf", "Beilage.pdf"];
    let atrust = Atrust::new(vec![]);
    let core = start(&fx, overlay(inputs(&fx, &names), &out), fake(atrust.backends())).unwrap();
    let v = overview_ready(&core, 3).await;
    select(&core, BackendId::AtrustNative);
    start_batch(&core, &v, BackendId::AtrustNative, ExistingPolicy::KeepBoth, None);
    login(&core).await;
    let mut references = Vec::new();
    for (k, name) in names.iter().enumerate() {
        let index = u32::try_from(k + 1).unwrap();
        let (v, reference) = confirmation(&core, index).await;
        let p = progress(&v).unwrap();
        assert_eq!((p.index, p.total, p.batch), (index, 3, true));
        assert_eq!(p.doc.as_ref().map(|d| d.name.as_str()), Some(*name), "Karte nennt das Dokument");
        assert_eq!(p.stage, SigningStage::Confirming);
        assert_eq!(p.confirmations_done, index - 1);
        assert_eq!(p.confirmations_total, 3);
        assert!(p.login_done);
        assert_eq!(p.certificate, if k == 0 { CertificateSource::Read } else { CertificateSource::Session });
        assert!(batch(&v).running && v.document.is_none(), "Karte über der Übersicht");
        assert!(matches!(item_status(&v, name), BatchItemStatusView::Active { stage: SigningStage::Confirming }));
        for later in &names[k + 1..] {
            assert_eq!(item_status(&v, later), BatchItemStatusView::Queued);
        }
        assert!(matches!(&v.step, StepView::Signing { document_leaves_device: true, .. }));
        references.push(reference);
        tan(&core, &v);
    }
    let v = summary(&core).await;
    let s = batch(&v).summary.clone().unwrap();
    assert_eq!((s.total, s.signed, s.skipped, s.rejected, s.failed), (3, 3, 0, 0, 0));
    assert_eq!(s.folder_display.as_deref(), Some(out.display().to_string().as_str()));
    assert!(
        v.notices.iter().any(|n| n.code == "ng-batch-done" && n.params["signed"] == "3" && n.params["total"] == "3")
    );
    assert_eq!(atrust.n(&atrust.opened), 1, "eine Sitzung");
    assert_eq!(atrust.n(&atrust.logins), 1, "eine Login-Maske");
    assert!(atrust.n(&atrust.certificates) <= 1, "höchstens ein InfoboxRead");
    assert_eq!(atrust.n(&atrust.signs), 3, "je Dokument ein Auftrag");
    let confirmations = atrust.confirmations.lock().unwrap().clone();
    assert_eq!(
        confirmations.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
        names,
        "eigene Bestätigung je Dokument"
    );
    let distinct: std::collections::BTreeSet<&String> = references.iter().collect();
    assert_eq!(distinct.len(), 3, "eigener Vergleichswert je Dokument: {references:?}");
    for name in names {
        let signed = out.join(name.replace(".pdf", "_signed.pdf"));
        assert!(signed.is_file(), "{name}");
        assert!(matches!(item_status(&v, name), BatchItemStatusView::Signed { path } if path == signed));
    }
    assert_eq!(v.done.len(), 3, "Ergebnisliste");
    assert_eq!(v.signers.selected, None, "„Jedes Mal fragen“: Auswahl nur für den Lauf (§8 F12)");
    // „Zuletzt signiert“ (ADR 0004 §3.10): jedes gespeicherte Dokument des Stapels, neueste zuerst
    let recent: Vec<&str> = v.recent.as_ref().expect("Liste").iter().map(|r| r.file_name.as_str()).collect();
    assert_eq!(recent, ["Beilage_signed.pdf", "Antrag_signed.pdf", "Vertrag_signed.pdf"]);
    let stored = pida_config::RecentStore::new(&fx.paths).load();
    assert_eq!(
        stored.iter().map(|e| e.path.clone()).collect::<Vec<_>>(),
        [out.join("Beilage_signed.pdf"), out.join("Antrag_signed.pdf"), out.join("Vertrag_signed.pdf")]
    );
}

// ------------------------------------------------------------------------------------------------
// Punkt 3 und 5: Überspringen ≠ Abbrechen, Zusammenfassung
// ------------------------------------------------------------------------------------------------

/// Überspringen beim 2. Dokument: weiter mit dem 3. ohne neue Login-Maske; danach „2 von 4 signiert,
/// 1 übersprungen, 1 abgelehnt“ mit Speicherort.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn skip_keeps_the_login_and_the_summary_counts_all_entries() {
    let fx = Fixture::new();
    let out = fx.dir("out");
    let mut files = inputs(&fx, &["a.pdf", "b.pdf", "c.pdf"]);
    files.insert(1, copy_pdf(PASSWORD_PDF, &fx.dir("in"), "geheim.pdf"));
    let atrust = Atrust::new(vec![]);
    let core = start(&fx, overlay(files, &out), fake(atrust.backends())).unwrap();
    let v = overview_ready(&core, 4).await;
    assert!(matches!(item_status(&v, "geheim.pdf"), BatchItemStatusView::Rejected { .. }));
    start_batch(&core, &v, BackendId::AtrustNative, ExistingPolicy::KeepBoth, None);
    login(&core).await;
    let (v, _) = confirmation(&core, 1).await;
    tan(&core, &v);
    let (v, _) = confirmation(&core, 2).await;
    assert_eq!(progress(&v).unwrap().total, 3, "abgelehnte Dokumente gehören nicht zum Lauf");
    core.dispatch(UiEvent::SkipBatchDocument);
    let (v, _) = confirmation(&core, 3).await;
    assert_eq!(item_status(&v, "b.pdf"), BatchItemStatusView::Skipped { existing: false });
    assert!(v.notices.iter().any(|n| n.code == "ng-batch-pending-app-request"), "Hinweis auf die offene App-Anfrage");
    assert_eq!(atrust.n(&atrust.logins), 1, "keine neue Login-Maske");
    assert_eq!(atrust.n(&atrust.cancelled), 0, "Überspringen bricht die Sitzung nicht ab");
    tan(&core, &v);
    let v = summary(&core).await;
    let s = batch(&v).summary.clone().unwrap();
    assert_eq!((s.total, s.signed, s.skipped, s.rejected, s.failed, s.cancelled, s.not_started), (4, 2, 1, 1, 0, 0, 0));
    assert!(!s.aborted, "Lauf zu Ende, nicht abgebrochen");
    assert_eq!(s.folder_display.as_deref(), Some(out.display().to_string().as_str()));
    assert!(!out.join("b_signed.pdf").exists());
    assert_eq!(atrust.n(&atrust.opened), 1);
    // „Im Ordner zeigen“ darf genau die geschriebenen Dateien zeigen (Status `Signed`)
    assert!(
        batch(&v)
            .items
            .iter()
            .any(|i| matches!(&i.status, BatchItemStatusView::Signed { path } if path == &out.join("a_signed.pdf")))
    );
}

/// „Stapel abbrechen“: Rest bleibt „Bereit“, Gespeichertes bleibt, Passwort gelöscht (F-72).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn abort_keeps_saved_documents_and_clears_the_password() {
    let fx = Fixture::new();
    let out = fx.dir("out");
    let atrust = Atrust::new(vec![]);
    let core = start(&fx, overlay(inputs(&fx, &["a.pdf", "b.pdf", "c.pdf"]), &out), fake(atrust.backends())).unwrap();
    let v = overview_ready(&core, 3).await;
    start_batch(&core, &v, BackendId::AtrustNative, ExistingPolicy::KeepBoth, None);
    login(&core).await;
    let (v, _) = confirmation(&core, 1).await;
    tan(&core, &v);
    confirmation(&core, 2).await;
    core.dispatch(UiEvent::AbortBatch);
    let v = summary(&core).await;
    assert!(matches!(item_status(&v, "a.pdf"), BatchItemStatusView::Signed { .. }));
    assert_eq!(item_status(&v, "b.pdf"), BatchItemStatusView::Cancelled);
    assert_eq!(item_status(&v, "c.pdf"), BatchItemStatusView::Ready);
    assert!(out.join("a_signed.pdf").is_file(), "Gespeichertes bleibt");
    assert!(v.notices.iter().any(|n| n.code == "ng-batch-aborted" && n.params["signed"] == "1"));
    assert!(batch(&v).summary.as_ref().unwrap().aborted, "Zusammenfassung: abgebrochen");
    for _ in 0..50 {
        if atrust.n(&atrust.cancelled) >= 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(atrust.n(&atrust.cancelled), 1, "Sitzung abgebrochen: Passwort gelöscht (F-72)");
    assert!(v.actions.start_batch, "der Rest kann erneut signiert werden");
    // neuer Lauf mit dem Rest: neue Sitzung, neue Anmeldung
    start_batch(&core, &v, BackendId::AtrustNative, ExistingPolicy::KeepBoth, None);
    login(&core).await;
    let (v, _) = confirmation(&core, 1).await;
    assert_eq!(progress(&v).unwrap().total, 1, "nur „Bereit“");
    assert_eq!(atrust.n(&atrust.logins), 2);
    // der zweite Lauf endet regulär: nicht „abgebrochen“, obwohl b.pdf aus dem ersten Lauf abgebrochen bleibt
    tan(&core, &v);
    let v = summary(&core).await;
    let s = batch(&v).summary.clone().unwrap();
    assert_eq!((s.signed, s.cancelled), (2, 1));
    assert!(!s.aborted);
}

/// „Für diesen Stapel automatisch positionieren“ (ADR 0004 Nachtrag 8) gegen den echten Kern: nur für diesen
/// Stapel – die Einstellung bleibt unverändert und wird nicht gespeichert –, startet nichts (F-54: erst „Alle
/// signieren“), alle Einträge „Bereit“; nach dem Ende bzw. Abbruch des Laufs gilt wieder die Einstellung. Mit
/// eingeschalteter Einstellung meldet die Übersicht „laut Einstellung“.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn auto_position_for_this_batch_only() {
    let fx = Fixture::new();
    fx.write_settings("[general]\n");
    let out = fx.dir("out");
    let atrust = Atrust::new(vec![]);
    let core = start(&fx, overlay(inputs(&fx, &["a.pdf", "b.pdf"]), &out), fake(atrust.backends())).unwrap();
    let v = overview_ready(&core, 2).await;
    let b = batch(&v);
    assert!(!b.auto_position_setting && !b.auto_position_once && b.manual_plans == 0);
    core.dispatch(UiEvent::SetBatchAutoPosition { enabled: true, replace_manual: false });
    let v = wait_for(&core, "für diesen Stapel", |v| v.batch.as_ref().is_some_and(|b| b.auto_position_once)).await;
    assert!(batch(&v).items.iter().all(|i| i.status == BatchItemStatusView::Ready), "{:?}", batch(&v).items);
    assert!(!batch(&v).auto_position_setting, "Einstellung unverändert");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(atrust.n(&atrust.opened), 0, "startet nichts (F-54)");
    assert!(matches!(core.view().step, StepView::Batch));
    let saved = std::fs::read_to_string(fx.paths.settings_file()).unwrap_or_default();
    assert!(!saved.contains("auto_position = true"), "nicht gespeichert: {saved}");
    // Lauf abbrechen → wieder laut Einstellung
    start_batch(&core, &v, BackendId::AtrustNative, ExistingPolicy::KeepBoth, None);
    login(&core).await;
    confirmation(&core, 1).await;
    core.dispatch(UiEvent::AbortBatch);
    let v = summary(&core).await;
    assert!(!batch(&v).auto_position_once, "nach dem Lauf wieder laut Einstellung");
    drop(core);

    // Einstellung an: „laut Einstellung“
    let fx = Fixture::new();
    fx.write_settings("[signature]\nauto_position = true\n");
    let core =
        start(&fx, overlay(inputs(&fx, &["a.pdf", "b.pdf"]), &fx.dir("out")), fake(Atrust::new(vec![]).backends()))
            .unwrap();
    let v = overview_ready(&core, 2).await;
    assert!(batch(&v).auto_position_setting && !batch(&v).auto_position_once);
    assert!(batch(&v).items.iter().all(|i| i.status == BatchItemStatusView::Ready));
}

// ------------------------------------------------------------------------------------------------
// Punkt 4: Pausen
// ------------------------------------------------------------------------------------------------

/// Netzfehler nach der Anmeldung → Pause (Verbindung) → „Erneut versuchen“ in derselben Sitzung ohne neue
/// Login-Maske.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn connection_pause_resumes_in_the_same_session() {
    let fx = Fixture::new();
    let atrust = Atrust::new(vec![(2, BackendError::Network("Verbindung getrennt".into()))]);
    let core = start(&fx, overlay(inputs(&fx, &["a.pdf", "b.pdf"]), &fx.dir("out")), fake(atrust.backends())).unwrap();
    let v = overview_ready(&core, 2).await;
    start_batch(&core, &v, BackendId::AtrustNative, ExistingPolicy::KeepBoth, None);
    login(&core).await;
    let (v, _) = confirmation(&core, 1).await;
    tan(&core, &v);
    let v = paused(&core, PauseScope::Session).await;
    let p = progress(&v).unwrap();
    let pause = p.pause.clone().unwrap();
    assert_eq!((pause.code.as_str(), pause.new_session, pause.can_retry), ("backend-network", false, true));
    assert_eq!(pause.doc.as_ref().map(|d| d.name.as_str()), Some("b.pdf"));
    assert_eq!(p.stage, SigningStage::Paused);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(progress(&core.view()).and_then(|p| p.pause.as_ref()).is_some(), "nie automatisch weiter");
    core.dispatch(UiEvent::Retry);
    let (v, _) = confirmation(&core, 2).await;
    assert!(progress(&v).unwrap().pause.is_none());
    tan(&core, &v);
    summary(&core).await;
    assert_eq!(atrust.n(&atrust.opened), 1, "dieselbe Sitzung");
    assert_eq!(atrust.n(&atrust.logins), 1, "ohne neue Login-Maske");
}

/// Sitzungsfehler (`UnknownPage`) → Pause mit „neue Anmeldung möglich“; „Erneut versuchen“ öffnet eine neue
/// Sitzung.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn session_pause_opens_a_new_session_on_retry() {
    let fx = Fixture::new();
    let out = fx.dir("out");
    let atrust = Atrust::new(vec![(2, BackendError::UnknownPage { diagnostic: "x".into() })]);
    let core = start(&fx, overlay(inputs(&fx, &["a.pdf", "b.pdf"]), &out), fake(atrust.backends())).unwrap();
    let v = overview_ready(&core, 2).await;
    start_batch(&core, &v, BackendId::AtrustNative, ExistingPolicy::KeepBoth, None);
    login(&core).await;
    let (v, _) = confirmation(&core, 1).await;
    tan(&core, &v);
    let v = paused(&core, PauseScope::Session).await;
    let pause = progress(&v).unwrap().pause.clone().unwrap();
    assert!(pause.new_session && pause.code == "backend-unknown-page");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(atrust.n(&atrust.opened), 1, "keine neue Sitzung ohne „Erneut versuchen“ (A1)");
    core.dispatch(UiEvent::Retry);
    login(&core).await;
    let (v, _) = confirmation(&core, 2).await;
    assert_eq!(progress(&v).unwrap().doc.as_ref().unwrap().name, "b.pdf", "weiter beim aktuellen Dokument");
    tan(&core, &v);
    let v = summary(&core).await;
    assert_eq!(batch(&v).summary.as_ref().unwrap().signed, 2);
    assert_eq!(atrust.n(&atrust.opened), 2);
    assert_eq!(atrust.n(&atrust.logins), 2, "neue Sitzung, neue Anmeldung");
}

/// Ablehnung in der App → Pause (Dokument), nie automatisch weiter; „Erneut anfordern“ ohne neue Anmeldung.
/// Nicht wiederholbarer Dokumentfehler (`DocumentTooLarge`) → Zeile „Fehler“, der Lauf geht weiter.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn declined_pauses_the_document_and_too_large_fails_it() {
    let fx = Fixture::new();
    let atrust = Atrust::new(vec![(1, BackendError::Cancelled), (3, BackendError::DocumentTooLarge)]);
    let core = start(&fx, overlay(inputs(&fx, &["a.pdf", "b.pdf", "c.pdf"]), &fx.dir("out")), fake(atrust.backends()))
        .unwrap();
    let v = overview_ready(&core, 3).await;
    start_batch(&core, &v, BackendId::AtrustNative, ExistingPolicy::KeepBoth, None);
    login(&core).await;
    let v = paused(&core, PauseScope::Document).await;
    let pause = progress(&v).unwrap().pause.clone().unwrap();
    assert_eq!(pause.code, "ng-signing-declined");
    assert_eq!(pause.doc.as_ref().map(|d| d.name.as_str()), Some("a.pdf"));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(progress(&core.view()).and_then(|p| p.pause.as_ref()).is_some(), "nie automatisch weiter");
    assert_eq!(atrust.n(&atrust.cancelled), 0, "Zugangsdaten bleiben bis zur Entscheidung (§8 F10)");
    core.dispatch(UiEvent::Retry);
    let (v, _) = confirmation(&core, 1).await;
    tan(&core, &v);
    // Auftrag 3 (Dokument b): zu groß → Fehler, weiter mit c
    let (v, _) = confirmation(&core, 3).await;
    assert!(
        matches!(item_status(&v, "b.pdf"), BatchItemStatusView::Failed { code, .. } if code == "backend-document-too-large")
    );
    tan(&core, &v);
    let v = summary(&core).await;
    let s = batch(&v).summary.clone().unwrap();
    assert_eq!((s.signed, s.failed), (2, 1));
    assert_eq!(atrust.n(&atrust.logins), 1);
    assert_eq!(atrust.n(&atrust.opened), 1);
}

// ------------------------------------------------------------------------------------------------
// Punkt 6: Einzeldokument aus der Übersicht
// ------------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn single_document_from_the_overview_keeps_its_plan_and_counts_as_signed() {
    let fx = Fixture::new();
    let out = fx.dir("out");
    let engine = FakeEngine::new();
    let pdfium = pdfium_dir().is_some();
    let o = Opts { backends: None, engine: Some(engine.clone()), pdfium };
    let core = start(&fx, keystore_overlay(inputs(&fx, &["a.pdf", "b.pdf", "c.pdf"]), &out), o).unwrap();
    let v = overview_ready(&core, 3).await;
    // b öffnen, verschieben bzw. unsichtbar, „Zurück zum Stapel“ → „Positioniert“ mit diesem Plan
    let b = doc_of(&v, "b.pdf");
    core.dispatch(UiEvent::OpenBatchItem { doc: b });
    let v = wait_for(&core, "Arbeitsbereich", |v| v.document.as_ref().is_some_and(|d| d.doc.id == b)).await;
    assert_eq!(batch(&v).open_item, Some(b), "Übersicht bleibt erhalten");
    assert!(v.actions.show_batch);
    let expected = if pdfium {
        let v = wait_for(&core, "Sticker", |v| v.document.as_ref().is_some_and(|d| d.sticker.is_some())).await;
        let d = v.document.unwrap();
        core.dispatch(UiEvent::MoveBlock { page: 0, x: 60.0, y: 400.0 });
        wait_for(&core, "verschoben", |v| {
            v.document.as_ref().is_some_and(|x| x.revision > d.revision && x.placement.x == 60.0)
        })
        .await;
        PositionSummaryView::Manual { page: 0 }
    } else {
        core.dispatch(UiEvent::SetInvisible { invisible: true });
        wait_for(&core, "unsichtbar", |v| v.document.as_ref().is_some_and(|d| d.invisible)).await;
        PositionSummaryView::Invisible
    };
    core.dispatch(UiEvent::BackToBatch);
    let v = wait_for(&core, "zurück", |v| matches!(v.step, StepView::Batch)).await;
    let item = batch(&v).items.iter().find(|i| i.doc == b).unwrap();
    assert_eq!((item.status.clone(), item.position.clone()), (BatchItemStatusView::Positioned, expected));
    assert!(v.document.is_none() && batch(&v).open_item.is_none());
    // c einzeln signieren → Hinweis, zurück zur Übersicht
    let c = doc_of(&v, "c.pdf");
    core.dispatch(UiEvent::OpenBatchItem { doc: c });
    wait_for(&core, "Arbeitsbereich c", |v| {
        v.document.as_ref().is_some_and(|d| d.doc.id == c && d.phase == pida_core_api::DocumentPhase::Editing)
    })
    .await;
    core.dispatch(UiEvent::SignWith { backend: BackendId::Keystore });
    let v = wait_for(&core, "zurück nach dem Signieren", |v| {
        matches!(v.step, StepView::Batch)
            && v.batch.as_ref().is_some_and(|b| {
                b.items.iter().any(|i| i.doc == c && matches!(i.status, BatchItemStatusView::Signed { .. }))
            })
    })
    .await;
    assert!(v.notices.iter().any(|n| n.code == "ng-batch-item-signed" && n.params["file"] == "c.pdf"));
    assert!(out.join("c_signed.pdf").is_file());
    // Lauf für den Rest: zählt c als signiert
    start_batch(&core, &v, BackendId::Keystore, ExistingPolicy::KeepBoth, None);
    let v = summary(&core).await;
    let s = batch(&v).summary.clone().unwrap();
    assert_eq!((s.total, s.signed), (3, 3));
    let plans = engine.plans.lock().unwrap().clone();
    assert_eq!(plans.len(), 3);
    if !pdfium {
        assert_eq!(
            plans.iter().filter(|p| matches!(p.visibility, pida_pades::Visibility::Invisible)).count(),
            1,
            "Plan von b"
        );
    }
}

/// Mehrfachsignatur im Stapel (echte Engine, PDFium nötig): ein Eintrag mit zwei von Hand platzierten Blöcken behält
/// sie als Plan (erneut geöffnet: beide wieder da), die Übersicht zählt drei Bestätigungen für zwei Dokumente, und
/// der Lauf signiert dieses Dokument zweimal in derselben Sitzung – das andere einmal.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_entry_with_two_blocks_is_signed_twice_in_the_run() {
    let fx = Fixture::new();
    let out = fx.dir("out");
    let o = Opts { backends: None, engine: None, pdfium: true };
    // unveränderte Dateien (die echte Engine lehnt die mit Anhang unterscheidbar gemachten ab)
    let dir = fx.dir("in");
    let files = vec![copy_pdf(PDF, &dir, "a.pdf"), copy_pdf(PDF, &dir, "b.pdf")];
    let Some(core) = start(&fx, keystore_overlay(files, &out), o) else {
        eprintln!("PIDA_PDFIUM_DIR nicht gesetzt – übersprungen");
        return;
    };
    let v = overview_ready(&core, 2).await;
    let b = doc_of(&v, "b.pdf");
    let blocks = |v: &ViewState| v.document.as_ref().and_then(|d| d.blocks.clone());
    core.dispatch(UiEvent::OpenBatchItem { doc: b });
    wait_for(&core, "Arbeitsbereich mit Block", |v| {
        v.document.as_ref().is_some_and(|d| d.doc.id == b && d.sticker.is_some())
            && blocks(v).is_some_and(|x| x.can_add)
    })
    .await;
    core.dispatch(UiEvent::AddBlock { page: Some(0) });
    let v = wait_for(&core, "zwei Blöcke", |v| blocks(v).is_some_and(|x| x.items.len() == 2)).await;
    let placed = blocks(&v).unwrap().items;
    core.dispatch(UiEvent::BackToBatch);
    let v = wait_for(&core, "zurück", |v| matches!(v.step, StepView::Batch)).await;
    let signatures =
        |v: &ViewState, name: &str| batch(v).items.iter().find(|i| i.name == name).and_then(|i| i.signatures);
    assert_eq!((signatures(&v, "a.pdf"), signatures(&v, "b.pdf")), (Some(1), Some(2)));
    assert_eq!(item_status(&v, "b.pdf"), BatchItemStatusView::Positioned);
    select(&core, BackendId::Keystore);
    let v = wait_for(&core, "Schätzung", |v| v.batch.as_ref().is_some_and(|x| x.estimate.is_some())).await;
    let estimate = batch(&v).estimate.clone().unwrap();
    // der Keystore bestätigt nicht je Dokument (Passwort einmal); mit ID Austria wären es drei Bestätigungen
    assert_eq!((estimate.documents, estimate.confirmations), (2, 0));
    // erneut öffnen: beide Blöcke wieder an ihrer Stelle, derselbe ausgewählt
    core.dispatch(UiEvent::OpenBatchItem { doc: b });
    let v = wait_for(&core, "wieder geöffnet", |v| {
        v.document.as_ref().is_some_and(|d| d.doc.id == b) && blocks(v).is_some_and(|x| x.items.len() == 2)
    })
    .await;
    let again = blocks(&v).unwrap().items;
    assert_eq!(again.iter().map(|x| (x.active, x.page)).collect::<Vec<_>>(), vec![(false, placed[0].page), (true, 0)]);
    for (x, y) in again.iter().zip(&placed) {
        let (r, q) = (x.rect.unwrap(), y.rect.unwrap());
        assert!((r.left - q.left).abs() < 0.5 && (r.top - q.top).abs() < 0.5, "{r:?} ≠ {q:?}");
    }
    core.dispatch(UiEvent::BackToBatch);
    let v = wait_for(&core, "zurück", |v| matches!(v.step, StepView::Batch)).await;
    start_batch(&core, &v, BackendId::Keystore, ExistingPolicy::KeepBoth, None);
    let v = summary(&core).await;
    let s = batch(&v).summary.clone().unwrap();
    assert_eq!((s.total, s.signed), (2, 2));
    let signed = |name: &str| {
        let bytes = std::fs::read(out.join(name)).unwrap();
        let pdf = pida_pdf_cos::SourcePdf::open(bytes, &pida_pdf_cos::Limits::default()).unwrap();
        pdf.signature_fields().unwrap().into_iter().filter(|f| f.signed).count()
    };
    assert_eq!((signed("a_signed.pdf"), signed("b_signed.pdf")), (1, 2));
    if tool("qpdf") {
        let o = Command::new("qpdf").arg("--check").arg(out.join("b_signed.pdf")).output().unwrap();
        assert!(o.status.success(), "qpdf: {}", String::from_utf8_lossy(&o.stdout));
    }
}

// ------------------------------------------------------------------------------------------------
// „PDF hinzufügen“ (ADR 0004 Nachtrag 9): Kopfzeile „+ PDF“, Miniaturleiste
// ------------------------------------------------------------------------------------------------

/// „Weitere PDFs hinzufügen“ im Arbeitsbereich: alle zusammen werden zum Stapel, die Platzierung des offenen
/// Dokuments bleibt dessen Plan; „Anderes PDF öffnen“ in einem aus der Übersicht geöffneten Eintrag ersetzt ihn.
/// Nichts davon öffnet eine Sitzung (F-54).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn more_pdfs_join_the_open_document_as_a_batch_and_replace_swaps_an_entry() {
    let fx = Fixture::new();
    let out = fx.dir("out");
    let files = inputs(&fx, &["a.pdf", "b.pdf", "c.pdf", "d.pdf"]);
    let atrust = Atrust::new(Vec::new());
    let core = start(&fx, overlay(vec![files[0].clone()], &out), fake(atrust.backends())).unwrap();
    wait_for(&core, "Arbeitsbereich a", |v| {
        v.document.as_ref().is_some_and(|d| d.doc.name == "a.pdf" && d.phase == pida_core_api::DocumentPhase::Editing)
    })
    .await;
    // eigener Plan für a (ohne PDFium: „Unsichtbar signieren“), dann „Weitere PDFs hinzufügen“ mit b und c
    core.dispatch(UiEvent::SetInvisible { invisible: true });
    wait_for(&core, "unsichtbar", |v| v.document.as_ref().is_some_and(|d| d.invisible)).await;
    let add = Some(pida_core_api::OpenFilesMode::Add);
    core.dispatch(UiEvent::OpenFiles { paths: vec![files[1].clone(), files[2].clone()], mode: add });
    let v = overview_ready(&core, 3).await;
    let a = batch(&v).items.iter().find(|i| i.name == "a.pdf").unwrap();
    assert_eq!(
        (a.status.clone(), a.position.clone()),
        (BatchItemStatusView::Positioned, PositionSummaryView::Invisible)
    );
    assert!(v.document.is_none() && !v.notices.iter().any(|n| n.code == "ng-batch-added"));
    // b öffnen, „Anderes PDF öffnen“ mit d: b verschwindet aus dem Stapel, d kommt dazu
    let b = doc_of(&v, "b.pdf");
    core.dispatch(UiEvent::OpenBatchItem { doc: b });
    wait_for(&core, "Arbeitsbereich b", |v| v.document.as_ref().is_some_and(|d| d.doc.id == b)).await;
    core.dispatch(UiEvent::OpenFiles {
        paths: vec![files[3].clone()],
        mode: Some(pida_core_api::OpenFilesMode::Replace),
    });
    let v = overview_ready(&core, 3).await;
    let mut names: Vec<_> = batch(&v).items.iter().map(|i| i.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["a.pdf", "c.pdf", "d.pdf"]);
    assert_eq!(atrust.n(&atrust.opened), 0, "keine Sitzung ohne „Alle signieren“ (F-54)");
}

/// Ablauf des Stapels je Signiermittel (ADR 0004 Nachtrag 12): der Kern liefert ihn aus den Fähigkeiten –
/// Keystore „einmal entsperren“, ID Austria „einmal anmelden, je Dokument bestätigen“; ohne Fähigkeiten keiner.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn batch_flow_follows_the_capabilities_of_each_signer() {
    use pida_core_api::BatchFlowView;
    let fx = Fixture::new();
    let files = inputs(&fx, &["a.pdf", "b.pdf"]);
    let core = start(&fx, keystore_overlay(files, &fx.dir("out")), fake(None)).unwrap();
    let v = overview_ready(&core, 2).await;
    let flow = |id: BackendId| v.signers.options.iter().find(|o| o.backend == id).and_then(|o| o.batch_flow);
    assert_eq!(flow(BackendId::Keystore), Some(BatchFlowView::UnlockOnce));
    assert_eq!(flow(BackendId::AtrustNative), Some(BatchFlowView::LoginThenConfirmEach));
    // nicht eingerichtet: keine Fähigkeiten, kein Ablauf
    let fx = Fixture::new();
    let files = inputs(&fx, &["a.pdf", "b.pdf"]);
    let core = start(&fx, overlay(files, &fx.dir("out")), fake(None)).unwrap();
    let v = overview_ready(&core, 2).await;
    let ks = v.signers.options.iter().find(|o| o.backend == BackendId::Keystore).expect("Keystore im Menü");
    assert!(!ks.available && ks.batch_flow.is_none());
    // aus den Fähigkeiten (Chipkarte: je Dokument eine Bestätigung ohne gemeinsame Anmeldung)
    let mut caps = atrust_caps();
    assert_eq!(BatchFlowView::from_capabilities(&caps), BatchFlowView::LoginThenConfirmEach);
    caps.login_once_per_batch = false;
    assert_eq!(BatchFlowView::from_capabilities(&caps), BatchFlowView::ConfirmEach);
    caps.max_batch = u16::MAX;
    assert_eq!(BatchFlowView::from_capabilities(&caps), BatchFlowView::UnlockOnce);
}

/// „Anderes PDF öffnen“ in einem Einzeldokument: das offene Dokument wird geschlossen (nie signiert), das gewählte
/// öffnet im Arbeitsbereich – ohne Stapel und ohne Sitzung.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn replace_opens_the_chosen_pdf_instead_of_the_open_one() {
    let fx = Fixture::new();
    let out = fx.dir("out");
    let files = inputs(&fx, &["a.pdf", "b.pdf"]);
    let atrust = Atrust::new(Vec::new());
    let core = start(&fx, overlay(vec![files[0].clone()], &out), fake(atrust.backends())).unwrap();
    wait_for(&core, "Arbeitsbereich a", |v| v.document.as_ref().is_some_and(|d| d.doc.name == "a.pdf")).await;
    core.dispatch(UiEvent::OpenFiles {
        paths: vec![files[1].clone()],
        mode: Some(pida_core_api::OpenFilesMode::Replace),
    });
    let v = wait_for(&core, "Arbeitsbereich b", |v| {
        v.document.as_ref().is_some_and(|d| d.doc.name == "b.pdf" && d.phase == pida_core_api::DocumentPhase::Editing)
    })
    .await;
    assert!(v.batch.is_none() && !v.actions.show_batch, "kein Stapel, nichts wartet");
    assert!(v.done.iter().any(|d| d.name == "a.pdf" && d.result == pida_workflow::DocResult::Cancelled));
    assert_eq!(atrust.n(&atrust.opened), 0, "keine Sitzung (F-54)");
}

// ------------------------------------------------------------------------------------------------
// „Zur Startseite“ parkt offene Arbeit (ADR 0004 Nachtrag 10)
// ------------------------------------------------------------------------------------------------

fn parked(v: &ViewState) -> Option<&pida_core_api::ParkedWorkView> {
    v.parked.as_ref()
}

/// Startansicht mit geparkter Arbeit (Karte „In Bearbeitung“).
async fn start_with_parked(core: &Core) -> ViewState {
    wait_for(core, "Startansicht mit geparkter Arbeit", |v| {
        matches!(v.step, StepView::Open { .. }) && v.parked.is_some() && v.document.is_none() && v.batch.is_none()
    })
    .await
}

/// „Zur Startseite“ parkt das Dokument mit seiner Platzierung; die verdeckte Arbeit lässt sich nicht signieren,
/// „Weiter bearbeiten“ bringt sie unverändert zurück, „Schließen“ verwirft sie. Keine Sitzung (F-54).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn home_parks_the_document_and_resume_brings_it_back_unchanged() {
    let fx = Fixture::new();
    let out = fx.dir("out");
    let files = inputs(&fx, &["a.pdf"]);
    let atrust = Atrust::new(Vec::new());
    let core = start(&fx, overlay(files, &out), fake(atrust.backends())).unwrap();
    wait_for(&core, "Arbeitsbereich a", |v| {
        v.document.as_ref().is_some_and(|d| d.doc.name == "a.pdf" && d.phase == pida_core_api::DocumentPhase::Editing)
    })
    .await;
    // eigene Platzierung (ohne PDFium: „Freie Fläche suchen“ beendet die Automatik) und „Unsichtbar signieren“
    core.dispatch(UiEvent::UseTarget { target: pida_core_api::PlacementTarget::Free });
    core.dispatch(UiEvent::SetInvisible { invisible: true });
    let before = wait_for(&core, "von Hand", |v| {
        v.document.as_ref().is_some_and(|d| d.invisible && d.detection.as_ref().is_some_and(|x| !x.auto_placed))
    })
    .await
    .document
    .unwrap();
    core.dispatch(UiEvent::GoHome);
    let v = start_with_parked(&core).await;
    let p = parked(&v).unwrap();
    assert_eq!((p.doc_name.as_deref(), p.documents, p.placed_by_hand, p.unsaved), (Some("a.pdf"), 1, true, false));
    assert!(!v.actions.sign && !v.actions.edit_placement && v.actions.open_files);
    assert!(v.done.is_empty(), "nichts verworfen");
    // verdeckt: „Signieren“ (⌘↵ bzw. Palette) bleibt ohne Wirkung
    core.dispatch(UiEvent::SignWith { backend: BackendId::AtrustNative });
    core.dispatch(UiEvent::ConfirmPosition);
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(core.view().parked.is_some() && !matches!(core.view().step, StepView::Signing { .. }));
    // „Weiter bearbeiten“: Dokument und Platzierung wie vorher
    core.dispatch(UiEvent::ResumeParked);
    let v = wait_for(&core, "fortgesetzt", |v| v.document.is_some()).await;
    let d = v.document.unwrap();
    assert!(v.parked.is_none());
    assert_eq!((d.doc.id, d.invisible, d.target, d.placement), (before.doc.id, true, before.target, before.placement));
    assert!(d.detection.is_some_and(|x| !x.auto_placed), "Platzierung der Nutzerin bleibt");
    // erneut parken und „Schließen“: verworfen, „abgebrochen“
    core.dispatch(UiEvent::GoHome);
    start_with_parked(&core).await;
    core.dispatch(UiEvent::DiscardParked);
    let v = wait_for(&core, "verworfen", |v| matches!(v.step, StepView::Open { .. }) && v.parked.is_none()).await;
    assert!(v.document.is_none());
    assert!(v.done.iter().any(|d| d.name == "a.pdf" && d.result == pida_workflow::DocResult::Cancelled));
    assert_eq!(atrust.n(&atrust.opened), 0, "keine Sitzung ohne „Signieren“ (F-54)");
}

/// Neue Dateien bei geparkter Arbeit: „Zum Stapel hinzufügen“ (`mode: add`) bildet mit dem geparkten Dokument den
/// Stapel (Plan bleibt), „Öffnen mit“ (ohne `mode`) kommt ohne Rückfrage dazu (Hinweis), „Offene schließen und neu
/// beginnen“ (`mode: replace`) verwirft den geparkten Stapel. Keine Sitzung (F-54).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn files_while_parked_join_the_batch_or_replace_it() {
    let fx = Fixture::new();
    let out = fx.dir("out");
    let files = inputs(&fx, &["a.pdf", "b.pdf", "c.pdf", "d.pdf"]);
    let atrust = Atrust::new(Vec::new());
    let core = start(&fx, overlay(vec![files[0].clone()], &out), fake(atrust.backends())).unwrap();
    wait_for(&core, "Arbeitsbereich a", |v| v.document.as_ref().is_some_and(|d| d.doc.name == "a.pdf")).await;
    core.dispatch(UiEvent::SetInvisible { invisible: true });
    wait_for(&core, "unsichtbar", |v| v.document.as_ref().is_some_and(|d| d.invisible)).await;
    core.dispatch(UiEvent::GoHome);
    start_with_parked(&core).await;
    // „Zum Stapel hinzufügen“
    let add = Some(pida_core_api::OpenFilesMode::Add);
    core.dispatch(UiEvent::OpenFiles { paths: vec![files[1].clone()], mode: add });
    let v = overview_ready(&core, 2).await;
    assert!(v.parked.is_none());
    let a = batch(&v).items.iter().find(|i| i.name == "a.pdf").unwrap();
    assert_eq!(a.position, PositionSummaryView::Invisible, "Plan des geparkten Dokuments bleibt");
    // Stapel parken, „Öffnen mit“ (zweite Instanz): ohne Rückfrage dazu, mit Hinweis
    core.dispatch(UiEvent::GoHome);
    let v = start_with_parked(&core).await;
    let p = parked(&v).unwrap();
    assert_eq!((p.doc_name.as_deref(), p.documents, p.placed_by_hand), (None, 2, true));
    core.dispatch(UiEvent::OpenFiles { paths: vec![files[2].clone()], mode: None });
    let v = overview_ready(&core, 3).await;
    assert!(v.notices.iter().any(|n| n.code == "ng-parked-files-added" && n.params["count"] == "1"));
    // „Offene schließen und neu beginnen“: der Stapel wird verworfen, d öffnet im Arbeitsbereich
    core.dispatch(UiEvent::GoHome);
    start_with_parked(&core).await;
    core.dispatch(UiEvent::OpenFiles {
        paths: vec![files[3].clone()],
        mode: Some(pida_core_api::OpenFilesMode::Replace),
    });
    let v = wait_for(&core, "Arbeitsbereich d", |v| v.document.as_ref().is_some_and(|d| d.doc.name == "d.pdf")).await;
    assert!(v.batch.is_none() && v.parked.is_none());
    for name in ["a.pdf", "b.pdf", "c.pdf"] {
        assert!(v.done.iter().any(|d| d.name == name && d.result == pida_workflow::DocResult::Cancelled), "{name}");
    }
    assert_eq!(atrust.n(&atrust.opened), 0, "keine Sitzung (F-54)");
}

/// „Erneut signieren“ aus „Zuletzt signiert“ (ADR 0004 Nachtrag 15) bei geparkter Arbeit wie „Öffnen“: nach der
/// Rückfrage „Zum Stapel hinzufügen“ (`mode: add`) bildet die Datei mit dem geparkten Dokument den Stapel, „Offene
/// schließen und neu beginnen“ (`mode: replace`) verwirft ihn. Keine Sitzung (F-54).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sign_recent_while_parked_joins_or_replaces_like_open_files() {
    let fx = Fixture::new();
    let out = fx.dir("out");
    let files = inputs(&fx, &["a.pdf", "Vertrag_signed.pdf"]);
    let store = pida_config::RecentStore::new(&fx.paths);
    let entry = store
        .push(pida_config::RecentEntry {
            id: 0,
            file_name: "Vertrag_signed.pdf".into(),
            path: files[1].clone(),
            signed_at: "2026-10-16T10:00:00+02:00".into(),
        })
        .unwrap();
    let atrust = Atrust::new(Vec::new());
    let core = start(&fx, overlay(vec![files[0].clone()], &out), fake(atrust.backends())).unwrap();
    wait_for(&core, "Arbeitsbereich a", |v| v.document.as_ref().is_some_and(|d| d.doc.name == "a.pdf")).await;
    core.dispatch(UiEvent::SetInvisible { invisible: true });
    wait_for(&core, "unsichtbar", |v| v.document.as_ref().is_some_and(|d| d.invisible)).await;
    core.dispatch(UiEvent::GoHome);
    let v = start_with_parked(&core).await;
    assert!(v.recent.as_ref().is_some_and(|r| r.iter().any(|i| i.id == entry.id && i.exists)));
    // „Zum Stapel hinzufügen“
    core.dispatch(UiEvent::SignRecent { id: entry.id, mode: Some(pida_core_api::OpenFilesMode::Add) });
    let v = overview_ready(&core, 2).await;
    assert!(v.parked.is_none());
    let a = batch(&v).items.iter().find(|i| i.name == "a.pdf").unwrap();
    assert_eq!(a.position, PositionSummaryView::Invisible, "Plan des geparkten Dokuments bleibt");
    assert!(batch(&v).items.iter().any(|i| i.name == "Vertrag_signed.pdf"));
    // „Offene schließen und neu beginnen“: der Stapel wird verworfen, die Datei öffnet im Arbeitsbereich
    core.dispatch(UiEvent::GoHome);
    start_with_parked(&core).await;
    core.dispatch(UiEvent::SignRecent { id: entry.id, mode: Some(pida_core_api::OpenFilesMode::Replace) });
    let v =
        wait_for(&core, "Arbeitsbereich", |v| v.document.as_ref().is_some_and(|d| d.doc.name == "Vertrag_signed.pdf"))
            .await;
    assert!(v.batch.is_none() && v.parked.is_none());
    assert!(v.done.iter().any(|d| d.name == "a.pdf" && d.result == pida_workflow::DocResult::Cancelled));
    assert_eq!(atrust.n(&atrust.opened), 0, "keine Sitzung (F-54)");
}

/// Ein geparkter Stapel bleibt vollständig: die Voranalyse läuft weiter, „Alle signieren“ wirkt erst nach „Weiter
/// bearbeiten“, der Lauf signiert dann alle (eine Sitzung).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parked_batch_keeps_preanalysing_and_signs_only_after_resume() {
    let fx = Fixture::new();
    let out = fx.dir("out");
    let core = start(&fx, keystore_overlay(inputs(&fx, &["a.pdf", "b.pdf", "c.pdf"]), &out), fake(None)).unwrap();
    // sofort parken, noch während der Voranalyse
    wait_for(&core, "Übersicht", |v| matches!(v.step, StepView::Batch)).await;
    core.dispatch(UiEvent::GoHome);
    start_with_parked(&core).await;
    let docs: Vec<DocId> = (1..=3).map(DocId).collect();
    core.dispatch(UiEvent::StartBatch {
        backend: BackendId::Keystore,
        docs,
        output_dir: None,
        on_existing: ExistingPolicy::KeepBoth,
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(core.view().parked.is_some(), "verdeckt: kein Lauf");
    core.dispatch(UiEvent::ResumeParked);
    let v = overview_ready(&core, 3).await;
    assert!(batch(&v).items.iter().all(|i| matches!(i.status, BatchItemStatusView::Ready)), "Voranalyse fertig");
    start_batch(&core, &v, BackendId::Keystore, ExistingPolicy::KeepBoth, None);
    let v = summary(&core).await;
    let s = batch(&v).summary.clone().unwrap();
    assert_eq!((s.total, s.signed), (3, 3));
}

/// Meldung des Auftraggebers (ADR 0004 Nachtrag 13): ein aus der Übersicht geöffnetes Dokument parken – „Weiter
/// bearbeiten“ führt zur Stapelübersicht, nicht ins Dokument; die Platzierung ist Plan des Eintrags („Positioniert“).
/// Die Karte nennt den Stapel. Ebenso ein Einzeldokument mit wartenden Dokumenten. Keine Sitzung (F-54).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resume_of_a_parked_batch_document_shows_the_overview_with_its_plan() {
    let fx = Fixture::new();
    let out = fx.dir("out");
    let files = inputs(&fx, &["a.pdf", "b.pdf", "c.pdf", "d.pdf"]);
    let atrust = Atrust::new(Vec::new());
    // mit PDFium (Sticker, Verschieben), sonst „Unsichtbar signieren“ als eigener Plan
    let o = Opts { backends: atrust.backends(), engine: Some(FakeEngine::new()), pdfium: pdfium_dir().is_some() };
    let core = start(&fx, overlay(files[..3].to_vec(), &out), o).unwrap();
    let v = overview_ready(&core, 3).await;
    let b = doc_of(&v, "b.pdf");
    core.dispatch(UiEvent::OpenBatchItem { doc: b });
    wait_for(&core, "Arbeitsbereich b", |v| {
        v.document.as_ref().is_some_and(|d| d.doc.id == b && d.phase == pida_core_api::DocumentPhase::Editing)
    })
    .await;
    // eigener Plan (ohne PDFium: „Unsichtbar signieren“)
    let expected = if pdfium_dir().is_some() {
        let v = wait_for(&core, "Sticker", |v| v.document.as_ref().is_some_and(|d| d.sticker.is_some())).await;
        let d = v.document.unwrap();
        core.dispatch(UiEvent::MoveBlock { page: 0, x: 60.0, y: 400.0 });
        wait_for(&core, "verschoben", |v| {
            v.document.as_ref().is_some_and(|x| x.revision > d.revision && x.placement.x == 60.0)
        })
        .await;
        PositionSummaryView::Manual { page: 0 }
    } else {
        core.dispatch(UiEvent::SetInvisible { invisible: true });
        wait_for(&core, "unsichtbar", |v| v.document.as_ref().is_some_and(|d| d.invisible)).await;
        PositionSummaryView::Invisible
    };
    core.dispatch(UiEvent::GoHome);
    let v = start_with_parked(&core).await;
    let p = parked(&v).unwrap();
    assert_eq!((p.doc_name.as_deref(), p.documents, p.placed_by_hand), (None, 3, pdfium_dir().is_some()));
    // „Weiter bearbeiten“: die Übersicht mit allen Einträgen, b mit seinem Plan
    core.dispatch(UiEvent::ResumeParked);
    let v = wait_for(&core, "Übersicht", |v| matches!(v.step, StepView::Batch)).await;
    assert!(v.parked.is_none() && v.document.is_none() && batch(&v).open_item.is_none());
    assert_eq!(batch(&v).items.len(), 3);
    let item = batch(&v).items.iter().find(|i| i.doc == b).unwrap();
    assert_eq!((item.status.clone(), item.position.clone()), (BatchItemStatusView::Positioned, expected.clone()));
    // erneut öffnen: der Plan gilt wieder
    core.dispatch(UiEvent::OpenBatchItem { doc: b });
    let v = wait_for(&core, "b mit Plan", |v| {
        v.document.as_ref().is_some_and(|d| d.doc.id == b && d.phase == pida_core_api::DocumentPhase::Editing)
    })
    .await;
    if expected == PositionSummaryView::Invisible {
        assert!(v.document.unwrap().invisible, "Plan des Eintrags");
    }

    // Einzeldokument mit einem wartenden: „Stapel · 2 Dokumente“, „Weiter bearbeiten“ → Übersicht
    let core = start(&fx, overlay(vec![files[3].clone()], &out), fake(atrust.backends())).unwrap();
    wait_for(&core, "Arbeitsbereich d", |v| {
        v.document.as_ref().is_some_and(|d| d.doc.name == "d.pdf" && d.phase == pida_core_api::DocumentPhase::Editing)
    })
    .await;
    core.dispatch(UiEvent::SetInvisible { invisible: true });
    wait_for(&core, "unsichtbar", |v| v.document.as_ref().is_some_and(|d| d.invisible)).await;
    core.dispatch(UiEvent::OpenFiles { paths: vec![files[0].clone()], mode: None });
    wait_for(&core, "wartet", |v| v.actions.show_batch).await;
    core.dispatch(UiEvent::GoHome);
    let v = start_with_parked(&core).await;
    let p = parked(&v).unwrap();
    assert_eq!((p.doc_name.as_deref(), p.documents), (None, 2));
    core.dispatch(UiEvent::ResumeParked);
    let v = overview_ready(&core, 2).await;
    let d = batch(&v).items.iter().find(|i| i.name == "d.pdf").unwrap();
    assert_eq!(d.position, PositionSummaryView::Invisible, "Plan des geparkten Dokuments");
    assert!(v.parked.is_none());
    assert_eq!(atrust.n(&atrust.opened), 0, "keine Sitzung (F-54)");
}

/// Das für einen geöffneten Eintrag gewählte Signiermittel (Konto-Chip; „Jedes Mal fragen“: gilt für dieses
/// Dokument, §8 F12) geht auf dem Weg Dokument → Startseite → Übersicht nicht verloren: erneut geöffnet ist es
/// wieder gewählt, „Unsichtbar signieren“ wieder Plan (Nachtrag 13). Keine Sitzung (F-54).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn signer_choice_of_a_parked_entry_survives_the_overview() {
    let fx = Fixture::new();
    let out = fx.dir("out");
    let atrust = Atrust::new(Vec::new());
    let core = start(&fx, overlay(inputs(&fx, &["a.pdf", "b.pdf"]), &out), fake(atrust.backends())).unwrap();
    let v = overview_ready(&core, 2).await;
    assert_eq!(v.signers.default, None, "„Jedes Mal fragen“");
    let b = doc_of(&v, "b.pdf");
    let editing = move |v: &ViewState| {
        v.document.as_ref().is_some_and(|d| d.doc.id == b && d.phase == pida_core_api::DocumentPhase::Editing)
    };
    core.dispatch(UiEvent::OpenBatchItem { doc: b });
    wait_for(&core, "Arbeitsbereich b", editing).await;
    select(&core, BackendId::AtrustNative);
    core.dispatch(UiEvent::SetInvisible { invisible: true });
    wait_for(&core, "gewählt", |v| {
        v.signers.selected == Some(BackendId::AtrustNative) && v.document.as_ref().is_some_and(|d| d.invisible)
    })
    .await;
    core.dispatch(UiEvent::GoHome);
    start_with_parked(&core).await;
    core.dispatch(UiEvent::ResumeParked);
    let v = wait_for(&core, "Übersicht", |v| matches!(v.step, StepView::Batch) && v.parked.is_none()).await;
    let item = batch(&v).items.iter().find(|i| i.doc == b).unwrap();
    assert_eq!(item.position, PositionSummaryView::Invisible);
    core.dispatch(UiEvent::OpenBatchItem { doc: b });
    let v = wait_for(&core, "b erneut", editing).await;
    assert_eq!(v.signers.selected, Some(BackendId::AtrustNative), "Wahl für dieses Dokument bleibt");
    assert!(v.document.unwrap().invisible, "Plan des Eintrags");
    assert_eq!(atrust.n(&atrust.opened), 0, "keine Sitzung (F-54)");
}

// ------------------------------------------------------------------------------------------------
// Punkt 7: vorhandene Zieldateien, Ordnerwahl
// ------------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn existing_targets_keep_both_skip_and_folder_choice() {
    // „Beide behalten“: (2), beim nächsten Mal (3)
    let fx = Fixture::new();
    let out = fx.dir("out");
    std::fs::write(out.join("a_signed.pdf"), b"alt").unwrap();
    for (round, expected) in [(1, "a_signed (2).pdf"), (2, "a_signed (3).pdf")] {
        let engine = FakeEngine::new();
        let names = ["a.pdf".to_owned(), format!("x{round}.pdf")];
        let files = inputs(&fx, &[names[0].as_str(), names[1].as_str()]);
        let core = start(&fx, keystore_overlay(files, &out), fake(None).with_engine(engine)).unwrap();
        let v = overview_ready(&core, 2).await;
        let a = batch(&v).items.iter().find(|i| i.name == "a.pdf").unwrap();
        assert!(a.target_exists, "Vorabprüfung");
        start_batch(&core, &v, BackendId::Keystore, ExistingPolicy::KeepBoth, None);
        summary(&core).await;
        assert!(out.join(expected).is_file(), "{expected}");
        assert_eq!(std::fs::read(out.join("a_signed.pdf")).unwrap(), b"alt", "nie ersetzt");
    }
    // „Überspringen“: keine Bestätigung (kein `prepare`) für dieses Dokument
    let engine = FakeEngine::new();
    let core =
        start(&fx, keystore_overlay(inputs(&fx, &["a.pdf", "y.pdf"]), &out), fake(None).with_engine(engine.clone()))
            .unwrap();
    let v = overview_ready(&core, 2).await;
    start_batch(&core, &v, BackendId::Keystore, ExistingPolicy::Skip, None);
    let v = summary(&core).await;
    assert_eq!(item_status(&v, "a.pdf"), BatchItemStatusView::Skipped { existing: true });
    assert!(matches!(item_status(&v, "y.pdf"), BatchItemStatusView::Signed { .. }));
    assert_eq!(engine.prepares.load(Ordering::SeqCst), 1, "nur y vorbereitet");
    drop(core);
    // „Überschreiben“
    let core = start(&fx, keystore_overlay(inputs(&fx, &["a.pdf", "z.pdf"]), &out), fake(None)).unwrap();
    let v = overview_ready(&core, 2).await;
    start_batch(&core, &v, BackendId::Keystore, ExistingPolicy::Overwrite, None);
    summary(&core).await;
    assert_ne!(std::fs::read(out.join("a_signed.pdf")).unwrap(), b"alt", "ersetzt");
    drop(core);

    // „jedes Mal fragen“ (kein Ausgabeordner): Ordnerwahl vor dem Start – Bestandsnutzer (Datei ohne
    // `output.folder`); eine Neuinstallation speichert „neben dem Original“ (ADR 0004 §3.12, §8 F4)
    let fx = Fixture::new();
    fx.write_settings("[general]\n");
    let mut o = keystore_overlay(inputs(&fx, &["a.pdf", "b.pdf"]), &fx.dir("unused"));
    o.output = None;
    let core = start(&fx, o, fake(None)).unwrap();
    let v = overview_ready(&core, 2).await;
    assert_eq!((batch(&v).output.mode, batch(&v).output.needs_folder), (OutputModeView::Ask, true));
    start_batch(&core, &v, BackendId::Keystore, ExistingPolicy::KeepBoth, None);
    wait_for(&core, "Hinweis", |v| v.notices.iter().any(|n| n.code == "ng-batch-output-folder-required")).await;
    assert!(matches!(core.view().step, StepView::Batch));
    let chosen = fx.dir("gewählt");
    start_batch(&core, &v, BackendId::Keystore, ExistingPolicy::KeepBoth, Some(chosen.clone()));
    let v = summary(&core).await;
    assert!(chosen.join("a_signed.pdf").is_file() && chosen.join("b_signed.pdf").is_file());
    assert_eq!(
        batch(&v).summary.as_ref().unwrap().folder_display.as_deref(),
        Some(chosen.display().to_string().as_str())
    );
    drop(core);
    // Ausgabeordner ist ein Dateipfad (F-82): sonst landeten alle Dokumente in derselben Datei
    let fx = Fixture::new();
    let file_target = fx.dir("x").join("ergebnis.pdf");
    let core = start(&fx, keystore_overlay(inputs(&fx, &["a.pdf", "b.pdf"]), &file_target), fake(None)).unwrap();
    let v = overview_ready(&core, 2).await;
    assert!(batch(&v).output.needs_folder);
    // neben dem Original („.“)
    let fx = Fixture::new();
    fx.write_settings("[output]\nfolder = \".\"\n");
    let mut o = keystore_overlay(inputs(&fx, &["a.pdf", "b.pdf"]), &fx.dir("unused"));
    o.output = None;
    let core = start(&fx, o, fake(None)).unwrap();
    let v = overview_ready(&core, 2).await;
    assert_eq!((batch(&v).output.mode, batch(&v).output.needs_folder), (OutputModeView::NextToSource, false));
    assert_eq!(batch(&v).output.folder_display.as_deref(), Some(fx.dir("in").display().to_string().as_str()));
}

/// „Überschreiben“ ersetzt Dateien, die es vor dem Stapel gab – nie das Ergebnis eines anderen Eintrags
/// desselben Stapels (gleicher Dateiname aus verschiedenen Ordnern): das zweite landet daneben.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn overwrite_never_replaces_a_result_of_the_same_batch() {
    let fx = Fixture::new();
    let out = fx.dir("out");
    std::fs::write(out.join("a_signed.pdf"), b"alt").unwrap();
    let one = copy_pdf(PDF, &fx.dir("eins"), "a.pdf");
    let two = copy_pdf(TEXT_PDF, &fx.dir("zwei"), "a.pdf");
    let core = start(&fx, keystore_overlay(vec![one, two], &out), fake(None)).unwrap();
    let v = overview_ready(&core, 2).await;
    start_batch(&core, &v, BackendId::Keystore, ExistingPolicy::Overwrite, None);
    let v = summary(&core).await;
    assert_eq!(batch(&v).summary.as_ref().unwrap().signed, 2);
    let paths: Vec<PathBuf> = batch(&v)
        .items
        .iter()
        .filter_map(|i| match &i.status {
            BatchItemStatusView::Signed { path } => Some(path.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(paths, vec![out.join("a_signed.pdf"), out.join("a_signed (2).pdf")], "beide Ergebnisse bleiben");
    assert_ne!(std::fs::read(out.join("a_signed.pdf")).unwrap(), b"alt", "vorhandene Datei ersetzt");
    assert_ne!(std::fs::read(&paths[0]).unwrap(), std::fs::read(&paths[1]).unwrap());
}

/// „Abbrechen“ in einer Meldung im Lauf: nur eine A-Trust-Fehlerseite gilt als Ablehnung (Pause des Dokuments,
/// Zugangsdaten bleiben); jede andere Meldung (HTTP, Proxy, Keystore …) ist ein eigener Abbruch – wie „Stapel
/// abbrechen“ (F-72), nie „in der App abgelehnt“ (ADR 0004 §3.6, §3.8).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancel_in_a_notice_during_the_run_is_declined_only_for_atrust_error_pages() {
    for (code, declined) in [(NoticeCode::AtrustError, true), (NoticeCode::Http { status: 502 }, false)] {
        let fx = Fixture::new();
        let atrust = Atrust::new(vec![]);
        *atrust.notice.lock().unwrap() = Some(code.clone());
        let files = inputs(&fx, &["a.pdf", "b.pdf"]);
        let core = start(&fx, overlay(files, &fx.dir("out")), fake(atrust.backends())).unwrap();
        let v = overview_ready(&core, 2).await;
        start_batch(&core, &v, BackendId::AtrustNative, ExistingPolicy::KeepBoth, None);
        login(&core).await;
        let v = wait_for(&core, "Meldung", |v| {
            matches!(&v.step, StepView::Signing { interaction: InteractionView::Notice { .. }, .. })
        })
        .await;
        let StepView::Signing { interaction: InteractionView::Notice { request, .. }, .. } = v.step else {
            unreachable!()
        };
        core.dispatch(UiEvent::AnswerNotice { request, answer: NoticeAnswer::Cancel });
        if declined {
            let v = paused(&core, PauseScope::Document).await;
            assert_eq!(progress(&v).unwrap().pause.as_ref().unwrap().code, "ng-signing-declined");
            assert_eq!(atrust.n(&atrust.cancelled), 0, "{code:?}: Zugangsdaten bleiben bis zur Entscheidung");
        } else {
            let v = summary(&core).await;
            assert!(v.notices.iter().any(|n| n.code == "ng-batch-aborted"), "{code:?}: eigener Abbruch");
            assert!(!v.notices.iter().any(|n| n.code == "ng-signing-declined"));
            assert_eq!(item_status(&v, "a.pdf"), BatchItemStatusView::Cancelled);
            assert_eq!(item_status(&v, "b.pdf"), BatchItemStatusView::Ready);
        }
    }
}

/// Die Schätzung „0 Anmeldungen“ für den Keystore liest den Schlüsselbund nicht bei jeder Projektion (eine
/// Rückfrage des Systems je Zugriff bzw. ein gesperrter Schlüsselbund unter Linux wären die Folge), sondern
/// einmal – erneut erst nach einer Änderung der Einstellungen bzw. nach einem Lauf.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn estimate_reads_the_keychain_once() {
    let fx = Fixture::new();
    let keystore = testdata("keystores/test-rsa2048.p12");
    fx.write_settings(&format!("[keystore]\nenabled = true\nfile = {keystore:?}\npassword_storage = \"keychain\"\n"));
    let store = Arc::new(CountingSecrets::default());
    pida_config::SecretStore::set(&store.inner, &pida_config::SecretId::keystore_store(&keystore), "test").unwrap();
    let mut cfg = fx.config(overlay(inputs(&fx, &["a.pdf", "b.pdf"]), &fx.dir("out")), false);
    cfg.pdfium_dir = None;
    let opts = CoreOptions {
        workspace_ui: Some(true),
        batch_overview: Some(true),
        engine: Some(FakeEngine::new() as Arc<dyn pida_core_api::engine::PadesEngine>),
        secret_store: Some(store.clone() as Arc<dyn pida_config::SecretStore>),
        ..CoreOptions::isolated()
    };
    let core = Core::new_with(cfg, opts).unwrap();
    overview_ready(&core, 2).await;
    select(&core, BackendId::Keystore);
    let v = wait_for(&core, "Schätzung", |v| v.batch.as_ref().is_some_and(|b| b.estimate.is_some())).await;
    assert_eq!(batch(&v).estimate.as_ref().unwrap().logins, 0, "Passwort im Schlüsselbund");
    let before = store.keystore_reads();
    // viele Projektionen über mehr als die kurze Gültigkeit (2 s) der übrigen Abfragen
    for _ in 0..50 {
        core.dispatch(UiEvent::SelectSigner { backend: Some(BackendId::Keystore), make_default: false });
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let _ = core.view();
    assert!(store.keystore_reads() <= before + 1, "{} Zugriffe", store.keystore_reads() - before);
}

/// Schlüsselbund, der Lesezugriffe auf Keystore-Passwörter zählt.
#[derive(Debug, Default)]
struct CountingSecrets {
    inner: pida_config::MemorySecretStore,
    reads: AtomicUsize,
}

impl CountingSecrets {
    fn keystore_reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
}

impl pida_config::SecretStore for CountingSecrets {
    fn get(&self, id: &pida_config::SecretId) -> pida_config::Result<Option<Zeroizing<String>>> {
        if matches!(id, pida_config::SecretId::KeystoreStorePassword { .. }) {
            self.reads.fetch_add(1, Ordering::SeqCst);
        }
        pida_config::SecretStore::get(&self.inner, id)
    }
    fn set(&self, id: &pida_config::SecretId, value: &str) -> pida_config::Result<()> {
        pida_config::SecretStore::set(&self.inner, id, value)
    }
    fn delete(&self, id: &pida_config::SecretId) -> pida_config::Result<()> {
        pida_config::SecretStore::delete(&self.inner, id)
    }
    fn is_persistent(&self) -> bool {
        true
    }
}

impl Opts {
    fn with_engine(mut self, engine: Arc<FakeEngine>) -> Opts {
        self.engine = Some(engine);
        self
    }
}

// ------------------------------------------------------------------------------------------------
// Punkt 9: D-06 ohne Übersicht; echte Engine mit pyHanko
// ------------------------------------------------------------------------------------------------

/// `-b KS -ks … -a -s a.pdf b.pdf` signiert mit eingeschalteter Stapelübersicht unverändert ohne Übersicht.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn start_arguments_with_keystore_stay_without_the_overview() {
    let fx = Fixture::new();
    let out = fx.dir("out");
    let mut o = keystore_overlay(inputs(&fx, &["a.pdf", "b.pdf"]), &out);
    o.backend = Some(pida_config::BackendChoice::Keystore);
    o.auto_position = true;
    o.skip_finish = true;
    let core = start(&fx, o, fake(None)).unwrap();
    let mut rx = core.subscribe();
    let watcher = tokio::spawn(async move {
        let mut overview = false;
        loop {
            overview |= matches!(rx.borrow_and_update().step, StepView::Batch) || rx.borrow().batch.is_some();
            if rx.changed().await.is_err() {
                return overview;
            }
        }
    });
    let code = tokio::time::timeout(Duration::from_secs(30), core.wait_finished()).await.unwrap();
    assert_eq!(code, ExitCode::Success, "{:#?}", core.view());
    assert!(out.join("a_signed.pdf").is_file() && out.join("b_signed.pdf").is_file());
    drop(core);
    assert!(!watcher.await.unwrap(), "nie eine Stapelübersicht (D-06)");
}

/// Stapel mit dem Keystore und der echten Engine: beide Ergebnisse prüft pyHanko (und qpdf).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn keystore_batch_with_the_real_engine_validates_with_pyhanko() {
    let fx = Fixture::new();
    let out = fx.dir("out");
    let input = fx.dir("in");
    let files = vec![copy_pdf(TEXT_PDF, &input, "Vertrag.pdf"), copy_pdf(PDF, &input, "Beilage.pdf")];
    let pdfium = pdfium_dir().is_some();
    let o = Opts { backends: None, engine: None, pdfium };
    let core = start(&fx, keystore_overlay(files, &out), o).unwrap();
    let v = overview_ready(&core, 2).await;
    start_batch(&core, &v, BackendId::Keystore, ExistingPolicy::KeepBoth, None);
    let v = summary(&core).await;
    assert_eq!(batch(&v).summary.as_ref().unwrap().signed, 2);
    for name in ["Vertrag_signed.pdf", "Beilage_signed.pdf"] {
        let signed = out.join(name);
        assert!(signed.is_file(), "{name}");
        if tool("pyhanko") {
            let o =
                Command::new("pyhanko").args(["sign", "validate", "--no-strict-syntax"]).arg(&signed).output().unwrap();
            let text = String::from_utf8_lossy(&o.stdout);
            assert!(text.contains("INTACT") || text.contains("intact"), "pyHanko {name}: {text}");
        } else {
            eprintln!("pyHanko fehlt – Orakel übersprungen");
        }
        if tool("qpdf") {
            let o = Command::new("qpdf").arg("--check").arg(&signed).output().unwrap();
            assert!(o.status.success(), "qpdf {name}: {}", String::from_utf8_lossy(&o.stdout));
        }
    }
}

// ------------------------------------------------------------------------------------------------
// Zustände des echten Kerns für die Stapel-Oberfläche (A2)
// ------------------------------------------------------------------------------------------------

/// Echte `ViewState`s des Stapels für die Oberfläche (A2, ADR 0004 §6.2 Punkt 4; eigene Datei neben
/// `real-core-views.json`, damit deren UI-Probe unverändert bleibt): Übersicht, Anmeldung, Bestätigung k/n,
/// Pause (Verbindung), Zusammenfassung. Neu schreiben:
/// `PIDA_BLESS=1 cargo test -p pida-core-api --test batch real_core_batch_views`.
const UI_FIXTURE: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../apps/app/src/core/fixtures/real-core-batch-views.json");

fn fixture_state(v: &ViewState, roots: &[(String, &str)]) -> serde_json::Value {
    fn walk(value: &mut serde_json::Value, roots: &[(String, &str)]) {
        match value {
            serde_json::Value::String(s) => {
                for (from, to) in roots {
                    if s.contains(from.as_str()) {
                        *s = s.replace(from.as_str(), to);
                    }
                }
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(|i| walk(i, roots)),
            serde_json::Value::Object(map) => {
                for (key, item) in map.iter_mut() {
                    if matches!(key.as_str(), "signing_time" | "occurred_at" | "signed_at") && item.is_string() {
                        *item = serde_json::Value::String("2026-10-15T10:42:17+02:00".into());
                    } else if key == "release_date" && item.is_string() {
                        // Versionsdatum des gebauten Stands („Über PIDA“, Nachtrag 21) – je Commit anders
                        *item = serde_json::Value::String("2026-10-17".into());
                    } else {
                        walk(item, roots);
                    }
                }
            }
            _ => {}
        }
    }
    let mut value = serde_json::to_value(v).expect("ViewState serialisierbar");
    walk(&mut value, roots);
    value["language"] = serde_json::Value::String("de".into());
    value
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_core_batch_views_for_the_ui() {
    let fx = Fixture::new();
    fx.write_settings("[general]\n"); // Bestandsnutzer: ohne Ersteinrichtung (K4)
    let tmp = fx.tmp.path().to_path_buf();
    let roots: Vec<(String, &str)> = [tmp.clone(), tmp.canonicalize().unwrap_or(tmp)]
        .into_iter()
        .map(|p| (p.display().to_string(), "/Users/test"))
        .collect();
    let out = fx.dir("Dokumente");
    let mut files = inputs(&fx, &["Vertrag.pdf", "Antrag.pdf", "Beilage.pdf"]);
    files.insert(1, copy_pdf(PASSWORD_PDF, &fx.dir("in"), "Geheim.pdf"));
    let atrust = Atrust::new(vec![(2, BackendError::Network("Verbindung getrennt".into()))]);
    let core = start(&fx, overlay(files, &out), fake(atrust.backends())).unwrap();
    let mut states: Vec<(&str, ViewState)> = Vec::new();
    let v = overview_ready(&core, 4).await;
    states.push(("batch_overview", v.clone()));
    select(&core, BackendId::AtrustNative);
    let v = wait_for(&core, "Schätzung", |v| v.batch.as_ref().is_some_and(|b| b.estimate.is_some())).await;
    states.push(("batch_overview_estimate", v.clone()));
    // „Für diesen Stapel automatisch positionieren“ (Nachtrag 8)
    core.dispatch(UiEvent::SetBatchAutoPosition { enabled: true, replace_manual: false });
    let v = wait_for(&core, "für diesen Stapel", |v| v.batch.as_ref().is_some_and(|b| b.auto_position_once)).await;
    states.push(("batch_auto_once", v.clone()));
    // „Zur Startseite“ parkt den Stapel (Nachtrag 10), „Weiter bearbeiten“ bringt ihn unverändert zurück
    core.dispatch(UiEvent::GoHome);
    states.push(("batch_parked", start_with_parked(&core).await));
    core.dispatch(UiEvent::ResumeParked);
    let resumed = wait_for(&core, "fortgesetzt", |v| v.batch.as_ref().is_some_and(|b| b.auto_position_once)).await;
    assert_eq!(resumed.batch, v.batch, "Stapel unverändert");
    start_batch(&core, &v, BackendId::AtrustNative, ExistingPolicy::KeepBoth, None);
    let v = wait_for(&core, "Anmeldung", |v| form(v).is_some_and(|(_, f)| matches!(f, NativeForm::Login { .. }))).await;
    states.push(("batch_login", v));
    login(&core).await;
    let (v, _) = confirmation(&core, 1).await;
    states.push(("batch_confirm_1", v.clone()));
    tan(&core, &v);
    let v = paused(&core, PauseScope::Session).await;
    states.push(("batch_paused_connection", v));
    core.dispatch(UiEvent::Retry);
    let (v, _) = confirmation(&core, 2).await;
    states.push(("batch_confirm_2", v.clone()));
    core.dispatch(UiEvent::SkipBatchDocument);
    let (v, _) = confirmation(&core, 3).await;
    tan(&core, &v);
    let v = summary(&core).await;
    assert!(!batch(&v).auto_position_once, "nach dem Lauf wieder laut Einstellung");
    states.push(("batch_summary", v));
    drop(core);

    // Meldung des Auftraggebers (Nachtrag 13): ein aus der Übersicht geöffneter Eintrag, geparkt – die Karte nennt
    // den Stapel, „Weiter bearbeiten“ führt zur Übersicht
    let core = start(&fx, overlay(inputs(&fx, &["Angebot.pdf", "Auftrag.pdf"]), &out), fake(None)).unwrap();
    let v = overview_ready(&core, 2).await;
    let doc = doc_of(&v, "Auftrag.pdf");
    core.dispatch(UiEvent::OpenBatchItem { doc });
    wait_for(&core, "Eintrag geöffnet", |v| v.document.as_ref().is_some_and(|d| d.doc.id == doc)).await;
    core.dispatch(UiEvent::GoHome);
    let v = start_with_parked(&core).await;
    assert_eq!(v.parked.as_ref().map(|p| (p.doc_name.clone(), p.documents)), Some((None, 2)));
    states.push(("batch_entry_parked", v));
    core.dispatch(UiEvent::ResumeParked);
    states.push(("batch_entry_resumed", overview_ready(&core, 2).await));
    drop(core);

    let fixture: serde_json::Map<String, serde_json::Value> =
        states.iter().map(|(name, v)| ((*name).to_owned(), fixture_state(v, &roots))).collect();
    if std::env::var_os("PIDA_BLESS").is_some() {
        let text = serde_json::to_string_pretty(&fixture).unwrap() + "\n";
        std::fs::write(UI_FIXTURE, text).expect("Fixture schreiben");
        return;
    }
    let text = std::fs::read_to_string(UI_FIXTURE).expect("Fixture lesen (PIDA_BLESS=1 erzeugt sie)");
    let stored: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&text).unwrap();
    for name in fixture.keys() {
        assert!(stored.contains_key(name), "real-core-batch-views.json ohne {name}: PIDA_BLESS=1 neu erzeugen");
    }
    for (name, value) in stored {
        let _: ViewState = serde_json::from_value(value)
            .unwrap_or_else(|e| panic!("{name}: real-core-batch-views.json passt nicht mehr zum Vertrag ({e})"));
    }
}
