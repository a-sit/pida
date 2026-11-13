//! Kern-Task: hält den Workflow-Zustand, wendet den Reducer an, führt Effekte aus und projiziert den
//! `ViewState` (Plan §3.4/§3.5). Alles, was den Zustand ändert, läuft seriell über einen Kanal; lange
//! Effekte (Laden, Backend-Aufrufe, `prepare`, `finish`, Schreiben) laufen als Tokio-Tasks und melden
//! sich als Ereignis zurück. Ergebnisse eines abgebrochenen Signiervorgangs werden über eine
//! Epoche verworfen.

use pida_fs::PathExt as _;
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use pida_backend_api::{
    BackendError, BackendEvent, BackendId, BackendSession, CancelToken, Capabilities, CertChain, DigestJob,
    DocumentJob, DocumentSource, ExcludedByteRange, Platform, SessionContext, SignInput, SignJob, SignResult,
};
use pida_cades::{Expect, ExpectedSigner, SignedAttrs, SignedAttrsBuilder, SigningTime};
use pida_config::{CertCacheKey, CertificateCache, Effective, Overlay, Policy, Settings};
use pida_i18n::Lang;
use pida_pades::{CheckContext, FinishError, Invariant, InvariantViolation, OutputTarget, PadesError, Reserve};
use pida_placement::{EmptyFieldCandidate, PageTransform, Placement, Point, ScreenGeometry};
use pida_render::RenderBackend;
use pida_sigblock::{FontProvider, ProfileStore};
use pida_workflow::{
    BackendOption, Context, DocId, Effect, Event, ExistingPolicy, ExitCode, FsProbe, InputDocument, OutputDecision,
    PositionChoice, ResumePlan, State, Step, WorkflowError, chosen_path, decide_output, reduce, resolve_without_dialog,
};
use tokio::sync::{mpsc, watch};

use crate::broker::{BrokerSink, CoreBroker, ShellHooks};
use crate::config::{ConfigState, StartupProblem, validate};
use crate::docs::{DocData, DocStore, Loaded};
use crate::engine::{PadesEngine, PreparedDoc, SignedDoc};
use crate::plan::{self, BlockSpec, PlanInput, SpecInput};
use crate::resume;
use crate::view::{
    BackendOptionView, BlockView, CertificateSource, DocInfo, DoneItemView, FieldInfo, InteractionView,
    KeystoreImportView, MainBarView, MigrationOfferView, NoticeView, OutputView, PlacementTarget, PlatformView, QrInfo,
    QueueItemView, ResumeOfferView, SaveDialogView, SettingsView, ShareHintView, SignersView, StepView, UiEvent,
    ViewState,
};
use crate::workspace::{Selection, WorkspaceRuntime};

/// Nachricht an den Kern-Task.
pub(crate) enum Msg {
    Ui(UiEvent),
    /// Ergebnis eines Effekts; `epoch` gesetzt = Signiervorgang (verworfen, wenn veraltet)
    Event {
        epoch: Option<u64>,
        event: Event,
    },
    Loaded(Box<Loaded>, Event),
    /// Ergebnis von „Automatisch platzieren“ für ein Dokument (verworfen, wenn inzwischen ein anderes
    /// Dokument positioniert wird)
    AutoPlaced(DocId, Placement),
    /// Blockvorschau gebaut (mit der Generation des Auftrags; ältere Ergebnisse werden verworfen)
    BlockBuilt(BlockInfo, u64),
    /// Blockvorschau nicht gebaut (Generation, i18n-ID)
    BlockFailed {
        generation: u64,
        code: String,
    },
    Notices(Vec<(String, BTreeMap<String, String>)>),
    Refresh,
    BackendEvent(BackendEvent),
    /// Belegungsraster einer Seite fertig (Arbeitsbereich, ADR 0004 §3.3; `None` = nicht berechenbar)
    GridReady {
        doc: DocId,
        page: usize,
        grid: Option<Arc<pida_placement::OccupancyGrid>>,
    },
    /// Voranalyse eines Stapeleintrags fertig (ADR 0004 §3.8) samt Miniatur
    Preanalyzed {
        doc: DocId,
        result: Result<pida_workflow::DocSummary, WorkflowError>,
        thumb: Option<Arc<[u8]>>,
    },
    /// Update-Prüfung fertig („Über PIDA“, ADR 0004 Nachtrag 21; Generation, Anlass, Releases bzw. Grund)
    UpdateChecked {
        generation: u64,
        kind: crate::update::CheckKind,
        result: Result<Vec<crate::update::Release>, &'static str>,
    },
    Shutdown,
}

/// Blockvorschau (aus dem echten Appearance-Objekt, Plan §6.6).
#[derive(Clone, Default)]
pub(crate) struct BlockInfo {
    pub doc: Option<DocId>,
    pub width: f32,
    pub height: f32,
    pub revision: u64,
    pub pdf: Option<Arc<[u8]>>,
    /// die Vorschau zeigt die neutrale Platzhalter-Bildmarke der Amtssignatur ([`plan::shows_amts_placeholder`])
    pub emblem_placeholder: bool,
}

impl std::fmt::Debug for BlockInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlockInfo")
            .field("doc", &self.doc)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("revision", &self.revision)
            .field("pdf_len", &self.pdf.as_ref().map(|p| p.len()))
            .finish()
    }
}

/// Schnappschuss der Konfiguration für Hintergrund-Tasks. `Debug` ohne Werte (Regel 9).
#[derive(Clone)]
pub(crate) struct Snapshot {
    pub settings: Settings,
    pub overlay: Overlay,
    pub policy: Option<Policy>,
}

impl std::fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Snapshot").field("overlay", &self.overlay).finish_non_exhaustive()
    }
}

/// Art eines UI-Ereignisses für Logs (ohne Pfade, Einstellungen oder Antworten, Regel 9).
fn ui_event_kind(e: &UiEvent) -> &'static str {
    match e {
        UiEvent::OpenFiles { .. } => "open_files",
        UiEvent::AnswerWarning { .. } => "answer_warning",
        UiEvent::PlaceholderAnswer { .. } => "placeholder_answer",
        UiEvent::MoveBlock { .. } => "move_block",
        UiEvent::NudgeBlock { .. } => "nudge_block",
        UiEvent::ChangePage { .. } => "change_page",
        UiEvent::ToggleNewPage => "toggle_new_page",
        UiEvent::AutoPlace => "auto_place",
        UiEvent::ConfirmPosition => "confirm_position",
        UiEvent::ChooseInvisible => "choose_invisible",
        UiEvent::ChooseBackend { .. } => "choose_backend",
        UiEvent::AcknowledgePrivacy { .. } => "acknowledge_privacy",
        UiEvent::SubmitForm { .. } => "submit_form",
        UiEvent::SubmitSecret { .. } => "submit_secret",
        UiEvent::AnswerNotice { .. } => "answer_notice",
        UiEvent::OpenSignatureData { .. } => "open_signature_data",
        UiEvent::Cancel => "cancel",
        UiEvent::Retry => "retry",
        UiEvent::Save { .. } => "save",
        UiEvent::ConfirmOverwrite { .. } => "confirm_overwrite",
        UiEvent::OpenResult => "open_result",
        UiEvent::ShareResult => "share_result",
        UiEvent::ExportResult => "export_result",
        UiEvent::Navigate { .. } => "navigate",
        UiEvent::OpenSettings => "open_settings",
        UiEvent::SaveSettings { .. } => "save_settings",
        UiEvent::CloseSettings => "close_settings",
        UiEvent::ResetSettings => "reset_settings",
        UiEvent::Backgrounded => "backgrounded",
        UiEvent::Foregrounded => "foregrounded",
        UiEvent::ResumeSignature => "resume_signature",
        UiEvent::DiscardResume => "discard_resume",
        UiEvent::ImportKeystore { .. } => "import_keystore",
        UiEvent::DismissKeystoreImport => "dismiss_keystore_import",
        UiEvent::Quit => "quit",
        UiEvent::DismissNotice { .. } => "dismiss_notice",
        UiEvent::AcceptMigration { .. } => "accept_migration",
        UiEvent::DeclineMigration => "decline_migration",
        // ADR 0004 (Vertrag E0)
        UiEvent::SetInvisible { .. } => "set_invisible",
        UiEvent::UseTarget { .. } => "use_target",
        UiEvent::InspectPages { .. } => "inspect_pages",
        UiEvent::SelectSigner { .. } => "select_signer",
        UiEvent::SignWith { .. } => "sign_with",
        UiEvent::ShowBatch => "show_batch",
        UiEvent::OpenBatchItem { .. } => "open_batch_item",
        UiEvent::BackToBatch => "back_to_batch",
        UiEvent::RemoveFromBatch { .. } => "remove_from_batch",
        UiEvent::StartBatch { .. } => "start_batch",
        UiEvent::SkipBatchDocument => "skip_batch_document",
        UiEvent::AbortBatch => "abort_batch",
        UiEvent::RemoveRecent { .. } => "remove_recent",
        UiEvent::ClearRecent => "clear_recent",
        UiEvent::ApplySettings { .. } => "apply_settings",
        UiEvent::SetUiLanguage { .. } => "set_ui_language",
        UiEvent::SetAccessibility { .. } => "set_accessibility",
        UiEvent::ExportQrPlaceholder { .. } => "export_qr_placeholder",
        UiEvent::UpdateFirstRun { .. } => "update_first_run",
        UiEvent::FinishFirstRun => "finish_first_run",
        UiEvent::OfferMigration => "offer_migration",
        UiEvent::InspectKeystore { .. } => "inspect_keystore",
        UiEvent::UnlockPlacement => "unlock_placement",
        UiEvent::AddBlock { .. } => "add_block",
        UiEvent::SelectBlock { .. } => "select_block",
        UiEvent::RemoveBlock { .. } => "remove_block",
        UiEvent::FillTargets => "fill_targets",
        UiEvent::SetBatchAutoPosition { .. } => "set_batch_auto_position",
        UiEvent::GoHome => "go_home",
        UiEvent::ResumeParked => "resume_parked",
        UiEvent::DiscardParked => "discard_parked",
        UiEvent::PostponeFirstRun => "postpone_first_run",
        UiEvent::FilesReceived { .. } => "files_received",
        UiEvent::DismissShareHint => "dismiss_share_hint",
        UiEvent::OpenRecent { .. } => "open_recent",
        UiEvent::RevealRecent { .. } => "reveal_recent",
        UiEvent::ShareRecent { .. } => "share_recent",
        UiEvent::SignRecent { .. } => "sign_recent",
        UiEvent::CheckForUpdates { .. } => "check_for_updates",
        UiEvent::DismissUpdateHint => "dismiss_update_hint",
        UiEvent::SetBlockLanguage { .. } => "set_block_language",
    }
}

impl Snapshot {
    fn effective(&self) -> Effective<'_> {
        Effective::new(&self.settings, &self.overlay, self.policy.as_ref())
    }
}

/// Zustand eines Signiervorgangs (zwischen den Effekten).
#[derive(Default)]
pub(crate) struct SignSlot {
    epoch: u64,
    cancel: CancelToken,
    caps: Option<Capabilities>,
    session: Option<Box<dyn BackendSession>>,
    chain: Option<CertChain>,
    prepared: Option<Box<dyn PreparedDoc>>,
    /// CMS-Profil des Plans (Modell A: die SignedAttributes müssen mit genau diesem Profil gebaut werden,
    /// auch wenn sich die Einstellungen zwischen `prepare` und `sign` ändern)
    cms_profile: Option<pida_cades::CmsProfile>,
    attrs: Option<SignedAttrs>,
    result: Option<SignResult>,
    signed: Option<Arc<dyn SignedDoc>>,
    /// Zertifikats-Cache (Opt-in) dieses Vorgangs: Cache, Schlüssel und ob `chain` aus dem Cache stammt
    cert_cache: Option<(CertificateCache, CertCacheKey, bool)>,
    /// Signiermittel der Sitzung (Wiederverwendung beim Wiederholen, ADR 0004 §3.6)
    backend: Option<BackendId>,
    /// Abbruch nur des laufenden Jobs (`sign_one`; „Dokument überspringen“ ab K2)
    job_cancel: CancelToken,
    /// Sitzung für „Erneut versuchen“ wiederverwendet: die Kette der Sitzung gilt weiter
    reused: bool,
    /// Herkunft des Zertifikats (Signierkarte)
    cert_source: CertificateSource,
    /// Seite des sichtbaren Blocks laut Plan (`Some(None)` = unsichtbar)
    block_page: Option<Option<usize>>,
    /// Zusammenfassung „Fertig“ (nach der Selbstprüfung)
    pub(crate) summary: Option<SummaryInfo>,
    /// Mehrfachsignatur: das zuletzt signierte Zwischenergebnis als Quelle der laufenden Runde (`None` = das
    /// Original); [`Effect::OpenSession`] setzt es zurück
    chain_doc: Option<Arc<DocData>>,
    /// Mehrfachsignatur: `signed` wird beim nächsten `prepare` zur Quelle ([`Effect::NextSignature`])
    chain_pending: bool,
}

impl SignSlot {
    /// Herkunft des Zertifikats für die Signierkarte.
    pub(crate) fn certificate_source(&self) -> CertificateSource {
        self.cert_source
    }
}

/// Angaben für `SignedSummaryView` (ADR 0004 §3.7). `Debug` ohne den Unterzeichner (Regel 9).
#[derive(Clone)]
pub(crate) struct SummaryInfo {
    /// CN des Signaturzertifikats
    pub signer: Option<String>,
    /// `/M` als RFC 3339
    pub signing_time: String,
    /// Seite des sichtbaren Blocks (`None` = unsichtbar)
    pub page: Option<usize>,
    /// Qualifikation laut qcStatements des Signerzertifikats (ADR 0004 §3.7/§3.9, K4): QcCompliance und
    /// QcSSCD → QES, sonst AdES
    pub qualification: pida_backend_api::Qualification,
}

impl std::fmt::Debug for SummaryInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SummaryInfo")
            .field("signer", &self.signer.as_ref().map(|_| "<redigiert>"))
            .field("signing_time", &self.signing_time)
            .field("page", &self.page)
            .field("qualification", &self.qualification)
            .finish()
    }
}

/// Qualifikation einer eben erstellten Signatur laut Zertifikat (qcStatements; nicht gegen die
/// Vertrauensliste geprüft, ADR 0004 §8 F3).
pub(crate) fn qualification_of(cert: &pida_cades::Certificate) -> pida_backend_api::Qualification {
    let qc = pida_cades::qc_statements(cert);
    if qc.compliance && qc.sscd { pida_backend_api::Qualification::Qes } else { pida_backend_api::Qualification::AdES }
}

/// Meldungs-ID: das Zertifikat aus dem Zertifikats-Cache passt nicht mehr (erneuert, anderes Konto);
/// der Eintrag ist gelöscht, „Wiederholen“ liest das Zertifikat neu und baut den Block neu.
pub(crate) const MSG_CERT_CACHE_MISMATCH: &str = "ng-certcache-mismatch";

/// Backends, für die der Zertifikats-Cache gilt: nur dort kostet das Lesen des Zertifikats eine eigene
/// Bestätigung (ID Austria, Plan §4.2 „Zwei SL-Requests“). Lokale BKU und Keystore lesen es ohne
/// Rückfrage.
fn uses_certificate_cache(id: BackendId) -> bool {
    matches!(id, BackendId::AtrustNative)
}

/// Fehlercodes der CMS-Prüfung, die ein anderes Signerzertifikat als das erwartete bedeuten.
fn is_signer_mismatch(code: &str) -> bool {
    matches!(code, "cms-unexpected-signer" | "cms-signing-certificate-mismatch")
}

/// Gemeinsame Daten (Kern-Task, Effekt-Tasks, Shell-Threads).
pub(crate) struct Shared {
    pub docs: DocStore,
    pub render: Option<Arc<dyn RenderBackend>>,
    pub engine: Arc<dyn PadesEngine>,
    pub profiles: Option<Arc<ProfileStore>>,
    pub fonts: Arc<FontProvider>,
    pub block: Mutex<BlockInfo>,
    pub broker: Arc<CoreBroker>,
    pub sign: Mutex<SignSlot>,
    pub tx: mpsc::UnboundedSender<Msg>,
    pub shell: Mutex<Option<Arc<dyn ShellHooks>>>,
    pub tile_scale: Mutex<f32>,
    /// Miniaturen der Stapeleinträge (PNG, `pida-render://thumb/<doc>`; ADR 0004 §3.8) – eigener kleiner
    /// Cache, nicht im LRU der Kacheln
    pub thumbs: Mutex<std::collections::HashMap<DocId, Arc<[u8]>>>,
}

impl std::fmt::Debug for Shared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shared")
            .field("docs", &self.docs)
            .field("render", &self.render.is_some())
            .finish_non_exhaustive()
    }
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Shared {
    pub(crate) fn send(&self, msg: Msg) {
        let _ = self.tx.send(msg);
    }

    pub(crate) fn shell(&self) -> Option<Arc<dyn ShellHooks>> {
        lock(&self.shell).clone()
    }

    /// Blockvorschau bauen (Profil, Bildmarke, Hinweis, Breite der angezeigten Seite).
    pub(crate) fn build_block(&self, snap: &Snapshot, doc: Option<&DocData>, page: usize) -> Result<BlockInfo, String> {
        let store = self.profiles.as_ref().ok_or("Profile nicht verfügbar")?;
        let emblem = snap.effective().emblem().and_then(plan::load_emblem);
        let spec = plan::block_spec(
            store,
            SpecInput {
                effective: snap.effective(),
                system_language: Lang::system(),
                pdfa: doc.and_then(|d| d.report.pdfa.as_ref()),
                cert: None,
                signing_time: plan::local_now(),
                emblem: emblem.as_ref(),
            },
        )
        .map_err(|e| e.code())?;
        let a4 = [plan::a4_page()];
        let pages: &[pida_pdf_cos::PageInfo] = doc.map(|d| d.pages.as_slice()).filter(|p| !p.is_empty()).unwrap_or(&a4);
        let geom = plan::block_geometry(&spec, pages, page, store, &self.fonts).map_err(|e| e.code())?;
        let pdf = if spec.visible() {
            let app = spec.build(geom.width, 0, store, &self.fonts).map_err(|e| e.code())?;
            app.to_standalone_pdf().ok().map(Arc::from)
        } else {
            None
        };
        let emblem_placeholder = plan::shows_amts_placeholder(&spec);
        Ok(BlockInfo {
            doc: doc.map(|d| d.id),
            width: geom.width,
            height: geom.height,
            revision: 0,
            pdf,
            emblem_placeholder,
        })
    }
}

/// Rückkanal des Brokers.
struct Sink(mpsc::UnboundedSender<Msg>);

impl BrokerSink for Sink {
    fn changed(&self) {
        let _ = self.0.send(Msg::Refresh);
    }
    fn backend_event(&self, event: BackendEvent) {
        let _ = self.0.send(Msg::BackendEvent(event));
    }
}

pub(crate) fn broker_sink(tx: mpsc::UnboundedSender<Msg>) -> Arc<dyn BrokerSink> {
    Arc::new(Sink(tx))
}

/// Plattform und Mobile-Optionen des Kerns (`CoreOptions::persist_prepared`, `default_output_dir`).
#[derive(Debug, Clone)]
pub(crate) struct MobileSetup {
    pub platform: Platform,
    /// vorbereiteten Zustand für „Signatur fortsetzen“ sichern (iOS/Android)
    pub persist_prepared: bool,
    /// Ausgabeordner ohne Einstellung (App-Dokumente auf Mobile)
    pub default_output_dir: Option<PathBuf>,
}

/// Meldungs-ID: Hinweis zu ID Austria auf Mobile – FIDO2 ist auf diesem Gerät nicht verfügbar (Plan
/// §4.7, §5.2); die Oberfläche zeigt FIDO2 ausgegraut mit diesem Text.
pub(crate) const MSG_FIDO2_UNAVAILABLE_MOBILE: &str = "ng-fido2-unavailable-mobile";
/// Meldungs-ID: PKCS#12 importiert.
pub(crate) const MSG_KEYSTORE_IMPORTED: &str = "ng-keystore-imported";
/// Meldungs-ID: Import gerade nicht möglich (Signiervorgang läuft).
pub(crate) const MSG_KEYSTORE_IMPORT_BUSY: &str = "ng-keystore-import-busy";
/// Meldungs-ID: Datei nicht lesbar bzw. zu groß.
pub(crate) const MSG_KEYSTORE_IMPORT_UNREADABLE: &str = "ng-keystore-import-unreadable";
/// Meldungs-ID: keine PKCS#12-Datei.
pub(crate) const MSG_KEYSTORE_IMPORT_FORMAT: &str = "ng-keystore-import-format";
/// Meldungs-ID: Kopie in die Sandbox fehlgeschlagen.
pub(crate) const MSG_KEYSTORE_IMPORT_FAILED: &str = "ng-keystore-import-failed";
/// Höchstgröße einer importierten PKCS#12-Datei (typisch wenige KiB).
pub(crate) const MAX_KEYSTORE_BYTES: u64 = 4 * 1024 * 1024;
/// Ordner der importierten Keystores unter `Paths::data_dir`.
pub(crate) const KEYSTORE_DIR: &str = "keystores";

/// Offener Dialog der Ausgabe (aus den Effekten `AskOverwrite`/`AskSavePath`).
#[derive(Debug, Clone, PartialEq)]
enum OutputDialog {
    Overwrite(PathBuf),
    Save(SaveDialogView),
}

/// Der Kern-Task.
pub(crate) struct CoreTask {
    pub shared: Arc<Shared>,
    pub state: State,
    pub ctx: Context,
    pub cfg: ConfigState,
    pub registry: crate::backends::Registry,
    pub headless: bool,
    pub view_tx: watch::Sender<ViewState>,
    pub finished_tx: watch::Sender<Option<ExitCode>>,
    pub startup: Option<StartupProblem>,
    pub fido2_available: bool,
    notices: Vec<NoticeView>,
    next_notice: u64,
    next_doc: u64,
    pub(crate) settings_view: Option<SettingsView>,
    pub(crate) options: Vec<BackendOption>,
    dialog: Option<OutputDialog>,
    queue: VecDeque<Event>,
    /// Import-Angebot mit „enthält Klartext-Passwörter“ (einmal je Altdatei ermittelt)
    migration_view: Option<MigrationOfferView>,
    /// Plattform und Mobile-Optionen
    pub(crate) mobile: MobileSetup,
    /// App im Hintergrund (Lebenszyklus; kein Abbruch)
    backgrounded: bool,
    /// gespeicherter, noch nicht angenommener Vorgang („Signatur fortsetzen“)
    resume_offer: Option<resume::Persisted>,
    /// beim Angebot ermittelt: Das Dokument verlässt mit dessen Signiermittel das Gerät (Kurzhinweis)
    resume_offer_leaves_device: bool,
    /// angenommener Vorgang: Dokument und Zustand für die Wiederherstellung beim nächsten `Prepare`
    resume_active: Option<(DocId, resume::Persisted)>,
    /// fortgesetztes Stapeldokument: die beim Start des Stapels gewählte Regel für vorhandene Zieldateien
    /// (gilt bis zur Ausgabe dieses Dokuments; `resume_active` ist dann schon verbraucht)
    resumed_on_existing: Option<(DocId, ExistingPolicy)>,
    /// der Ordner des persistierten Zustands gehört dem laufenden Vorgang (sonst hält er ein noch nicht
    /// angenommenes Angebot, das `DeletePersisted` eines anderen Vorgangs nicht löschen darf)
    persist_owned: bool,
    /// Ergebnis des PKCS#12-Imports
    keystore_import: Option<KeystoreImportView>,
    /// Oberfläche „Arbeitsbereich“ (ADR 0004)
    pub(crate) ws: WorkspaceRuntime,
    /// Stapelübersicht (`CoreOptions::batch_overview`, ADR 0004 §3.8)
    batch_overview: bool,
    /// Voranalyse der Stapeleinträge (nacheinander)
    preanalysis: crate::batch::Preanalysis,
    /// gecachte Abfragen der Projektion (Zieldateien, Ordner, gemerkte Passwörter, Zertifikats-Cache)
    pub(crate) probe: crate::batch::ProbeCache,
    /// Einstellungen „sofort wirksam“ (ADR 0004 §3.11)
    apply_state: ApplyState,
    /// Blockvorschau: Generation des letzten Auftrags bzw. des zuletzt übernommenen Ergebnisses
    block_requested: u64,
    block_done: u64,
    /// während `SettingsChanged` aus `ApplySettings`: ändert sich das Signaturbild? (`Some(false)` = keine
    /// Vorschau neu bauen)
    block_change: Option<bool>,
    /// „Zuletzt signiert“ (ADR 0004 §3.10)
    pub(crate) recent: crate::recent::RecentState,
    /// QR-Platzhaltersuchen (Einstellungen › Dokument, K3)
    qr_searches: QrSearches,
    /// Einstellungen › Keystore: Format, Einträge für die Alias-Auswahl (Nachtrag ADR 0004)
    pub(crate) keystore_info: crate::keystore_info::KeystoreInfo,
    /// „Über PIDA“ und Update-Prüfung (ADR 0004 Nachtrag 21)
    pub(crate) updates: crate::update::UpdateState,
}

