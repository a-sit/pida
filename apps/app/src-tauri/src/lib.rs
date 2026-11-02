//! `pida-app` – Tauri-2-Shell von PIDA (Desktop und Mobile; Mobile-Teil: [`mobile`],
//! docs/integration/mobile.md).
//!
//! Die Shell ist **dünn** (Plan §3.1): Sie startet `pida-core-api`, leitet den [`ViewState`] als
//! Ereignis [`VIEW_EVENT`] an das Fenster, nimmt [`pida_core_api::UiEvent`]s über den Command
//! `core_dispatch` entgegen und liefert Seitenkacheln und die Blockvorschau über das Custom Protocol
//! `pida-render://` ([`protocol`]). Fachlogik liegt nur im Kern; die Oberfläche (React, React Aria
//! Components) projiziert nur den Zustand.
//!
//! Plattformintegration:
//! - **Startargumente** mit der vollständigen PDF-Over-Semantik ([`args`], derselbe Parser wie
//!   `pida-cli`): `-i -o -b -n -p -ks… -proxy… -e -a -s -c -v` als Overlay des Kerns; ungültige
//!   Argumente zeigen den Fehlerschritt (`error-CmdLineArgs`), `-h` gibt die Hilfe aus.
//! - **Single-Instance** (Desktop): eine zweite Instanz übergibt ihre Dokumente an die erste (F-13,
//!   [`args::second_instance_documents`]); unter macOS nur mit eigenem Socket ([`instance`]).
//! - **„Öffnen mit“**: Startargumente unter Windows/Linux und `RunEvent::Opened`
//!   (macOS-Apple-Events zur Datei-Zuordnung `.pdf` im Bundle; iOS/Android „Öffnen in“). Dokumente, die
//!   vor `setup` eintreffen (macOS-Kaltstart über ein Dokument), merkt [`instance::EarlyDocuments`] vor.
//! - **Shell-Funktionen** des Kerns ([`hooks`]): Ergebnis öffnen (F-87), „Signaturdaten anzeigen“ im
//!   Browser (F-68), Elternfenster für Windows-Hello/FIDO2.
//! - **Sprache**: `Lang::set_system` aus `sys-locale` (unter Windows/macOS-GUI fehlen POSIX-Variablen). Das Plugin `deep-link` kommt erst mit eigenen URL-Schemata bzw. App Links (Szenario C/D,
//!   Mobile) hinzu; in v1 gäbe es nur tote Abhängigkeiten (u. a. `tiny-keccak`, CC0, im Lizenz-Gate).
//! - **Drag & Drop** über die Fenster-Ereignisse von Tauri (Oberfläche: `onDragDropEvent`).
//! - **Fensterzustand** (Plugin `window-state`), Dialoge (`dialog`, nur Desktop), Öffnen im System
//!   (`opener`).
//! - Logs: Datei im Log-Ordner der Plattform ([`pida_config::Paths`]) über
//!   `pida_core_api::logging` (rotierend auch während des Laufs), ohne Secrets und Dokumentinhalte
//!   (Regel 9); Filter über `--log-level`, sonst `PIDA_LOG`.
//! - **Kernfehler**: Startfehler und Paniken erscheinen als Fehlerschritt `ng-ui-core-unavailable`, ein
//!   unerwartet beendeter Kern-Task als [`CORE_STOPPED`] – jeweils mit „Wiederholen“ (Neustart des Kerns).
//! - **Startprüfung** (`PIDA_SMOKE=1`): beendet die App, sobald die Oberfläche den ersten
//!   ViewState abgefragt hat und der Kern bereit ist; gibt die Startzeiten auf stdout aus ([`smoke`]).
//! - **Beenden** (Fenster schließen, ⌘Q): läuft über den Kern (`UiEvent::Quit` → `Finished`), damit
//!   laufende Dialoge abgebrochen (F-73) und Einstellungen gesichert werden (F-105); antwortet der Kern
//!   nicht binnen [`QUIT_GRACE`], endet die App trotzdem.
//! - **Berechtigungen** der Oberfläche (capabilities/default.json) sind minimal: Ereignisse empfangen,
//!   die eigenen Commands, Datei-/Speichern-Dialog und `https:`/`mailto:`-Links – kein `core:default`
//!   (Menüs, Tray, Fenster-/Pfad-APIs bleiben gesperrt). Mobile (capabilities/mobile.json): ohne
//!   Dateidialog, Log-Ordner und Dateimanager, dafür `mobile_pick_files`.
//!
//! Mobile (`cfg(mobile)`, Plan §5.2/§5.3):
//! - Einstieg [`run`] mit `tauri::mobile_entry_point`; keine Startargumente (keine CLI), kein
//!   Single-Instance, kein Fensterzustand, kein Updater.
//! - Pfade der App-Sandbox (`Paths::sandbox`), Ausgabe in die App-Dokumente, Secrets über das Plugin
//!   ([`mobile::secret_store`]), PDFium aus dem Bundle bzw. `jniLibs` ([`pdfium::mobile_dir`]).
//! - Lebenszyklus `WindowEvent::Suspended`/`Resumed` → `UiEvent::Backgrounded`/`Foregrounded` (kein
//!   Abbruch); Schließen/Beenden geht nie als `Quit` an den Kern. Die App beendet sich nie selbst:
//!   `Finished` des Kerns bzw. „Beenden“ ohne Kern starten einen neuen Kern (`finish_on_mobile`).
//! - Aufräumen: alte Kopien im Eingangs- und Import-Ordner des Caches beim Start
//!   ([`mobile::purge_stale_copies`]).
//! - Dokument-Eingang: `RunEvent::Opened` (URLs, keine Pfade) und Intents über das Plugin
//!   (`take_pending_open_documents`, beim Start und nach jeder Rückkehr); Bytes werden in den
//!   Eingangsordner der Sandbox kopiert ([`mobile::ingest_urls`]).
//!
//! Ownership: Paket **I3** (Welle 2), danach Integration.

