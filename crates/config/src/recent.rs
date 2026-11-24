//! „Zuletzt signiert“ (ADR 0004 §3.10): höchstens [`MAX_RECENT`] Einträge in `<data_dir>/recent.json`,
//! neueste zuerst.
//!
//! - **Datensparsam:** gespeichert werden je Eintrag **nur** Kennung, Dateiname, Pfad und Zeitpunkt – kein
//!   Signiermittel, kein Unterzeichner, keine Seitenzahl. Die Datei ist eine JSON-Liste genau dieser
//!   Objekte (`id`, `file_name`, `path`, `signed_at`).
//! - **Nur für die Benutzerin lesbar:** atomar geschrieben (Temp-Datei, `fsync`, Umbenennen) mit Rechten
//!   0600 unter Unix wie die Einstellungen; Windows/Mobile: Profil- bzw. Sandbox-Ordner.
//! - **Kennungen** sind monoton: höchste bisherige + 1 (nach „Liste leeren“ beginnt die Zählung neu).
//! - Ein Eintrag mit demselben Pfad ersetzt den alten (die Datei wurde erneut signiert).
//! - Eine unlesbare Datei gilt als leer (Warnung ohne Pfad im Log, ADR 0001 Regel 9); der nächste
//!   Eintrag ersetzt sie.
//!
//! Ob überhaupt gespeichert wird, entscheidet der Kern (`general.recent_documents`, Richtlinie
//! [`crate::Policy::recent_documents`], nur mit Oberfläche); Abschalten löscht die Datei ([`RecentStore::clear`]).

use std::path::{Path, PathBuf};

use crate::paths::Paths;
use crate::{ConfigError, Result};

/// Höchstzahl der Einträge.
pub const MAX_RECENT: usize = 10;

/// Dateiname im Datenordner.
pub const RECENT_FILE: &str = "recent.json";

/// Größte gelesene Datei (zehn Einträge mit langen Pfaden passen mehrfach hinein).
const MAX_FILE_BYTES: u64 = 256 * 1024;

/// Eintrag „Zuletzt signiert“. `Debug` ohne Dateiname und Pfad (Regel 9).
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecentEntry {
    /// Kennung (monoton)
    pub id: u64,
    /// Dateiname der signierten Datei
    pub file_name: String,
    /// Pfad der signierten Datei
    pub path: PathBuf,
    /// Zeitpunkt (RFC 3339, lokale Zeit mit Offset)
    pub signed_at: String,
}

impl std::fmt::Debug for RecentEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecentEntry")
            .field("id", &self.id)
            .field("file_name", &"<redigiert>")
            .field("path", &"<redigiert>")
            .field("signed_at", &self.signed_at)
            .finish()
    }
}

/// Speicher „Zuletzt signiert“.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentStore {
    file: PathBuf,
}

impl RecentStore {
    /// Speicher unter `<data_dir>/recent.json`.
    pub fn new(paths: &Paths) -> Self {
        Self { file: paths.data_dir.join(RECENT_FILE) }
    }

    /// Pfad der Datei (Tests, Diagnose).
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// Einträge, neueste zuerst (höchstens [`MAX_RECENT`]); fehlt die Datei oder ist sie unlesbar, leer.
    pub fn load(&self) -> Vec<RecentEntry> {
        match self.read() {
            Ok(entries) => entries,
            Err(e) => {
                tracing::warn!(id = e.message_id(), "„Zuletzt signiert“ nicht lesbar – leere Liste");
                Vec::new()
            }
        }
    }

