//! Reducer der Stapelübersicht und des Stapellaufs (ADR 0004 §3.8, Paket K2; nur mit [`Context::batch`]).
//!
//! | Zustand | Ereignis | Ergebnis |
//! |---|---|---|
//! | `Idle` / `Starting` | ≥ 2 Dokumente | `BatchOverview`, `Preanalyze` je Dokument (der Kern arbeitet sie nacheinander ab) |
//! | jeder Zustand mit Stapel | `Preanalyzed` | `Ready` (QR-Platzhalter → `Positioned` mit Plan), `Rejected`, `NeedsReview` |
//! | `BatchOverview` | `BatchItemOpened` | `Analyzing` → Arbeitsbereich mit dem Plan des Eintrags (`return_to_batch`) |
//! | Einzeldokument aus dem Stapel | `BackToBatch` | Plan aus dem Bearbeitungszustand (`origin == Auto` → keiner), `Positioned`/`Ready` |
//! | Einzeldokument aus dem Stapel | `Saved` | zurück zur Übersicht, `Signed`, Hinweis `ng-batch-item-signed` |
//! | Einzeldokument mit wartenden Dokumenten | `ShowBatch` | Übersicht (Platzierung als Plan) |
//! | `BatchOverview` | `BatchStarted` | `Ready`/`Positioned`, Signiermittel verfügbar, Ziel bestimmbar → `BatchSigning` + `OpenSession`; sonst Hinweis |
//! | `BatchSigning` | je Dokument | (`Skip`: Ziel prüfen) → `Loading` → (`ReadingCertificate` einmal je Sitzung) → `Preparing` → `Confirming` (`SignDocument`) → `Finishing` → `Writing` → `Signed` → nächstes |
//! | `BatchSigning` | `BatchSkip` | laufenden Auftrag abbrechen (`SkipCurrentJob`), nach dessen Ende `Skipped`, nächstes – Sitzung bleibt |
//! | `BatchSigning` | Ablehnung, wiederholbarer Dokumentfehler | Pause `Document` – nie automatisch weiter |
//! | `BatchSigning` | Verbindungsfehler | Pause `Session { new_session: false }`, Sitzung bleibt; „Erneut versuchen“ ohne neue Anmeldung |
//! | `BatchSigning` | Sitzungsfehler | `CloseSession`, Pause `Session { new_session: true }`; „Erneut versuchen“ öffnet eine neue Sitzung |
//! | `BatchSigning` | nicht wiederholbarer Dokumentfehler | `Failed`, weiter mit dem nächsten |
//! | `BatchSigning` | `BatchAbort`/`Cancel` | `CancelSession`; aktuelles `Cancelled`, wartende wieder `Ready`/`Positioned` |
//! | `BatchSigning` | letztes Dokument fertig | `CloseSession` → Übersicht mit Zusammenfassung, Hinweis `ng-batch-done` |
//! | `BatchOverview` | `BatchAutoPositionSet` | „Für diesen Stapel automatisch positionieren“ (Nachtrag 8): nur für diesen Stapel bis Ende/Abbruch des Laufs; automatische Pläne gelten als „Bereit“, Pläne der Nutzerin bleiben (außer `replace_manual`); startet nichts |
//!
//! Invariante A1: `Effect::OpenSession` entsteht hier nur für (`BatchStarted`, `BatchOverview`) und
//! (`Retry`, `BatchSigning` mit Sitzungspause).

use pida_backend_api::{BackendError, ErrorScope};
use pida_pades::{AcceptanceClass, OutputInfo};
use pida_placement::Placement;

use super::{discard_all, files_added_single, grown_reserve, start_current, workspace_position};
use crate::batch::{Batch, BatchItem, BatchItemStatus, BatchPause, BatchPhase, BatchRun, BatchSummary, DocSummary};
use crate::queue::{DocId, DocResult, InputDocument, Queue};
use crate::state::{
    BackendOption, BatchOutput, BlockSlot, Context, DocFacts, Effect, Event, ExistingPolicy, MSG_BATCH_ABORTED,
    MSG_BATCH_ADDED, MSG_BATCH_DONE, MSG_BATCH_ITEM_SIGNED, MSG_BATCH_NOTHING_TO_SIGN,
    MSG_BATCH_OUTPUT_FOLDER_REQUIRED, MSG_BATCH_PENDING_APP_REQUEST, MSG_CERT_CACHE_MISMATCH, MSG_CONSTRAINT_CERTIFIED,
    MSG_SIGNER_UNAVAILABLE, MSG_SIGNING_DECLINED, ParkedPlacement, PlaceholderFound, PlacementOrigin, PositionChoice,
    RetryTarget, SignFlow, SigningPhase, State, Step, Target, Transition, WorkflowError, WorkspaceEdit,
};

// ------------------------------------------------------------------------------------------------
// Hilfen für den Arbeitsbereich (Einzeldokument aus dem Stapel)
// ------------------------------------------------------------------------------------------------

/// Plan des aus der Übersicht geöffneten Dokuments `doc` (`Some((None, _))` = geöffnet, ohne Plan) und ob er
/// aus der Automatik stammt (dann folgt das Dokument wie jedes andere den aktuellen Einstellungen).
pub(super) fn opened_plan(queue: &Queue, doc: DocId) -> Option<(Option<PositionChoice>, bool)> {
    let b = queue.batch.as_ref()?;
    if b.open != Some(doc) {
        return None;
    }
    Some(b.item(doc).map_or((None, true), |i| (i.plan.clone(), i.auto_plan)))
}

/// Mehrfachsignatur: die weiteren Blöcke des aus der Übersicht geöffneten Dokuments wieder in den
/// Bearbeitungszustand legen (nach [`apply_plan`]; nur mit einem Plan der Nutzerin). Freie Blöcke kommen an ihre
/// Lage, Blöcke in einem Feld bzw. im QR-Platzhalter an ihr Ziel; die gemerkte freie Lage solcher Blöcke ist die
/// Startlage des Dokuments.
/// Liegt einer dieser Blöcke auf der virtuellen neuen Seite, bleibt „Auf neuer Seite“ an (`to_workspace`).
pub(super) fn apply_more(edit: &mut WorkspaceEdit, queue: &Queue, doc: &DocFacts) {
    let Some(item) = queue.batch.as_ref().and_then(|b| b.item(doc.id)) else { return };
    if item.auto_plan || item.more.is_empty() {
        return;
    }
    let last = doc.page_count.saturating_sub(1);
    // freie Blöcke dürfen auf der neuen Seite liegen (nur, wo das Dokument eine neue Seite erlaubt)
    let free_last = if doc.new_page_allowed() { doc.page_count } else { last };
    let memory = |page: usize| doc.start.map(|p| Placement { page_index: page.min(last), ..p });
    let mut others = Vec::with_capacity(item.more.len());
    for position in &item.more {
        let slot = match position {
            PositionChoice::Manual(p) => Some(BlockSlot {
                target: Target::Free,
                origin: PlacementOrigin::User,
                page: p.page_index.min(free_last),
                placement: Some(Placement { page_index: p.page_index.min(free_last), ..*p }),
            }),
            PositionChoice::Field(f) => {
                edit.fields.iter().position(|c| c.qualified_name == f.qualified_name).map(|i| {
                    let page = f.page_index.unwrap_or(0).min(last);
                    BlockSlot {
                        target: Target::Field(i),
                        origin: PlacementOrigin::Field,
                        page,
                        placement: memory(page),
                    }
                })
            }
            PositionChoice::Placeholder(qr) => {
                let page = qr.page_index.min(last);
                edit.qr = Some(qr.clone());
                Some(BlockSlot { target: Target::Qr, origin: PlacementOrigin::Qr, page, placement: memory(page) })
            }
            PositionChoice::Auto | PositionChoice::Invisible => None,
        };
        others.extend(slot);
    }
    edit.active = item.active.min(others.len());
    edit.others = others;
}

/// Analysewarnungen dieses Stapeleintrags bereits bestätigt?
pub(super) fn reviewed(queue: &Queue, doc: DocId) -> bool {
    queue.batch.as_ref().and_then(|b| b.item(doc)).is_some_and(|i| i.reviewed)
}

