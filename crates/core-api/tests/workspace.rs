//! Oberfläche „Arbeitsbereich“ gegen den echten Kern (ADR 0004 §3.2–§3.7, Paket K1): Kernpfad ohne
//! Bildschirmwechsel (Keystore, echte Engine, pyHanko), Invariante A1/F-54 (keine Sitzung ohne „Signieren
//! mit“), Startargument-Automatik (D-06), Abbruch zurück in den Arbeitsbereich, Wiederholen in derselben
//! Sitzung, Ablehnung in der App, ohne PDFium, Ziele (Felder, QR), Einschränkungen, Layout und Überdeckung
//! ohne Renderer im Ereignispfad. PDFium-Fälle laufen nur mit `PIDA_PDFIUM_DIR` (sonst übersprungen).

mod common;

use std::collections::VecDeque;
use std::path::PathBuf;
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
    CertificateSource, CoreOptions, DocumentPhase, ErrorOrigin, FormAnswerView, InteractionView, PlacementTarget,
    SigningStage, StepView, UiEvent, ViewState,
};
use pida_workflow::{ExitCode, PlacementOrigin};
use zeroize::Zeroizing;

const PDF: &str = "pdf/generated/simple-xref-table.pdf";
const TEXT_PDF: &str = "pdf/pdfas/pdf-as-legacy/simple.pdf";

fn pdfium_dir() -> Option<PathBuf> {
    std::env::var_os("PIDA_PDFIUM_DIR").filter(|d| !d.is_empty()).map(Into::into)
}

fn tool(name: &str) -> bool {
    Command::new(name).arg("--version").output().is_ok_and(|o| o.status.success())
}

/// Kern im Arbeitsbereich-Modus; `pdfium` = mit PDFium (sonst `None`, wenn es fehlt).
fn start(
    fx: &Fixture,
    overlay: Overlay,
    backends: Option<Vec<Arc<dyn SignatureBackend>>>,
    engine: Option<Arc<FakeEngine>>,
    pdfium: bool,
) -> Option<pida_core_api::Core> {
    let mut cfg = fx.config(overlay, false);
    if pdfium {
        cfg.pdfium_dir = Some(pdfium_dir()?);
    }
    let opts = CoreOptions {
        workspace_ui: Some(true),
        backends,
        engine: engine.map(|e| e as Arc<dyn pida_core_api::engine::PadesEngine>),
        use_pdfium: pdfium,
        ..CoreOptions::isolated()
    };
    Some(pida_core_api::Core::new_with(cfg, opts).expect("Kern startet"))
}

fn keystore_overlay(inputs: Vec<PathBuf>, out: &std::path::Path) -> Overlay {
    Overlay {
        input_files: inputs,
        keystore_file: Some(testdata("keystores/test-rsa2048.p12")),
        keystore_store_password: Some(Zeroizing::new("test".into())),
        output: Some(out.to_path_buf()),
        ..Overlay::default()
    }
}

fn editing(v: &ViewState) -> bool {
    matches!(v.step, StepView::Position { .. })
        && v.document.as_ref().is_some_and(|d| d.phase == DocumentPhase::Editing)
}

fn saved(v: &ViewState) -> bool {
    matches!(&v.step, StepView::Output { result, .. } if result.path.is_some())
}

/// Sammelt alle beobachteten Zustände, bis `pred` gilt (höchstens 30 s).
async fn collect_until(core: &pida_core_api::Core, what: &str, pred: impl Fn(&ViewState) -> bool) -> Vec<ViewState> {
    let mut rx = core.subscribe();
    let mut seen = Vec::new();
    let r = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let v = rx.borrow_and_update().clone();
            let done = pred(&v);
            seen.push(v);
            if done {
                return;
            }
            if rx.changed().await.is_err() {
                panic!("Kern beendet");
            }
        }
    })
    .await;
    if r.is_err() {
        panic!("Zeitüberschreitung beim Warten auf {what}: {:#?}", core.view());
    }
    seen
}

async fn form_request(core: &pida_core_api::Core, what: &str, pred: impl Fn(&NativeForm) -> bool) -> u64 {
    let v = wait_for(
        core,
        what,
        |v| matches!(&v.step, StepView::Signing { interaction: InteractionView::Form { form, .. }, .. } if pred(form)),
    )
    .await;
    let StepView::Signing { interaction: InteractionView::Form { request, .. }, .. } = v.step else { unreachable!() };
    request
}

async fn login(core: &pida_core_api::Core) {
    let request = form_request(core, "Anmeldung", |f| matches!(f, NativeForm::Login { .. })).await;
    let answer =
        FormAnswerView::Login { username: "+436641234567".into(), password: "geheim1".into(), remember: false };
    core.dispatch(UiEvent::SubmitForm { request, answer });
}

async fn tan(core: &pida_core_api::Core) {
    let request = form_request(core, "TAN", |f| matches!(f, NativeForm::SmsTan { .. })).await;
    core.dispatch(UiEvent::SubmitForm { request, answer: FormAnswerView::Tan { tan: "123456".into() } });
}

// ------------------------------------------------------------------------------------------------
// ID-Austria-artiges Fake-Backend mit Skript: Anmeldung einmal je Sitzung, Fehler je `sign`-Aufruf
// ------------------------------------------------------------------------------------------------

#[derive(Debug)]
struct Scripted {
    key: Arc<LoadedKey>,
    opened: AtomicUsize,
    cancelled: AtomicUsize,
    certificates: AtomicUsize,
    logins: AtomicUsize,
    signs: AtomicUsize,
    /// Fehler der nächsten `sign`-Aufrufe (danach Erfolg)
    failures: Mutex<VecDeque<BackendError>>,
    /// Meldung beim nächsten `sign` (Wiederholen/Abbrechen; „Abbrechen“ → `Cancelled` wie im A-Trust-Dialog)
    notice: Mutex<Option<NoticeCode>>,
}

impl Scripted {
    fn new(failures: Vec<BackendError>) -> Arc<Scripted> {
        Arc::new(Scripted {
            // wie ein echtes ID-Austria-Zertifikat qualifiziert laut Zertifikat (qcStatements, K4)
            key: qes_test_key(),
            opened: AtomicUsize::new(0),
            cancelled: AtomicUsize::new(0),
            certificates: AtomicUsize::new(0),
            logins: AtomicUsize::new(0),
            signs: AtomicUsize::new(0),
            failures: Mutex::new(failures.into()),
            notice: Mutex::new(None),
        })
    }
}

#[derive(Debug)]
struct ScriptedBackend(Arc<Scripted>);

#[async_trait]
impl SignatureBackend for ScriptedBackend {
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
        Ok(Box::new(ScriptedSession { backend: self.0.clone(), ui, logged_in: false }))
    }
}

struct ScriptedSession {
    backend: Arc<Scripted>,
    ui: Arc<dyn InteractionBroker>,
    logged_in: bool,
}

fn interaction(e: InteractionError) -> BackendError {
    match e {
        InteractionError::Cancelled => BackendError::Cancelled,
        InteractionError::Unsupported => BackendError::Unavailable("keine Oberfläche".into()),
    }
}

impl ScriptedSession {
    /// wie A-Trust: eine Anmeldung je Sitzung (Zugangsdaten bleiben in der Sitzung)
    async fn ensure_login(&mut self) -> Result<(), BackendError> {
        if self.logged_in {
            return Ok(());
        }
        let form = NativeForm::Login { prefill_username: None, remember: false, ctx: FormContext::default() };
        match self.ui.native_form(form).await.map_err(interaction)? {
            FormAnswer::Login { username, remember, .. } => {
                self.backend.logins.fetch_add(1, Ordering::SeqCst);
                self.ui.event(BackendEvent::LoginSucceeded { username, remember });
                self.logged_in = true;
                Ok(())
            }
            _ => Err(BackendError::Protocol("unerwartet".into())),
        }
    }
}

#[async_trait]
impl BackendSession for ScriptedSession {
    async fn certificate(&mut self) -> Result<CertChain, BackendError> {
        self.backend.certificates.fetch_add(1, Ordering::SeqCst);
        self.ensure_login().await?;
        Ok(self.backend.key.chain().clone())
    }

    async fn sign(&mut self, jobs: Vec<SignJob>) -> Result<Vec<SignResult>, BackendError> {
        self.backend.signs.fetch_add(1, Ordering::SeqCst);
        self.ensure_login().await?;
        let failure = self.backend.failures.lock().unwrap().pop_front();
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
        let form = NativeForm::SmsTan { reference_value: "AB12".into(), ctx: FormContext::default() };
        match self.ui.native_form(form).await.map_err(interaction)? {
            FormAnswer::Tan(t) if t.as_str() == "123456" => {}
            _ => return Err(BackendError::Protocol("TAN falsch".into())),
        }
        let mut out = Vec::new();
        for job in jobs {
            let SignJob::Document(d) = job else { return Err(BackendError::Internal("nur Modell B".into())) };
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
        self.logged_in = false;
        self.backend.cancelled.fetch_add(1, Ordering::SeqCst);
    }
}

fn scripted(failures: Vec<BackendError>) -> (Arc<Scripted>, Option<Vec<Arc<dyn SignatureBackend>>>) {
    let s = Scripted::new(failures);
    let backends: Vec<Arc<dyn SignatureBackend>> = vec![Arc::new(ScriptedBackend(s.clone()))];
    (s, Some(backends))
}

// ------------------------------------------------------------------------------------------------
// Kernpfad (Abnahme E1 Punkt 1, 7)
// ------------------------------------------------------------------------------------------------

/// Datei → Arbeitsbereich (Startlage = PDF-AS-Automatik wie `-a`) → „Signieren mit Keystore“ → Fertig, ohne
/// dass die Dokumentansicht wegfällt; echte Engine, pyHanko validiert, Zusammenfassung mit Unterzeichner.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn keystore_core_path_without_screen_change() {
    let fx = Fixture::new();
    let input = fx.dir("in");
    let out = fx.dir("out");
    let a = copy_pdf(TEXT_PDF, &input, "doc.pdf");
    let pdfium = pdfium_dir().is_some();
    let core = start(&fx, keystore_overlay(vec![a.clone()], &out), None, None, pdfium).expect("Kern");
    let v = wait_for(&core, "Arbeitsbereich", editing).await;
    let d = v.document.as_ref().unwrap();
    assert_eq!((d.origin, d.target, d.invisible), (PlacementOrigin::Auto, PlacementTarget::Free, false));
    assert!(d.size_bytes > 0 && d.doc.page_count > 0);
    assert!(v.signers.options.iter().any(|s| s.backend == BackendId::Keystore && s.available));
    assert!(v.actions.sign && v.actions.edit_placement);
    if pdfium {
        // Startlage = Ergebnis von `PositionChoice::Auto` (PDF-AS-Regel, wie `-a`)
        let expected = expected_auto_position(&a);
        let v = wait_for(&core, "automatische Startlage", |v| {
            v.document
                .as_ref()
                .is_some_and(|d| (d.placement.x - expected.x).abs() < 0.5 && (d.placement.y - expected.y).abs() < 0.5)
        })
        .await;
        let d = v.document.unwrap();
        let n = d.doc.page_count;
        assert_eq!(d.placement.page_index, if expected.new_page { n } else { expected.page_index });
        assert_eq!(d.origin, PlacementOrigin::Auto);
        assert!(d.sticker.is_some());
    } else {
        eprintln!("PIDA_PDFIUM_DIR nicht gesetzt – Startlage ohne Vorschau geprüft");
        assert!(d.sticker.is_none());
    }
    core.dispatch(UiEvent::SignWith { backend: BackendId::Keystore });
    let seen = collect_until(&core, "gespeichert", saved).await;
    // die Seitenansicht bleibt eingehängt: in jedem beobachteten Zustand ein Dokument (Bearbeiten →
    // Signieren → Fertig), nie die Signiermittel-Auswahl
    for v in &seen {
        assert!(v.document.is_some(), "Dokumentansicht fehlt: {:?}", v.step);
        assert!(!matches!(v.step, StepView::Backend { .. }), "kein Auswahlschritt");
    }
    let phases: Vec<DocumentPhase> = seen.iter().filter_map(|v| v.document.as_ref().map(|d| d.phase)).collect();
    assert_eq!(phases.last(), Some(&DocumentPhase::Done));
    let v = seen.last().unwrap();
    let StepView::Output { result, .. } = &v.step else { unreachable!() };
    let summary = result.summary.as_ref().expect("Zusammenfassung");
    assert_eq!(summary.file_name, "doc_signed.pdf");
    assert_eq!(summary.folder_display.as_deref(), Some(out.display().to_string().as_str()));
    assert!(summary.signer.as_deref().is_some_and(|s| s.contains("NUR TEST")), "{:?}", summary.signer);
    assert!(summary.self_check);
    assert_eq!(summary.backend, BackendId::Keystore);
    assert!(summary.page.is_some(), "sichtbarer Block");
    assert!(time::OffsetDateTime::parse(&summary.signing_time, &time::format_description::well_known::Rfc3339).is_ok());
    let signed = out.join("doc_signed.pdf");
    assert!(signed.is_file());
    if tool("pyhanko") {
        let o = Command::new("pyhanko").args(["sign", "validate", "--no-strict-syntax"]).arg(&signed).output().unwrap();
        let text = String::from_utf8_lossy(&o.stdout);
        assert!(text.contains("INTACT") || text.contains("intact"), "pyHanko: {text}");
    } else {
        eprintln!("pyHanko fehlt – Orakel übersprungen");
    }
    if tool("qpdf") {
        let o = Command::new("qpdf").arg("--check").arg(&signed).output().unwrap();
        assert!(o.status.success(), "qpdf: {}", String::from_utf8_lossy(&o.stdout));
    }
    // „Nächstes Dokument“ bzw. Start: Dokumentansicht entfällt
    core.dispatch(UiEvent::Navigate { step: pida_workflow::Step::Open });
    let v = wait_for(&core, "Start", |v| matches!(v.step, StepView::Open { .. })).await;
    assert!(v.document.is_none());
}

/// Ergebnis der Automatik des Arbeitsbereichs (`plan::build_plan` mit `PositionChoice::Auto`, Widgets belegt) für
/// das Dokument.
fn expected_auto_position(path: &std::path::Path) -> pida_placement::ResolvedPosition {
    auto_position_with(path, true)
}

/// `plan::build_plan` mit `PositionChoice::Auto`; `avoid_widgets = false` = reine PDF-AS-Regel (CLI, klassisch).
fn auto_position_with(path: &std::path::Path, avoid_widgets: bool) -> pida_placement::ResolvedPosition {
    use pida_core_api::plan;
    let src = pida_pdf_cos::SourcePdf::open_file(path, &pida_pdf_cos::Limits::default()).unwrap();
    let report = pida_pades::analyze(&src).unwrap();
    let store = pida_sigblock::ProfileStore::builtin().unwrap();
    let fonts = pida_sigblock::FontProvider::builtin();
    let settings = pida_config::Settings::default();
    let overlay = Overlay::default();
    let spec = plan::block_spec(
        &store,
        plan::SpecInput {
            effective: pida_config::Effective::new(&settings, &overlay, None),
            system_language: pida_i18n::Lang::system(),
            pdfa: report.pdfa.as_ref(),
            cert: None,
            signing_time: plan::local_now(),
            emblem: None,
        },
    )
    .unwrap();
    let backend = pida_render::PdfiumBackend::bind(pdfium_dir().as_deref()).unwrap();
    let renderer = pida_render::RenderBackend::open(&backend, src.shared_bytes(), None).unwrap();
    let built = plan::build_plan(plan::PlanInput {
        report: &report,
        pages: src.pages(),
        position: &pida_workflow::PositionChoice::Auto,
        spec: &spec,
        chain: None,
        reserve: pida_pades::Reserve::ModelB,
        cms_profile: Default::default(),
        renderer: Some(renderer.as_ref()),
        avoid_widgets,
        store: &store,
        fonts: &fonts,
    })
    .unwrap();
    built.resolved.expect("sichtbare Position")
}

// ------------------------------------------------------------------------------------------------
// A1 / F-54 (Abnahme E1 Punkt 2, 5, 10)
// ------------------------------------------------------------------------------------------------

/// `-b MOBILE -a doc.pdf`: bleibt im Arbeitsbereich ohne Sitzung; erst „Signieren mit ID Austria“ öffnet sie.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn id_austria_start_arguments_stay_in_the_workspace() {
    let fx = Fixture::new();
    let a = copy_pdf(PDF, &fx.dir("in"), "a.pdf");
    let fake = FakeAtrust::new();
    let overlay = Overlay {
        input_files: vec![a],
        backend: Some(pida_config::BackendChoice::IdAustria),
        auto_position: true,
        ..Overlay::default()
    };
    let core = start(&fx, overlay, Some(vec![fake.backend()]), Some(FakeEngine::new()), false).unwrap();
    let v = wait_for(&core, "Arbeitsbereich", editing).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(fake.opened.load(Ordering::SeqCst), 0, "keine Sitzung ohne „Signieren“ (A1)");
    assert!(editing(&core.view()), "{:?}", core.view().step);
    assert_eq!(v.signers.selected, Some(BackendId::AtrustNative), "Konto-Chip: Signiermittel aus -b");
    // ohne Vorschau (isolierter Kern ohne PDFium): Hinweis, kein Sticker – signiert wird automatisch platziert
    assert!(v.document.as_ref().unwrap().sticker.is_none());
    assert!(v.notices.iter().any(|n| n.code == "error-PositioningNotPossible"));
    // Altereignis ohne Auswahl bzw. mit Auswahl: `confirm_position` → „Signieren mit“ (Konto-Chip)
    core.dispatch(UiEvent::ConfirmPosition);
    let request = form_request(&core, "Anmeldung", |f| matches!(f, NativeForm::Login { .. })).await;
    assert!(request > 0);
    assert_eq!(fake.opened.load(Ordering::SeqCst), 1);
    let v = core.view();
    assert!(matches!(v.step, StepView::Signing { document_leaves_device: true, .. }));
    let StepView::Signing { progress, .. } = &v.step else { unreachable!() };
    assert_eq!(progress.stage, SigningStage::Login);
    assert_eq!(progress.backend, Some(BackendId::AtrustNative));
    assert_eq!((progress.index, progress.total, progress.batch), (1, 1, false));
    assert_eq!(v.document.as_ref().unwrap().phase, DocumentPhase::Signing);
}