/// QR-Platzhaltersuchen je Dokument (Einstellungen › Dokument, K3).
#[derive(Debug, Default)]
struct QrSearches {
    /// die Analyse hat gesucht (Ergebnis „keiner“ dem Reducer noch nicht gemeldet, falls er es braucht)
    analyzed: std::collections::HashSet<DocId>,
    /// nachträgliche Suche läuft
    requested: std::collections::HashSet<DocId>,
}

/// Zustand der sofort wirksamen Einstellungen (ADR 0004 §3.11).
#[derive(Debug, Default)]
struct ApplyState {
    /// Revision des zuletzt übernommenen `ApplySettings`
    applied: u64,
    /// in `SettingsView::applied_revision` veröffentlichte Revision (wartet auf eine neue Vorschau, wenn
    /// sich das Signaturbild geändert hat – dann steigen beide in derselben Projektion)
    published: u64,
    /// Generation der Blockvorschau, auf die `published` wartet
    awaiting_block: Option<u64>,
    /// abgelehnte Felder → i18n-ID (bleiben, bis das Feld erneut gesendet bzw. die Ansicht geschlossen wird)
    rejected: BTreeMap<String, String>,
    /// klassisches `SaveSettings` abgelehnt: die Ansicht zeigt die Arbeitskopie statt der Kerneinstellungen
    draft: bool,
}

impl CoreTask {
    pub(crate) fn new(
        shared: Arc<Shared>,
        cfg: ConfigState,
        registry: crate::backends::Registry,
        headless: bool,
        view_tx: watch::Sender<ViewState>,
        finished_tx: watch::Sender<Option<ExitCode>>,
        startup: Option<StartupProblem>,
        fido2_available: bool,
        mobile: MobileSetup,
        workspace_ui: bool,
        batch_overview: bool,
        updates: crate::update::UpdateState,
    ) -> CoreTask {
        let paths = cfg.paths.clone();
        let overlay = &cfg.overlay;
        // Startargument-Automatik der GUI (D-06): `-b` und (`-a` oder `-v`), nicht `signature.auto_position`
        let cli_auto = overlay.backend.is_some() && (overlay.auto_position || overlay.invisible == Some(true));
        let ws = WorkspaceRuntime { enabled: workspace_ui && !headless, cli_auto, ..WorkspaceRuntime::default() };
        let mut ctx = cfg.context(headless);
        ctx.workspace = ws.enabled;
        ctx.cli_auto = ws.enabled && cli_auto;
        // D-06 (`-b … -a`) bleibt ohne Übersicht (ADR 0004 §3.2, Abnahme E2 Punkt 9)
        ctx.batch_overview = ws.enabled && batch_overview && !cli_auto;
        CoreTask {
            shared,
            state: State::Starting,
            ctx,
            cfg,
            registry,
            headless,
            view_tx,
            finished_tx,
            startup,
            fido2_available,
            notices: Vec::new(),
            next_notice: 1,
            next_doc: 1,
            settings_view: None,
            options: Vec::new(),
            dialog: None,
            queue: VecDeque::new(),
            migration_view: None,
            mobile,
            backgrounded: false,
            resume_offer: None,
            resume_offer_leaves_device: false,
            resume_active: None,
            resumed_on_existing: None,
            persist_owned: false,
            keystore_import: None,
            ws,
            batch_overview,
            preanalysis: crate::batch::Preanalysis::default(),
            probe: crate::batch::ProbeCache::default(),
            apply_state: ApplyState::default(),
            block_requested: 0,
            block_done: 0,
            block_change: None,
            recent: crate::recent::RecentState::new(&paths),
            qr_searches: QrSearches::default(),
            keystore_info: crate::keystore_info::KeystoreInfo::default(),
            updates,
        }
    }

    /// [`Self::refresh_ctx`] für die Module des Kern-Tasks.
    pub(crate) fn refresh_ctx_pub(&mut self) {
        self.refresh_ctx();
    }

    /// Reducer-Kontext aus den Einstellungen neu bilden (mit dem Arbeitsbereich-Modus).
    fn refresh_ctx(&mut self) {
        self.ctx = self.cfg.context(self.headless);
        self.ctx.workspace = self.ws.enabled;
        self.ctx.cli_auto = self.ws.enabled && self.ws.cli_auto;
        self.ctx.batch_overview = self.ws.enabled && self.batch_overview && !self.ws.cli_auto;
        // Einstellungen geändert: Ausgabeordner, Keystore-Passwort, Zertifikats-Cache neu abfragen
        self.probe.clear();
    }

    /// Folgeereignis eines Effekts in Reihenfolge (nach dem laufenden Reducer-Schritt).
    pub(crate) fn apply_later(&mut self, event: Event) {
        self.queue.push_back(event);
    }

    /// Ordner des persistierten Signierzustands (App-Sandbox, nicht der Cache).
    fn resume_dir(&self) -> PathBuf {
        self.cfg.paths.temp_dir.join(resume::RESUME_DIR)
    }

    /// Beim Start: unterbrochenen Vorgang suchen und „Signatur fortsetzen“ anbieten (Mobile).
    fn load_resume_offer(&mut self) {
        if !self.mobile.persist_prepared {
            return;
        }
        let dir = self.resume_dir();
        match resume::load(&dir, pida_fs::system_now()) {
            resume::Loaded::Nothing => {}
            // Mobile nur mit ID Austria (ADR 0004 Nachtrag 11): einen Keystore-Vorgang eines früheren Builds gibt
            // es nicht fortzusetzen – verwerfen statt anbieten (das Original bleibt, wo es war)
            resume::Loaded::Offer(p) if !self.cfg.keystore_offered && p.record.backend == BackendId::Keystore => {
                tracing::info!("unterbrochene Keystore-Signatur eines früheren Builds – verworfen (nur ID Austria)");
                resume::delete(&dir);
                self.notify(crate::mobile_signers::MSG_RESUME_SIGNER_NOT_OFFERED, BTreeMap::new());
            }
            resume::Loaded::Offer(p) => {
                tracing::info!("unterbrochener Signiervorgang gefunden – „Signatur fortsetzen“ wird angeboten");
                // Kurzhinweis beim Angebot (ADR 0003): Capability des gespeicherten Signiermittels
                let env = self.registry.environment(self.shared.broker.interactive(), &self.cfg);
                self.resume_offer_leaves_device = self
                    .registry
                    .get(p.record.backend)
                    .and_then(|b| b.capabilities(&env))
                    .is_some_and(|c| c.document_leaves_device);
                self.resume_offer = Some(*p);
            }
            resume::Loaded::Expired => {
                tracing::info!("unterbrochener Signiervorgang zu alt – verworfen");
                resume::delete(&dir);
                self.notify(resume::MSG_EXPIRED, BTreeMap::new());
            }
            resume::Loaded::Invalid(reason) => {
                tracing::warn!(reason = %reason, "persistierter Signierzustand ungültig – verworfen");
                resume::delete(&dir);
            }
        }
    }

    /// `Effect::DeletePersisted`: nur den Zustand des laufenden Vorgangs löschen.
    fn delete_persisted(&mut self) {
        self.resume_active = None;
        if self.mobile.persist_prepared && self.persist_owned {
            resume::delete(&self.resume_dir());
            self.persist_owned = false;
        }
    }

    /// „Signatur fortsetzen“ angenommen: Dokument (Sandbox-Kopie) mit Position und Signiermittel an den
    /// Workflow; das vorbereitete Dokument wird beim nächsten `Prepare` wiederhergestellt.
    fn resume_signature(&mut self) {
        if !matches!(self.state, State::Idle { .. }) {
            tracing::debug!("„Signatur fortsetzen“ nur im Auswahl-Screen");
            return;
        }
        let Some(persisted) = self.resume_offer.take() else { return };
        let document = persisted.document();
        if !document.fs_is_file() {
            resume::delete(&persisted.dir);
            self.notify(resume::MSG_RESTORE_FAILED, BTreeMap::new());
            return;
        }
        let id = DocId(self.next_doc);
        self.next_doc += 1;
        let input = InputDocument { id, path: document, display_name: persisted.record.name.clone() };
        let pending = self.input_documents(&persisted.existing_pending());
        let plan = ResumePlan { position: persisted.record.position.clone(), backend: persisted.record.backend };
        tracing::info!(backend = plan.backend.as_str(), pending = pending.len(), "Signatur wird fortgesetzt");
        // Stapel: „Beide behalten“ usw. aus dem Startdialog gilt weiter (sonst fragte die Ausgabe „Datei
        // überschreiben?“ – Emulator-Abnahme 16.10.2026)
        self.resumed_on_existing = persisted.record.on_existing.map(|p| (id, p));
        self.persist_owned = true;
        self.resume_active = Some((id, persisted));
        self.apply(Event::ResumeRequested { document: input, plan, pending });
    }

    /// „Signatur fortsetzen“ verworfen.
    fn discard_resume(&mut self) {
        if let Some(p) = self.resume_offer.take() {
            tracing::info!("unterbrochener Signiervorgang verworfen");
            resume::delete(&p.dir);
        }
    }

    /// Mobile: Pfade importierter Dateien nach einem Umzug der Sandbox neu auflösen.
    ///
    /// Die Einstellungen speichern absolute Pfade (`signature.emblem`). iOS vergibt bei jeder Installation
    /// bzw. jedem Update einen neuen Container (`…/Application/<UUID>/`), die Dateien ziehen mit um – der
    /// gespeicherte Pfad zeigte danach ins Leere. Liegt die Datei unter demselben Namen im Ordner der
    /// importierten Dateien (`data_dir/emblem`), wird der Pfad umgestellt. Andere Pfade bleiben unangetastet –
    /// auch `[keystore] file` eines früheren Mobile-Builds: auf iOS/Android wird nur mit ID Austria signiert,
    /// die Keystore-Werte werden nur ignoriert (ADR 0004 Nachtrag 11).
    fn relocate_sandbox_files(&mut self) {
        if self.mobile.platform.is_desktop() {
            return;
        }
        if let Some(old) = self.cfg.settings.signature.emblem.clone()
            && let Some(new) = relocated(&old, &self.cfg.paths.emblem_dir())
        {
            tracing::info!("Bildmarke nach Umzug der Sandbox neu zugeordnet");
            self.cfg.settings.signature.emblem = Some(new);
            if let Err(e) = self.cfg.save() {
                tracing::warn!(id = e.message_id(), "Einstellungen nach dem Umzug der Sandbox nicht gespeichert");
            }
        }
    }

    /// Lebenszyklus „Vordergrund“: Wartet ID Austria auf die Bestätigung in der App bzw. einen QR-Scan,
    /// wird der Long-Poll sofort neu gestartet (Seite neu laden, R7) – der Wechsel ist kein Abbruch.
    fn foregrounded(&mut self) {
        let was_background = std::mem::replace(&mut self.backgrounded, false);
        self.apply(Event::Foregrounded);
        self.refresh_signers();
        if was_background
            && matches!(self.state, State::Signing { .. } | State::BatchSigning { .. })
            && self.shared.broker.restart_long_poll()
        {
            tracing::info!("nach der Rückkehr in die App: Long-Poll neu gestartet");
        }
    }

