//! „Signatur fortsetzen“ (Mobile-Lebenszyklus, Plan §3.5, §7.7).
//!
//! Ein App-Wechsel – etwa in die App „ID Austria“ zur Bestätigung – ist **kein Abbruch**. Beendet das
//! System die App in dieser Zeit, geht nur die laufende A-Trust-Sitzung verloren (Cookies, Anmeldung;
//! das ID-Austria-Passwort wird nie persistiert). Damit die Nutzerin nicht von vorn beginnen muss,
//! sichert der Kern nach `prepare` den vorbereiteten Zustand in der App-Sandbox:
//!
//! | Datei (unter `Paths::temp_dir/resume/`, Ordner 0700, Dateien 0600) | Inhalt |
//! |---|---|
//! | `resume.json` | [`ResumeRecord`]: Name, Signiermittel, Position, wartende Dokumente, Regel für vorhandene Zieldateien (Stapel), Prüfsummen, Zeitpunkt |
//! | `document/<Dateiname>` | Kopie des Originals (Eingabe beim Fortsetzen; der Dateiname bestimmt den Ausgabenamen) |
//! | `pida-prepared-….pdf` + `.json` | `pades::Prepared::persist` (Platzhalter-PDF und Begleitdatei) |
//!
//! Beim nächsten Start bietet der Kern „Signatur fortsetzen“ an ([`crate::ResumeOfferView`]); danach
//! lädt der Workflow das Dokument (`State::Resuming`), öffnet die Sitzung des gespeicherten
//! Signiermittels neu und ersetzt `prepare` durch [`restore`] – Signaturzeitpunkt `/M`, Block und
//! Position bleiben die des ersten Versuchs (WYSIWYS). Nach Erfolg, Abbruch oder Fehler löscht
//! `Effect::DeletePersisted` den Ordner; ein Zustand älter als [`MAX_AGE`] wird nicht mehr
//! angeboten. Auf dem Desktop ist die Persistenz aus (`CoreOptions::persist_prepared`).

use pida_fs::PathExt as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pida_backend_api::BackendId;
use pida_cades::DigestAlg;
use pida_pades::PreparedHandle;
use pida_pdf_cos::SourcePdf;
use pida_workflow::{ExistingPolicy, PositionChoice};

use crate::engine::{PadesEngine, PreparedDoc};

/// Ordner unter `Paths::temp_dir`.
pub(crate) const RESUME_DIR: &str = "resume";
/// Datensatz.
pub(crate) const RECORD_FILE: &str = "resume.json";
/// Unterordner mit der Kopie des Originals.
const DOCUMENT_DIR: &str = "document";
/// Format des Datensatzes.
const FORMAT: u32 = 1;
/// Höchstalter eines angebotenen Zustands: Danach wäre der Signaturzeitpunkt `/M` des vorbereiteten
/// Dokuments irreführend alt (Plan §7.7 „Zeit-Plausibilisierung“); der Zustand wird verworfen.
pub(crate) const MAX_AGE: Duration = Duration::from_secs(60 * 60);
/// Meldungs-ID: ein gespeicherter Vorgang war zu alt und wurde verworfen.
pub(crate) const MSG_EXPIRED: &str = "ng-resume-expired";
/// Meldungs-ID: Wiederherstellen fehlgeschlagen – das Dokument wird neu vorbereitet.
pub(crate) const MSG_RESTORE_FAILED: &str = "ng-resume-restore-failed";

