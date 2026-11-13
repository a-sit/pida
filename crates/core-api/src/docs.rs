//! Geöffnete Dokumente: Laden, Analyse, Platzhaltersuche, Renderer je Dokument und LRU-Cache der
//! Seitenkacheln (Plan §6.1, §6.6, §6.8).

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use pida_pades::{AcceptanceClass, DocReport, PadesError};
use pida_pdf_cos::{CosError, Limits, PageInfo, SourcePdf};
use pida_placement::{
    PlaceholderMode, PlaceholderQuery, find_empty_sig_fields, find_qr_placeholders, select_placeholder,
};
use pida_render::{RenderBackend, RenderError, Renderer, RgbaImage};
use pida_workflow::{DocId, PlaceholderFound, WorkflowError};

use crate::engine::PadesEngine;

/// Geöffnetes Dokument. `Debug` ohne Inhalt (Regel 9).
pub(crate) struct DocData {
    pub id: DocId,
    pub name: String,
    pub src: Arc<SourcePdf>,
    pub report: DocReport,
    pub pages: Vec<PageInfo>,
    renderer: Mutex<Option<Box<dyn Renderer>>>,
}

impl std::fmt::Debug for DocData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocData")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("pages", &self.pages.len())
            .field("class", &self.report.class)
            .finish_non_exhaustive()
    }
}

impl DocData {
    pub(crate) fn new(id: DocId, path: PathBuf, src: Arc<SourcePdf>, report: DocReport) -> DocData {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let pages = src.pages().to_vec();
        DocData { id, name, src, report, pages, renderer: Mutex::new(None) }
    }

    /// Führt `f` mit dem (bei Bedarf geöffneten) Renderer aus.
    pub(crate) fn with_renderer<T>(
        &self,
        backend: &dyn RenderBackend,
        f: impl FnOnce(&dyn Renderer) -> T,
    ) -> Result<T, RenderError> {
        let mut guard = self.renderer.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if guard.is_none() {
            *guard = Some(backend.open(self.src.shared_bytes(), None)?);
        }
        let r = guard.as_deref().expect("eben geöffnet");
        Ok(f(r))
    }

    /// Sichtbare Seitengrößen in pt.
    pub(crate) fn page_sizes(&self) -> Vec<(f32, f32)> {
        self.pages.iter().map(PageInfo::visible_size).collect()
    }
}

/// Größte Datei, die PIDA zur Verarbeitung annimmt: Grenze der Dokumentensignatur (20 MB). Größere Dokumente
/// werden schon beim Laden abgelehnt (`ng-document-too-large`), nicht erst beim Signiermittel.
pub const MAX_DOCUMENT_BYTES: u64 = 20_000_000;

/// Fehlercode für ein nicht ladbares Dokument (PDF-Over F-15: `error.MayNotBeAPDF`,
/// `error.PDFPwdProtected`, `error.IOError`) mit Klasse (K5 = kein „Wiederholen“).
pub(crate) fn load_error(e: &CosError) -> WorkflowError {
    let (class, code) = match e {
        CosError::PasswordRequired => (AcceptanceClass::K5Rejected, "error-PDFPwdProtected"),
        CosError::UnsupportedSecurityHandler(_) | CosError::Decryption(_) => {
            (AcceptanceClass::K5Rejected, "error-PDFProtected")
        }
        CosError::Io(_) => (AcceptanceClass::K1Regular, "error-IOError"),
        _ => (AcceptanceClass::K1Regular, "error-MayNotBeAPDF"),
    };
    WorkflowError::Rejected { class, code: code.into() }
}

/// Fehler der Analyse.
pub(crate) fn analysis_error(e: &PadesError) -> WorkflowError {
    match e {
        PadesError::Cos(c) => load_error(c),
        PadesError::Io(_) => {
            WorkflowError::Rejected { class: AcceptanceClass::K1Regular, code: "error-IOError".into() }
        }
        other => {
            tracing::warn!(error = %other, "Analyse abgelehnt");
            WorkflowError::Rejected { class: AcceptanceClass::K5Rejected, code: "ng-document-rejected".into() }
        }
    }
}

