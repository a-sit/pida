//! Der reine Reducer (docs/analyse/01 §3.4 ohne implizite Re-Entry-Semantik).
//!
//! Jeder Übergang der PDF-Over-Tabelle ist hier ein expliziter Fall; die Abbildung steht in der
//! Crate-Doku ([`crate`]). Ereignisse, die im aktuellen Zustand keine Bedeutung haben (z. B. ein
//! verspätetes Ergebnis eines abgebrochenen Effekts), lassen den Zustand unverändert.
//!
//! Mit [`Context::workspace`] (Oberfläche „Arbeitsbereich“, ADR 0004 §3.2–§3.7) gelten zusätzlich:
//! keine Platzhalterfrage, keine automatische Backend-Wahl (außer der Startargument-Automatik
//! [`Context::cli_auto`] mit einem Signiermittel, bei dem das Dokument auf dem Gerät bleibt), Signieren
//! nur über [`Event::SignRequested`], Abbruch zurück in den Arbeitsbereich, Wiederholen in derselben
//! Sitzung (Invariante **A1**: `Effect::OpenSession` nur in den Paaren aus ADR 0004 §3.2).
//!
//! Mit der Stapelübersicht ([`Context::batch`], ADR 0004 §3.8) kommen `BatchOverview` und `BatchSigning`
//! hinzu (Modul `batch_run`): ab zwei Dokumenten die Übersicht statt „ein Dokument nach dem anderen“, ein
//! Lauf mit **einer** Sitzung (`Effect::OpenSession` nur nach `BatchStarted` bzw. „Erneut versuchen“ einer
//! Sitzungspause), ein aus der Übersicht geöffnetes Dokument kehrt dorthin zurück.
//!
//! „Zur Startseite“ ([`Event::WentHome`], ADR 0004 Nachtrag 10, Modul `parked`) parkt offene Arbeit statt sie zu
//! verwerfen; geparkte Arbeit ist verdeckt und öffnet nie eine Sitzung.

mod batch_run;
mod parked;

use pida_backend_api::{BackendId, ErrorScope};
use pida_pades::AcceptanceClass;
use pida_placement::{EmptyFieldCandidate, Placement};

use crate::output::{ExitCode, OutputDecision, save_dialog};
use crate::queue::{DocResult, Queue};
use crate::state::{
    BackendOption, BlockSlot, Context, DocFacts, Effect, Event, MAX_BLOCKS, MSG_BACKEND_CHOICE_REQUIRED,
    MSG_BLOCK_TARGET_TAKEN, MSG_BLOCKS_LIMIT, MSG_CONSTRAINT_CERTIFIED, MSG_CONSTRAINT_INVISIBLE_PROFILE,
    MSG_DEFAULT_BACKEND_UNAVAILABLE, MSG_DOCUMENT_LEAVES_DEVICE, MSG_FIELD_LOCKED, MSG_NO_BACKEND,
    MSG_POSITIONING_NOT_POSSIBLE, MSG_RESUME_BACKEND_UNAVAILABLE, MSG_SIGNER_UNAVAILABLE, MSG_SIGNING_CANCELLED,
    MSG_SIGNING_DECLINED, ParkedPlacement, PlaceholderAnswer, PlaceholderFound, PlacementLock, PlacementOrigin,
    PositionChoice, ResumePlan, RetryTarget, Rounds, SignFlow, SigningPhase, State, Step, Target, Transition,
    WorkflowError, WorkspaceEdit, backend_for_choice,
};

/// Meldungs-ID des Platzierungshinweises „neue Seite nicht erlaubt“ (`PlacementNotice::code`).
const MSG_NEW_PAGE_NOT_ALLOWED: &str = "placement.notice.new_page_not_allowed";

/// Mindestreserve nach „CMS zu groß“ (Plan §6.2: `max(16 KiB, 2 × gemessenes CMS)`).
const MIN_GROWN_RESERVE: usize = 16 * 1024;

/// Neue `/Contents`-Reserve nach [`WorkflowError::CmsTooLarge`]: `max(16 KiB, 2 × benötigt, alt + 1 KiB)`,
/// aufgerundet auf volle KiB.
pub fn grown_reserve(needed: usize, reserve: usize) -> usize {
    let target = needed.saturating_mul(2).max(reserve.saturating_add(1024)).max(MIN_GROWN_RESERVE);
    target.div_ceil(1024).saturating_mul(1024)
}

/// Reiner Reducer.
///
/// Geparkte Arbeit („Zur Startseite“, ADR 0004 Nachtrag 10, [`State::parked`]) behandelt das Modul `parked`:
/// nur Fortsetzen, Verwerfen, neue Dateien, Beenden und Ergebnisse von Effekten bzw. geänderte Einstellungen
/// wirken – alles andere betrifft die verdeckte Arbeit und bleibt ohne Wirkung (F-54).
pub fn reduce(state: State, event: Event, ctx: &Context) -> Transition {
    if state.parked() {
        return parked::reduce(state, event, ctx);
    }
    match event {
        Event::WentHome => parked::went_home(state, ctx),
        // ohne geparkte Arbeit ohne Wirkung
        Event::ParkedResumed | Event::ParkedDiscarded => Transition::stay(state),
        event => active(state, event, ctx),
    }
}

/// Reducer ohne geparkte Arbeit (bzw. für die an geparkte Arbeit weitergereichten Ereignisse).
fn active(state: State, event: Event, ctx: &Context) -> Transition {
    // Ereignisse, die in jedem Zustand gleich behandelt werden
    match event {
        Event::Quit => return quit(state),
        Event::FilesAdded(docs) => return files_added(state, docs, ctx),
        // „+ PDF“ im Arbeitsbereich (ADR 0004 Nachtrag 9)
        Event::MoreFilesAdded(docs) => return more_files_added(state, docs, ctx),
        Event::DocumentReplaced(docs) => return document_replaced(state, docs, ctx),
        Event::SettingsChanged => return settings_changed(state, ctx),
        // Mobile-Lebenszyklus: ein App-Wechsel (etwa in die App „ID Austria“) ist **kein Abbruch** – weder
        // die Sitzung noch der persistierte Zustand werden angetastet (Plan §3.5)
        Event::Backgrounded | Event::Foregrounded => return Transition::stay(state),
        // ohne Oberfläche gibt es keine Hauptleiste (sonst führte „Positionierung“ in einen Dialog);
        // der Arbeitsbereich hat keine Hauptleiste für Positionieren/Signieren (ADR 0004 §3.2)
        Event::Navigate(_) if ctx.headless => return Transition::stay(state),
        Event::Navigate(Step::Positioning | Step::Signing) if ctx.workspace => return Transition::stay(state),
        // Stapel: das Ergebnis der Voranalyse gilt in jedem Zustand mit Stapel (ADR 0004 §3.8)
        Event::Preanalyzed { doc, result } => return batch_run::preanalyzed(state, doc, result, ctx),
        _ => {}
    }
    // Stapel: Einzeldokument aus der Übersicht kehrt dorthin zurück, „Zur Stapelübersicht“
    let (state, event) = if ctx.batch() {
        match batch_run::intercept(state, event, ctx) {
            Ok(t) => return t,
            Err(pair) => *pair,
        }
    } else {
        (state, event)
    };
    match state {
        State::Starting => starting(event, ctx),
        State::Idle { queue } => idle(queue, event),
        State::Analyzing { queue } => analyzing(queue, event, ctx),
        s @ State::Resuming { .. } => resuming(s, event, ctx),
        s @ State::AnalysisWarning { .. } => analysis_warning(s, event, ctx),
        s @ State::PlaceholderQuestion { .. } => placeholder(s, event, ctx),
        s @ State::Positioning { .. } => positioning(s, event, ctx),
        s @ State::ChoosingBackend { .. } => choosing_backend(s, event, ctx),
        s @ State::Signing { .. } => signing(s, event, ctx),
        s @ State::Output { .. } => output(s, event, ctx),
        s @ State::Error { .. } => error(s, event, ctx),
        s @ State::Finished { .. } => Transition::stay(s),
        s @ State::BatchOverview { .. } => batch_run::overview(s, event, ctx),
        s @ State::BatchSigning { .. } => batch_run::signing(s, event, ctx),
    }
}

// ------------------------------------------------------------------------------------------------
// Gemeinsame Bausteine
// ------------------------------------------------------------------------------------------------

/// Leere Warteschlange: Dokumentauswahl bzw. (ohne Oberfläche) Programmende mit dem Exit-Code der
/// Ergebnisse.
fn idle_or_finish(queue: Queue, ctx: &Context, effects: Vec<Effect>) -> Transition {
    if ctx.headless {
        return finish(queue, effects);
    }
    Transition::with(State::Idle { queue }, effects)
}

/// Programmende mit dem Exit-Code der Ergebnisse.
fn finish(queue: Queue, mut effects: Vec<Effect>) -> Transition {
    let exit = queue.exit_code();
    effects.push(Effect::Exit(exit));
    Transition::with(State::Finished { exit, results: queue.done }, effects)
}

/// Programmende mit festem Exit-Code.
fn finish_with(exit: ExitCode, results: Vec<crate::queue::DocOutcome>, mut effects: Vec<Effect>) -> Transition {
    effects.push(Effect::Exit(exit));
    Transition::with(State::Finished { exit, results }, effects)
}

/// Aktuelles Dokument analysieren bzw. ohne Dokument auf Auswahl/Ende gehen.
fn start_current(queue: Queue, ctx: &Context, mut effects: Vec<Effect>) -> Transition {
    match queue.current_id() {
        Some(id) => {
            effects.push(Effect::Analyze(id));
            Transition::with(State::Analyzing { queue }, effects)
        }
        None => idle_or_finish(queue, ctx, effects),
    }
}

/// Aktuelles Dokument mit `result` abschließen und mit dem nächsten weitermachen (Batch, F-88).
fn next_doc(mut queue: Queue, result: DocResult, ctx: &Context, mut effects: Vec<Effect>) -> Transition {
    if let Some(id) = queue.current_id() {
        effects.push(Effect::Discard(id));
    }
    queue.finish_current(result);
    start_current(queue, ctx, effects)
}

/// Hauptleiste „Öffnen“ außerhalb des Ergebnis-Screens: Dokument und Warteschlange verwerfen (F-16).
fn discard_all(mut queue: Queue, mut effects: Vec<Effect>) -> Transition {
    effects.extend(queue.current.iter().chain(queue.pending.iter()).map(|d| Effect::Discard(d.id)));
    queue.clear();
    Transition::with(State::Idle { queue }, effects)
}

/// Fehler: mit Oberfläche als Fehlerzustand mit „Wiederholen“/„Abbrechen“, ohne Oberfläche wird das
/// Dokument als fehlgeschlagen verbucht und das nächste begonnen (kein Wiederholen ohne Nutzerin).
fn fail(
    from: State,
    error: WorkflowError,
    retry: Option<RetryTarget>,
    ctx: &Context,
    effects: Vec<Effect>,
) -> Transition {
    let Some(queue) = from.queue().cloned() else {
        // Fehler beim Start: ohne Oberfläche Ende, sonst Rückfrage (Werksreset)
        if ctx.headless {
            return finish_with(error.exit_code(), Vec::new(), effects);
        }
        return Transition::with(
            State::Error { queue: Queue::default(), error, retry, from: Some(Box::new(from)) },
            effects,
        );
    };
    if ctx.headless {
        return next_doc(queue, DocResult::Failed { error }, ctx, effects);
    }
    Transition::with(State::Error { queue, error, retry, from: Some(Box::new(from)) }, effects)
}

/// Positionierung bzw. – wenn das Profil unsichtbar ist, automatisch positioniert wird oder keine
/// Vorschau möglich ist – direkt die Backend-Auswahl.
fn after_position_question(queue: Queue, doc: DocFacts, ctx: &Context, effects: Vec<Effect>) -> Transition {
    if ctx.invisible {
        return to_backend_choice(queue, doc, PositionChoice::Invisible, Entry::Automatic, effects);
    }
    if ctx.auto_position || ctx.headless {
        return to_backend_choice(queue, doc, PositionChoice::Auto, Entry::Automatic, effects);
    }
    to_positioning(queue, doc, None, ctx, effects)
}