/// Plan der Nutzerin auf den Bearbeitungszustand übertragen (folgt danach nicht mehr den Einstellungen);
/// liefert die gemerkte freie Lage (manueller Plan).
pub(super) fn apply_plan(
    edit: &mut WorkspaceEdit,
    plan: Option<&PositionChoice>,
    doc: &DocFacts,
) -> Option<ParkedPlacement> {
    let n = doc.page_count;
    edit.automatic = false;
    match plan {
        Some(PositionChoice::Invisible) => edit.choose_invisible(true),
        Some(PositionChoice::Manual(p)) => {
            edit.target = Target::Free;
            edit.origin = PlacementOrigin::User;
            return Some(ParkedPlacement {
                page: p.page_index.min(n),
                placement: Some(*p),
                new_page: n > 0 && p.page_index >= n,
            });
        }
        Some(PositionChoice::Field(f)) => {
            if let Some(i) = edit.fields.iter().position(|c| c.qualified_name == f.qualified_name) {
                edit.target = Target::Field(i);
                edit.origin = PlacementOrigin::Field;
            }
        }
        Some(PositionChoice::Placeholder(qr)) => {
            edit.qr = Some(qr.clone());
            edit.target = Target::Qr;
            edit.origin = PlacementOrigin::Qr;
        }
        Some(PositionChoice::Auto) | None => {
            // „Freie Fläche suchen“ der Nutzerin: frei, automatische Lage, ohne Platzhalter
            edit.target = Target::Free;
            edit.origin = PlacementOrigin::Auto;
        }
    }
    None
}

/// Plan aus dem Bearbeitungszustand („Zurück zum Stapel“): unsichtbar, QR, Feld, frei verschoben; eine
/// automatische freie Lage ergibt keinen Plan (beim Signieren gilt dann die PDF-AS-Regel) – außer die
/// Nutzerin hat einen vorhandenen QR-Platzhalter abgewählt („Frei platzieren“): dann gilt die gezeigte Lage,
/// sonst signierte der Lauf doch in den Platzhalter (Vorschau = Ergebnis). Ob der Plan aus der Automatik
/// stammt (und damit weiter den Einstellungen folgt), sagt [`WorkspaceEdit::automatic`].
fn plan_of(
    edit: &WorkspaceEdit,
    placement: Option<Placement>,
    new_page: bool,
    doc: &DocFacts,
) -> Option<PositionChoice> {
    if edit.invisible && !edit.invisible_by_profile {
        return Some(PositionChoice::Invisible);
    }
    // „unsichtbar“ nur wegen des Profils: der sichtbare Plan bleibt (im Lauf erzwingt das Profil ohnehin
    // „unsichtbar“; zurück auf ein sichtbares Profil gilt wieder die Lage)
    let visible;
    let edit = if edit.invisible {
        visible = WorkspaceEdit { invisible: false, ..edit.clone() };
        &visible
    } else {
        edit
    };
    match edit.target {
        Target::Qr => edit.qr.clone().map(PositionChoice::Placeholder),
        Target::Field(i) => edit.fields.get(i).cloned().map(PositionChoice::Field),
        Target::Free if edit.automatic || (edit.origin == PlacementOrigin::Auto && edit.qr.is_none()) => None,
        Target::Free => match workspace_position(edit, placement, new_page, doc) {
            Some(PositionChoice::Auto) | None => None,
            plan => plan,
        },
    }
}

/// Hinweis mit Parametern.
fn notify(code: &str, params: &[(&str, String)]) -> Effect {
    Effect::NotifyWith(code.to_owned(), params.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect())
}

/// Einträge für alle offenen Dokumente ohne Eintrag anlegen (mit Voranalyse).
fn ensure_items(queue: &mut Queue, effects: &mut Vec<Effect>) {
    let ids: Vec<DocId> = queue.current.iter().chain(queue.pending.iter()).map(|d| d.id).collect();
    let batch = queue.batch.get_or_insert_with(Batch::default);
    for id in ids {
        if batch.item(id).is_none() {
            batch.items.push(BatchItem::analyzing(id));
            effects.push(Effect::Preanalyze(id));
        }
    }
}

/// Neue Stapelübersicht aus allen offenen Dokumenten (Voranalyse je Dokument).
pub(super) fn new_overview(mut queue: Queue, mut effects: Vec<Effect>) -> Transition {
    queue.park_current();
    ensure_items(&mut queue, &mut effects);
    if let Some(b) = &mut queue.batch {
        b.open = None;
    }
    Transition::with(State::BatchOverview { queue }, effects)
}

/// Wie ein Einzeldokument in die Übersicht zurückkehrt.
enum Back {
    /// Status und Plan unverändert (Abbrechen, „Start“)
    Keep,
    /// neuer Plan (`BackToBatch`, `ShowBatch`) und ob er aus der Automatik stammt; Warnungen gelten als
    /// bestätigt. Dazu die weiteren Blöcke der Mehrfachsignatur und die Stelle des Blocks des Plans.
    Plan(Box<Option<PositionChoice>>, bool, Vec<PositionChoice>, usize),
    /// beim Öffnen abgelehnt
    Rejected(String),
    /// einzeln signiert und gespeichert
    Signed(OutputInfo),
}

/// Zurück zur Übersicht: Dokument freigeben, Eintrag aktualisieren, kein aktuelles Dokument.
fn back(mut queue: Queue, doc: DocId, how: Back, mut effects: Vec<Effect>) -> Transition {
    effects.push(Effect::Discard(doc));
    // nach „Signatur fortsetzen“ eines unterbrochenen Stapels gibt es noch keine Einträge; auch der eben
    // signierte bekommt seine Voranalyse – nur für Miniatur, Seitenzahl und Größe der Zeile (Mobile-Check
    // 16.10.2026: die fortgesetzte Zeile blieb ohne sie), sein Status bleibt „Signiert“
    ensure_items(&mut queue, &mut effects);
    let batch = queue.batch.as_mut().expect("eben angelegt");
    batch.open = None;
    if let Some(item) = batch.item_mut(doc) {
        match &how {
            Back::Keep => {}
            Back::Plan(plan, automatic, more, active) => {
                item.plan = (**plan).clone();
                item.auto_plan = *automatic;
                item.more = more.clone();
                item.active = *active;
                item.reviewed = true;
                item.settle();
            }
            Back::Rejected(code) => item.status = BatchItemStatus::Rejected { code: code.clone() },
            Back::Signed(output) => item.status = BatchItemStatus::Signed { output: output.clone() },
        }
    }
    match how {
        Back::Signed(output) => {
            let file = queue.find(doc).map(|d| d.display_name.clone()).unwrap_or_default();
            effects.push(notify(MSG_BATCH_ITEM_SIGNED, &[("file", file)]));
            queue.complete(doc, DocResult::Signed { output });
        }
        _ => queue.park_current(),
    }
    Transition::with(State::BatchOverview { queue }, effects)
}

/// Einzeldokument im Arbeitsbereich → Übersicht mit der Platzierung als Plan des Dokuments („Zurück zum
/// Stapel“, „Zum Stapel“, „Weitere PDFs hinzufügen“).
fn back_with_plan(
    queue: Queue,
    placement: Option<Placement>,
    new_page: bool,
    doc: &DocFacts,
    edit: &WorkspaceEdit,
) -> Transition {
    let mut plan = plan_of(edit, placement, new_page, doc);
    // „Unsichtbar signieren“ von der Nutzerin ist ihr Plan: er folgt nicht mehr den Einstellungen
    // (sonst ginge er beim erneuten Öffnen bzw. bei der nächsten Einstellungsänderung verloren)
    let mut automatic = edit.automatic && !(edit.invisible && !edit.invisible_by_profile);
    // Mehrfachsignatur: die weiteren Blöcke gehören zum Plan; der ausgewählte Block bekommt dann immer seine
    // gezeigte Lage (Vorschau = Ergebnis), auch wenn sie aus „Freie Fläche suchen“ stammt
    let more: Vec<PositionChoice> =
        edit.others.iter().filter_map(|b| super::slot_position(edit, b, doc, new_page)).collect();
    if !more.is_empty() {
        automatic = false;
        if plan.is_none() && !edit.invisible {
            plan = workspace_position(edit, placement, new_page, doc);
        }
    }
    let active = edit.active_index().min(more.len());
    back(queue, doc.id, Back::Plan(Box::new(plan), automatic, more, active), Vec::new())
}