/// Datensatz „Signatur fortsetzen“. `Debug` ohne Pfade und Namen (Regel 9).
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ResumeRecord {
    format: u32,
    /// Sekunden seit 1970 (UTC) beim Persistieren
    pub created_unix: u64,
    /// Anzeigename des Dokuments
    pub name: String,
    /// Kopie des Originals relativ zum Ordner (`document/<Dateiname>`)
    document: PathBuf,
    /// SHA-256 der Kopie (hex)
    document_sha256: String,
    /// vorbereiteter Zustand (Dateiname relativ zum Ordner) und Prüfsumme von `pades`
    prepared_file: PathBuf,
    prepared_sha256: String,
    /// Position des vorbereiteten Dokuments
    pub position: PositionChoice,
    /// Signiermittel
    pub backend: BackendId,
    /// wartende Dokumente des Batches (absolute Pfade in der Sandbox; beim Fortsetzen nur vorhandene)
    pub pending: Vec<PathBuf>,
    /// Stapellauf: beim Start gewählte Regel für vorhandene Zieldateien – gilt auch für das fortgesetzte
    /// Dokument („Beide behalten“ statt der Rückfrage „Datei überschreiben?“). `None` = Einzeldokument bzw.
    /// Datensatz ohne Angabe (vor dieser Ergänzung gesichert; Format unverändert)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_existing: Option<ExistingPolicy>,
}

impl std::fmt::Debug for ResumeRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResumeRecord")
            .field("created_unix", &self.created_unix)
            .field("backend", &self.backend)
            .field("pending", &self.pending.len())
            .finish_non_exhaustive()
    }
}

/// Ein gültiger, angebotener Zustand: Datensatz und Ordner.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Persisted {
    pub dir: PathBuf,
    pub record: ResumeRecord,
}

impl Persisted {
    /// Kopie des Originals (Eingabedokument beim Fortsetzen).
    pub(crate) fn document(&self) -> PathBuf {
        self.dir.join(&self.record.document)
    }

    /// Alter in Minuten (Anzeige).
    pub(crate) fn age_minutes(&self, now: SystemTime) -> u32 {
        let secs = unix(now).saturating_sub(self.record.created_unix);
        u32::try_from(secs / 60).unwrap_or(u32::MAX)
    }

    /// Wartende Dokumente, die noch vorhanden sind.
    pub(crate) fn existing_pending(&self) -> Vec<PathBuf> {
        self.record.pending.iter().filter(|p| p.fs_is_file()).cloned().collect()
    }

    /// Stellt das vorbereitete Dokument für `src` wieder her (Prüfsummen, Original, Offsets).
    pub(crate) fn restore(&self, engine: &dyn PadesEngine, src: &SourcePdf) -> Result<Box<dyn PreparedDoc>, String> {
        let sha256 = hex::decode(&self.record.prepared_sha256).map_err(|_| "Prüfsumme ungültig".to_owned())?;
        let handle = PreparedHandle { path: self.dir.join(&self.record.prepared_file), sha256 };
        engine.restore(&handle, src).map_err(|e| e.code().to_owned())
    }
}

/// Ergebnis von [`load`].
#[derive(Debug, PartialEq)]
pub(crate) enum Loaded {
    /// nichts gespeichert
    Nothing,
    /// gültig und nicht zu alt
    Offer(Box<Persisted>),
    /// älter als [`MAX_AGE`] (verworfen)
    Expired,
    /// unlesbar bzw. unvollständig (verworfen)
    Invalid(String),
}

