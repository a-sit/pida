//! Stapel im Kern (ADR 0004 §3.8, Paket K2): Voranalyse mit Miniatur, Ausgabeziel je Dokument (F-81/F-82,
//! „Beide behalten“), Projektion von [`ViewState::batch`](crate::ViewState::batch) samt Schätzung
//! „n Dokumente · 1 Anmeldung · n Bestätigungen“ und der Fortschritt der Stapelkarte.
//!
//! Die Voranalyse lädt jedes Dokument einzeln im Blocking-Pool, analysiert es, sucht den QR-Platzhalter (nur
//! mit Platzhaltersuche, `kind = marker`), zählt leere Signaturfelder und rendert eine PNG-Miniatur (Breite
//! [`THUMB_WIDTH_PX`]) in einen eigenen kleinen Cache je Stapeleintrag (`pida-render://thumb/<doc>`, nicht
//! im LRU der Kacheln). Danach ist das Dokument freigegeben – es kommt nie in den `DocStore`, ein gerade
//! geöffnetes Dokument bleibt deshalb unberührt. Es läuft immer nur eine Voranalyse (niedrige Priorität
//! gegenüber Kacheln und Blockvorschau).

use pida_fs::PathExt as _;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use web_time::Instant;

use pida_backend_api::{BackendId, NativeForm, SecretKind};
use pida_render::RenderBackend;
use pida_workflow::{
    BatchItem, BatchItemStatus, BatchOutput, BatchPause, BatchPhase, BatchRun, DocId, DocSummary, ExistingPolicy,
    FsProbe, OutputDecision, PlaceholderFound, PositionChoice, Queue, State, WorkflowError, decide_output,
    output_file_name,
};

use crate::engine::PadesEngine;
use crate::view::{
    BatchEstimateView, BatchItemStatusView, BatchItemView, BatchOutputView, BatchPauseView, BatchSummaryView,
    BatchView, CertificateSource, ConfirmMethod, DocRefView, InteractionView, OutputModeView, PauseScope,
    PositionSummaryView, SigningProgressView, SigningStage,
};

/// Breite der Miniatur in px.
pub(crate) const THUMB_WIDTH_PX: f32 = 96.0;
/// Ab so vielen Dokumenten mit ID Austria erscheint der Hinweis `ng-batch-large` (§8 F9).
pub(crate) const LARGE_BATCH: u32 = 20;
/// Gültigkeit gecachter Dateisystem- und Speicherabfragen der Projektion (`target_exists`, Schätzung).
const PROBE_TTL: Duration = Duration::from_secs(2);

// ------------------------------------------------------------------------------------------------
// Voranalyse
// ------------------------------------------------------------------------------------------------

/// Warteschlange der Voranalyse: nacheinander, höchstens eine zur Zeit.
#[derive(Debug, Default)]
pub(crate) struct Preanalysis {
    waiting: VecDeque<DocId>,
    running: Option<DocId>,
}

impl Preanalysis {
    /// Dokument vormerken (doppelte Wünsche zählen einmal).
    pub(crate) fn push(&mut self, doc: DocId) {
        if self.running != Some(doc) && !self.waiting.contains(&doc) {
            self.waiting.push_back(doc);
        }
    }

    /// Nächstes Dokument, falls keines läuft; `wanted` filtert verworfene Wünsche (Eintrag entfernt).
    pub(crate) fn next(&mut self, wanted: impl Fn(DocId) -> bool) -> Option<DocId> {
        if self.running.is_some() {
            return None;
        }
        while let Some(doc) = self.waiting.pop_front() {
            if wanted(doc) {
                self.running = Some(doc);
                return Some(doc);
            }
        }
        None
    }

    /// Voranalyse von `doc` fertig.
    pub(crate) fn finished(&mut self, doc: DocId) {
        if self.running == Some(doc) {
            self.running = None;
        }
    }
}