/// Zustand und Ereignis, die [`intercept`] nicht behandelt (geboxt: beide sind groß).
pub(super) type Unhandled = Box<(State, Event)>;

/// Ereignisse eines aus der Übersicht geöffneten Dokuments bzw. „Zur Stapelübersicht“; `Err` = wie ohne
/// Stapel weiter.
pub(super) fn intercept(state: State, event: Event, _ctx: &Context) -> Result<Transition, Unhandled> {
    let opened = state.opened_from_batch();
    let show = matches!(event, Event::ShowBatch);
    match event {
        Event::ShowBatch | Event::BackToBatch => match state {
            State::Positioning { queue, placement, new_page, doc, edit: Some(edit), .. }
                if opened || (show && !queue.pending.is_empty()) =>
            {
                Ok(back_with_plan(queue, placement, new_page, &doc, &edit))
            }
            State::Output { mut queue, saved: Some(output), doc, .. }
                if show && !opened && !queue.pending.is_empty() =>
            {
                // einzeln signiert, weitere Dokumente warten: Übersicht mit dem signierten Eintrag
                let batch = queue.batch.get_or_insert_with(Batch::default);
                let mut effects = vec![Effect::Discard(doc.id)];
                if batch.item(doc.id).is_none() {
                    batch.items.push(BatchItem {
                        doc: doc.id,
                        summary: None,
                        plan: None,
                        status: BatchItemStatus::Signed { output: output.clone() },
                        reviewed: true,
                        auto_plan: false,
                        more: Vec::new(),
                        active: 0,
                    });
                    // Miniatur, Seitenzahl und Größe der Zeile (der Status bleibt „Signiert“)
                    effects.push(Effect::Preanalyze(doc.id));
                }
                queue.complete(doc.id, DocResult::Signed { output });
                Ok(new_overview(queue, effects))
            }
            s @ (State::Analyzing { .. } | State::AnalysisWarning { .. }) if opened => {
                let (queue, doc) = queue_and_doc(s);
                Ok(back(queue, doc, Back::Keep, Vec::new()))
            }
            State::Error { queue, from: Some(f), .. } if opened && matches!(*f, State::Analyzing { .. }) => {
                let doc = queue.current_id().expect("geöffnet");
                Ok(back(queue, doc, Back::Keep, Vec::new()))
            }
            s => Err(Box::new((s, event))),
        },
        Event::Cancel | Event::WarningAnswered(false) if opened => match state {
            s @ (State::Analyzing { .. } | State::AnalysisWarning { .. }) => {
                let (queue, doc) = queue_and_doc(s);
                Ok(back(queue, doc, Back::Keep, Vec::new()))
            }
            State::Positioning { queue, doc, .. } if matches!(event, Event::Cancel) => {
                Ok(back(queue, doc.id, Back::Keep, Vec::new()))
            }
            State::Error { queue, error, from: Some(f), .. }
                if matches!(event, Event::Cancel) && matches!(*f, State::Analyzing { .. }) =>
            {
                let doc = queue.current_id().expect("geöffnet");
                Ok(back(queue, doc, Back::Rejected(error.code()), Vec::new()))
            }
            s => Err(Box::new((s, event))),
        },
        Event::Navigate(Step::Open) if opened => match state {
            s @ State::Signing { .. } => Err(Box::new((s, event))),
            State::Error { queue, from: Some(f), .. } if matches!(*f, State::Signing { .. }) => {
                // eine für „Erneut versuchen“ offen gehaltene Sitzung wird verworfen (F-72)
                let doc = queue.current_id().expect("geöffnet");
                Ok(back(queue, doc, Back::Keep, vec![Effect::CancelSession, Effect::DeletePersisted]))
            }
            State::Output { queue, saved: Some(output), doc, .. } => {
                Ok(back(queue, doc.id, Back::Signed(output), Vec::new()))
            }
            mut s => {
                let Some(doc) = s.queue().and_then(Queue::current_id) else { return Err(Box::new((s, event))) };
                let queue = std::mem::take(s.queue_mut().expect("Warteschlange"));
                Ok(back(queue, doc, Back::Keep, Vec::new()))
            }
        },
        Event::Saved(info) if opened => match state {
            State::Output { queue, saved: None, doc, .. } => Ok(back(queue, doc.id, Back::Signed(info), Vec::new())),
            s => Err(Box::new((s, Event::Saved(info)))),
        },
        e => Err(Box::new((state, e))),
    }
}

/// Warteschlange und aktuelles Dokument von `Analyzing`/`AnalysisWarning`.
fn queue_and_doc(state: State) -> (Queue, DocId) {
    match state {
        State::Analyzing { queue } => {
            let doc = queue.current_id().expect("geöffnet");
            (queue, doc)
        }
        State::AnalysisWarning { queue, doc, .. } => (queue, doc.id),
        other => unreachable!("{}", other.name()),
    }
}

// ------------------------------------------------------------------------------------------------
// Globale Ereignisse mit Stapel
// ------------------------------------------------------------------------------------------------

/// Automatischer Plan nach der Voranalyse und den **aktuellen** Einstellungen › Dokument (nie mit
/// unsichtbarem Profil): mit Platzhaltersuche „QR-Code-Platzhalter“ der QR-Platzhalter, mit „Leere
/// Signaturfelder“ das erste leere Signaturfeld, sonst keiner (PDF-AS-Regel).
fn automatic_plan(summary: Option<&DocSummary>, ctx: &Context) -> Option<PositionChoice> {
    if !ctx.placeholder_search || ctx.invisible {
        return None;
    }
    let s = summary?;
    if ctx.placeholder_fields {
        s.first_field.clone().map(PositionChoice::Field)
    } else {
        s.placeholder.clone().map(PositionChoice::Placeholder)
    }
}

/// Muss die Voranalyse für die aktuellen Einstellungen (Platzhaltersuche „QR-Code-Platzhalter“) erneut
/// laufen, weil sie damals nicht nach dem QR-Platzhalter gesucht hat?
fn needs_qr_search(summary: Option<&DocSummary>, ctx: &Context) -> bool {
    ctx.placeholder_search
        && !ctx.placeholder_fields
        && !ctx.invisible
        && summary.is_some_and(|s| !s.qr_searched && s.placeholder.is_none())
}

/// Folgt der Plan des Eintrags den Einstellungen? Ein Plan aus der Automatik ja; einen im Arbeitsbereich
/// platzierten (verschoben, Feld, QR von Hand, unsichtbar) hat die Nutzerin gewählt – der bleibt.
fn follows_settings(item: &BatchItem) -> bool {
    item.auto_plan
}

/// Ergebnis der Voranalyse in den Eintrag übernehmen (jeder Zustand mit Stapel).
pub(super) fn preanalyzed(
    mut state: State,
    doc: DocId,
    result: Result<DocSummary, WorkflowError>,
    ctx: &Context,
) -> Transition {
    let mut effects = Vec::new();
    let item = state.queue_mut().and_then(|q| q.batch.as_mut()).and_then(|b| b.item_mut(doc));
    if let Some(item) = item {
        let waiting = item.status == BatchItemStatus::Analyzing;
        match result {
            Ok(summary) if waiting && follows_settings(item) && needs_qr_search(Some(&summary), ctx) => {
                // die Einstellungen wurden während der Voranalyse auf „QR-Code-Platzhalter“ umgestellt: noch
                // einmal mit Suche (der Eintrag bleibt „Voranalyse läuft“)
                effects.push(Effect::Preanalyze(doc));
            }
            Ok(summary) => {
                item.summary = Some(summary);
                if waiting {
                    if follows_settings(item) {
                        item.plan = automatic_plan(item.summary.as_ref(), ctx);
                    }
                    item.settle();
                }
            }
            Err(e) if waiting => item.status = BatchItemStatus::Rejected { code: e.code() },
            Err(_) => {}
        }
    }
    Transition::with(state, effects)
}

