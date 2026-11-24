//! `tauri-plugin-pida-mobile` – gemeinsames Mobile-Plugin von PIDA (Plan §5.2/§5.3,
//! docs/integration/mobile.md).
//!
//! Die Rust-Seite ist plattformneutral; die nativen Seiten setzen den Vertrag um (iOS:
//! `ios/Sources/*.swift`, docs/integration/mobile-ios.md; Android: `android/src/main/java/.../*.kt`,
//! docs/integration/mobile-android.md). Fehlt eine Methode (Code `not-implemented`), fällt die Shell auf
//! sichere Rückfälle zurück (Secrets nur im RAM, kein Teilen/Exportieren).
//!
//! Das Plugin hat **keine** Befehle für die WebView – nur die Rust-Shell ruft die nativen Methoden über
//! [`PidaMobile`] auf (der sichere Speicher wäre sonst aus der Oberfläche lesbar).
//!
//! # Vertrag mit der nativen Seite
//!
//! Methoden (Swift `@objc public func name(_ invoke: Invoke)`, Kotlin `@Command fun name(invoke: Invoke)`),
//! Argumente als JSON-Objekt (camelCase), Antwort über `invoke.resolve(...)`, Fehler über
//! `invoke.reject(message, code)`. Codes: `not-implemented` (Hülle), `cancelled` (Nutzerin hat
//! abgebrochen), sonst ein eigener kurzer Code ohne Pfade oder Werte.
//!
//! | Methode | Argumente | Antwort | Umsetzung iOS / Android |
//! |---|---|---|---|
//! | `secureStoreGet` | `{ key }` | `{ value: string \| null }` | Keychain (`kSecClassGenericPassword`, `kSecAttrAccessibleWhenUnlockedThisDeviceOnly`, nicht synchronisiert) / Android Keystore (AES-256-GCM-Schlüssel, Werte verschlüsselt in privaten SharedPreferences, ohne Sicherung) |
//! | `secureStoreSet` | `{ key, value }` | `{}` | wie oben (ersetzt einen vorhandenen Wert) |
//! | `secureStoreDelete` | `{ key }` | `{}` | wie oben (fehlender Eintrag ist kein Fehler) |
//! | `shareFile` | `{ path, mimeType }` | `{}` sobald das Share-Sheet gezeigt wird | `UIActivityViewController` / `ACTION_SEND` mit `FileProvider`-URI und `FLAG_GRANT_READ_URI_PERMISSION` |
//! | `openFile` | `{ path, mimeType }` | `{}` sobald die Anzeige startet | iOS: QuickLook (`QLPreviewController`, ohne Bearbeiten) / `ACTION_VIEW` mit `FileProvider`-URI (ohne die App selbst) |
//! | `exportFile` | `{ path, fileName, mimeType }` | `{ saved: bool, name?: string }` nach der Wahl (`name`: tatsächlicher Dateiname) | `UIDocumentPickerViewController(forExporting:asCopy:)` / SAF `ACTION_CREATE_DOCUMENT`, dann Bytes kopieren |
//! | `importFile` | `{ kind: "pdf" \| "pkcs12", multiple, destination }` | `{ files: [{ name, path }], skipped }` (Reihenfolge der Auswahl; `skipped` = gewählt, aber nicht kopiert) | `UIDocumentPickerViewController(forOpeningContentTypes:asCopy:true)` / SAF `ACTION_OPEN_DOCUMENT`; Inhalt nach `destination` kopieren (Security-Scoped bzw. `ContentResolver`) |
//! | `takePendingOpenDocuments` | `{ urls: [string], destination }` | `{ files: [{ name, path }] }` | eingegangene Dokumente („Öffnen in“/Share bzw. Intents `VIEW`/`SEND`/`SEND_MULTIPLE` `application/pdf`) **und** die übergebenen URLs (Security-Scoped `file://` bzw. `content://`) nach `destination` kopieren; die Warteschlange danach leeren |
//!
//! `destination` ist ein Ordner in der App-Sandbox, den die Shell vorgibt; zurückgegebene Pfade müssen
//! darin liegen (die Rust-Seite prüft das, [`within`]). Dateinamen ohne Pfadanteile.
//!
//! **Android, TLS:** `rustls-platform-verifier` braucht den JNI-Kontext der App; das Plugin initialisiert
//! ihn beim Start aus dem von tao gesetzten `ndk-context` ([`tls`]). Zusätzlich muss die Kotlin-Komponente
//! von `rustls-platform-verifier` im Gradle-Build stehen (Aufgabe M-Android, docs/integration/mobile.md).
//!
//! Aufrufe blockieren bis zur Antwort der nativen Seite und dürfen **nicht auf dem Hauptthread** laufen
//! (iOS führt die Methoden dort aus – Verklemmung). Die Shell ruft sie aus Hintergrund-Threads auf.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Manager, Runtime};