fn unix(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn sha256_hex(bytes: &[u8]) -> String {
    DigestAlg::Sha256.digest(bytes).to_hex()
}

/// Dateiname ohne Pfadanteile und Steuerzeichen (Ausgabename bleibt wie beim Original).
fn safe_file_name(name: &str) -> String {
    let cleaned: String =
        name.chars().map(|c| if c.is_control() || matches!(c, '/' | '\\' | ':' | '\0') { '_' } else { c }).collect();
    let trimmed = cleaned.trim().trim_start_matches('.');
    if trimmed.is_empty() { "Dokument.pdf".to_owned() } else { trimmed.to_owned() }
}

#[cfg(unix)]
fn restrict(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    pida_fs::set_permissions(path, pida_fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn restrict(_path: &Path, _mode: u32) -> std::io::Result<()> {
    Ok(())
}

/// Atomar schreiben (Temp-Datei im selben Ordner, Rechte 0600, Umbenennen).
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("part");
    pida_fs::write(&tmp, bytes)?;
    restrict(&tmp, 0o600)?;
    pida_fs::rename(&tmp, path)
}

/// Sichert einen vorbereiteten Vorgang (nach `prepare`, vor `sign`). Ein vorhandener Zustand wird
/// ersetzt. Fehler sind nicht fatal (der Vorgang läuft weiter, nur ohne „Fortsetzen“).
pub(crate) fn persist(
    dir: &Path,
    name: &str,
    source: &[u8],
    prepared: &dyn PreparedDoc,
    position: &PositionChoice,
    backend: BackendId,
    pending: Vec<PathBuf>,
    on_existing: Option<ExistingPolicy>,
    now: SystemTime,
) -> Result<(), String> {
    delete(dir);
    let io = |e: std::io::Error| format!("{:?}", e.kind());
    pida_fs::create_dir_all(dir.join(DOCUMENT_DIR)).map_err(io)?;
    restrict(dir, 0o700).map_err(io)?;
    restrict(&dir.join(DOCUMENT_DIR), 0o700).map_err(io)?;
    let document = Path::new(DOCUMENT_DIR).join(safe_file_name(name));
    write_private(&dir.join(&document), source).map_err(io)?;
    let handle = prepared.persist(dir).map_err(|e| e.code().to_owned())?;
    for p in [handle.path.clone(), handle.path.with_extension("json")] {
        if p.fs_is_file() {
            restrict(&p, 0o600).map_err(io)?;
        }
    }
    let prepared_file = handle
        .path
        .strip_prefix(dir)
        .map(Path::to_path_buf)
        .map_err(|_| "vorbereiteter Zustand außerhalb des Ordners".to_owned())?;
    let record = ResumeRecord {
        format: FORMAT,
        created_unix: unix(now),
        name: name.to_owned(),
        document,
        document_sha256: sha256_hex(source),
        prepared_file,
        prepared_sha256: hex::encode(&handle.sha256),
        position: position.clone(),
        backend,
        pending,
        on_existing,
    };
    let json = serde_json::to_vec(&record).map_err(|e| e.to_string())?;
    write_private(&dir.join(RECORD_FILE), &json).map_err(io)
}

/// Lädt einen gespeicherten Zustand und prüft Vollständigkeit, Prüfsumme der Dokumentkopie und Alter.
pub(crate) fn load(dir: &Path, now: SystemTime) -> Loaded {
    let file = dir.join(RECORD_FILE);
    let json = match pida_fs::read(&file) {
        Ok(j) => j,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return if dir.fs_exists() { Loaded::Invalid("Datensatz fehlt".into()) } else { Loaded::Nothing };
        }
        Err(e) => return Loaded::Invalid(format!("{:?}", e.kind())),
    };
    let record: ResumeRecord = match serde_json::from_slice(&json) {
        Ok(r) => r,
        Err(e) => return Loaded::Invalid(format!("Datensatz nicht lesbar (Zeile {})", e.line())),
    };
    if record.format != FORMAT {
        return Loaded::Invalid("Format".into());
    }
    // ein unsinniger Zeitpunkt (beschädigte Datei) darf nicht zum Absturz bei jedem Start führen
    let Some(created) = UNIX_EPOCH.checked_add(Duration::from_secs(record.created_unix)) else {
        return Loaded::Invalid("Zeitpunkt".into());
    };
    // Uhr zurückgestellt (Zeitpunkt in der Zukunft) gilt wie „zu alt“: nicht plausibel
    let age = now.duration_since(created).unwrap_or(Duration::MAX);
    if age > MAX_AGE {
        return Loaded::Expired;
    }
    // nur relative Pfade innerhalb des Ordners
    let inside = |p: &Path| p.is_relative() && p.components().all(|c| matches!(c, std::path::Component::Normal(_)));
    if !inside(&record.document) || !inside(&record.prepared_file) {
        return Loaded::Invalid("Pfad außerhalb des Ordners".into());
    }
    match pida_fs::read(dir.join(&record.document)) {
        Ok(bytes) if sha256_hex(&bytes) == record.document_sha256 => {}
        Ok(_) => return Loaded::Invalid("Dokumentkopie verändert".into()),
        Err(e) => return Loaded::Invalid(format!("Dokumentkopie: {:?}", e.kind())),
    }
    if !dir.join(&record.prepared_file).fs_is_file() {
        return Loaded::Invalid("vorbereiteter Zustand fehlt".into());
    }
    Loaded::Offer(Box::new(Persisted { dir: dir.to_path_buf(), record }))
}

/// Löscht den gespeicherten Zustand (Ordner samt Inhalt); ein fehlender Ordner ist kein Fehler.
pub(crate) fn delete(dir: &Path) {
    match pida_fs::remove_dir_all(dir) {
        Ok(()) => tracing::debug!("persistierter Signierzustand gelöscht"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => tracing::warn!(kind = ?e.kind(), "persistierter Signierzustand nicht gelöscht"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeEngine;
    use pida_pades::{Reserve, SignPlan};
    use pida_placement::Placement;

    const PDF: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/pdf/generated/simple-xref-table.pdf");

    fn prepared(engine: &FakeEngine, src: &SourcePdf) -> Box<dyn PreparedDoc> {
        let mut plan = SignPlan::invisible(time::OffsetDateTime::now_utc());
        plan.reserve = Reserve::ModelB;
        engine.prepare(src, &plan, None).expect("prepare")
    }

    fn open(bytes: &[u8]) -> SourcePdf {
        SourcePdf::open(bytes.to_vec(), &pida_pdf_cos::Limits::default()).expect("PDF")
    }

    fn position() -> PositionChoice {
        PositionChoice::Manual(Placement { page_index: 0, x: 10.0, y: 200.0, width: 100.0, height: 40.0 })
    }

    #[test]
    fn persist_load_restore_delete_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(RESUME_DIR);
        let bytes = pida_fs::read(PDF).unwrap();
        let src = open(&bytes);
        let engine = FakeEngine::default();
        let p = prepared(&engine, &src);
        let pending = tmp.path().join("b.pdf");
        pida_fs::write(&pending, b"%PDF-1.7").unwrap();
        let gone = tmp.path().join("weg.pdf");
        let now = pida_fs::system_now();
        persist(
            &dir,
            "Vertrag 1.pdf",
            &bytes,
            p.as_ref(),
            &position(),
            BackendId::AtrustNative,
            vec![pending.clone(), gone],
            Some(ExistingPolicy::KeepBoth),
            now,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(pida_fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
            let record = pida_fs::metadata(dir.join(RECORD_FILE)).unwrap().permissions().mode() & 0o777;
            assert_eq!(record, 0o600);
        }
        let Loaded::Offer(persisted) = load(&dir, now + Duration::from_secs(125)) else { panic!("kein Angebot") };
        assert_eq!(persisted.record.name, "Vertrag 1.pdf");
        assert_eq!(persisted.record.backend, BackendId::AtrustNative);
        assert_eq!(persisted.record.position, position());
        assert_eq!(persisted.record.on_existing, Some(ExistingPolicy::KeepBoth), "Regel des Stapels");
        assert_eq!(persisted.age_minutes(now + Duration::from_secs(125)), 2);
        assert_eq!(persisted.document().file_name().unwrap(), "Vertrag 1.pdf", "Ausgabename wie beim Original");
        assert_eq!(pida_fs::read(persisted.document()).unwrap(), bytes);
        assert_eq!(persisted.existing_pending(), vec![pending]);
        assert!(!format!("{persisted:?}").contains("Vertrag"), "Debug ohne Namen");
        let restored = persisted.restore(&engine, &src).expect("restore");
        assert_eq!(restored.total_len(), p.total_len());
        assert_eq!(restored.signing_time().unix_timestamp(), p.signing_time().unix_timestamp());
        // anderes Original passt nicht
        let other = open(&[bytes.as_slice(), b"\n%x\n"].concat());
        assert!(persisted.restore(&engine, &other).is_err());
        delete(&dir);
        assert!(!dir.fs_exists());
        assert_eq!(load(&dir, now), Loaded::Nothing);
        delete(&dir);
    }

    #[test]
    fn old_tampered_or_incomplete_states_are_not_offered() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(RESUME_DIR);
        let bytes = pida_fs::read(PDF).unwrap();
        let src = open(&bytes);
        let engine = FakeEngine::default();
        let p = prepared(&engine, &src);
        let now = pida_fs::system_now();
        persist(&dir, "a.pdf", &bytes, p.as_ref(), &PositionChoice::Auto, BackendId::Keystore, vec![], None, now)
            .unwrap();
        assert_eq!(load(&dir, now + MAX_AGE + Duration::from_secs(1)), Loaded::Expired);
        // Uhr zurückgestellt
        assert_eq!(load(&dir, now - Duration::from_secs(3600)), Loaded::Expired);
        // Dokumentkopie verändert
        pida_fs::write(dir.join(DOCUMENT_DIR).join("a.pdf"), b"anders").unwrap();
        assert!(matches!(load(&dir, now), Loaded::Invalid(_)));
        // Datensatz kaputt
        pida_fs::write(dir.join(RECORD_FILE), b"{").unwrap();
        assert!(matches!(load(&dir, now), Loaded::Invalid(_)));
        // Ordner ohne Datensatz
        pida_fs::remove_file(dir.join(RECORD_FILE)).unwrap();
        assert!(matches!(load(&dir, now), Loaded::Invalid(_)));
    }

    /// Datensätze ohne `on_existing` (vor der Ergänzung gesichert) bleiben lesbar: Einzeldokument-Verhalten;
    /// ein Einzeldokument schreibt das Feld nicht (Format unverändert).
    #[test]
    fn records_without_a_batch_policy_still_load() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(RESUME_DIR);
        let bytes = pida_fs::read(PDF).unwrap();
        let src = open(&bytes);
        let engine = FakeEngine::default();
        let p = prepared(&engine, &src);
        let now = pida_fs::system_now();
        persist(&dir, "a.pdf", &bytes, p.as_ref(), &PositionChoice::Auto, BackendId::Keystore, vec![], None, now)
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&pida_fs::read(dir.join(RECORD_FILE)).unwrap()).unwrap();
        assert!(json.get("on_existing").is_none(), "Einzeldokument: Feld fehlt wie im bisherigen Format");
        let Loaded::Offer(persisted) = load(&dir, now) else { panic!("kein Angebot") };
        assert_eq!(persisted.record.on_existing, None);
        // Stapel: als snake_case gespeichert und wieder gelesen
        let mut json = json;
        json["on_existing"] = serde_json::Value::String("keep_both".into());
        pida_fs::write(dir.join(RECORD_FILE), serde_json::to_vec(&json).unwrap()).unwrap();
        let Loaded::Offer(persisted) = load(&dir, now) else { panic!("kein Angebot") };
        assert_eq!(persisted.record.on_existing, Some(ExistingPolicy::KeepBoth));
    }

    #[test]
    fn records_with_paths_outside_the_folder_are_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(RESUME_DIR);
        pida_fs::create_dir_all(&dir).unwrap();
        let record = ResumeRecord {
            format: FORMAT,
            created_unix: unix(pida_fs::system_now()),
            name: "x.pdf".into(),
            document: PathBuf::from("../../etc/passwd"),
            document_sha256: String::new(),
            prepared_file: PathBuf::from("p.json"),
            prepared_sha256: String::new(),
            position: PositionChoice::Auto,
            backend: BackendId::Keystore,
            pending: vec![],
            on_existing: None,
        };
        pida_fs::write(dir.join(RECORD_FILE), serde_json::to_vec(&record).unwrap()).unwrap();
        assert_eq!(load(&dir, pida_fs::system_now()), Loaded::Invalid("Pfad außerhalb des Ordners".into()));
    }

    #[test]
    fn file_names_stay_inside_the_folder() {
        assert_eq!(safe_file_name("../../x.pdf"), "_.._x.pdf");
        assert_eq!(safe_file_name("a/b\\c:d.pdf"), "a_b_c_d.pdf");
        assert_eq!(safe_file_name(""), "Dokument.pdf");
        assert_eq!(safe_file_name("..."), "Dokument.pdf");
        assert_eq!(safe_file_name("Vertrag ä.pdf"), "Vertrag ä.pdf");
    }
}