    /// PKCS#12-Import in die App-Sandbox (Mobile, Plan §5.2): Datei kopieren, Passwort prüfen,
    /// Einstellungen › Keystore setzen, Passwort optional im sicheren Speicher.
    fn import_keystore(&mut self, path: &Path, password: Option<zeroize::Zeroizing<String>>, remember: bool) {
        let file_name =
            path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "keystore.p12".into());
        let result = self.import_keystore_inner(path, &file_name, password, remember);
        let (ok, code, password_stored) = match result {
            Ok(stored) => (true, MSG_KEYSTORE_IMPORTED.to_owned(), stored),
            Err(code) => (false, code, false),
        };
        tracing::info!(ok, code = %code, "PKCS#12-Import");
        self.keystore_import = Some(KeystoreImportView { file_name, ok, code, password_stored });
    }

    fn import_keystore_inner(
        &mut self,
        path: &Path,
        file_name: &str,
        password: Option<zeroize::Zeroizing<String>>,
        remember: bool,
    ) -> Result<bool, String> {
        // nicht während eines Signiervorgangs (Einstellungen dürfen dort nicht wechseln); in der
        // Signiermittel-Auswahl ja – danach ist der Keystore dort sofort verfügbar
        let busy = matches!(
            self.state,
            State::Signing { .. }
                | State::BatchSigning { .. }
                | State::Resuming { .. }
                | State::Starting
                | State::Finished { .. }
        );
        if busy {
            return Err(MSG_KEYSTORE_IMPORT_BUSY.into());
        }
        let len = pida_fs::metadata(path).map(|m| m.len()).map_err(|_| MSG_KEYSTORE_IMPORT_UNREADABLE.to_owned())?;
        if len == 0 || len > MAX_KEYSTORE_BYTES {
            return Err(MSG_KEYSTORE_IMPORT_UNREADABLE.into());
        }
        let bytes = pida_fs::read(path).map_err(|_| MSG_KEYSTORE_IMPORT_UNREADABLE.to_owned())?;
        match pida_backend_keystore::detect_format(&bytes) {
            pida_backend_keystore::KeystoreFormat::Pkcs12 => {}
            pida_backend_keystore::KeystoreFormat::Jks | pida_backend_keystore::KeystoreFormat::Jceks => {
                return Err("keystore-conversion-needed".into());
            }
            pida_backend_keystore::KeystoreFormat::Unknown => return Err(MSG_KEYSTORE_IMPORT_FORMAT.into()),
        }
        // Passwort prüfen (falls angegeben); bei genau einem Schlüssel wird dessen Alias übernommen
        let mut alias = None;
        let mut entries = None;
        if let Some(pw) = &password {
            let list = pida_backend_keystore::list_aliases(&bytes, pw).map_err(|e| e.code().to_owned())?;
            let mut keys = list.iter().filter(|e| e.is_key_entry);
            match (keys.next(), keys.next()) {
                (None, _) => return Err("keystore-no-key-entry".into()),
                (Some(only), None) => alias = Some(only.alias.clone()),
                _ => {}
            }
            // für die Alias-Auswahl in den Einstellungen (ohne erneute Passworteingabe)
            entries = Some(list);
        }
        // in die Sandbox kopieren (Ordner 0700, Datei 0600, atomar)
        let dir = self.cfg.paths.data_dir.join(KEYSTORE_DIR);
        let target = dir.join(sanitize_file_name(file_name, "keystore.p12"));
        write_private_file(&dir, &target, &bytes).map_err(|_| MSG_KEYSTORE_IMPORT_FAILED.to_owned())?;
        // vorher importierte Datei (anderer Name) samt gespeicherter Passwörter entfernen
        if let Some(old) = self.cfg.settings.keystore.file.clone()
            && old != target
            && old.parent() == Some(dir.as_path())
        {
            let _ = pida_fs::remove_file(&old);
            let _ = self.cfg.secrets.delete(&pida_config::SecretId::keystore_store(&old));
            let old_alias = self.cfg.settings.keystore.alias.clone().unwrap_or_default();
            let _ = self.cfg.secrets.delete(&pida_config::SecretId::keystore_key(&old, &old_alias));
        }
        let mut stored = false;
        if let (true, Some(pw)) = (remember, &password)
            && self.cfg.secrets.is_persistent()
        {
            let a = alias.clone().unwrap_or_default();
            let secrets = [
                (pida_config::SecretId::keystore_store(&target), pw.clone()),
                (pida_config::SecretId::keystore_key(&target, &a), pw.clone()),
            ];
            match pida_config::secrets::store_all(self.cfg.secrets.as_ref(), &secrets) {
                Ok(_) => stored = true,
                Err(e) => tracing::warn!(id = e.message_id(), "Keystore-Passwort nicht im sicheren Speicher"),
            }
        }
        let ks = &mut self.cfg.settings.keystore;
        ks.enabled = true;
        ks.file = Some(target.clone());
        ks.declared_type = Some("PKCS12".into());
        ks.alias = alias;
        if stored {
            ks.password_storage = pida_config::PasswordStorage::Keychain;
            // „Passwort sicher speichern“ ist eine ausdrückliche Wahl (keine Rückfrage beim ersten Signieren)
            ks.storage_chosen = true;
        } else if ks.password_storage == pida_config::PasswordStorage::Keychain {
            ks.password_storage = pida_config::PasswordStorage::Memory;
        }
        if let Some(list) = entries {
            self.keystore_info.adopt(&target, list);
        }
        if let Err(e) = self.cfg.save() {
            tracing::warn!(id = e.message_id(), "Einstellungen nach dem Import nicht gespeichert");
            self.notify("error-FailedToSaveSettings", BTreeMap::new());
        }
        if self.settings_view.is_some() {
            self.settings_view = Some(self.settings_view_for(self.cfg.settings.clone(), BTreeMap::new()));
        }
        self.refresh_ctx();
        self.registry.configure(&self.cfg);
        self.refresh_signers();
        self.apply(Event::SettingsChanged);
        // Die Kopie aus der Dateiauswahl (Shell, `mobile_pick_files` → Cache) ist jetzt überzählig: ein
        // zweites Exemplar des privaten Schlüssels bleibt nicht im Cache liegen.
        remove_cache_copy(path, &target, &self.cfg.paths.cache_dir);
        Ok(stored)
    }

    /// Import-Angebot für die Ansicht (Klartext-Passwörter einmal je Altdatei ermitteln).
    fn refresh_migration_view(&mut self) {
        let Some(file) = self.cfg.migration_offer.clone() else {
            self.migration_view = None;
            return;
        };
        if self.migration_view.as_ref().is_some_and(|m| m.legacy_file == file) {
            return;
        }
        let legacy_dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
        // eigene Panik-Grenze: Diese Prüfung läuft nach jedem Ereignis (`guarded`); ein Programmfehler
        // beim Lesen der Altdatei darf das gerade verarbeitete Ereignis nicht verwerfen und sich nicht
        // bei jedem weiteren Ereignis wiederholen (dann: ohne Rückfrage zu Passwörtern). Probelauf wie beim
        // Übernehmen (mit Pfadprüfung), nichts wird geschrieben; daraus auch die Übernahmeliste (§3.12).
        let keystore_offered = self.cfg.keystore_offered;
        let probe = std::panic::catch_unwind(|| {
            pida_config::LegacyConfig::load(&file)
                .map(|legacy| {
                    let ctx = pida_config::MigrationContext {
                        legacy_dir,
                        system_language: Lang::system(),
                        check_paths: true,
                    };
                    let mut r = pida_config::migrate(&legacy, &ctx);
                    // iOS/Android nur mit ID Austria (ADR 0004 Nachtrag 11): wie beim Übernehmen ohne Keystore;
                    // die Liste erklärt das (`ng-migration-item-signer-mobile`)
                    let dropped =
                        !keystore_offered && crate::mobile_signers::restrict_migration(&mut r, Lang::system());
                    let mut items = crate::first_run::migration_items(&r);
                    if dropped {
                        items.push(crate::view::MigrationItemView {
                            code: crate::mobile_signers::MSG_MIGRATION_ITEM_SIGNER.to_owned(),
                            detail: None,
                        });
                    }
                    (!r.report.secrets_found.is_empty(), items)
                })
                .unwrap_or((false, Vec::new()))
        });
        let (plaintext_secrets, items) = probe.unwrap_or_else(|_| {
            tracing::error!("Altkonfiguration nicht auswertbar (Programmfehler) – Angebot ohne Passwortfrage");
            (false, Vec::new())
        });
        self.migration_view = Some(MigrationOfferView { legacy_file: file, plaintext_secrets, items });
    }

    /// Übernahmeangebot neu auswerten (erneutes Angebot über Einstellungen › Erweitert).
    pub(crate) fn invalidate_migration_view(&mut self) {
        self.migration_view = None;
    }

    /// `ViewState::migration` (nur das aktuelle Angebot).
    pub(crate) fn projected_migration(&self) -> Option<MigrationOfferView> {
        self.migration_view.clone().filter(|m| self.cfg.migration_offer.as_ref() == Some(&m.legacy_file))
    }

    /// Hauptschleife.
    pub(crate) async fn run(mut self, mut rx: mpsc::UnboundedReceiver<Msg>) {
        self.guarded(|t| t.start());
        self.guarded(|t| t.after_message());
        self.publish();
        // Backend-Auswahl aktuell halten (lokale BKU erscheint, sobald sie erreichbar ist, F-51)
        let mut tick = crate::rt::Ticker::new(std::time::Duration::from_secs(5));
        loop {
            let msg = tokio::select! {
                m = rx.recv() => m,
                _ = tick.tick() => {
                    self.guarded(|t| t.refresh_backend_options());
                    self.guarded(|t| t.refresh_recent_exists(false));
                    // Konto-Chip aktuell halten (lokale BKU erscheint bzw. verschwindet), nicht im Signieren
                    if !matches!(self.state, State::Signing { .. } | State::BatchSigning { .. } | State::Resuming { .. }) {
                        self.guarded(|t| t.refresh_signers());
                    }
                    self.publish();
                    continue;
                }
            };
            let Some(msg) = msg else { break };
            if matches!(msg, Msg::Shutdown) {
                break;
            }
            self.guarded(|t| t.handle(msg));
            self.guarded(|t| t.after_message());
            self.publish();
        }
        self.cancel_session();
    }

    /// Nach jeder Nachricht (Arbeitsbereich): Revision der Platzierung, Belegungsraster anfordern, QR-Suche
    /// nachholen; Stapel: nächste Voranalyse, Miniaturen entfernter Einträge verwerfen; „Zuletzt signiert“:
    /// Dateien noch da?
    fn after_message(&mut self) {
        self.ensure_qr_search();
        self.track_revision();
        self.request_layout();
        self.pump_preanalysis();
        self.prune_thumbnails();
        self.refresh_recent_exists(false);
        self.refresh_keystore_info();
        self.sync_update_setting();
    }

    /// Nächste Voranalyse beginnen (eine zur Zeit; verworfene Einträge entfallen).
    fn pump_preanalysis(&mut self) {
        let state = &self.state;
        let wanted = |doc: DocId| {
            state.queue().and_then(|q| q.batch.as_ref()).and_then(|b| b.item(doc)).is_some_and(|i| i.summary.is_none())
        };
        let Some(doc) = self.preanalysis.next(wanted) else { return };
        let Some(input) = self.state.queue().and_then(|q| q.find(doc)).cloned() else {
            self.preanalysis.finished(doc);
            return;
        };
        // Ein signierter Eintrag, dessen Eingabe es nicht mehr gibt – „Signatur fortsetzen“ signiert die Kopie im
        // Ordner des gesicherten Zustands, die nach dem Erfolg entfällt –, zeigt Miniatur, Seiten und Größe der
        // signierten Datei (Mobile-Check 16.10.2026: die fortgesetzte Zeile blieb ohne sie)
        let signed =
            self.state.queue().and_then(|q| q.batch.as_ref()).and_then(|b| b.item(doc)).and_then(|i| match &i.status {
                pida_workflow::BatchItemStatus::Signed { output } => Some(output.path.clone()),
                _ => None,
            });
        let path = match signed {
            Some(output) if !input.path.fs_is_file() => output,
            _ => input.path.clone(),
        };
        let shared = self.shared.clone();
        let snap = self.snapshot();
        let search_qr = self.ctx.placeholder_search && !self.ctx.placeholder_fields;
        crate::rt::spawn_blocking(move || {
            let mark =
                |data: &DocData, fields: &mut [EmptyFieldCandidate]| mark_auto_fields(&shared, &snap, data, fields);
            let (result, thumb) = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::batch::preanalyze(doc, &path, shared.engine.as_ref(), search_qr, shared.render.as_deref(), &mark)
            })) {
                Ok(r) => r,
                Err(_) => {
                    tracing::error!("Voranalyse abgebrochen (Programmfehler)");
                    (Err(WorkflowError::Unexpected("Voranalyse".into())), None)
                }
            };
            shared.send(Msg::Preanalyzed { doc, result, thumb });
        });
    }

    /// Miniaturen nur für Einträge des Stapels behalten.
    fn prune_thumbnails(&self) {
        let mut thumbs = lock(&self.shared.thumbs);
        if thumbs.is_empty() {
            return;
        }
        let batch = self.state.queue().and_then(|q| q.batch.as_ref());
        thumbs.retain(|doc, _| batch.is_some_and(|b| b.item(*doc).is_some()));
    }

    /// Führt `f` aus; eine Panik (Programmfehler, feindliche Eingabe in einer Bibliothek) beendet nicht
    /// den Kern-Task (die UI hinge sonst ohne Rückmeldung), sondern wird wie in PDF-Over zu
    /// `error.Unexpected` (F-106): mit Oberfläche Fehleranzeige und weiter mit der Dokumentauswahl,
    /// ohne Oberfläche Programmende mit Exit-Code 1.
    fn guarded(&mut self, f: impl FnOnce(&mut CoreTask)) {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            f(self);
            self.refresh_migration_view();
        }));
        if result.is_err() {
            self.recover_from_panic();
        }
    }

    fn recover_from_panic(&mut self) {
        // kein Paniktext im Log: er könnte Pfade oder Werte enthalten (Regel 9)
        tracing::error!(state = self.state.name(), "unerwarteter Fehler im Kern – Vorgang verworfen");
        self.queue.clear();
        self.dialog = None;
        self.settings_view = None;
        // eine abgebrochene Übernahme (`apply_settings`) darf spätere Vorschauen nicht unterdrücken
        self.block_change = None;
        self.cancel_session();
        let error = WorkflowError::Unexpected("Programmfehler".into());
        let mut queue = self.state.queue().cloned().unwrap_or_default();
        for d in queue.current.iter().chain(queue.pending.iter()) {
            self.shared.docs.remove(d.id);
        }
        if queue.current.is_some() {
            queue.finish_current(pida_workflow::DocResult::Failed { error: error.clone() });
        }
        queue.clear();
        if self.headless {
            self.state = State::Finished { exit: ExitCode::General, results: queue.done };
            self.publish();
            let _ = self.finished_tx.send(Some(ExitCode::General));
        } else {
            self.state = State::Error { queue, error, retry: None, from: None };
        }
    }

    /// Eine Nachricht verarbeiten.
    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Shutdown => {}
            Msg::Ui(e) => self.on_ui(e),
            Msg::Event { epoch, event } => {
                if epoch.is_none_or(|e| e == lock(&self.shared.sign).epoch) {
                    // Arbeitsbereich: ein `Cancelled`, das core-api nicht selbst ausgelöst hat (SL 6000/6001,
                    // „Abbrechen“ in einer A-Trust-Meldung), ist eine Ablehnung (ADR 0004 §3.6)
                    let declined = self.ws.enabled
                        && epoch.is_some()
                        && event == Event::BackendFailed(BackendError::Cancelled)
                        && !self.ws.own_cancel
                        && matches!(self.state, State::Signing { .. } | State::BatchSigning { .. });
                    if declined {
                        tracing::info!("Signieren abgelehnt bzw. abgelaufen (nicht in PIDA abgebrochen)");
                        self.apply(Event::SigningDeclined);
                    } else {
                        self.apply(event);
                    }
                } else {
                    tracing::debug!("veraltetes Ergebnis eines abgebrochenen Vorgangs verworfen");
                }
            }
            Msg::Loaded(loaded, event) => self.on_loaded(*loaded, event),
            Msg::AutoPlaced(doc, placement) => {
                if matches!(&self.state, State::Positioning { doc: facts, .. } if facts.id == doc) {
                    // Arbeitsbereich: Herkunft „automatisch“ (verspätet verworfen, wenn inzwischen bewegt)
                    let event =
                        if self.ws.enabled { Event::AutoPlaced(placement) } else { Event::PlacementChanged(placement) };
                    self.apply(event);
                }
            }
            Msg::BlockBuilt(info, generation) => self.on_block(info, generation),
            Msg::BlockFailed { generation, code } => {
                self.notify(&code, BTreeMap::new());
                self.block_settled(generation);
            }
            Msg::Notices(list) => {
                for (code, params) in list {
                    self.notify(&code, params);
                }
            }
            Msg::Refresh => self.on_interaction_changed(),
            Msg::BackendEvent(e) => self.on_backend_event(e),
            Msg::GridReady { doc, page, grid } => self.on_grid_ready(doc, page, grid),
            Msg::UpdateChecked { generation, kind, result } => self.on_update_checked(generation, kind, result),
            Msg::Preanalyzed { doc, result, thumb } => {
                self.preanalysis.finished(doc);
                let known = self.state.queue().and_then(|q| q.batch.as_ref()).is_some_and(|b| b.item(doc).is_some());
                if known {
                    if let Some(png) = thumb {
                        lock(&self.shared.thumbs).insert(doc, png);
                    }
                    self.apply(Event::Preanalyzed { doc, result });
                }
            }
        }
    }

    /// Ergebnis der Analyse übernehmen – nur, wenn das Dokument noch analysiert wird. Wurde es inzwischen
    /// verworfen (Hauptleiste „Öffnen“, Abbrechen, Beenden), bleiben Dokument, Renderer und Hinweise
    /// nicht im Speicher zurück.
    fn on_loaded(&mut self, loaded: Loaded, event: Event) {
        let id = loaded.data.id;
        let current = match &self.state {
            State::Analyzing { queue } | State::Resuming { queue, doc: None, .. } => queue.current_id() == Some(id),
            // Stapellauf: Dokument wird geladen
            State::BatchSigning { queue, run } => {
                run.phase == pida_workflow::BatchPhase::Loading && queue.current_id() == Some(id)
            }
            _ => false,
        };
        if !current {
            tracing::debug!("Analyse eines verworfenen Dokuments verworfen");
            return;
        }
        // Warnungen der Analyse: mit Oberfläche als Rückfrage (State::AnalysisWarning), ohne als Hinweis
        if self.headless {
            for w in &loaded.warnings {
                self.notify(w, BTreeMap::new());
            }
        }
        self.shared.docs.insert(loaded.data.clone());
        if loaded.qr_searched {
            self.qr_searches.analyzed.insert(id);
        }
        self.apply(event);
    }

    /// Arbeitsbereich (Einstellungen › Dokument, K3): ist das Ergebnis der QR-Suche des positionierten
    /// Dokuments unbekannt (`WorkspaceEdit::qr_search == None`) und hat die Analyse schon gesucht, erfährt der
    /// Reducer „gesucht, keiner gefunden“ (ein späteres Umschalten braucht dann keine neue Suche); sonst wird
    /// die Suche nachgeholt, sobald „QR-Code-Platzhalter“ eingestellt ist – auch wenn die Suche erst während
    /// der Analyse bzw. der Analysewarnung eingeschaltet wurde.
    fn ensure_qr_search(&mut self) {
        let id = match &self.state {
            State::Positioning { doc, edit: Some(e), .. } if e.qr_search.is_none() => doc.id,
            _ => return,
        };
        if self.qr_searches.analyzed.remove(&id) {
            self.apply(Event::PlaceholderSearched { doc: id, qr: None });
            return;
        }
        if self.ctx.placeholder_search && !self.ctx.placeholder_fields && !self.ctx.invisible {
            self.effect_search_placeholder(id);
        }
    }

    /// In der Backend-Auswahl die Verfügbarkeit neu ermitteln (ohne automatische Wahl).
    fn refresh_backend_options(&mut self) {
        if !matches!(self.state, State::ChoosingBackend { auto_choice: false, .. }) {
            return;
        }
        let env = self.registry.environment(self.shared.broker.interactive(), &self.cfg);
        let options = self.registry.options(&env);
        if options != self.options {
            self.options = options.clone();
            self.apply(Event::BackendsProbed(options));
        }
    }

    /// Start: Einstellungen unlesbar → Rückfrage (F-05), sonst Dokumente aus dem Overlay.
    fn start(&mut self) {
        // „Zuletzt signiert“ laden bzw. – abgeschaltet – sofort löschen (ADR 0004 §3.10)
        self.sync_recent();
        if let Some(StartupProblem::Unreadable(e)) = self.startup.take() {
            if self.headless {
                // ohne Oberfläche gibt es keine Rückfrage (Werksreset): Grund als Hinweis, dann Ende (Exit 1)
                let params = e
                    .message_args()
                    .into_iter()
                    .map(|(k, v)| {
                        let v = match v {
                            pida_i18n::ArgValue::Str(s) => s,
                            pida_i18n::ArgValue::Num(n) => n.to_string(),
                        };
                        (k.to_owned(), v)
                    })
                    .collect();
                self.notify(e.message_id(), params);
            }
            self.apply(Event::SettingsUnreadable(e.localized(self.cfg.language())));
            return;
        }
        // Ersteinrichtung bei Neuinstallation (ADR 0004 §3.12)
        self.begin_first_run();
        self.relocate_sandbox_files();
        // Mobile nur mit ID Austria (ADR 0004 Nachtrag 11): Keystore-Passwörter früherer Builds einmalig entfernen,
        // danach deren Dateien im App-Speicher (Nachtrag 22)
        self.remove_former_keystore_secrets();
        self.remove_former_keystore_files();
        self.registry.configure(&self.cfg);
        self.refresh_signers();
        // Schlüsselbund für Keystore-Passwörter gewünscht, aber nicht verfügbar (z. B. Linux ohne Secret
        // Service): Hinweis – nur dort, wo es den Keystore gibt
        if self.cfg.keystore_offered
            && self.cfg.settings.keystore.password_storage == pida_config::PasswordStorage::Keychain
            && !self.cfg.secrets.is_persistent()
        {
            self.notify("ng-secrets-not-persistent", BTreeMap::new());
        }
        self.load_resume_offer();
        let docs = self.input_documents(&self.cfg.overlay.input_files.clone());
        self.apply(Event::Started { documents: docs });
        // Update-Prüfung im Hintergrund (höchstens einmal je 24 h, nie blockierend; ADR 0004 Nachtrag 21)
        self.update_check_on_startup();
        // Schritt 3 der Ersteinrichtung zeigt die Vorschau des Signaturbilds (ohne Dokument: A4-Beispiel)
        if self.first_run_active() && lock(&self.shared.block).pdf.is_none() {
            let doc = self.preview_doc();
            self.effect_block(doc);
        }
    }

    pub(crate) fn input_documents(&mut self, paths: &[PathBuf]) -> Vec<InputDocument> {
        let mut out = Vec::new();
        for p in paths {
            // F-12: nur vorhandene Dateien
            if !p.fs_is_file() {
                tracing::info!("Eingabe übersprungen (keine Datei)");
                continue;
            }
            let id = DocId(self.next_doc);
            self.next_doc += 1;
            out.push(InputDocument::new(id, p.clone()));
        }
        out
    }

    /// `FilesReceived` (Shell, Mobile: „Teilen“ bzw. „Öffnen in“; ADR 0004 Nachtrag 12): wie `OpenFiles` ohne
    /// `mode`. Auf iOS übergibt die Dateien-App von mehreren geteilten PDFs nur das erste – für das empfangene
    /// Dokument danach der Hinweis zum Teilen (`ViewState::share_hint`), sofern er in diesem Lauf nicht
    /// ausgeblendet wurde. Android empfängt mehrere (`SEND_MULTIPLE`), der Desktop „Öffnen mit“: kein Hinweis.
    fn files_received(&mut self, paths: &[PathBuf]) {
        let ids = self.open_selected(paths, None).1;
        if self.ws.enabled
            && self.mobile.platform == Platform::Ios
            && !self.ws.share_hint_dismissed
            && let Some(first) = ids.first()
        {
            self.ws.share_hint = Some(*first);
        }
    }

    /// Hinweis zum Teilen (Nachtrag 12): nur, solange das empfangene Dokument allein im Arbeitsbereich offen ist –
    /// kein Stapel, nichts wartet (mit „Weitere PDFs hinzufügen …“ ist der Hinweis erledigt).
    fn share_hint_view(
        &self,
        document: Option<&crate::view::DocumentView>,
        batch: Option<&crate::view::BatchView>,
    ) -> Option<ShareHintView> {
        let doc = self.ws.share_hint?;
        let alone = batch.is_none()
            && document.is_some_and(|d| d.doc.id == doc)
            && self.state.queue().is_some_and(|q| q.pending.is_empty());
        alone.then_some(ShareHintView { doc })
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            settings: self.cfg.settings.clone(),
            overlay: self.cfg.overlay.clone(),
            policy: self.cfg.policy.clone(),
        }
    }

    /// Hinweis ohne Parameter.
    pub(crate) fn notify_code(&mut self, code: &str) {
        self.notify(code, BTreeMap::new());
    }

    pub(crate) fn notify(&mut self, code: &str, params: BTreeMap<String, String>) {
        // gleiche Meldung nicht doppelt stapeln
        if self.notices.iter().any(|n| n.code == code && n.params == params) {
            return;
        }
        self.notices.push(NoticeView { id: self.next_notice, code: code.to_owned(), params });
        self.next_notice += 1;
    }

    /// Reducer anwenden und Effekte ausführen (synchron erzeugte Folgeereignisse in Reihenfolge).
    pub(crate) fn apply(&mut self, event: Event) {
        self.queue.push_back(event);
        while let Some(event) = self.queue.pop_front() {
            // „Zuletzt signiert“ (ADR 0004 §3.10): jede gespeicherte Datei (Einzeldokument und Stapel)
            if let Event::Saved(info) = &event {
                self.record_recent(&info.path);
            }
            // Ergebnis einer QR-Suche (Einstellungen › Dokument): eine neue Suche ist wieder möglich, falls der
            // Reducer es verwirft (Dokument gerade nicht im Arbeitsbereich)
            if let Event::PlaceholderSearched { doc, .. } = &event {
                self.qr_searches.requested.remove(doc);
            }
            // Kopie statt `mem::replace`: bricht der Reducer ab, bleibt der bisherige Zustand erhalten
            let t = reduce(self.state.clone(), event, &self.ctx);
            let was_error = matches!(self.state, State::Error { .. });
            let was_running = matches!(self.state, State::BatchSigning { .. });
            self.state = t.state;
            if was_running && !matches!(self.state, State::BatchSigning { .. }) {
                // „Jedes Mal fragen“: die Auswahl gilt nur bis zum Ende des Stapellaufs (§8 F12)
                if self.cfg.default_backend().is_none() {
                    self.ws.selection = None;
                }
                // Zieldateien, gemerkte Passwörter und Zertifikats-Cache können sich im Lauf geändert haben
                self.probe.clear();
            }
            // Ein Signierlauf beginnt bei offener Einstellungsansicht (Startargument-Automatik `-a`, die Ansicht
            // wurde während des Ladens geöffnet): Ansicht schließen. Mitten im Lauf dürfen sich Signiermittel,
            // Signaturbild und Ausgabe nicht ändern (`ApplySettings`/`SaveSettings` wirken nur bei offener
            // Ansicht), und die Signierkarte (Anmeldung, Passwort) darf nicht unter den Einstellungen liegen.
            if self.settings_view.is_some()
                && matches!(self.state, State::Signing { .. } | State::BatchSigning { .. } | State::Resuming { .. })
            {
                tracing::debug!("Einstellungsansicht beim Signierbeginn geschlossen");
                self.settings_view = None;
                self.reset_apply_view();
            }
            tracing::debug!(state = self.state.name(), effects = t.effects.len(), "Übergang");
            match (&self.state, was_error) {
                (State::Error { .. }, false) => self.ws.error_at = Some(crate::workspace::rfc3339_now()),
                (State::Error { .. }, true) => {}
                _ => self.ws.error_at = None,
            }
            if !matches!(self.state, State::Output { decision: Some(_), saved: None, .. }) {
                self.dialog = None;
            }
            for effect in t.effects {
                self.execute(effect);
            }
        }
    }

    fn current_doc(&self) -> Option<Arc<DocData>> {
        self.state.queue().and_then(|q| q.current_id()).and_then(|id| self.shared.docs.get(id))
    }

    fn spawn_event<F>(&self, epoch: Option<u64>, fut: F)
    where
        F: std::future::Future<Output = Event> + Send + 'static,
    {
        let tx = self.shared.tx.clone();
        crate::rt::spawn(async move {
            let event = fut.await;
            let _ = tx.send(Msg::Event { epoch, event });
        });
    }

    /// Blockierende Arbeit; eine Panik (z. B. ein feindliches PDF in einer Bibliothek) wird zum
    /// Ereignis `on_panic` statt den Kern zu beenden.
    fn spawn_blocking_event<F>(&self, epoch: Option<u64>, on_panic: fn(String) -> Event, f: F)
    where
        F: FnOnce() -> Event + Send + 'static,
    {
        self.spawn_event(epoch, async move {
            match crate::rt::blocking(f).await {
                Ok(ev) => ev,
                Err(e) => {
                    tracing::error!(panic = e.is_panic(), "Hintergrundaufgabe abgebrochen");
                    on_panic(panic_text(e))
                }
            }
        });
    }

    fn sign_epoch(&self) -> u64 {
        lock(&self.shared.sign).epoch
    }

    fn cancel_session(&mut self) {
        let session = {
            let mut slot = lock(&self.shared.sign);
            slot.cancel.cancel();
            let s = slot.session.take();
            let epoch = slot.epoch + 1;
            *slot = SignSlot { epoch, ..SignSlot::default() };
            s
        };
        self.shared.broker.reset();
        if let Some(mut session) = session {
            crate::rt::spawn(async move { session.cancel().await });
        }
    }

    // --------------------------------------------------------------------------------------------
    // Effekte
    // --------------------------------------------------------------------------------------------

    fn execute(&mut self, effect: Effect) {
        match effect {
            Effect::Analyze(id) => self.effect_analyze(id),
            Effect::RenderPage { doc, page } => self.effect_render(doc, page),
            // `ApplySettings` ohne Änderung am Signaturbild: keine neue Vorschau (genau ein Neubau je Änderung
            // am Signaturbild, ADR 0004 E3)
            Effect::BuildBlockPreview(_) if self.block_change == Some(false) => {}
            Effect::BuildBlockPreview(doc) => self.effect_block(Some(doc)),
            Effect::ProbeBackends => {
                let env = self.registry.environment(self.shared.broker.interactive(), &self.cfg);
                self.options = self.registry.options(&env);
                if self.ws.enabled {
                    self.ws.signer_options = self.options.clone();
                }
                self.queue.push_back(Event::BackendsProbed(self.options.clone()));
            }
            Effect::OpenSession(id) => self.effect_open(id),
            Effect::ReadCertificate => self.effect_certificate(),
            Effect::Prepare { doc, position, min_reserve } => self.effect_prepare(doc, position, min_reserve),
            Effect::NextSignature => lock(&self.shared.sign).chain_pending = true,
            Effect::Sign => self.effect_sign(false),
            Effect::SignDocument => self.effect_sign(true),
            Effect::ReleaseDocument => self.release_document(),
            Effect::AutoPlace(doc) => {
                if matches!(&self.state, State::Positioning { doc: facts, .. } if facts.id == doc) {
                    self.auto_place();
                }
            }
            Effect::Finish => self.effect_finish(),
            Effect::CloseSession => {
                let session = {
                    let mut slot = lock(&self.shared.sign);
                    // Mehrfachsignatur: das Zwischenergebnis wird nicht mehr gebraucht (das Ergebnis liegt in `signed`)
                    slot.chain_doc = None;
                    slot.chain_pending = false;
                    slot.session.take()
                };
                drop(session);
            }
            Effect::CancelSession => self.cancel_session(),
            Effect::DecideOutput(doc) => self.effect_decide(doc),
            Effect::AskSavePath(d) => {
                if let OutputDecision::Ask { directory, file_name } = d {
                    self.dialog = Some(OutputDialog::Save(SaveDialogView { directory, file_name }));
                }
            }
            Effect::AskOverwrite(p) => self.dialog = Some(OutputDialog::Overwrite(p)),
            Effect::Write(p) => self.effect_write(p, false),
            Effect::Overwrite(p) => self.effect_write(p, true),
            Effect::OpenResult(p) => self.effect_open_result(&p),
            Effect::ShareResult(p) => self.effect_share(p),
            Effect::ExportResult(p) => self.effect_export(p),
            Effect::Notify(code) => self.notify(&code, BTreeMap::new()),
            Effect::ResetSettings => self.reset_settings(true),
            Effect::DeletePersisted => self.delete_persisted(),
            Effect::Discard(id) => {
                self.shared.docs.remove(id);
                self.ws.layouts.remove(id);
            }
            Effect::RememberDirectory(dir) => self.cfg.remember_directory(&dir),
            // ADR 0004 §3.8: Stapel
            Effect::Preanalyze(doc) => self.preanalysis.push(doc),
            Effect::SkipCurrentJob => {
                // nur den laufenden Auftrag abbrechen – die Sitzung (samt Zugangsdaten) kommt zurück (§8 F10)
                self.ws.own_cancel = true;
                lock(&self.shared.sign).job_cancel.cancel();
            }
            Effect::DecideBatchOutput { doc, output, on_existing } => {
                self.effect_decide_batch_output(doc, &output, on_existing);
            }
            Effect::NotifyWith(code, params) => self.notify(&code, params),
            // Einstellungen › Dokument (K3): QR-Platzhalter nachträglich suchen
            Effect::SearchPlaceholder(doc) => self.effect_search_placeholder(doc),
            Effect::Exit(code) => {
                self.cancel_session();
                // erst den Endzustand veröffentlichen (Ergebnisse), dann das Ende melden
                self.publish();
                let _ = self.finished_tx.send(Some(code));
            }
        }
    }

    fn effect_analyze(&mut self, id: DocId) {
        let Some(input) = self.state.queue().and_then(|q| q.current.clone()).filter(|d| d.id == id) else { return };
        let shared = self.shared.clone();
        let snap = self.snapshot();
        let search = self.ctx.placeholder_search;
        let fields = self.ctx.placeholder_fields;
        let tx = shared.tx.clone();
        crate::rt::spawn(async move {
            let work = {
                let shared = shared.clone();
                move || -> Result<(Loaded, Option<Placement>, Vec<EmptyFieldCandidate>), WorkflowError> {
                    let render = shared.render.as_deref();
                    let loaded = crate::docs::load(id, &input.path, shared.engine.as_ref(), search, fields, render)?;
                    let start = match (render, shared.profiles.as_deref()) {
                        (Some(_), Some(store)) => start_placement(&shared, &snap, &loaded.data, store),
                        _ => None,
                    };
                    // Arbeitsbereich: leere Felder immer anbieten, unabhängig von `placeholder.*` (§3.3); die
                    // Automatik nimmt nur geeignete (Nachtrag 22)
                    let mut candidates = pida_placement::find_empty_sig_fields(&loaded.data.report.fields);
                    mark_auto_fields(&shared, &snap, &loaded.data, &mut candidates);
                    Ok((loaded, start, candidates))
                }
            };
            let msg = match crate::rt::blocking(work).await {
                Ok(Ok((loaded, start, candidates))) => {
                    let report = &loaded.data.report;
                    let event = Event::Analyzed {
                        doc: id,
                        class: report.class,
                        placeholder: loaded.placeholder.clone(),
                        signed: report.is_signed(),
                        page_count: loaded.data.pages.len(),
                        start,
                        warnings: loaded.warnings.clone(),
                        fields: candidates,
                        new_visible_field_allowed: report.allows_new_visible_field(),
                    };
                    Msg::Loaded(Box::new(loaded), event)
                }
                Ok(Err(error)) => Msg::Event { epoch: None, event: Event::AnalysisFailed { doc: id, error } },
                Err(e) => Msg::Event {
                    epoch: None,
                    event: Event::AnalysisFailed { doc: id, error: WorkflowError::Unexpected(panic_text(e)) },
                },
            };
            let _ = tx.send(msg);
        });
    }

    /// QR-Platzhalter eines geladenen Dokuments suchen (Einstellungen › Dokument nachträglich auf
    /// „QR-Code-Platzhalter“; ohne Renderer: keiner) → `Event::PlaceholderSearched`. Eine laufende Suche
    /// desselben Dokuments wird nicht verdoppelt.
    fn effect_search_placeholder(&mut self, doc: DocId) {
        let Some(data) = self.shared.docs.get(doc) else { return };
        if !self.qr_searches.requested.insert(doc) {
            return;
        }
        let render = self.shared.render.clone();
        let shared = self.shared.clone();
        crate::rt::spawn_blocking(move || {
            let found = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::docs::find_placeholder(&data, false, render.as_deref())
            }));
            let qr = match found {
                Ok(Some(pida_workflow::PlaceholderFound::Qr(qr))) => Some(qr),
                Ok(_) => None,
                Err(_) => {
                    tracing::error!("Platzhaltersuche abgebrochen (Programmfehler)");
                    None
                }
            };
            shared.send(Msg::Event { epoch: None, event: Event::PlaceholderSearched { doc, qr } });
        });
    }

    fn effect_render(&mut self, doc: DocId, page: usize) {
        let Some(render) = self.shared.render.clone() else { return };
        let shared = self.shared.clone();
        let scale = *lock(&shared.tile_scale);
        crate::rt::spawn_blocking(move || {
            if let Err(e) = shared.docs.tile(doc, page, scale, render.as_ref()) {
                tracing::debug!(code = e.code(), "Vorab-Rendern fehlgeschlagen");
            }
        });
    }

    fn effect_block(&mut self, doc: Option<DocId>) {
        let shared = self.shared.clone();
        let snap = self.snapshot();
        let data = doc.and_then(|d| shared.docs.get(d));
        let page = match &self.state {
            State::Positioning { page, doc: facts, .. } => (*page).min(facts.page_count.saturating_sub(1)),
            _ => data.as_ref().map(|d| d.pages.len().saturating_sub(1)).unwrap_or(0),
        };
        self.block_requested += 1;
        let generation = self.block_requested;
        crate::rt::spawn_blocking(move || {
            // eine Panik (Programmfehler, feindliche Bildmarke) meldet sich wie ein Fehler zurück – sonst
            // wartete eine Übernahme (`applied_revision`, ADR 0004 §3.11) für immer auf diese Vorschau
            let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                shared.build_block(&snap, data.as_deref(), page)
            }))
            .unwrap_or_else(|_| {
                tracing::error!("Blockvorschau abgebrochen (Programmfehler)");
                Err(pida_workflow::MSG_UNEXPECTED.into())
            });
            match built {
                Ok(info) => shared.send(Msg::BlockBuilt(info, generation)),
                Err(code) => {
                    tracing::warn!(code = %code, "Blockvorschau nicht erstellt");
                    shared.send(Msg::BlockFailed { generation, code });
                }
            }
        });
    }

    /// Vorschau der Einstellungsansicht neu bauen (Signaturbild geändert, der Reducer baut keine): mit dem
    /// aktuellen Dokument (Seitenmaße, PDF/A-Kennung – wie beim Öffnen der Ansicht), sonst mit dem A4-Beispiel.
    fn preview_doc(&self) -> Option<DocId> {
        self.current_doc().map(|d| d.id)
    }

    /// Blockvorschau der Generation `generation` angekommen bzw. fehlgeschlagen: eine auf sie wartende
    /// Übernahme (`ApplySettings`) wird zusammen mit der neuen Vorschau veröffentlicht.
    fn block_settled(&mut self, generation: u64) {
        if self.apply_state.awaiting_block.is_some_and(|g| generation >= g) {
            self.apply_state.awaiting_block = None;
            self.apply_state.published = self.apply_state.applied;
        }
    }

    fn on_block(&mut self, mut info: BlockInfo, generation: u64) {
        // ein älterer Auftrag, der nach einem neueren fertig wird, überschreibt dessen Vorschau nicht
        if generation < self.block_done {
            tracing::debug!(generation, "veraltete Blockvorschau verworfen");
            return;
        }
        self.block_done = generation;
        {
            let mut block = lock(&self.shared.block);
            info.revision = block.revision + 1;
            *block = info.clone();
        }
        self.block_settled(generation);
        // Blockgröße geändert (Profil, Hinweis, Logo): Position anpassen und klemmen (F-43); im Arbeitsbereich
        // behalten Herkunft und Ziel ihren Wert, automatisch platzierte Blöcke werden neu gesetzt (§3.3)
        if let State::Positioning { placement: Some(p), doc, edit, .. } = &self.state
            && info.doc == Some(doc.id)
            && ((p.width - info.width).abs() > 0.01 || (p.height - info.height).abs() > 0.01)
        {
            let workspace = edit.is_some();
            let moved = Placement { width: info.width, height: info.height, ..*p };
            if let Some(clamped) = self.clamp(&moved) {
                self.apply(if workspace { Event::BlockResized(clamped) } else { Event::PlacementChanged(clamped) });
            }
        }
    }

    /// Klemmt eine Position auf die (sichtbare) Seite (F-35, KR-F11).
    fn clamp(&self, p: &Placement) -> Option<Placement> {
        let doc = self.current_doc()?;
        let page = if p.page_index < doc.pages.len() {
            doc.pages[p.page_index].clone()
        } else {
            pida_placement::new_page_geometry(doc.pages.last()?)
        };
        let t = PageTransform::new(&page, ScreenGeometry::identity());
        let pt = t.clamp_block(Point { x: p.x, y: p.y }, p.width, p.height);
        Some(Placement { x: pt.x, y: pt.y, ..*p })
    }

    fn session_ctx(&self) -> SessionContext {
        SessionContext {
            language: self.cfg.language(),
            batch_size: match &self.state {
                State::BatchSigning { run, .. } => run.order.len().max(1),
                // Arbeitsbereich: ein Dokument je Sitzung (Mehrfachsignatur: je Block ein Auftrag)
                State::Signing { edit: Some(e), .. } if self.ws.enabled => {
                    e.rounds.as_ref().map_or(1, |r| r.positions.len().max(1))
                }
                _ if self.ws.enabled => 1,
                s => s.queue().map(|q| q.len()).unwrap_or(1).max(1),
            },
            cancel: lock(&self.shared.sign).cancel.clone(),
            headless: !self.shared.broker.interactive(),
        }
    }

    fn effect_open(&mut self, id: BackendId) {
        self.ws.own_cancel = false;
        // Mehrfachsignatur: jeder Start beginnt beim Original (ein Teilergebnis wird verworfen)
        {
            let mut slot = lock(&self.shared.sign);
            slot.chain_doc = None;
            slot.chain_pending = false;
        }
        let caps = self.options.iter().find(|o| o.id == id).and_then(|o| o.capabilities.clone());
        // Arbeitsbereich, „Erneut versuchen“: eine offene, unversehrte Sitzung desselben Signiermittels wird
        // wiederverwendet – ohne neue Anmeldung und ohne zweites Lesen des Zertifikats (ADR 0004 §3.6)
        if self.ws.enabled {
            let mut slot = lock(&self.shared.sign);
            if slot.session.is_some() && slot.backend == Some(id) && !slot.cancel.is_cancelled() {
                slot.epoch += 1;
                slot.prepared = None;
                slot.attrs = None;
                slot.result = None;
                slot.signed = None;
                slot.summary = None;
                slot.block_page = None;
                slot.job_cancel = CancelToken::new();
                slot.reused = true;
                if caps.is_some() {
                    slot.caps = caps;
                }
                drop(slot);
                tracing::info!(backend = id.as_str(), "Sitzung für „Erneut versuchen“ wiederverwendet");
                self.queue.push_back(Event::PhaseCompleted(pida_workflow::SigningPhase::OpeningSession));
                return;
            }
        }
        // neuer Vorgang: alte Reste verwerfen, neue Epoche
        self.cancel_session();
        self.ws.login_done = false;
        self.ws.last_method = None;
        {
            let mut slot = lock(&self.shared.sign);
            slot.caps = caps;
            slot.backend = Some(id);
        }
        let epoch = self.sign_epoch();
        let Some(backend) = self.registry.get(id) else {
            self.queue.push_back(Event::BackendFailed(BackendError::Unavailable(id.as_str().into())));
            return;
        };
        let ctx = self.session_ctx();
        let broker: Arc<dyn pida_backend_api::InteractionBroker> = self.shared.broker.clone();
        let shared = self.shared.clone();
        self.spawn_event(Some(epoch), async move {
            let cancel = ctx.cancel.clone();
            let opened = tokio::select! {
                biased;
                _ = cancel.cancelled() => Err(BackendError::Cancelled),
                r = backend.open(ctx, broker) => r,
            };
            match opened {
                Ok(session) => {
                    let mut slot = lock(&shared.sign);
                    if slot.epoch == epoch {
                        slot.session = Some(session);
                    }
                    Event::PhaseCompleted(pida_workflow::SigningPhase::OpeningSession)
                }
                Err(e) => Event::BackendFailed(e),
            }
        });
    }

    /// Cache und Schlüssel für das Backend des laufenden Vorgangs (nur mit Opt-in).
    fn certificate_cache(&self) -> Option<(CertificateCache, CertCacheKey)> {
        let backend = match &self.state {
            State::Signing { backend, .. } => *backend,
            State::BatchSigning { run, .. } => run.backend,
            _ => return None,
        };
        self.certificate_cache_for(backend)
    }

    /// Cache und Schlüssel für ein Signiermittel (nur mit Opt-in).
    pub(crate) fn certificate_cache_for(&self, backend: BackendId) -> Option<(CertificateCache, CertCacheKey)> {
        let settings = &self.cfg.settings.certificate_cache;
        let backend = &backend;
        if !settings.enabled || !uses_certificate_cache(*backend) {
            return None;
        }
        let account = self.cfg.effective().username().map(pida_config::normalize_mobile_number).unwrap_or_default();
        Some((CertificateCache::new(&self.cfg.paths, settings), CertCacheKey::new(backend.as_str(), account)))
    }

    /// Gültiges Zertifikat aus dem Cache (sonst wird der Eintrag verworfen).
    fn cached_certificate(cache: &CertificateCache, key: &CertCacheKey) -> Option<CertChain> {
        let der = match cache.get(key) {
            Ok(Some(der)) => der,
            Ok(None) => return None,
            Err(e) => {
                tracing::warn!(id = e.message_id(), "Zertifikats-Cache nicht lesbar");
                return None;
            }
        };
        match pida_cades::Certificate::from_der(&der) {
            Ok(cert) if cert.is_valid_at(time::OffsetDateTime::now_utc()) => Some(CertChain::new(cert)),
            _ => {
                tracing::info!("Zertifikat im Cache ungültig oder abgelaufen – wird neu gelesen");
                let _ = cache.remove(key);
                None
            }
        }
    }

    fn effect_certificate(&mut self) {
        {
            let mut slot = lock(&self.shared.sign);
            if slot.reused && slot.chain.is_some() {
                // wiederverwendete Sitzung: Zertifikat bereits gelesen (kein zweites InfoboxRead)
                if slot.cert_source == CertificateSource::Read {
                    slot.cert_source = CertificateSource::Session;
                }
                drop(slot);
                self.queue.push_back(Event::PhaseCompleted(pida_workflow::SigningPhase::ReadingCertificate));
                return;
            }
            slot.cert_source = CertificateSource::Read;
        }
        let cache = self.certificate_cache();
        if let Some((cache, key)) = &cache
            && let Some(chain) = Self::cached_certificate(cache, key)
        {
            // Plan §4.2: ab der zweiten Signatur entfällt das Lesen (InfoboxRead); I3 erzwingt danach die
            // Übereinstimmung mit dem Signerzertifikat der Antwort.
            tracing::info!("Zertifikat aus dem Zertifikats-Cache");
            let mut slot = lock(&self.shared.sign);
            slot.chain = Some(chain);
            slot.cert_cache = Some((cache.clone(), key.clone(), true));
            slot.cert_source = CertificateSource::Cache;
            drop(slot);
            self.queue.push_back(Event::PhaseCompleted(pida_workflow::SigningPhase::ReadingCertificate));
            return;
        }
        lock(&self.shared.sign).cert_cache = cache.map(|(c, k)| (c, k, false));
        let shared = self.shared.clone();
        let (epoch, session, cancel, job_cancel) = {
            let mut slot = lock(&shared.sign);
            if slot.job_cancel.is_cancelled() {
                slot.job_cancel = CancelToken::new();
            }
            (slot.epoch, slot.session.take(), slot.cancel.clone(), slot.job_cancel.clone())
        };
        let Some(mut session) = session else {
            self.queue.push_back(Event::BackendFailed(BackendError::Internal("keine Sitzung".into())));
            return;
        };
        self.ws.own_cancel = false;
        self.spawn_event(Some(epoch), async move {
            // Stapel: „Dokument überspringen“ während des Lesens bricht nur diesen Schritt ab, die Sitzung
            // bleibt (ADR 0004 §3.8)
            let r = tokio::select! {
                biased;
                _ = cancel.cancelled() => Err(BackendError::Cancelled),
                _ = job_cancel.cancelled() => Err(BackendError::Cancelled),
                r = session.certificate() => r,
            };
            if r.is_err() && cancel.is_cancelled() {
                session.cancel().await;
            }
            let mut slot = lock(&shared.sign);
            if slot.epoch != epoch {
                return Event::BackendFailed(BackendError::Cancelled);
            }
            slot.session = Some(session);
            match r {
                Ok(chain) => {
                    slot.chain = Some(chain);
                    Event::PhaseCompleted(pida_workflow::SigningPhase::ReadingCertificate)
                }
                Err(e) => Event::BackendFailed(e),
            }
        });
    }

    fn effect_prepare(&mut self, doc: DocId, position: PositionChoice, min_reserve: Option<usize>) {
        let Some(original) = self.shared.docs.get(doc) else {
            self.queue.push_back(Event::SigningFailed(WorkflowError::Unexpected("Dokument nicht geladen".into())));
            return;
        };
        // Mehrfachsignatur: ab der zweiten Runde ist das Zwischenergebnis die Quelle; ein solcher Vorgang wird
        // nicht für „Signatur fortsetzen“ gesichert (bricht er ab, wird alles verworfen)
        let (multi, first_round) = match &self.state {
            State::Signing { edit: Some(e), .. } => {
                (e.rounds.is_some(), e.rounds.as_ref().is_none_or(|r| r.index == 0))
            }
            State::BatchSigning { run, .. } => (run.positions.len() > 1, run.round == 0),
            _ => (false, true),
        };
        if first_round {
            // erste Signatur eines Dokuments (auch nach „Erneut versuchen“ bzw. im Stapel beim nächsten Dokument):
            // Quelle ist das Original, ein Zwischenergebnis wird verworfen
            let mut slot = lock(&self.shared.sign);
            slot.chain_doc = None;
            slot.chain_pending = false;
        }
        // Arbeitsbereich: dieselbe Automatik wie die Vorschau (`auto_place`), sonst PDF-AS-Regel
        let avoid_widgets = self.ws.enabled;
        let shared = self.shared.clone();
        let snap = self.snapshot();
        let (epoch, chain, caps) = {
            let slot = lock(&shared.sign);
            (slot.epoch, slot.chain.clone(), slot.caps.clone())
        };
        let backend = match &self.state {
            State::Signing { backend, .. } => Some(*backend),
            State::BatchSigning { run, .. } => Some(run.backend),
            _ => None,
        };
        // „Signatur fortsetzen“: genau einmal wiederherstellen – nur für dieses Dokument, dasselbe
        // Signiermittel und ohne vergrößerte Reserve (sonst neu vorbereiten)
        let restore = self
            .resume_active
            .take()
            .filter(|(id, p)| *id == doc && Some(p.record.backend) == backend && min_reserve.is_none() && !multi)
            .map(|(_, p)| p);
        // Mobile: nach `prepare` für „Signatur fortsetzen“ sichern (ersetzt ein nicht angenommenes Angebot)
        let persist = match (self.mobile.persist_prepared && !multi, backend) {
            (true, Some(backend)) if restore.is_none() => {
                self.resume_offer = None;
                self.persist_owned = true;
                let pending = self.persist_pending();
                let name = self
                    .state
                    .queue()
                    .and_then(|q| q.current.as_ref())
                    .map(|d| d.display_name.clone())
                    .unwrap_or_else(|| original.name.clone());
                // Regel für vorhandene Zieldateien: im Stapellauf die des Laufs, beim fortgesetzten
                // Stapeldokument (neu vorbereitet) die gespeicherte
                let on_existing = match &self.state {
                    State::BatchSigning { run, .. } => Some(run.on_existing),
                    _ => self.resumed_on_existing.filter(|(id, _)| *id == doc).map(|(_, p)| p),
                };
                Some(PersistJob { dir: self.resume_dir(), name, backend, pending, on_existing })
            }
            _ => None,
        };
        self.spawn_blocking_event(Some(epoch), signing_panic, move || {
            let data = match chained_source(&shared, epoch, &original) {
                Ok(data) => data,
                Err(event) => return *event,
            };
            if let Some(persisted) = &restore {
                match persisted.restore(shared.engine.as_ref(), &data.src) {
                    Ok(prepared) => {
                        tracing::info!("vorbereitetes Dokument wiederhergestellt („Signatur fortsetzen“)");
                        let profile = prepared
                            .cms_profile()
                            .unwrap_or_else(|| plan::cms_profile(snap.settings.signature.cms_profile));
                        return store_prepared(&shared, epoch, prepared, profile);
                    }
                    Err(code) => {
                        // neu vorbereiten (mit dem eben gelesenen Zertifikat); der alte Zustand ist verbraucht
                        tracing::warn!(code = %code, "Wiederherstellen fehlgeschlagen – neu vorbereiten");
                        shared.send(Msg::Notices(vec![(resume::MSG_RESTORE_FAILED.to_owned(), BTreeMap::new())]));
                    }
                }
            }
            let model_a = caps.as_ref().is_some_and(|c| matches!(c.input, SignInput::Digest { .. }));
            let reserve = match (min_reserve, model_a) {
                (Some(n), _) => Reserve::Bytes(n),
                (None, true) => Reserve::ModelA,
                (None, false) => Reserve::ModelB,
            };
            let Some(store) = shared.profiles.as_deref() else {
                return Event::SigningFailed(prepare_error("sigblock-missing-resource", "Profile"));
            };
            let emblem = snap.effective().emblem().and_then(plan::load_emblem);
            // QR-Platzhalter mit `profile=…` ersetzt das Profil (PDF-AS `placeholder_profile_overwrite`)
            let qr_profile = match &position {
                PositionChoice::Placeholder(qr) => qr.payload.profile.as_deref(),
                _ => None,
            };
            let spec = match plan::placeholder_block_spec(
                store,
                SpecInput {
                    effective: snap.effective(),
                    system_language: Lang::system(),
                    pdfa: data.report.pdfa.as_ref(),
                    cert: chain.as_ref().map(|c| &c.signer),
                    signing_time: plan::local_now(),
                    emblem: emblem.as_ref(),
                },
                qr_profile,
            ) {
                Ok((s, notice)) => {
                    if let Some((code, profile)) = notice {
                        let params = BTreeMap::from([("profile".to_owned(), profile)]);
                        shared.send(Msg::Notices(vec![(code.to_owned(), params)]));
                    }
                    s
                }
                Err(e) => return Event::SigningFailed(prepare_error(&e.code(), &e.to_string())),
            };
            let cms_profile = plan::cms_profile(snap.settings.signature.cms_profile);
            let built = data.with_renderer_opt(shared.render.as_deref(), |renderer| {
                plan::build_plan(PlanInput {
                    report: &data.report,
                    pages: &data.pages,
                    position: &position,
                    spec: &spec,
                    chain: chain.as_ref(),
                    reserve,
                    cms_profile,
                    renderer,
                    avoid_widgets,
                    store,
                    fonts: &shared.fonts,
                })
            });
            let built = match built {
                Ok(b) => b,
                Err(e) => return Event::SigningFailed(prepare_error(&e.code(), &e.to_string())),
            };
            if !built.notices.is_empty() {
                shared.send(Msg::Notices(built.notices.iter().map(|c| (c.clone(), BTreeMap::new())).collect()));
            }
            // Seite des sichtbaren Blocks für „Fertig“ (§3.7)
            let block_page = match &built.plan.visibility {
                pida_pades::Visibility::Visible(p) => Some(p.page_index),
                pida_pades::Visibility::Invisible => None,
            };
            {
                let mut slot = lock(&shared.sign);
                if slot.epoch == epoch {
                    slot.block_page = Some(block_page);
                }
            }
            let prepared = match shared.engine.prepare(&data.src, &built.plan, chain.as_ref()) {
                Ok(p) => p,
                Err(e) => return Event::SigningFailed(pades_error(&e)),
            };
            if let Some(job) = &persist {
                job.save(&data, prepared.as_ref(), &position);
            }
            store_prepared(&shared, epoch, prepared, cms_profile)
        });
    }

    /// Wartende Dokumente für „Signatur fortsetzen“: im Stapellauf alle übrigen, noch nicht signierten
    /// Einträge (fortgesetzt wird nur das unterbrochene Dokument, danach die Übersicht, ADR 0004 §3.8).
    fn persist_pending(&self) -> Vec<PathBuf> {
        let Some(queue) = self.state.queue() else { return Vec::new() };
        match (&self.state, &queue.batch) {
            (State::BatchSigning { .. }, Some(batch)) => batch
                .items
                .iter()
                .filter(|i| Some(i.doc) != queue.current_id())
                .filter(|i| !matches!(i.status, pida_workflow::BatchItemStatus::Signed { .. }))
                .filter_map(|i| queue.find(i.doc).map(|d| d.path.clone()))
                .collect(),
            _ => queue.pending.iter().map(|d| d.path.clone()).collect(),
        }
    }

    /// Backend `sign` (klassisch) bzw. `sign_one` mit eigenem Job-Abbruch (`one`, Arbeitsbereich).
    fn effect_sign(&mut self, one: bool) {
        // neuer Auftrag: ein später gemeldetes `Cancelled` ist eine Ablehnung, sofern PIDA nicht selbst abbricht
        self.ws.own_cancel = false;
        let shared = self.shared.clone();
        let name = self.current_doc().map(|d| d.name.clone()).unwrap_or_default();
        let (epoch, session, prepared, chain, caps, cancel, cms_profile, job_cancel) = {
            let mut slot = lock(&shared.sign);
            if slot.job_cancel.is_cancelled() {
                slot.job_cancel = CancelToken::new();
            }
            (
                slot.epoch,
                slot.session.take(),
                slot.prepared.take(),
                slot.chain.clone(),
                slot.caps.clone(),
                slot.cancel.clone(),
                slot.cms_profile.unwrap_or_else(|| plan::cms_profile(self.cfg.settings.signature.cms_profile)),
                slot.job_cancel.clone(),
            )
        };
        let (Some(mut session), Some(prepared)) = (session, prepared) else {
            self.queue.push_back(Event::BackendFailed(BackendError::Internal("Sitzung oder Dokument fehlt".into())));
            return;
        };
        let model_a = caps.as_ref().is_some_and(|c| matches!(c.input, SignInput::Digest { .. }));
        self.spawn_event(Some(epoch), async move {
            // Auftrag bauen
            let (job, holder, attrs) = if model_a {
                let Some(chain) = chain.as_ref() else {
                    return Event::SigningFailed(prepare_error("backend-internal", "Zertifikat fehlt"));
                };
                match digest_job(prepared.as_ref(), chain, cms_profile, &name) {
                    Ok((job, attrs)) => (SignJob::Digest(job), PreparedHolder::Owned(prepared), Some(attrs)),
                    Err(e) => return Event::SigningFailed(prepare_error(e.code(), &e.to_string())),
                }
            } else {
                let source = Arc::new(PreparedSource { name: name.clone(), inner: prepared });
                let job = SignJob::Document(DocumentJob {
                    source: source.clone(),
                    expected_signer: chain.as_ref().map(|c| c.signer.clone()),
                });
                (job, PreparedHolder::Shared(source), None)
            };
            let r = if one {
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => Err(BackendError::Cancelled),
                    r = session.sign_one(job, job_cancel) => r.map(|r| vec![r]),
                }
            } else {
                tokio::select! {
                    biased;
                    _ = cancel.cancelled() => Err(BackendError::Cancelled),
                    r = session.sign(vec![job]) => r,
                }
            };
            if r.is_err() && cancel.is_cancelled() {
                session.cancel().await;
            }
            let prepared = match holder.into_inner() {
                Some(p) => p,
                None => return Event::BackendFailed(BackendError::Internal("Dokument wird noch gehalten".into())),
            };
            let mut slot = lock(&shared.sign);
            if slot.epoch != epoch {
                return Event::BackendFailed(BackendError::Cancelled);
            }
            slot.session = Some(session);
            slot.prepared = Some(prepared);
            match r {
                Ok(mut results) if results.len() == 1 => {
                    slot.result = results.pop();
                    slot.attrs = attrs;
                    Event::PhaseCompleted(pida_workflow::SigningPhase::Signing)
                }
                Ok(results) => Event::BackendFailed(BackendError::Internal(format!(
                    "{} Ergebnisse für einen Auftrag",
                    results.len()
                ))),
                Err(e) => Event::BackendFailed(e),
            }
        });
    }

    fn effect_finish(&mut self) {
        let shared = self.shared.clone();
        let original = self.current_doc();
        let (epoch, prepared, result, attrs, chain, cert_cache, data) = {
            let mut slot = lock(&shared.sign);
            // Mehrfachsignatur: geprüft wird gegen die Quelle dieser Runde (das Zwischenergebnis)
            let data = slot.chain_doc.clone().or(original);
            (
                slot.epoch,
                slot.prepared.take(),
                slot.result.take(),
                slot.attrs.take(),
                slot.chain.clone(),
                slot.cert_cache.clone(),
                data,
            )
        };
        self.spawn_blocking_event(Some(epoch), signing_panic, move || {
            let (Some(data), Some(prepared), Some(result)) = (data, prepared, result) else {
                return Event::SigningFailed(WorkflowError::Unexpected("Signaturergebnis fehlt".into()));
            };
            // Zusammenfassung „Fertig“ (§3.7): Unterzeichner aus der Kette bzw. dem Ergebnis, Zeit wie `/M`
            let signing_time = crate::workspace::rfc3339(prepared.signing_time());
            let signer_cert = match (&result, &chain) {
                (_, Some(c)) => Some(c.signer.clone()),
                (SignResult::Raw { chain: c, .. }, None) => Some(c.signer.clone()),
                (SignResult::Cms(cms), None) => pida_cades::parse_cms(cms).ok().and_then(|info| {
                    info.signer.clone().or_else(|| crate::workspace::leaf_certificate(&info.certificates).cloned())
                }),
            };
            let signer = signer_cert.as_ref().and_then(pida_cades::Certificate::subject_cn);
            // Qualifikation laut Zertifikat (qcStatements, K4); ohne Zertifikat aus den Fähigkeiten
            let qualification = signer_cert.as_ref().map_or(pida_backend_api::Qualification::Unknown, qualification_of);
            let finished = match (result, attrs) {
                (SignResult::Cms(cms), _) => {
                    let expect = Expect {
                        signer: chain
                            .as_ref()
                            .map_or(ExpectedSigner::Any, |c| ExpectedSigner::Certificate(c.signer.clone())),
                        ..Expect::default()
                    };
                    prepared.finish_cms(&cms, &expect)
                }
                (SignResult::Raw { signature, chain: result_chain }, Some(attrs)) => {
                    prepared.finish_raw(&attrs, &signature, &result_chain)
                }
                (SignResult::Raw { .. }, None) => {
                    return Event::SigningFailed(WorkflowError::Unexpected("Attribute fehlen".into()));
                }
            };
            let signed = match finished {
                Ok(s) => s,
                Err(FinishError::CmsTooLarge { needed, reserve }) => return Event::CmsTooLarge { needed, reserve },
                Err(FinishError::Cms(e)) => {
                    if let Some((cache, key, true)) = &cert_cache
                        && is_signer_mismatch(e.code())
                    {
                        // Zertifikat erneuert bzw. anderes Konto: Ergebnis verwerfen, Eintrag löschen;
                        // „Wiederholen“ liest das Zertifikat neu und baut den Block neu (Plan §4.2).
                        tracing::warn!(code = e.code(), "Zertifikat aus dem Cache passt nicht – Eintrag gelöscht");
                        let _ = cache.remove(key);
                        // „Erneut versuchen“ in derselben Sitzung liest das Zertifikat neu
                        let mut slot = lock(&shared.sign);
                        if slot.epoch == epoch {
                            slot.chain = None;
                            slot.cert_cache = None;
                            slot.reused = false;
                        }
                        return Event::SigningFailed(prepare_error(MSG_CERT_CACHE_MISMATCH, e.code()));
                    }
                    return Event::SelfCheckFailed(InvariantViolation {
                        invariant: Invariant::I3Cms,
                        detail: e.code().into(),
                    });
                }
                Err(FinishError::Incremental(e)) => {
                    return Event::SigningFailed(prepare_error("error-PrepareDocument", &e.to_string()));
                }
            };
            let check = CheckContext {
                source: &data.src,
                expected_signer: chain.as_ref().map(|c| &c.signer),
                renderer: shared.render.as_deref(),
            };
            match signed.self_check(&check) {
                Ok(report) => {
                    for w in &report.warnings {
                        tracing::info!(warning = %w, "Selbstprüfung: Hinweis");
                    }
                }
                Err(v) => return Event::SelfCheckFailed(v),
            }
            // bestätigtes Signerzertifikat merken (nur mit Opt-in; nur das öffentliche Zertifikat)
            if let (Some((cache, key, _)), Some(chain)) = (&cert_cache, &chain)
                && let Err(e) = cache.put(key, chain.signer.as_der())
            {
                tracing::warn!(id = e.message_id(), "Zertifikats-Cache nicht beschreibbar");
            }
            let mut slot = lock(&shared.sign);
            if slot.epoch != epoch {
                return Event::BackendFailed(BackendError::Cancelled);
            }
            slot.signed = Some(Arc::from(signed));
            let page = slot.block_page.unwrap_or(None);
            slot.summary = Some(SummaryInfo { signer, signing_time, page, qualification });
            Event::PhaseCompleted(pida_workflow::SigningPhase::Finishing)
        });
    }

    fn effect_decide(&mut self, doc: DocId) {
        let Some(input) = self.state.queue().and_then(|q| q.current.clone()).filter(|d| d.id == doc) else { return };
        // Mobile: ohne Einstellung in die App-Dokumente (kein Speichern-Dialog; Teilen/Exportieren danach)
        let output = self.cfg.output().or_else(|| {
            let dir = self.mobile.default_output_dir.clone()?;
            match pida_fs::create_dir_all(&dir) {
                Ok(()) => Some(dir),
                Err(e) => {
                    tracing::warn!(kind = ?e.kind(), "App-Dokumente nicht anlegbar – Speichern-Dialog");
                    None
                }
            }
        });
        let d = decide_output(&input.path, output.as_deref(), &self.cfg.settings.output.postfix, &FsProbe);
        let d = if self.headless { resolve_without_dialog(d, &FsProbe) } else { d };
        // fortgesetztes Stapeldokument mit „Beide behalten“: erste freie Nummer wie im Lauf, keine Rückfrage
        // „Datei überschreiben?“ („Überschreiben“ fragt weiterhin – nach dem App-Ende ist nicht mehr
        // feststellbar, ob die Datei ein Ergebnis desselben Stapels ist)
        let d = match (d, self.resumed_on_existing.filter(|(id, _)| *id == doc)) {
            (OutputDecision::Save { path, exists: true }, Some((_, ExistingPolicy::KeepBoth))) => {
                OutputDecision::Save { path: crate::batch::keep_both(&path), exists: false }
            }
            (d, _) => d,
        };
        self.queue.push_back(Event::OutputDecided(d));
    }

    fn effect_write(&mut self, path: PathBuf, overwrite: bool) {
        let signed = lock(&self.shared.sign).signed.clone();
        let Some(signed) = signed else {
            self.queue.push_back(Event::SaveFailed("kein signiertes Dokument".into()));
            return;
        };
        self.spawn_blocking_event(None, Event::SaveFailed, move || {
            if !overwrite && path.fs_exists() {
                return Event::SaveFailed(format!("{} existiert bereits", file_name(&path)));
            }
            match signed.write_atomically(&OutputTarget { path, overwrite }) {
                Ok(info) => Event::Saved(info),
                Err(e) => Event::SaveFailed(e.to_string()),
            }
        });
    }

    /// Datei mit der Standardanwendung öffnen (Ergebnis, „Zuletzt signiert“); Fehler als Hinweis.
    pub(crate) fn effect_open_result(&mut self, path: &Path) {
        let result = match self.shared.shell() {
            Some(shell) => shell.open_path(path),
            None => Err("keine Shell".into()),
        };
        if let Err(e) = result {
            tracing::warn!(error = %e, "Ergebnis konnte nicht geöffnet werden");
            let mut p = BTreeMap::new();
            p.insert("arg1".into(), file_name(path));
            self.notify("error-FailedToOpenDocument", p);
        }
    }

    /// Ergebnis teilen (Mobile, Share-Sheet); im Hintergrund, Fehler als Hinweis.
    pub(crate) fn effect_share(&mut self, path: PathBuf) {
        let Some(shell) = self.shared.shell() else {
            self.notify("ng-share-failed", BTreeMap::new());
            return;
        };
        let shared = self.shared.clone();
        crate::rt::spawn_blocking(move || {
            if let Err(e) = shell.share_path(&path) {
                tracing::warn!(error = %e, "Teilen fehlgeschlagen");
                shared.send(Msg::Notices(vec![("ng-share-failed".to_owned(), BTreeMap::new())]));
            }
        });
    }

    /// Ergebnis exportieren („Sichern unter“, Mobile); im Hintergrund, Ergebnis als Hinweis.
    fn effect_export(&mut self, path: PathBuf) {
        let Some(shell) = self.shared.shell() else {
            self.notify("ng-export-failed", BTreeMap::new());
            return;
        };
        let shared = self.shared.clone();
        let name = file_name(&path);
        crate::rt::spawn_blocking(move || {
            // der Hinweis nennt den tatsächlich gesicherten Namen (Mobile-Check 16.10.2026: die Dateiauswahl legte
            // „Vertrag_signed (3).pdf“ an, der Hinweis nannte den Vorschlag); nur der Dateiname, nie ein Pfad
            let notice = match shell.export_path(&path, &name) {
                Ok(Some(saved)) => Some(("ng-export-done", file_name(Path::new(&saved)))),
                Ok(None) => None,
                Err(e) => {
                    tracing::warn!(error = %e, "Exportieren fehlgeschlagen");
                    Some(("ng-export-failed", name))
                }
            };
            if let Some((code, name)) = notice {
                let params = BTreeMap::from([("name".to_owned(), name)]);
                shared.send(Msg::Notices(vec![(code.to_owned(), params)]));
            }
        });
    }

    /// „Einstellungen zurücksetzen“ (F-05, Plan §7.1): nach `.old` verschieben, neu laden.
    fn reset_settings(&mut self, restart: bool) {
        let paths = self.cfg.paths.clone();
        match paths.reset() {
            Ok(backup) => {
                let mut p = BTreeMap::new();
                p.insert("path".into(), backup.display().to_string());
                self.notify("ng-config-reset-done", p);
            }
            Err(e) => tracing::warn!(id = e.message_id(), "Zurücksetzen fehlgeschlagen"),
        }
        let mut overlay = self.cfg.overlay.clone();
        overlay.config_file = None;
        // Altordner behalten: Einstellungen › Erweitert bietet die PDF-Over-Einstellungen danach weiter an
        // (`OfferMigration`, ADR 0004 §3.12); ohne Angebot beim Zurücksetzen selbst (`offer_migration = false`)
        let legacy_dir = self.cfg.legacy_dir.clone();
        match ConfigState::load(paths, overlay, self.cfg.policy.clone(), self.cfg.secrets.clone(), legacy_dir, false) {
            Ok((mut cfg, _)) => {
                cfg.adopt_platform(self.mobile.platform);
                self.cfg = cfg;
            }
            Err(e) => tracing::warn!(id = e.message_id(), "Einstellungen nach Zurücksetzen nicht geladen"),
        }
        self.refresh_ctx();
        self.registry.configure(&self.cfg);
        self.refresh_signers();
        self.sync_recent();
        if restart {
            let docs = self.input_documents(&self.cfg.overlay.input_files.clone());
            self.queue.push_back(Event::Started { documents: docs });
        }
    }

    // --------------------------------------------------------------------------------------------
    // UI-Ereignisse
    // --------------------------------------------------------------------------------------------

    /// Dateien öffnen (`open_files`, „Erneut signieren“ aus „Zuletzt signiert“): alle gewählten Dateien in der
    /// Reihenfolge der Auswahl; was nicht dazukommt (schon offen, keine Datei – F-12), meldet `report_selection`
    /// (ADR 0004 Nachtrag 17). Liefert, ob mindestens eine Datei übergeben wurde.
    pub(crate) fn open_paths(&mut self, paths: &[PathBuf], mode: Option<crate::view::OpenFilesMode>) -> bool {
        self.open_selected(paths, mode).0
    }

    /// Wie [`Self::open_paths`]; liefert dazu die Dokumente, die der Übergang tatsächlich aufgenommen hat (ohne
    /// Duplikate) – für den Hinweis zum Teilen auf iOS (`files_received`, ADR 0004 Nachtrag 12).
    fn open_selected(&mut self, paths: &[PathBuf], mode: Option<crate::view::OpenFilesMode>) -> (bool, Vec<DocId>) {
        let selection = self.select_documents(paths);
        let docs = selection.docs.clone();
        let any = !docs.is_empty();
        if any {
            // „+ PDF“ (ADR 0004 Nachtrag 9): was „hinzufügen“ bzw. „anderes öffnen“ im Zustand bedeutet, entscheidet
            // der Reducer; im klassischen Ablauf ohne Bedeutung
            self.apply(match mode.filter(|_| self.ws.enabled) {
                None => Event::FilesAdded(docs),
                Some(crate::view::OpenFilesMode::Add) => Event::MoreFilesAdded(docs),
                Some(crate::view::OpenFilesMode::Replace) => Event::DocumentReplaced(docs),
            });
        }
        let taken = self.taken(&selection);
        self.report_selection(&selection);
        (any, taken)
    }

    pub(crate) fn on_ui(&mut self, event: UiEvent) {
        tracing::debug!(event = ui_event_kind(&event), "UI");
        // geparkte Arbeit (ADR 0004 Nachtrag 10): die Startansicht verdeckt sie – nichts daran signieren oder
        // verschieben (F-54); der Reducer lässt diese Ereignisse ohnehin ohne Wirkung, hier bleiben auch Auswahl
        // und Klemmhinweis unberührt
        if self.state.parked()
            && matches!(
                event,
                UiEvent::SignWith { .. }
                    | UiEvent::StartBatch { .. }
                    | UiEvent::ConfirmPosition
                    | UiEvent::ChooseInvisible
                    | UiEvent::ChooseBackend { .. }
                    | UiEvent::MoveBlock { .. }
                    | UiEvent::NudgeBlock { .. }
            )
        {
            tracing::debug!(event = ui_event_kind(&event), "Arbeit geparkt – ignoriert");
            return;
        }
        // iOS/Android nur mit ID Austria (ADR 0004 Nachtrag 11): Import, Prüfung und jede Wahl des Keystores
        // (bzw. die Wahl der Chipkarte im Konto-Chip) bleiben ohne Wirkung
        if !self.cfg.keystore_offered
            && let Some(kind) = crate::mobile_signers::ignored_on_mobile(&event)
        {
            tracing::info!(event = kind, "Signiermittel auf Mobilgeräten nicht verfügbar (nur ID Austria) – ignoriert");
            crate::mobile_signers::discard_event(event);
            return;
        }
        match event {
            UiEvent::OpenFiles { paths, mode } => {
                self.open_paths(&paths, mode);
            }
            UiEvent::FilesReceived { paths } => self.files_received(&paths),
            UiEvent::DismissShareHint => {
                self.ws.share_hint = None;
                self.ws.share_hint_dismissed = true;
            }
            UiEvent::AnswerWarning { proceed } => self.apply(Event::WarningAnswered(proceed)),
            UiEvent::PlaceholderAnswer { answer, field_index } => {
                self.apply(Event::PlaceholderAnswered { answer, field_index });
            }
            UiEvent::MoveBlock { page, x, y } => {
                if let Some(p) = self.move_base()
                    && x.is_finite()
                    && y.is_finite()
                {
                    let moved = Placement { page_index: page, x, y, ..p };
                    if let Some(c) = self.clamp(&moved) {
                        self.ws.clamped = (c.x - moved.x).abs() > 0.01 || (c.y - moved.y).abs() > 0.01;
                        self.apply(Event::PlacementChanged(c));
                    }
                }
            }
            UiEvent::NudgeBlock { dx, dy, fine, coarse } => {
                if let Some(p) = self.move_base() {
                    // Einstellungen › Barrierefreiheit: Schrittweite der Pfeiltasten (Umschalt/Strg bleiben)
                    let step = if coarse {
                        pida_placement::KEYBOARD_COARSE_STEP_PT
                    } else if fine {
                        pida_placement::KEYBOARD_FINE_STEP_PT
                    } else {
                        self.cfg.settings.accessibility.keyboard_step.pt().unwrap_or(pida_placement::KEYBOARD_STEP_PT)
                    };
                    let moved = Placement { x: p.x + dx as f32 * step, y: p.y + dy as f32 * step, ..p };
                    if let Some(c) = self.clamp(&moved) {
                        self.ws.clamped = (c.x - moved.x).abs() > 0.01 || (c.y - moved.y).abs() > 0.01;
                        self.apply(Event::PlacementChanged(c));
                    }
                }
            }
            UiEvent::ChangePage { page } => self.apply(Event::PageChanged(page)),
            UiEvent::ToggleNewPage => self.apply(Event::NewPageToggled),
            // Arbeitsbereich: „Freie Fläche suchen“ = automatische Platzierung, Ziel frei (§3.3)
            UiEvent::AutoPlace if self.ws.enabled => self.apply(Event::TargetChosen(pida_workflow::Target::Free)),
            UiEvent::AutoPlace => self.auto_place(),
            // Arbeitsbereich: Altereignisse → „Signieren mit“ dem gewählten Signiermittel; ohne Auswahl ohne
            // Wirkung (ADR 0004 §3.2, A1)
            UiEvent::ConfirmPosition if self.ws.enabled => {
                if let Some(backend) = self.signers_view().selected {
                    self.sign_with(backend);
                }
            }
            UiEvent::ChooseInvisible if self.ws.enabled => {
                if let Some(backend) = self.signers_view().selected {
                    self.apply(Event::InvisibleSet(true));
                    self.sign_with(backend);
                }
            }
            UiEvent::ConfirmPosition => self.apply(Event::PositionConfirmed),
            UiEvent::ChooseInvisible => self.apply(Event::InvisibleChosen),
            UiEvent::ChooseBackend { backend } => self.apply(Event::BackendChosen(backend)),
            // veraltet (ADR 0003): kein Zustimmungsschritt mehr – ohne Wirkung
            UiEvent::AcknowledgePrivacy { .. } => tracing::debug!("acknowledge_privacy ist veraltet und ohne Wirkung"),
            UiEvent::SubmitForm { request, answer } => {
                if !self.shared.broker.answer_form(request, answer) {
                    tracing::debug!(request, "Maskenantwort ohne offene Anfrage");
                }
            }
            UiEvent::SubmitSecret { request, secret, password_storage } => {
                let secret = zeroize::Zeroizing::new(secret);
                // „Passwort merken?“ (erste Verwendung): vor der Antwort, damit sie schon für dieses Passwort gilt;
                // die Frage gibt es nur zum Keystore (auf Mobilgeräten ohne Wirkung, ADR 0004 Nachtrag 11)
                match password_storage {
                    Some(storage) if self.cfg.keystore_offered => self.answer_storage_for_prompt(request, storage),
                    Some(_) => tracing::info!("Speicherart auf Mobilgeräten ohne Keystore – ignoriert"),
                    None => {}
                }
                if !self.shared.broker.answer_secret(request, secret) {
                    tracing::debug!(request, "Passwortantwort ohne offene Anfrage");
                }
            }
            UiEvent::AnswerNotice { request, answer } => {
                // Arbeitsbereich: „Abbrechen“ in einer Meldung, die nicht von A-Trust stammt (Proxy, HTTP,
                // Keystore …), ist ein eigener Abbruch – nur die A-Trust-Fehlerseite zählt als Ablehnung
                // bzw. Zeitablauf (ADR 0004 §3.6); sonst hieße es beim Keystore „In der App abgelehnt“
                if self.ws.enabled
                    && answer == pida_backend_api::NoticeAnswer::Cancel
                    && matches!(self.state, State::Signing { .. } | State::BatchSigning { .. })
                    && matches!(
                        self.shared.broker.current(),
                        InteractionView::Notice { request: r, notice } if r == request
                            && notice.code != pida_backend_api::NoticeCode::AtrustError
                    )
                {
                    self.ws.own_cancel = true;
                }
                if !self.shared.broker.answer_notice(request, answer) {
                    tracing::debug!(request, "Antwort ohne offene Meldung");
                }
            }
            UiEvent::OpenSignatureData { request } => self.open_signature_data(request),
            // Stapellauf: Abbrechen (Esc) = „Stapel abbrechen“ (sicherer Standard, ADR 0004 §3.8)
            UiEvent::Cancel if matches!(self.state, State::BatchSigning { .. }) => {
                self.ws.own_cancel = true;
                self.apply(Event::BatchAbort);
            }
            UiEvent::Cancel => {
                // eigener Abbruch: ein folgendes `Cancelled` ist keine Ablehnung (ADR 0004 §3.6)
                if matches!(self.state, State::Signing { .. }) {
                    self.ws.own_cancel = true;
                }
                // Signieren mit offener Maske: Maske abbrechen (das Backend meldet den Abbruch, F-72)
                if matches!(self.state, State::Signing { .. }) && self.shared.broker.cancel_current() {
                    return;
                }
                self.apply(Event::Cancel);
            }
            UiEvent::Retry => {
                if matches!(self.state, State::Starting) {
                    return;
                }
                self.apply(Event::Retry);
            }
            UiEvent::Save { path: Some(path) } => {
                self.dialog = None;
                self.apply(Event::OutputDecided(chosen_path(path, &FsProbe)));
            }
            UiEvent::Save { path: None } => {
                let dialog_open =
                    matches!(self.state, State::Output { decision: Some(OutputDecision::Ask { .. }), .. });
                self.dialog = None;
                self.apply(if dialog_open { Event::OutputChosen(None) } else { Event::SaveRequested });
            }
            UiEvent::ConfirmOverwrite { overwrite } => {
                self.dialog = None;
                self.apply(Event::OverwriteAnswered(overwrite));
            }
            UiEvent::OpenResult => self.apply(Event::OpenResultRequested),
            UiEvent::ShareResult => self.apply(Event::ShareResultRequested),
            UiEvent::ExportResult => self.apply(Event::ExportResultRequested),
            UiEvent::Navigate { step } => {
                if step != Step::Finish {
                    self.apply(Event::Navigate(step));
                }
            }
            UiEvent::OpenSettings => {
                if self.state.settings_enabled() || matches!(self.state, State::Error { .. }) {
                    self.settings_view = Some(self.settings_view_for(self.cfg.settings.clone(), BTreeMap::new()));
                    self.reset_apply_view();
                    // Vorschau im Einstellungsdialog auch ohne positioniertes Dokument (PDF-Over zeigt sie immer)
                    if lock(&self.shared.block).pdf.is_none() {
                        let doc = self.current_doc().map(|d| d.id);
                        self.effect_block(doc);
                    }
                }
            }
            UiEvent::SaveSettings { settings } => {
                // nur aus der geöffneten Einstellungsansicht (Overlay); sonst könnten Einstellungen z. B.
                // mitten im Signieren wechseln
                if self.settings_view.is_some() {
                    self.save_settings(*settings);
                } else {
                    tracing::debug!("Einstellungen ohne geöffnete Ansicht verworfen");
                }
            }
            // sofort wirksam (ADR 0004 §3.11): Schließen verwirft nichts mehr
            UiEvent::CloseSettings => {
                self.settings_view = None;
                self.reset_apply_view();
            }
            UiEvent::ResetSettings => {
                self.settings_view = None;
                self.reset_apply_view();
                if matches!(&self.state, State::Error { error: WorkflowError::Config(_), .. }) {
                    self.apply(Event::Retry);
                } else {
                    self.reset_settings(false);
                    self.apply(Event::SettingsChanged);
                }
            }
            UiEvent::Backgrounded => {
                // kein Abbruch: Sitzung, Masken und persistierter Zustand bleiben (Plan §3.5)
                self.backgrounded = true;
                self.apply(Event::Backgrounded);
            }
            UiEvent::Foregrounded => self.foregrounded(),
            UiEvent::ResumeSignature => self.resume_signature(),
            UiEvent::DiscardResume => self.discard_resume(),
            UiEvent::ImportKeystore { path, password, remember } => {
                let password = password.map(zeroize::Zeroizing::new);
                self.import_keystore(&path, password, remember);
            }
            UiEvent::DismissKeystoreImport => self.keystore_import = None,
            // Einstellungen › Keystore „Schlüssel anzeigen“ (Nachtrag ADR 0004; in beiden Modi, nur mit offener
            // Ansicht)
            UiEvent::InspectKeystore { password, password_storage } => {
                self.inspect_keystore(password.map(zeroize::Zeroizing::new), password_storage);
            }
            UiEvent::Quit => {
                self.ws.own_cancel = true;
                self.apply(Event::Quit);
            }
            UiEvent::DismissNotice { id } => self.notices.retain(|n| n.id != id),
            UiEvent::AcceptMigration { store_secrets } => self.accept_migration(store_secrets),
            UiEvent::DeclineMigration => {
                self.cfg.migration_offer = None;
                // Ersteinrichtung: abgelehnt = Neuinstallation („neben dem Original“), ab jetzt wird gespeichert
                let first_run = self.first_run_active();
                self.migration_decided(false, first_run);
                if let Err(e) = self.cfg.save() {
                    tracing::warn!(id = e.message_id(), "Standardeinstellungen nicht gespeichert");
                }
                self.refresh_ctx();
            }
            // ADR 0004, Arbeitsbereich (K1): nur im Arbeitsbereich-Modus; keines dieser Ereignisse außer
            // `sign_with` startet etwas (A1)
            UiEvent::SetInvisible { invisible } if self.ws.enabled => self.apply(Event::InvisibleSet(invisible)),
            UiEvent::SetBlockLanguage { language } if self.ws.enabled => {
                let changes = BTreeMap::from([(
                    "signature.block_language".to_owned(),
                    serde_json::to_value(language).unwrap_or(serde_json::Value::Null),
                )]);
                self.apply_changes_now(&changes);
            }
            UiEvent::UseTarget { target } if self.ws.enabled => {
                let target = match target {
                    PlacementTarget::Free => pida_workflow::Target::Free,
                    PlacementTarget::Field { index } => pida_workflow::Target::Field(index as usize),
                    PlacementTarget::Qr => pida_workflow::Target::Qr,
                };
                self.apply(Event::TargetChosen(target));
            }
            UiEvent::InspectPages { mut pages } if self.ws.enabled => {
                pages.truncate(crate::workspace::MAX_INSPECTED);
                pages.sort_unstable();
                pages.dedup();
                self.ws.inspected = pages;
            }
            UiEvent::SelectSigner { backend, make_default } if self.ws.enabled => {
                self.select_signer(backend, make_default)
            }
            UiEvent::SignWith { backend } if self.ws.enabled => self.sign_with(backend),
            // ADR 0004 §3.8: Stapel (K2), nur mit Stapelübersicht
            UiEvent::ShowBatch if self.ctx.batch() => self.apply(Event::ShowBatch),
            UiEvent::OpenBatchItem { doc } if self.ctx.batch() => self.apply(Event::BatchItemOpened(doc)),
            UiEvent::BackToBatch if self.ctx.batch() => self.apply(Event::BackToBatch),
            UiEvent::RemoveFromBatch { doc } if self.ctx.batch() => self.apply(Event::BatchItemRemoved(doc)),
            UiEvent::StartBatch { backend, docs, output_dir, on_existing } if self.ctx.batch() => {
                self.start_batch(backend, docs, output_dir, on_existing);
            }
            // eigener Abbruch des Auftrags: `Effect::SkipCurrentJob` setzt `own_cancel` – nur, wenn der Reducer
            // tatsächlich etwas abbricht (ohne begonnenes Dokument bleibt ein späteres `Cancelled` eine Ablehnung)
            UiEvent::SkipBatchDocument if self.ctx.batch() => self.apply(Event::BatchSkip),
            // „Für diesen Stapel automatisch positionieren“ (Nachtrag 8): nur in der Übersicht, startet nichts
            UiEvent::SetBatchAutoPosition { enabled, replace_manual } if self.ctx.batch() => {
                self.apply(Event::BatchAutoPositionSet { enabled, replace_manual });
            }
            UiEvent::AbortBatch if self.ctx.batch() => {
                if matches!(self.state, State::BatchSigning { .. }) {
                    self.ws.own_cancel = true;
                }
                self.apply(Event::BatchAbort);
            }
            // Oberflächensprache (Kopfzeile, Palette): in beiden Modi; schreibgeschützt nur im Speicher
            UiEvent::SetUiLanguage { language } => self.set_ui_language(language),
            UiEvent::SetAccessibility { accessibility } => self.set_accessibility(accessibility),
            UiEvent::ExportQrPlaceholder { spec, format, path } => self.export_qr_placeholder(spec, format, path),
            // ADR 0004 §3.11 (K3): sofort wirksame Einstellungen (in beiden Modi, nur mit offener Ansicht)
            UiEvent::ApplySettings { changes, revision } => self.apply_settings(&changes, revision),
            // ADR 0004 §3.10 (K3): „Zuletzt signiert“
            UiEvent::RemoveRecent { id } => self.remove_recent(id),
            UiEvent::ClearRecent => self.clear_recent(),
            // Nachtrag 15: „Zuletzt signiert“ bietet, was „Fertig“ kann (statt des entfernten Prüfmodus)
            UiEvent::OpenRecent { id } => self.open_recent(id),
            UiEvent::RevealRecent { id } => self.reveal_recent(id),
            UiEvent::ShareRecent { id } => self.share_recent(id),
            UiEvent::SignRecent { id, mode } => self.sign_recent(id, mode),
            // Nachtrag 21: „Über PIDA“ – Update-Prüfung beim Öffnen bzw. auf Klick (in beiden Modi; startet nichts sonst)
            UiEvent::CheckForUpdates { explicit } => self.check_for_updates(explicit),
            UiEvent::DismissUpdateHint => self.dismiss_update_hint(),
            // ADR 0004 §3.12 (K4): Ersteinrichtung und erneutes Übernahmeangebot (Einstellungen › Erweitert)
            UiEvent::UpdateFirstRun { default_backend, profile, block_language } if self.ws.enabled => {
                self.update_first_run(default_backend, profile, block_language);
            }
            UiEvent::FinishFirstRun if self.ws.enabled => self.finish_first_run(false),
            UiEvent::PostponeFirstRun if self.ws.enabled => self.finish_first_run(true),
            UiEvent::OfferMigration if self.ws.enabled => self.offer_migration(),
            // Einstellung „Automatische Positionierung“: „Diesmal selbst platzieren“ (Nachtrag 8) – nur dieses
            // Dokument, startet nichts (A1)
            UiEvent::UnlockPlacement if self.ws.enabled => self.apply(Event::PlacementUnlocked),
            // Mehrfachsignatur (je Block eine eigene Signatur): nur im Arbeitsbereich, startet nichts (A1)
            UiEvent::AddBlock { page } if self.ws.enabled => self.add_block(page),
            UiEvent::SelectBlock { index } if self.ws.enabled => self.apply(Event::BlockSelected(index as usize)),
            UiEvent::RemoveBlock { index } if self.ws.enabled => self.apply(Event::BlockRemoved(index as usize)),
            UiEvent::FillTargets if self.ws.enabled => self.fill_targets(),
            // „Zur Startseite“ (Nachtrag 10): der Reducer parkt offene Arbeit (im klassischen Ablauf wie
            // `navigate{Open}`); Fortsetzen und Verwerfen nur im Arbeitsbereich-Modus
            UiEvent::GoHome => self.apply(Event::WentHome),
            UiEvent::ResumeParked if self.ws.enabled => self.apply(Event::ParkedResumed),
            UiEvent::DiscardParked if self.ws.enabled => self.apply(Event::ParkedDiscarded),
            // ADR 0004: im klassischen Ablauf ohne Wirkung die Ereignisse des Arbeitsbereichs, ohne
            // Stapelübersicht die des Stapels. Insbesondere öffnet keines dieser Ereignisse eine Sitzung (A1).
            e @ (UiEvent::SetInvisible { .. }
            | UiEvent::SetBlockLanguage { .. }
            | UiEvent::UseTarget { .. }
            | UiEvent::InspectPages { .. }
            | UiEvent::SelectSigner { .. }
            | UiEvent::SignWith { .. }
            | UiEvent::ShowBatch
            | UiEvent::OpenBatchItem { .. }
            | UiEvent::BackToBatch
            | UiEvent::RemoveFromBatch { .. }
            | UiEvent::StartBatch { .. }
            | UiEvent::SkipBatchDocument
            | UiEvent::AbortBatch
            | UiEvent::UpdateFirstRun { .. }
            | UiEvent::FinishFirstRun
            | UiEvent::PostponeFirstRun
            | UiEvent::OfferMigration
            | UiEvent::UnlockPlacement
            | UiEvent::AddBlock { .. }
            | UiEvent::SelectBlock { .. }
            | UiEvent::RemoveBlock { .. }
            | UiEvent::FillTargets
            | UiEvent::SetBatchAutoPosition { .. }
            | UiEvent::ResumeParked
            | UiEvent::DiscardParked) => {
                tracing::debug!(event = ui_event_kind(&e), "in diesem Modus ohne Wirkung (ADR 0004) – ignoriert");
            }
        }
    }

    /// „Weiteren Block hinzufügen“ (Mehrfachsignatur): freie Stelle auf der Seite suchen – zuerst die freien
    /// Flächen des Layouts, dann ein Raster von unten rechts – die keinen anderen Block überdeckt; Grenzen
    /// (Höchstzahl, neue Seite) meldet der Reducer.
    fn add_block(&mut self, page: Option<usize>) {
        let State::Positioning { placement: Some(current), .. } = &self.state else { return };
        let current = *current;
        let Some(view) = self.document_view() else { return };
        let n = view.doc.page_count;
        if n == 0 {
            return;
        }
        let page = page.filter(|p| *p < n).unwrap_or(view.placement.page_index.min(n - 1));
        let (w, h) = (view.block.width, view.block.height);
        let Some(&(page_w, page_h)) = view.doc.page_sizes.get(page) else { return };
        // belegte Rechtecke der vorhandenen Blöcke auf dieser Seite (mit 6 pt Abstand)
        const GAP: f32 = 6.0;
        let taken: Vec<pida_pdf_cos::Rect> = view
            .blocks
            .iter()
            .flat_map(|b| b.items.iter())
            .filter(|b| b.page == page)
            .filter_map(|b| b.rect)
            .map(|r| pida_pdf_cos::Rect {
                llx: r.left - GAP,
                lly: r.bottom - GAP,
                urx: r.right + GAP,
                ury: r.top + GAP,
            })
            .collect();
        let free = |x: f32, top: f32| {
            let mine = pida_pdf_cos::Rect { llx: x, lly: top - h, urx: x + w, ury: top };
            !taken.iter().any(|t| crate::workspace::rects_intersect(mine, *t))
        };
        // Kandidaten (linke obere Ecke): freie Flächen des Layouts, dann Raster von unten nach oben
        let mut candidates: Vec<(f32, f32)> = view
            .layout
            .iter()
            .filter(|l| l.page == page)
            .flat_map(|l| l.free.iter())
            .map(|r| (r.left, r.top))
            .collect();
        const MARGIN: f32 = 36.0;
        let xs = [page_w - w - MARGIN, MARGIN, (page_w - w) / 2.0];
        let mut top = MARGIN + h;
        while top <= page_h - MARGIN {
            candidates.extend(xs.iter().map(|x| (*x, top)));
            top += h + 2.0 * GAP;
        }
        let (x, y) = candidates
            .into_iter()
            .filter(|(x, top)| *x >= 0.0 && x + w <= page_w && top - h >= 0.0 && *top <= page_h)
            .find(|(x, top)| free(*x, *top))
            .unwrap_or((current.x + 24.0, current.y - 24.0));
        let wanted = Placement { page_index: page, x, y, width: w, height: h };
        // Klemmen gegen die Seite des neuen Blocks (nicht die des ausgewählten)
        let Some(doc) = self.current_doc() else { return };
        let Some(info) = doc.pages.get(page) else { return };
        let t = PageTransform::new(info, ScreenGeometry::identity());
        let pt = t.clamp_block(Point { x: wanted.x, y: wanted.y }, w, h);
        self.apply(Event::BlockAdded(Placement { x: pt.x, y: pt.y, ..wanted }));
    }

    /// „Alle Platzhalter belegen“ (Mehrfachsignatur): alle für den Block geeigneten leeren Signaturfelder und der
    /// QR-Platzhalter; welche davon noch frei sind, entscheidet der Reducer.
    fn fill_targets(&mut self) {
        let Some(view) = self.document_view() else { return };
        let mut targets: Vec<pida_workflow::Target> =
            view.fields.iter().filter(|f| f.suitable).map(|f| pida_workflow::Target::Field(f.index as usize)).collect();
        if view.qr.is_some() {
            targets.push(pida_workflow::Target::Qr);
        }
        self.apply(Event::TargetsFilled(targets));
    }

    /// Ausgangslage für Ziehen und Tastatur: frei die Lage, bei Feld/QR die linke obere Ecke des Ziels
    /// (mit der Blockgröße) – danach ist das Ziel frei (§3.3).
    fn move_base(&self) -> Option<Placement> {
        let State::Positioning { placement: Some(p), edit, doc, .. } = &self.state else { return None };
        // Einstellung „Automatische Positionierung“: gesperrt bis „Diesmal selbst platzieren“ (Nachtrag 8)
        if edit.as_ref().is_some_and(pida_workflow::WorkspaceEdit::placement_locked) {
            tracing::debug!("Platzierung gesperrt (automatische Positionierung) – Verschieben ignoriert");
            return None;
        }
        let target = edit.as_ref().map(|e| e.target).unwrap_or(pida_workflow::Target::Free);
        if target == pida_workflow::Target::Free {
            return Some(*p);
        }
        let view = self.document_view()?;
        let sticker = view.sticker?;
        let block = lock(&self.shared.block).clone();
        let (w, h) = if block.doc == Some(doc.id) { (block.width, block.height) } else { (p.width, p.height) };
        Some(Placement { page_index: view.placement.page_index, x: sticker.left, y: sticker.top, width: w, height: h })
    }

    /// „Alle signieren (n) mit X“ (ADR 0004 §3.8): Ausgabeziel vor dem Start festlegen (Ordnerwahl nur bei
    /// „jedes Mal fragen“ bzw. ohne vorhandenen Ordner), aktuelle Optionen, dann der Reducer (Prüfung, eine
    /// Sitzung).
    fn start_batch(
        &mut self,
        backend: BackendId,
        docs: Vec<DocId>,
        output_dir: Option<PathBuf>,
        on_existing: pida_workflow::ExistingPolicy,
    ) {
        let State::BatchOverview { queue } = &self.state else {
            tracing::debug!(state = self.state.name(), "start_batch nur in der Stapelübersicht");
            return;
        };
        let output = if !self.mobile.platform.is_desktop() {
            // Mobile: immer in die App-Dokumente (keine Ordnerwahl)
            Some(pida_workflow::BatchOutput::Configured)
        } else {
            match output_dir {
                Some(dir) if dir.fs_is_dir() => Some(pida_workflow::BatchOutput::Folder(dir)),
                Some(_) => None,
                None if self.batch_output_view(queue).needs_folder => None,
                None => Some(pida_workflow::BatchOutput::Configured),
            }
        };
        let env = self.registry.environment(self.shared.broker.interactive(), &self.cfg);
        self.options = self.registry.options(&env);
        self.ws.signer_options = self.options.clone();
        // ohne Signiermittel-Auswahl wie „Signieren mit“: der Grund statt „Bitte wählen Sie ein anderes“ (Nachtrag 20)
        if let Some(reason) = self.single_signer_unavailable(backend) {
            self.notify_code(&reason);
            return;
        }
        // „Jedes Mal fragen“: die Wahl gilt für diesen Lauf (§8 F12)
        if self.ws.selection.is_none() && self.cfg.default_backend().is_none() {
            self.ws.selection = Some(Selection { backend: Some(backend), doc: None });
        }
        self.apply(Event::BatchStarted { backend, docs, output, on_existing, options: self.options.clone() });
    }

    /// Konto-Chip (§3.4): Auswahl für diese Sitzung bzw. dieses Dokument; `make_default` speichert den
    /// Standard. Startet nichts (A1).
    fn select_signer(&mut self, backend: Option<BackendId>, make_default: bool) {
        let doc = self.state.queue().and_then(|q| q.current_id());
        self.ws.selection = Some(Selection { backend, doc });
        if make_default {
            self.cfg.settings.backend.default = backend.and_then(crate::workspace::choice_for);
            if let Some(v) = &mut self.settings_view {
                v.settings.backend.default = self.cfg.settings.backend.default;
            }
            // schreibgeschützt (`-c`): nur im Speicher
            if let Err(e) = self.cfg.save() {
                tracing::warn!(id = e.message_id(), "Standard-Signiermittel nicht gespeichert");
                self.notify("error-FailedToSaveSettings", BTreeMap::new());
            }
            self.refresh_ctx();
        }
    }

    /// „Signieren mit X“ (§3.5): ausdrückliche Nutzeraktion mit den aktuellen Optionen.
    fn sign_with(&mut self, backend: BackendId) {
        if !matches!(self.state, State::Positioning { .. } | State::ChoosingBackend { .. }) {
            tracing::debug!(state = self.state.name(), "sign_with in diesem Zustand ohne Wirkung");
            return;
        }
        let env = self.registry.environment(self.shared.broker.interactive(), &self.cfg);
        self.options = self.registry.options(&env);
        self.ws.signer_options = self.options.clone();
        // ohne Signiermittel-Auswahl (nur ein Signiermittel, Nachtrag 20): „Bitte wählen Sie ein anderes“ wäre
        // nicht möglich – der Hinweis nennt den Grund (z. B. Chipkarten-Software nicht gestartet); nichts startet
        if matches!(self.state, State::Positioning { .. })
            && let Some(reason) = self.single_signer_unavailable(backend)
        {
            self.notify_code(&reason);
            return;
        }
        // „Jedes Mal fragen“ ohne Auswahl: die Wahl im Menü gilt für dieses Dokument (§8 F12); mit Standard
        // bleibt der Konto-Chip beim Standard bzw. bei der Auswahl
        let default = self.cfg.default_backend();
        if self.ws.selection.is_none() && default.is_none() {
            let doc = self.state.queue().and_then(|q| q.current_id());
            self.ws.selection = Some(Selection { backend: Some(backend), doc });
        }
        self.apply(Event::SignRequested { backend, options: self.options.clone() });
    }

    /// Oberflächensprache setzen (`SetUiLanguage`, §3.11).
    fn set_ui_language(&mut self, language: Option<Lang>) {
        self.cfg.settings.general.ui_language = language;
        if let Some(v) = &mut self.settings_view {
            v.settings.general.ui_language = language;
        }
        if let Err(e) = self.cfg.save() {
            tracing::warn!(id = e.message_id(), "Oberflächensprache nicht gespeichert");
        }
    }

    /// Einstellungen › Barrierefreiheit setzen (`SetAccessibility`): je Feld wie `apply_settings` (Richtlinie),
    /// sofort gespeichert; eine offene Einstellungsansicht zeigt die neuen Werte.
    fn set_accessibility(&mut self, a: pida_config::AccessibilitySettings) {
        let changes = match serde_json::to_value(a) {
            Ok(serde_json::Value::Object(map)) => {
                map.into_iter().map(|(k, v)| (format!("accessibility.{k}"), v)).collect::<BTreeMap<_, _>>()
            }
            _ => return,
        };
        let rejected = self.apply_changes_now(&changes);
        if !rejected.is_empty() {
            tracing::debug!(fields = rejected.len(), "Barrierefreiheit teilweise abgelehnt");
        }
        if let Some(v) = &mut self.settings_view {
            v.settings.accessibility = self.cfg.settings.accessibility;
        }
    }

    /// „QR-Platzhalter erstellen“: Bild im Hintergrund erzeugen und speichern; mit Pfad Hinweis „gespeichert“, ohne
    /// Pfad teilen (Mobile) bzw. öffnen (Web: Download). Fehler als Hinweis.
    fn export_qr_placeholder(
        &mut self,
        spec: crate::qr_placeholder::QrPlaceholderSpec,
        format: crate::qr_placeholder::QrPlaceholderFormat,
        path: Option<PathBuf>,
    ) {
        use crate::qr_placeholder as qr;
        if let Err(code) = spec.validate() {
            self.notify(code, BTreeMap::new());
            return;
        }
        let target = path.clone().unwrap_or_else(|| qr::temp_target(&self.cfg.paths.temp_dir, &spec, format));
        let mobile = !self.mobile.platform.is_desktop();
        let render = self.shared.render.clone();
        let shell = self.shared.shell();
        let shared = self.shared.clone();
        crate::rt::spawn_blocking(move || {
            let written = qr::image(&spec, format, render.as_deref()).and_then(|bytes| {
                if let Some(dir) = target.parent() {
                    pida_fs::create_dir_all(dir).map_err(|_| qr::MSG_FAILED)?;
                }
                pida_fs::write(&target, bytes).map_err(|_| qr::MSG_FAILED)
            });
            let notice = match written {
                Err(code) => Some(qr::notice(Err(code))),
                Ok(()) if path.is_some() => Some(qr::notice(Ok(file_name(&target)))),
                Ok(()) => {
                    let handed = shell.map(|s| if mobile { s.share_path(&target) } else { s.open_path(&target) });
                    match handed {
                        Some(Ok(())) => None,
                        _ => {
                            tracing::warn!("QR-Platzhalter: Teilen bzw. Öffnen fehlgeschlagen");
                            Some(qr::notice(Err(qr::MSG_FAILED)))
                        }
                    }
                }
            };
            if let Some(n) = notice {
                shared.send(Msg::Notices(vec![n]));
            }
        });
    }

    /// Vorbereitetes Dokument, Attribute und Ergebnis verwerfen, Sitzung und Kette behalten (§3.6).
    fn release_document(&mut self) {
        let mut slot = lock(&self.shared.sign);
        slot.prepared = None;
        slot.attrs = None;
        slot.result = None;
        slot.signed = None;
        slot.summary = None;
        slot.block_page = None;
    }

    /// Interaktion geändert (Broker): Anmeldung erneut verlangt bzw. Bestätigungsweg merken.
    fn on_interaction_changed(&mut self) {
        if !self.ws.enabled {
            return;
        }
        let current = self.shared.broker.current();
        if matches!(current, InteractionView::Form { form: pida_backend_api::NativeForm::Login { .. }, .. }) {
            // A-Trust zeigt die Anmeldung erneut (Fehlerseite, abgelehnte Anmeldung): Stepper zurück
            self.ws.login_done = false;
        }
        if let Some(m) = crate::workspace::remembered_method(&current) {
            self.ws.last_method = Some(m);
        }
    }

    /// Einstellungsansicht; Keystore-Angaben, Revisionen, Beispiele und Sperren ergänzt die Projektion
    /// ([`CoreTask::live_settings_view`]).
    fn settings_view_for(&self, settings: Settings, problems: BTreeMap<String, String>) -> SettingsView {
        SettingsView {
            settings,
            problems,
            keychain_available: self.cfg.secrets.is_persistent(),
            fido2_available: self.fido2_available,
            keystore_ready: false,
            keystore_format: None,
            keystore_entries: None,
            keystore_inspect_problem: None,
            read_only: self.cfg.read_only(),
            preview_revision: 0,
            applied_revision: 0,
            examples: BTreeMap::new(),
            locked: None,
            emblem_placeholder: false,
        }
    }

    fn save_settings(&mut self, mut settings: Settings) {
        settings.sanitize();
        if !self.cfg.keystore_offered {
            // iOS/Android nur mit ID Austria (ADR 0004 Nachtrag 11): `[keystore]` nicht übernehmen, kein Keystore
            // bzw. keine Chipkarte als Standard
            settings.keystore = self.cfg.settings.keystore.clone();
            crate::mobile_signers::map_default_signer(&mut settings);
        }
        let (problems, blocking) = self.validate_here(&settings);
        if blocking {
            // klassisch: die Ansicht zeigt die abgelehnte Arbeitskopie mit den Problemen
            self.settings_view = Some(self.settings_view_for(settings, problems));
            self.apply_state.draft = true;
            return;
        }
        self.cfg.settings = settings;
        if let Err(e) = self.cfg.save() {
            tracing::warn!(id = e.message_id(), "Einstellungen nicht gespeichert");
            self.notify("error-FailedToSaveSettings", BTreeMap::new());
            self.settings_view = Some(self.settings_view_for(self.cfg.settings.clone(), problems));
            self.apply_state.draft = true;
            return;
        }
        self.settings_view = None;
        self.apply_state.draft = false;
        for (field, code) in problems {
            let mut p = BTreeMap::new();
            p.insert("field".into(), field);
            self.notify(&code, p);
        }
        self.refresh_ctx();
        self.registry.configure(&self.cfg);
        self.refresh_signers();
        self.sync_recent();
        self.apply(Event::SettingsChanged);
    }

    /// Prüfung der Einstellungen ([`validate`]); auf iOS/Android ohne Probleme unter `keystore.*` (dort nur ID
    /// Austria, die Keystore-Werte werden ignoriert).
    pub(crate) fn validate_here(&self, settings: &Settings) -> (BTreeMap<String, String>, bool) {
        let (mut problems, blocking) = validate(settings);
        if !self.cfg.keystore_offered {
            problems.retain(|field, _| !field.starts_with("keystore."));
        }
        (problems, blocking)
    }

    /// Ansicht geöffnet bzw. geschlossen: Feldfehler und Arbeitskopie verwerfen; die zuletzt übernommene
    /// Revision gilt sofort als veröffentlicht (eine neu eingehängte Ansicht zählt ab ihr weiter und sendet
    /// nie eine schon vergebene Revision).
    fn reset_apply_view(&mut self) {
        self.apply_state.rejected.clear();
        self.apply_state.draft = false;
        self.apply_state.awaiting_block = None;
        self.apply_state.published = self.apply_state.applied;
    }

    /// Einstellungen sofort übernehmen (`ApplySettings`, ADR 0004 §3.11): nur die geänderten Felder, auf die
    /// **aktuellen** Einstellungen; Felder mit Problem behalten ihren Wert (`problems`), alle anderen werden
    /// übernommen und gespeichert (schreibgeschützt `-c`: nur im Speicher bis Programmende). Danach wirkt die
    /// Änderung sofort: Kontext, Signiermittel, „Zuletzt signiert“, Arbeitsbereich (`SettingsChanged`) und –
    /// nur bei geändertem Signaturbild – **genau ein** Neubau der Vorschau; `applied_revision` steigt dann
    /// erst mit der neuen Vorschau (`preview_revision`) in derselben Projektion.
    fn apply_settings(&mut self, changes: &BTreeMap<String, serde_json::Value>, revision: u64) {
        if self.settings_view.is_none() {
            tracing::debug!("apply_settings ohne geöffnete Einstellungsansicht verworfen");
            return;
        }
        if revision <= self.apply_state.applied {
            tracing::debug!(revision, applied = self.apply_state.applied, "veraltetes apply_settings verworfen");
            return;
        }
        self.apply_state.applied = revision;
        let rejected = self.apply_changes_now(changes);
        for path in changes.keys() {
            self.apply_state.rejected.remove(path);
        }
        if !rejected.is_empty() {
            tracing::debug!(fields = rejected.len(), "Einstellungen teilweise abgelehnt");
        }
        self.apply_state.rejected.extend(rejected);
        self.apply_state.draft = false;
        if self.apply_state.awaiting_block.is_none() {
            self.apply_state.published = self.apply_state.applied;
        }
    }

    /// Geänderte Felder (TOML-Pfad → serde-JSON) sofort übernehmen und speichern (`ApplySettings`,
    /// `UpdateFirstRun`): Prüfung je Feld inkl. Richtlinie, danach Kontext, Signiermittel, „Zuletzt
    /// signiert“, Arbeitsbereich (`SettingsChanged`) und – nur bei geändertem Signaturbild – genau ein Neubau
    /// der Vorschau. Liefert die abgelehnten Felder (Pfad → i18n-ID).
    pub(crate) fn apply_changes_now(
        &mut self,
        changes: &BTreeMap<String, serde_json::Value>,
    ) -> BTreeMap<String, String> {
        let locked = crate::settings_apply::locked_paths(self.cfg.policy.as_ref());
        let mobile = !self.mobile.platform.is_desktop();
        let before = self.cfg.settings.clone();
        let applied = crate::settings_apply::apply_changes(&before, changes, &locked, self.cfg.policy.as_ref(), mobile);
        if applied.settings != before {
            self.cfg.settings = applied.settings;
            if let Err(e) = self.cfg.save() {
                tracing::warn!(id = e.message_id(), "Einstellungen nicht gespeichert");
                self.notify("error-FailedToSaveSettings", BTreeMap::new());
            }
            let block = crate::settings_apply::block_affected(&before, &self.cfg.settings);
            self.refresh_ctx();
            if crate::settings_apply::backends_affected(&before, &self.cfg.settings) {
                self.registry.configure(&self.cfg);
            }
            self.refresh_signers();
            self.sync_recent();
            let requested = self.block_requested;
            self.block_change = Some(block);
            self.apply(Event::SettingsChanged);
            self.block_change = None;
            if block {
                if self.block_requested == requested {
                    let doc = self.preview_doc();
                    self.effect_block(doc);
                }
                self.apply_state.awaiting_block = Some(self.block_requested);
            }
        }
        applied.rejected
    }

    fn accept_migration(&mut self, store_secrets: bool) {
        // nie mitten im Signieren Einstellungen und Signiermittel austauschen (das Angebot bleibt)
        if matches!(self.state, State::Signing { .. } | State::BatchSigning { .. } | State::Resuming { .. }) {
            tracing::debug!(state = self.state.name(), "accept_migration während des Signierens zurückgestellt");
            return;
        }
        let Some(file) = self.cfg.migration_offer.take() else { return };
        // vor dem Übernehmen: die übernommenen Einstellungen tragen `first_run_completed = true`
        let first_run = self.first_run_active();
        let legacy_dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
        let keystore_offered = self.cfg.keystore_offered;
        let result = pida_config::LegacyConfig::load(&file).map(|legacy| {
            let ctx = pida_config::MigrationContext { legacy_dir, system_language: Lang::system(), check_paths: true };
            let mut r = pida_config::migrate(&legacy, &ctx);
            // iOS/Android nur mit ID Austria (ADR 0004 Nachtrag 11): ohne Keystore-Datei, Alias und -Passwörter
            if !keystore_offered {
                crate::mobile_signers::restrict_migration(&mut r, Lang::system());
            }
            r
        });
        // erneutes Angebot (Einstellungen › Erweitert, ADR 0004 §3.12): die vorhandene `settings.toml` wird
        // vorher gesichert (`settings.toml.before-import`)
        let overwrite = self.cfg.paths.settings_file().fs_exists();
        let applied = result.and_then(|r| {
            let opts = pida_config::ApplyOptions { store_secrets, overwrite, ..pida_config::ApplyOptions::default() };
            pida_config::apply_migration(&r, &self.cfg.paths, self.cfg.secrets.as_ref(), &opts).map(|a| (r, a))
        });
        match applied {
            Ok((r, a)) => {
                let mut p = BTreeMap::new();
                p.insert("imported".into(), r.report.imported.len().to_string());
                p.insert("skipped".into(), r.report.skipped.len().to_string());
                self.notify("ng-migrate-done", p);
                // wie geschrieben (Bildmarke als Kopie im Datenordner)
                self.cfg.settings = match Settings::load(&a.settings_file) {
                    pida_config::LoadOutcome::Loaded(s) => *s,
                    _ => r.settings,
                };
                self.cfg.adopt_platform(self.mobile.platform);
                self.migration_decided(true, first_run);
                if self.first_run_active() {
                    // Ersteinrichtung bleibt offen (Schritte 2/3 zeigen die übernommenen Werte)
                    if let Err(e) = self.cfg.save() {
                        tracing::warn!(id = e.message_id(), "Einstellungen nicht gespeichert");
                    }
                }
                self.refresh_ctx();
                self.registry.configure(&self.cfg);
                self.refresh_signers();
                self.sync_recent();
                self.apply(Event::SettingsChanged);
                // Vorschau des übernommenen Signaturbilds: Ersteinrichtung Schritt 3 bzw. Live-Vorschau der
                // offenen Einstellungen (erneutes Angebot aus Einstellungen › Erweitert); im Arbeitsbereich baut
                // `SettingsChanged` sie selbst neu
                if (self.first_run_active() || self.settings_view.is_some())
                    && !matches!(self.state, State::Positioning { .. })
                {
                    let doc = self.preview_doc();
                    self.effect_block(doc);
                }
            }
            Err(e) => {
                let mut p = BTreeMap::new();
                p.insert("detail".into(), e.to_string());
                self.notify(e.message_id(), p);
                self.migration_decided(false, first_run);
                self.refresh_ctx();
            }
        }
    }

    fn open_signature_data(&mut self, request: u64) {
        let Some(ctx) = self.shared.broker.form_context(request) else { return };
        let Some(url) = ctx.signature_data_url else { return };
        let endpoint = self
            .registry
            .atrust()
            .map(|a| a.config().endpoint.clone())
            .or_else(|| pida_net::Url::parse(pida_backend_atrust::PROD_ENDPOINT).ok());
        let Some(endpoint) = endpoint else { return };
        if let Err(e) = pida_backend_atrust::open_signature_data(self.shared.broker.as_ref(), &url, &endpoint) {
            tracing::warn!(code = e.code(), "Signaturdaten nicht geöffnet");
            self.notify("atrust-signature-data-link-rejected", BTreeMap::new());
        }
    }

    /// „Automatisch platzieren“: Position wie bei `p:auto` berechnen und anzeigen.
    fn auto_place(&mut self) {
        let State::Positioning { doc: facts, .. } = &self.state else { return };
        let doc_id = facts.id;
        let Some(data) = self.shared.docs.get(doc_id) else { return };
        let shared = self.shared.clone();
        let snap = self.snapshot();
        crate::rt::spawn_blocking(move || {
            let Some(store) = shared.profiles.as_deref() else { return };
            let emblem = snap.effective().emblem().and_then(plan::load_emblem);
            let Ok(spec) = plan::block_spec(
                store,
                SpecInput {
                    effective: snap.effective(),
                    system_language: Lang::system(),
                    pdfa: data.report.pdfa.as_ref(),
                    cert: None,
                    signing_time: plan::local_now(),
                    emblem: emblem.as_ref(),
                },
            ) else {
                return;
            };
            let built = data.with_renderer_opt(shared.render.as_deref(), |renderer| {
                plan::build_plan(PlanInput {
                    report: &data.report,
                    pages: &data.pages,
                    position: &PositionChoice::Auto,
                    spec: &spec,
                    chain: None,
                    reserve: Reserve::ModelB,
                    cms_profile: Default::default(),
                    renderer,
                    // nur im Arbeitsbereich: wie die Überdeckungsprüfung keine Felder überdecken (Nachtrag 12)
                    avoid_widgets: true,
                    store,
                    fonts: &shared.fonts,
                })
            });
            match built.map(|b| b.resolved) {
                Ok(Some(r)) => {
                    let page_index = if r.new_page { data.pages.len() } else { r.page_index };
                    let p = Placement { page_index, x: r.x, y: r.y, width: r.width, height: r.height };
                    shared.send(Msg::AutoPlaced(doc_id, p));
                }
                Ok(None) => {}
                Err(e) => shared.send(Msg::Notices(vec![(e.code(), BTreeMap::new())])),
            }
        });
    }

    // --------------------------------------------------------------------------------------------
    // Backend-Ereignisse (Persistenz liegt beim Kern, ADR 0001 §5.5)
    // --------------------------------------------------------------------------------------------

    fn on_backend_event(&mut self, event: BackendEvent) {
        match event {
            BackendEvent::LoginSucceeded { username, remember } => {
                self.ws.login_done = true;
                if remember {
                    let normalized = pida_backend_atrust::validate::normalize_phone_number(&username);
                    let s = &mut self.cfg.settings.id_austria;
                    if s.username.as_deref() != Some(normalized.as_str()) || !s.remember {
                        s.username = Some(normalized);
                        s.remember = true;
                        if let Err(e) = self.cfg.save() {
                            tracing::warn!(id = e.message_id(), "Benutzername nicht gespeichert");
                        }
                        // das Backend kennt die Daten schon (Passwort im RAM) – nicht neu bauen
                        self.registry.adopt_atrust_settings(&self.cfg);
                    }
                }
            }
            BackendEvent::ForgetCredentials => {
                let s = &mut self.cfg.settings.id_austria;
                if s.username.is_some() || s.remember {
                    s.username = None;
                    s.remember = false;
                    if let Err(e) = self.cfg.save() {
                        tracing::warn!(id = e.message_id(), "Einstellungen nicht gespeichert");
                    }
                    self.registry.adopt_atrust_settings(&self.cfg);
                }
            }
            BackendEvent::ServerTime(server) => {
                let skew = (time::OffsetDateTime::now_utc() - server).whole_seconds().abs();
                if skew > 300 {
                    let mut p = BTreeMap::new();
                    p.insert("minutes".into(), (skew / 60).to_string());
                    self.notify("ng-clock-skew", p);
                }
            }
        }
    }

    // --------------------------------------------------------------------------------------------
    // ViewState
    // --------------------------------------------------------------------------------------------

    fn publish(&self) {
        let view = self.project();
        self.view_tx.send_if_modified(|v| {
            if *v == view {
                false
            } else {
                *v = view;
                true
            }
        });
    }

    fn queue_items(&self) -> Vec<QueueItemView> {
        let Some(q) = self.state.queue() else { return Vec::new() };
        q.current
            .iter()
            .map(|d| (d, true))
            .chain(q.pending.iter().map(|d| (d, false)))
            .map(|(d, current)| QueueItemView { id: d.id, name: d.display_name.clone(), current })
            .collect()
    }

    pub(crate) fn doc_info(&self, id: DocId) -> DocInfo {
        match self.shared.docs.get(id) {
            Some(d) => DocInfo {
                id,
                name: d.name.clone(),
                page_count: d.pages.len(),
                page_sizes: d.page_sizes(),
                signed: d.report.is_signed(),
            },
            None => {
                let name = self
                    .state
                    .queue()
                    .and_then(|q| q.current.iter().chain(q.pending.iter()).find(|d| d.id == id))
                    .map(|d| d.display_name.clone())
                    .unwrap_or_default();
                DocInfo { id, name, page_count: 0, page_sizes: Vec::new(), signed: false }
            }
        }
    }

    pub(crate) fn project(&self) -> ViewState {
        // geparkte Arbeit (ADR 0004 Nachtrag 10): die Startansicht verdeckt sie – Schritt, Hauptleiste und
        // Freigaben wie in der Dokumentauswahl, ohne Dokument und Stapel; dazu die Karte „In Bearbeitung“
        let parked = self.state.parked();
        let idle;
        let shown = if parked {
            idle = State::Idle { queue: pida_workflow::Queue::default() };
            &idle
        } else {
            &self.state
        };
        let step = if parked { StepView::Open { queue: Vec::new() } } else { self.step_view(&self.state) };
        let settings_open = self.settings_view.is_some();
        let main_bar = MainBarView {
            steps: shown.main_bar().to_vec(),
            settings_enabled: !settings_open && shown.settings_enabled(),
        };
        let mut actions = crate::actions::actions_for(shown, main_bar.settings_enabled);
        if self.ws.enabled {
            // Arbeitsbereich: keine Hauptleiste Positionieren/Signieren; „Signieren“ nur mit zulässiger Platzierung
            actions.sign = actions.sign && crate::actions::workspace_sign_allowed(shown);
        }
        if self.ctx.batch() {
            // Stapel (ADR 0004 §3.13): „Alle signieren“ mit mindestens einem signierbaren Eintrag,
            // „Zur Stapelübersicht“ aus einem Einzeldokument mit weiteren Dokumenten
            actions.start_batch = crate::actions::batch_start_allowed(shown);
            actions.show_batch = crate::actions::batch_show_allowed(shown);
        }
        let signers = if self.ws.enabled { self.signers_view() } else { SignersView::default() };
        if self.ws.enabled && !signers.choice_offered && signers.selected.is_none() {
            // ohne Signiermittel-Auswahl und ohne Signiermittel (die Richtlinie schließt jedes aus, Nachtrag 20): nichts,
            // womit „Signieren“/„Alle signieren“ (Knopf, ⌘↵, Palette) signieren könnte – gesperrt statt ohne Wirkung
            actions.sign = false;
            actions.start_batch = false;
        }
        let document = if parked { None } else { self.document_view() };
        let batch = if parked { None } else { self.batch_view() };
        let done = self
            .state
            .results()
            .iter()
            .map(|o| DoneItemView {
                id: o.doc.id,
                name: o.doc.display_name.clone(),
                input: o.doc.path.clone(),
                result: o.result.clone(),
            })
            .collect();
        ViewState {
            step,
            main_bar,
            settings: self.settings_view.clone().map(|v| self.live_settings_view(v)),
            language: self.cfg.language(),
            accessibility: Some(self.cfg.settings.accessibility),
            notices: self.notices.clone(),
            migration: self.projected_migration(),
            done,
            platform: PlatformView::from(self.mobile.platform),
            resume: self.resume_offer.as_ref().filter(|_| matches!(self.state, State::Idle { .. })).map(|p| {
                ResumeOfferView {
                    name: p.record.name.clone(),
                    backend: p.record.backend,
                    age_minutes: p.age_minutes(pida_fs::system_now()),
                    pending: u32::try_from(p.record.pending.len()).unwrap_or(u32::MAX),
                    document_leaves_device: self.resume_offer_leaves_device,
                }
            }),
            keystore_import: self.keystore_import.clone(),
            privacy_text: self.cfg.privacy_text(),
            // ADR 0004: K1 document, signers (Arbeitsbereich), K2 batch, K3 recent, K4 first_run
            share_hint: self.share_hint_view(document.as_ref(), batch.as_ref()),
            document,
            signers,
            batch,
            recent: self.recent_view(),
            first_run: self.first_run_view(),
            actions,
            parked: self.parked_view(),
            about: Some(self.about_view()),
            update_hint: self.update_hint_view(),
        }
    }

    /// Einstellungsansicht zum Projizieren (ADR 0004 §3.11): die **aktuellen** Einstellungen des Kerns (auch
    /// nach Kernänderungen wie `last_source_directory`), Probleme = Hinweise der Prüfung und abgelehnte
    /// Felder, veröffentlichte `applied_revision`, Beispiele, Richtliniensperren. Nach einem abgelehnten
    /// klassischen `SaveSettings` die Arbeitskopie mit ihren Problemen.
    fn live_settings_view(&self, mut v: SettingsView) -> SettingsView {
        v.preview_revision = lock(&self.shared.block).revision;
        if !self.apply_state.draft {
            v.settings = self.cfg.settings.clone();
            let (mut problems, _) = self.validate_here(&v.settings);
            problems.extend(self.apply_state.rejected.clone());
            v.problems = problems;
        }
        v.read_only = self.cfg.read_only();
        v.applied_revision = self.apply_state.published;
        v.examples = crate::settings_apply::examples(&v.settings);
        v.locked = Some(crate::settings_apply::locked_paths(self.cfg.policy.as_ref()));
        // Amtssignatur ohne eigene Bildmarke: Standard ist der Platzhalter, nicht das PIDA-Siegel (Backlog Nr. 10)
        v.emblem_placeholder = v.settings.signature.profile == pida_config::SignatureProfile::Amtssignaturblock
            && v.settings.signature.emblem.is_none();
        if !self.cfg.keystore_offered {
            // iOS/Android nur mit ID Austria (ADR 0004 Nachtrag 11): keine Keystore-Angaben
            v.keystore_ready = false;
            v.keystore_format = None;
            v.keystore_entries = None;
            v.keystore_inspect_problem = None;
            return v;
        }
        // Keystore (Nachtrag ADR 0004): Format am Inhalt; als Standard wählbar nur mit PKCS#12
        let (format, entries, problem) = self.keystore_info.view_for(v.settings.keystore.file.as_deref());
        v.keystore_ready = v.settings.keystore.enabled && format == Some(crate::view::KeystoreFormatView::Pkcs12);
        v.keystore_format = format;
        v.keystore_entries = entries;
        v.keystore_inspect_problem = problem;
        v
    }

    /// Verlässt das Dokument mit diesem Signiermittel das Gerät (Capability der letzten Prüfung)? Steuert
    /// den Kurzhinweis im Signierbereich (ADR 0003).
    fn leaves_device(&self, backend: BackendId) -> bool {
        self.options
            .iter()
            .any(|o| o.id == backend && o.capabilities.as_ref().is_some_and(|c| c.document_leaves_device))
    }

    fn step_view(&self, state: &State) -> StepView {
        match state {
            State::Starting => StepView::Starting,
            State::Idle { .. } => StepView::Open { queue: self.queue_items() },
            State::Analyzing { .. } | State::Resuming { doc: None, .. } => {
                StepView::Loading { queue: self.queue_items() }
            }
            // Fortsetzen: Dokument geladen, Signiermittel wird geprüft
            State::Resuming { doc: Some(doc), plan, .. } => StepView::Signing {
                interaction: self.interaction_view(),
                queue: self.queue_items(),
                document_leaves_device: self.leaves_device(plan.backend),
                progress: self.progress_view(plan.backend, None, doc, None),
            },
            State::AnalysisWarning { warnings, doc, .. } => {
                StepView::Warning { doc: self.doc_info(doc.id), codes: warnings.clone() }
            }
            State::PlaceholderQuestion { found, doc, .. } => {
                let (fields, qr) = match found {
                    pida_workflow::PlaceholderFound::Fields(f) => (
                        f.iter().map(|c| FieldInfo { name: c.qualified_name.clone(), page: c.page_index }).collect(),
                        None,
                    ),
                    pida_workflow::PlaceholderFound::Qr(q) => {
                        (Vec::new(), Some(QrInfo { page: q.page_index, id: q.payload.id.clone() }))
                    }
                };
                StepView::Placeholder { doc: self.doc_info(doc.id), fields, qr }
            }
            State::Positioning { page, placement, new_page, doc, .. } => {
                let block = lock(&self.shared.block).clone();
                let placement = placement.or(doc.start).unwrap_or(Placement {
                    page_index: *page,
                    x: 0.0,
                    y: 0.0,
                    width: block.width,
                    height: block.height,
                });
                let mut announce = BTreeMap::new();
                announce.insert("page".into(), (placement.page_index + 1).to_string());
                announce.insert("pages".into(), (doc.page_count + usize::from(*new_page)).to_string());
                let mm = |pt: f32| format!("{:.0}", pt * 25.4 / 72.0);
                // Live-Region und Zahlenfelder nennen dieselben Kanten (PAGE_ABS): linke Kante und
                // Oberkante, jeweils von links bzw. von unten
                announce.insert("left".into(), mm(placement.x));
                announce.insert("top".into(), mm(placement.y));
                StepView::Position {
                    doc: self.doc_info(doc.id),
                    page: *page,
                    block: BlockView {
                        width: if block.doc == Some(doc.id) { block.width } else { placement.width },
                        height: if block.doc == Some(doc.id) { block.height } else { placement.height },
                        preview_revision: block.revision,
                        emblem_placeholder: block.emblem_placeholder,
                        emblem_choosable: true,
                        language: self.cfg.effective().block_language(Lang::system()),
                    },
                    placement,
                    new_page: *new_page,
                    announce,
                }
            }
            State::ChoosingBackend { options, .. } => {
                StepView::Backend { options: option_views(options, self.mobile.platform) }
            }
            State::Signing { backend, phase, doc, flow, .. } => StepView::Signing {
                interaction: self.interaction_view(),
                queue: self.queue_items(),
                document_leaves_device: self.leaves_device(*backend),
                progress: self.progress_view(*backend, Some(*phase), doc, Some(flow)),
            },
            State::Output { saved, decision, queue, backend, .. } => {
                let (ask_overwrite, save_dialog) = match &self.dialog {
                    Some(OutputDialog::Overwrite(p)) => (Some(p.clone()), None),
                    Some(OutputDialog::Save(d)) => (None, Some(d.clone())),
                    None => (None, None),
                };
                StepView::Output {
                    result: OutputView {
                        path: saved.as_ref().map(|s| s.path.clone()),
                        save_pending: saved.is_none() && decision.is_none(),
                        ask_overwrite,
                        save_dialog,
                        summary: saved.as_ref().and_then(|info| {
                            let caps =
                                self.options.iter().find(|o| o.id == *backend).and_then(|o| o.capabilities.as_ref());
                            self.signed_summary(info, *backend, caps)
                        }),
                    },
                    next: queue.pending.front().map(|d| self.doc_info(d.id)),
                    pending: queue.pending.len(),
                }
            }
            State::Error { error, retry, from, .. } => {
                let mut params = error.params();
                if let WorkflowError::Save(_) = error
                    && let Some(State::Output { decision: Some(OutputDecision::Save { path, .. }), .. }) =
                        from.as_deref()
                {
                    params.insert("arg1".into(), file_name(path));
                }
                // JKS/JCEKS: Konvertierungsassistent mit `keytool`-Aufruf (Plan §4.9)
                if let WorkflowError::Backend { code, detail } = error
                    && code == "backend-keystore-format"
                    && let Some(file) = self.cfg.effective().keystore_file()
                {
                    let format = match detail.as_str() {
                        "JKS" => pida_backend_keystore::KeystoreFormat::Jks,
                        "JCEKS" => pida_backend_keystore::KeystoreFormat::Jceks,
                        _ => pida_backend_keystore::KeystoreFormat::Unknown,
                    };
                    if let Some(hint) = pida_backend_keystore::conversion_hint(format, file) {
                        params.insert("hint".into(), hint);
                    }
                }
                StepView::Error {
                    code: error.code(),
                    params,
                    retry: retry.is_some(),
                    context: self.error_context(error, from.as_deref()),
                }
            }
            State::Finished { exit, .. } => StepView::Finished { exit_code: *exit as u8 },
            // ADR 0004 §3.8: Übersicht (Inhalt in `ViewState::batch`), Lauf als Signierkarte über der Übersicht
            State::BatchOverview { .. } => StepView::Batch,
            State::BatchSigning { queue, run } => StepView::Signing {
                // ohne offene Maske meldet core-api den Fortschritt des Laufs selbst (Dokument k von n); in
                // der Pause wertet die UI `interaction` nicht aus
                interaction: match self.interaction_view() {
                    InteractionView::Waiting { .. }
                        if !matches!(
                            run.phase,
                            pida_workflow::BatchPhase::OpeningSession | pida_workflow::BatchPhase::ReadingCertificate
                        ) =>
                    {
                        InteractionView::Waiting {
                            progress: pida_backend_api::Progress::Document {
                                index: (run.index + 1).min(run.order.len()),
                                total: run.order.len(),
                            },
                        }
                    }
                    other => other,
                },
                queue: self.queue_items(),
                document_leaves_device: self.leaves_device(run.backend),
                progress: self.batch_progress_view(queue, run),
            },
        }
    }
}