/// Positionierung (Startseite = letzte Seite, Startposition 50 %/25 %, F-32/F-33; vorherige
/// manuelle Position bleibt beim Rücksprung erhalten, F-43). Im Arbeitsbereich-Modus immer der
/// Arbeitsbereich (nie die automatische Backend-Wahl, A1).
fn to_positioning(
    queue: Queue,
    doc: DocFacts,
    previous: Option<Placement>,
    ctx: &Context,
    mut effects: Vec<Effect>,
) -> Transition {
    if ctx.workspace {
        let position = previous.map(PositionChoice::Manual).unwrap_or(PositionChoice::Auto);
        return restore_workspace(queue, doc, &position, None, ctx, effects);
    }
    let placement = previous.or(doc.start);
    let Some(placement) = placement else {
        // keine Vorschau möglich (F-44): Hinweis und automatische Positionierung
        effects.push(Effect::Notify(MSG_POSITIONING_NOT_POSSIBLE.into()));
        return to_backend_choice(queue, doc, PositionChoice::Auto, Entry::Automatic, effects);
    };
    let new_page = doc.page_count > 0 && placement.page_index >= doc.page_count;
    let page = placement.page_index.min(doc.page_count);
    if page < doc.page_count {
        effects.push(Effect::RenderPage { doc: doc.id, page });
    }
    effects.push(Effect::BuildBlockPreview(doc.id));
    Transition::with(State::Positioning { queue, page, placement: Some(placement), new_page, doc, edit: None }, effects)
}

/// Wie die Backend-Auswahl erreicht wird – steuert die automatische Wahl (F-53/F-54/F-55).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entry {
    /// Weg Öffnen → Signieren ohne „Signieren“ (automatische Position, unsichtbares Profil, Platzhalter):
    /// automatische Wahl nur für Signiermittel, bei denen das Dokument auf dem Gerät bleibt
    Automatic,
    /// nach „Signieren“ (Position bestätigt bzw. „Unsichtbar signieren“): automatische Wahl ohne
    /// Einschränkung
    SignRequested,
    /// nach Abbruch, Fehler oder Hauptleiste: immer fragen (F-55)
    Ask,
}

/// Backend-Auswahl; die Optionen kommen über [`Effect::ProbeBackends`].
fn to_backend_choice(
    queue: Queue,
    doc: DocFacts,
    position: PositionChoice,
    entry: Entry,
    effects: Vec<Effect>,
) -> Transition {
    to_backend_choice_with(queue, doc, position, entry, None, effects)
}

/// Wie [`to_backend_choice`] mit Bearbeitungszustand (Arbeitsbereich, Startargument-Automatik).
fn to_backend_choice_with(
    queue: Queue,
    doc: DocFacts,
    position: PositionChoice,
    entry: Entry,
    edit: Option<WorkspaceEdit>,
    mut effects: Vec<Effect>,
) -> Transition {
    effects.push(Effect::ProbeBackends);
    let auto_choice = entry != Entry::Ask;
    let sign_requested = entry == Entry::SignRequested;
    let state = State::ChoosingBackend { queue, position, options: Vec::new(), doc, auto_choice, sign_requested, edit };
    Transition::with(state, effects)
}

/// Backend-Auswahl ohne automatische Wahl (mit bekannten Optionen).
fn ask_backend(
    queue: Queue,
    doc: DocFacts,
    position: PositionChoice,
    options: Vec<BackendOption>,
    edit: Option<WorkspaceEdit>,
    effects: Vec<Effect>,
) -> Transition {
    let state =
        State::ChoosingBackend { queue, position, options, doc, auto_choice: false, sign_requested: false, edit };
    Transition::with(state, effects)
}

/// Ergebnis der automatischen Backend-Wahl.
enum AutoChoice {
    Choose(BackendId),
    Ask,
    Fail(WorkflowError),
}

/// F-53 (Standard-BKU überspringt die Auswahl), F-54 (ohne erreichbare lokale BKU und ohne **aktiven**
/// Keystore automatisch ID Austria); ohne Oberfläche zusätzlich: genau ein verfügbares Backend wird
/// genommen, sonst Fehler.
///
/// „Aktiv“ heißt wie in PDF-Over (`BKUSelectionState.java:68-73`, `KEYSTORE_ENABLED`) eingeschaltet –
/// auch wenn die Datei gerade fehlt (die Option steht dann nicht verfügbar in der Liste). Sonst startete
/// eine eingerichtete, aber verschwundene Keystore-Datei (z. B. nach einem App-Update mit neuem
/// Sandbox-Pfad) ohne Rückfrage eine ID-Austria-Sitzung bei A-Trust.
fn auto_choice(options: &[BackendOption], ctx: &Context) -> AutoChoice {
    let available = |id: BackendId| options.iter().any(|o| o.id == id && o.available());
    let enabled = |id: BackendId| options.iter().any(|o| o.id == id);
    if let Some(choice) = ctx.default_backend {
        let id = backend_for_choice(choice);
        if available(id) {
            return AutoChoice::Choose(id);
        }
        if ctx.headless {
            return AutoChoice::Fail(WorkflowError::Backend {
                code: MSG_DEFAULT_BACKEND_UNAVAILABLE.into(),
                detail: id.as_str().into(),
            });
        }
        return AutoChoice::Ask;
    }
    if !available(BackendId::LocalBku) && !enabled(BackendId::Keystore) && available(BackendId::AtrustNative) {
        return AutoChoice::Choose(BackendId::AtrustNative);
    }
    if !ctx.headless {
        return AutoChoice::Ask;
    }
    let avail: Vec<&BackendOption> = options.iter().filter(|o| o.available()).collect();
    match avail.as_slice() {
        [] => AutoChoice::Fail(WorkflowError::Backend { code: MSG_NO_BACKEND.into(), detail: String::new() }),
        [one] => AutoChoice::Choose(one.id),
        _ => AutoChoice::Fail(WorkflowError::Backend {
            code: MSG_BACKEND_CHOICE_REQUIRED.into(),
            detail: avail.iter().map(|o| o.id.as_str()).collect::<Vec<_>>().join(","),
        }),
    }
}

/// Verlässt das Dokument mit diesem Signiermittel das Gerät (`document_leaves_device`)?
fn leaves_device(options: &[BackendOption], id: BackendId) -> bool {
    options.iter().any(|o| o.id == id && o.capabilities.as_ref().is_some_and(|c| c.document_leaves_device))
}

/// Backend gewählt (ausdrücklich, automatisch nach „Signieren“ bzw. beim Fortsetzen): Signieren – ohne
/// Zustimmungsschritt (ADR 0003). Ohne Oberfläche erscheint bei `document_leaves_device` ein einzeiliger
/// Hinweis ([`MSG_DOCUMENT_LEAVES_DEVICE`]); die Oberfläche zeigt den Kurzhinweis selbst.
fn choose_backend(
    queue: Queue,
    doc: DocFacts,
    position: PositionChoice,
    options: Vec<BackendOption>,
    id: BackendId,
    edit: Option<WorkspaceEdit>,
    ctx: &Context,
    mut effects: Vec<Effect>,
) -> Transition {
    let caps = options.iter().find(|o| o.id == id).and_then(|o| o.capabilities.clone());
    let Some(caps) = caps else {
        // nicht verfügbar – Auswahl bleibt
        if ctx.workspace {
            effects.push(Effect::Notify(MSG_SIGNER_UNAVAILABLE.into()));
        }
        return ask_backend(queue, doc, position, options, edit, effects);
    };
    if caps.document_leaves_device && ctx.headless {
        effects.push(Effect::Notify(MSG_DOCUMENT_LEAVES_DEVICE.into()));
    }
    let flow = SignFlow::new(&caps, &position);
    start_signing(queue, doc, position, id, flow, edit, effects)
}

fn start_signing(
    queue: Queue,
    doc: DocFacts,
    position: PositionChoice,
    backend: BackendId,
    flow: SignFlow,
    edit: Option<WorkspaceEdit>,
    mut effects: Vec<Effect>,
) -> Transition {
    // Mehrfachsignatur: jeder Start (auch „Erneut versuchen“) beginnt wieder bei der ersten Runde – ein
    // Teilergebnis wird verworfen (`Effect::OpenSession` setzt die Quelle auf das Original zurück)
    let (position, edit) = match edit {
        Some(mut e) => {
            let first = e.rounds.as_mut().and_then(|r| {
                r.index = 0;
                r.positions.first().cloned()
            });
            (first.unwrap_or(position), Some(e))
        }
        None => (position, None),
    };
    effects.push(Effect::OpenSession(backend));
    Transition::with(
        State::Signing { queue, position, backend, phase: SigningPhase::OpeningSession, doc, flow, edit },
        effects,
    )
}

// ------------------------------------------------------------------------------------------------
// Arbeitsbereich (ADR 0004 §3.2–§3.6)
// ------------------------------------------------------------------------------------------------

/// Arbeitsbereich (`Positioning` mit Bearbeitungszustand). Ohne gemerkte Lage gilt die Startlage des
/// Dokuments (`DocFacts::start`, 50 %/25 %); ohne Vorschau `placement = None` mit Hinweis. `auto_place`
/// fordert die automatische Platzierung an (PDF-AS-Regel, Ergebnis [`Event::AutoPlaced`]). Die Sperre der
/// Einstellung „Automatische Positionierung“ folgt den aktuellen Einstellungen (auch nach Abbruch bzw.
/// Fehler-„Schließen“, wenn sie inzwischen geändert wurden; Nachtrag 8).
fn to_workspace(
    queue: Queue,
    doc: DocFacts,
    parked: Option<ParkedPlacement>,
    mut edit: WorkspaceEdit,
    auto_place: bool,
    ctx: &Context,
    mut effects: Vec<Effect>,
) -> Transition {
    edit.follow_auto_position(ctx);
    let n = doc.page_count;
    let (page, placement, new_page) = match parked {
        Some(p) => (p.page, p.placement, p.new_page),
        None => match doc.start {
            Some(p) => (p.page_index.min(n), Some(p), n > 0 && p.page_index >= n),
            None => (n.saturating_sub(1), None, false),
        },
    };
    // Mehrfachsignatur: liegt ein ruhender Block auf der virtuellen neuen Seite, gibt es sie
    let new_page = new_page || (n > 0 && edit.others.iter().any(|b| b.page >= n));
    edit.parked = None;
    edit.rounds = None;
    match placement {
        None => {
            if parked.is_none() {
                // ohne Vorschau (kein PDFium): bleibt bedienbar, signiert mit `PositionChoice::Auto`
                effects.push(Effect::Notify(MSG_POSITIONING_NOT_POSSIBLE.into()));
            }
        }
        Some(_) => {
            if page < n {
                effects.push(Effect::RenderPage { doc: doc.id, page });
            }
            effects.push(Effect::BuildBlockPreview(doc.id));
            if auto_place {
                effects.push(Effect::AutoPlace(doc.id));
            }
        }
    }
    Transition::with(State::Positioning { queue, page, placement, new_page, doc, edit: Some(edit) }, effects)
}

/// Zurück in den Arbeitsbereich mit unverändertem Bearbeitungszustand (Abbruch, Fehler-„Schließen“).
fn restore_workspace(
    queue: Queue,
    doc: DocFacts,
    position: &PositionChoice,
    edit: Option<WorkspaceEdit>,
    ctx: &Context,
    effects: Vec<Effect>,
) -> Transition {
    let n = doc.page_count;
    let mut edit = edit.unwrap_or_else(|| WorkspaceEdit::from_position(position, ctx.invisible));
    let parked = edit.parked.take().or_else(|| {
        position.manual().map(|p| ParkedPlacement {
            page: p.page_index.min(n),
            placement: Some(*p),
            new_page: n > 0 && p.page_index >= n,
        })
    });
    to_workspace(queue, doc, parked, edit, false, ctx, effects)
}