pub mod args;
pub mod files;
pub mod hooks;
pub mod instance;
pub mod mobile;
pub mod pdfium;
pub mod protocol;
pub mod shell;
pub mod smoke;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pida_backend_api::Platform;
use pida_config::{Overlay, Paths};
use pida_core_api::{CoreConfig, ViewState};
use pida_i18n::Lang;
use tauri::{AppHandle, Emitter, Manager, State};

use args::GuiArgs;
use instance::EarlyDocuments;
use shell::{DispatchOutcome, Shell, Started};

/// Meldungs-ID: der Kern-Task hat unerwartet geendet (Detail im Fehlerschritt).
pub const CORE_STOPPED: &str = "ng-core-stopped";

/// Name des Ereignisses mit dem jeweils aktuellen [`ViewState`].
pub const VIEW_EVENT: &str = "pida:view";

/// Wartezeit auf `Finished` nach einem Beenden-Wunsch, bevor die App ohne Kern endet.
pub const QUIT_GRACE: Duration = Duration::from_secs(5);

/// Informationen für den Über-Dialog.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AppInfo {
    /// App-Version
    pub version: String,
    /// Plattform
    pub platform: Platform,
    /// Kern verfügbar
    pub core_available: bool,
}

#[tauri::command]
fn core_view(app: AppHandle, shell: State<'_, Shell>) -> ViewState {
    // Startprüfung: die Oberfläche läuft und spricht mit der Shell
    smoke::ui_ready(&app, &shell);
    shell.view()
}

#[tauri::command]
fn core_dispatch(app: AppHandle, shell: State<'_, Shell>, event: serde_json::Value) -> Result<(), String> {
    match shell.dispatch_json(event).map_err(str::to_owned)? {
        DispatchOutcome::Dispatched => {}
        DispatchOutcome::Restart => start_core(&app, &shell),
        // Mobile: Apps beenden sich nicht selbst (iOS: `app.exit` endet in tao in einer Panik, Android in
        // `exit()` mit laufenden Systemthreads) – „Beenden“ ohne Kern startet ihn neu
        DispatchOutcome::Quit if cfg!(mobile) => start_core(&app, &shell),
        DispatchOutcome::Quit => app.exit(0),
    }
    Ok(())
}

#[tauri::command]
fn app_info(app: AppHandle, shell: State<'_, Shell>) -> AppInfo {
    AppInfo {
        version: app.package_info().version.to_string(),
        platform: Platform::current(),
        core_available: shell.core().is_some(),
    }
}

#[tauri::command]
fn open_log_dir(app: AppHandle, shell: State<'_, Shell>) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let dir = shell.log_dir().ok_or_else(|| "ng-ui-open-failed".to_owned())?;
    std::fs::create_dir_all(&dir).map_err(|_| "ng-ui-open-failed".to_owned())?;
    app.opener().open_path(dir.to_string_lossy(), None::<&str>).map_err(|_| "ng-ui-open-failed".to_owned())
}

/// Zeigt die Ergebnisdatei im Dateimanager – nur Dateien, die der Kern geschrieben hat: ohne `doc` die des
/// aktuellen Ergebnis-Schritts, mit `doc` die Ausgabe dieses Dokuments (ADR 0004 §3.14).
#[tauri::command]
fn reveal_output(app: AppHandle, shell: State<'_, Shell>, doc: Option<pida_workflow::DocId>) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let path = shell.output_path_for(doc).ok_or_else(|| "ng-ui-open-failed".to_owned())?;
    app.opener().reveal_item_in_dir(path).map_err(|_| "ng-ui-open-failed".to_owned())
}