mod error;
#[cfg(mobile)]
mod mobile;
pub mod tls;

pub use error::{Error, Result};

/// Name des Plugins (Tauri).
pub const PLUGIN_NAME: &str = "pida-mobile";
/// Android: Paket der Kotlin-Klasse `PidaMobilePlugin`.
pub const ANDROID_PACKAGE: &str = "at.asit.pida.mobile";

/// Art einer zu importierenden Datei (Dokument-Picker bzw. SAF-Filter).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportKind {
    /// PDF-Dokumente (`com.adobe.pdf` bzw. `application/pdf`)
    Pdf,
    /// PKCS#12-Keystore (`.p12`/`.pfx`, `application/x-pkcs12`)
    Pkcs12,
}

/// In die Sandbox kopierte Datei.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportedFile {
    /// Anzeigename (Dateiname ohne Pfad)
    pub name: String,
    /// Kopie in der Sandbox (innerhalb des vorgegebenen Ordners)
    pub path: PathBuf,
}

/// Aufruf einer nativen Methode (Mobile: `PluginHandle::run_mobile_plugin`; Desktop: nicht unterstützt;
/// Tests: Attrappe).
pub trait Native: Send + Sync {
    /// Ruft `method` mit `payload` auf und liefert die Antwort als JSON.
    fn call(&self, method: &str, payload: serde_json::Value) -> Result<serde_json::Value>;
}

/// Desktop: keine native Seite.
#[derive(Debug, Clone, Copy, Default)]
pub struct Unsupported;

impl Native for Unsupported {
    fn call(&self, _method: &str, _payload: serde_json::Value) -> Result<serde_json::Value> {
        Err(Error::Unsupported)
    }
}

/// Zugriff auf die nativen Methoden (Tauri-State, [`PidaMobileExt::pida_mobile`]).
#[derive(Clone)]
pub struct PidaMobile {
    native: Arc<dyn Native>,
}

impl std::fmt::Debug for PidaMobile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PidaMobile")
    }
}

#[derive(Deserialize)]
struct ValueResponse {
    #[serde(default)]
    value: Option<String>,
}

#[derive(Deserialize)]
struct ExportResponse {
    #[serde(default)]
    saved: bool,
    /// tatsächlicher Dateiname am Ziel (iOS: Name der exportierten Kopie, Android: `DISPLAY_NAME` des Dokuments)
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct FilesResponse {
    #[serde(default)]
    files: Vec<ImportedFile>,
    /// gewählt, aber nicht kopiert (nicht lesbar, zu groß; ältere native Seiten ohne Feld: 0)
    #[serde(default)]
    skipped: usize,
}

/// Ergebnis der Dateiauswahl ([`PidaMobile::import_files`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Imported {
    /// Kopien in der Sandbox, in der Reihenfolge der Auswahl
    pub files: Vec<ImportedFile>,
    /// gewählte Dateien, die nicht übernommen wurden (native Seite konnte sie nicht kopieren bzw. Kopie
    /// außerhalb des Zielordners)
    pub skipped: usize,
}

fn parse<T: for<'de> Deserialize<'de>>(value: serde_json::Value) -> Result<T> {
    // `invoke.resolve()` ohne Daten liefert `null`
    let value = if value.is_null() { serde_json::json!({}) } else { value };
    serde_json::from_value(value).map_err(|_| Error::Native("bad-response".into()))
}