fn option_views(options: &[BackendOption], platform: Platform) -> Vec<BackendOptionView> {
    options
        .iter()
        .map(|o| {
            let mut notes = Vec::new();
            // Mobile: ID Austria ohne FIDO2 (Plan §4.7) – FIDO2 ausgegraut mit Hinweis
            if !platform.is_desktop()
                && o.id == BackendId::AtrustNative
                && o.capabilities.as_ref().is_none_or(|c| !c.factors.fido2)
            {
                notes.push(MSG_FIDO2_UNAVAILABLE_MOBILE.to_owned());
            }
            BackendOptionView { id: o.id, available: o.available(), capabilities: o.capabilities.clone(), notes }
        })
        .collect()
}

/// Sicherung des vorbereiteten Zustands nach `prepare` (Mobile, „Signatur fortsetzen“).
struct PersistJob {
    dir: PathBuf,
    name: String,
    backend: BackendId,
    pending: Vec<PathBuf>,
    /// Stapellauf: Regel für vorhandene Zieldateien
    on_existing: Option<ExistingPolicy>,
}

impl PersistJob {
    /// Sichert; Fehler sind nicht fatal (dann ohne „Fortsetzen“).
    fn save(&self, data: &DocData, prepared: &dyn PreparedDoc, position: &PositionChoice) {
        let src = data.src.bytes();
        let now = pida_fs::system_now();
        let (pending, on_existing) = (self.pending.clone(), self.on_existing);
        match resume::persist(&self.dir, &self.name, src, prepared, position, self.backend, pending, on_existing, now) {
            Ok(()) => tracing::info!("vorbereiteter Zustand für „Signatur fortsetzen“ gesichert"),
            Err(e) => {
                tracing::warn!(error = %e, "vorbereiteter Zustand nicht gesichert – ohne „Signatur fortsetzen“");
                resume::delete(&self.dir);
            }
        }
    }
}