/// Nach der Analyse im Arbeitsbereich-Modus: keine Platzhalterfrage. Das automatische Ziel folgt den
/// Einstellungen › Dokument ([`WorkspaceEdit::after_analysis`]): mit Platzhaltersuche „QR-Code-Platzhalter“
/// der gefundene QR-Platzhalter, mit „Leere Signaturfelder“ das erste leere Signaturfeld, sonst frei mit
/// automatischer Platzierung. Leere Felder werden immer angeboten (ohne Suche nie automatisch, §8 F2). Mit
/// der Einstellung „Automatische Positionierung“ ist die Platzierung gesperrt (nicht für Einträge der
/// Stapelübersicht, Nachtrag 8). Mit der Startargument-Automatik der klassische automatische Weg (D-06) mit
/// dem Bearbeitungszustand für den Rückfall.
fn workspace_after_analysis(
    queue: Queue,
    doc: DocFacts,
    placeholder: Option<PlaceholderFound>,
    fields: Vec<EmptyFieldCandidate>,
    ctx: &Context,
) -> Transition {
    let qr = match placeholder {
        Some(PlaceholderFound::Qr(qr)) => Some(qr),
        _ => None,
    };
    let mut edit = WorkspaceEdit::after_analysis(fields, qr, ctx.invisible, ctx);
    // aus der Stapelübersicht geöffnet: Plan des Eintrags übernehmen, „Zurück zum Stapel“ (ADR 0004 §3.8);
    // ein automatischer Plan folgt wie hier den aktuellen Einstellungen
    if ctx.batch()
        && let Some((plan, automatic)) = batch_run::opened_plan(&queue, doc.id)
    {
        edit.return_to_batch = true;
        let parked = if automatic { None } else { batch_run::apply_plan(&mut edit, plan.as_ref(), &doc) };
        batch_run::apply_more(&mut edit, &queue, &doc);
        let auto = edit.auto_free() && !edit.invisible;
        return to_workspace(queue, doc, parked, edit, auto, ctx, Vec::new());
    }
    // Einstellung „Automatische Positionierung“: automatisch platziert und gesperrt, bereit zum Signieren –
    // signiert wird trotzdem erst nach „Signieren mit“ (A1)
    edit.follow_auto_position(ctx);
    if ctx.cli_auto {
        let position = if ctx.invisible {
            PositionChoice::Invisible
        } else {
            match edit.target {
                Target::Qr => edit.qr.clone().map(PositionChoice::Placeholder).unwrap_or(PositionChoice::Auto),
                Target::Field(i) => {
                    edit.fields.get(i).cloned().map(PositionChoice::Field).unwrap_or(PositionChoice::Auto)
                }
                Target::Free => PositionChoice::Auto,
            }
        };
        return to_backend_choice_with(queue, doc, position, Entry::Automatic, Some(edit), Vec::new());
    }
    let auto = edit.auto_free() && !edit.invisible;
    to_workspace(queue, doc, None, edit, auto, ctx, Vec::new())
}

/// Position zum Signieren aus dem Bearbeitungszustand (ADR 0004 §3.5 Schritt 1): unsichtbar → Invisible,
/// QR → Placeholder, Feld → Field, frei → Manual (auch bei `origin == Auto`, WYSIWYS), ohne Vorschau Auto.
fn workspace_position(
    edit: &WorkspaceEdit,
    placement: Option<Placement>,
    new_page: bool,
    doc: &DocFacts,
) -> Option<PositionChoice> {
    if edit.invisible {
        return Some(PositionChoice::Invisible);
    }
    match edit.target {
        Target::Qr => edit.qr.clone().map(PositionChoice::Placeholder),
        Target::Field(i) => edit.fields.get(i).cloned().map(PositionChoice::Field),
        Target::Free => Some(match placement {
            Some(mut p) => {
                // „Neue Seite“ nur, wenn die Lage auf der virtuellen Seite liegt
                if !new_page && p.page_index >= doc.page_count {
                    p.page_index = doc.page_count.saturating_sub(1);
                }
                PositionChoice::Manual(p)
            }
            None => PositionChoice::Auto,
        }),
    }
}

/// Position eines ruhenden Blocks (Mehrfachsignatur) wie [`workspace_position`]; `None`, wenn sein Ziel fehlt.
/// Ein Block auf der virtuellen neuen Seite (`new_page`) behält sie: seine Position hängt die Seite an bzw. – ab
/// der zweiten Signatur – liegt auf der dann vorhandenen Seite.
fn slot_position(edit: &WorkspaceEdit, slot: &BlockSlot, doc: &DocFacts, new_page: bool) -> Option<PositionChoice> {
    match slot.target {
        Target::Qr => edit.qr.clone().map(PositionChoice::Placeholder),
        Target::Field(i) => edit.fields.get(i).cloned().map(PositionChoice::Field),
        Target::Free => slot.placement.map(|mut p| {
            p.page_index = p.page_index.min(last_page(doc.page_count, new_page));
            PositionChoice::Manual(p)
        }),
    }
}

/// Letzte Seite, auf der ein Block liegen kann: mit „Auf neuer Seite“ die virtuelle neue Seite (`== Seitenzahl`).
fn last_page(page_count: usize, new_page: bool) -> usize {
    if new_page { page_count } else { page_count.saturating_sub(1) }
}

/// Mehrfachsignatur mit „Auf neuer Seite“: Nur die erste Signatur kann die Seite anhängen (danach ist das Dokument
/// signiert). Blöcke auf der neuen Seite stehen deshalb in der Reihenfolge zuerst – automatisch umnummeriert,
/// sonst bleibt die Reihenfolge. Ohne neue Seite rücken Blöcke von der virtuellen Seite auf die letzte vorhandene.
/// `active_page` = Seite des ausgewählten Blocks.
fn order_blocks(edit: &mut WorkspaceEdit, active_page: usize, new_page: bool, page_count: usize) {
    if edit.others.is_empty() {
        return;
    }
    let last = page_count.saturating_sub(1);
    if !new_page {
        for b in edit.others.iter_mut().filter(|b| b.page >= page_count) {
            b.page = last;
            b.placement = b.placement.map(|p| Placement { page_index: last, ..p });
        }
        return;
    }
    let on_new = |page: usize| page >= page_count;
    let at = edit.active_index();
    // (ausgewählt?, Block) in der bisherigen Reihenfolge
    let mut order: Vec<Option<BlockSlot>> = edit.others.iter().copied().map(Some).collect();
    order.insert(at, None);
    let key = |b: &Option<BlockSlot>| !on_new(b.map_or(active_page, |b| b.page));
    if order.windows(2).all(|w| key(&w[0]) <= key(&w[1])) {
        return;
    }
    order.sort_by_key(key); // stabil: innerhalb der Gruppen bleibt die Reihenfolge
    edit.active = order.iter().position(Option::is_none).unwrap_or(0);
    edit.others = order.into_iter().flatten().collect();
}

/// Positionen aller Blöcke in ihrer Reihenfolge (`active` = Position des ausgewählten Blocks); leer bei genau
/// einem Block bzw. unsichtbar. `None`, wenn einem Block das Ziel fehlt.
fn all_positions(
    edit: &WorkspaceEdit,
    active: &PositionChoice,
    doc: &DocFacts,
    new_page: bool,
) -> Option<Vec<PositionChoice>> {
    if edit.invisible || edit.others.is_empty() {
        return Some(Vec::new());
    }
    let at = edit.active_index();
    let mut all = Vec::with_capacity(edit.others.len() + 1);
    for (k, slot) in edit.others.iter().enumerate() {
        if k == at {
            all.push(active.clone());
        }
        all.push(slot_position(edit, slot, doc, new_page)?);
    }
    if at == edit.others.len() {
        all.push(active.clone());
    }
    Some(all)
}

/// Ausgewählten Block als ruhenden Block ablegen (auf der virtuellen neuen Seite nur mit „Auf neuer Seite“).
fn park_block(
    edit: &WorkspaceEdit,
    page: usize,
    placement: Option<Placement>,
    doc: &DocFacts,
    new_page: bool,
) -> BlockSlot {
    let last = last_page(doc.page_count, new_page);
    BlockSlot {
        target: edit.target,
        origin: edit.origin,
        page: page.min(last),
        placement: placement.map(|p| Placement { page_index: p.page_index.min(last), ..p }),
    }
}

/// Seite eines Ziels (Feld bzw. QR-Platzhalter).
fn target_page(edit: &WorkspaceEdit, target: Target) -> Option<usize> {
    match target {
        Target::Free => None,
        Target::Qr => edit.qr.as_ref().map(|q| q.page_index),
        Target::Field(i) => edit.fields.get(i).and_then(|f| f.page_index),
    }
}

/// Herkunft zur Zielart.
fn origin_of(target: Target) -> PlacementOrigin {
    match target {
        Target::Free => PlacementOrigin::User,
        Target::Field(_) => PlacementOrigin::Field,
        Target::Qr => PlacementOrigin::Qr,
    }
}

/// Mehrfachsignatur: nächste Runde beginnen (Position der nächsten Signatur) bzw. `None` nach der letzten.
fn next_round(edit: &mut Option<WorkspaceEdit>) -> Option<PositionChoice> {
    let rounds = edit.as_mut()?.rounds.as_mut()?;
    let next = rounds.positions.get(rounds.index + 1)?.clone();
    rounds.index += 1;
    Some(next)
}

/// Fehler im Signieren (Arbeitsbereich): bei wiederholbaren Fehlern bleibt die Sitzung offen
/// (`ReleaseDocument` statt `CloseSession`, ADR 0004 §3.6).
fn keep_session(ctx: &Context, retry: bool, scope: Option<ErrorScope>) -> Effect {
    let keep = ctx.workspace
        && retry
        && match scope {
            Some(ErrorScope::Transient | ErrorScope::Document) | None => true,
            Some(_) => false,
        };
    if keep { Effect::ReleaseDocument } else { Effect::CloseSession }
}

// ------------------------------------------------------------------------------------------------
// Globale Ereignisse
// ------------------------------------------------------------------------------------------------

fn quit(state: State) -> Transition {
    match state {
        s @ State::Finished { .. } => Transition::stay(s),
        State::Signing { mut queue, .. } | State::BatchSigning { mut queue, .. } => {
            queue.clear();
            finish_with(ExitCode::Cancelled, queue.done, vec![Effect::CancelSession, Effect::DeletePersisted])
        }
        State::Resuming { mut queue, .. } => {
            queue.clear();
            finish_with(ExitCode::Cancelled, queue.done, vec![Effect::DeletePersisted])
        }
        State::Output { mut queue, saved: Some(output), .. } => {
            // das aktuelle Dokument ist gespeichert; nur noch wartende gelten als abgebrochen
            queue.finish_current(DocResult::Signed { output });
            if queue.is_empty() {
                return finish(queue, Vec::new());
            }
            queue.clear();
            finish_with(ExitCode::Cancelled, queue.done, Vec::new())
        }
        State::Idle { queue } if queue.is_empty() => finish(queue, Vec::new()),
        State::Starting => finish_with(ExitCode::Success, Vec::new(), Vec::new()),
        mut other => {
            // eine für „Erneut versuchen“ offen gehaltene Sitzung (Arbeitsbereich) wird verworfen (F-72)
            let held = matches!(&other, State::Error { from: Some(f), .. } if matches!(**f, State::Signing { .. }));
            let results = match other.queue_mut() {
                Some(q) => {
                    q.clear();
                    std::mem::take(&mut q.done)
                }
                None => Vec::new(),
            };
            let effects = if held { vec![Effect::CancelSession] } else { Vec::new() };
            finish_with(ExitCode::Cancelled, results, effects)
        }
    }
}

/// Dateien hinzugefügt (Dialog, Drop, „Öffnen mit“): im Auswahl-Screen sofort öffnen, im
/// Ergebnis-Screen anhängen und – wenn zuletzt gespeichert – sofort weiter (F-89), sonst nur an die
/// Warteschlange anhängen (Single-Instance).
fn files_added(state: State, docs: Vec<crate::queue::InputDocument>, ctx: &Context) -> Transition {
    if ctx.batch() {
        return batch_run::files_added(state, docs, ctx);
    }
    files_added_single(state, docs, ctx)
}