/// Lizenztexte für die Anzeige in der App (Backlog Nr. 2; Einstellungen › Über PIDA › „Lizenzen“): `eupl` (EUPL-1.2,
/// Lizenz von PIDA), `notices` (THIRD-PARTY-NOTICES.md) bzw. `rust_std` (RUST-STD-COPYRIGHT.html, HTML). Eingebettet in
/// das Programm (`pida-cli`, Feature `notices`) statt aus dem Ressourcenordner gelesen – so auch auf Android und iOS.
/// Fehlen die Hinweise (Entwicklungsbuild ohne `npm run notices`), liefert der Command `ng-ui-licenses-missing`.
#[tauri::command]
async fn license_text(kind: String) -> Result<String, String> {
    use pida_cli::notices;
    match kind.as_str() {
        "eupl" => Ok(notices::EUPL.to_owned()),
        "notices" | "rust_std" => {
            let name = if kind == "notices" { "THIRD-PARTY-NOTICES.md" } else { "RUST-STD-COPYRIGHT.html" };
            notices::text(name).ok_or_else(|| {
                tracing::info!("Drittlizenz-Hinweise nicht eingebettet");
                "ng-ui-licenses-missing".to_owned()
            })
        }
        _ => Err(shell::EVENT_INVALID.to_owned()),
    }
}

/// Ergebnis von [`mobile_pick_files`]: Pfade der Kopien (Reihenfolge der Auswahl) und die Zahl der gewählten, aber
/// nicht übernommenen Dateien (ADR 0004 Nachtrag 17).
#[derive(Debug, serde::Serialize)]
struct MobilePick {
    paths: Vec<String>,
    skipped: usize,
}

/// Mobile: System-Dateiauswahl über das Plugin (`kind` = `pdf`); die gewählten Dateien liegen danach als
/// Kopien im Import-Ordner der Sandbox (die Oberfläche sendet ihre Pfade als `open_files`). `pkcs12` wird seit
/// ADR 0004 Nachtrag 11 abgelehnt: auf iOS/Android wird nur mit ID Austria signiert, einen Keystore-Import gibt
/// es nicht mehr. Desktop: `ng-mobile-unsupported` (dort gilt der Dateidialog). Läuft im Hintergrund (die
/// native Seite darf nicht vom Hauptthread aus blockierend gerufen werden).
#[tauri::command]
async fn mobile_pick_files(app: AppHandle, kind: String, multiple: bool) -> Result<MobilePick, String> {
    use tauri_plugin_pida_mobile::{ImportKind, PidaMobileExt};
    let kind = match kind.as_str() {
        "pdf" => ImportKind::Pdf,
        _ => return Err(shell::EVENT_INVALID.to_owned()),
    };
    let dest = app.path().app_cache_dir().map_err(|_| "ng-ui-open-failed".to_owned())?.join(mobile::IMPORT_DIR);
    std::fs::create_dir_all(&dest).map_err(|_| "ng-ui-open-failed".to_owned())?;
    let plugin = app.pida_mobile();
    let imported = tauri::async_runtime::spawn_blocking(move || plugin.import_files(kind, multiple, &dest))
        .await
        .map_err(|_| "ng-ui-open-failed".to_owned())?
        .map_err(|e| e.code().to_owned())?;
    if imported.skipped > 0 {
        tracing::info!(count = imported.skipped, "Dateiauswahl: Dateien nicht übernommen");
    }
    let paths = imported.files.into_iter().map(|f| f.path.to_string_lossy().into_owned()).collect();
    Ok(MobilePick { paths, skipped: imported.skipped })
}

/// Demodokument der Apps („Demo starten“, ⌘/Strg+D): eine Seite mit QR-Platzhalter Nr. 1
/// (`tools/make-demo-pdf.py`), im Programm eingebettet wie die Lizenztexte.
const DEMO_PDF: &[u8] = include_bytes!("../../public/PIDA-Demo.pdf");
/// Ordner der Demo-Kopie im Cache-Ordner der App.
const DEMO_DIR: &str = "demo";
/// Dateiname der Demo-Kopie.
const DEMO_FILE: &str = "PIDA-Demo.pdf";