/// Einstellungen mit automatischer Position und Standard-Signiermittel (ID Austria, Keystore, Bürgerkarte):
/// im Arbeitsbereich startet nie etwas ohne „Signieren“ (§8 F14) – auch nicht der Keystore (bisher F-53).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn default_signer_with_auto_position_never_starts_by_itself() {
    for default in ["id_austria", "keystore"] {
        let fx = Fixture::new();
        let input = fx.dir("in");
        let a = copy_pdf(PDF, &input, "a.pdf");
        fx.write_settings(&format!("[signature]\nauto_position = true\n[backend]\ndefault = \"{default}\"\n"));
        let fake = FakeAtrust::new();
        let engine = FakeEngine::new();
        let mut overlay = keystore_overlay(vec![a], &fx.dir("out"));
        overlay.keystore_store_password = Some(Zeroizing::new("test".into()));
        // wie am Desktop mehr als ein Signiermittel (sonst gäbe es keine Auswahl, Nachtrag 20)
        let backends: Option<Vec<Arc<dyn SignatureBackend>>> =
            if default == "id_austria" { Some(vec![fake.backend(), IdleBackend::card()]) } else { None };
        let core = start(&fx, overlay, backends, Some(engine.clone()), false).unwrap();
        let v = wait_for(&core, "Arbeitsbereich", editing).await;
        // „Automatische Positionierung“ (Nachtrag 8): bereit zum Signieren, Platzierung gesperrt – signiert
        // wird trotzdem erst nach „Signieren mit“
        assert!(v.document.as_ref().is_some_and(|d| d.placement_locked), "{default}: gesperrt");
        assert!(v.actions.sign && v.actions.edit_placement, "{default}: Optionen und „Signieren“ bedienbar");
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(editing(&core.view()), "{default}: {:?}", core.view().step);
        assert_eq!(fake.opened.load(Ordering::SeqCst), 0, "{default}");
        assert_eq!(engine.prepares.load(Ordering::SeqCst), 0, "{default}: nichts vorbereitet");
        // Konto-Chip wählen startet nichts (A1)
        core.dispatch(UiEvent::SelectSigner { backend: Some(BackendId::Keystore), make_default: false });
        wait_for(&core, "Auswahl", |v| v.signers.selected == Some(BackendId::Keystore)).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(editing(&core.view()));
        assert_eq!(engine.prepares.load(Ordering::SeqCst), 0);
    }
}

/// Einstellung „Automatische Positionierung“ (ADR 0004 Nachtrag 8) gegen den echten Kern: beim Öffnen
/// automatisch platziert (PDF-AS-Regel) und gesperrt – `move_block`, `nudge_block`, `change_page`, `use_target`
/// und `auto_place` bleiben ohne Wirkung, „Unsichtbar“ und „Auf neuer Seite“ wirken; `unlock_placement` gibt
/// nur dieses Dokument frei (das nächste ist wieder gesperrt); ausgeschaltet (`apply_settings`) sofort frei.
/// Nie eine Sitzung bzw. Vorbereitung ohne „Signieren mit“ (F-54). Mit PDFium auch die Lage selbst.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn auto_position_locks_the_placement_until_unlocked() {
    let fx = Fixture::new();
    let input = fx.dir("in");
    let out = fx.dir("out");
    let a = copy_pdf(TEXT_PDF, &input, "a.pdf");
    let b = copy_pdf(TEXT_PDF, &input, "b.pdf");
    fx.write_settings("[signature]\nauto_position = true\n");
    let pdfium = pdfium_dir().is_some();
    let engine = FakeEngine::new();
    let core = start(&fx, keystore_overlay(vec![a.clone(), b], &out), None, Some(engine.clone()), pdfium).unwrap();
    if pdfium {
        settled(&core, true).await;
        // Die automatische Lage kommt asynchron (`auto_place` im Hintergrund, PDFium prozessweit gesperrt und von
        // den parallel laufenden Tests geteilt): auf sie warten wie `keystore_core_path_without_screen_change`
        // statt nur auf die feste Pause in `settled` (CI 17.10.2026, macOS und Linux: dort noch die Startlage
        // 50 %/25 %). Kommt sie nie, scheitert das Warten nach 20 s mit dem ganzen Zustand.
        let expected = expected_auto_position(&a);
        wait_for(&core, "automatische Lage", |v| {
            v.document
                .as_ref()
                .is_some_and(|d| (d.placement.x - expected.x).abs() < 0.5 && (d.placement.y - expected.y).abs() < 0.5)
        })
        .await;
    } else {
        // ohne Vorschau: keine Lage und keine Blockvorschau, die Sperre gilt trotzdem
        wait_for(&core, "Arbeitsbereich", editing).await;
    }
    let v = core.view();
    let d = v.document.clone().unwrap();
    assert!(d.placement_locked, "gesperrt");
    assert_eq!((d.origin, d.target), (PlacementOrigin::Auto, PlacementTarget::Free));
    assert!(d.detection.as_ref().is_some_and(|x| x.auto_placed));
    assert!(v.actions.sign && v.actions.edit_placement);
    // Verschieben aller Art bleibt ohne Wirkung (Lage und Revision unverändert)
    for e in [
        UiEvent::MoveBlock { page: 0, x: 40.0, y: 500.0 },
        UiEvent::NudgeBlock { dx: 2, dy: 1, fine: false, coarse: true },
        UiEvent::ChangePage { page: 0 },
        UiEvent::UseTarget { target: PlacementTarget::Free },
        UiEvent::AutoPlace,
    ] {
        core.dispatch(e);
    }
    tokio::time::sleep(Duration::from_millis(400)).await;
    let after = core.view().document.unwrap();
    assert_eq!((after.placement, after.revision, after.origin), (d.placement, d.revision, d.origin));
    assert!(after.placement_locked && !after.check.clamped);
    // „Unsichtbar signieren“ bleibt bedienbar
    core.dispatch(UiEvent::SetInvisible { invisible: true });
    wait_for(&core, "unsichtbar", |v| v.document.as_ref().is_some_and(|d| d.invisible && d.placement_locked)).await;
    core.dispatch(UiEvent::SetInvisible { invisible: false });
    wait_for(&core, "sichtbar", |v| v.document.as_ref().is_some_and(|d| !d.invisible)).await;
    // „Diesmal selbst platzieren“: frei, startet nichts
    core.dispatch(UiEvent::UnlockPlacement);
    let v = wait_for(&core, "freigegeben", |v| v.document.as_ref().is_some_and(|d| !d.placement_locked)).await;
    assert_eq!(v.document.as_ref().unwrap().placement, d.placement, "Lage unverändert");
    if pdfium {
        core.dispatch(UiEvent::MoveBlock { page: 0, x: 40.0, y: 500.0 });
        let v =
            wait_for(&core, "verschoben", |v| v.document.as_ref().is_some_and(|d| d.origin == PlacementOrigin::User))
                .await;
        assert_eq!(v.document.unwrap().placement.page_index, 0);
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(engine.prepares.load(Ordering::SeqCst), 0, "nichts vorbereitet ohne „Signieren mit“ (F-54)");
    // signieren → „Fertig“ → nächstes Dokument: wieder gesperrt
    core.dispatch(UiEvent::SignWith { backend: BackendId::Keystore });
    wait_for(&core, "gespeichert", saved).await;
    core.dispatch(UiEvent::Navigate { step: pida_workflow::Step::Open });
    let v = wait_for(&core, "nächstes Dokument", |v| {
        editing(v) && v.document.as_ref().is_some_and(|d| d.doc.name == "b.pdf")
    })
    .await;
    assert!(v.document.as_ref().unwrap().placement_locked, "Freigabe galt nur für das erste Dokument");
    // Einstellungen › Dokument: ausgeschaltet → sofort frei; wieder eingeschaltet → gesperrt (noch automatisch)
    core.dispatch(UiEvent::OpenSettings);
    wait_for(&core, "Einstellungen", |v| v.settings.is_some()).await;
    let apply = |on: bool, revision: u64| UiEvent::ApplySettings {
        changes: [("signature.auto_position".to_owned(), serde_json::json!(on))].into(),
        revision,
    };
    core.dispatch(apply(false, 1));
    let v = wait_for(&core, "aus", |v| v.document.as_ref().is_some_and(|d| !d.placement_locked)).await;
    let view = v.settings.as_ref().unwrap();
    assert!(!view.settings.signature.auto_position && !view.problems.contains_key("signature.auto_position"));
    let saved_toml = std::fs::read_to_string(fx.paths.settings_file()).unwrap();
    assert!(saved_toml.contains("auto_position = false"), "gespeichert: {saved_toml}");
    core.dispatch(apply(true, 2));
    wait_for(&core, "wieder an", |v| v.document.as_ref().is_some_and(|d| d.placement_locked)).await;
    assert_eq!(engine.prepares.load(Ordering::SeqCst), 1, "nur das ausdrücklich signierte Dokument");
}

/// D-06: `-b KS -ks … -a -s a.pdf b.pdf` signiert im Arbeitsbereich-Modus beide Dokumente automatisch und
/// endet (Signiermittel ohne `document_leaves_device`).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn start_arguments_with_keystore_sign_both_documents_and_finish() {
    let fx = Fixture::new();
    let input = fx.dir("in");
    let out = fx.dir("out");
    let a = copy_pdf(PDF, &input, "a.pdf");
    let b = copy_pdf(PDF, &input, "b.pdf");
    let mut overlay = keystore_overlay(vec![a, b], &out);
    overlay.backend = Some(pida_config::BackendChoice::Keystore);
    overlay.auto_position = true;
    overlay.skip_finish = true;
    let core = start(&fx, overlay, None, Some(FakeEngine::new()), false).unwrap();
    let code = tokio::time::timeout(Duration::from_secs(30), core.wait_finished()).await.unwrap();
    assert_eq!(code, ExitCode::Success, "{:#?}", core.view());
    assert!(out.join("a_signed.pdf").is_file() && out.join("b_signed.pdf").is_file());
}

/// `SelectSigner { make_default }` speichert den Standard (auch „Jedes Mal fragen“) und öffnet nie eine
/// Sitzung; die Kontoanzeige ist maskiert.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn signer_chip_selection_and_default() {
    let fx = Fixture::new();
    let a = copy_pdf(PDF, &fx.dir("in"), "a.pdf");
    fx.write_settings("[id_austria]\nusername = \"+436641234567\"\nremember = true\n");
    let fake = FakeAtrust::new();
    // wie am Desktop mit Chipkarte: mit nur einem Signiermittel gäbe es keine Auswahl (Nachtrag 20)
    let core = start(
        &fx,
        Overlay { input_files: vec![a], ..Overlay::default() },
        Some(vec![fake.backend(), IdleBackend::card()]),
        Some(FakeEngine::new()),
        false,
    )
    .unwrap();
    let v = wait_for(&core, "Arbeitsbereich", editing).await;
    assert!(v.signers.choice_offered);
    assert_eq!((v.signers.selected, v.signers.default), (None, None), "„Jedes Mal fragen“");
    let atrust = v.signers.options.iter().find(|s| s.backend == BackendId::AtrustNative).unwrap();
    assert_eq!(atrust.account.as_deref(), Some("+43 664 ···567"));
    assert!(atrust.capabilities.as_ref().is_some_and(|c| c.document_leaves_device));
    core.dispatch(UiEvent::SelectSigner { backend: Some(BackendId::AtrustNative), make_default: true });
    let v = wait_for(&core, "Standard", |v| v.signers.default == Some(BackendId::AtrustNative)).await;
    assert_eq!(v.signers.selected, Some(BackendId::AtrustNative));
    let saved = std::fs::read_to_string(fx.paths.settings_file()).unwrap();
    assert!(saved.contains("id_austria"), "{saved}");
    core.dispatch(UiEvent::SelectSigner { backend: None, make_default: true });
    wait_for(&core, "Jedes Mal fragen", |v| v.signers.default.is_none() && v.signers.selected.is_none()).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(fake.opened.load(Ordering::SeqCst), 0, "SelectSigner öffnet nie eine Sitzung");
}

/// Kern am Desktop mit Richtlinie und Dokument im Arbeitsbereich (Nachtrag 20).
fn start_with_policy(
    fx: &Fixture,
    backends: Vec<Arc<dyn SignatureBackend>>,
    policy: Option<pida_config::Policy>,
) -> pida_core_api::Core {
    let a = copy_pdf(PDF, &fx.dir("in"), "a.pdf");
    let opts = CoreOptions {
        workspace_ui: Some(true),
        backends: Some(backends),
        engine: Some(FakeEngine::new()),
        policy,
        ..CoreOptions::isolated()
    };
    let overlay = Overlay { input_files: vec![a], ..Overlay::default() };
    pida_core_api::Core::new_with(fx.config(overlay, false), opts).expect("Kern startet")
}

/// Keine Signiermittel-Auswahl bei nur einem Signiermittel am Desktop (ADR 0004 Nachtrag 20): Eine Richtlinie,
/// die nur ID Austria freigibt (`allowed_backends`) bzw. vorschreibt (`forced_backend`), lässt nichts zu wählen –
/// `choice_offered = false`, `selected` = ID Austria auch bei „Jedes Mal fragen“ und nach `select_signer` ohne
/// bzw. mit einem anderen Signiermittel; die Chipkarte bleibt in den Optionen (gesperrt mit Grund). Signiert wird
/// erst nach „Signieren mit“ (A1). Gegenprobe ohne Richtlinie: Auswahl wie bisher, „Jedes Mal fragen“ ohne Vorauswahl.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn single_signer_by_policy_offers_no_choice() {
    use pida_config::{BackendChoice, Policy};
    let only_ida = Policy { allowed_backends: Some(vec![BackendChoice::IdAustria]), ..Policy::default() };
    let forced = Policy { forced_backend: Some(BackendChoice::IdAustria), ..Policy::default() };
    for (name, policy) in [("allowed_backends", only_ida), ("forced_backend", forced)] {
        let fx = Fixture::new();
        fx.write_settings("[general]\n"); // „Jedes Mal fragen“ (kein `backend.default`), ohne Ersteinrichtung
        let fake = FakeAtrust::new();
        let core = start_with_policy(&fx, vec![fake.backend(), IdleBackend::card()], Some(policy));
        let v = wait_for(&core, "Arbeitsbereich", editing).await;
        assert!(!v.signers.choice_offered, "{name}");
        assert_eq!(v.signers.selected, Some(BackendId::AtrustNative), "{name}: das eine Signiermittel ist gewählt");
        assert_eq!(v.signers.options.len(), 2, "{name}: Optionen unverändert");
        for signer in [None, Some(BackendId::LocalBku)] {
            core.dispatch(UiEvent::SelectSigner { backend: signer, make_default: false });
            tokio::time::sleep(Duration::from_millis(100)).await;
            let v = core.view();
            assert_eq!(v.signers.selected, Some(BackendId::AtrustNative), "{name}: {signer:?}");
            assert!(!v.signers.choice_offered && editing(&v));
        }
        assert_eq!(fake.opened.load(Ordering::SeqCst), 0, "{name}: keine Sitzung ohne „Signieren“ (A1)");
        core.dispatch(UiEvent::SignWith { backend: BackendId::AtrustNative });
        form_request(&core, "Anmeldung", |f| matches!(f, NativeForm::Login { .. })).await;
        assert_eq!(fake.opened.load(Ordering::SeqCst), 1, "{name}");
    }

    // Gegenprobe: ohne Richtlinie zwei Signiermittel – Auswahl, „Jedes Mal fragen“ ohne Vorauswahl
    let fx = Fixture::new();
    fx.write_settings("[general]\n");
    let core = start_with_policy(&fx, vec![FakeAtrust::new().backend(), IdleBackend::card()], None);
    let v = wait_for(&core, "Arbeitsbereich", editing).await;
    assert!(v.signers.choice_offered);
    assert_eq!(v.signers.selected, None);
}

