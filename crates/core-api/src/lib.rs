//! `pida-core-api` – Fassade des Kerns für Shell (Tauri 2), CLI und Plan-B-Shell
//! (uniffi-tauglich, ohne Tauri-Abhängigkeit; Plan §2.5, §3.4).
//!
//! Die UI hält **keinen Fachzustand**: Sie projiziert nur [`ViewState`] und sendet [`UiEvent`].
//! Beide Typen sind serde-serialisierbar (`tag`-basiert) für die Tauri-Bridge
//! (`tauri-specta`/`ts-rs` erzeugen daraus die TypeScript-Typen).
//!
//! # Aufbau
//!
//! - [`Core::new`] lädt die Einstellungen (inkl. `-c`/Altdateien, Migrationsangebot, Richtlinie),
//!   bindet PDFium, registriert die Backends (ID Austria Szenario A, lokale BKU mit
//!   Erreichbarkeits-Polling 5 s/30 s auf dem Desktop, PKCS#12-Keystore mit Schlüsselbund-Adapter)
//!   und startet den Kern-Task (Tokio).
//! - Der Kern-Task wendet den Reducer aus `pida-workflow` an und führt die Effekte aus: Dokument
//!   laden und analysieren, Platzhaltersuche, Seitenkacheln (LRU), Blockvorschau aus dem echten
//!   Appearance-Objekt, Signierplan, Backend-Sitzung mit [`InteractionView`]s als Kanal zur UI,
//!   `finish`, Selbstprüfung, Ausgabe.
//! - Fehler erreichen die UI als i18n-Code mit Parametern ([`StepView::Error`], [`NoticeView`]).
//!
//! Die PAdES-Engine wird über [`engine::PadesEngine`] angesprochen; Tests setzen über
//! [`CoreOptions::engine`] eine Fake-Engine ein.
//!
//! # Bridge-Vertrag (Shell ↔ Kern)
//!
//! | Schritt (`ViewState::step`) | Anzeige | Antworten der UI |
//! |---|---|---|
//! | `open` | Drop-Zone, Warteschlange | `open_files` (nicht vorhandene Pfade werden ignoriert, F-12) |
//! | `loading` | Dokument wird geladen/analysiert | `cancel`, `navigate{open}` |
//! | `warning` | Warnung der Analyse (`codes`, z. B. `pdf-usage-rights`) mit Abbruchmöglichkeit | `answer_warning{proceed}`, `cancel` |
//! | `placeholder` | „Platzhalter verwenden?“ (Felder bzw. QR) | `placeholder_answer{answer, field_index}` |
//! | `position` | Seite (`pida-render://` → [`Core::page_tile`]), Block ([`Core::block_preview`], `preview_revision`), `announce` | `move_block`, `nudge_block`, `change_page`, `toggle_new_page`, `auto_place`, `confirm_position`, `choose_invisible` |
//! | `backend` | Optionen; bei `capabilities.document_leaves_device` Kurzhinweis „wird an A-Trust übermittelt“ (nicht blockierend, ADR 0003) | `choose_backend` |
//! | `signing` | `interaction`: `waiting`/`form`/`secret`/`notice`; `document_leaves_device` = Kurzhinweis | `submit_form`, `submit_secret`, `answer_notice`, `open_signature_data`, `cancel` |
//! | `output` | Ergebnis; `ask_overwrite` = Rückfrage, `save_dialog` = Speichern-Dialog öffnen, `save_pending` = Knopf „Speichern“ | `confirm_overwrite`, `save{path}` (`null` = Dialog abgebrochen bzw. Knopf), `open_result`, `share_result`/`export_result` (Mobile), `navigate` |
//! | `error` | `code` + `params` (i18n), `retry` | `retry`, `cancel` |
//! | `finished` | Exit-Code | – |
//!
//! Querschnitt: `notices` (Hinweise ohne Rückfrage, `dismiss_notice{id}`), `migration`
//! (`accept_migration{store_secrets}`/`decline_migration`), `settings` (Overlay: `open_settings`,
//! `save_settings{settings}` – unbekannte Keys unverändert zurückgeben –, `close_settings`,
//! `reset_settings`), `done` (Ergebnisliste), `main_bar` (Matrix docs/analyse/01 §3.5), `platform`
//! (Desktop/Mobile), `resume` („Signatur fortsetzen“: `resume_signature`/`discard_resume`),
//! `keystore_import` (Ergebnis von `import_keystore{path, password, remember}`, schließen mit
//! `dismiss_keystore_import`; seit ADR 0004 Nachtrag 11 auf Mobilgeräten ohne Wirkung), `privacy_text` (eigener Datenschutztext per Richtlinie für Einstellungen ›
//! Datenschutz). Lebenszyklus: `backgrounded`/`foregrounded` (kein Abbruch). `acknowledge_privacy` ist
//! veraltet und ohne Wirkung (kein Zustimmungsschritt mehr, ADR 0003).
//! Dateien öffnen bzw. URLs im Browser, Teilen und Exportieren übernimmt die Shell über [`ShellHooks`]
//! ([`Core::set_shell`]).
//!
//! # Mobile (Plan §3.5, §5.2; docs/integration/mobile.md)
//!
//! - **Lebenszyklus:** `UiEvent::Backgrounded`/`Foregrounded` sind kein Abbruch. Nach der Rückkehr
//!   beantwortet der Kern eine wartende App-/QR-Maske mit „weiter“ – das Backend lädt die A-Trust-Seite
//!   neu und startet den Long-Poll frisch (die alte Verbindung hat das System oft getrennt).
//! - **Persistenz:** mit [`CoreOptions::persist_prepared`] (Standard auf iOS/Android) sichert der Kern
//!   nach `prepare` den vorbereiteten Zustand in der Sandbox (`resume`-Modul, `pades::Prepared::persist`);
//!   nach einem App-Kill bietet er „Signatur fortsetzen“ an und stellt das Dokument mit
//!   `Prepared::restore` wieder her. `Effect::DeletePersisted` löscht ihn nach Erfolg/Abbruch/Fehler.
//! - **Ausgabe:** [`CoreOptions::default_output_dir`] (App-Dokumente) statt Speichern-Dialog;
//!   Teilen/Exportieren über [`ShellHooks::share_path`]/[`ShellHooks::export_path`].
//! - **Signiermittel:** nur ID Austria (ADR 0004 Nachtrag 11, Modul `mobile_signers`): keine lokale BKU,
//!   kein Keystore (nicht registriert, nirgends angeboten; `import_keystore`/`inspect_keystore` und jede Wahl
//!   des Keystores ohne Wirkung); FIDO2 nur mit Hinweis `ng-fido2-unavailable-mobile`
//!   (`BackendOptionView::notes`). Ein gespeicherter Standard Keystore gilt als ID Austria, die
//!   Keystore-Werte der Datei bleiben unberührt; Keystore-Passwörter früherer Builds entfernt der Kern einmalig.
//! - **Secrets:** die Shell setzt [`CoreOptions::secret_store`] auf `pida_config::ShellSecretStore`
//!   (Keychain bzw. Android Keystore); das ID-Austria-Passwort wird nie persistiert.
//!
//! # Vertrag Kern ↔ Oberfläche (Welle 3)
//!
//! `view.rs` ist die **einzige Quelle**: die TypeScript-Typen der Oberfläche
//! (`apps/app/src/core/generated.ts`) entstehen daraus mit ts-rs (Modul `ts`, Feature `ts`); der Test
//! `tests/ts_bindings.rs` schlägt bei jeder Abweichung fehl. Entscheidungen zu den Vorschlägen der
//! Oberfläche (`extensions.ts`, entfernt):
//!
//! | Vorschlag der UI | Entscheidung |
//! |---|---|
//! | `OutputView.prompt` (`save_path`/`overwrite`) | ersetzt durch die vorhandenen Felder [`OutputView::save_dialog`] (Startordner + Namensvorschlag) und [`OutputView::ask_overwrite`]; Antworten `save{path}` bzw. `confirm_overwrite` |
//! | `ViewState.notification` (ein Toast mit `id`) | ersetzt durch [`ViewState::notices`] (Liste, älteste zuerst, schließen mit `dismiss_notice{id}`) – mehrere Hinweise gehen nicht verloren |
//! | `UiEvent::ChooseInvisible` | übernommen: [`UiEvent::ChooseInvisible`] → `Event::InvisibleChosen` → unsichtbare Signatur nur für dieses Dokument |
//! | Schlüssel von `SettingsView::problems` | TOML-Punktpfade der Felder (`proxy.host`, `output.folder`, `keystore.file`, `signature.emblem` …) |
//! | `Position.announce` | Schlüssel `page`, `pages`, `left` (linke Kante), `top` (Oberkante von unten, wie das Feld Y; mm, ganzzahlig) |
//! | Migrationsassistent | [`MigrationOfferView::plaintext_secrets`] ergänzt: die UI fragt nur dann nach der Übernahme der Passwörter in den Schlüsselbund |
//!
//! # Neue Oberfläche „Arbeitsbereich“ (ADR 0004, Vertrag E0)
//!
//! Alle Bridge-Typen und Ereignisse aus ADR 0004 §3 stehen additiv in `view.rs` (neue Felder mit
//! `#[serde(default)]`, neue Varianten am Ende): [`ViewState::document`], `signers`, `batch`,
//! `recent`, `first_run`, `actions`; `StepView::Signing::progress`, `StepView::Error::context`,
//! [`StepView::Batch`]; [`OutputView::summary`]; `SettingsView::{applied_revision, examples}`;
//! [`MigrationOfferView::items`] sowie 23 neue [`UiEvent`]s (`set_invisible` … `offer_migration`).
//! Gemeinsame Typen von Reducer und Bridge liegen in `pida-workflow` (`PlacementOrigin`, `ExistingPolicy`,
//! `BatchOutput`). Klassischer Ablauf und CLI bleiben unverändert.
//!
//! **Umgesetzt (K1, Etappe E1)** mit [`CoreOptions::workspace_ui`] (Shell: `PIDA_UI=workspace` in
//! Debug-Builds): Arbeitsbereich-Modus des Reducers (`Context::workspace`, Startargument-Automatik
//! `cli_auto`), Invariante A1 (eine Sitzung nur nach `sign_with`, Rückfall-Wahl, „Signatur fortsetzen“,
//! „Erneut versuchen“ bzw. D-06 mit einem Signiermittel ohne `document_leaves_device`),
//! [`ViewState::document`] (Sticker, Felder, QR, vorhandene Signaturen, Einschränkungen, Layout aus dem
//! Belegungsraster, Live-Prüfung), `set_invisible`, `use_target`, `inspect_pages`, automatische
//! Startplatzierung (PDF-AS-Regel), [`ViewState::signers`] mit `select_signer`, `sign_with`,
//! `StepView::Signing::progress`, `StepView::Error::context`, Abbruch zurück in den Arbeitsbereich
//! (`ng-signing-cancelled`), Ablehnung in der App (`ng-signing-declined`), Wiederholen in derselben Sitzung
//! (`sign_one`, `ReleaseDocument`), [`OutputView::summary`] und `set_ui_language` (beide Modi).
//!
//! **Umgesetzt (K2, Etappe E2)** mit [`CoreOptions::batch_overview`] (Shell: `PIDA_UI=workspace,batch` in
//! Debug-Builds; Standard aus, eingeschaltet wird im Merge-Commit von A2): ab zwei Dokumenten die
//! Stapelübersicht [`ViewState::batch`] mit Voranalyse je Datei (Seiten, Größe, Status inkl. abgelehnt,
//! Analysewarnung und QR-Platzhalter, Miniatur über [`Core::thumbnail`] bzw. `pida-render://thumb/<doc>`),
//! Schätzung „n Dokumente · Anmeldungen · Bestätigungen“ (Zertifikats-Cache, gemerktes Passwort),
//! `open_batch_item`/`back_to_batch` mit eigenem Plan je Dokument, `start_batch` mit Ordnerwahl und Umgang
//! mit vorhandenen Dateien, ein Lauf mit **einer** Sitzung (`sign_one` je Dokument, höchstens ein
//! Zertifikat), Pausen nach `ErrorScope` (Verbindung ohne neue Anmeldung, Sitzung mit neuer Sitzung erst nach
//! „Erneut versuchen“, Dokument), `skip_batch_document` (Sitzung bleibt), `abort_batch` bzw. `cancel` (F-72),
//! Zusammenfassung über alle Einträge; Mobile setzt nach einem App-Kill nur das unterbrochene Dokument fort,
//! danach die Übersicht. Die Startargument-Automatik (D-06) bleibt ohne Übersicht.
//!
//! „Zuletzt signiert“ und „sofort wirksam“ (K3): siehe `recent.rs`, `settings_apply.rs`.
//!
//! „PDF hinzufügen“ (ADR 0004 Nachtrag 9, nur im Arbeitsbereich-Modus): `open_files { mode: add }` („Weitere
//! PDFs hinzufügen“) nimmt die Dateien zum offenen Dokument – mit Stapelübersicht werden alle zusammen zum
//! Stapel, die Platzierung des offenen Dokuments bleibt dessen Plan; `open_files { mode: replace }` („Anderes PDF
//! öffnen“) schließt das offene Dokument und öffnet die Dateien wie in der Dokumentauswahl. Ohne `mode` wie
//! bisher. Keines öffnet eine Sitzung (A1).
//!
//! „Zur Startseite“ (ADR 0004 Nachtrag 10, nur im Arbeitsbereich-Modus): `go_home` parkt das offene Dokument bzw.
//! den Stapel (Reducer, `Queue::parked`) statt es zu verwerfen; die Projektion zeigt dann die Dokumentauswahl mit
//! [`ViewState::parked`] (`parked.rs`). `resume_parked` setzt fort (ein Stapel in der Stapelübersicht, Nachtrag 13;
//! sonst unverändert), `discard_parked` verwirft; neue Dateien
//! kommen mit `mode: add` bzw. ohne `mode` („Öffnen mit“, Hinweis) zum Stapel, `mode: replace` beginnt neu. Nichts
//! davon öffnet eine Sitzung (A1).
//!
//! Ersteinrichtung (K4, §3.12, `first_run.rs`): nur bei fehlender Einstellungsdatei, Übernahmeliste per
//! Probelauf, Neuinstallation „neben dem Original“, `update_first_run`/`finish_first_run`/`offer_migration`.
//! Keines dieser Ereignisse öffnet eine Sitzung. Im klassischen Ablauf (ohne `workspace_ui`) bleiben die
//! Ereignisse des Arbeitsbereichs und der Ersteinrichtung ohne Wirkung, ohne `batch_overview` die des Stapels.
//!
//! Kein Prüfmodus (ADR 0004 Nachtrag 15): Die App prüft keine fremden Signaturen mehr – wie PDF-Over verweist
//! sie auf signaturpruefung.gv.at. Vorhandene Signaturen erkennt der Kern weiter beim Signieren
//! ([`ExistingSignatureView`], Sperren, Zertifizierung). „Zuletzt signiert“ bietet, was „Fertig“ kann:
//! `open_recent`, `reveal_recent` (Desktop), `share_recent` (Mobile) und `sign_recent` („Erneut signieren“ wie
//! `open_files`); keines öffnet eine Sitzung. `pades::verify_document` bleibt als Bibliothek (Tests, Golden- und
//! Interop-Tests); das Entwickler-Werkzeug `pida verify` prüft mit [`tools::verify_file`].
//!
//! „Über PIDA“ und Update-Prüfung (ADR 0004 Nachtrag 21, [`update`]): [`ViewState::about`] mit Version,
//! Versionsdatum (beim Bauen eingebettet), Links und [`UpdateCheckView`]; geprüft wird nur am Desktop mit Oberfläche
//! (GitHub-Releases über `pida-net`, beim Start höchstens einmal je 24 h im Hintergrund, beim Öffnen von „Über PIDA“
//! bzw. auf Klick mit `check_for_updates`), nie mit Richtlinie „aus“; Transport austauschbar
//! ([`CoreOptions::update_transport`]). Lädt und installiert nichts.
//!
//! Ownership: **Integration**.