/// [`files_added`] ohne Stapelübersicht (auch Rückfall der Stapelregeln für Zustände ohne Stapel).
fn files_added_single(state: State, docs: Vec<crate::queue::InputDocument>, ctx: &Context) -> Transition {
    match state {
        State::Idle { mut queue } => {
            let mut effects = Vec::new();
            if let Some(dir) = docs.first().and_then(|d| d.path.parent()).filter(|p| !p.as_os_str().is_empty()) {
                effects.push(Effect::RememberDirectory(dir.to_path_buf()));
            }
            queue.add(docs);
            match queue.current_id() {
                Some(id) => {
                    effects.push(Effect::Analyze(id));
                    Transition::with(State::Analyzing { queue }, effects)
                }
                None => Transition::with(State::Idle { queue }, effects),
            }
        }
        State::Output { mut queue, saved: Some(output), decision, doc, position, backend, edit } => {
            queue.add(docs);
            if queue.pending.is_empty() {
                return Transition::stay(State::Output {
                    queue,
                    saved: Some(output),
                    decision,
                    doc,
                    position,
                    backend,
                    edit,
                });
            }
            // F-89: zuletzt erfolgreich gespeichert → sofort weiter
            next_doc(queue, DocResult::Signed { output }, ctx, Vec::new())
        }
        s @ (State::Starting | State::Finished { .. }) => Transition::stay(s),
        mut s => {
            // Arbeitsbereich: Dateien aus dem Fehlerschritt des Signierens verwerfen die für „Erneut
            // versuchen“ offen gehaltene Sitzung (ADR 0004 §3.6, F-72)
            let held = ctx.workspace
                && matches!(&s, State::Error { from: Some(f), .. } if matches!(**f, State::Signing { .. }));
            if let Some(q) = s.queue_mut() {
                q.add(docs);
            }
            let effects = if held { vec![Effect::CancelSession] } else { Vec::new() };
            Transition::with(s, effects)
        }
    }
}

/// „Weitere PDFs hinzufügen“ (Kopfzeile „+ PDF“, Miniaturleiste; ADR 0004 Nachtrag 9): mit der
/// Stapelübersicht werden das offene Dokument und die neuen Dateien zusammen zum Stapel (Modul `batch_run`),
/// sonst wie [`files_added`].
fn more_files_added(state: State, docs: Vec<crate::queue::InputDocument>, ctx: &Context) -> Transition {
    if ctx.batch() {
        return batch_run::more_files_added(state, docs, ctx);
    }
    files_added(state, docs, ctx)
}

/// „Anderes PDF öffnen“ (ADR 0004 Nachtrag 9): das offene Dokument der Analysewarnung bzw. des
/// Arbeitsbereichs wird geschlossen – verworfen ([`Effect::Discard`]), nie signiert; in der Ergebnisliste
/// „abgebrochen“ wie nach „Start“, ein Stapeleintrag wird entfernt –, danach öffnen die Dateien wie in der
/// Dokumentauswahl ([`files_added`] in `Idle`: ein Dokument → Arbeitsbereich, mit Stapelübersicht ab zwei
/// offenen Dokumenten die Übersicht). Wartende Dokumente bleiben. Aus der Stapelübersicht geöffnet und mit
/// weiteren Einträgen: zurück zur Übersicht mit den neuen Einträgen. In allen anderen Zuständen (und ohne
/// Dateien) wie [`files_added`]. Startet nichts (A1).
fn document_replaced(state: State, docs: Vec<crate::queue::InputDocument>, ctx: &Context) -> Transition {
    if docs.is_empty() {
        return files_added(state, docs, ctx);
    }
    let (mut queue, id) = match state {
        State::AnalysisWarning { queue, doc, .. } | State::Positioning { queue, doc, .. } => (queue, doc.id),
        s => return files_added(s, docs, ctx),
    };
    let mut effects = vec![Effect::Discard(id)];
    // Stapeleintrag: entfernen wie „Entfernen“ in der Übersicht; sonst wie „Start“ als abgebrochen verbuchen
    if let Some(batch) = queue.batch.as_mut().filter(|b| b.item(id).is_some()) {
        batch.items.retain(|i| i.doc != id);
        queue.remove(id);
    } else {
        queue.complete(id, DocResult::Cancelled);
    }
    if let Some(batch) = queue.batch.as_mut() {
        batch.open = None;
    }
    if queue.batch.as_ref().is_some_and(|b| b.items.is_empty()) {
        queue.batch = None;
    }
    if ctx.batch() && queue.batch.is_some() {
        // aus der Übersicht geöffnet, weitere Einträge: die Dateien kommen in den Stapel, zurück zur Übersicht
        queue.append(docs);
        return batch_run::new_overview(queue, effects);
    }
    let t = files_added(State::Idle { queue }, docs, ctx);
    effects.extend(t.effects);
    Transition::with(t.state, effects)
}

/// Einstellungen geändert: Blockvorschau neu (Positionierung), Backends neu prüfen (Auswahl). Im
/// Arbeitsbereich folgt ein automatisch platziertes Dokument den Einstellungen › Dokument (ADR 0004 §3.11
/// „sofort wirksam“): QR-Platzhalter bzw. erstes leeres Signaturfeld bzw. wieder frei mit automatischer
/// Platzierung; ein von Hand platziertes bleibt. „Automatische Positionierung“ sperrt bzw. entsperrt die
/// Platzierung des aktuellen Dokuments ([`WorkspaceEdit::follow_auto_position`], Nachtrag 8).
fn settings_changed(state: State, ctx: &Context) -> Transition {
    let mut effects = match &state {
        State::Positioning { doc, .. } => vec![Effect::BuildBlockPreview(doc.id)],
        State::ChoosingBackend { .. } => vec![Effect::ProbeBackends],
        _ => Vec::new(),
    };
    // Arbeitsbereich: unsichtbares Profil erzwingt „unsichtbar“ (zurück auf ein sichtbares Profil wird der
    // Block wieder sichtbar, außer die Nutzerin hat „unsichtbar“ selbst gewählt); Platzhaltersuche
    let mut state = match state {
        State::Positioning { queue, page, placement, new_page, doc, edit: Some(mut edit) } => {
            let visible_again = edit.follow_profile(ctx);
            let follow = edit.follow_settings(ctx);
            edit.follow_auto_position(ctx);
            if (follow.auto_place || (visible_again && edit.auto_free())) && placement.is_some() {
                effects.push(Effect::AutoPlace(doc.id));
            }
            if follow.search_qr {
                effects.push(Effect::SearchPlaceholder(doc.id));
            }
            let (page, placement, new_page) = settle_on_target(&edit, page, placement, new_page, &doc, &mut effects);
            State::Positioning { queue, page, placement, new_page, doc, edit: Some(edit) }
        }
        other => other,
    };
    // Stapel: automatische Pläne der Einträge folgen den Einstellungen (ADR 0004 §3.8, §3.11)
    if ctx.batch() {
        effects.extend(batch_run::settings_changed(&mut state, ctx));
    }
    Transition::with(state, effects)
}

// ------------------------------------------------------------------------------------------------
// Zustände
// ------------------------------------------------------------------------------------------------

fn starting(event: Event, ctx: &Context) -> Transition {
    match event {
        Event::Started { documents } => {
            let mut queue = Queue::default();
            if ctx.batch() && documents.len() >= 2 {
                // „Öffnen mit“ mehrerer Dateien beim Start: Stapelübersicht (ADR 0004 §3.8)
                queue.append(documents);
                if queue.pending.len() >= 2 {
                    return batch_run::new_overview(queue, Vec::new());
                }
                queue.advance();
                return start_current(queue, ctx, Vec::new());
            }
            queue.add(documents);
            start_current(queue, ctx, Vec::new())
        }
        Event::SettingsUnreadable(detail) => {
            fail(State::Starting, WorkflowError::Config(detail), Some(RetryTarget::ResetSettings), ctx, Vec::new())
        }
        _ => Transition::stay(State::Starting),
    }
}

fn idle(mut queue: Queue, event: Event) -> Transition {
    // „Öffnen“ im Auswahl-Screen öffnet den Dateidialog der Shell (kein Zustandswechsel).
    match event {
        Event::ResumeRequested { document, plan, pending } => {
            // Auswahl-Screen: kein aktuelles Dokument; das fortgesetzte kommt vor die damals wartenden
            queue.add(std::iter::once(document).chain(pending));
            match queue.current_id() {
                Some(id) => Transition::with(State::Resuming { queue, plan, doc: None }, vec![Effect::Analyze(id)]),
                None => Transition::with(State::Idle { queue }, vec![Effect::DeletePersisted]),
            }
        }
        _ => Transition::stay(State::Idle { queue }),
    }
}

/// „Signatur fortsetzen“: Dokument laden (ohne Rückfragen zur Analyse, ohne Platzhalterfrage und
/// Positionierung – die Position steht im vorbereiteten Dokument fest), dann das gespeicherte
/// Signiermittel. Abbruch, Fehler oder ein nicht mehr verfügbares Signiermittel verwerfen den
/// persistierten Zustand.
fn resuming(state: State, event: Event, ctx: &Context) -> Transition {
    let State::Resuming { queue, plan, doc } = state else { unreachable!("Resuming") };
    let current = queue.current_id();
    match (event, doc) {
        (Event::Analyzed { doc: id, class, signed, page_count, start, new_visible_field_allowed, .. }, None)
            if Some(id) == current =>
        {
            if matches!(class, AcceptanceClass::K5Rejected | AcceptanceClass::K2Repairable) {
                let error = WorkflowError::Rejected { class, code: "ng-document-rejected".into() };
                let from = State::Resuming { queue, plan, doc: None };
                return fail(from, error, None, ctx, vec![Effect::DeletePersisted]);
            }
            let facts = DocFacts { id, page_count, signed, class, start, new_visible_field_allowed };
            Transition::with(State::Resuming { queue, plan, doc: Some(facts) }, vec![Effect::ProbeBackends])
        }
        (Event::AnalysisFailed { doc: id, error }, None) if Some(id) == current => {
            fail(State::Resuming { queue, plan, doc: None }, error, None, ctx, vec![Effect::DeletePersisted])
        }
        (Event::BackendsProbed(options), Some(doc)) => {
            let ResumePlan { position, backend } = plan;
            // Stapel unterbrochen (Mobile, App-Kill): nur dieses Dokument wird fortgesetzt, danach die
            // Stapelübersicht mit den übrigen (ADR 0004 §3.8)
            let edit = (ctx.batch() && !queue.pending.is_empty()).then(|| {
                let mut e = WorkspaceEdit::from_position(&position, ctx.invisible);
                e.return_to_batch = true;
                e
            });
            if options.iter().any(|o| o.id == backend && o.available()) {
                return choose_backend(queue, doc, position, options, backend, edit, ctx, Vec::new());
            }
            // Signiermittel nicht mehr verfügbar (Richtlinie, Keystore entfernt): Auswahl, neu vorbereiten
            let effects = vec![Effect::Notify(MSG_RESUME_BACKEND_UNAVAILABLE.into()), Effect::DeletePersisted];
            ask_backend(queue, doc, position, options, edit, effects)
        }
        (Event::Cancel, _) => next_doc(queue, DocResult::Cancelled, ctx, vec![Effect::DeletePersisted]),
        (Event::Navigate(Step::Open), _) => discard_all(queue, vec![Effect::DeletePersisted]),
        (_, doc) => Transition::stay(State::Resuming { queue, plan, doc }),
    }
}