/// Das eine Signiermittel ist nicht verfügbar (Richtlinie gibt nur die Chipkarte frei, die Chipkarten-Software läuft
/// nicht, Nachtrag 20): keine Auswahl, die Chipkarte ist gewählt; „Signieren mit“ nennt den Grund
/// (`ng-signer-localbku-not-running`) statt „Bitte wählen Sie ein anderes“ und startet nichts. Mit Auswahl
/// (ohne Richtlinie) bleibt der allgemeine Hinweis des Workflows.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn single_unavailable_signer_names_the_reason() {
    use pida_config::{BackendChoice, Policy};
    let fx = Fixture::new();
    fx.write_settings("[general]\n");
    let fake = FakeAtrust::new();
    let only_card = Policy { allowed_backends: Some(vec![BackendChoice::LocalBku]), ..Policy::default() };
    let core = start_with_policy(&fx, vec![fake.backend(), IdleBackend::card_not_running()], Some(only_card));
    let v = wait_for(&core, "Arbeitsbereich", editing).await;
    assert!(!v.signers.choice_offered);
    assert_eq!(v.signers.selected, Some(BackendId::LocalBku));
    let card = v.signers.options.iter().find(|o| o.backend == BackendId::LocalBku).unwrap();
    assert_eq!(card.unavailable_reason.as_deref(), Some("ng-signer-localbku-not-running"));
    core.dispatch(UiEvent::SignWith { backend: BackendId::LocalBku });
    let v = wait_for(&core, "Hinweis", |v| v.notices.iter().any(|n| n.code == "ng-signer-localbku-not-running")).await;
    assert!(editing(&v), "{:?}", v.step);
    assert!(!v.notices.iter().any(|n| n.code == "ng-signer-unavailable"));
    assert_eq!(fake.opened.load(Ordering::SeqCst), 0);

    // Gegenprobe mit Auswahl: der allgemeine Hinweis („Bitte wählen Sie ein anderes“)
    let fx = Fixture::new();
    fx.write_settings("[general]\n");
    let core = start_with_policy(&fx, vec![FakeAtrust::new().backend(), IdleBackend::card_not_running()], None);
    let v = wait_for(&core, "Arbeitsbereich", editing).await;
    assert!(v.signers.choice_offered);
    core.dispatch(UiEvent::SignWith { backend: BackendId::LocalBku });
    let v = wait_for(&core, "Hinweis", |v| v.notices.iter().any(|n| n.code == "ng-signer-unavailable")).await;
    assert!(editing(&v));
}

/// Kern am Desktop mit Richtlinie und Stapelübersicht (zwei Dokumente, Ausgabeordner fest).
fn start_batch_with_policy(
    fx: &Fixture,
    backends: Vec<Arc<dyn SignatureBackend>>,
    policy: Option<pida_config::Policy>,
) -> pida_core_api::Core {
    let dir = fx.dir("in");
    let inputs: Vec<PathBuf> = ["a.pdf", "b.pdf"]
        .iter()
        .map(|n| {
            let p = copy_pdf(PDF, &dir, n);
            std::fs::write(&p, [std::fs::read(&p).unwrap().as_slice(), format!("\n%{n}\n").as_bytes()].concat())
                .unwrap();
            p
        })
        .collect();
    let opts = CoreOptions {
        workspace_ui: Some(true),
        batch_overview: Some(true),
        backends: Some(backends),
        engine: Some(FakeEngine::new()),
        policy,
        ..CoreOptions::isolated()
    };
    let overlay = Overlay { input_files: inputs, output: Some(fx.dir("out")), ..Overlay::default() };
    pida_core_api::Core::new_with(fx.config(overlay, false), opts).expect("Kern startet")
}

/// Stapelübersicht ohne Signiermittel-Auswahl (ADR 0004 Nachtrag 20): Ist das eine Signiermittel nicht verfügbar
/// (Richtlinie gibt nur die Chipkarte frei, die Chipkarten-Software läuft nicht), nennt „Alle signieren“ wie „Signieren
/// mit“ den Grund statt „Bitte wählen Sie ein anderes“; der Lauf beginnt nicht.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn single_unavailable_signer_in_the_batch_names_the_reason() {
    use pida_config::{BackendChoice, Policy};
    let fx = Fixture::new();
    fx.write_settings("[general]\n");
    let fake = FakeAtrust::new();
    let only_card = Policy { allowed_backends: Some(vec![BackendChoice::LocalBku]), ..Policy::default() };
    let core = start_batch_with_policy(&fx, vec![fake.backend(), IdleBackend::card_not_running()], Some(only_card));
    let v = wait_for(&core, "Übersicht", |v| matches!(v.step, StepView::Batch) && v.actions.start_batch).await;
    assert!(!v.signers.choice_offered);
    assert_eq!(v.signers.selected, Some(BackendId::LocalBku));
    let docs = v.batch.as_ref().unwrap().items.iter().map(|i| i.doc).collect();
    core.dispatch(UiEvent::StartBatch {
        backend: BackendId::LocalBku,
        docs,
        output_dir: None,
        on_existing: pida_workflow::ExistingPolicy::KeepBoth,
    });
    let v = wait_for(&core, "Hinweis", |v| v.notices.iter().any(|n| n.code == "ng-signer-localbku-not-running")).await;
    assert!(!v.notices.iter().any(|n| n.code == "ng-signer-unavailable"), "{:?}", v.notices);
    assert!(matches!(v.step, StepView::Batch) && !v.batch.as_ref().unwrap().running);
    assert_eq!(fake.opened.load(Ordering::SeqCst), 0);
}

/// Keine Signiermittel-Auswahl und gar kein Signiermittel (die Richtlinie gibt nur den Keystore frei, den es hier nicht
/// gibt; Nachtrag 20): `selected = None`, und der Kern sperrt „Signieren“ (Arbeitsbereich, ⌘↵, Palette) bzw. „Alle
/// signieren“ selbst – die Oberfläche leitet das nicht aus den Signiermitteln ab. Gegenprobe ohne Richtlinie: frei.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_signer_at_all_locks_sign_and_start_batch() {
    use pida_config::{BackendChoice, Policy};
    let only_keystore = || Policy { allowed_backends: Some(vec![BackendChoice::Keystore]), ..Policy::default() };
    // Arbeitsbereich
    let fx = Fixture::new();
    fx.write_settings("[general]\n");
    let fake = FakeAtrust::new();
    let core = start_with_policy(&fx, vec![fake.backend(), IdleBackend::card()], Some(only_keystore()));
    let v = wait_for(&core, "Arbeitsbereich", |v| {
        matches!(v.step, StepView::Position { .. })
            && v.document.as_ref().is_some_and(|d| d.phase == DocumentPhase::Editing)
    })
    .await;
    assert!(!v.signers.choice_offered && v.signers.selected.is_none(), "{:?}", v.signers);
    assert!(v.signers.options.iter().all(|o| o.unavailable_reason.as_deref() == Some("ng-signer-policy")));
    assert!(!v.actions.sign, "nichts, womit signiert werden könnte");
    assert!(v.actions.edit_placement, "Platzierung bleibt bedienbar");
    // Stapelübersicht
    let fx = Fixture::new();
    fx.write_settings("[general]\n");
    let core = start_batch_with_policy(&fx, vec![fake.backend(), IdleBackend::card()], Some(only_keystore()));
    let v = wait_for(&core, "Übersicht voranalysiert", |v| {
        matches!(v.step, StepView::Batch)
            && v.batch.as_ref().is_some_and(|b| {
                b.items.iter().any(|i| {
                    matches!(
                        i.status,
                        pida_core_api::BatchItemStatusView::Ready | pida_core_api::BatchItemStatusView::Positioned
                    )
                })
            })
    })
    .await;
    assert!(!v.signers.choice_offered && v.signers.selected.is_none());
    assert!(!v.actions.start_batch, "nichts, womit der Stapel signiert werden könnte");
    assert_eq!(fake.opened.load(Ordering::SeqCst), 0);
    // Gegenprobe ohne Richtlinie
    let fx = Fixture::new();
    fx.write_settings("[general]\n");
    let core = start_batch_with_policy(&fx, vec![FakeAtrust::new().backend(), IdleBackend::card()], None);
    let v = wait_for(&core, "Übersicht", |v| matches!(v.step, StepView::Batch) && v.actions.start_batch).await;
    assert!(v.signers.choice_offered && v.signers.selected.is_none(), "„Jedes Mal fragen“: Menü statt Sperre");
}

/// Ohne PDFium (Abnahme E1 Punkt 10): Arbeitsbereich bedienbar, Hinweis, „Signieren“ signiert automatisch
/// platziert; ohne „Signieren mit“ keine Sitzung.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn without_pdfium_the_workspace_signs_automatically_placed() {
    let fx = Fixture::new();
    let out = fx.dir("out");
    let a = copy_pdf(PDF, &fx.dir("in"), "a.pdf");
    let engine = FakeEngine::new();
    let core = start(&fx, keystore_overlay(vec![a], &out), None, Some(engine.clone()), false).unwrap();
    let v = wait_for(&core, "Arbeitsbereich", editing).await;
    let d = v.document.as_ref().unwrap();
    assert!(d.sticker.is_none() && d.layout.is_empty());
    assert_eq!(d.check.overlaps_content, None, "ohne Renderer unbekannt");
    assert!(v.notices.iter().any(|n| n.code == "error-PositioningNotPossible"));
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(engine.prepares.load(Ordering::SeqCst), 0);
    core.dispatch(UiEvent::SignWith { backend: BackendId::Keystore });
    wait_for(&core, "gespeichert", saved).await;
    let plan = engine.last_plan();
    assert!(matches!(plan.visibility, pida_pades::Visibility::Visible(_)), "automatisch platziert, sichtbar");
}

// ------------------------------------------------------------------------------------------------
// Signierkarte: Abbruch, Wiederholen, Ablehnung (Abnahme E1 Punkt 6)
// ------------------------------------------------------------------------------------------------

/// Abbrechen in der Anmeldung → Arbeitsbereich mit unveränderter Platzierung und „Es wurde nichts signiert“.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancel_returns_to_the_workspace_unchanged() {
    let fx = Fixture::new();
    let a = copy_pdf(PDF, &fx.dir("in"), "a.pdf");
    let (backend, backends) = scripted(vec![]);
    let core =
        start(&fx, Overlay { input_files: vec![a], ..Overlay::default() }, backends, Some(FakeEngine::new()), false)
            .unwrap();
    let before = wait_for(&core, "Arbeitsbereich", editing).await.document.unwrap();
    core.dispatch(UiEvent::SetInvisible { invisible: true });
    let before_invisible = wait_for(&core, "unsichtbar", |v| v.document.as_ref().is_some_and(|d| d.invisible)).await;
    let before_invisible = before_invisible.document.unwrap();
    assert!(before_invisible.revision > before.revision, "Revision steigt mit jeder Änderung");
    core.dispatch(UiEvent::SignWith { backend: BackendId::AtrustNative });
    // unsichtbar: kein Zertifikat nötig – Anmeldung im Signieren
    form_request(&core, "Anmeldung", |f| matches!(f, NativeForm::Login { .. })).await;
    let v = core.view();
    let StepView::Signing { progress, .. } = &v.step else { panic!() };
    assert_eq!(progress.certificate, CertificateSource::NotNeeded);
    core.dispatch(UiEvent::Cancel);
    let v = wait_for(&core, "zurück im Arbeitsbereich", |v| {
        editing(v) && v.notices.iter().any(|n| n.code == "ng-signing-cancelled")
    })
    .await;
    let after = v.document.unwrap();
    assert_eq!(
        (after.placement, after.target, after.origin, after.invisible),
        (before_invisible.placement, before_invisible.target, before_invisible.origin, true)
    );
    assert!(!matches!(v.step, StepView::Error { .. }), "eigener Abbruch ist kein Fehler");
    // `CancelSession` räumt die Sitzung asynchron auf (Passwort gelöscht, F-72)
    for _ in 0..50 {
        if backend.cancelled.load(Ordering::SeqCst) >= 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(backend.cancelled.load(Ordering::SeqCst) >= 1, "Sitzung abgebrochen (F-72)");
}

/// Netzfehler **nach** erfolgreicher Anmeldung: Fehlerschritt in der Karte (Herkunft, Zeitpunkt); „Erneut
/// versuchen“ in derselben Sitzung – ohne zweite Login-Maske und ohne zweites `certificate()`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn retry_after_network_error_reuses_the_session() {
    let fx = Fixture::new();
    let out = fx.dir("out");
    let a = copy_pdf(PDF, &fx.dir("in"), "a.pdf");
    let (backend, backends) = scripted(vec![BackendError::Network("Verbindung getrennt".into())]);
    let overlay = Overlay { input_files: vec![a], output: Some(out.clone()), ..Overlay::default() };
    let core = start(&fx, overlay, backends, Some(FakeEngine::new()), false).unwrap();
    wait_for(&core, "Arbeitsbereich", editing).await;
    core.dispatch(UiEvent::SignWith { backend: BackendId::AtrustNative });
    login(&core).await;
    let v = wait_for(&core, "Fehler", |v| matches!(v.step, StepView::Error { .. })).await;
    let StepView::Error { code, retry, context, .. } = &v.step else { unreachable!() };
    assert_eq!((code.as_str(), *retry), ("backend-network", true));
    assert_eq!(context.origin, ErrorOrigin::Signing);
    assert_eq!(context.backend, Some(BackendId::AtrustNative));
    assert!(context.occurred_at.is_some() && context.doc.is_some());
    assert_eq!(v.document.as_ref().map(|d| d.phase), Some(DocumentPhase::Signing), "Karte über dem Dokument");
    core.dispatch(UiEvent::Retry);
    let v = wait_for(&core, "TAN", |v| {
        matches!(
            &v.step,
            StepView::Signing { interaction: InteractionView::Form { form: NativeForm::SmsTan { .. }, .. }, .. }
        )
    })
    .await;
    let StepView::Signing { progress, .. } = &v.step else { unreachable!() };
    assert!(progress.login_done, "Anmeldung der Sitzung gilt weiter");
    assert_eq!(progress.certificate, CertificateSource::Session);
    assert_eq!(progress.stage, SigningStage::Confirming);
    tan(&core).await;
    wait_for(&core, "gespeichert", saved).await;
    assert_eq!(backend.opened.load(Ordering::SeqCst), 1, "dieselbe Sitzung");
    assert_eq!(backend.logins.load(Ordering::SeqCst), 1, "keine zweite Login-Maske");
    assert_eq!(backend.certificates.load(Ordering::SeqCst), 1, "kein zweites InfoboxRead");
    assert_eq!(backend.signs.load(Ordering::SeqCst), 2);
}

/// `Cancelled`, das PIDA nicht selbst ausgelöst hat (SL 6000/6001): Fehlerschritt `ng-signing-declined`;
/// „Erneut versuchen“ fordert in derselben Sitzung eine neue Bestätigung an; „Schließen“ verwirft sie.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn declined_in_the_app_offers_retry_in_the_same_session() {
    let fx = Fixture::new();
    let out = fx.dir("out");
    let a = copy_pdf(PDF, &fx.dir("in"), "a.pdf");
    let (backend, backends) = scripted(vec![BackendError::Cancelled, BackendError::Cancelled]);
    let overlay = Overlay { input_files: vec![a], output: Some(out), ..Overlay::default() };
    let core = start(&fx, overlay, backends, Some(FakeEngine::new()), false).unwrap();
    wait_for(&core, "Arbeitsbereich", editing).await;
    core.dispatch(UiEvent::SignWith { backend: BackendId::AtrustNative });
    login(&core).await;
    let v = wait_for(
        &core,
        "abgelehnt",
        |v| matches!(&v.step, StepView::Error { code, .. } if code == "ng-signing-declined"),
    )
    .await;
    let StepView::Error { retry, context, .. } = &v.step else { unreachable!() };
    assert!(*retry);
    assert_eq!(context.origin, ErrorOrigin::Signing);
    assert_eq!(backend.cancelled.load(Ordering::SeqCst), 0, "Sitzung (samt Zugangsdaten) bleibt bis zur Entscheidung");
    // erneut abgelehnt, dann „Schließen“ → Arbeitsbereich, Sitzung verworfen
    core.dispatch(UiEvent::Retry);
    wait_for(&core, "wieder abgelehnt", |v| {
        matches!(&v.step, StepView::Error { code, .. } if code == "ng-signing-declined")
            && backend.signs.load(Ordering::SeqCst) == 2
    })
    .await;
    assert_eq!(backend.logins.load(Ordering::SeqCst), 1, "keine neue Anmeldung");
    core.dispatch(UiEvent::Cancel);
    wait_for(&core, "Arbeitsbereich", editing).await;
    // `CancelSession` räumt die Sitzung asynchron auf (wie oben): unter Last stand der Arbeitsbereich schon,
    // bevor das Backend den Abbruch zählte (Integration mob2/int, 1 von 5 Läufen)
    for _ in 0..50 {
        if backend.cancelled.load(Ordering::SeqCst) >= 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(backend.cancelled.load(Ordering::SeqCst), 1, "Schließen verwirft die Sitzung (F-72)");
    // neuer Versuch: neue Sitzung, neue Anmeldung, Erfolg
    core.dispatch(UiEvent::SignWith { backend: BackendId::AtrustNative });
    login(&core).await;
    tan(&core).await;
    wait_for(&core, "gespeichert", saved).await;
    assert_eq!(backend.opened.load(Ordering::SeqCst), 2);
}

/// „Abbrechen“ in einer Meldung während des Signierens: nur eine A-Trust-Fehlerseite gilt als Ablehnung bzw.
/// Zeitablauf (Fehlerschritt `ng-signing-declined`); jede andere Meldung (HTTP, Proxy, Keystore …) ist ein
/// eigener Abbruch – zurück in den Arbeitsbereich mit „es wurde nichts signiert“ (ADR 0004 §3.6).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancel_in_a_notice_counts_as_declined_only_for_atrust_error_pages() {
    for (code, declined) in [(NoticeCode::AtrustError, true), (NoticeCode::Http { status: 502 }, false)] {
        let fx = Fixture::new();
        let a = copy_pdf(PDF, &fx.dir("in"), "a.pdf");
        let (backend, backends) = scripted(vec![]);
        *backend.notice.lock().unwrap() = Some(code.clone());
        let overlay = Overlay { input_files: vec![a], ..Overlay::default() };
        let core = start(&fx, overlay, backends, Some(FakeEngine::new()), false).unwrap();
        wait_for(&core, "Arbeitsbereich", editing).await;
        core.dispatch(UiEvent::SignWith { backend: BackendId::AtrustNative });
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
            wait_for(
                &core,
                "abgelehnt",
                |v| matches!(&v.step, StepView::Error { code, .. } if code == "ng-signing-declined"),
            )
            .await;
        } else {
            let v = wait_for(&core, "abgebrochen", |v| {
                editing(v) && v.notices.iter().any(|n| n.code == "ng-signing-cancelled")
            })
            .await;
            assert!(!matches!(v.step, StepView::Error { .. }), "{code:?}: eigener Abbruch ist keine Ablehnung");
        }
    }
}