mod actions;
mod backends;
mod batch;
mod broker;
mod config;
mod docs;
pub mod engine;
mod first_run;
mod keystore_info;
pub mod logging;
pub mod messages;
mod mobile_signers;
mod opening;
mod parked;
pub mod plan;
pub mod qr_placeholder;
mod recent;
mod resume;
mod rt;
mod runtime;
mod settings_apply;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
pub mod tools;
#[cfg(feature = "ts")]
pub mod ts;
pub mod update;
mod view;
mod workspace;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub use backends::{SecretStoreCache, password_storage};
pub use broker::ShellHooks;
pub use docs::MAX_DOCUMENT_BYTES;
pub use view::{
    BackendOptionView, BlockView, DocInfo, DoneItemView, FieldInfo, FormAnswerView, InteractionView, KeystoreEntryView,
    KeystoreFormatView, KeystoreImportView, MainBarView, MigrationOfferView, NoticeView, OutputView, PlatformOs,
    PlatformView, QrInfo, QueueItemView, ResumeOfferView, SaveDialogView, SettingsView, StepView, UiEvent, ViewState,
};
// ADR 0004 (neue Oberfläche „Arbeitsbereich“, Vertrag E0)
pub use view::{
    ActionsView, BatchEstimateView, BatchItemStatusView, BatchItemView, BatchOutputView, BatchPauseView,
    BatchSummaryView, BatchView, CertificateSource, ConfirmMethod, DocConstraintsView, DocRefView, DocumentPhase,
    DocumentView, ErrorContextView, ErrorOrigin, ExistingSignatureView, FieldTargetView, FirstRunView, GuideAxis,
    GuideKind, GuideView, MigrationItemView, OutputModeView, PageLayoutView, PauseScope, PlacementCheckView,
    PlacementTarget, PositionSummaryView, PriorStatusView, QrTargetView, RecentItemView, RectPt, SignedSummaryView,
    SignerView, SignersView, SigningProgressView, SigningStage,
};
// Nachtrag K3 (Einstellungen › Dokument)
pub use view::{DocumentDetectionView, PdfaDecision, PdfaIdView};
// Nachtrag 9 („PDF hinzufügen“)
pub use view::OpenFilesMode;
// Nachtrag 10 („Zur Startseite“ parkt offene Arbeit)
pub use view::ParkedWorkView;
// Nachtrag 12 (Randfälle aus dem Mobile-Check: Stapelwortlaut je Signiermittel, Hinweis zum Teilen auf iOS)
pub use view::{BatchFlowView, ShareHintView};
// Nachtrag 21 („Über PIDA“ und Update-Prüfung)
pub use view::{AboutView, UpdateCheckState, UpdateCheckView, UpdateHintView};