/// Einstellungen geändert (Platzhaltersuche, Profil „unsichtbar“; ADR 0004 §3.11 „sofort wirksam“):
/// automatische Pläne der wartenden Einträge folgen den neuen Einstellungen (QR-Platzhalter bzw. erstes
/// leeres Signaturfeld an bzw. aus), Pläne der Nutzerin bleiben. Wurde ein Eintrag ohne QR-Suche
/// voranalysiert, läuft seine Voranalyse erneut (`Preanalyze`). Im Lauf entscheidet die Analyse des Laufs.
pub(super) fn settings_changed(state: &mut State, ctx: &Context) -> Vec<Effect> {
    let mut effects = Vec::new();
    if matches!(state, State::BatchSigning { .. }) {
        return effects;
    }
    let Some(batch) = state.queue_mut().and_then(|q| q.batch.as_mut()) else { return effects };
    for item in &mut batch.items {
        if !matches!(item.status, BatchItemStatus::Ready | BatchItemStatus::Positioned) || !follows_settings(item) {
            continue;
        }
        if needs_qr_search(item.summary.as_ref(), ctx) {
            item.summary = None;
            item.plan = None;
            item.status = BatchItemStatus::Analyzing;
            effects.push(Effect::Preanalyze(item.doc));
            continue;
        }
        item.plan = automatic_plan(item.summary.as_ref(), ctx);
        item.settle();
    }
    effects
}

/// Dateien hinzugefügt (Stapelmodus): ab zwei Dokumenten die Übersicht; mit Stapel neue Einträge (nie in
/// einen laufenden Lauf, A1); sonst wie ohne Stapel mit Hinweis `ng-batch-added`.
pub(super) fn files_added(state: State, docs: Vec<InputDocument>, ctx: &Context) -> Transition {
    let remember = |docs: &[InputDocument]| -> Vec<Effect> {
        docs.first()
            .and_then(|d| d.path.parent())
            .filter(|p| !p.as_os_str().is_empty())
            .map(|dir| vec![Effect::RememberDirectory(dir.to_path_buf())])
            .unwrap_or_default()
    };
    match state {
        State::Idle { mut queue } => {
            let effects = remember(&docs);
            queue.park_current();
            queue.append(docs);
            match queue.pending.len() {
                0 => Transition::with(State::Idle { queue }, effects),
                1 => {
                    queue.advance();
                    start_current(queue, ctx, effects)
                }
                _ => new_overview(queue, effects),
            }
        }
        State::Output { mut queue, saved: Some(output), doc, .. } => {
            // gespeichert: als signiert verbuchen, dann wie im Auswahl-Screen
            let effects = vec![Effect::Discard(doc.id)];
            queue.complete(doc.id, DocResult::Signed { output });
            queue.append(docs);
            if queue.pending.len() >= 2 {
                return new_overview(queue, effects);
            }
            queue.advance();
            start_current(queue, ctx, effects)
        }
        State::BatchOverview { mut queue } => {
            let added = queue.append(docs);
            let mut effects = Vec::new();
            ensure_items(&mut queue, &mut effects);
            if added.is_empty() {
                effects.clear();
            }
            Transition::with(State::BatchOverview { queue }, effects)
        }
        mut s if s.queue().is_some_and(|q| q.batch.is_some()) => {
            let held = matches!(&s, State::Error { from: Some(f), .. } if matches!(**f, State::Signing { .. }));
            let q = s.queue_mut().expect("Warteschlange");
            let added = q.append(docs).len();
            let mut effects = if held { vec![Effect::CancelSession] } else { Vec::new() };
            ensure_items(q, &mut effects);
            if added > 0 {
                effects.push(notify(MSG_BATCH_ADDED, &[("count", added.to_string())]));
            }
            Transition::with(s, effects)
        }
        s => {
            let before = s.queue().map_or(0, Queue::len);
            let mut t = files_added_single(s, docs, ctx);
            let added = t.state.queue().map_or(0, Queue::len).saturating_sub(before);
            let queued = !matches!(t.state, State::Analyzing { .. } | State::Idle { .. } | State::Starting);
            if added > 0 && queued {
                t.effects.push(notify(MSG_BATCH_ADDED, &[("count", added.to_string())]));
            }
            t
        }
    }
}

/// „Weitere PDFs hinzufügen“ (Kopfzeile „+ PDF“, Miniaturleiste; ADR 0004 Nachtrag 9): im Arbeitsbereich kommen
/// die Dateien zum offenen Dokument, und alle zusammen werden zum Stapel – die Platzierung des offenen Dokuments
/// wird wie bei „Zum Stapel“ dessen Plan (aus der Übersicht geöffnet: wie „Zurück zum Stapel“ mit den neuen
/// Einträgen). Nur Duplikate → unverändert. In allen anderen Zuständen wie [`files_added`] (z. B. Analysewarnung:
/// an die Warteschlange mit Hinweis `ng-batch-added`). Startet nichts (A1).
pub(super) fn more_files_added(state: State, docs: Vec<InputDocument>, ctx: &Context) -> Transition {
    match state {
        State::Positioning { mut queue, page, placement, new_page, doc, edit: Some(edit) } => {
            if queue.append(docs).is_empty() {
                let edit = Some(edit);
                return Transition::stay(State::Positioning { queue, page, placement, new_page, doc, edit });
            }
            back_with_plan(queue, placement, new_page, &doc, &edit)
        }
        s => files_added(s, docs, ctx),
    }
}

/// Neue Dateien zur geparkten Arbeit („Zum Stapel hinzufügen“, „Öffnen mit“; ADR 0004 Nachtrag 10, Modul
/// `parked`): die geparkten und die neuen Dokumente bilden den Stapel. Arbeitsbereich wie „Weitere PDFs
/// hinzufügen“ (die Platzierung bleibt Plan des Dokuments), Stapelübersicht mit neuen Einträgen; eine
/// Analysewarnung kommt als Eintrag in die Übersicht („Bitte prüfen“ nach der Voranalyse; aus der Übersicht
/// geöffnet: zurück, Eintrag unverändert). Nur Duplikate → unverändert. Sonst wie [`more_files_added`]. Startet
/// nichts (A1).
pub(super) fn join_parked(state: State, docs: Vec<InputDocument>, ctx: &Context) -> Transition {
    match state {
        State::AnalysisWarning { mut queue, warnings, doc, placeholder, fields } => {
            let added = !queue.append(docs).is_empty();
            let s = State::AnalysisWarning { queue, warnings, doc, placeholder, fields };
            if added { to_overview(s) } else { Transition::stay(s) }
        }
        s => more_files_added(s, docs, ctx),
    }
}

/// Offenes Dokument eines Stapels in die Übersicht („Weiter bearbeiten“ geparkter Arbeit, ADR 0004 Nachtrag 13,
/// Modul `parked`; [`join_parked`]): Arbeitsbereich wie „Zur Stapelübersicht“ bzw. „Zurück zum Stapel“ (die
/// Platzierung wird Plan des Dokuments, nichts geht verloren); Analysewarnung eines aus der Übersicht geöffneten
/// Eintrags zurück (Eintrag unverändert), sonst als Eintrag der Übersicht („Bitte prüfen“ nach der Voranalyse).
/// Übersicht und alle anderen Zustände unverändert. Startet nichts (A1).
pub(super) fn to_overview(state: State) -> Transition {
    match state {
        State::Positioning { queue, placement, new_page, doc, edit: Some(edit), .. } => {
            back_with_plan(queue, placement, new_page, &doc, &edit)
        }
        State::AnalysisWarning { queue, doc, .. } => {
            if queue.batch.as_ref().is_some_and(|b| b.open == Some(doc.id)) {
                back(queue, doc.id, Back::Keep, Vec::new())
            } else {
                new_overview(queue, vec![Effect::Discard(doc.id)])
            }
        }
        s => Transition::stay(s),
    }
}

// ------------------------------------------------------------------------------------------------
// Stapelübersicht
// ------------------------------------------------------------------------------------------------