/// Quelle der laufenden Signaturrunde: das Original bzw. – in der Mehrfachsignatur ab der zweiten Runde – das
/// eben signierte Zwischenergebnis ([`Effect::NextSignature`]), neu geöffnet und analysiert (Seiten, Felder und
/// vorhandene Signaturen wie bei einem geladenen Dokument). Es bleibt nur im Speicher; bricht der Vorgang ab,
/// wird es verworfen.
fn chained_source(shared: &Shared, epoch: u64, original: &Arc<DocData>) -> Result<Arc<DocData>, Box<Event>> {
    let signed = {
        let mut slot = lock(&shared.sign);
        if slot.epoch != epoch {
            return Err(Box::new(Event::BackendFailed(BackendError::Cancelled)));
        }
        if !slot.chain_pending {
            return Ok(slot.chain_doc.clone().unwrap_or_else(|| original.clone()));
        }
        slot.signed.take()
    };
    let Some(signed) = signed else {
        return Err(Box::new(Event::SigningFailed(WorkflowError::Unexpected("Zwischenergebnis fehlt".into()))));
    };
    let src = pida_pdf_cos::SourcePdf::open(signed.bytes(), &pida_pdf_cos::Limits::default())
        .map_err(|e| Box::new(Event::SigningFailed(crate::docs::load_error(&e))))?;
    let report =
        shared.engine.analyze(&src).map_err(|e| Box::new(Event::SigningFailed(crate::docs::analysis_error(&e))))?;
    let data = Arc::new(DocData::new(original.id, PathBuf::from(&original.name), Arc::new(src), report));
    let mut slot = lock(&shared.sign);
    if slot.epoch != epoch {
        return Err(Box::new(Event::BackendFailed(BackendError::Cancelled)));
    }
    slot.chain_doc = Some(data.clone());
    slot.chain_pending = false;
    Ok(data)
}