use pida_backend_api::{Platform, SignatureBackend};
use pida_config::{Overlay, Paths, Policy, SecretStore};
use pida_i18n::Lang;
use pida_render::RenderBackend;
use pida_workflow::{DocId, ExitCode};
use tokio::sync::{mpsc, watch};

use crate::runtime::{CoreTask, Msg, Shared};

/// Fehler der Fassade.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// Konfiguration
    #[error(transparent)]
    Config(#[from] pida_config::ConfigError),
    /// Renderer
    #[error(transparent)]
    Render(#[from] pida_render::RenderError),
    /// Dokument unbekannt
    #[error("Dokument {0:?} unbekannt")]
    UnknownDocument(DocId),
    /// Kern beendet
    #[error("Kern beendet")]
    Stopped,
    /// Keine Blockvorschau (unsichtbares Profil bzw. noch nicht gebaut)
    #[error("keine Blockvorschau verfügbar")]
    NoPreview,
    /// Tokio-Laufzeit nicht erstellbar
    #[error("Laufzeit: {0}")]
    Runtime(String),
    /// Prüfung (`verify`) nicht möglich (Datei nicht lesbar)
    #[error("Prüfung nicht möglich: {0}")]
    Verify(String),
    /// QR-Platzhalter nicht erstellbar (Meldungs-ID)
    #[error("QR-Platzhalter: {0}")]
    QrPlaceholder(&'static str),
}

impl CoreError {
    /// Stabile Meldungs-ID.
    pub fn message_id(&self) -> &'static str {
        match self {
            CoreError::Config(e) => e.message_id(),
            CoreError::Render(e) => e.code(),
            CoreError::UnknownDocument(_) => "ng-unknown-document",
            CoreError::Stopped => "ng-core-stopped",
            CoreError::NoPreview => "ng-no-preview",
            CoreError::Runtime(_) => "error-Unexpected",
            CoreError::Verify(_) => "ng-verify-failed",
            CoreError::QrPlaceholder(id) => id,
        }
    }
}

/// Start-Konfiguration.
#[derive(Debug, Clone)]
pub struct CoreConfig {
    /// Pfade (Plattform oder Sandbox)
    pub paths: Paths,
    /// CLI-Overlay
    pub overlay: Overlay,
    /// Plattform
    pub platform: Platform,
    /// Ordner der PDFium-Bibliothek (`None` = neben der Binary/System)
    pub pdfium_dir: Option<PathBuf>,
    /// ohne UI (CLI `--headless`)
    pub headless: bool,
    /// Plattform-FIDO2 (nur Desktop; von der Shell bzw. aus `backend-atrust`-Features)
    pub fido2: Option<Arc<dyn pida_backend_atrust::Fido2Provider>>,
    /// App-Version (User-Agent der lokalen BKU, Über-Dialog)
    pub app_version: String,
}

/// Erweiterte Optionen (additiv zu [`CoreConfig`]; Shell, CLI und Tests).
#[derive(Clone)]
pub struct CoreOptions {
    /// Backends dürfen über den Broker fragen (Masken, Passwörter, Meldungen). `None` = `!headless`.
    /// Die CLI mit Terminal setzt `Some(true)` trotz `headless` (Terminal-Broker).
    pub interactive: Option<bool>,
    /// PAdES-Engine (Standard: `pida-pades`)
    pub engine: Option<Arc<dyn engine::PadesEngine>>,
    /// fest vorgegebene Backends statt der Registrierung (Tests, Einbettung)
    pub backends: Option<Vec<Arc<dyn SignatureBackend>>>,
    /// ID Austria über dieses Backend statt des nativen A-Trust-Ablaufs (Browser: A-Trust-Overlay mit
    /// Rückkanal, `pida-backend-atrust-web`); die übrigen Backends kommen weiter aus der Registrierung
    pub id_austria_backend: Option<Arc<dyn SignatureBackend>>,
    /// Secret-Speicher (Standard: Schlüsselbund der Plattform, sonst RAM)
    pub secret_store: Option<Arc<dyn SecretStore>>,
    /// Richtlinie der Shell (Registry, Managed Preferences) – hat Vorrang vor der Datei
    pub policy: Option<Policy>,
    /// Richtliniendatei des Systems lesen (`Policy::load_system`)
    pub load_system_policy: bool,
    /// Renderer (Standard: PDFium aus [`CoreConfig::pdfium_dir`])
    pub render: Option<Arc<dyn RenderBackend>>,
    /// PDFium binden, wenn kein Renderer vorgegeben ist
    pub use_pdfium: bool,
    /// Shell-Funktionen (Öffnen von Dateien/URLs)
    pub shell: Option<Arc<dyn ShellHooks>>,
    /// Systemsprache der Shell (`Lang::set_system`)
    pub system_language: Option<Lang>,
    /// Logdatei einrichten (mit Rotation; ein bereits gesetzter Subscriber bleibt)
    pub init_logging: bool,
    /// Log-Stufe
    pub log_level: String,
    /// Altordner `~/.pdf-over` (Migration, `-c`); `None` mit `detect_legacy_dir` = automatisch
    pub legacy_dir: Option<PathBuf>,
    /// Altordner automatisch suchen
    pub detect_legacy_dir: bool,
    /// lokale BKU per TCP-Probe überwachen (Desktop)
    pub probe_local_bku: bool,
    /// Budget des Kachel-Caches in Bytes
    pub tile_cache_bytes: usize,
    /// Vorbereiteten Zustand für „Signatur fortsetzen“ in der Sandbox sichern (Mobile-Lebenszyklus,
    /// Plan §3.5). `None` = nach Plattform (iOS/Android ja, Desktop nein).
    pub persist_prepared: Option<bool>,
    /// Ausgabeordner, wenn weder Overlay noch Einstellung einen nennen (Mobile: App-Dokumente, dann ohne
    /// Speichern-Dialog; Teilen/Exportieren im Ergebnis-Screen). `None` = Speichern-Dialog (Desktop).
    pub default_output_dir: Option<PathBuf>,
    /// Oberfläche „Arbeitsbereich“ (ADR 0004 §3.2): keine Platzhalterfrage, keine automatische
    /// Backend-Wahl, „Signieren mit X“ (`sign_with`) als einziger Weg zur Sitzung (Invariante A1), Abbruch
    /// zurück zum Arbeitsbereich. `None` = `false` (CLI, Tests, klassische UI); ohne Oberfläche
    /// (`headless`) nie.
    pub workspace_ui: Option<bool>,
    /// Stapelübersicht bei ≥ 2 Dokumenten (ADR 0004 §3.8, nur mit `workspace_ui`): eine Sitzung, eine
    /// Anmeldung, je Dokument eine Bestätigung. `None` = `false`: mehrere Dokumente laufen wie bisher
    /// nacheinander (im Arbeitsbereich über „Fertig“ → „Nächstes Dokument“). Die Startargument-Automatik
    /// (`-b … -a`, D-06) bleibt immer ohne Übersicht.
    pub batch_overview: Option<bool>,
    /// Transport der Update-Prüfung („Über PIDA“, ADR 0004 Nachtrag 21): Standard `pida-net`
    /// ([`update::NetTransport`], GitHub-Releases); Tests setzen einen Ersatz ein. `None` = nie prüfen
    /// ([`CoreOptions::isolated`]: kein Netz in Tests). Geprüft wird ohnehin nur am Desktop mit Oberfläche.
    pub update_transport: Option<Arc<dyn update::UpdateTransport>>,
}

impl Default for CoreOptions {
    fn default() -> Self {
        Self {
            interactive: None,
            engine: None,
            backends: None,
            id_austria_backend: None,
            secret_store: None,
            policy: None,
            load_system_policy: true,
            render: None,
            use_pdfium: true,
            shell: None,
            system_language: None,
            init_logging: true,
            log_level: "info".into(),
            legacy_dir: None,
            detect_legacy_dir: true,
            probe_local_bku: true,
            tile_cache_bytes: 64 * 1024 * 1024,
            persist_prepared: None,
            default_output_dir: None,
            workspace_ui: None,
            batch_overview: None,
            update_transport: Some(Arc::new(update::NetTransport)),
        }
    }
}

impl CoreOptions {
    /// Für Tests: keine Systemrichtlinie, kein PDFium, kein Logging, kein Altordner, keine
    /// TCP-Probe, Secrets nur im RAM, keine Update-Prüfung (kein Netz).
    pub fn isolated() -> CoreOptions {
        CoreOptions {
            load_system_policy: false,
            use_pdfium: false,
            init_logging: false,
            detect_legacy_dir: false,
            probe_local_bku: false,
            secret_store: Some(Arc::new(pida_config::MemorySecretStore::default())),
            update_transport: None,
            ..CoreOptions::default()
        }
    }
}

impl std::fmt::Debug for CoreOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoreOptions")
            .field("interactive", &self.interactive)
            .field("engine", &self.engine)
            .field("backends", &self.backends.as_ref().map(Vec::len))
            .field("secret_store", &self.secret_store)
            .field("policy", &self.policy)
            .field("render", &self.render.is_some())
            .field("use_pdfium", &self.use_pdfium)
            .field("shell", &self.shell.is_some())
            .field("init_logging", &self.init_logging)
            .field("persist_prepared", &self.persist_prepared)
            .field("workspace_ui", &self.workspace_ui)
            .field("batch_overview", &self.batch_overview)
            .field("update_transport", &self.update_transport.is_some())
            .finish_non_exhaustive()
    }
}