/// Ergebnis des Ladens.
#[derive(Debug)]
pub(crate) struct Loaded {
    pub data: Arc<DocData>,
    pub placeholder: Option<PlaceholderFound>,
    /// Warnungen der Analyse (Hybrid-XFA, `/Perms /UR3` …) als Codes
    pub warnings: Vec<String>,
    /// nach dem QR-Platzhalter gesucht (Platzhaltersuche mit `kind = marker`; Einstellungen › Dokument)
    pub qr_searched: bool,
}

/// Platzhaltersuche (F-20/F-21): `fields` = `USE_FIELDS` (Vorrang), sonst QR-Marker (id 1,
/// SORTED); Fehler der Suche werden still ignoriert (F-22).
pub(crate) fn find_placeholder(
    data: &DocData,
    fields: bool,
    renderer: Option<&dyn RenderBackend>,
) -> Option<PlaceholderFound> {
    if fields {
        let found = find_empty_sig_fields(&data.report.fields);
        return (!found.is_empty()).then_some(PlaceholderFound::Fields(found));
    }
    let backend = renderer?;
    let mut invocations = Vec::new();
    for i in 0..data.pages.len() {
        match data.src.image_invocations(i, false) {
            Ok(mut v) => invocations.append(&mut v),
            Err(e) => tracing::debug!(error = %e, "Bildaufrufe nicht lesbar – Seite übersprungen"),
        }
    }
    if invocations.is_empty() {
        return None;
    }
    let candidates = match data.with_renderer(backend, |r| find_qr_placeholders(r, &data.pages, &invocations)) {
        Ok(Ok(c)) => c,
        Ok(Err(e)) => {
            tracing::debug!(code = e.code(), "Platzhaltersuche fehlgeschlagen (ignoriert, F-22)");
            return None;
        }
        Err(e) => {
            tracing::debug!(code = e.code(), "Renderer nicht verfügbar (Platzhaltersuche übersprungen)");
            return None;
        }
    };
    let query = PlaceholderQuery {
        id: Some("1".into()),
        mode: PlaceholderMode::Sorted,
        used_names: data.report.used_placeholder_names(),
    };
    match select_placeholder(&candidates, &query) {
        Ok(Some(p)) => Some(PlaceholderFound::Qr(p.clone())),
        _ => None,
    }
}

/// Lädt und analysiert ein Dokument (läuft im Blocking-Pool).
pub(crate) fn load(
    id: DocId,
    path: &Path,
    engine: &dyn PadesEngine,
    placeholder_search: bool,
    fields: bool,
    renderer: Option<&dyn RenderBackend>,
) -> Result<Loaded, WorkflowError> {
    if pida_fs::metadata(path).is_ok_and(|m| m.len() > MAX_DOCUMENT_BYTES) {
        return Err(WorkflowError::Rejected {
            class: AcceptanceClass::K5Rejected,
            code: "ng-document-too-large".into(),
        });
    }
    let src = SourcePdf::open_file(path, &Limits::default()).map_err(|e| load_error(&e))?;
    let report = engine.analyze(&src).map_err(|e| analysis_error(&e))?;
    if report.class == AcceptanceClass::K5Rejected || report.class == AcceptanceClass::K2Repairable {
        let code = report
            .findings
            .iter()
            .find(|f| !f.warning && f.class == report.class)
            .map(|f| f.code.clone())
            .unwrap_or_else(|| "ng-document-rejected".into());
        return Err(WorkflowError::Rejected { class: report.class, code });
    }
    if !report.may_sign {
        return Err(WorkflowError::Rejected {
            class: AcceptanceClass::K5Rejected,
            code: "ng-pdf-no-sign-rights".into(),
        });
    }
    let warnings = report.findings.iter().filter(|f| f.warning).map(|f| f.code.clone()).collect();
    let data = Arc::new(DocData::new(id, path.to_path_buf(), Arc::new(src), report));
    let placeholder = if placeholder_search { find_placeholder(&data, fields, renderer) } else { None };
    let qr_searched = placeholder_search && !fields;
    Ok(Loaded { data, placeholder, warnings, qr_searched })
}