/// Vorbereitetes Dokument in den Signiervorgang übernehmen (verworfen, wenn der Vorgang inzwischen
/// abgebrochen wurde).
fn store_prepared(
    shared: &Shared,
    epoch: u64,
    prepared: Box<dyn PreparedDoc>,
    cms_profile: pida_cades::CmsProfile,
) -> Event {
    let mut slot = lock(&shared.sign);
    if slot.epoch != epoch {
        return Event::BackendFailed(BackendError::Cancelled);
    }
    slot.prepared = Some(prepared);
    slot.cms_profile = Some(cms_profile);
    Event::PhaseCompleted(pida_workflow::SigningPhase::Preparing)
}

/// Entfernt die Kopie `source` der Dateiauswahl, wenn sie im Cache der App liegt (nie das Ziel `kept`, nie
/// Dateien außerhalb des Caches – etwa eine vom Desktop übergebene Datei); ein leerer Unterordner
/// (`import/<n>/`) geht mit.
fn remove_cache_copy(source: &Path, kept: &Path, cache_dir: &Path) {
    let (Ok(source), Ok(cache)) = (pida_fs::canonicalize(source), pida_fs::canonicalize(cache_dir)) else { return };
    if !source.starts_with(&cache) || source == cache || pida_fs::canonicalize(kept).is_ok_and(|k| k == source) {
        return;
    }
    match pida_fs::remove_file(&source) {
        Ok(()) => {
            if let Some(parent) = source.parent().filter(|p| *p != cache) {
                let _ = pida_fs::remove_dir(parent);
            }
        }
        Err(e) => tracing::warn!(kind = ?e.kind(), "Kopie der Dateiauswahl nicht entfernt"),
    }
}