/// FIDO2-Authenticator der Plattform (nur mit den Features `fido2-windows`/`fido2-hid` von
/// `pida-backend-atrust`; sonst `None`, z. B. Mobile).
pub fn platform_fido2() -> Option<Arc<dyn pida_backend_atrust::Fido2Provider>> {
    pida_backend_atrust::fido2::platform_provider()
}

/// Der Kern (ein Prozess, eine Instanz; Single-Instance übernimmt die Shell).
pub struct Core {
    tx: mpsc::UnboundedSender<Msg>,
    view_rx: watch::Receiver<ViewState>,
    finished_rx: watch::Receiver<Option<ExitCode>>,
    shared: Arc<Shared>,
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    runtime: Option<tokio::runtime::Runtime>,
    _log: Mutex<Option<logging::LogGuard>>,
}

impl std::fmt::Debug for Core {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Core").field("shared", &self.shared).finish_non_exhaustive()
    }
}

fn initial_view(lang: Lang, platform: Platform) -> ViewState {
    ViewState {
        step: StepView::Starting,
        main_bar: MainBarView { steps: pida_workflow::State::Starting.main_bar().to_vec(), settings_enabled: false },
        settings: None,
        language: lang,
        accessibility: None,
        notices: Vec::new(),
        migration: None,
        done: Vec::new(),
        platform: PlatformView::from(platform),
        resume: None,
        keystore_import: None,
        privacy_text: None,
        document: None,
        signers: SignersView::default(),
        batch: None,
        recent: None,
        first_run: None,
        actions: actions::actions_for(&pida_workflow::State::Starting, false),
        parked: None,
        share_hint: None,
        about: None,
        update_hint: None,
    }
}