// ------------------------------------------------------------------------------------------------
// Ziele, Einschränkungen, vorhandene Signaturen (Abnahme E1 Punkt 4)
// ------------------------------------------------------------------------------------------------

/// Leere Signaturfelder werden ohne Platzhaltersuche angeboten (nie automatisch); „Ins Feld setzen“ setzt
/// den Sticker ins Feld, „Signieren“ signiert ins Feld; „Frei platzieren“ kehrt zurück.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn empty_fields_are_offered_and_used_only_on_request() {
    let fx = Fixture::new();
    let out = fx.dir("out");
    let a = copy_pdf("pdf/pdfas/pdf-as-pdfbox-2/manySignFields.pdf", &fx.dir("in"), "fields.pdf");
    let engine = FakeEngine::new();
    let core = start(&fx, keystore_overlay(vec![a], &out), None, Some(engine.clone()), false).unwrap();
    let v = wait_for(&core, "Arbeitsbereich", editing).await;
    let d = v.document.unwrap();
    assert!(!d.fields.is_empty(), "Felder auch ohne placeholder.search");
    assert_eq!(d.target, PlacementTarget::Free, "nie automatisch ins Feld (§8 F2)");
    let f0 = d.fields[0].clone();
    assert!(f0.rect.is_some() && f0.selectable && f0.page.is_some());
    core.dispatch(UiEvent::UseTarget { target: PlacementTarget::Field { index: 0 } });
    let v = wait_for(&core, "im Feld", |v| {
        v.document.as_ref().is_some_and(|d| d.target == PlacementTarget::Field { index: 0 })
    })
    .await;
    let d = v.document.unwrap();
    assert_eq!(d.origin, PlacementOrigin::Field);
    // Nachtrag 16: der Block steht proportional und mittig im Feld (nicht auf das Feld gestreckt)
    // gegen dieselbe Sicht vergleichen: die Blocklage im Feld kann sich bis hierher noch ändern (Blockmaß kommt nach)
    assert_eq!(d.sticker, d.fields[0].block, "Sticker = Lage des Blocks im Feld");
    assert_eq!(d.check.overlaps_content, Some(false), "das Zielfeld ist kein Inhalt");
    assert_eq!(Some(d.placement.page_index), f0.page);
    core.dispatch(UiEvent::SignWith { backend: BackendId::Keystore });
    wait_for(&core, "gespeichert", saved).await;
    let plan = engine.last_plan();
    assert!(
        matches!(&plan.field, pida_pades::FieldTarget::Existing { qualified_name, .. } if *qualified_name == f0.name)
    );
}

/// Die Automatik des Arbeitsbereichs überdeckt keine vorhandenen Felder (Mobile-Check 16.10.2026, ADR 0004
/// Nachtrag 12): `manySignFields-xref-stream.pdf` hat auf der letzten Seite ein seitenfüllendes leeres
/// Signaturfeld (`Signature_7`). Vorher meldete die Sprechblase „automatisch auf freier Fläche platziert“ mitten im
/// Feld, zugleich warnte das Sheet „überdeckt ein Feld“, und „Freie Fläche suchen“ landete wieder dort. Jetzt: neue
/// Seite ohne Überdeckung, „Freie Fläche suchen“ ebenso, und signiert wird genau so (Vorschau = Ergebnis). Die reine
/// PDF-AS-Regel (CLI, klassischer Ablauf) bleibt unverändert im Feld. PDFium nötig.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn automatic_placement_stays_out_of_existing_fields() {
    let fx = Fixture::new();
    let a = copy_pdf("pdf/generated/manySignFields-xref-stream.pdf", &fx.dir("in"), "fields.pdf");
    let engine = FakeEngine::new();
    let Some(core) = start(&fx, keystore_overlay(vec![a.clone()], &fx.dir("out")), None, Some(engine.clone()), true)
    else {
        eprintln!("PIDA_PDFIUM_DIR nicht gesetzt – übersprungen");
        return;
    };
    let pdfas = auto_position_with(&a, false);
    assert!(!pdfas.new_page, "PDF-AS-Regel: im seitenfüllenden Feld der letzten Seite");
    let placed = |v: &ViewState| {
        v.document.as_ref().is_some_and(|d| {
            editing(v) && d.new_page && d.placement.page_index == d.doc.page_count && d.check.for_revision == d.revision
        })
    };
    let v = wait_for(&core, "automatisch auf neuer Seite", placed).await;
    let d = v.document.unwrap();
    assert_eq!((d.origin, d.target), (PlacementOrigin::Auto, PlacementTarget::Free));
    assert!(d.detection.as_ref().is_some_and(|x| x.auto_placed), "Sprechblase „automatisch platziert“");
    assert!(!d.check.overlaps_widget, "kein Feld überdeckt");
    assert_eq!(d.check.overlaps_content, Some(false), "die neue Seite ist leer");
    assert!(expected_auto_position(&a).new_page);
    // „Freie Fläche suchen“: dasselbe Ergebnis
    core.dispatch(UiEvent::AutoPlace);
    tokio::time::sleep(Duration::from_millis(400)).await;
    let v = wait_for(&core, "wieder auf neuer Seite", placed).await;
    assert!(!v.document.unwrap().check.overlaps_widget);
    core.dispatch(UiEvent::SignWith { backend: BackendId::Keystore });
    wait_for(&core, "gespeichert", saved).await;
    match engine.last_plan().visibility {
        pida_pades::Visibility::Visible(p) => assert!(p.new_page, "signiert wie gezeigt: neue Seite"),
        other => panic!("sichtbar erwartet: {other:?}"),
    }
}

/// Nachtrag 12, Duplikate (mit Nachtrag 17 zusammengeführt): Vor dem Speichern ist das Dokument offen – derselbe
/// Pfad kommt nicht noch einmal dazu (Hinweis `ng-open-duplicates`). Signiert und gespeichert („Fertig“) öffnet eine
/// Kopie derselben Datei wie vor Nachtrag 12 als nächstes Dokument (F-89 „sofort weiter“), ohne „ist bereits
/// geöffnet“. Desktop vergleicht Pfade; die Kopie auf Mobilgeräten prüft `mobile.rs`
/// (`shared_copy_opens_again_after_signing_on_mobile`).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_same_file_opens_again_after_it_was_signed() {
    let fx = Fixture::new();
    let a = copy_pdf(TEXT_PDF, &fx.dir("in"), "doc.pdf");
    let again = copy_pdf(TEXT_PDF, &fx.dir("inbox/2"), "doc.pdf");
    let core = start(&fx, keystore_overlay(vec![a.clone()], &fx.dir("out")), None, None, false).expect("Kern");
    let duplicate = |v: &ViewState| v.notices.iter().any(|n| n.code == "ng-open-duplicates");
    wait_for(&core, "Arbeitsbereich", editing).await;
    // offen: dieselbe Datei kommt nicht dazu
    core.dispatch(UiEvent::FilesReceived { paths: vec![a.clone()] });
    let v = wait_for(&core, "Hinweis", duplicate).await;
    assert!(editing(&v) && !v.actions.show_batch, "kein zweites Dokument");
    for n in v.notices.iter().filter(|n| n.code == "ng-open-duplicates") {
        core.dispatch(UiEvent::DismissNotice { id: n.id });
    }
    core.dispatch(UiEvent::SignWith { backend: BackendId::Keystore });
    let v = wait_for(&core, "gespeichert", saved).await;
    let first = v.document.as_ref().map(|d| (d.doc.id, d.phase));
    assert!(matches!(first, Some((_, DocumentPhase::Done))), "{first:?}");
    // signiert und gespeichert: eine Kopie derselben Datei öffnet als nächstes Dokument
    core.dispatch(UiEvent::FilesReceived { paths: vec![again] });
    let v = wait_for(&core, "erneut geöffnet", |v| {
        editing(v) && v.document.as_ref().is_some_and(|d| Some(d.doc.id) != first.map(|f| f.0))
    })
    .await;
    assert_eq!(v.document.as_ref().map(|d| d.doc.name.as_str()), Some("doc.pdf"));
    assert!(!duplicate(&v), "nicht „ist bereits geöffnet“");
}

// ------------------------------------------------------------------------------------------------
// Signaturfelder: Plausibilität, Block im Feld = geschriebene Signatur, neue Seite (ADR 0004 Nachtrag 16)
// ------------------------------------------------------------------------------------------------

fn pt_rect(r: pida_core_api::RectPt) -> pida_pdf_cos::Rect {
    pida_pdf_cos::Rect::new(r.left, r.bottom, r.right, r.top)
}

fn assert_rect_close(a: pida_pdf_cos::Rect, b: pida_pdf_cos::Rect, tol: f32, what: &str) {
    let ok = [(a.llx, b.llx), (a.lly, b.lly), (a.urx, b.urx), (a.ury, b.ury)].iter().all(|(x, y)| (x - y).abs() <= tol);
    assert!(ok, "{what}: {a:?} ≠ {b:?} (± {tol})");
}

/// Begrenzungsrechteck von `r` unter der Matrix `m`.
fn transform(m: [f32; 6], r: pida_pdf_cos::Rect) -> pida_pdf_cos::Rect {
    let pts = [(r.llx, r.lly), (r.urx, r.lly), (r.llx, r.ury), (r.urx, r.ury)]
        .map(|(x, y)| (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]));
    let xs = pts.map(|p| p.0);
    let ys = pts.map(|p| p.1);
    pida_pdf_cos::Rect::new(
        xs.iter().copied().fold(f32::INFINITY, f32::min),
        ys.iter().copied().fold(f32::INFINITY, f32::min),
        xs.iter().copied().fold(f32::NEG_INFINITY, f32::max),
        ys.iter().copied().fold(f32::NEG_INFINITY, f32::max),
    )
}

/// Wohin ein Viewer das Inner-Form zeichnet (ISO 32000-1 §12.5.5: transformierte Holder-BBox → `/Rect`).
fn viewer_block(
    holder_bbox: pida_pdf_cos::Rect,
    matrix: [f32; 6],
    inner_bbox: pida_pdf_cos::Rect,
    rect: pida_pdf_cos::Rect,
) -> pida_pdf_cos::Rect {
    let tb = transform(matrix, holder_bbox);
    let t = transform(matrix, inner_bbox);
    let r = rect.normalized();
    let (sx, sy) = (r.width() / tb.width(), r.height() / tb.height());
    pida_pdf_cos::Rect::new(
        r.llx + (t.llx - tb.llx) * sx,
        r.lly + (t.lly - tb.lly) * sy,
        r.llx + (t.urx - tb.llx) * sx,
        r.lly + (t.ury - tb.lly) * sy,
    )
}

fn floats(o: &pida_pdf_cos::lopdf::Object) -> Vec<f32> {
    o.as_array().map(|a| a.iter().filter_map(|v| v.as_float().ok()).collect()).unwrap_or_default()
}

fn rect_of(o: &pida_pdf_cos::lopdf::Object) -> pida_pdf_cos::Rect {
    let v = floats(o);
    pida_pdf_cos::Rect::new(v[0], v[1], v[2], v[3]).normalized()
}

/// Lage des Blocks im signierten Feld `name`, wie ein Viewer sie aus `/Rect`, Holder-`/BBox`/`/Matrix` und der
/// BBox des Inner-Forms zeichnet (Benutzerraum).
fn written_block(pdf: &pida_pdf_cos::SourcePdf, name: &str) -> (usize, pida_pdf_cos::Rect) {
    use pida_pdf_cos::lopdf::Object;
    let field = pdf.signature_fields().unwrap().into_iter().find(|f| f.qualified_name == name).expect("Feld");
    assert!(field.signed, "{name} signiert");
    let w = &field.widgets[0];
    let wd = pdf.get_object(w.id).unwrap().as_dict().unwrap();
    let deref = |o: &Object| -> Object {
        match o {
            Object::Reference(id) => pdf.get_object(*id).unwrap().clone(),
            other => other.clone(),
        }
    };
    let ap = deref(wd.get(b"AP").unwrap());
    let holder = deref(ap.as_dict().unwrap().get(b"N").unwrap());
    let holder = holder.as_stream().unwrap();
    let matrix = holder.dict.get(b"Matrix").map(floats).unwrap_or_else(|_| vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    let matrix: [f32; 6] = matrix.try_into().unwrap();
    let res = deref(holder.dict.get(b"Resources").unwrap());
    let xobj = deref(res.as_dict().unwrap().get(b"XObject").unwrap());
    let inner = deref(xobj.as_dict().unwrap().get(b"FRM1").unwrap());
    let inner_bbox = rect_of(inner.as_stream().unwrap().dict.get(b"BBox").unwrap());
    let holder_bbox = rect_of(holder.dict.get(b"BBox").unwrap());
    (w.page_index.unwrap(), viewer_block(holder_bbox, matrix, inner_bbox, w.rect))
}

/// Begrenzungsrechteck der Bildpunkte, die sich zwischen zwei Renderings derselben Seite unterscheiden
/// (`PAGE_ABS` in pt bei `scale` px/pt; `None` = gleich).
fn changed_region(
    a: &pida_render::RgbaImage,
    b: &pida_render::RgbaImage,
    scale: f32,
    page_h: f32,
) -> Option<pida_pdf_cos::Rect> {
    assert_eq!((a.width, a.height), (b.width, b.height));
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for y in 0..a.height {
        for x in 0..a.width {
            let i = ((y * a.width + x) * 4) as usize;
            let d = (0..3).map(|c| (i32::from(a.data[i + c]) - i32::from(b.data[i + c])).abs()).max().unwrap_or(0);
            if d > 24 {
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
            }
        }
    }
    (x0 <= x1 && y0 <= y1 && x0 != u32::MAX).then(|| {
        pida_pdf_cos::Rect::new(
            x0 as f32 / scale,
            page_h - y1 as f32 / scale,
            x1 as f32 / scale,
            page_h - y0 as f32 / scale,
        )
    })
}

/// Das Dokument der Meldung (`manySignFields.pdf`, identisch mit den PDF-AS-Testdaten): angeboten werden nur
/// plausible Felder – nicht das seitengroße `Signature_7`, nicht die 3 pt schmalen bzw. winzigen; in jedem Feld
/// steht der Block proportional, höchstens in natürlicher Größe und mittig, und genau dort schreibt ihn der Plan.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_plausible_fields_are_offered_and_the_block_keeps_its_size() {
    let fx = Fixture::new();
    let a = copy_pdf("pdf/pdfas/pdf-as-pdfbox-2/manySignFields.pdf", &fx.dir("in"), "fields.pdf");
    let engine = FakeEngine::new();
    let core =
        start(&fx, keystore_overlay(vec![a.clone()], &fx.dir("out")), None, Some(engine.clone()), false).unwrap();
    let v = wait_for(&core, "Arbeitsbereich mit Blockgröße", |v| {
        editing(v) && v.document.as_ref().is_some_and(|d| d.block.preview_revision > 0)
    })
    .await;
    let d = v.document.unwrap();
    let block = pida_placement::BlockGeometry { width: d.block.width, height: d.block.height };
    let by_name = |n: &str| d.fields.iter().find(|f| f.name == n).cloned().expect(n);
    assert_eq!(d.fields.len(), 8);
    for f in &d.fields {
        let rect = pt_rect(f.rect.expect("Rechteck"));
        let fit = pida_placement::field_fit(rect, block);
        let page = d.doc.page_sizes[f.page.expect("Seite")];
        assert_eq!(f.suitable, pida_placement::field_suitable(rect, block, page), "{}", f.name);
        let b = pt_rect(f.block.expect("Lage im Feld"));
        assert_rect_close(b, fit.rect, 1e-3, &f.name);
        assert!(b.width() <= block.width + 1e-3 && b.height() <= block.height + 1e-3, "{}: nie größer", f.name);
        assert!(((b.width() / b.height()) - (block.width / block.height)).abs() < 1e-3, "{}: proportional", f.name);
    }
    for n in ["Signature_0", "Signature_5", "Signature_6"] {
        assert!(by_name(n).suitable, "{n}: übliches Feld (148 × 72, 171 × 150, 59 × 50 pt)");
    }
    for n in ["Signature_1", "Signature_2", "Signature_3", "Signature_4", "Signature_7"] {
        assert!(!by_name(n).suitable, "{n} ist kein plausibles Ziel");
    }
    // ausdrücklich ins seitengroße Feld (nicht angeboten, aber über den Vertrag erreichbar): natürliche Größe,
    // mittig auf der Seite – nicht die ganze Seite
    let f7 = by_name("Signature_7");
    core.dispatch(UiEvent::UseTarget { target: PlacementTarget::Field { index: f7.index } });
    let v = wait_for(&core, "im seitengroßen Feld", |v| {
        v.document.as_ref().is_some_and(|d| d.target == PlacementTarget::Field { index: f7.index })
    })
    .await;
    let d7 = v.document.unwrap();
    assert_eq!(d7.sticker, f7.block);
    let s = pt_rect(d7.sticker.unwrap());
    assert!((s.width() - block.width).abs() < 1e-3 && (s.height() - block.height).abs() < 1e-3, "Blockgröße: {s:?}");
    assert_eq!((d7.placement.page_index, d7.placement.width, d7.placement.height), (1, s.width(), s.height()));
    // ins übliche Feld und signieren: der Plan schreibt den Block genau dorthin, wo ihn die Ansicht zeigt
    let f0 = by_name("Signature_0");
    core.dispatch(UiEvent::UseTarget { target: PlacementTarget::Field { index: f0.index } });
    wait_for(&core, "im Feld", |v| v.document.as_ref().is_some_and(|d| d.sticker == f0.block)).await;
    core.dispatch(UiEvent::SignWith { backend: BackendId::Keystore });
    wait_for(&core, "gespeichert", saved).await;
    let plan = engine.last_plan();
    let pida_pades::Visibility::Visible(bp) = &plan.visibility else { panic!("sichtbar") };
    let app = &bp.appearance;
    let drawn = viewer_block(
        app.holder_bbox().unwrap(),
        app.holder_matrix(),
        pida_pdf_cos::Rect::new(0.0, 0.0, app.width + 1.0, app.height + 1.0),
        bp.rect,
    );
    assert_rect_close(drawn, pt_rect(f0.block.unwrap()), 0.01, "Plan = Vorschau");
    assert_rect_close(bp.rect.normalized(), pt_rect(f0.rect.unwrap()), 1e-3, "/Rect bleibt");

    // alle Felder: Plan (`build_plan`, derselbe Code wie beim Signieren) = `FieldTargetView::block`
    use pida_core_api::plan;
    let src = pida_pdf_cos::SourcePdf::open_file(&a, &pida_pdf_cos::Limits::default()).unwrap();
    let report = pida_pades::analyze(&src).unwrap();
    let store = pida_sigblock::ProfileStore::builtin().unwrap();
    let fonts = pida_sigblock::FontProvider::builtin();
    let settings = pida_config::Settings::default();
    let overlay = Overlay::default();
    let spec = plan::block_spec(
        &store,
        plan::SpecInput {
            effective: pida_config::Effective::new(&settings, &overlay, None),
            system_language: pida_i18n::Lang::system(),
            pdfa: report.pdfa.as_ref(),
            cert: None,
            signing_time: plan::local_now(),
            emblem: None,
        },
    )
    .unwrap();
    for cand in pida_pades::empty_fields(&report) {
        let view = by_name(&cand.qualified_name);
        let built = plan::build_plan(plan::PlanInput {
            report: &report,
            pages: src.pages(),
            position: &pida_workflow::PositionChoice::Field(cand.clone()),
            spec: &spec,
            chain: None,
            reserve: pida_pades::Reserve::ModelB,
            cms_profile: Default::default(),
            renderer: None,
            // wie beim Signieren im Arbeitsbereich (Nachtrag 12); für ein Feld als Ziel ohne Bedeutung
            avoid_widgets: true,
            store: &store,
            fonts: &fonts,
        })
        .unwrap();
        let pida_pades::Visibility::Visible(bp) = &built.plan.visibility else { panic!("sichtbar") };
        let app = &bp.appearance;
        assert!(
            (app.width - block.width).abs() < 1e-3,
            "{}: Breite der Vorschau, nicht des Felds",
            cand.qualified_name
        );
        let drawn = viewer_block(
            app.holder_bbox().unwrap(),
            app.holder_matrix(),
            pida_pdf_cos::Rect::new(0.0, 0.0, app.width + 1.0, app.height + 1.0),
            bp.rect,
        );
        assert_rect_close(drawn, pt_rect(view.block.unwrap()), 0.01, &cand.qualified_name);
    }
}