pub(super) fn overview(state: State, event: Event, ctx: &Context) -> Transition {
    let State::BatchOverview { mut queue } = state else { unreachable!("BatchOverview") };
    let stay = |queue| Transition::stay(State::BatchOverview { queue });
    match event {
        // „Für diesen Stapel automatisch positionieren“ (Nachtrag 8): ändert die Einstellung nicht; von Hand
        // platzierte Einträge behalten ihren Plan, außer die Nutzerin bestätigt das Neusetzen ausdrücklich
        Event::BatchAutoPositionSet { enabled, replace_manual } => {
            let Some(batch) = queue.batch.as_mut() else { return stay(queue) };
            batch.auto_position_once = enabled;
            let mut effects = Vec::new();
            if enabled && replace_manual {
                for item in batch.items.iter_mut().filter(|i| i.status.signable() && !i.auto_plan) {
                    item.auto_plan = true;
                    item.more.clear();
                    item.active = 0;
                    if needs_qr_search(item.summary.as_ref(), ctx) {
                        item.summary = None;
                        item.plan = None;
                        item.status = BatchItemStatus::Analyzing;
                        effects.push(Effect::Preanalyze(item.doc));
                        continue;
                    }
                    item.plan = automatic_plan(item.summary.as_ref(), ctx);
                    item.settle();
                }
            }
            Transition::with(State::BatchOverview { queue }, effects)
        }
        Event::BatchItemOpened(doc) => {
            let openable = queue.batch.as_ref().and_then(|b| b.item(doc)).is_some_and(|i| {
                !matches!(
                    i.status,
                    BatchItemStatus::Signed { .. }
                        | BatchItemStatus::Rejected { .. }
                        | BatchItemStatus::Queued
                        | BatchItemStatus::Active
                )
            });
            if !openable || !queue.make_current(doc) {
                return stay(queue);
            }
            if let Some(b) = &mut queue.batch {
                b.open = Some(doc);
            }
            Transition::with(State::Analyzing { queue }, vec![Effect::Analyze(doc)])
        }
        Event::BatchItemRemoved(doc) => {
            let Some(batch) = queue.batch.as_mut() else { return stay(queue) };
            if batch.item(doc).is_none() {
                return stay(queue);
            }
            batch.items.retain(|i| i.doc != doc);
            let empty = batch.items.is_empty();
            queue.remove(doc);
            if empty {
                queue.batch = None;
                queue.park_current();
                return Transition::with(State::Idle { queue }, vec![Effect::Discard(doc)]);
            }
            Transition::with(State::BatchOverview { queue }, vec![Effect::Discard(doc)])
        }
        Event::BatchStarted { backend, docs, output, on_existing, options } => {
            start(queue, backend, &docs, output, on_existing, &options)
        }
        Event::Navigate(Step::Open) => discard_all(queue, Vec::new()),
        // „Stapel abbrechen“ während des Schreibens: gespeichert ist gespeichert
        Event::Saved(info) => {
            let Some(doc) = queue.batch.as_mut().and_then(|b| b.writing.take()) else { return stay(queue) };
            let batch = queue.batch.as_mut().expect("eben gelesen");
            if let Some(item) = batch.item_mut(doc) {
                item.status = BatchItemStatus::Signed { output: info.clone() };
            }
            if let Some(last) = &mut batch.last_run {
                last.signed += 1;
            }
            queue.complete(doc, DocResult::Signed { output: info });
            stay(queue)
        }
        Event::SaveFailed(_) | Event::BatchOutputDecided { .. } => {
            if let Some(b) = &mut queue.batch {
                b.writing = None;
            }
            stay(queue)
        }
        _ => stay(queue),
    }
}

/// „Alle signieren (n) mit X“: Prüfung, dann **eine** Sitzung (A1: nur hier und nach „Erneut versuchen“
/// einer Sitzungspause).
fn start(
    mut queue: Queue,
    backend: pida_backend_api::BackendId,
    docs: &[DocId],
    output: Option<BatchOutput>,
    on_existing: ExistingPolicy,
    options: &[BackendOption],
) -> Transition {
    let refuse =
        |queue, code: &str| Transition::with(State::BatchOverview { queue }, vec![Effect::Notify(code.to_owned())]);
    let order: Vec<DocId> = queue
        .batch
        .as_ref()
        .map(|b| b.items.iter().filter(|i| docs.contains(&i.doc) && i.status.signable()).map(|i| i.doc).collect())
        .unwrap_or_default();
    if order.is_empty() {
        return refuse(queue, MSG_BATCH_NOTHING_TO_SIGN);
    }
    let Some(caps) = options.iter().find(|o| o.id == backend).and_then(|o| o.capabilities.clone()) else {
        return refuse(queue, MSG_SIGNER_UNAVAILABLE);
    };
    let Some(output) = output else { return refuse(queue, MSG_BATCH_OUTPUT_FOLDER_REQUIRED) };
    queue.park_current();
    let batch = queue.batch.as_mut().expect("Einträge geprüft");
    batch.last_run = None;
    batch.writing = None;
    for doc in &order {
        if let Some(item) = batch.item_mut(*doc) {
            item.status = BatchItemStatus::Queued;
        }
    }
    let run = BatchRun {
        backend,
        caps,
        order,
        index: 0,
        phase: BatchPhase::OpeningSession,
        flow: SignFlow::default(),
        certificate_read: false,
        certificate_doc: None,
        output,
        on_existing,
        pause: None,
        confirmations_done: 0,
        signed: 0,
        session_open: false,
        doc: None,
        position: None,
        skipping: false,
        positions: Vec::new(),
        round: 0,
    };
    Transition::with(State::BatchSigning { queue, run: Box::new(run) }, vec![Effect::OpenSession(backend)])
}

// ------------------------------------------------------------------------------------------------
// Stapellauf
// ------------------------------------------------------------------------------------------------

/// Arbeitsstand eines Reducer-Schritts im Lauf.
struct Lauf<'c> {
    queue: Queue,
    run: BatchRun,
    effects: Vec<Effect>,
    ctx: &'c Context,
}