impl Core {
    /// Startet den Kern (Einstellungen laden, Richtlinie, Backends registrieren, Reducer-Task).
    pub fn new(config: CoreConfig) -> Result<Core, CoreError> {
        Core::new_with(config, CoreOptions::default())
    }

    /// Wie [`Core::new`] mit erweiterten Optionen.
    pub fn new_with(config: CoreConfig, options: CoreOptions) -> Result<Core, CoreError> {
        // Browser: keine eigene Laufzeit – der Reducer läuft im Ereignis-Takt der Seite (`rt::spawn`)
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        let (runtime, handle) = match tokio::runtime::Handle::try_current() {
            Ok(h) => (None, h),
            Err(_) => {
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .thread_name("pida-core")
                    .build()
                    .map_err(|e| CoreError::Runtime(e.to_string()))?;
                let h = rt.handle().clone();
                (Some(rt), h)
            }
        };
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        let _enter = handle.enter();
        if let Some(lang) = options.system_language {
            Lang::set_system(Some(lang));
        }
        let log = options.init_logging.then(|| {
            logging::init(
                &config.paths,
                &logging::LogOptions { level: options.log_level.clone(), stderr: false, file: true },
            )
        });
        config.paths.ensure()?;
        tracing::info!(version = %config.app_version, platform = ?config.platform, headless = config.headless, "Kern startet");

        let policy = match options.policy.clone() {
            Some(p) => Some(p),
            None if options.load_system_policy => match Policy::load_system() {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(id = e.message_id(), "Richtlinie nicht lesbar – ignoriert");
                    None
                }
            },
            None => None,
        };
        let secrets = options.secret_store.clone().unwrap_or_else(pida_config::platform_secret_store);
        let legacy_dir =
            options.legacy_dir.clone().or_else(|| if options.detect_legacy_dir { Paths::legacy_dir() } else { None });
        let (mut cfg, startup) = config::ConfigState::load(
            config.paths.clone(),
            config.overlay.clone(),
            policy,
            secrets,
            legacy_dir,
            !config.headless,
        )?;
        // iOS/Android: nur ID Austria (ADR 0004 Nachtrag 11)
        cfg.adopt_platform(config.platform);