/// Seitengroßes Feld `Signature_7` (Meldung: „befüllt auf einmal die ganze Seite“) mit echter Engine und
/// Keystore: geschrieben wird der Block in natürlicher Größe mittig auf Seite 2 – genau im Sticker; PDFium zeichnet
/// ihn dort (Unterschied der Renderings), pyHanko prüft die Signatur.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn page_sized_field_is_signed_exactly_where_the_preview_shows_the_block() {
    let fx = Fixture::new();
    let out = fx.dir("out");
    let a = copy_pdf("pdf/pdfas/pdf-as-pdfbox-2/manySignFields.pdf", &fx.dir("in"), "fields.pdf");
    let pdfium = pdfium_dir().is_some();
    let core = start(&fx, keystore_overlay(vec![a.clone()], &out), None, None, pdfium).expect("Kern");
    let v = wait_for(&core, "Arbeitsbereich", |v| {
        editing(v) && v.document.as_ref().is_some_and(|d| d.block.preview_revision > 0)
    })
    .await;
    let d = v.document.unwrap();
    let f7 = d.fields.iter().find(|f| f.name == "Signature_7").cloned().unwrap();
    assert!(!f7.suitable);
    if pdfium {
        // frei auf Seite 2 (automatisch platziert): das seitengroße leere Feld ist kein Ziel und damit auch kein
        // Hindernis – keine Überdeckungswarnung, die „Freie Fläche suchen“ nie beheben könnte
        let v = wait_for(&core, "frei auf Seite 2 geprüft", |v| {
            v.document.as_ref().is_some_and(|d| {
                d.target == PlacementTarget::Free && d.placement.page_index == 1 && d.check.for_revision == d.revision
            })
        })
        .await;
        assert!(!v.document.unwrap().check.overlaps_widget, "seitengroßes Feld überdeckt nicht");
        // über dem passenden leeren Feld `Signature_0` (Seite 1) warnt die Prüfung weiter
        let f0 = d.fields.iter().find(|f| f.name == "Signature_0").cloned().unwrap();
        let r0 = f0.rect.unwrap();
        core.dispatch(UiEvent::MoveBlock { page: 0, x: r0.left, y: r0.top });
        let v = wait_for(&core, "über Signature_0", |v| {
            v.document.as_ref().is_some_and(|d| d.placement.page_index == 0 && d.check.for_revision == d.revision)
        })
        .await;
        assert!(v.document.unwrap().check.overlaps_widget, "passendes leeres Feld bleibt ein Hindernis");
    }
    sign_into_field_like_the_preview(&core, &a, &out.join("fields_signed.pdf"), &f7).await;
}

/// Übliches, aber kleineres Feld `Signature_5` (171 × 150 pt, Block auf etwa drei Viertel verkleinert): angeboten,
/// geschrieben proportional und mittig genau im Sticker (echte Engine, Keystore, PDFium, pyHanko).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scaled_field_is_signed_exactly_where_the_preview_shows_the_block() {
    let fx = Fixture::new();
    let out = fx.dir("out");
    let a = copy_pdf("pdf/pdfas/pdf-as-pdfbox-2/manySignFields.pdf", &fx.dir("in"), "fields.pdf");
    let core = start(&fx, keystore_overlay(vec![a.clone()], &out), None, None, pdfium_dir().is_some()).expect("Kern");
    let v = wait_for(&core, "Arbeitsbereich", |v| {
        editing(v) && v.document.as_ref().is_some_and(|d| d.block.preview_revision > 0)
    })
    .await;
    let d = v.document.unwrap();
    let f5 = d.fields.iter().find(|f| f.name == "Signature_5").cloned().unwrap();
    assert!(f5.suitable, "angeboten");
    let b = pt_rect(f5.block.unwrap());
    let scale = b.width() / d.block.width;
    assert!(scale > 0.7 && scale < 0.8, "verkleinert: {scale}");
    assert!(((b.height() / d.block.height) - scale).abs() < 1e-3, "proportional");
    sign_into_field_like_the_preview(&core, &a, &out.join("fields_signed.pdf"), &f5).await;
}

/// Platzhalterfeld der PDF-AS-Testdaten (`placeholder_sigfield_and_qr.pdf`, `Signature_0` mit 63,7 × 61,1 pt – genau
/// für das Signieren in ein vorhandenes Feld gebaut): angeboten, obwohl der Block darin auf gut ein Viertel
/// verkleinert wird (Prüfung „bricht keine normalen Felder“: mit der Entwurfsschwelle ⅓ war es kein Ziel), und
/// geschrieben genau im Sticker (echte Engine, Keystore, PDFium, pyHanko).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pdfas_placeholder_field_is_offered_and_signed_where_the_preview_shows_the_block() {
    let fx = Fixture::new();
    let out = fx.dir("out");
    let a = copy_pdf("pdf/pdfas/pdf-as-web/data/placeholder_sigfield_and_qr.pdf", &fx.dir("in"), "fields.pdf");
    let core = start(&fx, keystore_overlay(vec![a.clone()], &out), None, None, pdfium_dir().is_some()).expect("Kern");
    let v = wait_for(&core, "Arbeitsbereich", |v| {
        editing(v) && v.document.as_ref().is_some_and(|d| d.block.preview_revision > 0)
    })
    .await;
    let d = v.document.unwrap();
    let f = d.fields.iter().find(|f| f.name == "Signature_0").cloned().expect("Signature_0");
    assert!(f.suitable, "Platzhalterfeld der PDF-AS-Testdaten angeboten");
    let b = pt_rect(f.block.unwrap());
    let scale = b.width() / d.block.width;
    assert!(scale > 0.25 && scale < 1.0 / 3.0, "verkleinert: {scale}");
    sign_into_field_like_the_preview(&core, &a, &out.join("fields_signed.pdf"), &f).await;
}

/// DocMDP-zertifiziertes Dokument (P = 2) mit nur einem vorhandenen leeren Feld (`Leer`, 230 × 50 pt): Frei platzieren
/// ist nicht zulässig, das Feld wird angeboten und ist der einzige sichtbare Weg; signiert steht der Block genau im
/// Sticker, und die erweiterte Holder-`/BBox` bleibt eine zulässige Änderung (pyHanko: beide Signaturen intakt, alle
/// Änderungen „signing and form filling“, mit der DocMDP-Regel vereinbar).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn certified_document_is_signed_into_its_only_field_like_the_preview() {
    let fx = Fixture::new();
    let out = fx.dir("out");
    let a = copy_pdf("pades/certified-docmdp-p2.pdf", &fx.dir("in"), "cert.pdf");
    let core = start(&fx, keystore_overlay(vec![a.clone()], &out), None, None, pdfium_dir().is_some()).expect("Kern");
    let v = wait_for(&core, "Arbeitsbereich", |v| {
        matches!(v.step, StepView::Warning { .. })
            || (editing(v) && v.document.as_ref().is_some_and(|d| d.block.preview_revision > 0))
    })
    .await;
    if matches!(v.step, StepView::Warning { .. }) {
        core.dispatch(UiEvent::AnswerWarning { proceed: true });
    }
    let v = wait_for(&core, "Arbeitsbereich", |v| {
        editing(v) && v.document.as_ref().is_some_and(|d| d.block.preview_revision > 0)
    })
    .await;
    let d = v.document.unwrap();
    assert!(!d.constraints.new_visible_field_allowed);
    let f = d.fields.iter().find(|f| f.name == "Leer").cloned().expect("Feld Leer");
    assert!(f.suitable, "einziges Feld des zertifizierten Dokuments angeboten");
    sign_into_field_like_the_preview(&core, &a, &out.join("cert_signed.pdf"), &f).await;
    if tool("pyhanko") {
        let o = Command::new("pyhanko")
            .args(["sign", "validate", "--no-strict-syntax", "--pretty-print"])
            .arg(out.join("cert_signed.pdf"))
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&o.stdout).to_lowercase();
        // beide Signaturen kryptografisch intakt, die Zertifizierung sieht nur Signieren und Formular ausfüllen
        assert_eq!(text.matches("the signature is cryptographically sound").count(), 2, "pyHanko: {text}");
        assert!(
            text.contains("all modifications relate to signing and form filling operations")
                && text.contains("compatible with the current document modification policy"),
            "pyHanko: {text}"
        );
    }
}

/// `use_target` in das Feld, signieren (Keystore) und prüfen: Sticker = `FieldTargetView::block`, der Viewer zeichnet
/// den Block aus `/Rect` und Holder-`/BBox` genau dort (Seite ohne CropBox und Drehung: Benutzerraum = sichtbare
/// Seite), PDFium ebenso (Unterschied der Renderings), `/Rect` bleibt das Feld, pyHanko `INTACT`, qpdf.
async fn sign_into_field_like_the_preview(
    core: &pida_core_api::Core,
    input: &std::path::Path,
    signed: &std::path::Path,
    field: &pida_core_api::FieldTargetView,
) {
    let page = field.page.expect("Seite");
    core.dispatch(UiEvent::UseTarget { target: PlacementTarget::Field { index: field.index } });
    let v = wait_for(core, "im Feld", |v| {
        v.document.as_ref().is_some_and(|d| d.sticker == field.block && d.sticker.is_some())
    })
    .await;
    let d = v.document.unwrap();
    let sticker = pt_rect(d.sticker.unwrap());
    let page_h = d.doc.page_sizes[page].1;
    core.dispatch(UiEvent::SignWith { backend: BackendId::Keystore });
    wait_for(core, "gespeichert", saved).await;
    let bytes = std::fs::read(signed).unwrap();
    let pdf = pida_pdf_cos::SourcePdf::open(bytes.clone(), &pida_pdf_cos::Limits::default()).unwrap();
    let (written_page, drawn) = written_block(&pdf, &field.name);
    assert_eq!(written_page, page);
    assert_rect_close(drawn, sticker, 0.01, "geschrieben = Vorschau");
    let f = pdf.signature_fields().unwrap().into_iter().find(|f| f.qualified_name == field.name).unwrap();
    assert_rect_close(f.widgets[0].rect.normalized(), pt_rect(field.rect.unwrap()), 1e-3, "/Rect bleibt");
    if let Some(dir) = pdfium_dir() {
        let backend = pida_render::PdfiumBackend::bind(Some(dir.as_path())).unwrap();
        let before = pida_render::RenderBackend::open(&backend, std::fs::read(input).unwrap().into(), None).unwrap();
        let after = pida_render::RenderBackend::open(&backend, bytes.clone().into(), None).unwrap();
        let scale = 2.0;
        let region = changed_region(
            &before.render_page(page, scale).unwrap(),
            &after.render_page(page, scale).unwrap(),
            scale,
            page_h,
        )
        .expect("Block gezeichnet");
        // ein Bildpunkt (0,5 pt) Rundung der Kantenglättung
        assert_rect_close(region, sticker, 1.0, "PDFium zeichnet den Block im Sticker");
    } else {
        eprintln!("PIDA_PDFIUM_DIR nicht gesetzt – Renderprüfung übersprungen");
    }
    if tool("pyhanko") {
        let o = Command::new("pyhanko").args(["sign", "validate", "--no-strict-syntax"]).arg(signed).output().unwrap();
        let text = String::from_utf8_lossy(&o.stdout);
        assert!(text.contains("INTACT") || text.contains("intact"), "pyHanko: {text}");
    } else {
        eprintln!("pyHanko fehlt – Orakel übersprungen");
    }
    if tool("qpdf") {
        let o = Command::new("qpdf").arg("--check").arg(signed).output().unwrap();
        assert!(o.status.success(), "qpdf: {}", String::from_utf8_lossy(&o.stdout));
    }
}