impl Lauf<'_> {
    fn stay(self) -> Transition {
        Transition::with(State::BatchSigning { queue: self.queue, run: Box::new(self.run) }, self.effects)
    }

    fn set_status(&mut self, doc: DocId, status: BatchItemStatus) {
        if let Some(item) = self.queue.batch.as_mut().and_then(|b| b.item_mut(doc)) {
            item.status = status;
        }
    }

    fn item(&self, doc: DocId) -> Option<&BatchItem> {
        self.queue.batch.as_ref().and_then(|b| b.item(doc))
    }

    /// Ziel beim Schreiben: „Überspringen“ ist vorab geprüft; entsteht die Datei danach, gilt „Beide
    /// behalten“ (ADR 0004 §3.8).
    fn write_policy(&self) -> ExistingPolicy {
        match self.run.on_existing {
            ExistingPolicy::Skip => ExistingPolicy::KeepBoth,
            p => p,
        }
    }

    /// Dokument an `run.index` beginnen; ohne weiteres Dokument Ende des Laufs.
    /// Mehrfachsignatur: zurück zur ersten Runde – nach einem Fehler wird ein Teilergebnis verworfen, „Erneut
    /// versuchen“ signiert das Dokument wieder ab der ersten Signatur (`Effect::Prepare` der ersten Runde setzt
    /// die Quelle im Kern auf das Original zurück).
    fn first_round(&mut self) {
        if let Some(first) = self.run.positions.first() {
            self.run.position = Some(first.clone());
        }
        self.run.round = 0;
    }

    /// Mehrfachsignatur: nächste Runde des aktuellen Dokuments (das Ergebnis ist die Quelle der nächsten
    /// Signatur, dieselbe Sitzung); `false` nach der letzten.
    fn next_round(&mut self) -> bool {
        let Some(next) = self.run.positions.get(self.run.round + 1).cloned() else { return false };
        self.run.round += 1;
        self.run.position = Some(next);
        self.effects.push(Effect::NextSignature);
        true
    }

    fn start_doc(mut self) -> Transition {
        self.run.doc = None;
        self.run.position = None;
        self.run.positions.clear();
        self.run.round = 0;
        self.run.skipping = false;
        self.run.flow = SignFlow::default();
        while let Some(doc) = self.run.current() {
            if self.item(doc).is_none() || !self.queue.make_current(doc) {
                self.run.index += 1;
                continue;
            }
            self.set_status(doc, BatchItemStatus::Active);
            if self.run.on_existing == ExistingPolicy::Skip {
                // vor dem Signieren: keine Bestätigung für ein Ergebnis, das nicht gespeichert würde
                self.run.phase = BatchPhase::CheckingTarget;
                let output = self.run.output.clone();
                self.effects.push(Effect::DecideBatchOutput { doc, output, on_existing: ExistingPolicy::Skip });
            } else {
                self.run.phase = BatchPhase::Loading;
                self.effects.push(Effect::Analyze(doc));
            }
            return self.stay();
        }
        self.finish()
    }

    fn next(mut self) -> Transition {
        self.run.index += 1;
        self.start_doc()
    }

    /// Dokument geladen: Zertifikat (einmal je Sitzung, nur wenn nötig) bzw. vorbereiten.
    fn continue_loaded(mut self) -> Transition {
        if self.run.flow.needs_certificate && !self.run.certificate_read {
            self.run.phase = BatchPhase::ReadingCertificate;
            self.effects.push(Effect::ReadCertificate);
            return self.stay();
        }
        self.prepare()
    }

    fn prepare(mut self) -> Transition {
        let (Some(doc), Some(position)) = (self.run.current(), self.run.position.clone()) else {
            return self.start_doc();
        };
        self.run.phase = BatchPhase::Preparing;
        self.effects.push(Effect::Prepare { doc, position, min_reserve: self.run.flow.min_reserve });
        self.stay()
    }

    fn write(mut self) -> Transition {
        let Some(doc) = self.run.current() else { return self.start_doc() };
        self.run.phase = BatchPhase::Writing;
        let output = self.run.output.clone();
        self.effects.push(Effect::DecideBatchOutput { doc, output, on_existing: self.write_policy() });
        self.stay()
    }

    /// Dokument nach einer Entscheidung abschließen: Dokument und Ergebnis freigeben, weiter.
    fn release(&mut self, doc: DocId) {
        self.effects.extend([Effect::ReleaseDocument, Effect::Discard(doc), Effect::DeletePersisted]);
    }

    fn doc_failed(mut self, error: WorkflowError) -> Transition {
        let Some(doc) = self.run.current() else { return self.start_doc() };
        self.set_status(doc, BatchItemStatus::Failed { error });
        self.release(doc);
        self.next()
    }

    fn doc_skipped(mut self, existing: bool) -> Transition {
        let Some(doc) = self.run.current() else { return self.start_doc() };
        self.set_status(doc, BatchItemStatus::Skipped { existing });
        self.release(doc);
        self.next()
    }

    /// „Dokument überspringen“ abgeschlossen (der laufende Schritt ist zurück). Nach einem Sitzungsfehler
    /// des abgebrochenen Auftrags wartet das nächste Dokument auf „Erneut versuchen“ (neue Sitzung, A1).
    fn skip_done(mut self, session_error: Option<WorkflowError>) -> Transition {
        let Some(doc) = self.run.current() else { return self.start_doc() };
        self.run.skipping = false;
        if self.run.phase == BatchPhase::Confirming && self.run.caps.document_leaves_device {
            self.effects.push(Effect::Notify(MSG_BATCH_PENDING_APP_REQUEST.into()));
        }
        self.set_status(doc, BatchItemStatus::Skipped { existing: false });
        self.release(doc);
        match session_error {
            Some(error) => {
                self.close_session();
                self.run.index += 1;
                self.pause_for_next_session(error)
            }
            None => self.next(),
        }
    }

    fn close_session(&mut self) {
        self.effects.push(Effect::CloseSession);
        self.run.session_open = false;
        self.run.certificate_read = false;
        self.run.certificate_doc = None;
    }

    /// Sitzung geschlossen, nächstes Dokument wartet auf „Erneut versuchen“; ohne weiteres Dokument Ende.
    fn pause_for_next_session(mut self, error: WorkflowError) -> Transition {
        self.run.doc = None;
        self.run.position = None;
        self.run.positions.clear();
        self.run.round = 0;
        match self.run.current() {
            Some(next) => {
                self.run.phase = BatchPhase::OpeningSession;
                self.run.pause = Some(BatchPause::Session {
                    doc: Some(next),
                    error,
                    new_session: true,
                    resume: BatchPhase::OpeningSession,
                });
                self.stay()
            }
            None => self.finish(),
        }
    }

    /// Phase, die „Erneut versuchen“ in derselben Sitzung wieder aufnimmt.
    fn resume_phase(&self) -> BatchPhase {
        match self.run.phase {
            BatchPhase::ReadingCertificate => BatchPhase::ReadingCertificate,
            BatchPhase::Writing => BatchPhase::Writing,
            _ => BatchPhase::Preparing,
        }
    }

    fn pause_document(mut self, error: WorkflowError, retry: RetryTarget, resume: BatchPhase) -> Transition {
        let Some(doc) = self.run.current() else { return self.start_doc() };
        if resume != BatchPhase::Writing {
            self.effects.push(Effect::ReleaseDocument);
            self.first_round();
        }
        self.run.pause = Some(BatchPause::Document { doc, error, retry, resume });
        self.stay()
    }

    /// Aktuelles Dokument begonnen (`Active`)?
    fn doc_started(&self) -> Option<DocId> {
        self.run.current().filter(|d| self.item(*d).is_some_and(|i| i.status == BatchItemStatus::Active))
    }

    fn backend_failed(mut self, e: BackendError) -> Transition {
        let error = WorkflowError::from_backend(&e);
        match e.scope() {
            // eigener Abbruch außerhalb von „Überspringen“ (z. B. „Abbrechen“ in einer Meldung, die nicht von
            // A-Trust stammt): wie „Stapel abbrechen“
            ErrorScope::Cancelled => self.abort(),
            ErrorScope::Document if e.is_retryable() => {
                let resume = self.resume_phase();
                self.pause_document(error, RetryTarget::Prepare, resume)
            }
            ErrorScope::Document => self.doc_failed(error),
            ErrorScope::Transient if self.run.session_open => {
                let resume = self.resume_phase();
                if resume != BatchPhase::Writing {
                    self.effects.push(Effect::ReleaseDocument);
                }
                let doc = self.doc_started();
                self.run.pause = Some(BatchPause::Session { doc, error, new_session: false, resume });
                self.stay()
            }
            _ => self.session_failed(error),
        }
    }

    /// Sitzung unbrauchbar (bzw. gar nicht geöffnet): schließen, Pause; „Erneut versuchen“ öffnet eine neue
    /// Sitzung (A1) und macht beim aktuellen Dokument weiter.
    fn session_failed(mut self, error: WorkflowError) -> Transition {
        self.effects.push(Effect::ReleaseDocument);
        self.close_session();
        let doc = self.doc_started();
        self.run.phase = BatchPhase::OpeningSession;
        self.run.pause =
            Some(BatchPause::Session { doc, error, new_session: true, resume: BatchPhase::OpeningSession });
        self.stay()
    }

    /// „Stapel abbrechen“: Sitzung abbrechen (F-72: Passwort gelöscht), aktuelles `Cancelled`, wartende
    /// wieder bereit; Gespeichertes bleibt.
    fn abort(mut self) -> Transition {
        self.effects.extend([Effect::CancelSession, Effect::DeletePersisted]);
        if let Some(doc) = self.run.current() {
            if self.item(doc).is_some_and(|i| i.status == BatchItemStatus::Active) {
                self.set_status(doc, BatchItemStatus::Cancelled);
                // das Schreiben des Ergebnisses kann schon laufen: `Saved` kommt dann in der Übersicht an
                if self.run.phase == BatchPhase::Writing
                    && self.run.pause.is_none()
                    && let Some(b) = self.queue.batch.as_mut()
                {
                    b.writing = Some(doc);
                }
            }
            self.effects.push(Effect::Discard(doc));
        }
        let signed = self.run.signed;
        self.effects.push(notify(MSG_BATCH_ABORTED, &[("signed", signed.to_string())]));
        self.into_overview(true)
    }

    /// Letztes Dokument fertig: Sitzung schließen, Übersicht mit Zusammenfassung.
    fn finish(mut self) -> Transition {
        self.effects.push(Effect::CloseSession);
        let (signed, total) = (self.run.signed, self.run.order.len());
        self.effects.push(notify(MSG_BATCH_DONE, &[("signed", signed.to_string()), ("total", total.to_string())]));
        self.into_overview(false)
    }

    fn into_overview(mut self, aborted: bool) -> Transition {
        self.queue.park_current();
        let folder = match &self.run.output {
            BatchOutput::Folder(p) => Some(p.clone()),
            BatchOutput::Configured => None,
        };
        let total = u32::try_from(self.run.order.len()).unwrap_or(u32::MAX);
        let batch = self.queue.batch.get_or_insert_with(Batch::default);
        for item in &mut batch.items {
            if matches!(item.status, BatchItemStatus::Queued | BatchItemStatus::Active) {
                item.status = if item.plan.is_some() { BatchItemStatus::Positioned } else { BatchItemStatus::Ready };
            }
        }
        batch.last_run = Some(BatchSummary { total, signed: self.run.signed, aborted, folder });
        // „Für diesen Stapel automatisch positionieren“ galt nur für diesen Lauf (Nachtrag 8)
        batch.auto_position_once = false;
        Transition::with(State::BatchOverview { queue: self.queue }, self.effects)
    }

    /// Analyse des aktuellen Dokuments fertig.
    fn analyzed(
        mut self,
        class: AcceptanceClass,
        placeholder: Option<PlaceholderFound>,
        fields: Vec<pida_placement::EmptyFieldCandidate>,
        facts: DocFacts,
        warnings: Vec<String>,
    ) -> Transition {
        if matches!(class, AcceptanceClass::K5Rejected | AcceptanceClass::K2Repairable) {
            return self.doc_failed(WorkflowError::Rejected { class, code: "ng-document-rejected".into() });
        }
        let doc = facts.id;
        let item = self.item(doc).cloned();
        if !warnings.is_empty() && !item.as_ref().is_some_and(|i| i.reviewed) {
            // Analysewarnung: erst einzeln öffnen und bestätigen – nie im Lauf durchwinken
            self.set_status(doc, BatchItemStatus::NeedsReview { codes: warnings });
            self.effects.push(Effect::Discard(doc));
            return self.next();
        }
        let ctx = self.ctx;
        // automatisch nach den aktuellen Einstellungen › Dokument: QR-Platzhalter bzw. erstes leeres
        // Signaturfeld (nur mit Platzhaltersuche), sonst PDF-AS-Regel
        // nur ein für den Block geeignetes Feld (Einrastregel, ADR 0004 Nachtrag 22)
        let first_field = fields.into_iter().find(|f| f.auto_ok);
        let automatic = match (placeholder, first_field) {
            _ if ctx.invisible => PositionChoice::Invisible,
            (_, Some(f)) if ctx.placeholder_search && ctx.placeholder_fields => PositionChoice::Field(f),
            (Some(PlaceholderFound::Qr(qr)), _) if ctx.placeholder_search && !ctx.placeholder_fields => {
                PositionChoice::Placeholder(qr)
            }
            _ => PositionChoice::Auto,
        };
        // ein unsichtbares Profil erzwingt „unsichtbar“ – auch gegen einen Plan aus der Übersicht bzw. dem
        // Arbeitsbereich, der vor dem Wechsel des Profils entstand (wie `settings_changed` im Arbeitsbereich);
        // ein automatischer Plan (Platzhalter der Voranalyse) folgt den aktuellen Einstellungen
        let manual = item.filter(|i| !follows_settings(i));
        let plan = manual.as_ref().and_then(|i| i.plan.clone());
        let position = if ctx.invisible { PositionChoice::Invisible } else { plan.unwrap_or(automatic) };
        // Mehrfachsignatur: von Hand platzierte weitere Blöcke – je Block eine Signatur (unsichtbar: eine)
        let positions = match &manual {
            Some(i) if !i.more.is_empty() && position != PositionChoice::Invisible => i.positions(position.clone()),
            _ => Vec::new(),
        };
        let is_new_visible = |p: &PositionChoice| {
            matches!(p, PositionChoice::Manual(_) | PositionChoice::Auto | PositionChoice::Placeholder(_))
        };
        let new_visible = is_new_visible(&position) || positions.iter().any(is_new_visible);
        let position = positions.first().cloned().unwrap_or(position);
        self.run.positions = positions;
        self.run.round = 0;
        if new_visible && !facts.new_visible_field_allowed {
            // DocMDP-zertifiziert: nur vorhandene Felder oder unsichtbar
            let error =
                WorkflowError::Rejected { class: AcceptanceClass::K4Restricted, code: MSG_CONSTRAINT_CERTIFIED.into() };
            return self.doc_failed(error);
        }
        self.run.flow = SignFlow::new(&self.run.caps, &position);
        self.run.doc = Some(facts);
        self.run.position = Some(position);
        self.continue_loaded()
    }
}