    fn read(&self) -> Result<Vec<RecentEntry>> {
        let meta = match pida_fs::metadata(&self.file) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(ConfigError::Io(e)),
        };
        if meta.len() > MAX_FILE_BYTES {
            return Err(ConfigError::Parse("recent.json zu groß".into()));
        }
        let bytes = pida_fs::read(&self.file)?;
        let mut entries: Vec<RecentEntry> = serde_json::from_slice(&bytes)
            .map_err(|e| ConfigError::Parse(format!("recent.json: {:?}", e.classify())))?;
        // gleiche Kennung bzw. gleicher Pfad nur einmal (die neueste Fassung zählt), höchstens zehn
        let mut seen_ids = std::collections::HashSet::new();
        let mut seen_paths = std::collections::HashSet::new();
        entries.retain(|e| seen_ids.insert(e.id) && seen_paths.insert(e.path.clone()));
        entries.truncate(MAX_RECENT);
        Ok(entries)
    }

    fn write(&self, entries: &[RecentEntry]) -> Result<()> {
        let json = serde_json::to_vec_pretty(entries).map_err(|e| ConfigError::Serialize(e.to_string()))?;
        crate::settings::write_atomic(&self.file, &json)
    }

    /// Neuer Eintrag ganz vorne: Kennung = höchste bisherige + 1 (`entry.id` wird ersetzt); ein Eintrag mit
    /// demselben Pfad ersetzt den alten; danach höchstens [`MAX_RECENT`]. Liefert den gespeicherten Eintrag.
    pub fn push(&self, entry: RecentEntry) -> Result<RecentEntry> {
        let mut entries = self.load();
        // die Kennungen stammen aus der Datei: bei der größtmöglichen neu durchzählen statt überzulaufen
        let id = match entries.iter().map(|e| e.id).max().unwrap_or(0).checked_add(1) {
            Some(id) => id,
            None => {
                let n = entries.len() as u64;
                for (i, e) in entries.iter_mut().enumerate() {
                    e.id = n - i as u64;
                }
                n + 1
            }
        };
        let entry = RecentEntry { id, ..entry };
        entries.retain(|e| e.path != entry.path);
        entries.insert(0, entry.clone());
        entries.truncate(MAX_RECENT);
        self.write(&entries)?;
        Ok(entry)
    }

    /// Pfade umstellen (Mobile: neuer Sandbox-Container nach einem Update). `relocate` liefert je Pfad den
    /// neuen Ort oder `None` (unverändert). Ergebnis: Anzahl umgestellter Einträge; ohne Änderung wird
    /// nichts geschrieben. Derselbe Pfad danach nur einmal (der neuere Eintrag zählt).
    pub fn relocate(&self, relocate: impl Fn(&Path) -> Option<PathBuf>) -> Result<usize> {
        let mut entries = self.load();
        let mut moved = 0;
        for entry in &mut entries {
            if let Some(new) = relocate(&entry.path) {
                entry.path = new;
                moved += 1;
            }
        }
        if moved > 0 {
            let mut seen = std::collections::HashSet::new();
            entries.retain(|e| seen.insert(e.path.clone()));
            self.write(&entries)?;
        }
        Ok(moved)
    }

    /// Eintrag entfernen (unbekannte Kennung: nichts zu tun).
    pub fn remove(&self, id: u64) -> Result<()> {
        let mut entries = self.load();
        let before = entries.len();
        entries.retain(|e| e.id != id);
        if entries.len() == before {
            return Ok(());
        }
        if entries.is_empty() {
            return self.clear();
        }
        self.write(&entries)
    }

    /// Liste leeren: löscht die Datei.
    pub fn clear(&self) -> Result<()> {
        match pida_fs::remove_file(&self.file) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(ConfigError::Io(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pida_fs::PathExt as _;

    fn entry(name: &str, dir: &Path) -> RecentEntry {
        RecentEntry {
            id: 0,
            file_name: name.into(),
            path: dir.join(name),
            signed_at: "2026-10-16T10:42:17+02:00".into(),
        }
    }

    fn store() -> (tempfile::TempDir, RecentStore) {
        let tmp = tempfile::tempdir().unwrap();
        let s = RecentStore::new(&Paths::under(tmp.path()));
        (tmp, s)
    }

    #[test]
    fn only_name_path_and_time_are_stored() {
        let (tmp, s) = store();
        assert!(s.load().is_empty(), "keine Datei = leer");
        let e = s.push(entry("Vertrag_signed.pdf", tmp.path())).unwrap();
        assert_eq!(e.id, 1);
        let text = pida_fs::read_to_string(s.file()).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        let list = json.as_array().expect("Liste");
        assert_eq!(list.len(), 1);
        let keys: Vec<&str> = list[0].as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, ["file_name", "id", "path", "signed_at"], "feldgenau, kein Signiermittel/Unterzeichner");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = pida_fs::metadata(s.file()).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "nur für die Benutzerin lesbar");
        }
    }

    /// Kennungen aus der Datei am oberen Rand: neu durchzählen statt überlaufen (Argus R-02).
    #[test]
    fn ids_at_the_limit_are_renumbered() {
        let (tmp, s) = store();
        let a = RecentEntry { id: u64::MAX, ..entry("a.pdf", tmp.path()) };
        let b = RecentEntry { id: 7, ..entry("b.pdf", tmp.path()) };
        s.write(&[a, b]).unwrap();
        let c = s.push(entry("c.pdf", tmp.path())).unwrap();
        assert_eq!(c.id, 3);
        let list = s.load();
        let got: Vec<(&str, u64)> = list.iter().map(|e| (e.file_name.as_str(), e.id)).collect();
        assert_eq!(got, [("c.pdf", 3), ("a.pdf", 2), ("b.pdf", 1)], "Reihenfolge bleibt, Kennungen eindeutig");
    }

    #[test]
    fn newest_first_at_most_ten_same_path_replaces_ids_monotonic() {
        let (tmp, s) = store();
        for i in 0..12 {
            s.push(entry(&format!("d{i}.pdf"), tmp.path())).unwrap();
        }
        let list = s.load();
        assert_eq!(list.len(), MAX_RECENT);
        assert_eq!(list[0].file_name, "d11.pdf");
        assert_eq!(list[0].id, 12);
        assert_eq!(list[9].file_name, "d2.pdf");
        // erneut signiert: alter Eintrag weg, neuer vorne mit neuer Kennung
        let again = s.push(entry("d5.pdf", tmp.path())).unwrap();
        assert_eq!(again.id, 13);
        let list = s.load();
        assert_eq!(list.len(), MAX_RECENT);
        assert_eq!(list[0].file_name, "d5.pdf");
        assert_eq!(list.iter().filter(|e| e.file_name == "d5.pdf").count(), 1);
        // entfernen
        s.remove(13).unwrap();
        assert!(s.load().iter().all(|e| e.id != 13));
        s.remove(999).unwrap();
        assert_eq!(s.load().len(), MAX_RECENT - 1);
    }

    #[test]
    fn clear_deletes_the_file_and_numbering_restarts() {
        let (tmp, s) = store();
        s.push(entry("a.pdf", tmp.path())).unwrap();
        s.push(entry("b.pdf", tmp.path())).unwrap();
        s.clear().unwrap();
        assert!(!s.file().fs_exists(), "Liste leeren löscht die Datei");
        s.clear().unwrap();
        assert_eq!(s.push(entry("c.pdf", tmp.path())).unwrap().id, 1);
        // letzter Eintrag entfernt: Datei weg
        s.remove(1).unwrap();
        assert!(!s.file().fs_exists());
    }

    #[test]
    fn unreadable_file_is_empty_and_gets_replaced() {
        let (tmp, s) = store();
        pida_fs::create_dir_all(s.file().parent().unwrap()).unwrap();
        pida_fs::write(s.file(), b"{kein json").unwrap();
        assert!(s.load().is_empty());
        pida_fs::write(s.file(), br#"[{"id":1,"file_name":"a","path":"/a","signed_at":"x","signer":"y"}]"#).unwrap();
        assert!(s.load().is_empty(), "unbekannte Felder (z. B. Unterzeichner) werden nicht übernommen");
        assert_eq!(s.push(entry("a.pdf", tmp.path())).unwrap().id, 1);
        assert_eq!(s.load().len(), 1);
    }

    #[test]
    fn relocate_rewrites_only_returned_paths_and_writes_only_on_change() {
        let (tmp, s) = store();
        let old = tmp.path().join("ALT/Documents");
        s.push(entry("a.pdf", &old)).unwrap();
        s.push(entry("b.pdf", tmp.path())).unwrap();
        let before = pida_fs::read(s.file()).unwrap();
        assert_eq!(s.relocate(|_| None).unwrap(), 0);
        assert_eq!(pida_fs::read(s.file()).unwrap(), before, "ohne Änderung nicht geschrieben");
        let new = tmp.path().join("NEU/Documents");
        let moved = s.relocate(|p| p.starts_with(&old).then(|| new.join(p.file_name().unwrap()))).unwrap();
        assert_eq!(moved, 1);
        let list = s.load();
        assert_eq!(list.len(), 2);
        assert_eq!(list[1].path, new.join("a.pdf"));
        assert_eq!(list[1].id, 1, "Kennung bleibt");
        assert_eq!(list[0].path, tmp.path().join("b.pdf"));
        // fällt ein Pfad auf einen vorhandenen, bleibt nur der neuere Eintrag
        s.relocate(|_| Some(tmp.path().join("x.pdf"))).unwrap();
        assert_eq!(s.load().len(), 1);
        assert_eq!(s.load()[0].file_name, "b.pdf");
    }

    #[test]
    fn debug_redacts_names_and_paths() {
        let e = entry("Geheim.pdf", Path::new("/Users/demo"));
        let text = format!("{e:?}");
        assert!(!text.contains("Geheim") && !text.contains("demo"), "{text}");
    }
}