/// Virtuelle neue Seite (Aufgabe Punkt 4): Größe = MediaBox der letzten Seite (nicht deren kleinere CropBox, wie
/// PDF-AS `new PDPage(lastPage.getMediaBox())`), der Block behält dort seine Größe, und geschrieben wird er in der
/// Ecke, in der ihn die Vorschau zeigt (PDFium nötig; echte Engine, pyHanko).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn new_page_has_the_media_box_size_and_the_block_keeps_its_size() {
    use pida_pdf_cos::lopdf;
    let fx = Fixture::new();
    let out = fx.dir("out");
    // letzte Seite mit kleinerer CropBox
    let src = testdata(TEXT_PDF);
    let mut doc = lopdf::Document::load(&src).unwrap();
    let pages: Vec<_> = doc.get_pages().into_values().collect();
    let last = *pages.last().unwrap();
    let media = {
        let page = doc.get_object(last).unwrap().as_dict().unwrap();
        let mb = page.get(b"MediaBox").map(floats).unwrap_or_else(|_| vec![0.0, 0.0, 595.0, 842.0]);
        pida_pdf_cos::Rect::new(mb[0], mb[1], mb[2], mb[3]).normalized()
    };
    let crop = [media.llx + 40.0, media.lly + 60.0, media.urx - 40.0, media.ury - 30.0];
    doc.get_object_mut(last)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("CropBox", lopdf::Object::Array(crop.iter().map(|v| lopdf::Object::Real(*v)).collect()));
    let a = fx.dir("in").join("cropped.pdf");
    doc.save(&a).unwrap();
    let Some(core) = start(&fx, keystore_overlay(vec![a.clone()], &out), None, None, true) else {
        eprintln!("PIDA_PDFIUM_DIR nicht gesetzt – übersprungen");
        return;
    };
    let v = wait_for(&core, "Arbeitsbereich mit Sticker", |v| {
        editing(v) && v.document.as_ref().is_some_and(|d| d.sticker.is_some() && d.block.preview_revision > 0)
    })
    .await;
    let d = v.document.unwrap();
    let n = d.doc.page_count;
    let (cw, ch) = d.doc.page_sizes[n - 1];
    assert!((cw - (crop[2] - crop[0])).abs() < 0.01 && (ch - (crop[3] - crop[1])).abs() < 0.01, "sichtbar = CropBox");
    let (nw, nh) = d.new_page_size.expect("Größe der neuen Seite");
    assert!(
        (nw - media.width()).abs() < 0.01 && (nh - media.height()).abs() < 0.01,
        "neue Seite = MediaBox: {nw} × {nh}"
    );
    let (bw, bh) = (d.block.width, d.block.height);
    if !d.new_page {
        core.dispatch(UiEvent::ToggleNewPage);
        wait_for(&core, "neue Seite", |v| v.document.as_ref().is_some_and(|d| d.new_page)).await;
    }
    // rechts unten in die Ecke der neuen Seite
    core.dispatch(UiEvent::MoveBlock { page: n, x: nw - bw, y: bh });
    let v = wait_for(&core, "auf der neuen Seite", |v| {
        v.document.as_ref().is_some_and(|d| d.placement.page_index == n && (d.placement.x - (nw - bw)).abs() < 0.01)
    })
    .await;
    let d = v.document.unwrap();
    let s = pt_rect(d.sticker.unwrap());
    assert!((s.width() - bw).abs() < 1e-3 && (s.height() - bh).abs() < 1e-3, "Blockgröße bleibt: {s:?}");
    assert_rect_close(s, pida_pdf_cos::Rect::new(nw - bw, 0.0, nw, bh), 0.01, "Ecke der neuen Seite");
    core.dispatch(UiEvent::SignWith { backend: BackendId::Keystore });
    wait_for(&core, "gespeichert", saved).await;
    let signed = out.join("cropped_signed.pdf");
    let pdf = pida_pdf_cos::SourcePdf::open(std::fs::read(&signed).unwrap(), &pida_pdf_cos::Limits::default()).unwrap();
    assert_eq!(pdf.page_count(), n + 1);
    let new_page = &pdf.pages()[n];
    assert_rect_close(new_page.media_box.normalized(), media, 1e-3, "MediaBox der letzten Seite");
    assert_rect_close(new_page.crop_box.normalized(), media, 1e-3, "ohne CropBox");
    let field = pdf.signature_fields().unwrap().into_iter().find(|f| f.signed).expect("Signaturfeld");
    assert_eq!(field.widgets[0].page_index, Some(n));
    // neue Seite ohne CropBox und Drehung: `/Rect` = sichtbare Lage (bis auf die PDF-AS-Abbildung W × H)
    assert_rect_close(field.widgets[0].rect.normalized(), s, 0.01, "geschrieben = Vorschau");
    if tool("pyhanko") {
        let o = Command::new("pyhanko").args(["sign", "validate", "--no-strict-syntax"]).arg(&signed).output().unwrap();
        let text = String::from_utf8_lossy(&o.stdout);
        assert!(text.contains("INTACT") || text.contains("intact"), "pyHanko: {text}");
    }
}

/// Einseitiges Dokument (Letter) mit `/Rotate rotation`, CropBox ≠ MediaBox und einem leeren Signaturfeld `Feld`
/// mit dem Widget-`/Rect` `rect` (Benutzerraum).
fn rotated_field_pdf(dir: &std::path::Path, rotation: i64, rect: [f32; 4]) -> PathBuf {
    use pida_pdf_cos::lopdf::{self, Object, dictionary};
    let mut doc = lopdf::Document::load(testdata(PDF)).unwrap();
    let page_id = *doc.get_pages().values().next().unwrap();
    let field = doc.add_object(dictionary! {
        "FT" => "Sig",
        "T" => Object::string_literal("Feld"),
        "Type" => "Annot",
        "Subtype" => "Widget",
        "F" => 4,
        "P" => page_id,
        "Rect" => rect.iter().map(|v| Object::Real(*v)).collect::<Vec<_>>(),
    });
    let page = doc.get_object_mut(page_id).unwrap().as_dict_mut().unwrap();
    page.set("Rotate", rotation);
    page.set("MediaBox", vec![0.into(), 0.into(), 612.into(), 792.into()]);
    page.set("CropBox", vec![Object::Real(30.0), Object::Real(40.0), Object::Real(590.0), Object::Real(770.0)]);
    page.set("Annots", vec![Object::Reference(field)]);
    let acro = doc.add_object(dictionary! { "Fields" => vec![Object::Reference(field)] });
    let root = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    doc.get_object_mut(root).unwrap().as_dict_mut().unwrap().set("AcroForm", acro);
    let path = dir.join(format!("feld-{rotation}.pdf"));
    doc.save(&path).unwrap();
    path
}

/// Gedrehte Seite mit CropBox-Ursprung (Nachtrag 16, Prüfung „gedrehte Seiten, CropBox ≠ MediaBox“): Ansicht
/// (`FieldTargetView::rect`/`block`, Sticker im sichtbaren Raum) und Ergebnis stimmen für alle Drehungen überein –
/// PDFium zeichnet den signierten Block genau im Sticker, `/Rect` bleibt, pyHanko `INTACT`. Ein großes Feld (Block in
/// natürlicher Größe) und ein kleines (Block verkleinert).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rotated_cropped_field_is_signed_exactly_where_the_preview_shows_the_block() {
    let Some(dir) = pdfium_dir() else {
        eprintln!("PIDA_PDFIUM_DIR nicht gesetzt – übersprungen");
        return;
    };
    let backend = pida_render::PdfiumBackend::bind(Some(dir.as_path())).unwrap();
    for rotation in [0, 90, 180, 270] {
        // 160 × 300 pt (bei 90°/270° sichtbar 300 × 160: Block in natürlicher Größe) bzw. 90 × 200 pt (verkleinert)
        for rect in [[120.0, 200.0, 280.0, 500.0], [300.0, 450.0, 390.0, 650.0]] {
            let fx = Fixture::new();
            let out = fx.dir("out");
            let a = rotated_field_pdf(&fx.dir("in"), rotation, rect);
            let core = start(&fx, keystore_overlay(vec![a.clone()], &out), None, None, true).expect("Kern");
            let v = wait_for(&core, "Arbeitsbereich", |v| {
                editing(v) && v.document.as_ref().is_some_and(|d| d.block.preview_revision > 0)
            })
            .await;
            let d = v.document.unwrap();
            let what = format!("{rotation}° {rect:?}");
            let f = d.fields.iter().find(|f| f.name == "Feld").cloned().expect("Feld");
            let shown = pt_rect(f.rect.unwrap());
            // sichtbarer Raum: CropBox-Ursprung abgezogen, bei 90°/270° Achsen getauscht
            let (fw, fh) = (rect[2] - rect[0], rect[3] - rect[1]);
            let (sw, sh) = if rotation % 180 == 0 { (fw, fh) } else { (fh, fw) };
            assert!((shown.width() - sw).abs() < 1e-2 && (shown.height() - sh).abs() < 1e-2, "{what}: {shown:?}");
            core.dispatch(UiEvent::UseTarget { target: PlacementTarget::Field { index: f.index } });
            let v = wait_for(&core, "im Feld", |v| v.document.as_ref().is_some_and(|d| d.sticker == f.block)).await;
            let d = v.document.unwrap();
            let sticker = pt_rect(d.sticker.unwrap());
            let page_h = d.doc.page_sizes[0].1;
            core.dispatch(UiEvent::SignWith { backend: BackendId::Keystore });
            wait_for(&core, "gespeichert", saved).await;
            let signed = out.join(format!("feld-{rotation}_signed.pdf"));
            let bytes = std::fs::read(&signed).unwrap();
            let pdf = pida_pdf_cos::SourcePdf::open(bytes.clone(), &pida_pdf_cos::Limits::default()).unwrap();
            let field = pdf.signature_fields().unwrap().into_iter().find(|f| f.qualified_name == "Feld").unwrap();
            assert!(field.signed, "{what}");
            let want = pida_pdf_cos::Rect::new(rect[0], rect[1], rect[2], rect[3]);
            assert_rect_close(field.widgets[0].rect.normalized(), want, 1e-3, &format!("{what}: /Rect bleibt"));
            let before = pida_render::RenderBackend::open(&backend, std::fs::read(&a).unwrap().into(), None).unwrap();
            let after = pida_render::RenderBackend::open(&backend, bytes.into(), None).unwrap();
            let scale = 2.0;
            let region = changed_region(
                &before.render_page(0, scale).unwrap(),
                &after.render_page(0, scale).unwrap(),
                scale,
                page_h,
            )
            .expect("Block gezeichnet");
            assert_rect_close(region, sticker, 1.0, &format!("{what}: PDFium zeichnet den Block im Sticker"));
            if tool("pyhanko") {
                let o = Command::new("pyhanko")
                    .args(["sign", "validate", "--no-strict-syntax"])
                    .arg(&signed)
                    .output()
                    .unwrap();
                let text = String::from_utf8_lossy(&o.stdout);
                assert!(text.contains("INTACT") || text.contains("intact"), "{what}: pyHanko: {text}");
            }
        }
    }
}

/// QR-Platzhalter mit eingeschalteter Platzhaltersuche: automatisch Ziel QR (PDFium nötig).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn qr_placeholder_is_used_automatically_with_search() {
    let fx = Fixture::new();
    let a = copy_pdf("pdf/pdfas/pdf-as-pdfbox-2/new_qr_2-2.pdf", &fx.dir("in"), "qr.pdf");
    fx.write_settings("[placeholder]\nsearch = true\n");
    let Some(core) = start(&fx, keystore_overlay(vec![a], &fx.dir("out")), None, Some(FakeEngine::new()), true) else {
        eprintln!("PIDA_PDFIUM_DIR nicht gesetzt – übersprungen");
        return;
    };
    let v = wait_for(&core, "Arbeitsbereich", editing).await;
    let d = v.document.unwrap();
    assert_eq!((d.target, d.origin), (PlacementTarget::Qr, PlacementOrigin::Qr), "{:?}", d.qr);
    let qr = d.qr.expect("QR-Platzhalter");
    assert_eq!(d.sticker, Some(qr.rect));
    assert!(!matches!(v.step, StepView::Placeholder { .. }), "keine Platzhalterfrage");
}

/// Amtssignatur ohne eigene Bildmarke (Backlog Nr. 10): `block.emblem_placeholder` meldet die neutrale
/// Platzhalter-Bildmarke – auch wenn die eingetragene Bildmarke fehlt (der Block zeigt dann ebenfalls den Platzhalter);
/// mit eigener Bildmarke, mit einem anderen Profil oder unsichtbar nicht. Entscheidend ist der gebaute Block
/// (Vorschau), daher erst nach der Blockvorschau prüfen.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn official_signature_without_emblem_reports_the_placeholder() {
    let emblem = concat!(env!("CARGO_MANIFEST_DIR"), "/../sigblock/assets/images/pida-seal.png");
    let missing = concat!(env!("CARGO_MANIFEST_DIR"), "/../sigblock/assets/images/gibt-es-nicht.png");
    for (settings, expected) in [
        ("[signature]\nprofile = \"AMTSSIGNATURBLOCK\"\n".to_owned(), true),
        (format!("[signature]\nprofile = \"AMTSSIGNATURBLOCK\"\nemblem = {missing:?}\n"), true),
        (format!("[signature]\nprofile = \"AMTSSIGNATURBLOCK\"\nemblem = {emblem:?}\n"), false),
        ("[signature]\nprofile = \"SIGNATURBLOCK_SMALL\"\n".to_owned(), false),
    ] {
        let fx = Fixture::new();
        let a = copy_pdf(TEXT_PDF, &fx.dir("in"), "a.pdf");
        fx.write_settings(&settings);
        // die Blockvorschau entsteht mit dem Renderer (wie in der App)
        let Some(core) = start(&fx, keystore_overlay(vec![a], &fx.dir("out")), None, None, true) else {
            eprintln!("PIDA_PDFIUM_DIR nicht gesetzt – übersprungen");
            return;
        };
        let v = wait_for(&core, "Blockvorschau", |v| {
            editing(v) && v.document.as_ref().is_some_and(|d| d.block.preview_revision > 0)
        })
        .await;
        let d = v.document.unwrap();
        assert_eq!(d.block.emblem_placeholder, expected, "{settings}");
        assert!(d.block.emblem_choosable, "{settings}: frei platziert gilt die eigene Bildmarke");
        if expected {
            core.dispatch(UiEvent::SetInvisible { invisible: true });
            let v = wait_for(&core, "unsichtbar", |v| v.document.as_ref().is_some_and(|d| d.invisible)).await;
            assert!(!v.document.unwrap().block.emblem_placeholder, "unsichtbar ohne Hinweis");
        }
    }
}

/// QR-Platzhalter mit eigenem Profil (`profile=SIGNATURBLOCK_SMALL_DE` in den PDF-AS-Testdaten): signiert wird dieses
/// Profil (PDF-Over), nicht die eingestellte Amtssignatur – kein Platzhalter-Hinweis, und eine eigene Bildmarke wirkt
/// dort nicht (`emblem_choosable = false`).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn qr_placeholder_profile_decides_the_emblem_hint() {
    let fx = Fixture::new();
    let a = copy_pdf("pdf/pdfas/pdf-as-pdfbox-2/new_qr_2-2.pdf", &fx.dir("in"), "qr.pdf");
    fx.write_settings("[signature]\nprofile = \"AMTSSIGNATURBLOCK\"\n[placeholder]\nsearch = true\n");
    let Some(core) = start(&fx, keystore_overlay(vec![a], &fx.dir("out")), None, Some(FakeEngine::new()), true) else {
        eprintln!("PIDA_PDFIUM_DIR nicht gesetzt – übersprungen");
        return;
    };
    let v = wait_for(&core, "QR-Ziel", |v| {
        v.document.as_ref().is_some_and(|d| d.target == PlacementTarget::Qr && d.block.preview_revision > 0)
    })
    .await;
    let d = v.document.unwrap();
    assert!(!d.block.emblem_placeholder, "QR-Profil SIGNATURBLOCK_SMALL_DE zeigt das PIDA-Siegel");
    assert!(!d.block.emblem_choosable, "eigene Bildmarke gilt im Profil des Platzhalters nicht");
}

/// QR-Platzhalter höher als der Block (Backlog Nr. 32): `check.placeholder_uncovered` meldet es, der Block bleibt
/// PDF-AS-gleich. Gegenprobe: der Platzhalter aus den PDF-AS-Testdaten (225 × 45 pt, Block 225 × 49 pt) ist bedeckt;
/// unsichtbar signiert gibt es keinen Hinweis.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tall_qr_placeholder_is_reported_as_uncovered() {
    for (file, uncovered) in
        [("pdf/pdfas/pdf-as-pdfbox-2/new_qr_2-2.pdf", false), ("pdf/generated/qr-tall-placeholder.pdf", true)]
    {
        let fx = Fixture::new();
        let a = copy_pdf(file, &fx.dir("in"), "qr.pdf");
        fx.write_settings("[placeholder]\nsearch = true\n");
        let Some(core) = start(&fx, keystore_overlay(vec![a], &fx.dir("out")), None, Some(FakeEngine::new()), true)
        else {
            eprintln!("PIDA_PDFIUM_DIR nicht gesetzt – übersprungen");
            return;
        };
        let v = wait_for(&core, "QR-Ziel geprüft", |v| {
            v.document.as_ref().is_some_and(|d| d.target == PlacementTarget::Qr && d.check.for_revision == d.revision)
        })
        .await;
        let d = v.document.unwrap();
        assert_eq!(d.check.placeholder_uncovered, uncovered, "{file}: {:?}", d.check);
        core.dispatch(UiEvent::SetInvisible { invisible: true });
        let v = wait_for(&core, "unsichtbar", |v| v.document.as_ref().is_some_and(|d| d.invisible)).await;
        assert!(!v.document.unwrap().check.placeholder_uncovered, "{file}: unsichtbar ohne Hinweis");
    }
}

/// Bereits signiertes bzw. DocMDP-zertifiziertes Dokument (echte Analyse): Einschränkungen mit Begründung,
/// vorhandene Signaturen; „Signieren“ frei platziert ist auf dem zertifizierten Dokument nicht freigegeben.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn constraints_of_signed_and_certified_documents() {
    let fx = Fixture::new();
    let input = fx.dir("in");
    let signed = copy_pdf("pdf/pdfas/pdf-as-pdfbox-2/new_qr_2_signed.pdf", &input, "signed.pdf");
    let core = start(&fx, keystore_overlay(vec![signed], &fx.dir("out")), None, None, false).unwrap();
    let v = wait_for(&core, "Arbeitsbereich", |v| editing(v) || matches!(v.step, StepView::Warning { .. })).await;
    let d = v.document.unwrap();
    assert!(d.doc.signed && !d.constraints.new_page_allowed);
    assert!(d.constraints.reasons.contains(&"ng-constraint-signed".to_owned()));
    assert!(!d.existing.is_empty());
    for e in &d.existing {
        assert!(!e.field_name.is_empty());
        if e.status == pida_core_api::PriorStatusView::Valid {
            assert!(e.signer.is_some() && e.signing_time.is_some(), "{e:?}");
        }
    }
    if matches!(v.step, StepView::Position { .. }) {
        core.dispatch(UiEvent::ToggleNewPage);
        wait_for(&core, "Hinweis", |v| v.notices.iter().any(|n| n.code == "placement.notice.new_page_not_allowed"))
            .await;
    }

    let fx = Fixture::new();
    let cert = copy_pdf("pades/certified-docmdp-p2.pdf", &fx.dir("in"), "cert.pdf");
    let core = start(&fx, keystore_overlay(vec![cert], &fx.dir("out")), None, None, false).unwrap();
    let v = wait_for(&core, "Arbeitsbereich", |v| {
        v.document.is_some() || matches!(v.step, StepView::Error { .. } | StepView::Warning { .. })
    })
    .await;
    if matches!(v.step, StepView::Warning { .. }) {
        core.dispatch(UiEvent::AnswerWarning { proceed: true });
    }
    let v = wait_for(&core, "Arbeitsbereich", editing).await;
    let d = v.document.unwrap();
    assert!(!d.constraints.new_visible_field_allowed);
    assert!(d.constraints.reasons.contains(&"ng-constraint-certified".to_owned()), "{:?}", d.constraints);
    assert!(!v.actions.sign, "frei platziert nicht zulässig");
    core.dispatch(UiEvent::SignWith { backend: BackendId::Keystore });
    wait_for(&core, "Hinweis", |v| v.notices.iter().any(|n| n.code == "ng-constraint-certified")).await;
    assert!(editing(&core.view()));
    core.dispatch(UiEvent::SetInvisible { invisible: true });
    let v = wait_for(&core, "unsichtbar", |v| v.document.as_ref().is_some_and(|d| d.invisible)).await;
    assert!(v.actions.sign, "unsichtbar zulässig");
}