/// Ereignis passt zur Phase des laufenden Schritts.
fn completes(phase: BatchPhase, done: SigningPhase) -> bool {
    matches!(
        (phase, done),
        (BatchPhase::OpeningSession, SigningPhase::OpeningSession)
            | (BatchPhase::ReadingCertificate, SigningPhase::ReadingCertificate)
            | (BatchPhase::Preparing, SigningPhase::Preparing)
            | (BatchPhase::Confirming, SigningPhase::Signing)
            | (BatchPhase::Finishing, SigningPhase::Finishing)
    )
}

pub(super) fn signing(state: State, event: Event, ctx: &Context) -> Transition {
    let State::BatchSigning { queue, run } = state else { unreachable!("BatchSigning") };
    let mut l = Lauf { queue, run: *run, effects: Vec::new(), ctx };
    let current = l.run.current();

    // Pause: nur ausdrückliche Entscheidungen
    if let Some(pause) = l.run.pause.clone() {
        return match event {
            Event::Retry => {
                l.run.pause = None;
                if let Some(doc) = pause.doc() {
                    l.set_status(doc, BatchItemStatus::Active);
                }
                match pause {
                    BatchPause::Session { new_session: true, .. } => {
                        // A1: eine weitere Sitzung nur nach „Erneut versuchen“ einer Sitzungspause
                        l.run.phase = BatchPhase::OpeningSession;
                        l.effects.push(Effect::OpenSession(l.run.backend));
                        l.stay()
                    }
                    BatchPause::Document { resume, .. } | BatchPause::Session { resume, .. } => match resume {
                        BatchPhase::ReadingCertificate => {
                            l.run.certificate_read = false;
                            l.continue_loaded()
                        }
                        BatchPhase::Writing => l.write(),
                        _ if l.run.doc.is_some() => l.prepare(),
                        _ => l.start_doc(),
                    },
                }
            }
            Event::BatchSkip => {
                l.run.pause = None;
                match pause {
                    BatchPause::Session { doc: None, .. } => {
                        l.run.pause = Some(pause);
                        l.stay()
                    }
                    BatchPause::Session { new_session: true, error, doc: Some(doc), .. } => {
                        // Sitzung bereits geschlossen: das nächste Dokument wartet weiter auf eine neue Sitzung
                        l.set_status(doc, BatchItemStatus::Skipped { existing: false });
                        l.effects.extend([Effect::ReleaseDocument, Effect::Discard(doc), Effect::DeletePersisted]);
                        l.run.index += 1;
                        l.pause_for_next_session(error)
                    }
                    _ => l.doc_skipped(false),
                }
            }
            Event::BatchAbort | Event::Cancel => l.abort(),
            _ => l.stay(),
        };
    }

    match event {
        Event::BatchAbort | Event::Cancel => l.abort(),
        Event::BatchSkip => {
            // ohne begonnenes Dokument (Sitzung wird geöffnet) gibt es nichts zu überspringen
            if l.doc_started().is_none() || l.run.skipping {
                return l.stay();
            }
            l.run.skipping = true;
            l.effects.push(Effect::SkipCurrentJob);
            l.stay()
        }
        // gespeichert ist gespeichert – auch nach „Überspringen“ während des Schreibens
        Event::Saved(info) if l.run.phase == BatchPhase::Writing => {
            let Some(doc) = current else { return l.stay() };
            l.run.skipping = false;
            l.set_status(doc, BatchItemStatus::Signed { output: info.clone() });
            l.run.signed += 1;
            l.queue.complete(doc, DocResult::Signed { output: info });
            l.release(doc);
            l.next()
        }
        Event::PhaseCompleted(done) if completes(l.run.phase, done) => {
            if l.run.skipping {
                return l.skip_done(None);
            }
            match l.run.phase {
                BatchPhase::OpeningSession => {
                    l.run.session_open = true;
                    let loaded = l.run.doc.as_ref().is_some_and(|d| Some(d.id) == current) && l.run.position.is_some();
                    if loaded { l.continue_loaded() } else { l.start_doc() }
                }
                BatchPhase::ReadingCertificate => {
                    l.run.certificate_read = true;
                    l.run.certificate_doc = current;
                    l.prepare()
                }
                BatchPhase::Preparing => {
                    l.run.phase = BatchPhase::Confirming;
                    l.effects.push(Effect::SignDocument);
                    l.stay()
                }
                BatchPhase::Confirming => {
                    l.run.confirmations_done += 1;
                    l.run.phase = BatchPhase::Finishing;
                    l.effects.push(Effect::Finish);
                    l.stay()
                }
                // Mehrfachsignatur: weitere Blöcke dieses Dokuments zuerst, dann speichern
                BatchPhase::Finishing => {
                    if l.next_round() {
                        l.prepare()
                    } else {
                        l.write()
                    }
                }
                _ => l.stay(),
            }
        }
        Event::BatchOutputDecided { doc, path, exists } if Some(doc) == current => match l.run.phase {
            BatchPhase::CheckingTarget => {
                if l.run.skipping {
                    return l.skip_done(None);
                }
                if exists {
                    // „Überspringen“: keine Bestätigung für ein Ergebnis, das nicht gespeichert würde
                    return l.doc_skipped(true);
                }
                l.run.phase = BatchPhase::Loading;
                l.effects.push(Effect::Analyze(doc));
                l.stay()
            }
            BatchPhase::Writing => {
                let overwrite = l.run.on_existing == ExistingPolicy::Overwrite && exists;
                l.effects.push(if overwrite { Effect::Overwrite(path) } else { Effect::Write(path) });
                l.stay()
            }
            _ => l.stay(),
        },
        Event::Analyzed {
            doc,
            class,
            placeholder,
            signed,
            page_count,
            start,
            warnings,
            fields,
            new_visible_field_allowed,
        } if Some(doc) == current && l.run.phase == BatchPhase::Loading => {
            if l.run.skipping {
                return l.skip_done(None);
            }
            let facts = DocFacts { id: doc, page_count, signed, class, start, new_visible_field_allowed };
            l.analyzed(class, placeholder, fields, facts, warnings)
        }
        Event::AnalysisFailed { doc, error } if Some(doc) == current && l.run.phase == BatchPhase::Loading => {
            if l.run.skipping {
                return l.skip_done(None);
            }
            l.doc_failed(error)
        }
        Event::BackendFailed(e) if l.run.skipping => {
            let session = (e.scope() == ErrorScope::Session).then(|| WorkflowError::from_backend(&e));
            l.skip_done(session)
        }
        Event::BackendFailed(e) => l.backend_failed(e),
        Event::SigningDeclined if l.run.skipping => l.skip_done(None),
        Event::SigningDeclined if !l.run.session_open => {
            // beim Öffnen abgelehnt (z. B. Anmeldung bzw. Entsperren abgebrochen): es gibt keine Sitzung, in
            // der „Erneut anfordern“ weitermachen könnte – „Erneut versuchen“ öffnet eine neue (A1)
            let error = WorkflowError::Backend { code: MSG_SIGNING_DECLINED.into(), detail: String::new() };
            l.session_failed(error)
        }
        Event::SigningDeclined => {
            // in der App abgelehnt bzw. abgelaufen: nie automatisch weiter; Zugangsdaten bleiben (§8 F10)
            let error = WorkflowError::Backend { code: MSG_SIGNING_DECLINED.into(), detail: String::new() };
            let resume = l.resume_phase();
            l.pause_document(error, RetryTarget::Prepare, resume)
        }
        Event::SigningFailed(_) | Event::SelfCheckFailed(_) | Event::CmsTooLarge { .. } | Event::SaveFailed(_)
            if l.run.skipping =>
        {
            l.skip_done(None)
        }
        Event::SigningFailed(WorkflowError::Prepare { code, detail }) if code == MSG_CERT_CACHE_MISMATCH => {
            // Zertifikat aus dem Cache passt nicht: neu lesen (gleiche Sitzung) und neu vorbereiten
            l.run.certificate_read = false;
            l.run.certificate_doc = None;
            let error = WorkflowError::Prepare { code, detail };
            l.pause_document(error, RetryTarget::Prepare, BatchPhase::ReadingCertificate)
        }
        Event::SigningFailed(error) => l.doc_failed(error),
        Event::SelfCheckFailed(v) => l.doc_failed(WorkflowError::Invariant(v)),
        Event::CmsTooLarge { needed, reserve } => {
            let n = grown_reserve(needed, reserve);
            l.run.flow.min_reserve = Some(n.max(l.run.flow.min_reserve.unwrap_or(0)));
            l.pause_document(
                WorkflowError::CmsTooLarge { needed, reserve },
                RetryTarget::PrepareWithReserve(n),
                BatchPhase::Preparing,
            )
        }
        Event::SaveFailed(detail) if l.run.phase == BatchPhase::Writing => {
            l.pause_document(WorkflowError::Save(detail), RetryTarget::Save, BatchPhase::Writing)
        }
        _ => l.stay(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn facts(n: usize) -> DocFacts {
        DocFacts {
            id: DocId(1),
            page_count: n,
            signed: false,
            class: AcceptanceClass::K1Regular,
            start: None,
            new_visible_field_allowed: true,
        }
    }

    #[test]
    fn plan_roundtrip_through_the_workspace() {
        let p = Placement { page_index: 1, x: 10.0, y: 300.0, width: 100.0, height: 40.0 };
        let mut edit = WorkspaceEdit::new(Vec::new(), None, false);
        assert_eq!(plan_of(&edit, Some(p), false, &facts(3)), None, "automatisch = kein Plan");
        let parked = apply_plan(&mut edit, Some(&PositionChoice::Manual(p)), &facts(3));
        assert_eq!(parked.and_then(|x| x.placement), Some(p));
        assert_eq!(plan_of(&edit, Some(p), false, &facts(3)), Some(PositionChoice::Manual(p)));
        let mut edit = WorkspaceEdit::new(Vec::new(), None, false);
        apply_plan(&mut edit, Some(&PositionChoice::Invisible), &facts(3));
        assert_eq!(plan_of(&edit, None, false, &facts(3)), Some(PositionChoice::Invisible));
        // „unsichtbar“ nur wegen des Profils: der sichtbare Plan bleibt (das Profil erzwingt es im Lauf)
        let mut edit = WorkspaceEdit::new(Vec::new(), None, true);
        apply_plan(&mut edit, Some(&PositionChoice::Manual(p)), &facts(3));
        assert!(edit.invisible && edit.invisible_by_profile);
        assert_eq!(plan_of(&edit, Some(p), false, &facts(3)), Some(PositionChoice::Manual(p)));
        // neue Seite bleibt erhalten
        let np = Placement { page_index: 3, ..p };
        let mut edit = WorkspaceEdit::new(Vec::new(), None, false);
        let parked = apply_plan(&mut edit, Some(&PositionChoice::Manual(np)), &facts(3)).unwrap();
        assert!(parked.new_page);
        assert_eq!(plan_of(&edit, Some(np), true, &facts(3)), Some(PositionChoice::Manual(np)));
    }

    #[test]
    fn notify_carries_parameters() {
        let e = notify(MSG_BATCH_DONE, &[("signed", "2".into()), ("total", "3".into())]);
        let Effect::NotifyWith(code, params) = e else { panic!() };
        assert_eq!(code, MSG_BATCH_DONE);
        assert_eq!(params, BTreeMap::from([("signed".into(), "2".into()), ("total".into(), "3".into())]));
    }
}