        let render: Option<Arc<dyn RenderBackend>> = match options.render.clone() {
            Some(r) => Some(r),
            None if options.use_pdfium => match pida_render::PdfiumBackend::bind(config.pdfium_dir.as_deref()) {
                Ok(b) => Some(Arc::new(b)),
                Err(e) => {
                    tracing::warn!(
                        code = e.code(),
                        "PDFium nicht verfügbar – keine Vorschau, automatische Positionierung"
                    );
                    None
                }
            },
            None => None,
        };
        let profiles = match pida_sigblock::ProfileStore::builtin() {
            Ok(p) => Some(Arc::new(p)),
            Err(e) => {
                tracing::error!(code = e.code(), "Signaturprofile nicht ladbar");
                None
            }
        };

        let (tx, rx) = mpsc::unbounded_channel();
        let interactive = options.interactive.unwrap_or(!config.headless);
        let broker = Arc::new(broker::CoreBroker::new(interactive, runtime::broker_sink(tx.clone())));
        broker.set_shell(options.shell.clone());
        let fido2_available = config.fido2.as_ref().is_some_and(|f| f.is_available());
        let shared = Arc::new(Shared {
            docs: docs::DocStore::new(options.tile_cache_bytes),
            render,
            engine: options.engine.clone().unwrap_or_else(engine::default_engine),
            profiles,
            fonts: Arc::new(pida_sigblock::FontProvider::builtin()),
            block: Mutex::new(runtime::BlockInfo::default()),
            broker,
            sign: Mutex::new(runtime::SignSlot::default()),
            tx: tx.clone(),
            shell: Mutex::new(options.shell.clone()),
            tile_scale: Mutex::new(1.0),
            thumbs: Mutex::new(std::collections::HashMap::new()),
        });
        let mut registry = backends::Registry::new(
            config.platform,
            config.fido2.clone(),
            &config.app_version,
            options.backends.clone(),
            options.probe_local_bku,
        );
        if let Some(backend) = options.id_austria_backend.clone() {
            registry.set_id_austria(backend);
        }
        let (view_tx, view_rx) = watch::channel(initial_view(cfg.language(), config.platform));
        let (finished_tx, finished_rx) = watch::channel(None);
        let mobile = runtime::MobileSetup {
            platform: config.platform,
            persist_prepared: options.persist_prepared.unwrap_or(!config.platform.is_desktop()) && !config.headless,
            default_output_dir: options.default_output_dir.clone(),
        };
        // „Über PIDA“, Update-Prüfung (Nachtrag 21): nur am Desktop mit Oberfläche wird der Zwischenspeicher gelesen
        let transport = options.update_transport.clone().filter(|_| config.platform.is_desktop() && !config.headless);
        let updates = update::UpdateState::new(&config.app_version, transport, &config.paths.cache_dir);
        let task = CoreTask::new(
            shared.clone(),
            cfg,
            registry,
            config.headless,
            view_tx,
            finished_tx,
            startup,
            fido2_available,
            mobile,
            options.workspace_ui.unwrap_or(false),
            options.batch_overview.unwrap_or(false),
            updates,
        );
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        {
            handle.spawn(task.run(rx));
            Ok(Core { tx, view_rx, finished_rx, shared, runtime, _log: Mutex::new(log) })
        }
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        {
            rt::spawn(task.run(rx));
            Ok(Core { tx, view_rx, finished_rx, shared, _log: Mutex::new(log) })
        }
    }

    /// Sendet ein UI-Ereignis (nicht blockierend).
    pub fn dispatch(&self, event: UiEvent) {
        if self.tx.send(Msg::Ui(event)).is_err() {
            tracing::warn!("Kern beendet – Ereignis verworfen");
        }
    }

    /// Abonniert den ViewState (aktueller Wert sofort verfügbar).
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<ViewState> {
        self.view_rx.clone()
    }

    /// Aktueller ViewState.
    pub fn view(&self) -> ViewState {
        self.view_rx.borrow().clone()
    }

    /// PNG einer Seitenkachel (Custom Protocol `pida-render://`); `page == Seitenzahl` liefert die
    /// weiße virtuelle neue Seite.
    pub fn page_tile(&self, doc: DocId, page: usize, scale: f32) -> Result<Vec<u8>, CoreError> {
        let render = self
            .shared
            .render
            .as_ref()
            .ok_or(pida_render::RenderError::LibraryNotFound("PDFium nicht gebunden".into()))?;
        if scale.is_finite() && scale > 0.0 {
            *self.shared.tile_scale.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = scale;
        }
        match self.shared.docs.tile(doc, page, scale, render.as_ref())? {
            Some(png) => Ok(png.to_vec()),
            None => Err(CoreError::UnknownDocument(doc)),
        }
    }

    /// PNG-Miniatur eines Stapeleintrags aus der Voranalyse (Custom Protocol `pida-render://thumb/<doc>`,
    /// ADR 0004 §3.8; Breite 96 px, nur mit PDFium).
    pub fn thumbnail(&self, doc: DocId) -> Result<Vec<u8>, CoreError> {
        let thumbs = self.shared.thumbs.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        thumbs.get(&doc).map(|png| png.to_vec()).ok_or(CoreError::UnknownDocument(doc))
    }

    /// PNG der Blockvorschau aus dem echten Appearance-Objekt (Plan §6.6).
    pub fn block_preview(&self, scale: f32) -> Result<Vec<u8>, CoreError> {
        let render = self
            .shared
            .render
            .as_ref()
            .ok_or(pida_render::RenderError::LibraryNotFound("PDFium nicht gebunden".into()))?;
        let pdf = self
            .shared
            .block
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pdf
            .clone()
            .ok_or(CoreError::NoPreview)?;
        let scale = if scale.is_finite() && scale > 0.0 { scale.min(16.0) } else { 1.0 };
        let renderer = render.open(pdf, None)?;
        let image = renderer.render_page(0, scale)?;
        Ok(pida_render::encode_png(&image)?)
    }

    /// Bild eines QR-Platzhalters („QR-Platzhalter erstellen“; Custom Protocol `pida-render://qr…`, Web): SVG immer,
    /// PNG nur mit PDFium.
    pub fn qr_placeholder(
        &self,
        spec: &qr_placeholder::QrPlaceholderSpec,
        format: qr_placeholder::QrPlaceholderFormat,
    ) -> Result<Vec<u8>, CoreError> {
        let render = self.shared.render.as_deref().map(|r| r as &dyn pida_render::RenderBackend);
        qr_placeholder::image(spec, format, render).map_err(CoreError::QrPlaceholder)
    }

    /// Wartet bis `Finished` (CLI) und liefert den Exit-Code.
    pub async fn wait_finished(&self) -> ExitCode {
        let mut rx = self.finished_rx.clone();
        loop {
            if let Some(code) = *rx.borrow_and_update() {
                return code;
            }
            if rx.changed().await.is_err() {
                return ExitCode::General;
            }
        }
    }

    /// Renderer (PDFium) gebunden? Ohne ihn gibt es keine Vorschau, die Position ist automatisch.
    pub fn has_renderer(&self) -> bool {
        self.shared.render.is_some()
    }

    /// Setzt die Shell-Funktionen (Öffnen von Dateien/URLs, Elternfenster).
    pub fn set_shell(&self, shell: Option<Arc<dyn ShellHooks>>) {
        self.shared.broker.set_shell(shell.clone());
        *self.shared.shell.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = shell;
    }

    /// Ergebnisse der abgeschlossenen Dokumente (CLI `--json`, Warteschlangen-Panel).
    pub fn results(&self) -> Vec<DoneItemView> {
        self.view().done
    }

    /// Anzahl der im Speicher gehaltenen Dokumente (Diagnose; Feature `testing`).
    #[cfg(any(test, feature = "testing"))]
    pub fn open_document_count(&self) -> usize {
        self.shared.docs.len()
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Shutdown);
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        if let Some(rt) = self.runtime.take() {
            rt.shutdown_background();
        }
    }
}