fn analyzing(queue: Queue, event: Event, ctx: &Context) -> Transition {
    let current = queue.current_id();
    match event {
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
        } if Some(doc) == current => {
            if matches!(class, AcceptanceClass::K5Rejected | AcceptanceClass::K2Repairable) {
                let error = WorkflowError::Rejected { class, code: "ng-document-rejected".into() };
                return fail(State::Analyzing { queue }, error, None, ctx, Vec::new());
            }
            let facts = DocFacts { id: doc, page_count, signed, class, start, new_visible_field_allowed };
            // Warnung mit Abbruchmöglichkeit (Plan §6.1); ohne Oberfläche nur Hinweis (core-api), weiter. Ein
            // Stapeleintrag, dessen Warnungen schon bestätigt wurden, fragt nicht erneut (ADR 0004 §3.8)
            if !warnings.is_empty() && !ctx.headless && !batch_run::reviewed(&queue, doc) {
                return Transition::stay(State::AnalysisWarning { queue, warnings, doc: facts, placeholder, fields });
            }
            after_analysis(queue, facts, placeholder, fields, ctx)
        }
        Event::AnalysisFailed { doc, error } if Some(doc) == current => {
            let retry = match &error {
                WorkflowError::Rejected {
                    class: AcceptanceClass::K5Rejected | AcceptanceClass::K2Repairable, ..
                } => None,
                _ => Some(RetryTarget::Load),
            };
            fail(State::Analyzing { queue }, error, retry, ctx, Vec::new())
        }
        Event::Navigate(Step::Open) => discard_all(queue, Vec::new()),
        Event::Cancel => next_doc(queue, DocResult::Cancelled, ctx, Vec::new()),
        _ => Transition::stay(State::Analyzing { queue }),
    }
}

/// Nach der Analyse: Platzhalterfrage (F-20/F-21), sonst Positionierung bzw. Auswahl; im
/// Arbeitsbereich-Modus [`workspace_after_analysis`].
fn after_analysis(
    queue: Queue,
    doc: DocFacts,
    placeholder: Option<PlaceholderFound>,
    fields: Vec<EmptyFieldCandidate>,
    ctx: &Context,
) -> Transition {
    if ctx.workspace && !ctx.headless {
        return workspace_after_analysis(queue, doc, placeholder, fields, ctx);
    }
    let found = placeholder.filter(|p| ctx.placeholder_search && !p.is_empty() && !ctx.invisible);
    match found {
        Some(found) if ctx.headless => {
            // ohne Oberfläche: QR-Platzhalter bzw. genau ein Feld verwenden, sonst automatisch
            match found {
                PlaceholderFound::Qr(qr) => {
                    to_backend_choice(queue, doc, PositionChoice::Placeholder(qr), Entry::Automatic, Vec::new())
                }
                PlaceholderFound::Fields(mut fields) if fields.len() == 1 => {
                    let field = fields.remove(0);
                    to_backend_choice(queue, doc, PositionChoice::Field(field), Entry::Automatic, Vec::new())
                }
                PlaceholderFound::Fields(_) => after_position_question(queue, doc, ctx, Vec::new()),
            }
        }
        Some(found) => Transition::stay(State::PlaceholderQuestion { queue, found, doc }),
        None => after_position_question(queue, doc, ctx, Vec::new()),
    }
}

fn analysis_warning(state: State, event: Event, ctx: &Context) -> Transition {
    let State::AnalysisWarning { queue, warnings, doc, placeholder, fields } = state else {
        unreachable!("AnalysisWarning")
    };
    match event {
        Event::WarningAnswered(true) => after_analysis(queue, doc, placeholder, fields, ctx),
        Event::WarningAnswered(false) | Event::Cancel => next_doc(queue, DocResult::Cancelled, ctx, Vec::new()),
        Event::Navigate(Step::Open) => discard_all(queue, Vec::new()),
        _ => Transition::stay(State::AnalysisWarning { queue, warnings, doc, placeholder, fields }),
    }
}

fn placeholder(state: State, event: Event, ctx: &Context) -> Transition {
    let State::PlaceholderQuestion { queue, found, doc } = state else { unreachable!("PlaceholderQuestion") };
    match event {
        Event::PlaceholderAnswered { answer: PlaceholderAnswer::Yes, field_index } => match found {
            PlaceholderFound::Qr(qr) => {
                to_backend_choice(queue, doc, PositionChoice::Placeholder(qr), Entry::Automatic, Vec::new())
            }
            PlaceholderFound::Fields(fields) => {
                let index = match (fields.len(), field_index) {
                    (1, None) => Some(0),
                    (n, Some(i)) if i < n => Some(i),
                    _ => None,
                };
                match index {
                    // Feldwahl gilt nur für diese Signatur (B-05), keine globalen Flags (B-04)
                    Some(i) => {
                        let field = fields[i].clone();
                        to_backend_choice(queue, doc, PositionChoice::Field(field), Entry::Automatic, Vec::new())
                    }
                    // Feldauswahl abgebrochen bzw. ungültig → erneut fragen (OpenState.java:165-171)
                    None => Transition::stay(State::PlaceholderQuestion {
                        queue,
                        found: PlaceholderFound::Fields(fields),
                        doc,
                    }),
                }
            }
        },
        Event::PlaceholderAnswered { answer: PlaceholderAnswer::No, .. } => {
            after_position_question(queue, doc, ctx, Vec::new())
        }
        Event::PlaceholderAnswered { answer: PlaceholderAnswer::Cancel, .. } | Event::Cancel => {
            // Dokument verworfen; B-06 behoben: es geht mit dem nächsten Dokument bzw. der Auswahl weiter
            next_doc(queue, DocResult::Cancelled, ctx, Vec::new())
        }
        Event::Navigate(Step::Open) => discard_all(queue, Vec::new()),
        _ => Transition::stay(State::PlaceholderQuestion { queue, found, doc }),
    }
}

/// Lage auf die Seiten des Dokuments übernehmen (wie `PlacementChanged`): Seite außerhalb bzw. nicht
/// endlich → `None`; „neue Seite“ auf einem signierten Dokument → dieselben Koordinaten auf der letzten
/// Seite mit Hinweis. Liefert (Seite, Lage, neue Seite, Effekte).
fn accept_placement(
    p: Placement,
    page: usize,
    new_page: bool,
    doc: &DocFacts,
) -> Option<(usize, Placement, bool, Vec<Effect>)> {
    let n = doc.page_count;
    if p.page_index > n || !(p.x.is_finite() && p.y.is_finite()) {
        return None;
    }
    if p.page_index == n && !doc.new_page_allowed() {
        // wie PDF-Over/PDF-AS (docs/analyse/04 §4.5): auf signierten Dokumenten keine neue Seite,
        // sondern dieselben Koordinaten auf der letzten Seite – mit Hinweis statt stillschweigend
        let last = n.saturating_sub(1);
        let moved = Placement { page_index: last, ..p };
        let mut effects = vec![Effect::Notify(MSG_NEW_PAGE_NOT_ALLOWED.into())];
        if page != last {
            effects.push(Effect::RenderPage { doc: doc.id, page: last });
            effects.push(Effect::BuildBlockPreview(doc.id));
        }
        return Some((last, moved, false, effects));
    }
    let new_page = new_page || p.page_index == n;
    Some((p.page_index, p, new_page, Vec::new()))
}

/// Ziel Feld bzw. QR-Platzhalter (Automatik nach den Einstellungen › Dokument oder Wahl der Nutzerin): Der
/// Block liegt dann im Ziel auf einer vorhandenen Seite – „Auf neuer Seite“ entfällt, und die Ansicht verlässt
/// die virtuelle neue Seite zur Seite des Ziels (Mobile-Check 16.10.2026: `qr.pdf` mit ausgeschalteter Suche auf
/// neuer Seite, Suche eingeschaltet → im QR-Platzhalter, aber weiter „Seite 1 / 2“ mit leerer neuer Seite). Frei
/// bleibt alles, wie es ist (die automatische Platzierung bestimmt die Seite selbst). Eine freie Lage auf der
/// virtuellen Seite rückt auf die letzte vorhandene (sie gilt erst wieder nach „Frei platzieren“ bzw. der
/// Automatik, die sie neu berechnen). Liefert (Seite, Lage, neue Seite).
fn settle_on_target(
    edit: &WorkspaceEdit,
    page: usize,
    placement: Option<Placement>,
    new_page: bool,
    doc: &DocFacts,
    effects: &mut Vec<Effect>,
) -> (usize, Option<Placement>, bool) {
    let target_page = match edit.target {
        Target::Free => return (page, placement, new_page),
        Target::Qr => edit.qr.as_ref().map(|q| q.page_index),
        Target::Field(i) => edit.fields.get(i).and_then(|f| f.page_index),
    };
    let n = doc.page_count;
    if n == 0 {
        return (page, placement, false);
    }
    let placement = placement.map(|p| Placement { page_index: p.page_index.min(n - 1), ..p });
    if page < n {
        return (page, placement, false);
    }
    let target = target_page.filter(|p| *p < n).unwrap_or(n - 1);
    effects.push(Effect::RenderPage { doc: doc.id, page: target });
    effects.push(Effect::BuildBlockPreview(doc.id));
    (target, placement, false)
}