/// Liegt `path` innerhalb von `dir` (ohne `..`, nach Auflösen von Symlinks, falls vorhanden)?
pub fn within(dir: &Path, path: &Path) -> bool {
    if path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return false;
    }
    let dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    path.starts_with(&dir) && path != dir
}

impl PidaMobile {
    /// Mit einer eigenen nativen Seite (Mobile intern, Tests).
    pub fn with_native(native: Arc<dyn Native>) -> Self {
        Self { native }
    }

    /// Desktop bzw. ohne native Seite: jede Methode liefert [`Error::Unsupported`].
    pub fn unsupported() -> Self {
        Self::with_native(Arc::new(Unsupported))
    }

    /// Liest einen Wert aus dem sicheren Speicher (`None` = nicht vorhanden).
    pub fn secure_store_get(&self, key: &str) -> Result<Option<String>> {
        let r: ValueResponse = parse(self.native.call("secureStoreGet", serde_json::json!({ "key": key }))?)?;
        Ok(r.value)
    }

    /// Schreibt einen Wert in den sicheren Speicher.
    pub fn secure_store_set(&self, key: &str, value: &str) -> Result<()> {
        self.native.call("secureStoreSet", serde_json::json!({ "key": key, "value": value }))?;
        Ok(())
    }

    /// Löscht einen Wert aus dem sicheren Speicher.
    pub fn secure_store_delete(&self, key: &str) -> Result<()> {
        self.native.call("secureStoreDelete", serde_json::json!({ "key": key }))?;
        Ok(())
    }

    /// Teilt eine Datei (Share-Sheet bzw. `ACTION_SEND`).
    pub fn share_file(&self, path: &Path, mime_type: &str) -> Result<()> {
        let payload = serde_json::json!({ "path": path, "mimeType": mime_type });
        self.native.call("shareFile", payload)?;
        Ok(())
    }

    /// Zeigt eine Datei in einer anderen App an (Ergebnis „Öffnen“, F-87).
    pub fn open_file(&self, path: &Path, mime_type: &str) -> Result<()> {
        let payload = serde_json::json!({ "path": path, "mimeType": mime_type });
        self.native.call("openFile", payload)?;
        Ok(())
    }