/// Lädt, analysiert und rendert die Miniatur eines Dokuments (Blocking-Pool); das Dokument wird danach
/// freigegeben.
pub(crate) fn preanalyze(
    id: DocId,
    path: &Path,
    engine: &dyn PadesEngine,
    search_qr: bool,
    render: Option<&dyn RenderBackend>,
    mark_auto_fields: &dyn Fn(&crate::docs::DocData, &mut [pida_placement::EmptyFieldCandidate]),
) -> (Result<DocSummary, WorkflowError>, Option<Arc<[u8]>>) {
    let loaded = match crate::docs::load(id, path, engine, search_qr, false, render) {
        Ok(l) => l,
        Err(e) => return (Err(e), None),
    };
    let data = &loaded.data;
    let placeholder = match &loaded.placeholder {
        Some(PlaceholderFound::Qr(qr)) => Some(qr.clone()),
        _ => None,
    };
    let summary = DocSummary {
        page_count: data.pages.len(),
        size_bytes: data.src.len(),
        signed: data.report.is_signed(),
        class: data.report.class,
        qr: placeholder.is_some(),
        fields: pida_placement::find_empty_sig_fields(&data.report.fields).len(),
        warnings: loaded.warnings.clone(),
        placeholder,
        new_visible_field_allowed: data.report.allows_new_visible_field(),
        // für die Automatik nur ein geeignetes Feld (Einrastregel, ADR 0004 Nachtrag 22)
        first_field: {
            let mut fields = pida_placement::find_empty_sig_fields(&data.report.fields);
            mark_auto_fields(data, &mut fields);
            fields.into_iter().find(|f| f.auto_ok)
        },
        qr_searched: loaded.qr_searched,
    };
    let thumb = render.and_then(|backend| {
        let png = data.with_renderer(backend, |r| -> pida_render::Result<Vec<u8>> {
            let size = r.page_size(0)?;
            let scale = if size.width > 0.0 { (THUMB_WIDTH_PX / size.width).clamp(0.02, 2.0) } else { 0.15 };
            pida_render::encode_png(&r.render_page(0, scale)?)
        });
        match png {
            Ok(Ok(png)) => Some(Arc::from(png)),
            Ok(Err(e)) | Err(e) => {
                tracing::debug!(code = e.code(), "Miniatur nicht erstellt");
                None
            }
        }
    });
    (Ok(summary), thumb)
}

// ------------------------------------------------------------------------------------------------
// Ausgabeziel
// ------------------------------------------------------------------------------------------------

/// Erste freie Nummer ab 2 (`Name_signed (2).pdf`, `(3)` …) neben `path`.
pub(crate) fn keep_both(path: &Path) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let (base, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], Some(&name[i + 1..])),
        _ => (name.as_str(), None),
    };
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    for n in 2u32.. {
        let candidate = match ext {
            Some(ext) => format!("{base} ({n}).{ext}"),
            None => format!("{base} ({n})"),
        };
        let p = dir.join(candidate);
        if !p.fs_exists() {
            return p;
        }
    }
    unreachable!("u32-Bereich erschöpft")
}

/// Ziel eines Stapeldokuments nach F-81/F-82 im Ordner `folder` (ohne Ordner: neben der Quelle) und der
/// Regel für vorhandene Dateien: (Pfad, Ziel nach F-81 existierte). „Beide behalten“ liefert bei
/// vorhandenem Ziel die erste freie Nummer.
pub(crate) fn batch_target(
    input: &Path,
    folder: Option<&Path>,
    postfix: &str,
    on_existing: ExistingPolicy,
) -> (PathBuf, bool) {
    let path = target_path(input, folder, postfix);
    let exists = path.fs_exists();
    match on_existing {
        ExistingPolicy::KeepBoth if exists => (keep_both(&path), true),
        _ => (path, exists),
    }
}

/// Ist `path` das Ergebnis eines Eintrags dieses Stapels (im Lauf bzw. einzeln aus der Übersicht signiert)?
fn written_by_batch(queue: &Queue, path: &Path) -> bool {
    queue
        .batch
        .iter()
        .flat_map(|b| b.items.iter())
        .any(|i| matches!(&i.status, BatchItemStatus::Signed { output } if output.path == path))
}

/// Ziel nach F-81/F-82 im Ordner `folder` (ohne Ordner bzw. Speichern-Dialog: neben der Quelle).
fn target_path(input: &Path, folder: Option<&Path>, postfix: &str) -> PathBuf {
    match decide_output(input, folder, postfix, &FsProbe) {
        OutputDecision::Save { path, .. } => path,
        OutputDecision::Ask { directory, file_name } => directory.join(file_name),
    }
}