// ------------------------------------------------------------------------------------------------
// Layout, Einrastlinien, Überdeckung (Abnahme E1 Punkt 3) – Zähl-Renderer: nie PDFium im Ereignispfad
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Default)]
struct Counts {
    forced: AtomicUsize,
    /// künstliche Dauer je Raster in ms (Prüfung „None, bis das Raster da ist“)
    delay_ms: AtomicUsize,
}

struct CountingBackend {
    inner: pida_render::PdfiumBackend,
    counts: Arc<Counts>,
}

impl pida_render::RenderBackend for CountingBackend {
    fn open(&self, data: Arc<[u8]>, password: Option<&str>) -> pida_render::Result<Box<dyn pida_render::Renderer>> {
        Ok(Box::new(CountingRenderer { inner: self.inner.open(data, password)?, counts: self.counts.clone() }))
    }
    fn describe(&self) -> String {
        self.inner.describe()
    }
}

struct CountingRenderer {
    inner: Box<dyn pida_render::Renderer>,
    counts: Arc<Counts>,
}

impl pida_render::Renderer for CountingRenderer {
    fn page_count(&self) -> usize {
        self.inner.page_count()
    }
    fn page_size(&self, page: usize) -> pida_render::Result<pida_render::PageSize> {
        self.inner.page_size(page)
    }
    fn render_page(&self, page: usize, scale: f32) -> pida_render::Result<pida_render::RgbaImage> {
        self.inner.render_page(page, scale)
    }
    fn render_forced_color(
        &self,
        page: usize,
        opts: &pida_render::ForcedColorOptions,
    ) -> pida_render::Result<pida_render::RgbaImage> {
        self.counts.forced.fetch_add(1, Ordering::SeqCst);
        let delay = self.counts.delay_ms.load(Ordering::SeqCst);
        if delay > 0 {
            std::thread::sleep(Duration::from_millis(delay as u64));
        }
        self.inner.render_forced_color(page, opts)
    }
    fn image_objects(&self, page: usize) -> pida_render::Result<Vec<pida_render::ImageObject>> {
        self.inner.image_objects(page)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn layout_and_overlap_check_read_only_the_grid() {
    let Some(dir) = pdfium_dir() else {
        eprintln!("PIDA_PDFIUM_DIR nicht gesetzt – übersprungen");
        return;
    };
    let fx = Fixture::new();
    let a = copy_pdf(TEXT_PDF, &fx.dir("in"), "doc.pdf");
    let counts = Arc::new(Counts::default());
    let backend =
        CountingBackend { inner: pida_render::PdfiumBackend::bind(Some(&dir)).unwrap(), counts: counts.clone() };
    let mut cfg = fx.config(keystore_overlay(vec![a], &fx.dir("out")), false);
    cfg.pdfium_dir = Some(dir);
    let opts = CoreOptions {
        workspace_ui: Some(true),
        engine: Some(FakeEngine::new()),
        render: Some(Arc::new(backend)),
        ..CoreOptions::isolated()
    };
    let core = pida_core_api::Core::new_with(cfg, opts).unwrap();
    let v = wait_for(&core, "Arbeitsbereich", editing).await;
    let page = v.document.as_ref().unwrap().placement.page_index;
    core.dispatch(UiEvent::InspectPages { pages: vec![page] });
    // Layout der Seite und Prüfung, sobald das Raster da ist
    let v = wait_for(&core, "Layout", |v| {
        v.document.as_ref().is_some_and(|d| {
            d.layout.iter().any(|l| l.page == page && l.content.is_some()) && d.check.overlaps_content.is_some()
        })
    })
    .await;
    let d = v.document.unwrap();
    let l = d.layout.iter().find(|l| l.page == page).unwrap();
    let content = l.content.unwrap();
    assert!(!l.guides.is_empty() && l.free.len() <= 16);
    assert!(l.guides.iter().any(|g| g.kind == pida_core_api::GuideKind::ContentBottom));
    assert!(d.announce.contains_key("bottom"), "Live-Region mit Unterkante");
    // warten, bis die automatische Platzierung (PDF-AS-Scan) durch ist, dann zählen – unter Last (volle
    // Testsuite) dauert sie länger als eine feste Wartezeit: bis der Zähler 400 ms lang ruht
    let mut before = counts.forced.load(Ordering::SeqCst);
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(400)).await;
        let now = counts.forced.load(Ordering::SeqCst);
        if now == before && now > 0 {
            break;
        }
        before = now;
    }
    // auf den Inhalt ziehen → Überdeckung; in eine freie Fläche → keine
    core.dispatch(UiEvent::MoveBlock { page, x: content.left, y: content.top });
    let v = wait_for(&core, "Überdeckung", |v| {
        v.document
            .as_ref()
            .is_some_and(|d| d.check.for_revision == d.revision && d.check.overlaps_content == Some(true))
    })
    .await;
    assert_eq!(v.document.as_ref().unwrap().origin, PlacementOrigin::User);
    let free = l.free.first().copied().expect("freie Fläche");
    core.dispatch(UiEvent::MoveBlock { page, x: free.left, y: free.top });
    wait_for(&core, "frei", |v| {
        v.document
            .as_ref()
            .is_some_and(|d| d.check.for_revision == d.revision && d.check.overlaps_content == Some(false))
    })
    .await;
    for i in 0..20 {
        core.dispatch(UiEvent::NudgeBlock { dx: if i % 2 == 0 { 1 } else { -1 }, dy: 0, fine: true, coarse: false });
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(counts.forced.load(Ordering::SeqCst), before, "kein PDFium-Aufruf im Ereignispfad");
    // „Freie Fläche suchen“: automatische Platzierung (PDF-AS-Scan), Herkunft wieder automatisch
    core.dispatch(UiEvent::AutoPlace);
    wait_for(&core, "automatisch", |v| v.document.as_ref().is_some_and(|d| d.origin == PlacementOrigin::Auto)).await;
}

/// Seite ohne fertiges Raster: `overlaps_content = None`, bis es da ist – dann gilt das Ergebnis für dieselbe
/// Revision (Raster künstlich verzögert).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn overlap_is_unknown_until_the_grid_is_ready() {
    let Some(dir) = pdfium_dir() else {
        eprintln!("PIDA_PDFIUM_DIR nicht gesetzt – übersprungen");
        return;
    };
    let fx = Fixture::new();
    let a = copy_pdf("pdf/pdfas/pdf-as-pdfbox-2/manySignFields.pdf", &fx.dir("in"), "multi.pdf");
    let counts = Arc::new(Counts::default());
    counts.delay_ms.store(600, Ordering::SeqCst);
    let backend =
        CountingBackend { inner: pida_render::PdfiumBackend::bind(Some(&dir)).unwrap(), counts: counts.clone() };
    let mut cfg = fx.config(keystore_overlay(vec![a], &fx.dir("out")), false);
    cfg.pdfium_dir = Some(dir);
    let opts = CoreOptions {
        workspace_ui: Some(true),
        engine: Some(FakeEngine::new()),
        render: Some(Arc::new(backend)),
        ..CoreOptions::isolated()
    };
    let core = pida_core_api::Core::new_with(cfg, opts).unwrap();
    let v = wait_for(&core, "Arbeitsbereich", editing).await;
    let d = v.document.unwrap();
    if d.doc.page_count < 2 {
        eprintln!("Dokument mit einer Seite – übersprungen");
        return;
    }
    let other = if d.placement.page_index == 0 { 1 } else { 0 };
    core.dispatch(UiEvent::MoveBlock { page: other, x: 40.0, y: 300.0 });
    let v =
        wait_for(&core, "verschoben", |v| v.document.as_ref().is_some_and(|d| d.placement.page_index == other)).await;
    let d = v.document.unwrap();
    assert_eq!(d.check.overlaps_content, None, "Raster der Seite noch nicht berechnet");
    let revision = d.revision;
    let v =
        wait_for(&core, "Raster", |v| v.document.as_ref().is_some_and(|d| d.check.overlaps_content.is_some())).await;
    assert_eq!(v.document.unwrap().check.for_revision, revision, "neue Projektion mit derselben Revision");
}

// ------------------------------------------------------------------------------------------------
// Zustände des echten Kerns für die Oberfläche (Integration K1 × A1, ADR 0004 §6.2 Punkt 4)
// ------------------------------------------------------------------------------------------------

/// Echte `ViewState`s der Kernpfade für `apps/app/src/app/RealCore.test.tsx`: Die Oberfläche rendert sie und
/// prüft „ohne Bildschirmwechsel“, Karte, Fehler, Abbruch und „Fertig“ (F3) gegen den echten Vertrag statt
/// gegen den Mock. Neu schreiben: `PIDA_BLESS=1 cargo test -p pida-core-api --test workspace real_core_views`
/// (mit `PIDA_PDFIUM_DIR`, dann mit Kacheln, Layout und Startlage der PDF-AS-Regel). Ohne `PIDA_BLESS`
/// laufen dieselben Abläufe als Test, und die Datei muss sich als `ViewState` lesen lassen.
const UI_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../apps/app/src/core/fixtures/real-core-views.json");

/// Zustand für die Datei: lokale Pfade und Zeitpunkte ersetzt (stabil, ohne Daten des Rechners).
fn ui_fixture_state(v: &ViewState, roots: &[(String, &str)]) -> serde_json::Value {
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
    // Sprache der Oberfläche fest auf Deutsch (der Kern nimmt sonst die Systemsprache des Rechners; die
    // UI-Probe prüft deutsche Texte)
    value["language"] = serde_json::Value::String("de".into());
    value
}

/// Arbeitsbereich mit Blockvorschau und (mit PDFium) Layout und Prüfung; danach die automatische Platzierung
/// (PDF-AS-Scan) abwarten.
async fn settled(core: &pida_core_api::Core, pdfium: bool) {
    wait_for(core, "Arbeitsbereich mit Vorschau und Layout", |v| {
        editing(v)
            && v.document.as_ref().is_some_and(|d| {
                d.block.preview_revision > 0
                    && (!pdfium || (!d.layout.is_empty() && d.check.overlaps_content.is_some()))
            })
    })
    .await;
    tokio::time::sleep(Duration::from_millis(400)).await;
}

fn signing_form(v: &ViewState, pred: impl Fn(&NativeForm) -> bool) -> bool {
    matches!(&v.step, StepView::Signing { interaction: InteractionView::Form { form, .. }, .. } if pred(form))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_core_views_for_the_ui() {
    let pdfium = pdfium_dir().is_some();
    let mut states: Vec<(String, ViewState)> = Vec::new();
    let mut roots: Vec<(String, &str)> = Vec::new();
    let mut add_roots = |fx: &Fixture| {
        let tmp = fx.tmp.path().to_path_buf();
        for p in [tmp.clone(), tmp.canonicalize().unwrap_or(tmp)] {
            roots.push((p.display().to_string(), "/Users/test"));
        }
    };

    // 1. Keystore, echte Engine: Arbeitsbereich → Signieren → gespeichert (Abnahme E1 Punkt 1, 7)
    let fx = Fixture::new();
    fx.write_settings("[general]\n"); // Bestandsnutzer: ohne Ersteinrichtung (K4)
    add_roots(&fx);
    let out = fx.dir("Dokumente");
    let a = copy_pdf(TEXT_PDF, &fx.dir("in"), "Vertrag.pdf");
    let core = start(&fx, keystore_overlay(vec![a], &out), None, None, pdfium).expect("Kern");
    settled(&core, pdfium).await;
    let v = core.view();
    assert!(editing(&v), "{:?}", v.step);
    let editing_doc = v.document.clone();
    states.push(("keystore_editing".into(), v));
    // „Zur Startseite“ parkt das Dokument (Nachtrag 10), „Weiter bearbeiten“ bringt es unverändert zurück
    core.dispatch(UiEvent::GoHome);
    let v = wait_for(&core, "geparkt", |v| v.parked.is_some() && v.document.is_none()).await;
    states.push(("doc_parked".into(), v));
    core.dispatch(UiEvent::ResumeParked);
    let v = wait_for(&core, "fortgesetzt", |v| v.parked.is_none() && editing(v)).await;
    assert_eq!(v.document.map(|d| (d.doc.id, d.placement)), editing_doc.map(|d| (d.doc.id, d.placement)));
    core.dispatch(UiEvent::SignWith { backend: BackendId::Keystore });
    let seen = collect_until(&core, "gespeichert", saved).await;
    if let Some(v) = seen.iter().rev().find(|v| matches!(v.step, StepView::Signing { .. })) {
        states.push(("keystore_signing".into(), v.clone()));
    }
    let done = seen.last().unwrap().clone();
    assert!(matches!(&done.step, StepView::Output { result, .. } if result.summary.is_some()));
    states.push(("keystore_done".into(), done));
    drop(core);

    // 2. ID-Austria-artig (Skript): Anmeldung → Netzfehler → „Erneut versuchen“ → TAN → gespeichert (Punkt 6)
    let fx = Fixture::new();
    fx.write_settings("[general]\n"); // Bestandsnutzer: ohne Ersteinrichtung (K4)
    add_roots(&fx);
    let out = fx.dir("Dokumente");
    let a = copy_pdf(PDF, &fx.dir("in"), "Antrag.pdf");
    let (_backend, backends) = scripted(vec![BackendError::Network("Verbindung getrennt".into())]);
    let overlay = Overlay { input_files: vec![a], output: Some(out), ..Overlay::default() };
    let core = start(&fx, overlay, backends, None, pdfium).expect("Kern");
    settled(&core, pdfium).await;
    core.dispatch(UiEvent::SelectSigner { backend: Some(BackendId::AtrustNative), make_default: false });
    let v = wait_for(&core, "gewählt", |v| v.signers.selected == Some(BackendId::AtrustNative)).await;
    states.push(("atrust_editing".into(), v));
    core.dispatch(UiEvent::SignWith { backend: BackendId::AtrustNative });
    let v = wait_for(&core, "Anmeldung", |v| signing_form(v, |f| matches!(f, NativeForm::Login { .. }))).await;
    states.push(("atrust_login".into(), v));
    login(&core).await;
    let v = wait_for(&core, "Netzfehler", |v| matches!(v.step, StepView::Error { .. })).await;
    states.push(("atrust_network_error".into(), v));
    core.dispatch(UiEvent::Retry);
    let v = wait_for(&core, "TAN", |v| signing_form(v, |f| matches!(f, NativeForm::SmsTan { .. }))).await;
    states.push(("atrust_tan".into(), v));
    tan(&core).await;
    let v = wait_for(&core, "gespeichert", saved).await;
    states.push(("atrust_done".into(), v));
    drop(core);

    // 3. Abbrechen in der Anmeldung → Arbeitsbereich mit Hinweis „es wurde nichts signiert“
    let fx = Fixture::new();
    fx.write_settings("[general]\n"); // Bestandsnutzer: ohne Ersteinrichtung (K4)
    add_roots(&fx);
    let a = copy_pdf(PDF, &fx.dir("in"), "Antrag.pdf");
    let (_backend, backends) = scripted(vec![]);
    let core =
        start(&fx, Overlay { input_files: vec![a], ..Overlay::default() }, backends, None, pdfium).expect("Kern");
    settled(&core, pdfium).await;
    core.dispatch(UiEvent::SignWith { backend: BackendId::AtrustNative });
    form_request(&core, "Anmeldung", |f| matches!(f, NativeForm::Login { .. })).await;
    core.dispatch(UiEvent::Cancel);
    let v =
        wait_for(&core, "abgebrochen", |v| editing(v) && v.notices.iter().any(|n| n.code == "ng-signing-cancelled"))
            .await;
    states.push(("atrust_cancelled".into(), v));
    drop(core);

    // 4. Einstellungen › Dokument (E3, Nachtrag 5): erkannte PDF/A-Kennung, erzwungene PDF/A-Kompatibilität,
    // Platzhaltersuche „Leere Signaturfelder“ (erstes Feld automatisch) und „QR-Code-Platzhalter“ (mit PDFium)
    let doc_cases: [(&str, &str, &str, bool); 5] = [
        ("doc_pdfa_detected", "pdf/pdfas-suites/PDFA_SIGNATURBLOCK_DE/TestGhostscriptPdfA.pdf", "", false),
        // Einstellung „Automatische Positionierung“ (Nachtrag 8): automatisch platziert und gesperrt
        ("doc_auto_locked", TEXT_PDF, "[signature]\nauto_position = true\n", false),
        ("doc_pdfa_forced", TEXT_PDF, "[signature]\npdfa_force = true\n", false),
        (
            "doc_field_auto",
            "pdf/pdfas/pdf-as-pdfbox-2/manySignFields.pdf",
            "[placeholder]\nsearch = true\nkind = \"fields\"\n",
            false,
        ),
        (
            "doc_qr_auto",
            "pdf/pdfas/pdf-as-pdfbox-2/new_qr_2-2.pdf",
            "[placeholder]\nsearch = true\nkind = \"marker\"\n",
            true,
        ),
    ];
    for (name, pdf, toml, needs_pdfium) in doc_cases {
        if needs_pdfium && !pdfium {
            continue;
        }
        let fx = Fixture::new();
        add_roots(&fx);
        // Bestandsnutzer: ohne Ersteinrichtung (K4; Merge E4 – sonst läge sie über dem Arbeitsbereich)
        fx.write_settings(if toml.is_empty() { "[general]\n" } else { toml });
        let a = copy_pdf(pdf, &fx.dir("in"), "Formular.pdf");
        let core =
            start(&fx, Overlay { input_files: vec![a], ..Overlay::default() }, None, None, pdfium).expect("Kern");
        // im Feld bzw. QR-Platzhalter ohne Layoutanfrage der Oberfläche: nur Vorschau und Prüfung abwarten
        wait_for(&core, "Arbeitsbereich mit Vorschau", |v| {
            editing(v)
                && v.document
                    .as_ref()
                    .is_some_and(|d| d.block.preview_revision > 0 && (!pdfium || d.check.overlaps_content.is_some()))
        })
        .await;
        tokio::time::sleep(Duration::from_millis(400)).await;
        let v = core.view();
        let det = v.document.as_ref().and_then(|d| d.detection.clone()).expect("Erkennung");
        match name {
            "doc_pdfa_detected" => assert_eq!(det.pdfa_decision, pida_core_api::PdfaDecision::Detected),
            "doc_pdfa_forced" => assert_eq!(det.pdfa_decision, pida_core_api::PdfaDecision::Forced),
            "doc_field_auto" => assert!(
                det.auto_placed && matches!(v.document.as_ref().unwrap().target, PlacementTarget::Field { index: 0 })
            ),
            "doc_auto_locked" => assert!(det.auto_placed && v.document.as_ref().unwrap().placement_locked),
            _ => assert!(det.auto_placed && v.document.as_ref().unwrap().target == PlacementTarget::Qr),
        }
        states.push((name.into(), v));
        drop(core);
    }

    // 5. Einstellungen „sofort wirksam“ (E3): offene Ansicht mit Dokument, dann eine Teilübernahme mit
    // ungültigem Proxy-Port (Feldfehler) und gültigen Änderungen (Signaturbild, Einstellungen › Dokument)
    let fx = Fixture::new();
    fx.write_settings("[general]\n"); // Bestandsnutzer: ohne Ersteinrichtung (K4)
    add_roots(&fx);
    let a = copy_pdf(TEXT_PDF, &fx.dir("in"), "Vertrag.pdf");
    let core = start(&fx, Overlay { input_files: vec![a], ..Overlay::default() }, None, None, pdfium).expect("Kern");
    settled(&core, pdfium).await;
    core.dispatch(UiEvent::OpenSettings);
    let v = wait_for(&core, "Einstellungen", |v| v.settings.as_ref().is_some_and(|s| s.preview_revision > 0)).await;
    states.push(("settings_open".into(), v));
    let mut changes = std::collections::BTreeMap::new();
    changes.insert("proxy.mode".to_owned(), serde_json::json!("manual"));
    changes.insert("proxy.host".to_owned(), serde_json::json!("proxy.local"));
    changes.insert("proxy.port".to_owned(), serde_json::json!(70000));
    changes.insert("signature.profile".to_owned(), serde_json::json!("AMTSSIGNATURBLOCK"));
    changes.insert("placeholder.search".to_owned(), serde_json::json!(true));
    changes.insert("signature.pdfa_force".to_owned(), serde_json::json!(true));
    core.dispatch(UiEvent::ApplySettings { changes, revision: 1 });
    let v = wait_for(&core, "übernommen", |v| v.settings.as_ref().is_some_and(|s| s.applied_revision >= 1)).await;
    assert_eq!(
        v.settings.as_ref().unwrap().problems.get("proxy.port").map(String::as_str),
        Some("ng-settings-invalid-port")
    );
    states.push(("settings_partial".into(), v));
    drop(core);

    // Repository-Pfad (Keystore-Datei) ebenfalls ersetzen
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let repo = repo.canonicalize().unwrap_or(repo);
    roots.push((repo.display().to_string(), "/Users/test/pida"));
    // die Proben zeigen Arbeitsbereich und Einstellungen ohne die Ersteinrichtung darüber (Merge E4)
    for (name, v) in &states {
        assert!(v.first_run.is_none(), "{name}: Ersteinrichtung offen – Probe als Bestandsnutzer erzeugen");
    }
    let fixture: serde_json::Map<String, serde_json::Value> =
        states.iter().map(|(name, v)| (name.clone(), ui_fixture_state(v, &roots))).collect();
    if std::env::var_os("PIDA_BLESS").is_some() {
        if !pdfium {
            eprintln!("Hinweis: ohne PIDA_PDFIUM_DIR erzeugt – ohne Layout und PDF-AS-Startlage");
        }
        let text = serde_json::to_string_pretty(&fixture).unwrap() + "\n";
        std::fs::write(UI_FIXTURE, text).expect("Fixture schreiben");
        return;
    }
    let text = std::fs::read_to_string(UI_FIXTURE).expect("Fixture lesen (PIDA_BLESS=1 erzeugt sie)");
    let stored: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&text).unwrap();
    // `keystore_signing` kann fehlen, wenn das Signieren zwischen zwei Projektionen fertig wurde
    let required: Vec<&String> = fixture.keys().filter(|k| *k != "keystore_signing").collect();
    for name in required {
        assert!(
            stored.contains_key(name),
            "real-core-views.json ohne {name}: PIDA_BLESS=1 cargo test -p pida-core-api --test workspace real_core_views"
        );
    }
    for (name, value) in stored {
        let _: ViewState = serde_json::from_value(value).unwrap_or_else(|e| {
            panic!("{name}: real-core-views.json passt nicht mehr zum Vertrag ({e}); mit PIDA_BLESS=1 neu erzeugen")
        });
    }
}