fn positioning(state: State, event: Event, ctx: &Context) -> Transition {
    let State::Positioning { queue, page, placement, new_page, doc, edit } = state else { unreachable!("Positioning") };
    let n = doc.page_count;
    let stay = |queue, page, placement: Option<Placement>, new_page, doc, edit: Option<WorkspaceEdit>, effects| {
        // Mehrfachsignatur: Blöcke auf der neuen Seite zuerst (nur die erste Signatur kann die Seite anhängen)
        let edit = edit.map(|mut e| {
            order_blocks(&mut e, placement.map_or(page, |p| p.page_index), new_page, n);
            e
        });
        Transition::with(State::Positioning { queue, page, placement, new_page, doc, edit }, effects)
    };
    // Bearbeitung durch die Nutzerin (Ziehen, Tastatur, Seite, neue Seite): frei, Herkunft „User“, nicht
    // mehr aus der Automatik (folgt den Einstellungen › Dokument nicht mehr)
    let by_user = |edit: Option<WorkspaceEdit>| {
        edit.map(|mut e| {
            e.target = Target::Free;
            e.origin = PlacementOrigin::User;
            e.automatic = false;
            e
        })
    };
    // Einstellung „Automatische Positionierung“ (Nachtrag 8): solange gesperrt, verschiebt nichts den Block –
    // weder Ziehen/Tastatur noch Seitenwechsel noch eine Zielwahl („Ins Feld setzen“, „Frei platzieren“,
    // „Neu platzieren“); „Unsichtbar“, „Auf neuer Seite“ und „Signieren mit“ wirken weiter
    let locked = edit.as_ref().is_some_and(WorkspaceEdit::placement_locked);
    match event {
        Event::PlacementChanged(_)
        | Event::PageChanged(_)
        | Event::TargetChosen(_)
        | Event::BlockAdded(_)
        | Event::BlockSelected(_)
        | Event::BlockRemoved(_)
        | Event::TargetsFilled(_)
            if locked =>
        {
            stay(queue, page, placement, new_page, doc, edit, Vec::new())
        }
        // --- Mehrfachsignatur: je Block eine eigene Signatur ---------------------------------------
        Event::BlockAdded(p) => {
            let Some(mut e) = edit else { return stay(queue, page, placement, new_page, doc, None, Vec::new()) };
            let usable = placement.is_some() && !e.invisible && doc.new_visible_field_allowed;
            // auf der virtuellen neuen Seite nur, wenn es sie gibt („Auf neuer Seite“)
            let outside = p.page_index > n || (p.page_index == n && !new_page);
            if !usable || outside || !(p.x.is_finite() && p.y.is_finite()) {
                return stay(queue, page, placement, new_page, doc, Some(e), Vec::new());
            }
            if e.others.len() + 1 >= MAX_BLOCKS {
                let effects = vec![Effect::Notify(MSG_BLOCKS_LIMIT.into())];
                return stay(queue, page, placement, new_page, doc, Some(e), effects);
            }
            let slot = park_block(&e, page, placement, &doc, new_page);
            let at = e.active_index();
            e.others.insert(at, slot);
            e.active = e.others.len();
            e.target = Target::Free;
            e.origin = PlacementOrigin::User;
            e.automatic = false;
            let mut effects = Vec::new();
            if p.page_index != page && p.page_index < n {
                effects.push(Effect::RenderPage { doc: doc.id, page: p.page_index });
            }
            effects.push(Effect::BuildBlockPreview(doc.id));
            stay(queue, p.page_index, Some(p), new_page, doc, Some(e), effects)
        }
        Event::BlockSelected(i) => {
            let Some(mut e) = edit else { return stay(queue, page, placement, new_page, doc, None, Vec::new()) };
            let at = e.active_index();
            if i > e.others.len() || i == at {
                return stay(queue, page, placement, new_page, doc, Some(e), Vec::new());
            }
            let slot = park_block(&e, page, placement, &doc, new_page);
            let next = e.others.remove(if i < at { i } else { i - 1 });
            e.others.insert(if at < i { at } else { at - 1 }, slot);
            e.active = i;
            e.target = next.target;
            e.origin = next.origin;
            let mut effects = Vec::new();
            if next.page != page && next.page < n {
                effects.push(Effect::RenderPage { doc: doc.id, page: next.page });
            }
            effects.push(Effect::BuildBlockPreview(doc.id));
            stay(queue, next.page, next.placement, new_page, doc, Some(e), effects)
        }
        Event::BlockRemoved(i) => {
            let Some(mut e) = edit else { return stay(queue, page, placement, new_page, doc, None, Vec::new()) };
            let at = e.active_index();
            if e.others.is_empty() || i > e.others.len() {
                return stay(queue, page, placement, new_page, doc, Some(e), Vec::new());
            }
            if i != at {
                e.others.remove(if i < at { i } else { i - 1 });
                e.active = if i < at { at - 1 } else { at };
                return stay(queue, page, placement, new_page, doc, Some(e), Vec::new());
            }
            // der ausgewählte Block: der vorige (sonst der nächste) wird ausgewählt
            let j = at.saturating_sub(1);
            let next = e.others.remove(j);
            e.active = j;
            e.target = next.target;
            e.origin = next.origin;
            let mut effects = Vec::new();
            if next.page != page && next.page < n {
                effects.push(Effect::RenderPage { doc: doc.id, page: next.page });
            }
            effects.push(Effect::BuildBlockPreview(doc.id));
            stay(queue, next.page, next.placement, new_page, doc, Some(e), effects)
        }
        Event::TargetsFilled(targets) => {
            let Some(mut e) = edit else { return stay(queue, page, placement, new_page, doc, None, Vec::new()) };
            if e.invisible || placement.is_none() {
                return stay(queue, page, placement, new_page, doc, Some(e), Vec::new());
            }
            let mut free: Vec<Target> = Vec::new();
            for t in targets {
                let known = match t {
                    Target::Free => false,
                    Target::Field(i) => i < e.fields.len(),
                    Target::Qr => e.qr.is_some(),
                };
                if known && t != e.target && !e.target_taken(t) && !free.contains(&t) {
                    free.push(t);
                }
            }
            if free.is_empty() {
                return stay(queue, page, placement, new_page, doc, Some(e), Vec::new());
            }
            let mut effects = Vec::new();
            let (mut page, mut placement, mut new_page) = (page, placement, new_page);
            let mut rest = free.into_iter();
            if e.target == Target::Free {
                // der frei platzierte ausgewählte Block wandert in das erste Ziel
                if let Some(t) = rest.next() {
                    e.target = t;
                    e.origin = origin_of(t);
                    (page, placement, new_page) = settle_on_target(&e, page, placement, new_page, &doc, &mut effects);
                }
            }
            let last = n.saturating_sub(1);
            let mut limited = false;
            for t in rest {
                if e.others.len() + 1 >= MAX_BLOCKS {
                    limited = true;
                    break;
                }
                let at = target_page(&e, t).unwrap_or(page).min(last);
                let memory = placement.map(|p| Placement { page_index: at, ..p });
                e.others.push(BlockSlot { target: t, origin: origin_of(t), page: at, placement: memory });
            }
            e.automatic = false;
            if limited {
                effects.push(Effect::Notify(MSG_BLOCKS_LIMIT.into()));
            }
            stay(queue, page, placement, new_page, doc, Some(e), effects)
        }
        Event::PlacementUnlocked => {
            // „Diesmal selbst platzieren“: nur dieses Dokument, Lage unverändert
            let edit = edit.map(|mut e| {
                if e.placement_locked() {
                    e.lock = PlacementLock::Released;
                }
                e
            });
            stay(queue, page, placement, new_page, doc, edit, Vec::new())
        }
        Event::PlacementChanged(p) => match accept_placement(p, page, new_page, &doc) {
            Some((page, p, new_page, effects)) => stay(queue, page, Some(p), new_page, doc, by_user(edit), effects),
            None => stay(queue, page, placement, new_page, doc, edit, Vec::new()),
        },
        Event::AutoPlaced(p) => {
            // nur, solange die automatische Lage maßgeblich ist (inzwischen verschoben oder ein Feld
            // gewählt: verspätetes Ergebnis verwerfen)
            let applies = edit.as_ref().is_none_or(WorkspaceEdit::auto_free);
            match accept_placement(p, page, new_page && edit.is_none(), &doc).filter(|_| applies) {
                Some((target_page, p, new_page, mut effects)) => {
                    if edit.is_some() && target_page != page {
                        if target_page < n {
                            effects.push(Effect::RenderPage { doc: doc.id, page: target_page });
                        }
                        effects.push(Effect::BuildBlockPreview(doc.id));
                    }
                    stay(queue, target_page, Some(p), new_page, doc, edit, effects)
                }
                None => stay(queue, page, placement, new_page, doc, edit, Vec::new()),
            }
        }
        Event::BlockResized(p) => {
            let Some(e) = edit else {
                // klassischer Ablauf: wie bisher eine Lageänderung
                return positioning(
                    State::Positioning { queue, page, placement, new_page, doc, edit: None },
                    Event::PlacementChanged(p),
                    ctx,
                );
            };
            let auto = e.auto_free() && !e.invisible;
            match accept_placement(p, page, new_page, &doc) {
                Some((page, p, new_page, mut effects)) => {
                    if auto {
                        effects.push(Effect::AutoPlace(doc.id));
                    }
                    stay(queue, page, Some(p), new_page, doc, Some(e), effects)
                }
                None => stay(queue, page, placement, new_page, doc, Some(e), Vec::new()),
            }
        }
        Event::PageChanged(target) => {
            let max = if new_page { n } else { n.saturating_sub(1) };
            if target > max || target == page {
                return stay(queue, page, placement, new_page, doc, edit, Vec::new());
            }
            let placement = placement.map(|p| Placement { page_index: target, ..p });
            let mut effects =
                if target < n { vec![Effect::RenderPage { doc: doc.id, page: target }] } else { Vec::new() };
            // Die Blockbreite hängt bei Profilen ohne `w` von der Seitenbreite ab (KR-F01: Vorschau =
            // Ergebnis) – Vorschau für die neue Seite neu bauen.
            effects.push(Effect::BuildBlockPreview(doc.id));
            stay(queue, target, placement, new_page, doc, by_user(edit), effects)
        }
        Event::NewPageToggled => {
            if new_page {
                let last = n.saturating_sub(1);
                let placement = placement.map(|p| Placement { page_index: p.page_index.min(last), ..p });
                let page = page.min(last);
                let effects = vec![Effect::RenderPage { doc: doc.id, page }, Effect::BuildBlockPreview(doc.id)];
                stay(queue, page, placement, false, doc, by_user(edit), effects)
            } else if !doc.new_page_allowed() {
                // wie PDF-Over: „Neue Seite“ auf einem signierten Dokument landet auf der letzten Seite
                // (docs/analyse/04 §4.5); PIDA zeigt dazu einen Hinweis
                let last = n.saturating_sub(1);
                let placement = placement.map(|p| Placement { page_index: last, ..p });
                let mut effects = vec![Effect::Notify(MSG_NEW_PAGE_NOT_ALLOWED.into())];
                if page != last {
                    effects.push(Effect::RenderPage { doc: doc.id, page: last });
                    effects.push(Effect::BuildBlockPreview(doc.id));
                }
                stay(queue, last, placement, false, doc, edit, effects)
            } else {
                let placement = placement.map(|p| Placement { page_index: n, ..p });
                let effects = vec![Effect::BuildBlockPreview(doc.id)];
                stay(queue, n, placement, true, doc, by_user(edit), effects)
            }
        }
        // Arbeitsbereich: die Altereignisse bildet core-api auf `SignRequested` ab (ADR 0004 §3.2); kommen
        // sie trotzdem an, starten sie nichts (A1)
        Event::PositionConfirmed | Event::InvisibleChosen if edit.is_some() || ctx.workspace => {
            stay(queue, page, placement, new_page, doc, edit, Vec::new())
        }
        Event::PositionConfirmed => match placement {
            Some(mut p) => {
                // „Neue Seite“ nur, wenn die Position auf der virtuellen Seite liegt
                if !new_page && p.page_index >= n {
                    p.page_index = n.saturating_sub(1);
                }
                // „Signieren“: ausdrückliche Nutzeraktion (automatische Wahl auch für ID Austria)
                to_backend_choice(queue, doc, PositionChoice::Manual(p), Entry::SignRequested, Vec::new())
            }
            None => stay(queue, page, placement, new_page, doc, edit, Vec::new()),
        },
        Event::InvisibleChosen => {
            to_backend_choice(queue, doc, PositionChoice::Invisible, Entry::SignRequested, Vec::new())
        }
        Event::InvisibleSet(invisible) => match edit {
            Some(mut e) => {
                if !invisible && ctx.invisible {
                    // unsichtbares Profil: ein sichtbarer Block ist nicht möglich
                    let effects = vec![Effect::Notify(MSG_CONSTRAINT_INVISIBLE_PROFILE.into())];
                    return stay(queue, page, placement, new_page, doc, Some(e), effects);
                }
                e.choose_invisible(invisible);
                stay(queue, page, placement, new_page, doc, Some(e), Vec::new())
            }
            None => stay(queue, page, placement, new_page, doc, None, Vec::new()),
        },
        Event::TargetChosen(target) => {
            let Some(mut e) = edit else { return stay(queue, page, placement, new_page, doc, None, Vec::new()) };
            if e.target_taken(target) {
                // Mehrfachsignatur: jedes Feld bzw. der QR-Platzhalter nur für einen Block
                let effects = vec![Effect::Notify(MSG_BLOCK_TARGET_TAKEN.into())];
                return stay(queue, page, placement, new_page, doc, Some(e), effects);
            }
            let mut effects = Vec::new();
            match target {
                Target::Free => {
                    // „Frei platzieren“ bzw. „Freie Fläche suchen“ = automatische Platzierung (auf Wunsch der
                    // Nutzerin: folgt danach nicht mehr der Platzhaltersuche)
                    e.target = Target::Free;
                    e.origin = PlacementOrigin::Auto;
                    e.automatic = false;
                    if placement.is_some() {
                        effects.push(Effect::AutoPlace(doc.id));
                    }
                }
                Target::Field(i) if i < e.fields.len() => {
                    e.target = Target::Field(i);
                    e.origin = PlacementOrigin::Field;
                    e.automatic = false;
                }
                Target::Field(_) => effects.push(Effect::Notify(MSG_FIELD_LOCKED.into())),
                Target::Qr if e.qr.is_some() => {
                    e.target = Target::Qr;
                    e.origin = PlacementOrigin::Qr;
                    e.automatic = false;
                }
                Target::Qr => {}
            }
            let (page, placement, new_page) = settle_on_target(&e, page, placement, new_page, &doc, &mut effects);
            stay(queue, page, placement, new_page, doc, Some(e), effects)
        }
        Event::PlaceholderSearched { doc: d, qr } if d == doc.id => match edit {
            // nachträgliche QR-Suche (Einstellungen › Dokument): ein automatisch platziertes Dokument nimmt den
            // gefundenen Platzhalter, sonst wird er nur angeboten
            Some(mut e) => {
                e.qr_search = Some(qr);
                let follow = e.follow_settings(ctx);
                let mut effects =
                    if follow.auto_place && placement.is_some() { vec![Effect::AutoPlace(doc.id)] } else { Vec::new() };
                let (page, placement, new_page) = settle_on_target(&e, page, placement, new_page, &doc, &mut effects);
                stay(queue, page, placement, new_page, doc, Some(e), effects)
            }
            None => stay(queue, page, placement, new_page, doc, None, Vec::new()),
        },
        Event::SignRequested { backend, options } => {
            let mut e = match edit {
                Some(e) if ctx.workspace => e,
                edit => return stay(queue, page, placement, new_page, doc, edit, Vec::new()),
            };
            if !options.iter().any(|o| o.id == backend && o.available()) {
                let effects = vec![Effect::Notify(MSG_SIGNER_UNAVAILABLE.into())];
                return stay(queue, page, placement, new_page, doc, Some(e), effects);
            }
            let Some(position) = workspace_position(&e, placement, new_page, &doc) else {
                return stay(queue, page, placement, new_page, doc, Some(e), Vec::new());
            };
            // Mehrfachsignatur: Positionen aller Blöcke in ihrer Reihenfolge
            let Some(all) = all_positions(&e, &position, &doc, new_page) else {
                return stay(queue, page, placement, new_page, doc, Some(e), Vec::new());
            };
            // DocMDP-zertifiziert: kein neues sichtbares Feld (nur vorhandene Felder oder unsichtbar)
            let new_visible = |p: &PositionChoice| {
                matches!(p, PositionChoice::Manual(_) | PositionChoice::Auto | PositionChoice::Placeholder(_))
            };
            if (new_visible(&position) || all.iter().any(new_visible)) && !doc.new_visible_field_allowed {
                let effects = vec![Effect::Notify(MSG_CONSTRAINT_CERTIFIED.into())];
                return stay(queue, page, placement, new_page, doc, Some(e), effects);
            }
            e.parked = Some(ParkedPlacement { page, placement, new_page });
            e.rounds = (all.len() > 1).then_some(Rounds { positions: all, index: 0 });
            choose_backend(queue, doc, position, options, backend, Some(e), ctx, Vec::new())
        }
        Event::PreviewUnavailable { doc: d } if d == doc.id && edit.is_some() => {
            // Arbeitsbereich ohne Vorschau: bleibt bedienbar, signiert automatisch platziert (A1)
            let effects = vec![Effect::Notify(MSG_POSITIONING_NOT_POSSIBLE.into())];
            stay(queue, page.min(n.saturating_sub(1)), None, false, doc, edit, effects)
        }
        Event::PreviewUnavailable { doc: d } if d == doc.id => {
            let effects = vec![Effect::Notify(MSG_POSITIONING_NOT_POSSIBLE.into())];
            to_backend_choice(queue, doc, PositionChoice::Auto, Entry::Automatic, effects)
        }
        Event::Navigate(Step::Open) => discard_all(queue, Vec::new()),
        Event::Cancel => next_doc(queue, DocResult::Cancelled, ctx, Vec::new()),
        _ => stay(queue, page, placement, new_page, doc, edit, Vec::new()),
    }
}