/// Schlüssel einer Kachel.
type TileKey = (DocId, usize, u32);

/// LRU-Cache der Seitenkacheln (PNG) mit Byte-Budget (Plan §6.6: mobil ≈ 64 MB).
#[derive(Debug)]
pub(crate) struct TileCache {
    budget: usize,
    used: usize,
    order: VecDeque<TileKey>,
    tiles: HashMap<TileKey, Arc<[u8]>>,
}

impl TileCache {
    pub(crate) fn new(budget: usize) -> TileCache {
        TileCache { budget, used: 0, order: VecDeque::new(), tiles: HashMap::new() }
    }

    pub(crate) fn get(&mut self, key: &TileKey) -> Option<Arc<[u8]>> {
        let v = self.tiles.get(key)?.clone();
        if let Some(pos) = self.order.iter().position(|k| k == key) {
            self.order.remove(pos);
        }
        self.order.push_back(*key);
        Some(v)
    }

    pub(crate) fn put(&mut self, key: TileKey, png: Arc<[u8]>) {
        if png.len() > self.budget {
            return;
        }
        if let Some(old) = self.tiles.insert(key, png.clone()) {
            self.used -= old.len();
            self.order.retain(|k| *k != key);
        }
        self.used += png.len();
        self.order.push_back(key);
        while self.used > self.budget {
            let Some(k) = self.order.pop_front() else { break };
            if let Some(v) = self.tiles.remove(&k) {
                self.used -= v.len();
            }
        }
    }