/// Neuer Ort einer in die Sandbox importierten Datei nach einem Umzug der Sandbox: `old` fehlt, lag in
/// einem Ordner mit dem Namen von `dir` (z. B. `keystores`), und `dir/<Dateiname>` ist vorhanden.
pub(crate) fn relocated(old: &Path, dir: &Path) -> Option<PathBuf> {
    if old.fs_is_file() || old.parent()?.file_name()? != dir.file_name()? {
        return None;
    }
    let new = dir.join(old.file_name()?);
    (new != old && new.fs_is_file()).then_some(new)
}

/// Dateiname ohne Pfadanteile und Steuerzeichen; leer → `fallback`.
fn sanitize_file_name(name: &str, fallback: &str) -> String {
    let cleaned: String =
        name.chars().map(|c| if c.is_control() || matches!(c, '/' | '\\' | ':' | '\0') { '_' } else { c }).collect();
    let trimmed = cleaned.trim().trim_start_matches('.');
    if trimmed.is_empty() { fallback.to_owned() } else { trimmed.to_owned() }
}

/// Schreibt eine private Datei atomar (Ordner 0700, Datei 0600).
fn write_private_file(dir: &Path, target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    pida_fs::create_dir_all(dir)?;
    let tmp = target.with_extension("part");
    pida_fs::write(&tmp, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        pida_fs::set_permissions(dir, pida_fs::Permissions::from_mode(0o700))?;
        pida_fs::set_permissions(&tmp, pida_fs::Permissions::from_mode(0o600))?;
    }
    pida_fs::rename(&tmp, target)
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string())
}