/// Gültigkeit der Abfrage „gemerktes Passwort“ (Schätzung): sie liest den Schlüsselbund – das darf nicht bei
/// jeder Projektion geschehen (Rückfrage des Systems je Zugriff, gesperrter Schlüsselbund unter Linux). Neu
/// gefragt wird nach einer Änderung der Einstellungen und nach jedem Lauf ([`ProbeCache::clear`]).
pub(crate) const SECRET_PROBE_TTL: Duration = Duration::from_secs(600);

/// Kleiner Zeit-Cache für Abfragen der Projektion (Dateisystem, Schlüsselbund, Zertifikats-Cache).
#[derive(Debug, Default)]
pub(crate) struct ProbeCache {
    /// Wert und Ablauf
    entries: Mutex<HashMap<String, (bool, Instant)>>,
}

impl ProbeCache {
    /// Gecachte Abfrage (kurze Gültigkeit, [`PROBE_TTL`]).
    pub(crate) fn get(&self, key: String, probe: impl FnOnce() -> bool) -> bool {
        self.get_for(key, PROBE_TTL, probe)
    }

    /// Gecachte Abfrage mit eigener Gültigkeit.
    pub(crate) fn get_for(&self, key: String, ttl: Duration, probe: impl FnOnce() -> bool) -> bool {
        let mut entries = self.entries.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let now = Instant::now();
        if let Some((value, until)) = entries.get(&key)
            && now < *until
        {
            return *value;
        }
        let value = probe();
        if entries.len() > 512 {
            entries.retain(|_, (_, until)| now < *until);
        }
        entries.insert(key, (value, now + ttl));
        value
    }

    /// Alles neu abfragen (Einstellungen geändert, Lauf beendet).
    pub(crate) fn clear(&self) {
        self.entries.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clear();
    }
}

// ------------------------------------------------------------------------------------------------
// Projektion
// ------------------------------------------------------------------------------------------------

/// Phase der Karte nach der Phase des Laufs (ohne offene Maske).
fn stage_of(phase: BatchPhase) -> SigningStage {
    match phase {
        BatchPhase::OpeningSession => SigningStage::Connecting,
        BatchPhase::ReadingCertificate => SigningStage::Certificate,
        BatchPhase::CheckingTarget | BatchPhase::Loading | BatchPhase::Preparing => SigningStage::Preparing,
        BatchPhase::Confirming => SigningStage::Confirming,
        BatchPhase::Finishing => SigningStage::Finishing,
        BatchPhase::Writing => SigningStage::Saving,
    }
}

impl crate::runtime::CoreTask {
    /// Ordner, in den der Stapel schreibt: gewählter Ordner bzw. nach den Einstellungen (Overlay vor
    /// Einstellung; Mobile: die App-Dokumente, beim Schreiben angelegt – `create`).
    pub(crate) fn batch_folder(&self, output: &BatchOutput, create: bool) -> Option<PathBuf> {
        match output {
            BatchOutput::Folder(dir) => Some(dir.clone()),
            BatchOutput::Configured => self.cfg.output().or_else(|| {
                let dir = self.mobile.default_output_dir.clone()?;
                if create && let Err(e) = pida_fs::create_dir_all(&dir) {
                    tracing::warn!(kind = ?e.kind(), "App-Dokumente nicht anlegbar");
                }
                Some(dir)
            }),
        }
    }

    /// `Effect::DecideBatchOutput`: Ziel des Dokuments bestimmen.
    pub(crate) fn effect_decide_batch_output(&mut self, doc: DocId, output: &BatchOutput, on_existing: ExistingPolicy) {
        let Some(queue) = self.state.queue() else { return };
        let Some(input) = queue.find(doc).map(|d| d.path.clone()) else { return };
        let folder = self.batch_folder(output, true);
        let postfix = &self.cfg.settings.output.postfix;
        // „Überschreiben“ gilt für Dateien, die es vor dem Stapel gab – nie für ein Ergebnis dieses Stapels
        // (gleicher Dateiname aus verschiedenen Ordnern): dann „Beide behalten“, das Ziel ist frei
        let (path, exists) = if on_existing == ExistingPolicy::Overwrite
            && written_by_batch(queue, &target_path(&input, folder.as_deref(), postfix))
        {
            (batch_target(&input, folder.as_deref(), postfix, ExistingPolicy::KeepBoth).0, false)
        } else {
            batch_target(&input, folder.as_deref(), postfix, on_existing)
        };
        self.apply_later(pida_workflow::Event::BatchOutputDecided { doc, path, exists });
    }