/// Inspektor › Optionen: Sprache des Signaturblocks umschalten (`set_block_language`) – ändert und speichert die
/// Einstellung, der Block zeigt sie; im Arbeitsbereich ohne Seitenwechsel.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn block_language_switch_changes_the_setting() {
    let fx = Fixture::new();
    let a = copy_pdf(PDF, &fx.dir("in"), "a.pdf");
    let core = start(&fx, Overlay { input_files: vec![a], ..Overlay::default() }, None, None, false).unwrap();
    wait_for(&core, "Arbeitsbereich", editing).await;
    // unabhängig von der Systemsprache des Rechners (ohne Einstellung gilt sie): auf die gespeicherte Datei warten –
    // eine Wahl gleich der Systemsprache ändert die Ansicht nicht, wohl aber die Einstellung
    let saved = |code: &str| {
        std::fs::read_to_string(fx.paths.settings_file())
            .is_ok_and(|t| t.contains(&format!("block_language = \"{code}\"")))
    };
    for lang in [pida_i18n::Lang::En, pida_i18n::Lang::De, pida_i18n::Lang::En] {
        core.dispatch(UiEvent::SetBlockLanguage { language: lang });
        let code = if lang == pida_i18n::Lang::En { "en" } else { "de" };
        let v = wait_for(&core, "Sprache gespeichert", |v| {
            saved(code) && v.document.as_ref().is_some_and(|d| d.block.language == lang)
        })
        .await;
        assert!(editing(&v), "bleibt im Arbeitsbereich");
    }
}

/// Einstellungen › Barrierefreiheit ohne Einstellungsansicht (`set_accessibility`, Schalter „Barrierefreie
/// Bedienung“): sofort in `ViewState::accessibility` und gespeichert; die Schrittweite der Pfeiltasten gilt für
/// `nudge_block` (Umschalt bleibt 1 pt).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn accessibility_settings_apply_without_settings_view_and_set_the_keyboard_step() {
    let fx = Fixture::new();
    let input = fx.dir("in");
    let out = fx.dir("out");
    let a = copy_pdf(TEXT_PDF, &input, "a.pdf");
    // Verschieben braucht die Seitengeometrie (PDFium); ohne PDFium übersprungen wie die übrigen Layout-Tests
    let Some(core) = start(&fx, keystore_overlay(vec![a], &out), None, Some(FakeEngine::new()), true) else { return };
    settled(&core, true).await;
    assert_eq!(core.view().accessibility, Some(pida_config::AccessibilitySettings::default()));
    let wanted = pida_config::AccessibilitySettings {
        mode: true,
        start: pida_config::PlacementStart::Methods,
        announce: pida_config::AnnounceDetail::Detailed,
        announce_after_move: true,
        keyboard_step: pida_config::KeyboardStep::Mm10,
        check_before_sign: true,
    };
    core.dispatch(UiEvent::SetAccessibility { accessibility: wanted });
    wait_for(&core, "Barrierefreiheit übernommen", |v| v.accessibility == Some(wanted)).await;
    let saved = std::fs::read_to_string(fx.paths.settings_file()).expect("Einstellungen gespeichert");
    assert!(saved.contains("[accessibility]") && saved.contains("keyboard_step = \"mm10\""), "{saved}");

    core.dispatch(UiEvent::MoveBlock { page: 0, x: 100.0, y: 500.0 });
    wait_for(&core, "verschoben", |v| v.document.as_ref().is_some_and(|d| (d.placement.x - 100.0).abs() < 0.01)).await;
    core.dispatch(UiEvent::NudgeBlock { dx: 1, dy: -1, fine: false, coarse: false });
    let step = 10.0 * 72.0 / 25.4;
    wait_for(&core, "um 10 mm verschoben", |v| {
        v.document.as_ref().is_some_and(|d| {
            (d.placement.x - (100.0 + step)).abs() < 0.01 && (d.placement.y - (500.0 - step)).abs() < 0.01
        })
    })
    .await;
    core.dispatch(UiEvent::NudgeBlock { dx: 1, dy: 0, fine: true, coarse: false });
    wait_for(&core, "Umschalt: 1 pt", |v| {
        v.document.as_ref().is_some_and(|d| (d.placement.x - (101.0 + step)).abs() < 0.01)
    })
    .await;
}

// ------------------------------------------------------------------------------------------------
// Mehrfachsignatur: mehrere Blöcke in einem Dokument, je Block eine eigene Signatur
// ------------------------------------------------------------------------------------------------

fn blocks(v: &ViewState) -> Option<&pida_core_api::BlocksView> {
    v.document.as_ref().and_then(|d| d.blocks.as_ref())
}

/// Drei Blöcke platzieren (hinzufügen, auswählen, entfernen, wieder hinzufügen), mit dem Keystore signieren: eine
/// Sitzung, drei Bestätigungen, das Ergebnis trägt drei Signaturen an den gezeigten Stellen; jede bleibt gültig
/// (pyHanko), qpdf. PDFium nötig (ohne Vorschau gibt es nur einen automatisch platzierten Block).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn several_blocks_sign_the_document_once_per_block() {
    let fx = Fixture::new();
    let input = fx.dir("in");
    let out = fx.dir("out");
    let a = copy_pdf(TEXT_PDF, &input, "doc.pdf");
    let Some(core) = start(&fx, keystore_overlay(vec![a.clone()], &out), None, None, true) else {
        eprintln!("PIDA_PDFIUM_DIR nicht gesetzt – übersprungen");
        return;
    };
    let v = wait_for(&core, "Arbeitsbereich mit Block", |v| {
        editing(v) && v.document.as_ref().is_some_and(|d| d.sticker.is_some()) && blocks(v).is_some_and(|b| b.can_add)
    })
    .await;
    let b = blocks(&v).unwrap();
    assert_eq!((b.items.len(), b.limit, b.items[0].active), (1, 10, true));
    let first = b.items[0].clone();

    // zweiter Block auf Seite 1, dritter auf der Seite des ausgewählten – keiner überdeckt einen anderen
    core.dispatch(UiEvent::AddBlock { page: Some(0) });
    let v = wait_for(&core, "zwei Blöcke", |v| blocks(v).is_some_and(|b| b.items.len() == 2)).await;
    let b = blocks(&v).unwrap();
    assert!(b.items[1].active && !b.items[0].active, "der neue Block ist ausgewählt");
    assert_eq!(b.items[0].rect, first.rect, "der erste Block bleibt, wo er war");
    assert_eq!(b.items[1].page, 0);
    let d = v.document.as_ref().unwrap();
    assert!(!d.check.overlaps_block);
    assert_eq!(d.sticker, b.items[1].rect);
    core.dispatch(UiEvent::AddBlock { page: None });
    let v = wait_for(&core, "drei Blöcke", |v| blocks(v).is_some_and(|b| b.items.len() == 3)).await;
    assert!(!v.document.as_ref().unwrap().check.overlaps_block);

    // auswählen: die Werkzeuge wirken auf den ausgewählten Block, die anderen bleiben liegen
    core.dispatch(UiEvent::SelectBlock { index: 0 });
    let v = wait_for(&core, "erster ausgewählt", |v| blocks(v).is_some_and(|b| b.items[0].active)).await;
    let before = blocks(&v).unwrap().clone();
    assert_eq!(v.document.as_ref().unwrap().sticker, before.items[0].rect);
    core.dispatch(UiEvent::NudgeBlock { dx: 3, dy: 0, fine: false, coarse: false });
    let v = wait_for(&core, "verschoben", |v| blocks(v).is_some_and(|b| b.items[0].rect != before.items[0].rect)).await;
    let after = blocks(&v).unwrap();
    assert_eq!((&after.items[1], &after.items[2]), (&before.items[1], &before.items[2]));

    // den auf den ersten geschobenen Block meldet die Prüfung
    let r0 = after.items[0].rect.unwrap();
    core.dispatch(UiEvent::SelectBlock { index: 2 });
    wait_for(&core, "dritter ausgewählt", |v| blocks(v).is_some_and(|b| b.items[2].active)).await;
    core.dispatch(UiEvent::MoveBlock { page: after.items[0].page, x: r0.left + 5.0, y: r0.top - 5.0 });
    wait_for(&core, "überdeckt", |v| v.document.as_ref().is_some_and(|d| d.check.overlaps_block)).await;

    // entfernen und wieder hinzufügen
    core.dispatch(UiEvent::RemoveBlock { index: 2 });
    let v = wait_for(&core, "wieder zwei", |v| blocks(v).is_some_and(|b| b.items.len() == 2)).await;
    assert!(blocks(&v).unwrap().items[1].active, "der vorige Block ist ausgewählt");
    core.dispatch(UiEvent::AddBlock { page: Some(0) });
    let v = wait_for(&core, "wieder drei", |v| {
        blocks(v).is_some_and(|b| b.items.len() == 3) && v.document.as_ref().is_some_and(|d| !d.check.overlaps_block)
    })
    .await;
    let placed = blocks(&v).unwrap().items.clone();

    core.dispatch(UiEvent::SignWith { backend: BackendId::Keystore });
    let seen = collect_until(&core, "gespeichert", saved).await;
    let progress: Vec<(u32, u32)> = seen
        .iter()
        .filter_map(|v| match &v.step {
            StepView::Signing { progress, .. } => Some((progress.confirmations_done, progress.confirmations_total)),
            _ => None,
        })
        .collect();
    assert!(progress.iter().all(|(_, total)| *total == 3), "je Block eine Bestätigung: {progress:?}");
    assert!(progress.windows(2).all(|w| w[0].0 <= w[1].0), "Fortschritt nur vorwärts: {progress:?}");
    for v in &seen {
        assert!(v.document.is_some(), "Dokumentansicht fehlt: {:?}", v.step);
    }

    let signed = out.join("doc_signed.pdf");
    let bytes = std::fs::read(&signed).unwrap();
    let pdf = pida_pdf_cos::SourcePdf::open(bytes, &pida_pdf_cos::Limits::default()).unwrap();
    let fields: Vec<_> = pdf.signature_fields().unwrap().into_iter().filter(|f| f.signed).collect();
    assert_eq!(fields.len(), 3, "drei Signaturen");
    // jede Signatur steht dort, wo ihr Block lag (Seite ohne CropBox und Drehung)
    for block in &placed {
        let want = pt_rect(block.rect.unwrap());
        let hit = fields.iter().any(|f| {
            let (page, drawn) = written_block(&pdf, &f.qualified_name);
            page == block.page && (drawn.llx - want.llx).abs() < 0.5 && (drawn.ury - want.ury).abs() < 0.5
        });
        assert!(hit, "Block {} nicht an seiner Stelle: {want:?}", block.index);
    }
    if tool("pyhanko") {
        let o = Command::new("pyhanko").args(["sign", "validate", "--no-strict-syntax"]).arg(&signed).output().unwrap();
        let text = String::from_utf8_lossy(&o.stdout);
        assert_eq!(text.matches("INTACT").count() + text.matches("intact").count(), 3, "pyHanko: {text}");
    } else {
        eprintln!("pyHanko fehlt – Orakel übersprungen");
    }
    if tool("qpdf") {
        let o = Command::new("qpdf").arg("--check").arg(&signed).output().unwrap();
        assert!(o.status.success(), "qpdf: {}", String::from_utf8_lossy(&o.stdout));
    }
}