    /// Exportiert eine Datei („Sichern unter“); `Ok(Some(name))` = gesichert unter dem tatsächlichen Dateinamen
    /// (ohne Angabe der nativen Seite der Vorschlag `file_name`), `Ok(None)` = abgebrochen.
    pub fn export_file(&self, path: &Path, file_name: &str, mime_type: &str) -> Result<Option<String>> {
        let payload = serde_json::json!({ "path": path, "fileName": file_name, "mimeType": mime_type });
        match self.native.call("exportFile", payload) {
            Ok(v) => {
                let r = parse::<ExportResponse>(v)?;
                // nur ein Dateiname (Schutz gegen fehlerhafte native Seiten), sonst der Vorschlag
                let name = r
                    .name
                    .as_deref()
                    .and_then(|n| Path::new(n).file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| file_name.to_owned());
                Ok(r.saved.then_some(name))
            }
            Err(Error::Cancelled) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// System-Dateiauswahl; die gewählten Dateien kopiert die native Seite nach `destination`.
    /// Abbruch = leere Liste.
    pub fn import_file(&self, kind: ImportKind, multiple: bool, destination: &Path) -> Result<Vec<ImportedFile>> {
        Ok(self.import_files(kind, multiple, destination)?.files)
    }

    /// Wie [`Self::import_file`], dazu die Zahl der gewählten, aber nicht übernommenen Dateien (Mehrfachauswahl,
    /// ADR 0004 Nachtrag 17: Hinweis statt stillem Verwerfen). Reihenfolge der Auswahl. Abbruch = leer.
    pub fn import_files(&self, kind: ImportKind, multiple: bool, destination: &Path) -> Result<Imported> {
        let payload = serde_json::json!({ "kind": kind, "multiple": multiple, "destination": destination });
        match self.native.call("importFile", payload) {
            Ok(v) => {
                let r = parse::<FilesResponse>(v)?;
                let returned = r.files.len();
                let files = checked(r.files, destination);
                // `skipped` kommt aus der nativen Schicht: nie überlaufen lassen
                Ok(Imported { skipped: r.skipped.saturating_add(returned.saturating_sub(files.len())), files })
            }
            Err(Error::Cancelled) => Ok(Imported::default()),
            Err(e) => Err(e),
        }
    }

    /// Eingegangene Dokumente („Öffnen in“, Share, Intents) und die übergebenen, von Rust nicht lesbaren
    /// URLs (`content://`, Security-Scoped) als Kopien in `destination`.
    pub fn take_pending_open_documents(&self, urls: &[String], destination: &Path) -> Result<Vec<ImportedFile>> {
        let payload = serde_json::json!({ "urls": urls, "destination": destination });
        let files = parse::<FilesResponse>(self.native.call("takePendingOpenDocuments", payload)?)?.files;
        Ok(checked(files, destination))
    }
}

/// Nur Dateien innerhalb von `destination` (Schutz gegen fehlerhafte native Seiten).
fn checked(files: Vec<ImportedFile>, destination: &Path) -> Vec<ImportedFile> {
    files
        .into_iter()
        .filter(|f| {
            let ok = within(destination, &f.path) && f.path.is_file();
            if !ok {
                tracing::warn!("native Seite lieferte eine Datei außerhalb des Zielordners – ignoriert");
            }
            ok
        })
        .collect()
}

/// Zugriff aus `App`, `AppHandle`, `Window` …
pub trait PidaMobileExt<R: Runtime> {
    /// Die nativen Methoden.
    fn pida_mobile(&self) -> PidaMobile;
}

impl<R: Runtime, T: Manager<R>> PidaMobileExt<R> for T {
    fn pida_mobile(&self) -> PidaMobile {
        self.try_state::<PidaMobile>().map(|s| s.inner().clone()).unwrap_or_else(PidaMobile::unsupported)
    }
}

/// Registriert das Plugin (Desktop: ohne native Seite; Mobile: Swift-/Kotlin-Klasse, unter Android
/// zusätzlich die TLS-Initialisierung).
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new(PLUGIN_NAME)
        .setup(|app, api| {
            #[cfg(mobile)]
            let plugin = mobile::init(app, api)?;
            #[cfg(desktop)]
            let plugin = {
                let _ = (app, api);
                PidaMobile::unsupported()
            };
            if let Err(e) = tls::init_platform_verifier() {
                tracing::warn!(code = e.code(), "TLS-Prüfung der Plattform nicht initialisiert");
            }
            app.manage(plugin);
            Ok(())
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Antwort der Attrappe je Methode und Argumenten.
    type Answer = Box<dyn Fn(&str, &serde_json::Value) -> Result<serde_json::Value> + Send + Sync>;

    /// Native Seite im Test: zeichnet Aufrufe auf und antwortet fest.
    struct Fake {
        calls: Mutex<Vec<(String, serde_json::Value)>>,
        answer: Answer,
    }

    impl Native for Fake {
        fn call(&self, method: &str, payload: serde_json::Value) -> Result<serde_json::Value> {
            let r = (self.answer)(method, &payload);
            self.calls.lock().unwrap().push((method.to_owned(), payload));
            r
        }
    }

    fn fake(
        answer: impl Fn(&str, &serde_json::Value) -> Result<serde_json::Value> + Send + Sync + 'static,
    ) -> (PidaMobile, Arc<Fake>) {
        let f = Arc::new(Fake { calls: Mutex::new(Vec::new()), answer: Box::new(answer) });
        (PidaMobile::with_native(f.clone()), f)
    }

    #[test]
    fn desktop_is_unsupported() {
        let m = PidaMobile::unsupported();
        assert!(matches!(m.secure_store_get("k"), Err(Error::Unsupported)));
        assert!(matches!(m.share_file(Path::new("/x.pdf"), "application/pdf"), Err(Error::Unsupported)));
        assert_eq!(Error::Unsupported.code(), "ng-mobile-unsupported");
    }

    #[test]
    fn empty_native_shells_report_not_implemented() {
        let (m, _) = fake(|_, _| Err(Error::from_native_code(Some("not-implemented"))));
        let e = m.secure_store_set("PIDA/x", "test").unwrap_err();
        assert!(matches!(e, Error::NotImplemented));
        assert_eq!(e.code(), "ng-mobile-not-implemented");
        assert!(!e.to_string().contains("test"));
    }

    #[test]
    fn secure_store_contract() {
        let (m, f) = fake(|method, p| match method {
            "secureStoreGet" if p["key"] == "a" => Ok(serde_json::json!({ "value": "test" })),
            "secureStoreGet" => Ok(serde_json::json!({ "value": null })),
            _ => Ok(serde_json::Value::Null),
        });
        assert_eq!(m.secure_store_get("a").unwrap().as_deref(), Some("test"));
        assert_eq!(m.secure_store_get("b").unwrap(), None);
        m.secure_store_set("a", "neu").unwrap();
        m.secure_store_delete("a").unwrap();
        let calls = f.calls.lock().unwrap();
        assert_eq!(calls[2], ("secureStoreSet".to_owned(), serde_json::json!({ "key": "a", "value": "neu" })));
        assert_eq!(calls[3].0, "secureStoreDelete");
    }

    #[test]
    fn open_file_contract() {
        let (m, f) = fake(|_, _| Ok(serde_json::Value::Null));
        m.open_file(Path::new("/docs/a_signed.pdf"), "application/pdf").unwrap();
        assert_eq!(
            f.calls.lock().unwrap()[0],
            ("openFile".to_owned(), serde_json::json!({ "path": "/docs/a_signed.pdf", "mimeType": "application/pdf" }))
        );
        let (m, _) = fake(|_, _| Err(Error::from_native_code(Some("no-app"))));
        assert_eq!(m.open_file(Path::new("/a.pdf"), "application/pdf").unwrap_err(), Error::Native("no-app".into()));
    }

    #[test]
    fn export_and_import_treat_cancel_as_no_result() {
        let (m, _) = fake(|_, _| Err(Error::from_native_code(Some("cancelled"))));
        assert_eq!(m.export_file(Path::new("/a.pdf"), "a.pdf", "application/pdf").unwrap(), None);
        assert!(m.import_file(ImportKind::Pkcs12, false, Path::new("/tmp")).unwrap().is_empty());
        let (m, _) = fake(|_, _| Ok(serde_json::json!({ "saved": false })));
        assert_eq!(m.export_file(Path::new("/a.pdf"), "a.pdf", "application/pdf").unwrap(), None);
        // tatsächlicher Name der nativen Seite (vom System nummeriert); nur der Dateiname zählt
        for (name, expected) in
            [("a_signed (3).pdf", "a_signed (3).pdf"), ("../x/b.pdf", "b.pdf"), ("", "a_signed.pdf")]
        {
            let (m, _) = fake(move |_, _| Ok(serde_json::json!({ "saved": true, "name": name })));
            let saved = m.export_file(Path::new("/a_signed.pdf"), "a_signed.pdf", "application/pdf").unwrap();
            assert_eq!(saved.as_deref(), Some(expected));
        }
        let (m, f) = fake(|_, _| Ok(serde_json::json!({ "saved": true })));
        assert_eq!(
            m.export_file(Path::new("/a_signed.pdf"), "a_signed.pdf", "application/pdf").unwrap().as_deref(),
            Some("a_signed.pdf"),
            "ohne Angabe der Vorschlag"
        );
        let calls = f.calls.lock().unwrap();
        assert_eq!(
            calls[0].1,
            serde_json::json!({ "path": "/a_signed.pdf", "fileName": "a_signed.pdf", "mimeType": "application/pdf" })
        );
    }

    #[test]
    fn imported_files_must_lie_in_the_destination() {
        let tmp = std::env::temp_dir().join(format!("pida-mobile-plugin-{}", std::process::id()));
        let dest = tmp.join("import");
        std::fs::create_dir_all(&dest).unwrap();
        let inside = dest.join("k.p12");
        std::fs::write(&inside, b"x").unwrap();
        let outside = tmp.join("fremd.p12");
        std::fs::write(&outside, b"x").unwrap();
        let (i, o) = (inside.clone(), outside.clone());
        let (m, f) = fake(move |_, _| {
            Ok(serde_json::json!({ "files": [
                { "name": "k.p12", "path": i },
                { "name": "fremd.p12", "path": o },
                { "name": "x", "path": dest_join_parent(&i) },
            ] }))
        });
        let files = m.import_file(ImportKind::Pkcs12, true, &dest).unwrap();
        assert_eq!(files, vec![ImportedFile { name: "k.p12".into(), path: inside }]);
        assert_eq!(f.calls.lock().unwrap()[0].1["kind"], "pkcs12");
        let docs = m.take_pending_open_documents(&["content://x/1".into()], &dest).unwrap();
        assert_eq!(docs.len(), 1);
        assert!(!within(&dest, &dest), "der Ordner selbst ist keine Datei darin");
        let _ = std::fs::remove_dir_all(tmp);
    }

    /// Mehrfachauswahl (ADR 0004 Nachtrag 17): alle Kopien in der Reihenfolge der Auswahl; was die native Seite
    /// nicht kopieren konnte bzw. außerhalb des Zielordners liegt, zählt als nicht übernommen.
    #[test]
    fn multiple_import_keeps_order_and_counts_skipped_files() {
        let tmp = std::env::temp_dir().join(format!("pida-mobile-multi-{}", std::process::id()));
        let dest = tmp.join("import");
        let names = ["Vertrag.pdf", "Angebot.pdf", "Rechnung.pdf"];
        let paths: Vec<PathBuf> = names
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let p = dest.join((i + 1).to_string()).join(n);
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                std::fs::write(&p, b"%PDF-1.7").unwrap();
                p
            })
            .collect();
        let outside = tmp.join("fremd.pdf");
        std::fs::write(&outside, b"%PDF-1.7").unwrap();
        let (listed, o) = (paths.clone(), outside.clone());
        let (m, f) = fake(move |_, _| {
            let mut files: Vec<serde_json::Value> = listed
                .iter()
                .map(|p| serde_json::json!({ "name": p.file_name().unwrap().to_str(), "path": p }))
                .collect();
            files.insert(1, serde_json::json!({ "name": "fremd.pdf", "path": o }));
            Ok(serde_json::json!({ "files": files, "skipped": 2 }))
        });
        let r = m.import_files(ImportKind::Pdf, true, &dest).unwrap();
        let got: Vec<&str> = r.files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(got, names, "Reihenfolge der Auswahl");
        assert_eq!(r.skipped, 3, "2 nicht kopiert + 1 außerhalb des Zielordners");
        assert_eq!(
            f.calls.lock().unwrap()[0].1,
            serde_json::json!({ "kind": "pdf", "multiple": true, "destination": dest })
        );
        // unsinnig großer Zähler der nativen Seite: kein Überlauf (Argus R-01)
        let o = outside.clone();
        let (m, _) = fake(move |_, _| {
            Ok(serde_json::json!({ "files": [{ "name": "fremd.pdf", "path": o }], "skipped": usize::MAX }))
        });
        assert_eq!(m.import_files(ImportKind::Pdf, true, &dest).unwrap().skipped, usize::MAX);
        // ältere native Seite ohne `skipped`
        let p0 = paths[0].clone();
        let (m, _) = fake(move |_, _| Ok(serde_json::json!({ "files": [{ "name": "Vertrag.pdf", "path": p0 }] })));
        assert_eq!(m.import_files(ImportKind::Pdf, true, &dest).unwrap().skipped, 0);
        let _ = std::fs::remove_dir_all(tmp);
    }

    fn dest_join_parent(p: &Path) -> PathBuf {
        p.parent().unwrap().join("..").join("fremd.p12")
    }

    #[test]
    fn malformed_native_answers_are_errors() {
        let (m, _) = fake(|_, _| Ok(serde_json::json!({ "value": 5 })));
        assert!(matches!(m.secure_store_get("a"), Err(Error::Native(c)) if c == "bad-response"));
    }
}