    /// Ausgabe des Stapels in der Übersicht: Art, Anzeige des Ordners, Ordnerwahl vor dem Start nötig
    /// („jedes Mal fragen“ bzw. kein vorhandener Ordner – sonst wäre er ein Dateipfad, F-82).
    pub(crate) fn batch_output_view(&self, queue: &Queue) -> BatchOutputView {
        if !self.mobile.platform.is_desktop() {
            return BatchOutputView { mode: OutputModeView::AppDocuments, folder_display: None, needs_folder: false };
        }
        let Some(configured) = self.cfg.output().filter(|p| !p.as_os_str().to_string_lossy().trim().is_empty()) else {
            return BatchOutputView { mode: OutputModeView::Ask, folder_display: None, needs_folder: true };
        };
        let sources: Vec<PathBuf> = queue
            .batch
            .iter()
            .flat_map(|b| b.items.iter())
            .filter(|i| !matches!(i.status, BatchItemStatus::Signed { .. } | BatchItemStatus::Rejected { .. }))
            .filter_map(|i| queue.find(i.doc))
            .map(|d| d.path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(".")))
            .collect();
        let resolved: Vec<PathBuf> = if configured.is_relative() {
            sources.iter().map(|s| s.join(&configured)).collect()
        } else {
            vec![configured.clone()]
        };
        let needs_folder = resolved.iter().any(|r| !self.probe.get(format!("dir:{}", r.display()), || r.fs_is_dir()));
        let next_to_source =
            configured.is_relative() && configured.components().all(|c| c == std::path::Component::CurDir);
        let mode = if next_to_source { OutputModeView::NextToSource } else { OutputModeView::Folder };
        let first = resolved.first().cloned();
        let folder_display =
            first.filter(|f| resolved.iter().all(|r| r == f)).map(|f| strip_cur_dir(&f).display().to_string());
        BatchOutputView { mode, folder_display, needs_folder }
    }

    /// `ViewState::batch`: in der Übersicht, im Lauf und in einem aus der Übersicht geöffneten Dokument.
    pub(crate) fn batch_view(&self) -> Option<BatchView> {
        if !self.ctx.batch() {
            return None;
        }
        let running = matches!(self.state, State::BatchSigning { .. });
        let shown = running || matches!(self.state, State::BatchOverview { .. }) || self.state.opened_from_batch();
        if !shown {
            return None;
        }
        let queue = self.state.queue()?;
        let batch = queue.batch.as_ref()?;
        let run = match &self.state {
            State::BatchSigning { run, .. } => Some(run.as_ref()),
            _ => None,
        };
        let output = self.batch_output_view(queue);
        let postfix = &self.cfg.settings.output.postfix;
        let thumbs = crate::runtime::lock(&self.shared.thumbs);
        let auto_position = batch.auto_position(&self.ctx);
        let items = batch
            .items
            .iter()
            .map(|item| {
                let input = queue.find(item.doc);
                let target_name = input.map(|d| output_file_name(&d.path, postfix));
                let target_exists = match (input, output.needs_folder) {
                    (Some(d), false) if item.status.signable() => {
                        let folder = self.batch_folder(&BatchOutput::Configured, false);
                        let path = target_path(&d.path, folder.as_deref(), postfix);
                        self.probe.get(format!("file:{}", path.display()), || path.fs_exists())
                    }
                    _ => false,
                };
                BatchItemView {
                    doc: item.doc,
                    name: input.map(|d| d.display_name.clone()).unwrap_or_default(),
                    page_count: item.summary.as_ref().map(|s| s.page_count),
                    size_bytes: item.summary.as_ref().map(|s| s.size_bytes),
                    status: self.item_status_view(item, run, auto_position),
                    position: self.position_summary(item),
                    target_name,
                    target_exists,
                    thumbnail: thumbs.contains_key(&item.doc),
                    signatures: Some(self.item_signatures(item)),
                }
            })
            .collect();
        drop(thumbs);
        let summary = batch.last_run.as_ref().map(|last| {
            let count = |f: fn(&BatchItemStatus) -> bool| {
                u32::try_from(batch.items.iter().filter(|i| f(&i.status)).count()).unwrap_or(u32::MAX)
            };
            BatchSummaryView {
                total: u32::try_from(batch.items.len()).unwrap_or(u32::MAX),
                signed: count(|s| matches!(s, BatchItemStatus::Signed { .. })),
                rejected: count(|s| matches!(s, BatchItemStatus::Rejected { .. })),
                failed: count(|s| matches!(s, BatchItemStatus::Failed { .. })),
                skipped: count(|s| matches!(s, BatchItemStatus::Skipped { .. })),
                cancelled: count(|s| matches!(s, BatchItemStatus::Cancelled)),
                not_started: count(BatchItemStatus::not_started),
                aborted: last.aborted,
                folder_display: if self.mobile.platform.is_desktop() {
                    last.folder.as_ref().map(|f| f.display().to_string()).or_else(|| output.folder_display.clone())
                } else {
                    None
                },
            }
        });
        let estimate = if running { None } else { self.batch_estimate(batch) };
        Some(BatchView {
            items,
            open_item: batch.open,
            running,
            summary,
            output,
            estimate,
            auto_position_setting: self.ctx.auto_position,
            auto_position_once: batch.auto_position_once,
            manual_plans: u32::try_from(batch.manual_plans()).unwrap_or(u32::MAX),
        })
    }

    fn item_status_view(&self, item: &BatchItem, run: Option<&BatchRun>, auto_position: bool) -> BatchItemStatusView {
        // automatische Positionierung im Stapel: automatische Pläne gelten als „Bereit“ (Nachtrag 8)
        match &item.shown_status(auto_position) {
            BatchItemStatus::Analyzing => BatchItemStatusView::Analyzing,
            BatchItemStatus::Ready => BatchItemStatusView::Ready,
            BatchItemStatus::Positioned => BatchItemStatusView::Positioned,
            BatchItemStatus::Rejected { code } => BatchItemStatusView::Rejected { code: code.clone() },
            BatchItemStatus::NeedsReview { codes } => BatchItemStatusView::NeedsReview { codes: codes.clone() },
            BatchItemStatus::Queued => BatchItemStatusView::Queued,
            BatchItemStatus::Active => {
                let stage = run.map_or(SigningStage::Preparing, |r| self.batch_stage(r).0);
                BatchItemStatusView::Active { stage }
            }
            BatchItemStatus::Signed { output } => BatchItemStatusView::Signed { path: output.path.clone() },
            BatchItemStatus::Failed { error } => {
                BatchItemStatusView::Failed { code: error.code(), params: error.params() }
            }
            BatchItemStatus::Skipped { existing } => BatchItemStatusView::Skipped { existing: *existing },
            BatchItemStatus::Cancelled => BatchItemStatusView::Cancelled,
        }
    }

    /// Mehrfachsignatur: Signaturen dieses Eintrags im Lauf (unsichtbares Profil: immer eine).
    fn item_signatures(&self, item: &BatchItem) -> u32 {
        if self.ctx.invisible { 1 } else { u32::try_from(item.signatures()).unwrap_or(u32::MAX) }
    }

    fn position_summary(&self, item: &BatchItem) -> PositionSummaryView {
        if self.ctx.invisible {
            // unsichtbares Profil: der Lauf signiert jeden Eintrag unsichtbar (auch mit Plan)
            return PositionSummaryView::Invisible;
        }
        match &item.plan {
            Some(PositionChoice::Manual(p)) => PositionSummaryView::Manual { page: p.page_index },
            Some(PositionChoice::Field(f)) => PositionSummaryView::Field { name: f.qualified_name.clone() },
            Some(PositionChoice::Placeholder(_)) => PositionSummaryView::Qr,
            Some(PositionChoice::Invisible) => PositionSummaryView::Invisible,
            Some(PositionChoice::Auto) | None => PositionSummaryView::Auto,
        }
    }

    /// Schätzung für das gewählte Signiermittel (`None` ohne Auswahl).
    fn batch_estimate(&self, batch: &pida_workflow::Batch) -> Option<BatchEstimateView> {
        let backend = self.signers_view().selected?;
        let caps = self.ws.signer_options.iter().find(|o| o.id == backend).and_then(|o| o.capabilities.clone());
        let documents = u32::try_from(batch.signable()).unwrap_or(u32::MAX);
        // nur der Keystore liest dafür den Schlüsselbund (ID Austria: „Merken“-Speicher im Speicher)
        let ttl = if backend == BackendId::Keystore { SECRET_PROBE_TTL } else { PROBE_TTL };
        let logins = self
            .probe
            .get_for(format!("logins:{}", backend.as_str()), ttl, || self.registry.expected_logins(backend) > 0);
        // je Dokument eine Bestätigung; Mehrfachsignatur: je Block eine
        let signatures: u32 = batch.items.iter().filter(|i| i.status.signable()).map(|i| self.item_signatures(i)).sum();
        let confirmations = match &caps {
            Some(c) if c.max_batch == 1 => signatures,
            Some(_) => 0,
            None => signatures,
        };
        let visible = !self.ctx.invisible
            && batch
                .items
                .iter()
                .filter(|i| i.status.signable())
                .any(|i| !matches!(i.plan, Some(PositionChoice::Invisible)));
        let reads_with_confirmation = backend == BackendId::AtrustNative
            && caps.as_ref().is_some_and(|c| c.certificate != pida_backend_api::CertNeed::No);
        let certificate_confirmation_possible = visible
            && reads_with_confirmation
            && !self.probe.get(format!("certcache:{}", backend.as_str()), || self.certificate_cached(backend));
        Some(BatchEstimateView {
            backend,
            documents,
            logins: u32::from(logins),
            confirmations,
            certificate_confirmation_possible,
            large: backend == BackendId::AtrustNative && documents >= LARGE_BATCH,
        })
    }

    /// Phase und Bestätigungsweg der Stapelkarte.
    fn batch_stage(&self, run: &BatchRun) -> (SigningStage, Option<ConfirmMethod>) {
        if run.pause.is_some() {
            return (SigningStage::Paused, None);
        }
        let default_method = match run.backend {
            BackendId::Keystore => Some(ConfirmMethod::Password),
            BackendId::LocalBku => Some(ConfirmMethod::Card),
            _ => self.ws.last_method,
        };
        match self.shared.broker.current() {
            InteractionView::Form { form: NativeForm::Login { .. }, .. } => (SigningStage::Login, None),
            InteractionView::Form { form, .. } => (
                SigningStage::Confirming,
                crate::workspace::remembered_method(&InteractionView::Form { request: 0, form }),
            ),
            InteractionView::Secret { prompt, .. } if prompt.kind == SecretKind::Fido2Pin => {
                (SigningStage::Confirming, Some(ConfirmMethod::Fido2))
            }
            InteractionView::Secret { prompt, .. }
                if matches!(prompt.kind, SecretKind::KeystorePassword | SecretKind::KeyPassword) =>
            {
                (SigningStage::Confirming, Some(ConfirmMethod::Password))
            }
            _ => {
                let stage = stage_of(run.phase);
                (stage, if stage == SigningStage::Confirming { default_method } else { None })
            }
        }
    }

    /// Fortschritt der Stapelkarte (`StepView::Signing::progress`, `batch == true`).
    pub(crate) fn batch_progress_view(&self, queue: &Queue, run: &BatchRun) -> SigningProgressView {
        let (stage, method) = self.batch_stage(run);
        let current = run.current();
        let doc_ref =
            |id: DocId| DocRefView { id, name: queue.find(id).map(|d| d.display_name.clone()).unwrap_or_default() };
        let certificate = if !run.flow.needs_certificate {
            CertificateSource::NotNeeded
        } else {
            let slot = crate::runtime::lock(&self.shared.sign).certificate_source();
            match slot {
                CertificateSource::Cache => CertificateSource::Cache,
                _ if run.certificate_read && run.certificate_doc != current => CertificateSource::Session,
                other => other,
            }
        };
        let total = u32::try_from(run.order.len()).unwrap_or(u32::MAX);
        let index = u32::try_from(run.index + 1).unwrap_or(u32::MAX).min(total.max(1));
        // Bestätigungen: je Dokument eine, in der Mehrfachsignatur je Block eine
        let batch_items = queue.batch.as_ref();
        let confirmations_total: u32 =
            run.order.iter().map(|d| batch_items.and_then(|b| b.item(*d)).map_or(1, |i| self.item_signatures(i))).sum();
        let pause = run.pause.as_ref().map(|p| {
            let (scope, new_session) = match p {
                BatchPause::Document { .. } => (PauseScope::Document, false),
                BatchPause::Session { new_session, .. } => (PauseScope::Session, *new_session),
            };
            let error = p.error();
            let mut params: BTreeMap<String, String> = error.params();
            if let (WorkflowError::Save(_), Some(d)) = (error, p.doc().and_then(|d| queue.find(d))) {
                params.insert("arg1".into(), output_file_name(&d.path, &self.cfg.settings.output.postfix));
            }
            BatchPauseView {
                doc: p.doc().map(doc_ref),
                code: error.code(),
                params,
                scope,
                can_retry: true,
                new_session,
            }
        });
        SigningProgressView {
            backend: Some(run.backend),
            stage,
            method,
            doc: current.map(doc_ref),
            index,
            total,
            login_done: self.ws.login_done,
            confirmations_done: run.confirmations_done.min(confirmations_total),
            confirmations_total,
            certificate,
            batch: true,
            pause,
        }
    }

    /// Gültiges Zertifikat im Zertifikats-Cache (nur lesend, für die Schätzung)?
    fn certificate_cached(&self, backend: BackendId) -> bool {
        let Some((cache, key)) = self.certificate_cache_for(backend) else { return false };
        match cache.get(&key) {
            Ok(Some(der)) => {
                pida_cades::Certificate::from_der(&der).is_ok_and(|c| c.is_valid_at(time::OffsetDateTime::now_utc()))
            }
            _ => false,
        }
    }
}