/// Legt das Demodokument frisch im Cache-Ordner ab und liefert den Pfad (die Oberfläche öffnet ihn wie eine gewählte
/// Datei). Nicht aus dem Programmordner: der ist schreibgeschützt, und das Ergebnis landet standardmäßig neben der
/// Quelle. Der Ordner wird jedes Mal geleert, damit eine frühere Demo-Signatur keine Überschreib-Rückfrage auslöst.
#[tauri::command]
async fn demo_document(app: AppHandle) -> Result<String, String> {
    let dir = app.path().app_cache_dir().map_err(|_| "ng-ui-open-failed".to_owned())?.join(DEMO_DIR);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|_| "ng-ui-open-failed".to_owned())?;
    let path = dir.join(DEMO_FILE);
    std::fs::write(&path, DEMO_PDF).map_err(|_| "ng-ui-open-failed".to_owned())?;
    Ok(path.to_string_lossy().into_owned())
}

/// Startet den Kern (bzw. erneut nach „Wiederholen“) und abonniert den ViewState.
fn start_core(app: &AppHandle, shell: &Shell) {
    // Der Kern darf `tokio::spawn` im Kontext der Tauri-Laufzeit nutzen.
    let runtime = tauri::async_runtime::handle();
    let _guard = runtime.inner().enter();
    match shell.start() {
        Started::Running(core) => {
            let Ok(mut rx) = catch_unwind(AssertUnwindSafe(|| core.subscribe())) else {
                tracing::error!("ViewState-Abo fehlgeschlagen (Panik)");
                shell.mark_unavailable("subscribe");
                let _ = app.emit(VIEW_EVENT, shell.view());
                return;
            };
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let mut title_lang = None;
                loop {
                    let view = rx.borrow_and_update().clone();
                    if title_lang != Some(view.language) {
                        title_lang = Some(view.language);
                        set_window_title(&app, view.language);
                    }
                    let finished = match &view.step {
                        pida_core_api::StepView::Finished { exit_code } => Some(i32::from(*exit_code)),
                        _ => None,
                    };
                    let _ = app.emit(VIEW_EVENT, &view);
                    if let Some(code) = finished {
                        if cfg!(mobile) {
                            // Mobile beendet sich nicht selbst (siehe `finish_on_mobile`): neuer Kern, der
                            // wieder bei „Dokument öffnen“ beginnt
                            finish_on_mobile(&app, code);
                        } else {
                            app.exit(code);
                        }
                        break;
                    }
                    if rx.changed().await.is_err() {
                        // Kern-Task beendet, ohne `Finished` zu melden: Fehlerschritt mit „Wiederholen“
                        tracing::error!("Kern-Task unerwartet beendet");
                        if let Some(shell) = app.try_state::<Shell>() {
                            shell.mark_stopped(CORE_STOPPED);
                            let _ = app.emit(VIEW_EVENT, shell.view());
                        }
                        break;
                    }
                }
            });
        }
        Started::Unavailable => {
            let _ = app.emit(VIEW_EVENT, shell.view());
        }
    }
}

/// Fenstertitel in der Sprache der Oberfläche („PIDA – PDF-Signatur ID Austria“); nur Desktop.
fn set_window_title(app: &AppHandle, lang: Lang) {
    #[cfg(desktop)]
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_title(&pida_i18n::translate(lang, "ng-window-title", &[]));
    }
    #[cfg(not(desktop))]
    let _ = (app, lang);
}

/// Mobile: Der Kern meldet `Finished` (z. B. „Abbrechen“ im Fehlerschritt beim Start, `SKIP_FINISH`).
/// iOS-Apps beenden sich nicht selbst (App-Store-Richtlinien; tao kennt kein `ControlFlow::Exit`,
/// `app.exit` endet dort in einer Panik und `abort`), unter Android liefe `exit()` gegen die noch
/// aktiven Systemthreads. Stattdessen startet ein neuer Kern – die App steht wieder bei „Dokument
/// öffnen“; das System verwaltet den Prozess.
fn finish_on_mobile(app: &AppHandle, code: i32) {
    tracing::info!(code, "Kern beendet – Mobile startet neu statt zu beenden");
    if let Some(shell) = app.try_state::<Shell>() {
        start_core(app, &shell);
    }
}

#[cfg(desktop)]
/// Beenden über den Kern anfordern (Fenster schließen, ⌘Q). `true` = der Kern beendet die App über
/// `Finished` (Schließen/Beenden jetzt verhindern); nach [`QUIT_GRACE`] endet sie in jedem Fall.
fn quit_via_core(app: &AppHandle) -> bool {
    let Some(shell) = app.try_state::<Shell>() else {
        return false;
    };
    match shell.request_quit() {
        shell::QuitRequest::NoCore => false,
        shell::QuitRequest::AlreadyRequested => true,
        shell::QuitRequest::Requested => {
            let app = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(QUIT_GRACE);
                tracing::warn!("Kern hat das Beenden nicht bestätigt; Programmende");
                app.exit(0);
            });
            true
        }
    }
}