fn choosing_backend(state: State, event: Event, ctx: &Context) -> Transition {
    let State::ChoosingBackend { queue, position, options, doc, auto_choice: auto, sign_requested, edit } = state
    else {
        unreachable!("ChoosingBackend")
    };
    match event {
        Event::BackendsProbed(options) => {
            if !auto {
                return ask_backend(queue, doc, position, options, edit, Vec::new());
            }
            if ctx.workspace && !ctx.headless {
                // Arbeitsbereich: automatisch nur die Startargument-Automatik mit einem Signiermittel, bei
                // dem das Dokument auf dem Gerät bleibt (D-06, A1); sonst Arbeitsbereich mit Konto-Chip
                return match auto_choice(&options, ctx) {
                    AutoChoice::Choose(id) if ctx.cli_auto && !leaves_device(&options, id) => {
                        choose_backend(queue, doc, position, options, id, edit, ctx, Vec::new())
                    }
                    _ => {
                        let auto_place = edit.as_ref().is_none_or(WorkspaceEdit::auto_free);
                        let edit = edit.unwrap_or_else(|| WorkspaceEdit::from_position(&position, ctx.invisible));
                        to_workspace(queue, doc, None, edit, auto_place, ctx, Vec::new())
                    }
                };
            }
            match auto_choice(&options, ctx) {
                // Autostart-Befund F-54: ohne „Signieren“ startet kein Signiermittel automatisch, bei dem
                // das Dokument das Gerät verlässt – die Auswahl (mit Kurzhinweis) ist dann die Nutzeraktion.
                // Ohne Oberfläche ist der Aufruf `pida-cli sign …` selbst die Nutzeraktion.
                AutoChoice::Choose(id) if !sign_requested && !ctx.headless && leaves_device(&options, id) => {
                    ask_backend(queue, doc, position, options, edit, Vec::new())
                }
                AutoChoice::Choose(id) => choose_backend(queue, doc, position, options, id, edit, ctx, Vec::new()),
                AutoChoice::Ask => ask_backend(queue, doc, position, options, edit, Vec::new()),
                AutoChoice::Fail(error) => {
                    let from = State::ChoosingBackend {
                        queue,
                        position,
                        options,
                        doc,
                        auto_choice: false,
                        sign_requested: false,
                        edit,
                    };
                    fail(from, error, None, ctx, Vec::new())
                }
            }
        }
        Event::BackendChosen(id) => choose_backend(queue, doc, position, options, id, edit, ctx, Vec::new()),
        // „Signieren mit X“ in der Auswahl = Wahl des Signiermittels (ADR 0004 §3.5)
        Event::SignRequested { backend, options } if ctx.workspace => {
            choose_backend(queue, doc, position, options, backend, edit, ctx, Vec::new())
        }
        Event::Navigate(Step::Open) => discard_all(queue, Vec::new()),
        Event::Navigate(Step::Positioning) if !ctx.invisible => {
            let previous = position.manual().copied();
            to_positioning(queue, doc, previous, ctx, Vec::new())
        }
        Event::Cancel if ctx.workspace && edit.is_some() => {
            restore_workspace(queue, doc, &position, edit, ctx, Vec::new())
        }
        Event::Cancel => match position {
            PositionChoice::Manual(p) if !ctx.headless => to_positioning(queue, doc, Some(p), ctx, Vec::new()),
            _ => next_doc(queue, DocResult::Cancelled, ctx, Vec::new()),
        },
        _ => Transition::stay(State::ChoosingBackend {
            queue,
            position,
            options,
            doc,
            auto_choice: auto,
            sign_requested,
            edit,
        }),
    }
}

/// Rückkehr zur Backend-Auswahl ohne automatische Wahl (Abbruch, F-55/F-72); im Arbeitsbereich zurück
/// in den Arbeitsbereich mit Hinweis „Es wurde nichts signiert.“ (ADR 0004 §3.6).
fn back_to_backend(state: State, ctx: &Context, mut effects: Vec<Effect>) -> Transition {
    let (queue, doc, position, edit) = match state {
        State::Signing { queue, doc, position, edit, .. } | State::Output { queue, doc, position, edit, .. } => {
            (queue, doc, position, edit)
        }
        other => return Transition::with(other, effects),
    };
    if ctx.headless {
        return next_doc(queue, DocResult::Cancelled, ctx, effects);
    }
    if ctx.workspace {
        effects.push(Effect::Notify(MSG_SIGNING_CANCELLED.into()));
        return restore_workspace(queue, doc, &position, edit, ctx, effects);
    }
    to_backend_choice(queue, doc, position, Entry::Ask, effects)
}

fn signing(state: State, event: Event, ctx: &Context) -> Transition {
    let State::Signing { queue, position, backend, phase, doc, flow, edit } = state else { unreachable!("Signing") };
    let ws = ctx.workspace;
    match event {
        Event::PhaseCompleted(done) if done == phase => {
            let (next, effect) = match phase {
                SigningPhase::OpeningSession if flow.needs_certificate => {
                    (SigningPhase::ReadingCertificate, vec![Effect::ReadCertificate])
                }
                SigningPhase::OpeningSession | SigningPhase::ReadingCertificate => (
                    SigningPhase::Preparing,
                    vec![Effect::Prepare { doc: doc.id, position: position.clone(), min_reserve: flow.min_reserve }],
                ),
                // Arbeitsbereich: ein Job mit eigenem Job-Abbruch (`sign_one`); die Sitzung bleibt bis nach
                // der Selbstprüfung offen (Wiederholen nach „CMS zu groß“ ohne neue Anmeldung)
                SigningPhase::Preparing if ws => (SigningPhase::Signing, vec![Effect::SignDocument]),
                SigningPhase::Preparing => (SigningPhase::Signing, vec![Effect::Sign]),
                SigningPhase::Signing if ws => (SigningPhase::Finishing, vec![Effect::Finish]),
                SigningPhase::Signing => (SigningPhase::Finishing, vec![Effect::CloseSession, Effect::Finish]),
                SigningPhase::Finishing | SigningPhase::Writing => {
                    let id = doc.id;
                    let mut edit = edit;
                    if ws && let Some(next) = next_round(&mut edit) {
                        // Mehrfachsignatur: das Ergebnis ist die Quelle der nächsten Signatur (dieselbe Sitzung)
                        let effects = vec![
                            Effect::NextSignature,
                            Effect::Prepare { doc: id, position: next.clone(), min_reserve: flow.min_reserve },
                        ];
                        let phase = SigningPhase::Preparing;
                        return Transition::with(
                            State::Signing { queue, position: next, backend, phase, doc, flow, edit },
                            effects,
                        );
                    }
                    let mut effects = if ws { vec![Effect::CloseSession] } else { Vec::new() };
                    effects.extend([Effect::DeletePersisted, Effect::DecideOutput(id)]);
                    return Transition::with(
                        State::Output { queue, saved: None, decision: None, doc, position, backend, edit },
                        effects,
                    );
                }
            };
            Transition::with(State::Signing { queue, position, backend, phase: next, doc, flow, edit }, effect)
        }
        Event::BackendFailed(e) => {
            let s = State::Signing { queue, position, backend, phase, doc, flow, edit };
            if e.is_user_cancel() {
                // still zurück zur Auswahl (Arbeitsbereich: zum Arbeitsbereich); gemerktes Passwort wird
                // gelöscht (F-72)
                return back_to_backend(s, ctx, vec![Effect::CancelSession, Effect::DeletePersisted]);
            }
            let retry = e.is_retryable().then_some(RetryTarget::Prepare);
            let close = keep_session(ctx, retry.is_some(), Some(e.scope()));
            fail(s, WorkflowError::from_backend(&e), retry, ctx, vec![close, Effect::DeletePersisted])
        }
        Event::SigningDeclined => {
            let s = State::Signing { queue, position, backend, phase, doc, flow, edit };
            if !ws {
                // klassisch: wie ein Abbruch (SL 6000/6001 = `Cancelled`)
                return back_to_backend(s, ctx, vec![Effect::CancelSession, Effect::DeletePersisted]);
            }
            // in der App abgelehnt bzw. abgelaufen: Fehlerschritt mit „Erneut versuchen“ in derselben
            // Sitzung (Zugangsdaten bleiben bis zur Entscheidung, §8 F10)
            let error = WorkflowError::Backend { code: MSG_SIGNING_DECLINED.into(), detail: String::new() };
            fail(s, error, Some(RetryTarget::Prepare), ctx, vec![Effect::ReleaseDocument, Effect::DeletePersisted])
        }
        Event::SelfCheckFailed(v) => {
            let s = State::Signing { queue, position, backend, phase, doc, flow, edit };
            fail(s, WorkflowError::Invariant(v), None, ctx, vec![Effect::CloseSession, Effect::DeletePersisted])
        }
        Event::SigningFailed(error) => {
            let retry = match error {
                WorkflowError::Rejected { .. } | WorkflowError::Invariant(_) => None,
                _ => Some(RetryTarget::Prepare),
            };
            let close = keep_session(ctx, retry.is_some(), None);
            let s = State::Signing { queue, position, backend, phase, doc, flow, edit };
            fail(s, error, retry, ctx, vec![close, Effect::DeletePersisted])
        }
        Event::CmsTooLarge { needed, reserve } => {
            let retry = Some(RetryTarget::PrepareWithReserve(grown_reserve(needed, reserve)));
            let close = keep_session(ctx, true, None);
            let s = State::Signing { queue, position, backend, phase, doc, flow, edit };
            fail(s, WorkflowError::CmsTooLarge { needed, reserve }, retry, ctx, vec![close, Effect::DeletePersisted])
        }
        Event::Cancel => {
            let s = State::Signing { queue, position, backend, phase, doc, flow, edit };
            back_to_backend(s, ctx, vec![Effect::CancelSession, Effect::DeletePersisted])
        }
        _ => Transition::stay(State::Signing { queue, position, backend, phase, doc, flow, edit }),
    }
}