/// `./x` → `x` (Anzeige).
fn strip_cur_dir(p: &Path) -> PathBuf {
    p.components().filter(|c| *c != std::path::Component::CurDir).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keep_both_numbers_from_two() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("Vertrag_signed.pdf");
        assert_eq!(keep_both(&base), tmp.path().join("Vertrag_signed (2).pdf"));
        pida_fs::write(tmp.path().join("Vertrag_signed (2).pdf"), b"x").unwrap();
        assert_eq!(keep_both(&base), tmp.path().join("Vertrag_signed (3).pdf"));
        assert_eq!(keep_both(&tmp.path().join("noext")), tmp.path().join("noext (2)"));
    }

    #[test]
    fn batch_target_follows_the_policy() {
        let tmp = tempfile::tempdir().unwrap();
        let input = tmp.path().join("in").join("a.pdf");
        let out = tmp.path().join("out");
        pida_fs::create_dir_all(&out).unwrap();
        let (p, exists) = batch_target(&input, Some(&out), "_signed", ExistingPolicy::KeepBoth);
        assert_eq!((p, exists), (out.join("a_signed.pdf"), false));
        pida_fs::write(out.join("a_signed.pdf"), b"x").unwrap();
        assert_eq!(
            batch_target(&input, Some(&out), "_signed", ExistingPolicy::KeepBoth),
            (out.join("a_signed (2).pdf"), true)
        );
        assert_eq!(
            batch_target(&input, Some(&out), "_signed", ExistingPolicy::Overwrite),
            (out.join("a_signed.pdf"), true)
        );
        assert_eq!(batch_target(&input, Some(&out), "_signed", ExistingPolicy::Skip), (out.join("a_signed.pdf"), true));
        // ohne Ordner: neben der Quelle
        assert_eq!(batch_target(&input, None, "_signed", ExistingPolicy::Skip).0, tmp.path().join("in/a_signed.pdf"));
    }

    #[test]
    fn preanalysis_runs_one_at_a_time() {
        let mut p = Preanalysis::default();
        p.push(DocId(1));
        p.push(DocId(2));
        p.push(DocId(1));
        assert_eq!(p.next(|_| true), Some(DocId(1)));
        assert_eq!(p.next(|_| true), None, "eine zur Zeit");
        p.finished(DocId(1));
        assert_eq!(p.next(|d| d != DocId(2)), None, "verworfener Wunsch");
        p.push(DocId(3));
        assert_eq!(p.next(|_| true), Some(DocId(3)));
    }
}