fn core_config(app: &AppHandle, overlay: Overlay) -> Result<CoreConfig, pida_config::ConfigError> {
    let paths = platform_paths(app)?;
    let exe = std::env::current_exe().ok();
    let pdfium_dir = if cfg!(mobile) {
        pdfium::mobile_dir(exe.as_deref(), cfg!(target_os = "android"))
    } else {
        let env_dir = pdfium::env_override();
        let resources = app.path().resource_dir().ok();
        let candidates = pdfium::candidates(env_dir, exe.as_deref(), resources.as_deref());
        pdfium::resolve(&candidates, exe.as_deref(), !cfg!(debug_assertions))
    };
    Ok(CoreConfig {
        paths,
        overlay,
        platform: Platform::current(),
        pdfium_dir,
        headless: false,
        fido2: fido2_provider(),
        app_version: app.package_info().version.to_string(),
    })
}

/// Portabler Modus (Backlog Nr. 31): die Fenster der Konfiguration mit dem WebView-Datenordner `<PIDA-Daten>/webview`
/// anlegen (sonst `%LOCALAPPDATA%\\<Kennung>\\EBWebView`).
fn portable_main_window(app: &tauri::App, root: &std::path::Path) -> tauri::Result<()> {
    for config in app.config().app.windows.clone() {
        tauri::WebviewWindowBuilder::from_config(app.handle(), &config)?
            .data_directory(root.join("webview"))
            .build()?;
    }
    Ok(())
}

/// Pfade: Desktop plattformüblich, Mobile die App-Sandbox (`app_data_dir` gesichert, `app_cache_dir`
/// darf das System leeren).
fn platform_paths(app: &AppHandle) -> Result<Paths, pida_config::ConfigError> {
    if cfg!(mobile) {
        let data = app.path().app_data_dir().map_err(|_| pida_config::ConfigError::NoHomeDirectory)?;
        let cache = app.path().app_cache_dir().map_err(|_| pida_config::ConfigError::NoHomeDirectory)?;
        Ok(Paths::sandbox(&data, &cache))
    } else {
        Paths::platform()
    }
}

/// Mobile: Optionen des Kerns (sicherer Speicher über das Plugin, Ausgabe in die App-Dokumente, keine
/// Altkonfiguration, keine lokale BKU).
#[cfg(mobile)]
fn mobile_options(app: &AppHandle, shell: &Shell) {
    use tauri_plugin_pida_mobile::PidaMobileExt;
    // iOS: `Documents` (in der Dateien-App sichtbar, UIFileSharingEnabled). Android: `filesDir/Documents`
    // statt `getExternalFilesDir(DOCUMENTS)` (Tauri `document_dir`) – den lesen unter Android 9/10 alle
    // Apps mit Speicher-Berechtigung mit; Teilen/„Sichern unter“ geben das Dokument gezielt weiter.
    let documents = if cfg!(target_os = "android") {
        app.path().app_data_dir().ok().map(|d| d.join(mobile::ANDROID_OUTPUT_DIR))
    } else {
        app.path().document_dir().ok()
    };
    let secrets = mobile::secret_store(app.pida_mobile());
    shell.update_options(move |o| {
        o.secret_store = Some(secrets);
        o.default_output_dir = documents;
        o.detect_legacy_dir = false;
        o.probe_local_bku = false;
    });
}

/// Mobile: Lebenszyklus an den Kern (kein Abbruch); nach der Rückkehr eingegangene Dokumente abholen.
#[cfg(mobile)]
fn lifecycle(app: &AppHandle, suspended: bool) {
    if let Some(shell) = app.try_state::<Shell>() {
        tracing::info!(suspended, "Lebenszyklus");
        shell.dispatch(mobile::lifecycle_event(suspended));
    }
    if !suspended {
        take_pending_documents(app, Vec::new());
    }
}

/// Mobile: alte Kopien im Eingangs- und Import-Ordner des Caches entfernen (im Hintergrund).
#[cfg(mobile)]
fn purge_stale_copies(app: &AppHandle) {
    let Ok(cache) = app.path().app_cache_dir() else { return };
    std::thread::spawn(move || {
        let now = std::time::SystemTime::now();
        let removed: usize = [mobile::INBOX_DIR, mobile::IMPORT_DIR]
            .iter()
            .map(|d| mobile::purge_stale_copies(&cache.join(d), mobile::STALE_COPY_AGE, now))
            .sum();
        if removed > 0 {
            tracing::info!(removed, "alte Kopien im Cache entfernt");
        }
    });
}