fn panic_text(e: crate::rt::JoinError) -> String {
    if !e.is_panic() {
        return "abgebrochen".into();
    }
    let p = e.into_panic();
    p.downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "Panik".into())
}

/// Panik beim Vorbereiten/Abschließen → unerwarteter Fehler (Wiederholen möglich).
fn signing_panic(text: String) -> Event {
    Event::SigningFailed(WorkflowError::Unexpected(text))
}

fn prepare_error(code: &str, detail: &str) -> WorkflowError {
    WorkflowError::Prepare { code: code.to_owned(), detail: detail.to_owned() }
}

/// Fehler von `prepare` (PDF-AS: `error.PrepareDocument` bzw. 11021).
fn pades_error(e: &PadesError) -> WorkflowError {
    match e {
        PadesError::Rejected(_) => WorkflowError::Rejected {
            class: pida_pades::AcceptanceClass::K5Rejected,
            code: "ng-document-rejected".into(),
        },
        PadesError::FieldAlreadySigned(_) => WorkflowError::Rejected {
            class: pida_pades::AcceptanceClass::K4Restricted,
            code: "ng-field-already-signed".into(),
        },
        PadesError::FieldLocked(_) => {
            WorkflowError::Rejected { class: pida_pades::AcceptanceClass::K4Restricted, code: "ng-field-locked".into() }
        }
        PadesError::CertificateNotValid => prepare_error("ng-certificate-not-valid", ""),
        PadesError::SigBlock(s) => prepare_error(s.code(), &s.to_string()),
        PadesError::Cms(c) => prepare_error(c.code(), &c.to_string()),
        other => prepare_error("error-PrepareDocument", &other.to_string()),
    }
}

/// Modell A (ADR 0001 §8.3): Hashverfahren aus dem Schlüssel, Hash der ByteRange-Daten,
/// SignedAttributes, Hash der Attribute → `DigestJob`.
fn digest_job(
    prepared: &dyn PreparedDoc,
    chain: &CertChain,
    profile: pida_cades::CmsProfile,
    name: &str,
) -> Result<(DigestJob, SignedAttrs), pida_cades::CmsError> {
    let key = chain.signer.key_type()?;
    let alg = pida_cades::select_algorithm(&key)?.digest();
    let message_digest = prepared.digest(alg);
    let attrs = SignedAttrsBuilder::new(profile, &message_digest, &chain.signer, SigningTime(prepared.signing_time()))
        .build()?;
    let digest = attrs.digest(alg);
    Ok((DigestJob { display_name: name.to_owned(), alg, digest: digest.value }, attrs))
}

/// Startposition der Positionierung (letzte Seite, 50 %/25 %) mit der Blockgröße des Profils.
/// Eignung der leeren Signaturfelder für die **automatische** Wahl (Platzhaltersuche „Leere Signaturfelder“, Stapel;
/// ADR 0004 Nachtrag 22): Einrastregel [`pida_placement::field_suitable`] für den Block nach den aktuellen
/// Einstellungen – wie die Markierungen im Arbeitsbereich (Nachtrag 16). Ohne Profile bzw. ohne sichtbaren Block
/// bleiben alle Felder `auto_ok` (bisheriges Verhalten).
pub(crate) fn mark_auto_fields(shared: &Shared, snap: &Snapshot, data: &DocData, fields: &mut [EmptyFieldCandidate]) {
    let Some(store) = shared.profiles.as_deref() else { return };
    let input = SpecInput {
        effective: snap.effective(),
        system_language: Lang::system(),
        pdfa: data.report.pdfa.as_ref(),
        cert: None,
        signing_time: plan::local_now(),
        emblem: None,
    };
    let Ok(spec) = plan::block_spec(store, input) else { return };
    if !spec.visible() {
        return;
    }
    // Blockgröße je Seite nur einmal bauen (Dokumente mit vielen Feldern)
    let mut blocks: std::collections::HashMap<usize, Option<pida_placement::BlockGeometry>> = Default::default();
    for f in fields.iter_mut() {
        let Some((page, info)) = f.page_index.and_then(|p| data.pages.get(p).map(|i| (p, i))) else { continue };
        let block = *blocks
            .entry(page)
            .or_insert_with(|| plan::block_geometry(&spec, &data.pages, page, store, &shared.fonts).ok());
        let Some(block) = block else { continue };
        let rect = pida_placement::user_rect_to_visible(info, f.rect.normalized());
        f.auto_ok = pida_placement::field_suitable(rect, block, info.visible_size());
    }
}

fn start_placement(shared: &Shared, snap: &Snapshot, data: &DocData, store: &ProfileStore) -> Option<Placement> {
    let spec: BlockSpec = plan::block_spec(
        store,
        SpecInput {
            effective: snap.effective(),
            system_language: Lang::system(),
            pdfa: data.report.pdfa.as_ref(),
            cert: None,
            signing_time: plan::local_now(),
            emblem: None,
        },
    )
    .ok()?;
    match plan::start_placement(&spec, &data.pages, store, &shared.fonts) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(code = %e.code(), "Startposition nicht berechenbar");
            None
        }
    }
}

/// `DocumentSource` um ein vorbereitetes Dokument (Modell B, ADR 0001 §5.3). Das Backend darf es
/// nicht über `sign` hinaus halten; danach übernimmt der Kern es wieder exklusiv.
pub(crate) struct PreparedSource {
    name: String,
    inner: Box<dyn PreparedDoc>,
}

impl std::fmt::Debug for PreparedSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedSource").field("name", &self.name).field("len", &self.inner.total_len()).finish()
    }
}

impl DocumentSource for PreparedSource {
    fn display_name(&self) -> &str {
        &self.name
    }
    fn len(&self) -> u64 {
        self.inner.total_len()
    }
    fn excluded_range(&self) -> ExcludedByteRange {
        let r = self.inner.excluded_range();
        ExcludedByteRange { from: r.from, to: r.to }
    }
    fn open_blacked_out(&self) -> std::io::Result<Box<dyn std::io::Read + Send + '_>> {
        Ok(self.inner.blacked_out())
    }
}

enum PreparedHolder {
    Owned(Box<dyn PreparedDoc>),
    Shared(Arc<PreparedSource>),
}

impl PreparedHolder {
    fn into_inner(self) -> Option<Box<dyn PreparedDoc>> {
        match self {
            PreparedHolder::Owned(p) => Some(p),
            PreparedHolder::Shared(arc) => Arc::try_unwrap(arc).ok().map(|s| s.inner),
        }
    }
}

impl DocData {
    /// Wie `with_renderer`, aber ohne Renderer (bzw. bei Renderfehlern) mit `None`.
    pub(crate) fn with_renderer_opt<T>(
        &self,
        backend: Option<&dyn RenderBackend>,
        f: impl FnOnce(Option<&dyn pida_render::Renderer>) -> T,
    ) -> T {
        match backend {
            Some(b) => {
                let mut f = Some(f);
                match self.with_renderer(b, |r| (f.take().expect("einmal"))(Some(r))) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::debug!(code = e.code(), "Renderer nicht verfügbar");
                        (f.take().expect("nicht aufgerufen"))(None)
                    }
                }
            }
            None => f(None),
        }
    }
}