    pub(crate) fn purge(&mut self, doc: DocId) {
        self.order.retain(|k| k.0 != doc);
        let keys: Vec<TileKey> = self.tiles.keys().filter(|k| k.0 == doc).copied().collect();
        for k in keys {
            if let Some(v) = self.tiles.remove(&k) {
                self.used -= v.len();
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.tiles.len()
    }
}

/// Gemeinsamer Dokumentspeicher (Kern-Task und Shell-Threads).
#[derive(Debug)]
pub(crate) struct DocStore {
    docs: Mutex<HashMap<DocId, Arc<DocData>>>,
    tiles: Mutex<TileCache>,
}

/// Grenze der Kachelskalierung (Schutz gegen Riesen-Bitmaps; PDFium prüft zusätzlich `MAX_PIXELS`).
const MAX_SCALE: f32 = 8.0;

impl DocStore {
    pub(crate) fn new(tile_budget: usize) -> DocStore {
        DocStore { docs: Mutex::new(HashMap::new()), tiles: Mutex::new(TileCache::new(tile_budget)) }
    }

    pub(crate) fn insert(&self, data: Arc<DocData>) {
        self.docs.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(data.id, data);
    }

    pub(crate) fn get(&self, id: DocId) -> Option<Arc<DocData>> {
        self.docs.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(&id).cloned()
    }

    /// Anzahl geöffneter Dokumente (Diagnose, Tests).
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn len(&self) -> usize {
        self.docs.lock().unwrap_or_else(std::sync::PoisonError::into_inner).len()
    }

    pub(crate) fn remove(&self, id: DocId) {
        self.docs.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&id);
        self.tiles.lock().unwrap_or_else(std::sync::PoisonError::into_inner).purge(id);
    }

    /// PNG einer Seite; `page == Seitenzahl` = weiße virtuelle neue Seite (F-38).
    pub(crate) fn tile(
        &self,
        id: DocId,
        page: usize,
        scale: f32,
        backend: &dyn RenderBackend,
    ) -> Result<Option<Arc<[u8]>>, RenderError> {
        let Some(doc) = self.get(id) else { return Ok(None) };
        let scale = if scale.is_finite() && scale > 0.0 { scale.min(MAX_SCALE) } else { 1.0 };
        let key = (id, page, (scale * 1000.0).round() as u32);
        if let Some(t) = self.tiles.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(&key) {
            return Ok(Some(t));
        }
        let image = if page == doc.pages.len() && page > 0 {
            let (w, h) = doc.pages[page - 1].visible_size();
            let (w, h) = ((w * scale).round().max(1.0) as u32, (h * scale).round().max(1.0) as u32);
            // dieselbe Grenze wie für gerenderte Seiten: eine riesige Seitengröße darf keinen Speicher erzwingen
            if u64::from(w) * u64::from(h) > pida_render::MAX_PIXELS {
                return Err(RenderError::Render(format!("Raster {w}×{h} zu groß")));
            }
            RgbaImage::filled(w, h, [255, 255, 255, 255])
        } else {
            doc.with_renderer(backend, |r| r.render_page(page, scale))??
        };
        let png: Arc<[u8]> = Arc::from(pida_render::encode_png(&image)?);
        self.tiles.lock().unwrap_or_else(std::sync::PoisonError::into_inner).put(key, png.clone());
        Ok(Some(png))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lru_respects_budget() {
        let mut c = TileCache::new(10);
        c.put((DocId(1), 0, 1000), Arc::from(vec![0u8; 4]));
        c.put((DocId(1), 1, 1000), Arc::from(vec![0u8; 4]));
        assert!(c.get(&(DocId(1), 0, 1000)).is_some()); // 0 zuletzt benutzt
        c.put((DocId(1), 2, 1000), Arc::from(vec![0u8; 4]));
        assert!(c.get(&(DocId(1), 1, 1000)).is_none(), "ältester Eintrag verdrängt");
        assert!(c.get(&(DocId(1), 0, 1000)).is_some());
        c.put((DocId(2), 0, 1000), Arc::from(vec![0u8; 11]));
        assert!(c.get(&(DocId(2), 0, 1000)).is_none(), "größer als Budget");
        c.purge(DocId(1));
        assert_eq!(c.len(), 0);
    }

    #[test]
    fn load_errors_map_to_pdf_over_messages() {
        assert_eq!(load_error(&CosError::PasswordRequired).code(), "error-PDFPwdProtected");
        assert_eq!(load_error(&CosError::NotAPdf).code(), "error-MayNotBeAPDF");
        assert_eq!(load_error(&CosError::Io(std::io::Error::other("x"))).code(), "error-IOError");
        assert!(matches!(
            load_error(&CosError::UnsupportedSecurityHandler("PubSec".into())),
            WorkflowError::Rejected { class: AcceptanceClass::K5Rejected, .. }
        ));
    }

    #[test]
    fn not_a_pdf_fails_before_engine() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("x.pdf");
        pida_fs::write(&p, b"kein pdf").unwrap();
        let err = load(DocId(1), &p, &crate::engine::RealPades, false, false, None).unwrap_err();
        assert_eq!(err.code(), "error-MayNotBeAPDF");
        let err =
            load(DocId(1), &tmp.path().join("fehlt.pdf"), &crate::engine::RealPades, false, false, None).unwrap_err();
        assert_eq!(err.code(), "error-IOError");
    }

    #[test]
    fn documents_over_20_mb_are_rejected_before_reading() {
        let tmp = tempfile::tempdir().unwrap();
        let big = tmp.path().join("gross.pdf");
        // dünn besetzte Datei: nur die Größe zählt, gelesen wird sie nicht
        pida_fs::File::create(&big).unwrap().set_len(MAX_DOCUMENT_BYTES + 1).unwrap();
        let err = load(DocId(1), &big, &crate::engine::RealPades, false, false, None).unwrap_err();
        assert_eq!(err.code(), "ng-document-too-large");
        assert!(matches!(err, WorkflowError::Rejected { class: AcceptanceClass::K5Rejected, .. }));
        // genau an der Grenze: angenommen (hier dann „kein PDF“)
        let edge = tmp.path().join("grenze.pdf");
        pida_fs::File::create(&edge).unwrap().set_len(MAX_DOCUMENT_BYTES).unwrap();
        let err = load(DocId(1), &edge, &crate::engine::RealPades, false, false, None).unwrap_err();
        assert_eq!(err.code(), "error-MayNotBeAPDF");
    }
}