/// Mobile: Dokumente aus „Öffnen in“/Share/Intents in die Sandbox kopieren und an den Kern geben – im
/// Hintergrund (Dateizugriff, native Seite; Ereignisse kommen auf dem Hauptthread).
#[cfg(mobile)]
fn take_pending_documents(app: &AppHandle, urls: Vec<url::Url>) {
    use tauri_plugin_pida_mobile::PidaMobileExt;
    let Ok(cache) = app.path().app_cache_dir() else { return };
    let inbox = cache.join(mobile::INBOX_DIR);
    let app = app.clone();
    std::thread::spawn(move || {
        let ingested = mobile::ingest_urls(&urls, &inbox);
        let mut docs = ingested.files;
        if std::fs::create_dir_all(&inbox).is_ok() {
            match app.pida_mobile().take_pending_open_documents(&ingested.unresolved, &inbox) {
                Ok(files) => docs.extend(files.into_iter().map(|f| f.path)),
                Err(e) => {
                    if !ingested.unresolved.is_empty() || !matches!(e, tauri_plugin_pida_mobile::Error::NotImplemented)
                    {
                        tracing::warn!(code = e.code(), "eingehende Dokumente der Plattform nicht abgeholt");
                    }
                }
            }
        }
        if !docs.is_empty() {
            tracing::info!(count = docs.len(), "Dokumente eingegangen");
            deliver_documents(&app, docs);
        }
    });
}

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
fn fido2_provider() -> Option<Arc<dyn pida_backend_atrust::Fido2Provider>> {
    pida_backend_atrust::fido2::platform_provider()
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn fido2_provider() -> Option<Arc<dyn pida_backend_atrust::Fido2Provider>> {
    None
}

/// Hält den Schreib-Thread der Logdatei bis zum Programmende (beim Drop wird geleert).
struct LogState(Mutex<Option<pida_core_api::logging::LogGuard>>);

/// Logdatei im Log-Ordner der Plattform (rotierend, Plan §7.1); Filter aus `--log-level`, sonst
/// `PIDA_LOG`, sonst `info`. In Debug-Builds zusätzlich stderr.
fn init_logging(paths: &Paths, log_level: Option<String>) -> pida_core_api::logging::LogGuard {
    let level = log_level
        .or_else(|| std::env::var("PIDA_LOG").ok().filter(|l| !l.trim().is_empty()))
        .unwrap_or_else(|| "info".into());
    pida_core_api::logging::init(
        paths,
        &pida_core_api::logging::LogOptions { level, stderr: cfg!(debug_assertions), file: true },
    )
}

/// Logs nur auf stderr (ohne Konfiguration kein Log-Ordner).
fn init_logging_stderr() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_env("PIDA_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).with_ansi(false).with_writer(std::io::stderr).try_init();
}

/// Systemsprache für `Lang::system` (unter Windows/macOS-GUI fehlen die POSIX-Variablen).
fn init_language() -> Option<Lang> {
    let lang = sys_locale::get_locale().as_deref().and_then(Lang::from_tag);
    if lang.is_some() {
        Lang::set_system(lang);
    }
    lang
}

/// Dokumente aus „Öffnen mit“ bzw. einer zweiten Instanz: vor `setup` vormerken, danach an die Shell.
fn deliver_documents(app: &AppHandle, docs: Vec<std::path::PathBuf>) {
    let early = app.try_state::<EarlyDocuments>();
    instance::deliver(early.as_deref(), docs, |docs| match app.try_state::<Shell>() {
        Some(shell) => {
            shell.open_files(docs);
            true
        }
        None => false,
    });
}

#[cfg(desktop)]
fn focus_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Einstieg (Desktop über `main.rs`, Mobile über den Einstiegspunkt der Plattform).
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    smoke::mark_process_start();
    let system_language = init_language();

    // Startargumente wie PDF-Over (Hilfe/Version ohne Fenster); Mobile hat keine Kommandozeile.
    let cwd = std::env::current_dir().unwrap_or_default();
    let start_args = if cfg!(mobile) {
        GuiArgs::Start { overlay: Box::default(), log_level: None }
    } else {
        args::parse_gui_args(std::env::args_os(), &cwd, &|name| std::env::var_os(name))
    };
    let start_args = match start_args {
        GuiArgs::Help(text) => {
            print!("{text}");
            return;
        }
        GuiArgs::Version(text) => {
            println!("{text}");
            return;
        }
        other => other,
    };

    // Portabler Modus (Windows-ZIP, Backlog Nr. 31): Ordner `PIDA-Daten` neben `pida.exe`
    let portable = pida_config::portable_root();
    let mut context = tauri::generate_context!();
    if portable.is_some() {
        // Daten der WebView2 (Cache, Speicher) in den portablen Ordner statt nach %LOCALAPPDATA%: Die Konfiguration
        // nimmt nur relative Pfade, also legt `setup` das Hauptfenster selbst an (`portable_main_window`).
        for w in &mut context.config_mut().app.windows {
            w.create = false;
        }
    }

    // vor `build` registriert: `RunEvent::Opened` kann unter macOS vor `setup` eintreffen
    let builder = tauri::Builder::default().manage(EarlyDocuments::default());

    // Nur Desktop: Single-Instance (Mobile: das System startet die App einmal) und Fensterzustand.
    let single_instance = cfg!(desktop) && instance::single_instance_allowed();
    #[cfg(desktop)]
    let builder = {
        let mut builder = builder;
        // Muss als erstes Plugin registriert werden (macOS: nur mit eigenem Socket, siehe `instance`).
        if single_instance {
            builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
                let docs = args::second_instance_documents(&argv, std::path::Path::new(&cwd));
                deliver_documents(app, docs);
                focus_main_window(app);
            }));
        }
        // Fensterzustand liegt in %APPDATA% – im portablen Modus nicht merken
        if portable.is_none() {
            builder = builder.plugin(tauri_plugin_window_state::Builder::default().build());
        }
        builder
    };

    // Datei-/Speichern-Dialoge nur auf dem Desktop (Mobile: Dokument-Picker bzw. SAF über
    // `pida-mobile`); das Plugin ist dort gar nicht im Binary (Cargo.toml, iOS: ohne Photos/PhotosUI).
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_dialog::init());

    // iOS/macOS: Beendet das System den Web-Prozess der WebView (iOS nach längerer Zeit im Hintergrund, um Speicher
    // freizugeben), bleibt sonst das letzte Bild stehen – Scrollen geht noch (das übernimmt das System), Knöpfe nicht
    // (kein JavaScript mehr). Dann die Oberfläche neu laden: Der Zustand liegt im Kern, die Oberfläche holt ihn beim
    // Start mit `core_view` wieder ab. wry/Tauri laden von sich aus nicht neu.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    let builder = builder.on_web_content_process_terminate(|webview| {
        tracing::warn!("Web-Prozess beendet – Oberfläche wird neu geladen");
        if let Err(e) = webview.reload() {
            tracing::warn!(error = %e, "Oberfläche nicht neu geladen");
        }
    });

    let app = builder
        .plugin(tauri_plugin_opener::init())
        // Mobile: native Seite (Swift/Kotlin); Desktop: ohne Funktion. Keine Befehle für die WebView.
        .plugin(tauri_plugin_pida_mobile::init())
        .register_asynchronous_uri_scheme_protocol(protocol::SCHEME, |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let path = request.uri().path().to_owned();
            let query = request.uri().query().map(str::to_owned);
            tauri::async_runtime::spawn_blocking(move || {
                let response = match app.try_state::<Shell>() {
                    Some(shell) => shell.render(&path, query.as_deref()),
                    None => protocol::error(503, shell::CORE_UNAVAILABLE),
                };
                responder.respond(response);
            });
        })
        .invoke_handler(tauri::generate_handler![
            core_view,
            core_dispatch,
            app_info,
            open_log_dir,
            reveal_output,
            license_text,
            mobile_pick_files,
            demo_document
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            let (overlay, log_level, args_error) = match start_args {
                GuiArgs::Start { overlay, log_level } => (*overlay, log_level, None),
                GuiArgs::Invalid { code, params } => (Overlay::default(), None, Some((code, params))),
                GuiArgs::Help(_) | GuiArgs::Version(_) => (Overlay::default(), None, None),
            };
            let shell = match core_config(&handle, overlay) {
                Ok(config) => {
                    app.manage(LogState(Mutex::new(Some(init_logging(&config.paths, log_level)))));
                    match args_error {
                        Some((code, params)) => {
                            tracing::warn!(code = %code, "Startargumente ungültig – Kern startet nicht");
                            Shell::start_error(Some(config), &code, params)
                        }
                        None => Shell::new(config),
                    }
                }
                Err(e) => {
                    // ohne Benutzerordner: Fehlerschritt statt Panik beim Start
                    init_logging_stderr();
                    tracing::error!(id = e.message_id(), "Konfiguration nicht ermittelbar");
                    Shell::without_config(&e.to_string())
                }
            };
            tracing::info!(version = %handle.package_info().version, "PIDA startet");
            if let Some(root) = &portable {
                tracing::info!(dir = %root.display(), "Portabler Modus – Passwörter nur bis zum Beenden");
                // kein Schlüsselbund: nicht dauerhaft gespeichert (Entscheidung vom 20.10.2026, Option a); der Kern
                // blendet „Im Schlüsselbund“ aus und behandelt eine solche Einstellung wie „Bis zum Beenden“
                shell.update_options(|o| o.secret_store = Some(Arc::new(pida_config::MemorySecretStore::default())));
            }
            if cfg!(desktop) && !single_instance {
                tracing::warn!("Single-Instance-Socket gehört nicht diesem Konto – Start ohne Single-Instance");
            }
            shell.set_system_language(system_language);
            // Oberfläche „Arbeitsbereich“ (ADR 0004 §3.14, eingeschaltet im Merge-Commit von A1)
            shell.update_options(|o| o.workspace_ui = Some(true));
            // Stapelübersicht ab zwei Dokumenten (ADR 0004 §3.14, eingeschaltet im Merge-Commit von A2)
            shell.update_options(|o| o.batch_overview = Some(true));
            // `PIDA_UI=workspace` (nur Debug-Builds) bleibt für die Abnahme späterer Etappen
            let pida_ui = std::env::var("PIDA_UI").ok();
            if let Some(workspace) = shell::workspace_ui_from_env(pida_ui.as_deref(), cfg!(debug_assertions)) {
                tracing::info!("Oberfläche „Arbeitsbereich“ (PIDA_UI)");
                shell.update_options(|o| o.workspace_ui = Some(workspace));
            }
            // `PIDA_UI=workspace,batch` (nur Debug-Builds): Stapelübersicht für die Abnahme vor dem Merge von A2
            if let Some(batch) = shell::batch_overview_from_env(pida_ui.as_deref(), cfg!(debug_assertions)) {
                tracing::info!("Stapelübersicht (PIDA_UI)");
                shell.update_options(|o| o.batch_overview = Some(batch));
            }
            shell.set_hooks(Arc::new(hooks::TauriHooks::new(handle.clone())));
            #[cfg(mobile)]
            {
                mobile_options(&handle, &shell);
                purge_stale_copies(&handle);
            }
            app.manage(shell);
            let shell = handle.state::<Shell>();
            // vor `setup` eingetroffene Dokumente (macOS-Kaltstart über ein Dokument) gehen mit dem Start
            // an den Kern; ab hier liefert `deliver_documents` direkt an die Shell
            shell.open_files(handle.state::<EarlyDocuments>().close());
            start_core(&handle, &shell);
            // Mobile-Kaltstart über „Öffnen in“/Share bzw. Intent: vorgemerkte Dokumente der Plattform
            #[cfg(mobile)]
            take_pending_documents(&handle, Vec::new());
            // erst jetzt, da der Kern verwaltet wird (die Oberfläche fragt sofort `core_view` ab)
            if let Some(root) = &portable {
                portable_main_window(app, root)?;
            }
            smoke::arm_watchdog(&handle);
            Ok(())
        })
        .build(context)
        .expect("Tauri-Anwendung konnte nicht erstellt werden");

    app.run(|app, event| match event {
        // Desktop: Schließen/Beenden über den Kern. Mobile verwaltet das System den Prozess – ein
        // Schließen der Activity bzw. ein App-Wechsel ist kein Abbruch (Plan §3.5).
        #[cfg(desktop)]
        tauri::RunEvent::WindowEvent { event: tauri::WindowEvent::CloseRequested { api, .. }, .. } => {
            // Fenster schließen während eines Dialogs gilt als Abbruch (F-73); der Kern räumt auf, sichert
            // die Einstellungen und meldet `Finished` → `app.exit(code)`.
            if quit_via_core(app) {
                api.prevent_close();
            }
        }
        // Beenden durch den Nutzer (⌘Q, Menü); `Some(code)` stammt von `app.exit` und wird nicht gehalten.
        #[cfg(desktop)]
        tauri::RunEvent::ExitRequested { code: None, api, .. } => {
            if quit_via_core(app) {
                api.prevent_exit();
            }
        }
        tauri::RunEvent::Exit => {
            // Logdatei leeren (nicht blockierender Schreiber)
            if let Some(log) = app.try_state::<LogState>() {
                drop(log.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take());
            }
        }
        #[cfg(mobile)]
        tauri::RunEvent::WindowEvent { event: tauri::WindowEvent::Suspended, .. } => lifecycle(app, true),
        #[cfg(mobile)]
        tauri::RunEvent::WindowEvent { event: tauri::WindowEvent::Resumed, .. } => lifecycle(app, false),
        // Mobile: URLs (keine Pfade) – Bytes in die Sandbox kopieren, `content://` über das Plugin
        #[cfg(mobile)]
        tauri::RunEvent::Opened { urls } => take_pending_documents(app, urls),
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Opened { urls } => {
            deliver_documents(app, files::documents_from_urls(&urls));
            focus_main_window(app);
        }
        _ => {}
    });
}