fn output(state: State, event: Event, ctx: &Context) -> Transition {
    let State::Output { queue, saved, decision, doc, position, backend, edit } = state else { unreachable!("Output") };
    let mk = |queue, saved, decision, doc, position, backend, edit| State::Output {
        queue,
        saved,
        decision,
        doc,
        position,
        backend,
        edit,
    };
    match event {
        Event::OutputDecided(d) if saved.is_none() => {
            decided(mk(queue, saved, None, doc, position, backend, edit), d, ctx)
        }
        Event::OutputChosen(Some(path)) if saved.is_none() => {
            let d = OutputDecision::Save { path, exists: false };
            decided(mk(queue, saved, None, doc, position, backend, edit), d, ctx)
        }
        Event::OutputChosen(None) if saved.is_none() => {
            // Dialog abgebrochen: nicht gespeichert, Knopf „Speichern“ (F-86)
            if ctx.headless {
                return next_doc(queue, DocResult::Cancelled, ctx, Vec::new());
            }
            Transition::stay(mk(queue, None, None, doc, position, backend, edit))
        }
        Event::OverwriteAnswered(yes) if saved.is_none() => match decision {
            Some(OutputDecision::Save { path, exists: true }) if yes => {
                let d = Some(OutputDecision::Save { path: path.clone(), exists: true });
                Transition::with(mk(queue, None, d, doc, position, backend, edit), vec![Effect::Overwrite(path)])
            }
            Some(OutputDecision::Save { exists: true, .. }) => {
                if ctx.headless {
                    return next_doc(queue, DocResult::Cancelled, ctx, Vec::new());
                }
                Transition::stay(mk(queue, None, None, doc, position, backend, edit))
            }
            other => Transition::stay(mk(queue, None, other, doc, position, backend, edit)),
        },
        Event::SaveRequested if saved.is_none() => {
            let Some(input) = queue.current.as_ref().map(|d| d.path.clone()) else {
                return Transition::stay(mk(queue, saved, decision, doc, position, backend, edit));
            };
            let d = save_dialog(&input, &ctx.output_postfix);
            if ctx.headless {
                return decided(mk(queue, None, None, doc, position, backend, edit), d, ctx);
            }
            Transition::with(
                mk(queue, None, Some(d.clone()), doc, position, backend, edit),
                vec![Effect::AskSavePath(d)],
            )
        }
        Event::Saved(info) if saved.is_none() => {
            // F-88 gilt nicht im Arbeitsbereich (ohne Startargument-Automatik): „Fertig“ mit
            // „Nächstes Dokument“ (ADR 0004 §3.15)
            let auto_next = !ctx.workspace || ctx.cli_auto;
            if !queue.pending.is_empty() && auto_next {
                // F-88: Ergebnis-Screen überspringen, nächstes Dokument
                return next_doc(queue, DocResult::Signed { output: info }, ctx, Vec::new());
            }
            if (ctx.skip_finish || ctx.headless) && queue.pending.is_empty() {
                // F-90: Ende nach dem Speichern
                let mut queue = queue;
                let mut effects = Vec::new();
                if let Some(id) = queue.current_id() {
                    effects.push(Effect::Discard(id));
                }
                queue.finish_current(DocResult::Signed { output: info });
                return finish(queue, effects);
            }
            Transition::stay(mk(queue, Some(info), None, doc, position, backend, edit))
        }
        Event::SaveFailed(detail) if saved.is_none() => {
            let from = mk(queue, None, decision, doc, position, backend, edit);
            fail(from, WorkflowError::Save(detail), Some(RetryTarget::Save), ctx, Vec::new())
        }
        Event::OpenResultRequested => {
            let effects = saved.as_ref().map(|s| vec![Effect::OpenResult(s.path.clone())]).unwrap_or_default();
            Transition::with(mk(queue, saved, decision, doc, position, backend, edit), effects)
        }
        Event::ShareResultRequested => {
            let effects = saved.as_ref().map(|s| vec![Effect::ShareResult(s.path.clone())]).unwrap_or_default();
            Transition::with(mk(queue, saved, decision, doc, position, backend, edit), effects)
        }
        Event::ExportResultRequested => {
            let effects = saved.as_ref().map(|s| vec![Effect::ExportResult(s.path.clone())]).unwrap_or_default();
            Transition::with(mk(queue, saved, decision, doc, position, backend, edit), effects)
        }
        Event::Cancel if decision.is_some() && saved.is_none() => {
            if ctx.headless {
                return next_doc(queue, DocResult::Cancelled, ctx, Vec::new());
            }
            Transition::stay(mk(queue, None, None, doc, position, backend, edit))
        }
        Event::Navigate(Step::Open) => {
            // F-16: aus dem Ergebnis-Screen wird das nächste Dokument der Warteschlange genommen
            let result = match saved {
                Some(output) => DocResult::Signed { output },
                None => DocResult::Cancelled,
            };
            next_doc(queue, result, ctx, Vec::new())
        }
        Event::Navigate(Step::Positioning) if !ctx.invisible => {
            // F-91: Original erneut signieren, vorherige Position (F-43)
            let previous = position.manual().copied();
            to_positioning(queue, doc, previous, ctx, Vec::new())
        }
        Event::Navigate(Step::Signing) => {
            // F-55: Auswahl erzwingen
            to_backend_choice(queue, doc, position, Entry::Ask, Vec::new())
        }
        _ => Transition::stay(mk(queue, saved, decision, doc, position, backend, edit)),
    }
}

/// Ausgabeziel entschieden: schreiben, Überschreib-Rückfrage oder Speichern-Dialog.
fn decided(state: State, d: OutputDecision, ctx: &Context) -> Transition {
    let State::Output { queue, doc, position, backend, edit, .. } = state else { unreachable!("Output") };
    let (decision, effects) = match d {
        OutputDecision::Save { path, exists: false } => {
            (OutputDecision::Save { path: path.clone(), exists: false }, vec![Effect::Write(path)])
        }
        OutputDecision::Save { path, exists: true } => {
            (OutputDecision::Save { path: path.clone(), exists: true }, vec![Effect::AskOverwrite(path)])
        }
        OutputDecision::Ask { directory, file_name } if ctx.headless => {
            // ohne Dialog neben der Quelle; eine vorhandene Datei wird nicht ersetzt (Write)
            let path = directory.join(file_name);
            (OutputDecision::Save { path: path.clone(), exists: false }, vec![Effect::Write(path)])
        }
        ask @ OutputDecision::Ask { .. } => (ask.clone(), vec![Effect::AskSavePath(ask)]),
    };
    Transition::with(
        State::Output { queue, saved: None, decision: Some(decision), doc, position, backend, edit },
        effects,
    )
}

fn error(state: State, event: Event, ctx: &Context) -> Transition {
    let State::Error { queue, error, retry, from } = state else { unreachable!("Error") };
    match event {
        Event::Retry => match (retry, from) {
            (Some(RetryTarget::ResetSettings), _) => Transition::with(State::Starting, vec![Effect::ResetSettings]),
            (Some(RetryTarget::Load), Some(f)) if matches!(*f, State::Analyzing { .. }) => {
                start_current(queue, ctx, Vec::new())
            }
            (Some(target @ (RetryTarget::Prepare | RetryTarget::PrepareWithReserve(_))), Some(f))
                if matches!(*f, State::Signing { .. }) =>
            {
                let State::Signing { position, backend, doc, mut flow, edit, .. } = *f else { unreachable!("Signing") };
                if let RetryTarget::PrepareWithReserve(n) = target {
                    flow.min_reserve = Some(n.max(flow.min_reserve.unwrap_or(0)));
                }
                // Arbeitsbereich: `OpenSession` verwendet eine offen gehaltene Sitzung wieder (core-api)
                start_signing(queue, doc, position, backend, flow, edit, Vec::new())
            }
            (Some(RetryTarget::Save), Some(f)) if matches!(*f, State::Output { .. }) => {
                let State::Output { decision, doc, position, backend, edit, .. } = *f else { unreachable!("Output") };
                let effect = match &decision {
                    Some(OutputDecision::Save { path, exists: true }) => Effect::Overwrite(path.clone()),
                    Some(OutputDecision::Save { path, exists: false }) => Effect::Write(path.clone()),
                    // ohne festes Ziel: Speichern-Dialog erneut zeigen (sonst wartete der Schritt ohne Dialog)
                    Some(ask @ OutputDecision::Ask { .. }) => Effect::AskSavePath(ask.clone()),
                    None => {
                        let s = State::Output { queue, saved: None, decision: None, doc, position, backend, edit };
                        return Transition::stay(s);
                    }
                };
                let s = State::Output { queue, saved: None, decision, doc, position, backend, edit };
                Transition::with(s, vec![effect])
            }
            (retry, from) => Transition::stay(State::Error { queue, error, retry, from }),
        },
        Event::Cancel => match from.map(|b| *b) {
            Some(State::Starting) | None if matches!(error, WorkflowError::Config(_)) => {
                finish_with(ExitCode::General, queue.done, Vec::new())
            }
            Some(State::Analyzing { .. }) | Some(State::Resuming { .. }) => {
                next_doc(queue, DocResult::Failed { error }, ctx, Vec::new())
            }
            // Arbeitsbereich: „Schließen“ im Fehlerschritt → Arbeitsbereich mit unverändertem
            // Bearbeitungszustand; eine für „Erneut versuchen“ offen gehaltene Sitzung samt Passwort wird
            // verworfen (F-72)
            Some(State::Signing { doc, position, edit, .. }) if ctx.workspace => {
                restore_workspace(queue, doc, &position, edit, ctx, vec![Effect::CancelSession])
            }
            Some(State::ChoosingBackend { doc, position, edit, .. }) if ctx.workspace => {
                restore_workspace(queue, doc, &position, edit, ctx, Vec::new())
            }
            Some(State::Signing { doc, position, .. }) | Some(State::ChoosingBackend { doc, position, .. }) => {
                to_backend_choice(queue, doc, position, Entry::Ask, Vec::new())
            }
            Some(State::Output { doc, position, backend, edit, .. }) => {
                // nicht gespeichert: Knopf „Speichern“ (F-86); der Batch läuft über „Öffnen“ weiter (B-14:
                // die Warteschlange bleibt erhalten)
                Transition::stay(State::Output { queue, saved: None, decision: None, doc, position, backend, edit })
            }
            Some(mut other) => {
                if let Some(q) = other.queue_mut() {
                    *q = queue;
                }
                Transition::stay(other)
            }
            None => Transition::stay(State::Idle { queue }),
        },
        // Arbeitsbereich: „Öffnen“ aus dem Fehlerschritt des Signierens verwirft das Dokument und die offen
        // gehaltene Sitzung (ADR 0004 §3.6)
        Event::Navigate(Step::Open)
            if ctx.workspace && from.as_deref().is_some_and(|f| matches!(f, State::Signing { .. })) =>
        {
            discard_all(queue, vec![Effect::CancelSession, Effect::DeletePersisted])
        }
        _ => Transition::stay(State::Error { queue, error, retry, from }),
    }
}
