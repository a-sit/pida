//! Tabellentests der Stapelübersicht und des Stapellaufs (ADR 0004 §3.8, Paket K2): Übersicht ab zwei
//! Dokumenten mit Voranalyse, Einzeldokument öffnen/zurück mit eigenem Plan, „Alle signieren“ mit **einer**
//! Sitzung (eine Anmeldung, höchstens ein Zertifikat, je Dokument eine Bestätigung), Überspringen ≠ Abbrechen,
//! Pausen je Fehlerart, Umgang mit vorhandenen Zieldateien, Zusammenfassung.

mod common;

use common::*;
use pida_backend_api::{BackendError, BackendId};
use pida_pades::{AcceptanceClass, OutputInfo};
use pida_placement::Placement;
use pida_workflow::{
    BatchItemStatus, BatchOutput, BatchPause, BatchPhase, Context, DocId, DocSummary, Effect, Event, ExistingPolicy,
    PlaceholderFound, PositionChoice, Queue, RetryTarget, SigningPhase, State, Step, Target, WorkflowError,
};

fn ctx() -> Context {
    Context { workspace: true, batch_overview: true, ..Context::default() }
}

fn summary(pages: usize) -> DocSummary {
    DocSummary {
        page_count: pages,
        size_bytes: 1234,
        signed: false,
        class: AcceptanceClass::K1Regular,
        qr: false,
        fields: 0,
        warnings: Vec::new(),
        placeholder: None,
        new_visible_field_allowed: true,
        first_field: None,
        qr_searched: true,
    }
}

fn info(path: &str) -> OutputInfo {
    OutputInfo { path: path.into(), len: 10, sha256: "ab".into() }
}

fn batch_of(s: &State) -> &pida_workflow::Batch {
    s.queue().and_then(|q| q.batch.as_ref()).expect("Stapel")
}

fn status(s: &State, id: u64) -> BatchItemStatus {
    batch_of(s).item(DocId(id)).expect("Eintrag").status.clone()
}

fn opens(effects: &[Effect]) -> usize {
    effects.iter().filter(|e| matches!(e, Effect::OpenSession(_))).count()
}

fn count(effects: &[Effect], pred: impl Fn(&Effect) -> bool) -> usize {
    effects.iter().filter(|e| pred(e)).count()
}

/// Übersicht mit `ids`, alle voranalysiert (`Ready`).
fn overview(ids: &[u64]) -> State {
    let t =
        step(State::Idle { queue: Queue::default() }, Event::FilesAdded(ids.iter().map(|i| doc(*i)).collect()), &ctx());
    let mut s = t.state;
    for id in ids {
        s = step(s, Event::Preanalyzed { doc: DocId(*id), result: Ok(summary(3)) }, &ctx()).state;
    }
    s
}

fn started(s: State, backend: BackendId, docs: &[u64], on_existing: ExistingPolicy) -> pida_workflow::Transition {
    step(
        s,
        Event::BatchStarted {
            backend,
            docs: docs.iter().map(|d| DocId(*d)).collect(),
            output: Some(BatchOutput::Folder("/out".into())),
            on_existing,
            options: options(true, true),
        },
        &ctx(),
    )
}

/// Treibt einen Lauf: protokolliert alle Effekte, beantwortet sie wie core-api (Erfolg) bis zum Ende bzw.
/// bis `stop` für ein Ereignis gilt.
struct Driver {
    state: State,
    effects: Vec<Effect>,
}

impl Driver {
    fn new(t: pida_workflow::Transition) -> Driver {
        Driver { state: t.state, effects: t.effects }
    }

    fn send(&mut self, e: Event) -> Vec<Effect> {
        let t = step(self.state.clone(), e, &ctx());
        self.state = t.state;
        self.effects.extend(t.effects.clone());
        t.effects
    }

    fn current(&self) -> u64 {
        self.state.queue().and_then(Queue::current_id).map(|d| d.0).unwrap_or(0)
    }

    fn phase(&self) -> BatchPhase {
        match &self.state {
            State::BatchSigning { run, .. } => run.phase,
            other => panic!("kein Lauf: {}", other.name()),
        }
    }

    fn pause(&self) -> Option<BatchPause> {
        match &self.state {
            State::BatchSigning { run, .. } => run.pause.clone(),
            _ => None,
        }
    }

    /// Erfolgreicher Schritt der aktuellen Phase.
    fn ok(&mut self) -> Vec<Effect> {
        let id = self.current();
        match self.phase() {
            BatchPhase::OpeningSession => self.send(Event::PhaseCompleted(SigningPhase::OpeningSession)),
            BatchPhase::ReadingCertificate => self.send(Event::PhaseCompleted(SigningPhase::ReadingCertificate)),
            BatchPhase::CheckingTarget => self.send(Event::BatchOutputDecided {
                doc: DocId(id),
                path: format!("/out/doc{id}_signed.pdf").into(),
                exists: false,
            }),
            BatchPhase::Loading => self.send(analyzed(id, 3, None, false)),
            BatchPhase::Preparing => self.send(Event::PhaseCompleted(SigningPhase::Preparing)),
            BatchPhase::Confirming => self.send(Event::PhaseCompleted(SigningPhase::Signing)),
            BatchPhase::Finishing => self.send(Event::PhaseCompleted(SigningPhase::Finishing)),
            BatchPhase::Writing => {
                let decided = self.effects.iter().rev().find_map(|e| match e {
                    Effect::DecideBatchOutput { doc, .. } => Some(*doc),
                    _ => None,
                });
                let written = self.effects.last().is_some_and(|e| matches!(e, Effect::Write(_) | Effect::Overwrite(_)));
                if written || decided != Some(DocId(id)) {
                    self.send(Event::Saved(info(&format!("/out/doc{id}_signed.pdf"))))
                } else {
                    self.send(Event::BatchOutputDecided {
                        doc: DocId(id),
                        path: format!("/out/doc{id}_signed.pdf").into(),
                        exists: false,
                    })
                }
            }
        }
    }

    /// Bis zur Phase `phase` des Dokuments `id` (Erfolg in allen Schritten davor).
    fn until(&mut self, id: u64, phase: BatchPhase) {
        for _ in 0..200 {
            if matches!(self.state, State::BatchSigning { .. }) && self.current() == id && self.phase() == phase {
                return;
            }
            assert!(matches!(self.state, State::BatchSigning { .. }), "Lauf endete vor {id}/{phase:?}");
            self.ok();
        }
        panic!("{id}/{phase:?} nicht erreicht");
    }

    fn finish(&mut self) {
        for _ in 0..400 {
            if !matches!(self.state, State::BatchSigning { .. }) {
                return;
            }
            self.ok();
        }
        panic!("Lauf endet nicht");
    }
}

// ------------------------------------------------------------------------------------------------
// Übersicht
// ------------------------------------------------------------------------------------------------

#[test]
fn two_or_more_documents_open_the_overview_with_preanalysis() {
    let t = step(State::Idle { queue: Queue::default() }, Event::FilesAdded(vec![doc(1), doc(2), doc(3)]), &ctx());
    let State::BatchOverview { queue } = &t.state else { panic!("{:?}", t.state) };
    assert_eq!(queue.current_id(), None, "Übersicht ohne aktuelles Dokument");
    assert_eq!(queue.pending.len(), 3);
    for id in 1..=3 {
        assert!(t.effects.contains(&Effect::Preanalyze(DocId(id))));
        assert_eq!(status(&t.state, id), BatchItemStatus::Analyzing);
    }
    assert!(t.effects.contains(&Effect::RememberDirectory("/in".into())));
    // „Öffnen mit“ mehrerer Dateien beim Start
    let t = step(State::Starting, Event::Started { documents: vec![doc(1), doc(2)] }, &ctx());
    assert!(matches!(t.state, State::BatchOverview { .. }));
    // ein Dokument: Arbeitsbereich wie bisher
    let t = step(State::Idle { queue: Queue::default() }, Event::FilesAdded(vec![doc(1)]), &ctx());
    assert!(matches!(t.state, State::Analyzing { .. }));
    // ohne Stapelübersicht bzw. mit Startargument-Automatik (D-06): nacheinander
    for c in [Context { workspace: true, ..Context::default() }, Context { cli_auto: true, ..ctx() }] {
        let t = step(State::Idle { queue: Queue::default() }, Event::FilesAdded(vec![doc(1), doc(2)]), &c);
        assert!(matches!(t.state, State::Analyzing { .. }), "{:?}", t.state);
    }
}

#[test]
fn preanalysis_sets_ready_positioned_rejected_and_needs_review() {
    let t = step(State::Idle { queue: Queue::default() }, Event::FilesAdded((1..=4).map(doc).collect()), &ctx());
    let c = Context { placeholder_search: true, ..ctx() };
    let mut s = step(t.state, Event::Preanalyzed { doc: DocId(1), result: Ok(summary(2)) }, &c).state;
    let with_qr = DocSummary { qr: true, placeholder: Some(qr()), ..summary(1) };
    s = step(s, Event::Preanalyzed { doc: DocId(2), result: Ok(with_qr) }, &c).state;
    let rejected = WorkflowError::Rejected { class: AcceptanceClass::K5Rejected, code: "error-PDFPwdProtected".into() };
    s = step(s, Event::Preanalyzed { doc: DocId(3), result: Err(rejected) }, &c).state;
    let warned = DocSummary { warnings: vec!["pdf-usage-rights".into()], ..summary(1) };
    s = step(s, Event::Preanalyzed { doc: DocId(4), result: Ok(warned) }, &c).state;
    assert_eq!(status(&s, 1), BatchItemStatus::Ready);
    assert_eq!(status(&s, 2), BatchItemStatus::Positioned);
    assert!(matches!(batch_of(&s).item(DocId(2)).unwrap().plan, Some(PositionChoice::Placeholder(_))));
    assert_eq!(status(&s, 3), BatchItemStatus::Rejected { code: "error-PDFPwdProtected".into() });
    assert_eq!(status(&s, 4), BatchItemStatus::NeedsReview { codes: vec!["pdf-usage-rights".into()] });
    assert_eq!(batch_of(&s).signable(), 2);
    // StartBatch nimmt nur Ready/Positioned: ein Lauf mit 1, 2
    let t = started(s, BackendId::AtrustNative, &[1, 2, 3, 4], ExistingPolicy::KeepBoth);
    let State::BatchSigning { run, .. } = &t.state else { panic!() };
    assert_eq!(run.order, vec![DocId(1), DocId(2)]);
    assert_eq!(t.effects, vec![Effect::OpenSession(BackendId::AtrustNative)]);
}

#[test]
fn start_is_refused_without_documents_signer_or_folder() {
    let s = step(State::Idle { queue: Queue::default() }, Event::FilesAdded(vec![doc(1), doc(2)]), &ctx()).state;
    // noch in der Voranalyse
    let t = started(s.clone(), BackendId::Keystore, &[1, 2], ExistingPolicy::KeepBoth);
    assert!(matches!(t.state, State::BatchOverview { .. }));
    assert_eq!(t.effects, vec![Effect::Notify("ng-batch-nothing-to-sign".into())]);
    let s = overview(&[1, 2]);
    // Signiermittel nicht verfügbar
    let t = step(
        s.clone(),
        Event::BatchStarted {
            backend: BackendId::LocalBku,
            docs: vec![DocId(1)],
            output: Some(BatchOutput::Configured),
            on_existing: ExistingPolicy::KeepBoth,
            options: options(false, true),
        },
        &ctx(),
    );
    assert!(matches!(t.state, State::BatchOverview { .. }));
    assert_eq!(t.effects, vec![Effect::Notify("ng-signer-unavailable".into())]);
    // Ziel nicht bestimmbar („jedes Mal fragen“ ohne Ordnerwahl)
    let t = step(
        s,
        Event::BatchStarted {
            backend: BackendId::Keystore,
            docs: vec![DocId(1), DocId(2)],
            output: None,
            on_existing: ExistingPolicy::KeepBoth,
            options: options(false, true),
        },
        &ctx(),
    );
    assert!(matches!(t.state, State::BatchOverview { .. }));
    assert_eq!(t.effects, vec![Effect::Notify("ng-batch-output-folder-required".into())]);
}

#[test]
fn removing_entries_ends_in_idle() {
    let s = overview(&[1, 2]);
    let t = step(s, Event::BatchItemRemoved(DocId(1)), &ctx());
    assert!(matches!(t.state, State::BatchOverview { .. }));
    assert_eq!(batch_of(&t.state).items.len(), 1);
    assert_eq!(t.effects, vec![Effect::Discard(DocId(1))]);
    let t = step(t.state, Event::BatchItemRemoved(DocId(2)), &ctx());
    let State::Idle { queue } = &t.state else { panic!("{:?}", t.state) };
    assert!(queue.is_empty() && queue.batch.is_none());
}

#[test]
fn files_added_to_the_overview_and_while_running_are_not_part_of_the_run() {
    let s = overview(&[1, 2]);
    let t = step(s, Event::FilesAdded(vec![doc(3)]), &ctx());
    assert_eq!(t.effects, vec![Effect::Preanalyze(DocId(3))]);
    assert_eq!(status(&t.state, 3), BatchItemStatus::Analyzing);
    let s = step(t.state, Event::Preanalyzed { doc: DocId(3), result: Ok(summary(1)) }, &ctx()).state;
    let mut d = Driver::new(started(s, BackendId::Keystore, &[1, 2], ExistingPolicy::KeepBoth));
    d.until(1, BatchPhase::Confirming);
    let effects = d.send(Event::FilesAdded(vec![doc(4)]));
    assert!(effects.contains(&Effect::Preanalyze(DocId(4))));
    assert!(effects.iter().any(|e| matches!(e, Effect::NotifyWith(c, _) if c == "ng-batch-added")));
    let State::BatchSigning { run, .. } = &d.state else { panic!() };
    assert_eq!(run.order, vec![DocId(1), DocId(2)], "genau die genannten Dokumente (A1)");
    d.finish();
    assert_eq!(status(&d.state, 3), BatchItemStatus::Ready, "nicht im Lauf");
    assert_eq!(status(&d.state, 4), BatchItemStatus::Analyzing);
}

// ------------------------------------------------------------------------------------------------
// Einzeldokument aus der Übersicht
// ------------------------------------------------------------------------------------------------

#[test]
fn open_item_move_and_back_to_batch_keeps_the_plan() {
    let s = overview(&[1, 2]);
    let t = step(s, Event::BatchItemOpened(DocId(2)), &ctx());
    assert_eq!(t.effects, vec![Effect::Analyze(DocId(2))]);
    assert!(matches!(&t.state, State::Analyzing { queue } if queue.current_id() == Some(DocId(2))));
    assert!(t.state.opened_from_batch());
    let t = step(t.state, analyzed(2, 3, None, false), &ctx());
    let State::Positioning { edit: Some(edit), .. } = &t.state else { panic!("{:?}", t.state) };
    assert!(edit.return_to_batch);
    assert!(t.effects.contains(&Effect::AutoPlace(DocId(2))), "ohne Plan automatisch platziert");
    // Bewegen → „Zurück zum Stapel“ → Positioned mit diesem Plan
    let p = Placement { page_index: 1, x: 40.0, y: 300.0, width: 230.0, height: 49.0 };
    let s = step(t.state, Event::PlacementChanged(p), &ctx()).state;
    let t = step(s, Event::BackToBatch, &ctx());
    assert!(matches!(t.state, State::BatchOverview { .. }), "{:?}", t.state);
    assert!(t.effects.contains(&Effect::Discard(DocId(2))), "Dokument freigegeben");
    assert_eq!(status(&t.state, 2), BatchItemStatus::Positioned);
    assert_eq!(batch_of(&t.state).item(DocId(2)).unwrap().plan, Some(PositionChoice::Manual(p)));
    assert_eq!(t.state.queue().unwrap().current_id(), None);
    // erneut öffnen: Plan wiederhergestellt (frei, Herkunft Nutzerin), keine automatische Platzierung
    let t = step(t.state, Event::BatchItemOpened(DocId(2)), &ctx());
    let t = step(t.state, analyzed(2, 3, None, false), &ctx());
    let State::Positioning { placement: Some(q), page: 1, edit: Some(edit), .. } = &t.state else {
        panic!("{:?}", t.state)
    };
    assert_eq!(*q, p);
    assert_eq!(edit.origin, pida_workflow::PlacementOrigin::User);
    assert!(!t.effects.contains(&Effect::AutoPlace(DocId(2))));
    // Abbrechen bzw. „Start“ kehrt ohne Änderung zurück
    for e in [Event::Cancel, Event::Navigate(Step::Open)] {
        let back = step(t.state.clone(), e, &ctx());
        assert!(matches!(back.state, State::BatchOverview { .. }), "{:?}", back.state);
        assert_eq!(batch_of(&back.state).item(DocId(2)).unwrap().plan, Some(PositionChoice::Manual(p)));
    }
    // unsichtbar → Plan „unsichtbar“; automatisch platziert → kein Plan (Ready)
    let s = step(t.state, Event::InvisibleSet(true), &ctx()).state;
    let t = step(s, Event::BackToBatch, &ctx());
    assert_eq!(batch_of(&t.state).item(DocId(2)).unwrap().plan, Some(PositionChoice::Invisible));
    let t = step(t.state, Event::BatchItemOpened(DocId(1)), &ctx());
    let t = step(t.state, analyzed(1, 3, None, false), &ctx());
    let t = step(t.state, Event::BackToBatch, &ctx());
    assert_eq!(status(&t.state, 1), BatchItemStatus::Ready);
}

/// Einstellung „Automatische Positionierung“ (ADR 0004 Nachtrag 8): der Stapel bleibt unverändert – Einträge
/// automatisch platziert und „Bereit“; ein aus der Übersicht geöffneter Eintrag ist nicht gesperrt (die
/// Nutzerin öffnet ihn, um ihn zu prüfen bzw. zu platzieren), auch nicht nach einer Einstellungsänderung.
#[test]
fn auto_position_leaves_the_batch_unchanged() {
    let auto = Context { auto_position: true, ..ctx() };
    let t = step(State::Idle { queue: Queue::default() }, Event::FilesAdded(vec![doc(1), doc(2)]), &auto);
    let mut s = t.state;
    for id in [1, 2] {
        s = step(s, Event::Preanalyzed { doc: DocId(id), result: Ok(summary(3)) }, &auto).state;
        assert_eq!(status(&s, id), BatchItemStatus::Ready);
    }
    let t = step(s, Event::BatchItemOpened(DocId(1)), &auto);
    let t = step(t.state, analyzed(1, 3, None, false), &auto);
    let State::Positioning { edit: Some(edit), .. } = &t.state else { panic!("{:?}", t.state) };
    assert!(edit.return_to_batch && !edit.placement_locked());
    let t = step(t.state, Event::SettingsChanged, &auto);
    let State::Positioning { edit: Some(edit), .. } = &t.state else { panic!("{:?}", t.state) };
    assert!(!edit.placement_locked());
    let p = Placement { page_index: 1, x: 40.0, y: 300.0, width: 230.0, height: 49.0 };
    let t = step(t.state, Event::PlacementChanged(p), &auto);
    let t = step(t.state, Event::BackToBatch, &auto);
    assert_eq!(batch_of(&t.state).item(DocId(1)).unwrap().plan, Some(PositionChoice::Manual(p)));
    let t = started(t.state, BackendId::Keystore, &[1, 2], ExistingPolicy::KeepBoth);
    assert!(matches!(t.state, State::BatchSigning { .. }), "{:?}", t.state);
}

fn summary_with_qr(pages: usize) -> DocSummary {
    DocSummary { qr: true, placeholder: Some(qr()), qr_searched: true, ..summary(pages) }
}

/// „Für diesen Stapel automatisch positionieren“ (ADR 0004 Nachtrag 8) bei ausgeschalteter Einstellung: nur
/// für diesen Stapel; automatisch platzierte Einträge (auch QR-Platzhalter, Platzhaltersuche hat Vorrang)
/// gelten als „Bereit“, von Hand platzierte behalten ihren Plan; erst mit `replace_manual` (ausdrücklich
/// bestätigt) werden auch sie automatisch platziert. Startet nichts; nach dem Lauf gilt wieder die Einstellung.
#[test]
fn auto_position_for_this_batch_only() {
    let qr_ctx = Context { placeholder_search: true, ..ctx() };
    let t = step(State::Idle { queue: Queue::default() }, Event::FilesAdded(vec![doc(1), doc(2), doc(3)]), &qr_ctx);
    let mut s = t.state;
    s = step(s, Event::Preanalyzed { doc: DocId(1), result: Ok(summary(3)) }, &qr_ctx).state;
    s = step(s, Event::Preanalyzed { doc: DocId(2), result: Ok(summary_with_qr(3)) }, &qr_ctx).state;
    s = step(s, Event::Preanalyzed { doc: DocId(3), result: Ok(summary(3)) }, &qr_ctx).state;
    // Dokument 3 von Hand platzieren
    let t = step(s, Event::BatchItemOpened(DocId(3)), &qr_ctx);
    let t = step(t.state, analyzed(3, 3, None, false), &qr_ctx);
    let p = Placement { page_index: 1, x: 40.0, y: 300.0, width: 230.0, height: 49.0 };
    let t = step(t.state, Event::PlacementChanged(p), &qr_ctx);
    let s = step(t.state, Event::BackToBatch, &qr_ctx).state;
    let b = batch_of(&s);
    assert_eq!((b.manual_plans(), b.auto_position(&qr_ctx)), (1, false));
    assert_eq!(b.item(DocId(2)).unwrap().shown_status(false), BatchItemStatus::Positioned, "QR wie bisher");

    // einschalten, eigene Pläne behalten (Standard)
    let t = step(s, Event::BatchAutoPositionSet { enabled: true, replace_manual: false }, &qr_ctx);
    assert!(t.effects.is_empty() && opens(&t.effects) == 0, "startet nichts: {:?}", t.effects);
    let b = batch_of(&t.state);
    assert!(b.auto_position_once && b.auto_position(&qr_ctx));
    let shown = |b: &pida_workflow::Batch, id: u64| b.item(DocId(id)).unwrap().shown_status(b.auto_position(&qr_ctx));
    assert_eq!(shown(b, 1), BatchItemStatus::Ready);
    assert_eq!(shown(b, 2), BatchItemStatus::Ready, "automatischer QR-Plan gilt als „Bereit“");
    assert_eq!(b.item(DocId(2)).unwrap().plan, Some(PositionChoice::Placeholder(qr())), "Platzhaltersuche hat Vorrang");
    assert_eq!(shown(b, 3), BatchItemStatus::Positioned, "von Hand platziert: behalten");
    assert_eq!(b.item(DocId(3)).unwrap().plan, Some(PositionChoice::Manual(p)));
    assert_eq!(b.manual_plans(), 1);

    // ausdrücklich bestätigt: auch die eigenen Pläne neu automatisch
    let t = step(t.state, Event::BatchAutoPositionSet { enabled: true, replace_manual: true }, &qr_ctx);
    let b = batch_of(&t.state);
    let item = b.item(DocId(3)).unwrap();
    assert!(item.auto_plan && item.plan.is_none() && item.status == BatchItemStatus::Ready);
    assert_eq!(b.manual_plans(), 0);
    assert!(opens(&t.effects) == 0);

    // die Einstellung wurde nicht geändert; ausschalten = wieder wie eingestellt
    let off = step(t.state.clone(), Event::BatchAutoPositionSet { enabled: false, replace_manual: false }, &qr_ctx);
    let b = batch_of(&off.state);
    assert!(!b.auto_position_once && !b.auto_position(&qr_ctx));
    assert_eq!(shown(b, 2), BatchItemStatus::Positioned);

    // nach dem Lauf gilt wieder die Einstellung (Ende wie Abbruch)
    let t = step(t.state, Event::BatchAutoPositionSet { enabled: true, replace_manual: false }, &qr_ctx);
    let run = started(t.state, BackendId::Keystore, &[1, 2, 3], ExistingPolicy::KeepBoth);
    assert!(matches!(run.state, State::BatchSigning { .. }), "{:?}", run.state);
    assert!(batch_of(&run.state).auto_position_once, "gilt für diesen Lauf");
    let aborted = step(run.state, Event::BatchAbort, &qr_ctx);
    assert!(matches!(aborted.state, State::BatchOverview { .. }), "{:?}", aborted.state);
    assert!(!batch_of(&aborted.state).auto_position_once, "nach dem Abbruch wieder laut Einstellung");
    // laut Einstellung: immer automatisch
    let setting = Context { auto_position: true, ..qr_ctx.clone() };
    assert!(batch_of(&aborted.state).auto_position(&setting));
    // außerhalb der Übersicht ohne Wirkung
    let single = workspace_single();
    let t = step(single.clone(), Event::BatchAutoPositionSet { enabled: true, replace_manual: true }, &ctx());
    assert_eq!(t.state, single);
}

/// Einzeldokument im Arbeitsbereich (ohne Stapel).
fn workspace_single() -> State {
    step(State::Analyzing { queue: queue(&[1]) }, analyzed(1, 3, None, false), &ctx()).state
}

#[test]
fn opened_item_signed_individually_returns_to_the_overview() {
    let s = overview(&[1, 2]);
    let s = step(s, Event::BatchItemOpened(DocId(1)), &ctx()).state;
    let s = step(s, analyzed(1, 3, None, false), &ctx()).state;
    let t = step(s, Event::SignRequested { backend: BackendId::Keystore, options: options(false, true) }, &ctx());
    assert!(matches!(t.state, State::Signing { .. }));
    assert!(t.state.opened_from_batch());
    let (s, _) = run(
        t.state,
        [
            Event::PhaseCompleted(SigningPhase::OpeningSession),
            Event::PhaseCompleted(SigningPhase::ReadingCertificate),
            Event::PhaseCompleted(SigningPhase::Preparing),
            Event::PhaseCompleted(SigningPhase::Signing),
            Event::PhaseCompleted(SigningPhase::Finishing),
        ],
        &ctx(),
    );
    assert!(matches!(s, State::Output { .. }), "{s:?}");
    let s = step(
        s,
        Event::OutputDecided(pida_workflow::OutputDecision::Save { path: "/in/doc1_signed.pdf".into(), exists: false }),
        &ctx(),
    )
    .state;
    let t = step(s, Event::Saved(info("/in/doc1_signed.pdf")), &ctx());
    assert!(matches!(t.state, State::BatchOverview { .. }), "{:?}", t.state);
    assert_eq!(status(&t.state, 1), BatchItemStatus::Signed { output: info("/in/doc1_signed.pdf") });
    assert!(
        t.effects
            .iter()
            .any(|e| matches!(e, Effect::NotifyWith(c, p) if c == "ng-batch-item-signed" && p["file"] == "doc1.pdf"))
    );
    let q = t.state.queue().unwrap();
    assert!(q.done.iter().any(|o| o.doc.id == DocId(1)), "Ergebnisliste");
    // die Zusammenfassung zählt den einzeln signierten Eintrag (Lauf für den Rest)
    let mut d = Driver::new(started(t.state, BackendId::Keystore, &[1, 2], ExistingPolicy::KeepBoth));
    let State::BatchSigning { run, .. } = &d.state else { panic!() };
    assert_eq!(run.order, vec![DocId(2)], "signierte Einträge nie erneut");
    d.finish();
    let b = batch_of(&d.state);
    assert_eq!(b.items.iter().filter(|i| matches!(i.status, BatchItemStatus::Signed { .. })).count(), 2);
    assert!(b.last_run.is_some());
}

#[test]
fn opened_item_with_cancelled_save_dialog_returns_to_the_overview() {
    // einzeln signiert, Speichern-Dialog abgebrochen (kein Ausgabeordner): „Weiteres Dokument“ (Navigate Open)
    // führt zurück zur Übersicht, der Stapel bleibt, der Eintrag unverändert (Nachtrag 4)
    let s = overview(&[1, 2]);
    let s = step(s, Event::BatchItemOpened(DocId(1)), &ctx()).state;
    let s = step(s, analyzed(1, 3, None, false), &ctx()).state;
    let t = step(s, Event::SignRequested { backend: BackendId::Keystore, options: options(false, true) }, &ctx());
    let (s, _) = run(
        t.state,
        [
            Event::PhaseCompleted(SigningPhase::OpeningSession),
            Event::PhaseCompleted(SigningPhase::ReadingCertificate),
            Event::PhaseCompleted(SigningPhase::Preparing),
            Event::PhaseCompleted(SigningPhase::Signing),
            Event::PhaseCompleted(SigningPhase::Finishing),
        ],
        &ctx(),
    );
    let ask = pida_workflow::OutputDecision::Ask { directory: "/in".into(), file_name: "doc1_signed.pdf".into() };
    let s = step(s, Event::OutputDecided(ask), &ctx()).state;
    assert!(matches!(s, State::Output { saved: None, .. }), "{s:?}");
    let t = step(s, Event::Navigate(Step::Open), &ctx());
    assert!(matches!(t.state, State::BatchOverview { .. }), "{:?}", t.state);
    assert_eq!(batch_of(&t.state).items.len(), 2, "Stapel bleibt");
    assert_eq!(status(&t.state, 1), BatchItemStatus::Ready, "nichts gespeichert");
    assert_eq!(batch_of(&t.state).open, None);
}

#[test]
fn analysis_warning_from_the_overview() {
    let t = step(State::Idle { queue: Queue::default() }, Event::FilesAdded(vec![doc(1), doc(2)]), &ctx());
    let warned = DocSummary { warnings: vec!["pdf-usage-rights".into()], ..summary(1) };
    let s = step(t.state, Event::Preanalyzed { doc: DocId(1), result: Ok(warned) }, &ctx()).state;
    let s = step(s, Event::BatchItemOpened(DocId(1)), &ctx()).state;
    let Event::Analyzed { doc, class, placeholder, signed, page_count, start, fields, .. } =
        analyzed(1, 3, None, false)
    else {
        unreachable!()
    };
    let with_warning = Event::Analyzed {
        doc,
        class,
        placeholder,
        signed,
        page_count,
        start,
        warnings: vec!["pdf-usage-rights".into()],
        fields,
        new_visible_field_allowed: true,
    };
    let w = step(s, with_warning.clone(), &ctx()).state;
    assert!(matches!(w, State::AnalysisWarning { .. }));
    // „Abbrechen“ bzw. „Zurück“ ohne Bestätigung: bleibt zu prüfen
    for e in [Event::WarningAnswered(false), Event::BackToBatch, Event::Cancel] {
        let back = step(w.clone(), e, &ctx()).state;
        assert!(matches!(back, State::BatchOverview { .. }));
        assert!(matches!(status(&back, 1), BatchItemStatus::NeedsReview { .. }));
    }
    // „Fortfahren“ → Arbeitsbereich → zurück: bestätigt, bereit; erneutes Öffnen fragt nicht mehr
    let s = step(w, Event::WarningAnswered(true), &ctx()).state;
    assert!(matches!(s, State::Positioning { .. }));
    let s = step(s, Event::BackToBatch, &ctx()).state;
    assert_eq!(status(&s, 1), BatchItemStatus::Ready);
    let s = step(s, Event::BatchItemOpened(DocId(1)), &ctx()).state;
    assert!(matches!(step(s, with_warning, &ctx()).state, State::Positioning { .. }));
}

#[test]
fn signed_single_document_joins_the_overview_with_its_preanalysis() {
    // einzeln signiert, weitere warten (noch kein Stapel): „Zum Stapel“ legt den signierten Eintrag an – mit
    // Voranalyse für Miniatur, Seitenzahl und Größe der Zeile; der Status bleibt „Signiert“
    let s = step(State::Idle { queue: Queue::default() }, Event::FilesAdded(vec![doc(1)]), &ctx()).state;
    let s = step(s, analyzed(1, 3, None, false), &ctx()).state;
    let s = step(s, Event::FilesAdded(vec![doc(2)]), &ctx()).state;
    let t = step(s, Event::SignRequested { backend: BackendId::Keystore, options: options(false, true) }, &ctx());
    let (s, _) = run(
        t.state,
        [
            Event::PhaseCompleted(SigningPhase::OpeningSession),
            Event::PhaseCompleted(SigningPhase::ReadingCertificate),
            Event::PhaseCompleted(SigningPhase::Preparing),
            Event::PhaseCompleted(SigningPhase::Signing),
            Event::PhaseCompleted(SigningPhase::Finishing),
            Event::OutputDecided(pida_workflow::OutputDecision::Save {
                path: "/in/doc1_signed.pdf".into(),
                exists: false,
            }),
            Event::Saved(info("/in/doc1_signed.pdf")),
        ],
        &ctx(),
    );
    assert!(matches!(s, State::Output { saved: Some(_), .. }), "{s:?}");
    let t = step(s, Event::ShowBatch, &ctx());
    assert!(matches!(t.state, State::BatchOverview { .. }), "{:?}", t.state);
    assert!(matches!(status(&t.state, 1), BatchItemStatus::Signed { .. }));
    assert!(t.effects.contains(&Effect::Preanalyze(DocId(1))) && t.effects.contains(&Effect::Preanalyze(DocId(2))));
    let t = step(t.state, Event::Preanalyzed { doc: DocId(1), result: Ok(summary(3)) }, &ctx());
    assert!(matches!(status(&t.state, 1), BatchItemStatus::Signed { .. }), "bleibt signiert");
    assert_eq!(batch_of(&t.state).item(DocId(1)).unwrap().summary.as_ref().map(|s| s.page_count), Some(3));
}

#[test]
fn show_batch_from_a_single_document_with_waiting_documents() {
    // ein Dokument im Arbeitsbereich, weitere kommen dazu → Hinweis, „Zur Stapelübersicht“
    let s = step(State::Idle { queue: Queue::default() }, Event::FilesAdded(vec![doc(1)]), &ctx()).state;
    let s = step(s, analyzed(1, 3, None, false), &ctx()).state;
    let t = step(s, Event::FilesAdded(vec![doc(2), doc(3)]), &ctx());
    assert!(matches!(t.state, State::Positioning { .. }));
    assert!(
        t.effects.iter().any(|e| matches!(e, Effect::NotifyWith(c, p) if c == "ng-batch-added" && p["count"] == "2"))
    );
    let p = Placement { page_index: 0, x: 40.0, y: 300.0, width: 230.0, height: 49.0 };
    let s = step(t.state, Event::PlacementChanged(p), &ctx()).state;
    let t = step(s, Event::ShowBatch, &ctx());
    assert!(matches!(t.state, State::BatchOverview { .. }), "{:?}", t.state);
    assert_eq!(batch_of(&t.state).items.len(), 3);
    assert_eq!(batch_of(&t.state).item(DocId(1)).unwrap().plan, Some(PositionChoice::Manual(p)));
    assert!(t.effects.contains(&Effect::Preanalyze(DocId(2))) && t.effects.contains(&Effect::Discard(DocId(1))));
}

#[test]
fn resumed_document_of_an_interrupted_batch_returns_to_the_overview() {
    let plan = pida_workflow::ResumePlan { position: PositionChoice::Auto, backend: BackendId::AtrustNative };
    let t = step(
        State::Idle { queue: Queue::default() },
        Event::ResumeRequested { document: doc(5), plan, pending: vec![doc(6)] },
        &ctx(),
    );
    let s = step(t.state, analyzed(5, 2, None, false), &ctx()).state;
    let t = step(s, Event::BackendsProbed(options(false, false)), &ctx());
    assert!(matches!(t.state, State::Signing { .. }), "nur das unterbrochene Dokument wird fortgesetzt");
    assert_eq!(opens(&t.effects), 1);
    let (s, _) = run(
        t.state,
        [
            Event::PhaseCompleted(SigningPhase::OpeningSession),
            Event::PhaseCompleted(SigningPhase::ReadingCertificate),
            Event::PhaseCompleted(SigningPhase::Preparing),
            Event::PhaseCompleted(SigningPhase::Signing),
            Event::PhaseCompleted(SigningPhase::Finishing),
            Event::OutputDecided(pida_workflow::OutputDecision::Save {
                path: "/docs/doc5_signed.pdf".into(),
                exists: false,
            }),
        ],
        &ctx(),
    );
    let t = step(s, Event::Saved(info("/docs/doc5_signed.pdf")), &ctx());
    assert!(matches!(t.state, State::BatchOverview { .. }), "{:?}", t.state);
    assert!(matches!(status(&t.state, 5), BatchItemStatus::Signed { .. }));
    assert_eq!(status(&t.state, 6), BatchItemStatus::Analyzing, "die übrigen: neue Voranalyse");
    // auch der fortgesetzte bekommt seine Voranalyse – nur für Miniatur, Seitenzahl und Größe der Zeile
    // (Mobile-Check 16.10.2026: die Zeile blieb ohne sie); sein Status bleibt „Signiert“
    assert!(t.effects.contains(&Effect::Preanalyze(DocId(5))) && t.effects.contains(&Effect::Preanalyze(DocId(6))));
    assert_eq!(t.effects.iter().filter(|e| matches!(e, Effect::Preanalyze(_))).count(), 2);
    assert_eq!(opens(&t.effects), 0, "der Lauf wird nicht automatisch fortgesetzt");
    let t = step(t.state, Event::Preanalyzed { doc: DocId(5), result: Ok(summary(2)) }, &ctx());
    assert!(matches!(status(&t.state, 5), BatchItemStatus::Signed { .. }), "bleibt signiert");
    assert_eq!(batch_of(&t.state).item(DocId(5)).unwrap().summary.as_ref().map(|s| s.page_count), Some(2));
    assert_eq!(opens(&t.effects), 0);
}

// ------------------------------------------------------------------------------------------------
// „PDF hinzufügen“ (ADR 0004 Nachtrag 9): „Weitere PDFs hinzufügen“, „Anderes PDF öffnen“
// ------------------------------------------------------------------------------------------------

/// Arbeitsbereich mit Dokument `id` (eben analysiert, automatisch platziert).
fn workspace_with(id: u64, c: &Context) -> State {
    let s = step(State::Idle { queue: Queue::default() }, Event::FilesAdded(vec![doc(id)]), c).state;
    let s = step(s, analyzed(id, 3, None, false), c).state;
    assert!(matches!(s, State::Positioning { edit: Some(_), .. }), "{}", s.name());
    s
}

fn notified(effects: &[Effect], code: &str) -> bool {
    effects.iter().any(|e| matches!(e, Effect::NotifyWith(c, _) if c == code))
}

#[test]
fn more_files_in_the_workspace_become_a_batch_with_the_placement_as_plan() {
    let s = workspace_with(1, &ctx());
    let p = Placement { page_index: 0, x: 40.0, y: 300.0, width: 230.0, height: 49.0 };
    let s = step(s, Event::PlacementChanged(p), &ctx()).state;
    let t = step(s, Event::MoreFilesAdded(vec![doc(2), doc(3)]), &ctx());
    assert!(matches!(t.state, State::BatchOverview { .. }), "{}", t.state.name());
    let b = batch_of(&t.state);
    assert_eq!(b.items.iter().map(|i| i.doc.0).collect::<Vec<_>>(), vec![1, 2, 3], "alle zusammen");
    assert_eq!(b.item(DocId(1)).unwrap().plan, Some(PositionChoice::Manual(p)), "Platzierung bleibt Plan");
    assert!(b.open.is_none());
    assert!(t.effects.contains(&Effect::Discard(DocId(1))));
    assert!(t.effects.contains(&Effect::Preanalyze(DocId(2))) && t.effects.contains(&Effect::Preanalyze(DocId(3))));
    assert!(!notified(&t.effects, "ng-batch-added"), "kein Hinweis – die Übersicht zeigt sie");
    assert_eq!(opens(&t.effects), 0, "startet nichts (A1)");
    // automatisch platziert: kein eigener Plan, folgt den Einstellungen
    let t = step(workspace_with(1, &ctx()), Event::MoreFilesAdded(vec![doc(2)]), &ctx());
    let item = batch_of(&t.state).item(DocId(1)).cloned().unwrap();
    assert!(item.plan.is_none() && item.auto_plan, "{item:?}");
}

#[test]
fn more_files_that_are_already_open_change_nothing() {
    let s = workspace_with(1, &ctx());
    let same = pida_workflow::InputDocument::new(DocId(9), "/in/doc1.pdf");
    let t = step(s.clone(), Event::MoreFilesAdded(vec![same]), &ctx());
    assert_eq!(t.state, s);
    assert!(t.effects.is_empty(), "{:?}", t.effects);
}

#[test]
fn more_files_in_an_opened_item_return_to_the_overview_with_the_new_entries() {
    let s = step(overview(&[1, 2]), Event::BatchItemOpened(DocId(1)), &ctx()).state;
    let s = step(s, analyzed(1, 3, None, false), &ctx()).state;
    assert!(s.opened_from_batch());
    let t = step(s, Event::MoreFilesAdded(vec![doc(3)]), &ctx());
    assert!(matches!(t.state, State::BatchOverview { .. }), "{}", t.state.name());
    assert_eq!(batch_of(&t.state).items.len(), 3);
    assert_eq!(status(&t.state, 3), BatchItemStatus::Analyzing);
    assert_eq!(status(&t.state, 1), BatchItemStatus::Ready);
    assert!(t.effects.contains(&Effect::Preanalyze(DocId(3))));
}

#[test]
fn more_files_outside_the_workspace_or_without_overview_behave_like_files_added() {
    // Analysewarnung: an die Warteschlange mit Hinweis (wie `FilesAdded`)
    let s = step(State::Idle { queue: Queue::default() }, Event::FilesAdded(vec![doc(1)]), &ctx()).state;
    let mut warning = analyzed(1, 3, None, false);
    if let Event::Analyzed { warnings, .. } = &mut warning {
        warnings.push("pdf-usage-rights".into());
    }
    let s = step(s, warning, &ctx()).state;
    assert!(matches!(s, State::AnalysisWarning { .. }));
    let a = step(s.clone(), Event::MoreFilesAdded(vec![doc(2)]), &ctx());
    let b = step(s, Event::FilesAdded(vec![doc(2)]), &ctx());
    assert_eq!((a.state, a.effects), (b.state, b.effects));
    // Dokumentauswahl: wie `FilesAdded` (zwei → Übersicht)
    let idle = || State::Idle { queue: Queue::default() };
    let a = step(idle(), Event::MoreFilesAdded(vec![doc(1), doc(2)]), &ctx());
    assert_eq!(a, step(idle(), Event::FilesAdded(vec![doc(1), doc(2)]), &ctx()));
    // ohne Stapelübersicht: an die Warteschlange, der Arbeitsbereich bleibt
    let single = Context { workspace: true, ..Context::default() };
    let t = step(workspace_with(1, &single), Event::MoreFilesAdded(vec![doc(2)]), &single);
    let State::Positioning { queue, .. } = &t.state else { panic!("{}", t.state.name()) };
    assert_eq!(queue.pending.iter().map(|d| d.id.0).collect::<Vec<_>>(), vec![2]);
}

#[test]
fn replace_closes_the_document_and_opens_the_chosen_one() {
    let s = workspace_with(1, &ctx());
    let p = Placement { page_index: 0, x: 40.0, y: 300.0, width: 230.0, height: 49.0 };
    let s = step(s, Event::PlacementChanged(p), &ctx()).state;
    let t = step(s, Event::DocumentReplaced(vec![doc(2)]), &ctx());
    let State::Analyzing { queue } = &t.state else { panic!("{}", t.state.name()) };
    assert_eq!(queue.current_id(), Some(DocId(2)));
    assert!(queue.pending.is_empty() && queue.batch.is_none());
    assert_eq!(queue.done.len(), 1);
    assert_eq!((queue.done[0].doc.id, &queue.done[0].result), (DocId(1), &pida_workflow::DocResult::Cancelled));
    assert_eq!(t.effects.first(), Some(&Effect::Discard(DocId(1))));
    assert!(t.effects.contains(&Effect::Analyze(DocId(2))));
    assert_eq!(opens(&t.effects), 0, "startet nichts (A1)");
    // dieselbe Datei erneut: frisch geöffnet (eigene Kennung, Platzierung neu)
    let s = workspace_with(1, &ctx());
    let same = pida_workflow::InputDocument::new(DocId(9), "/in/doc1.pdf");
    let t = step(s, Event::DocumentReplaced(vec![same]), &ctx());
    assert!(matches!(&t.state, State::Analyzing { queue } if queue.current_id() == Some(DocId(9))));
}

#[test]
fn replace_with_waiting_documents_keeps_them() {
    // mit Stapelübersicht: das neue und das wartende Dokument → Übersicht (ohne das geschlossene)
    let s = workspace_with(1, &ctx());
    let s = step(s, Event::FilesAdded(vec![doc(2)]), &ctx()).state;
    let t = step(s, Event::DocumentReplaced(vec![doc(3)]), &ctx());
    assert!(matches!(t.state, State::BatchOverview { .. }), "{}", t.state.name());
    assert_eq!(batch_of(&t.state).items.iter().map(|i| i.doc.0).collect::<Vec<_>>(), vec![2, 3]);
    assert!(t.effects.contains(&Effect::Discard(DocId(1))));
    // ohne Stapelübersicht: das neue wird geöffnet, das wartende wartet weiter
    let single = Context { workspace: true, ..Context::default() };
    let s = step(workspace_with(1, &single), Event::FilesAdded(vec![doc(2)]), &single).state;
    let t = step(s, Event::DocumentReplaced(vec![doc(3)]), &single);
    let State::Analyzing { queue } = &t.state else { panic!("{}", t.state.name()) };
    assert_eq!(queue.current_id(), Some(DocId(3)));
    assert_eq!(queue.pending.iter().map(|d| d.id.0).collect::<Vec<_>>(), vec![2]);
}

#[test]
fn replace_in_an_opened_item_replaces_the_entry() {
    let s = step(overview(&[1, 2]), Event::BatchItemOpened(DocId(1)), &ctx()).state;
    let s = step(s, analyzed(1, 3, None, false), &ctx()).state;
    let t = step(s, Event::DocumentReplaced(vec![doc(3)]), &ctx());
    assert!(matches!(t.state, State::BatchOverview { .. }), "{}", t.state.name());
    assert_eq!(batch_of(&t.state).items.iter().map(|i| i.doc.0).collect::<Vec<_>>(), vec![2, 3]);
    assert!(t.effects.contains(&Effect::Discard(DocId(1))) && t.effects.contains(&Effect::Preanalyze(DocId(3))));
    assert!(t.state.queue().unwrap().find(DocId(1)).is_none(), "entfernt, nicht verbucht");
    // einziger Eintrag: der Stapel entfällt, das neue Dokument öffnet im Arbeitsbereich
    let s = step(overview(&[1, 2]), Event::BatchItemRemoved(DocId(2)), &ctx()).state;
    let s = step(s, Event::BatchItemOpened(DocId(1)), &ctx()).state;
    let s = step(s, analyzed(1, 3, None, false), &ctx()).state;
    let t = step(s, Event::DocumentReplaced(vec![doc(3)]), &ctx());
    let State::Analyzing { queue } = &t.state else { panic!("{}", t.state.name()) };
    assert_eq!(queue.current_id(), Some(DocId(3)));
    assert!(queue.batch.is_none());
}

#[test]
fn replace_outside_the_workspace_behaves_like_files_added() {
    // im Stapellauf: nie ein Dokument des Laufs schließen – an die Übersicht wie `FilesAdded`
    let mut d = Driver::new(started(overview(&[1, 2]), BackendId::Keystore, &[1, 2], ExistingPolicy::KeepBoth));
    d.until(1, BatchPhase::Confirming);
    let a = step(d.state.clone(), Event::DocumentReplaced(vec![doc(3)]), &ctx());
    let b = step(d.state.clone(), Event::FilesAdded(vec![doc(3)]), &ctx());
    assert_eq!(a, b);
    // Stapelübersicht und Dokumentauswahl: wie `FilesAdded`
    let a = step(overview(&[1, 2]), Event::DocumentReplaced(vec![doc(3)]), &ctx());
    assert_eq!(a, step(overview(&[1, 2]), Event::FilesAdded(vec![doc(3)]), &ctx()));
    // ohne Dateien: unverändert
    let s = workspace_with(1, &ctx());
    let t = step(s.clone(), Event::DocumentReplaced(Vec::new()), &ctx());
    assert_eq!((t.state, t.effects.len()), (s, 0));
}

// ------------------------------------------------------------------------------------------------
// Lauf
// ------------------------------------------------------------------------------------------------

/// Eine Sitzung, höchstens ein Zertifikat (vor dem ersten sichtbaren Block), je Dokument eine Bestätigung.
#[test]
fn run_with_one_session_one_certificate_and_one_confirmation_per_document() {
    let mut d =
        Driver::new(started(overview(&[1, 2, 3]), BackendId::AtrustNative, &[1, 2, 3], ExistingPolicy::KeepBoth));
    assert_eq!(status(&d.state, 1), BatchItemStatus::Queued);
    d.until(2, BatchPhase::Confirming);
    assert_eq!(status(&d.state, 2), BatchItemStatus::Active);
    assert!(matches!(status(&d.state, 1), BatchItemStatus::Signed { .. }));
    assert_eq!(status(&d.state, 3), BatchItemStatus::Queued);
    let State::BatchSigning { run, .. } = &d.state else { panic!() };
    assert_eq!((run.index, run.confirmations_done, run.certificate_doc), (1, 1, Some(DocId(1))));
    d.finish();
    assert_eq!(opens(&d.effects), 1, "eine Sitzung");
    assert_eq!(count(&d.effects, |e| *e == Effect::ReadCertificate), 1, "höchstens ein Zertifikat");
    assert_eq!(count(&d.effects, |e| *e == Effect::SignDocument), 3, "je Dokument eine Bestätigung");
    assert_eq!(count(&d.effects, |e| matches!(e, Effect::Write(_))), 3);
    assert_eq!(count(&d.effects, |e| *e == Effect::CloseSession), 1);
    assert!(!d.effects.contains(&Effect::CancelSession));
    assert!(d.effects.iter().any(
        |e| matches!(e, Effect::NotifyWith(c, p) if c == "ng-batch-done" && p["signed"] == "3" && p["total"] == "3")
    ));
    let State::BatchOverview { queue } = &d.state else { panic!("{:?}", d.state) };
    let last = queue.batch.as_ref().unwrap().last_run.clone().unwrap();
    assert_eq!((last.total, last.signed, last.aborted), (3, 3, false));
    assert_eq!(last.folder.as_deref(), Some(std::path::Path::new("/out")));
    assert_eq!(queue.done.len(), 3);
}

/// Das Zertifikat wird vor dem ersten Dokument **mit sichtbarem Block** gelesen, nicht zwingend vor dem
/// ersten Dokument.
#[test]
fn certificate_is_read_before_the_first_visible_document() {
    let mut s = overview(&[1, 2]);
    if let Some(i) = s.queue_mut().and_then(|q| q.batch.as_mut()).and_then(|b| b.item_mut(DocId(1))) {
        // Plan der Nutzerin (im Arbeitsbereich „unsichtbar“ gewählt)
        i.plan = Some(PositionChoice::Invisible);
        i.auto_plan = false;
        i.status = BatchItemStatus::Positioned;
    }
    let mut d = Driver::new(started(s, BackendId::AtrustNative, &[1, 2], ExistingPolicy::KeepBoth));
    d.until(1, BatchPhase::Preparing);
    assert_eq!(count(&d.effects, |e| *e == Effect::ReadCertificate), 0, "unsichtbar: kein Zertifikat");
    d.until(2, BatchPhase::ReadingCertificate);
    d.finish();
    assert_eq!(count(&d.effects, |e| *e == Effect::ReadCertificate), 1);
}

#[test]
fn skip_keeps_the_session_and_abort_cancels_it() {
    let mut d =
        Driver::new(started(overview(&[1, 2, 3]), BackendId::AtrustNative, &[1, 2, 3], ExistingPolicy::KeepBoth));
    d.until(2, BatchPhase::Confirming);
    let e = d.send(Event::BatchSkip);
    assert_eq!(e, vec![Effect::SkipCurrentJob], "nur den Auftrag abbrechen");
    assert_eq!(d.current(), 2, "wartet auf das Ende des Auftrags");
    let e = d.send(Event::BackendFailed(BackendError::Cancelled));
    assert_eq!(status(&d.state, 2), BatchItemStatus::Skipped { existing: false });
    assert!(e.contains(&Effect::Notify("ng-batch-pending-app-request".into())));
    assert!(!e.contains(&Effect::CancelSession) && !e.contains(&Effect::CloseSession));
    assert_eq!(d.current(), 3);
    assert_eq!(opens(&e), 0, "dieselbe Sitzung");
    d.finish();
    assert_eq!(opens(&d.effects), 1);
    let s = batch_of(&d.state);
    assert_eq!(s.items.iter().filter(|i| matches!(i.status, BatchItemStatus::Signed { .. })).count(), 2);

    // „Stapel abbrechen“ beim zweiten Dokument: Sitzung abgebrochen (F-72), Rest bleibt bereit
    let mut d =
        Driver::new(started(overview(&[1, 2, 3]), BackendId::AtrustNative, &[1, 2, 3], ExistingPolicy::KeepBoth));
    d.until(2, BatchPhase::Confirming);
    let e = d.send(Event::BatchAbort);
    assert!(e.contains(&Effect::CancelSession) && e.contains(&Effect::DeletePersisted));
    assert!(e.iter().any(|e| matches!(e, Effect::NotifyWith(c, p) if c == "ng-batch-aborted" && p["signed"] == "1")));
    assert!(matches!(d.state, State::BatchOverview { .. }));
    assert!(matches!(status(&d.state, 1), BatchItemStatus::Signed { .. }), "Gespeichertes bleibt");
    assert_eq!(status(&d.state, 2), BatchItemStatus::Cancelled);
    assert_eq!(status(&d.state, 3), BatchItemStatus::Ready);
    assert!(batch_of(&d.state).last_run.as_ref().unwrap().aborted);
    // Esc im Lauf bedeutet dasselbe
    let mut d = Driver::new(started(overview(&[1, 2]), BackendId::AtrustNative, &[1, 2], ExistingPolicy::KeepBoth));
    d.until(1, BatchPhase::Loading);
    assert!(d.send(Event::Cancel).contains(&Effect::CancelSession));
    assert_eq!(status(&d.state, 1), BatchItemStatus::Cancelled);
}

#[test]
fn abort_while_writing_keeps_the_saved_result() {
    // „Stapel abbrechen“, während das Ergebnis des zweiten Dokuments geschrieben wird: das Schreiben läuft
    // zu Ende (ohne Epoche) – die Datei liegt auf der Platte und zählt als signiert
    let mut d =
        Driver::new(started(overview(&[1, 2, 3]), BackendId::AtrustNative, &[1, 2, 3], ExistingPolicy::KeepBoth));
    d.until(2, BatchPhase::Writing);
    let e = d.ok();
    assert!(e.iter().any(|e| matches!(e, Effect::Write(_))), "Schreiben angestoßen: {e:?}");
    let e = d.send(Event::BatchAbort);
    assert!(e.contains(&Effect::CancelSession));
    assert_eq!(status(&d.state, 2), BatchItemStatus::Cancelled);
    d.send(Event::Saved(info("/out/doc2_signed.pdf")));
    assert!(matches!(d.state, State::BatchOverview { .. }));
    assert!(matches!(status(&d.state, 2), BatchItemStatus::Signed { .. }), "gespeichert ist gespeichert");
    assert_eq!(status(&d.state, 3), BatchItemStatus::Ready);
    assert_eq!(batch_of(&d.state).last_run.as_ref().unwrap().signed, 2);
    assert_eq!(batch_of(&d.state).writing, None);
    assert!(d.state.queue().unwrap().done.iter().any(|o| o.doc.id == DocId(2)));
    // ein späteres `Saved` ohne laufendes Schreiben ändert nichts
    d.send(Event::Saved(info("/out/fremd.pdf")));
    assert_eq!(status(&d.state, 3), BatchItemStatus::Ready);

    // Abbruch vor der Zielentscheidung: es wird nichts geschrieben, der Eintrag bleibt „Abgebrochen“
    let mut d =
        Driver::new(started(overview(&[1, 2, 3]), BackendId::AtrustNative, &[1, 2, 3], ExistingPolicy::KeepBoth));
    d.until(2, BatchPhase::Writing);
    d.send(Event::BatchAbort);
    d.send(Event::BatchOutputDecided { doc: DocId(2), path: "/out/doc2_signed.pdf".into(), exists: false });
    assert_eq!(batch_of(&d.state).writing, None);
    d.send(Event::Saved(info("/out/doc2_signed.pdf")));
    assert_eq!(status(&d.state, 2), BatchItemStatus::Cancelled);
    // Schreiben fehlgeschlagen: bleibt „Abgebrochen“
    let mut d = Driver::new(started(overview(&[1, 2]), BackendId::AtrustNative, &[1, 2], ExistingPolicy::KeepBoth));
    d.until(2, BatchPhase::Writing);
    d.ok();
    d.send(Event::BatchAbort);
    d.send(Event::SaveFailed("voll".into()));
    assert_eq!(status(&d.state, 2), BatchItemStatus::Cancelled);
    assert_eq!(batch_of(&d.state).writing, None);
}

#[test]
fn skip_in_phases_without_a_backend_job_waits_for_the_step() {
    let mut d = Driver::new(started(overview(&[1, 2]), BackendId::Keystore, &[1, 2], ExistingPolicy::KeepBoth));
    d.until(1, BatchPhase::Preparing);
    d.send(Event::BatchSkip);
    // das vorbereitete Dokument kommt zurück → verworfen, nächstes Dokument
    let e = d.send(Event::PhaseCompleted(SigningPhase::Preparing));
    assert!(e.contains(&Effect::ReleaseDocument));
    assert!(!e.contains(&Effect::SignDocument));
    assert_eq!(status(&d.state, 1), BatchItemStatus::Skipped { existing: false });
    assert!(!e.contains(&Effect::Notify("ng-batch-pending-app-request".into())), "keine App-Anfrage");
    // während des Öffnens der Sitzung gibt es nichts zu überspringen
    let mut d = Driver::new(started(overview(&[1, 2]), BackendId::Keystore, &[1, 2], ExistingPolicy::KeepBoth));
    assert!(d.send(Event::BatchSkip).is_empty());
    // gespeichert ist gespeichert
    let mut d = Driver::new(started(overview(&[1, 2]), BackendId::Keystore, &[1, 2], ExistingPolicy::KeepBoth));
    d.until(1, BatchPhase::Writing);
    d.ok();
    d.send(Event::BatchSkip);
    d.send(Event::Saved(info("/out/doc1_signed.pdf")));
    assert!(matches!(status(&d.state, 1), BatchItemStatus::Signed { .. }));
}

#[test]
fn connection_error_pauses_and_retries_in_the_same_session() {
    let mut d = Driver::new(started(overview(&[1, 2]), BackendId::AtrustNative, &[1, 2], ExistingPolicy::KeepBoth));
    d.until(2, BatchPhase::Confirming);
    let e = d.send(Event::BackendFailed(BackendError::Network("weg".into())));
    assert!(!e.contains(&Effect::CloseSession) && !e.contains(&Effect::CancelSession), "Sitzung bleibt");
    assert!(matches!(d.pause(), Some(BatchPause::Session { new_session: false, doc: Some(DocId(2)), .. })));
    // nie automatisch weiter
    for e in [Event::PhaseCompleted(SigningPhase::Signing), analyzed(2, 3, None, false)] {
        assert!(d.send(e).is_empty());
        assert!(d.pause().is_some());
    }
    let e = d.send(Event::Retry);
    assert_eq!(opens(&e), 0, "ohne neue Sitzung");
    assert!(matches!(e.as_slice(), [Effect::Prepare { doc: DocId(2), .. }]), "{e:?}");
    d.finish();
    assert_eq!(opens(&d.effects), 1);
}

#[test]
fn session_error_pauses_and_retry_opens_a_new_session() {
    let mut d = Driver::new(started(overview(&[1, 2]), BackendId::AtrustNative, &[1, 2], ExistingPolicy::KeepBoth));
    d.until(2, BatchPhase::Confirming);
    let e = d.send(Event::BackendFailed(BackendError::UnknownPage { diagnostic: "x".into() }));
    assert!(e.contains(&Effect::CloseSession));
    assert!(matches!(d.pause(), Some(BatchPause::Session { new_session: true, doc: Some(DocId(2)), .. })));
    let e = d.send(Event::Retry);
    assert_eq!(e, vec![Effect::OpenSession(BackendId::AtrustNative)], "neue Sitzung (ggf. neue Anmeldung)");
    // neue Sitzung: das Zertifikat wird erneut gelesen, weiter beim aktuellen Dokument
    let e = d.send(Event::PhaseCompleted(SigningPhase::OpeningSession));
    assert_eq!(e, vec![Effect::ReadCertificate]);
    assert_eq!(d.current(), 2);
    d.finish();
    assert_eq!(opens(&d.effects), 2);
    assert!(matches!(status(&d.state, 2), BatchItemStatus::Signed { .. }));

    // Sitzungsfehler vor dem ersten Dokument: Pause ohne Dokument, Überspringen nicht möglich
    let mut d = Driver::new(started(overview(&[1, 2]), BackendId::Keystore, &[1, 2], ExistingPolicy::KeepBoth));
    d.send(Event::BackendFailed(BackendError::Keystore("x".into())));
    assert!(matches!(d.pause(), Some(BatchPause::Session { doc: None, new_session: true, .. })));
    assert!(d.send(Event::BatchSkip).is_empty());
    assert!(d.pause().is_some());
    // Überspringen in einer Sitzungspause mit Dokument: nächstes Dokument wartet weiter auf eine neue Sitzung
    let mut d =
        Driver::new(started(overview(&[1, 2, 3]), BackendId::AtrustNative, &[1, 2, 3], ExistingPolicy::KeepBoth));
    d.until(1, BatchPhase::Confirming);
    d.send(Event::BackendFailed(BackendError::Protocol("x".into())));
    let e = d.send(Event::BatchSkip);
    assert_eq!(opens(&e), 0, "A1: neue Sitzung nur nach „Erneut versuchen“");
    assert_eq!(status(&d.state, 1), BatchItemStatus::Skipped { existing: false });
    assert!(matches!(d.pause(), Some(BatchPause::Session { new_session: true, doc: Some(DocId(2)), .. })));
    assert_eq!(d.send(Event::Retry), vec![Effect::OpenSession(BackendId::AtrustNative)]);
}

#[test]
fn declined_in_the_app_pauses_the_document_never_automatically_on() {
    let mut d = Driver::new(started(overview(&[1, 2]), BackendId::AtrustNative, &[1, 2], ExistingPolicy::KeepBoth));
    d.until(1, BatchPhase::Confirming);
    let e = d.send(Event::SigningDeclined);
    assert!(e.contains(&Effect::ReleaseDocument) && !e.contains(&Effect::CancelSession), "Zugangsdaten bleiben");
    let Some(BatchPause::Document { doc, error, retry, .. }) = d.pause() else { panic!("{:?}", d.pause()) };
    assert_eq!((doc, error.code(), retry), (DocId(1), "ng-signing-declined".to_owned(), RetryTarget::Prepare));
    assert!(d.send(Event::PhaseCompleted(SigningPhase::Signing)).is_empty(), "nie automatisch weiter");
    // „Erneut anfordern“: neue Bestätigung in derselben Sitzung
    let e = d.send(Event::Retry);
    assert!(matches!(e.as_slice(), [Effect::Prepare { doc: DocId(1), .. }]));
    assert_eq!(status(&d.state, 1), BatchItemStatus::Active);
    d.until(1, BatchPhase::Confirming);
    d.send(Event::SigningDeclined);
    // „Überspringen“ in der Pause
    let e = d.send(Event::BatchSkip);
    assert!(e.contains(&Effect::Analyze(DocId(2))));
    assert_eq!(status(&d.state, 1), BatchItemStatus::Skipped { existing: false });
}

#[test]
fn document_errors_fail_the_document_or_pause_when_retryable() {
    // nicht wiederholbar: Fehler, der Lauf geht weiter
    let mut d = Driver::new(started(overview(&[1, 2]), BackendId::AtrustNative, &[1, 2], ExistingPolicy::KeepBoth));
    d.until(1, BatchPhase::Confirming);
    let e = d.send(Event::BackendFailed(BackendError::DocumentTooLarge));
    assert!(
        matches!(status(&d.state, 1), BatchItemStatus::Failed { error } if error.code() == "backend-document-too-large")
    );
    assert!(e.contains(&Effect::Analyze(DocId(2))) && d.pause().is_none());
    for e in [
        Event::SigningFailed(WorkflowError::Prepare { code: "sigblock-x".into(), detail: String::new() }),
        Event::SelfCheckFailed(pida_pades::InvariantViolation {
            invariant: pida_pades::Invariant::I1Prefix,
            detail: "x".into(),
        }),
    ] {
        let mut d = Driver::new(started(overview(&[1, 2]), BackendId::Keystore, &[1, 2], ExistingPolicy::KeepBoth));
        d.until(1, BatchPhase::Finishing);
        d.send(e);
        assert!(matches!(status(&d.state, 1), BatchItemStatus::Failed { .. }));
        assert_eq!(d.current(), 2);
    }
    // wiederholbar: SL-Fehler, „CMS zu groß“ (mit größerer Reserve), Speichern, Zertifikats-Cache
    let mut d = Driver::new(started(overview(&[1, 2]), BackendId::AtrustNative, &[1, 2], ExistingPolicy::KeepBoth));
    d.until(1, BatchPhase::Confirming);
    d.send(Event::BackendFailed(BackendError::SecurityLayer { code: 6002, info: "x".into() }));
    assert!(matches!(d.pause(), Some(BatchPause::Document { .. })));
    d.send(Event::Retry);
    d.until(1, BatchPhase::Finishing);
    d.send(Event::CmsTooLarge { needed: 20_000, reserve: 16_384 });
    let Some(BatchPause::Document { retry: RetryTarget::PrepareWithReserve(n), .. }) = d.pause() else { panic!() };
    let e = d.send(Event::Retry);
    assert!(matches!(e.as_slice(), [Effect::Prepare { min_reserve: Some(m), .. }] if *m == n));
    d.until(1, BatchPhase::Writing);
    d.ok();
    d.send(Event::SaveFailed("voll".into()));
    let Some(BatchPause::Document { retry: RetryTarget::Save, .. }) = d.pause() else { panic!("{:?}", d.pause()) };
    let e = d.send(Event::Retry);
    assert!(matches!(e.as_slice(), [Effect::DecideBatchOutput { doc: DocId(1), .. }]), "{e:?}");
    d.ok();
    d.send(Event::Saved(info("/out/doc1_signed.pdf")));
    d.until(2, BatchPhase::Finishing);
    d.send(Event::SigningFailed(WorkflowError::Prepare {
        code: "ng-certcache-mismatch".into(),
        detail: String::new(),
    }));
    assert!(matches!(d.pause(), Some(BatchPause::Document { resume: BatchPhase::ReadingCertificate, .. })));
    assert_eq!(d.send(Event::Retry), vec![Effect::ReadCertificate], "Zertifikat neu lesen, gleiche Sitzung");
    d.finish();
    assert_eq!(opens(&d.effects), 1);
}

#[test]
fn existing_targets_skip_without_confirmation_keep_both_or_overwrite() {
    // „Überspringen“: Ziel vor dem Signieren geprüft – keine Bestätigung
    let mut d = Driver::new(started(overview(&[1, 2]), BackendId::Keystore, &[1, 2], ExistingPolicy::Skip));
    let e = d.send(Event::PhaseCompleted(SigningPhase::OpeningSession));
    assert!(matches!(
        e.as_slice(),
        [Effect::DecideBatchOutput { doc: DocId(1), on_existing: ExistingPolicy::Skip, .. }]
    ));
    let e = d.send(Event::BatchOutputDecided { doc: DocId(1), path: "/out/doc1_signed.pdf".into(), exists: true });
    assert_eq!(status(&d.state, 1), BatchItemStatus::Skipped { existing: true });
    assert!(e.iter().any(|e| matches!(e, Effect::DecideBatchOutput { doc: DocId(2), .. })));
    d.finish();
    assert_eq!(count(&d.effects, |e| *e == Effect::SignDocument), 1, "nur das zweite Dokument");
    // beim Schreiben gilt dann „Beide behalten“
    assert!(
        d.effects.iter().any(|e| matches!(
            e,
            Effect::DecideBatchOutput { doc: DocId(2), on_existing: ExistingPolicy::KeepBoth, .. }
        ))
    );
    // „Beide behalten“: Pfad mit Nummer aus core-api wird geschrieben (nie ersetzt)
    let mut d = Driver::new(started(overview(&[1, 2]), BackendId::Keystore, &[1, 2], ExistingPolicy::KeepBoth));
    d.until(1, BatchPhase::Writing);
    let e = d.send(Event::BatchOutputDecided { doc: DocId(1), path: "/out/doc1_signed (2).pdf".into(), exists: true });
    assert_eq!(e, vec![Effect::Write("/out/doc1_signed (2).pdf".into())]);
    // „Überschreiben“
    let mut d = Driver::new(started(overview(&[1, 2]), BackendId::Keystore, &[1, 2], ExistingPolicy::Overwrite));
    d.until(1, BatchPhase::Writing);
    let e = d.send(Event::BatchOutputDecided { doc: DocId(1), path: "/out/doc1_signed.pdf".into(), exists: true });
    assert_eq!(e, vec![Effect::Overwrite("/out/doc1_signed.pdf".into())]);
}

#[test]
fn certified_documents_and_analysis_warnings_in_the_run() {
    // DocMDP-zertifiziert, automatisch (neues sichtbares Feld): Fehler, weiter
    let mut d = Driver::new(started(overview(&[1, 2]), BackendId::Keystore, &[1, 2], ExistingPolicy::KeepBoth));
    d.until(1, BatchPhase::Loading);
    let Event::Analyzed { doc, class, placeholder, signed, page_count, start, fields, warnings, .. } =
        analyzed(1, 3, None, false)
    else {
        unreachable!()
    };
    d.send(Event::Analyzed {
        doc,
        class,
        placeholder,
        signed,
        page_count,
        start,
        warnings,
        fields,
        new_visible_field_allowed: false,
    });
    assert!(
        matches!(status(&d.state, 1), BatchItemStatus::Failed { error } if error.code() == "ng-constraint-certified")
    );
    // Warnung im Lauf ohne Bestätigung: nie durchwinken
    let mut d = Driver::new(started(overview(&[1, 2]), BackendId::Keystore, &[1, 2], ExistingPolicy::KeepBoth));
    d.until(1, BatchPhase::Loading);
    let Event::Analyzed { doc, class, placeholder, signed, page_count, start, fields, .. } =
        analyzed(1, 3, None, false)
    else {
        unreachable!()
    };
    let e = d.send(Event::Analyzed {
        doc,
        class,
        placeholder,
        signed,
        page_count,
        start,
        warnings: vec!["pdf-usage-rights".into()],
        fields,
        new_visible_field_allowed: true,
    });
    assert!(matches!(status(&d.state, 1), BatchItemStatus::NeedsReview { .. }));
    assert!(!e.contains(&Effect::SignDocument));
    // QR-Platzhalter ohne Plan: im Lauf der Platzhalter (Platzhaltersuche)
    let c = Context { placeholder_search: true, ..ctx() };
    let t = step(
        started(overview(&[1, 2]), BackendId::Keystore, &[1, 2], ExistingPolicy::KeepBoth).state,
        Event::PhaseCompleted(SigningPhase::OpeningSession),
        &c,
    );
    let t = step(t.state, analyzed(1, 3, Some(PlaceholderFound::Qr(qr())), false), &c);
    let State::BatchSigning { run, .. } = &t.state else { panic!() };
    assert!(matches!(run.position, Some(PositionChoice::Placeholder(_))));
}

#[test]
fn quit_while_running_cancels_the_session() {
    let mut d = Driver::new(started(overview(&[1, 2]), BackendId::AtrustNative, &[1, 2], ExistingPolicy::KeepBoth));
    d.until(1, BatchPhase::Confirming);
    let e = d.send(Event::Quit);
    assert!(e.contains(&Effect::CancelSession));
    assert!(matches!(d.state, State::Finished { .. }));
    // „Start“ aus der Übersicht verwirft den Stapel
    let t = step(overview(&[1, 2]), Event::Navigate(Step::Open), &ctx());
    assert!(matches!(&t.state, State::Idle { queue } if queue.batch.is_none() && queue.is_empty()));
}

#[test]
fn target_of_moves_in_the_workspace() {
    // Feld als Plan
    let s = overview(&[1, 2]);
    let s = step(s, Event::BatchItemOpened(DocId(1)), &ctx()).state;
    let Event::Analyzed { doc, class, placeholder, signed, page_count, start, warnings, .. } =
        analyzed(1, 3, None, false)
    else {
        unreachable!()
    };
    let s = step(
        s,
        Event::Analyzed {
            doc,
            class,
            placeholder,
            signed,
            page_count,
            start,
            warnings,
            fields: vec![field("Sig1")],
            new_visible_field_allowed: true,
        },
        &ctx(),
    )
    .state;
    let s = step(s, Event::TargetChosen(Target::Field(0)), &ctx()).state;
    let s = step(s, Event::BackToBatch, &ctx()).state;
    assert!(
        matches!(&batch_of(&s).item(DocId(1)).unwrap().plan, Some(PositionChoice::Field(f)) if f.qualified_name == "Sig1")
    );
}

// ------------------------------------------------------------------------------------------------
// Review K2: Profil „unsichtbar“, Ablehnung beim Öffnen der Sitzung
// ------------------------------------------------------------------------------------------------

/// Ein unsichtbares Profil erzwingt „unsichtbar“ im Lauf – auch gegen Pläne (verschoben, QR-Platzhalter), die
/// vor dem Wechsel des Profils entstanden (wie `settings_changed` im Arbeitsbereich).
#[test]
fn invisible_profile_overrides_plans_in_the_run() {
    let mut s = overview(&[1, 2]);
    let p = Placement { page_index: 0, x: 40.0, y: 300.0, width: 230.0, height: 49.0 };
    for (id, plan) in [(1, PositionChoice::Manual(p)), (2, PositionChoice::Placeholder(qr()))] {
        let item = s.queue_mut().and_then(|q| q.batch.as_mut()).and_then(|b| b.item_mut(DocId(id))).unwrap();
        // verschoben = Plan der Nutzerin, QR-Platzhalter = Plan aus der Automatik
        item.auto_plan = !matches!(plan, PositionChoice::Manual(_));
        item.plan = Some(plan);
        item.status = BatchItemStatus::Positioned;
    }
    let c = Context { invisible: true, ..ctx() };
    let started = Event::BatchStarted {
        backend: BackendId::AtrustNative,
        docs: vec![DocId(1), DocId(2)],
        output: Some(BatchOutput::Folder("/out".into())),
        on_existing: ExistingPolicy::KeepBoth,
        options: options(false, true),
    };
    let t = step(s, started, &c);
    let (mut state, mut last) = (t.state, t.effects);
    let mut effects = last.clone();
    for _ in 0..60 {
        let State::BatchSigning { run, .. } = &state else { break };
        let id = run.current().unwrap();
        let event = match run.phase {
            BatchPhase::OpeningSession => Event::PhaseCompleted(SigningPhase::OpeningSession),
            BatchPhase::Loading => analyzed(id.0, 3, None, false),
            BatchPhase::ReadingCertificate => Event::PhaseCompleted(SigningPhase::ReadingCertificate),
            BatchPhase::Preparing => Event::PhaseCompleted(SigningPhase::Preparing),
            BatchPhase::Confirming => Event::PhaseCompleted(SigningPhase::Signing),
            BatchPhase::Finishing => Event::PhaseCompleted(SigningPhase::Finishing),
            BatchPhase::Writing | BatchPhase::CheckingTarget => match last.as_slice() {
                [Effect::Write(p)] => Event::Saved(info(&p.display().to_string())),
                _ => Event::BatchOutputDecided {
                    doc: id,
                    path: format!("/out/doc{}_signed.pdf", id.0).into(),
                    exists: false,
                },
            },
        };
        let t = step(state, event, &c);
        state = t.state;
        effects.extend(t.effects.iter().cloned());
        last = t.effects;
    }
    let prepared: Vec<PositionChoice> = effects
        .iter()
        .filter_map(|e| match e {
            Effect::Prepare { position, .. } => Some(position.clone()),
            _ => None,
        })
        .collect();
    let certificates = count(&effects, |e| *e == Effect::ReadCertificate);
    assert!(matches!(state, State::BatchOverview { .. }), "{:?}", state.name());
    assert_eq!(prepared, vec![PositionChoice::Invisible, PositionChoice::Invisible], "nie sichtbar");
    assert_eq!(certificates, 0, "unsichtbar: kein Zertifikat für den Block");
}

/// `Cancelled` ohne eigenen Abbruch schon beim Öffnen der Sitzung (z. B. Entsperren abgebrochen): es gibt
/// keine Sitzung, in der „Erneut anfordern“ weitermachen könnte – Pause der Sitzung, „Erneut versuchen“ öffnet
/// eine neue (A1), nie automatisch weiter.
#[test]
fn declined_while_opening_the_session_pauses_for_a_new_session() {
    let mut d = Driver::new(started(overview(&[1, 2]), BackendId::Keystore, &[1, 2], ExistingPolicy::KeepBoth));
    let e = d.send(Event::SigningDeclined);
    assert!(e.contains(&Effect::CloseSession) && !e.contains(&Effect::Analyze(DocId(1))), "{e:?}");
    let Some(BatchPause::Session { new_session: true, doc: None, error, .. }) = d.pause() else {
        panic!("{:?}", d.pause())
    };
    assert_eq!(error.code(), "ng-signing-declined");
    assert_eq!(status(&d.state, 1), BatchItemStatus::Queued, "kein Dokument begonnen");
    assert_eq!(d.send(Event::Retry), vec![Effect::OpenSession(BackendId::Keystore)]);
    d.finish();
    assert!(matches!(status(&d.state, 1), BatchItemStatus::Signed { .. }));
    assert!(matches!(status(&d.state, 2), BatchItemStatus::Signed { .. }));
}

/// Einstellungen „sofort wirksam“ (ADR 0004 §3.11): der automatische Plan (QR-Platzhalter der Voranalyse)
/// folgt der Platzhaltersuche bzw. dem Profil „unsichtbar“; ein Plan der Nutzerin (verschoben) bleibt. Im Lauf
/// entscheidet die Analyse des Laufs mit den aktuellen Einstellungen.
#[test]
fn automatic_plans_follow_the_settings_user_plans_stay() {
    let on = Context { placeholder_search: true, ..ctx() };
    let off = ctx();
    let t = step(State::Idle { queue: Queue::default() }, Event::FilesAdded(vec![doc(1), doc(2)]), &on);
    let with_qr = DocSummary { qr: true, placeholder: Some(qr()), ..summary(1) };
    let mut s = step(t.state, Event::Preanalyzed { doc: DocId(1), result: Ok(with_qr.clone()) }, &on).state;
    s = step(s, Event::Preanalyzed { doc: DocId(2), result: Ok(with_qr) }, &on).state;
    // Dokument 2 verschoben („Zurück zum Stapel“)
    let p = Placement { page_index: 0, x: 40.0, y: 300.0, width: 230.0, height: 49.0 };
    s = step(s, Event::BatchItemOpened(DocId(2)), &on).state;
    s = step(s, analyzed(2, 1, Some(PlaceholderFound::Qr(qr())), false), &on).state;
    s = step(s, Event::PlacementChanged(p), &on).state;
    s = step(s, Event::BackToBatch, &on).state;
    assert_eq!(status(&s, 1), BatchItemStatus::Positioned);
    assert_eq!(batch_of(&s).item(DocId(2)).unwrap().plan, Some(PositionChoice::Manual(p)));
    // Platzhaltersuche aus: QR-Plan entfällt, Nutzerplan bleibt
    let t = step(s, Event::SettingsChanged, &off);
    assert_eq!(
        (status(&t.state, 1), batch_of(&t.state).item(DocId(1)).unwrap().plan.clone()),
        (BatchItemStatus::Ready, None)
    );
    assert_eq!(batch_of(&t.state).item(DocId(2)).unwrap().plan, Some(PositionChoice::Manual(p)));
    // wieder an: QR-Plan zurück; Profil „unsichtbar“: kein QR
    let s = step(t.state, Event::SettingsChanged, &on).state;
    assert!(matches!(batch_of(&s).item(DocId(1)).unwrap().plan, Some(PositionChoice::Placeholder(_))));
    let invisible = Context { invisible: true, ..on.clone() };
    let s2 = step(s.clone(), Event::SettingsChanged, &invisible).state;
    assert_eq!(batch_of(&s2).item(DocId(1)).unwrap().plan, None);
    // Lauf mit ausgeschalteter Suche, auch wenn der QR-Plan noch in der Übersicht steht: kein QR-Platzhalter
    let started = Event::BatchStarted {
        backend: BackendId::Keystore,
        docs: vec![DocId(1)],
        output: Some(BatchOutput::Folder("/out".into())),
        on_existing: ExistingPolicy::KeepBoth,
        options: options(false, true),
    };
    let t = step(s, started, &off);
    let t = step(t.state, Event::PhaseCompleted(SigningPhase::OpeningSession), &off);
    let t = step(t.state, analyzed(1, 1, None, false), &off);
    let State::BatchSigning { run, .. } = &t.state else { panic!("{:?}", t.state.name()) };
    assert_eq!(run.position, Some(PositionChoice::Auto), "Suche aus: nichts automatisch");
}

/// Vorschau = Ergebnis: wählt die Nutzerin im geöffneten Dokument statt des QR-Platzhalters „Frei
/// platzieren“, signiert der Lauf an der gezeigten Lage – nicht doch in den Platzhalter.
#[test]
fn free_placement_instead_of_the_qr_placeholder_is_kept() {
    let c = Context { placeholder_search: true, ..ctx() };
    let t = step(State::Idle { queue: Queue::default() }, Event::FilesAdded(vec![doc(1), doc(2)]), &c);
    let with_qr = DocSummary { qr: true, placeholder: Some(qr()), ..summary(1) };
    let mut s = step(t.state, Event::Preanalyzed { doc: DocId(1), result: Ok(with_qr) }, &c).state;
    s = step(s, Event::Preanalyzed { doc: DocId(2), result: Ok(summary(1)) }, &c).state;
    s = step(s, Event::BatchItemOpened(DocId(1)), &c).state;
    s = step(s, analyzed(1, 1, Some(PlaceholderFound::Qr(qr())), false), &c).state;
    let State::Positioning { edit: Some(edit), .. } = &s else { panic!("{:?}", s.name()) };
    assert_eq!(edit.target, Target::Qr, "QR-Platzhalter automatisch");
    // unverändert zurück: weiter der (automatische) QR-Plan
    let back = step(s.clone(), Event::BackToBatch, &c).state;
    assert!(matches!(batch_of(&back).item(DocId(1)).unwrap().plan, Some(PositionChoice::Placeholder(_))));
    // „Frei platzieren“: automatische freie Lage, dann zurück → genau diese Lage
    let p = Placement { page_index: 0, x: 300.0, y: 120.0, width: 230.0, height: 49.0 };
    s = step(s, Event::TargetChosen(Target::Free), &c).state;
    s = step(s, Event::AutoPlaced(p), &c).state;
    let s = step(s, Event::BackToBatch, &c).state;
    assert_eq!(batch_of(&s).item(DocId(1)).unwrap().plan, Some(PositionChoice::Manual(p)));
    assert_eq!(status(&s, 1), BatchItemStatus::Positioned);
    let started = Event::BatchStarted {
        backend: BackendId::Keystore,
        docs: vec![DocId(1)],
        output: Some(BatchOutput::Folder("/out".into())),
        on_existing: ExistingPolicy::KeepBoth,
        options: options(false, true),
    };
    let t = step(s, started, &c);
    let t = step(t.state, Event::PhaseCompleted(SigningPhase::OpeningSession), &c);
    let t = step(t.state, analyzed(1, 1, Some(PlaceholderFound::Qr(qr())), false), &c);
    let State::BatchSigning { run, .. } = &t.state else { panic!("{:?}", t.state.name()) };
    assert_eq!(run.position, Some(PositionChoice::Manual(p)), "nicht in den Platzhalter");
}

// ------------------------------------------------------------------------------------------------
// K3: Einstellungen › Dokument (Platzhaltersuche QR | leere Signaturfelder) im Stapel
// ------------------------------------------------------------------------------------------------

/// Mit Platzhaltersuche „Leere Signaturfelder“ ist das erste leere Feld der automatische Plan (`Positioned`);
/// der Lauf signiert hinein. Umstellen wirkt sofort auf automatische Pläne; Pläne der Nutzerin bleiben.
#[test]
fn field_search_plans_the_first_empty_field_and_follows_the_settings() {
    let fields = Context { placeholder_search: true, placeholder_fields: true, ..ctx() };
    let t = step(State::Idle { queue: Queue::default() }, Event::FilesAdded(vec![doc(1), doc(2)]), &fields);
    let with_field = DocSummary { fields: 1, first_field: Some(field("Unterschrift")), ..summary(1) };
    let mut s = step(t.state, Event::Preanalyzed { doc: DocId(1), result: Ok(with_field) }, &fields).state;
    s = step(s, Event::Preanalyzed { doc: DocId(2), result: Ok(summary(1)) }, &fields).state;
    assert_eq!(status(&s, 1), BatchItemStatus::Positioned);
    assert!(
        matches!(&batch_of(&s).item(DocId(1)).unwrap().plan, Some(PositionChoice::Field(f)) if f.qualified_name == "Unterschrift")
    );
    assert_eq!((status(&s, 2), batch_of(&s).item(DocId(2)).unwrap().plan.clone()), (BatchItemStatus::Ready, None));
    // Suche aus: Plan entfällt; wieder „Leere Signaturfelder“: Plan zurück
    let off = step(s.clone(), Event::SettingsChanged, &ctx()).state;
    assert_eq!(status(&off, 1), BatchItemStatus::Ready);
    let s = step(off, Event::SettingsChanged, &fields).state;
    assert_eq!(status(&s, 1), BatchItemStatus::Positioned);
    // im Lauf: das erste leere Feld der Analyse des Laufs
    let started = Event::BatchStarted {
        backend: BackendId::Keystore,
        docs: vec![DocId(1)],
        output: Some(BatchOutput::Folder("/out".into())),
        on_existing: ExistingPolicy::KeepBoth,
        options: options(false, true),
    };
    let t = step(s, started, &fields);
    let t = step(t.state, Event::PhaseCompleted(SigningPhase::OpeningSession), &fields);
    let Event::Analyzed { doc, class, signed, page_count, start, .. } = analyzed(1, 1, None, false) else {
        unreachable!()
    };
    let with_fields = Event::Analyzed {
        doc,
        class,
        placeholder: Some(PlaceholderFound::Fields(vec![field("Unterschrift")])),
        signed,
        page_count,
        start,
        warnings: Vec::new(),
        fields: vec![field("Unterschrift"), field("Zweite")],
        new_visible_field_allowed: true,
    };
    let t = step(t.state, with_fields, &fields);
    let State::BatchSigning { run, .. } = &t.state else { panic!("{:?}", t.state.name()) };
    assert!(
        matches!(&run.position, Some(PositionChoice::Field(f)) if f.qualified_name == "Unterschrift"),
        "{:?}",
        run.position
    );
}

/// Ohne QR-Suche voranalysierte Einträge werden nach dem Einschalten der Suche „QR-Code-Platzhalter“ erneut
/// voranalysiert („Voranalyse läuft“), automatische Pläne folgen; ein Ergebnis ohne Suche unter
/// eingeschalteter Suche wird noch einmal angefordert. Einträge mit Plan der Nutzerin bleiben unberührt.
#[test]
fn turning_on_qr_search_preanalyzes_again() {
    let mut s = overview(&[1, 2]);
    let not_searched = DocSummary { qr_searched: false, ..summary(1) };
    for id in [1, 2] {
        let item = s.queue_mut().and_then(|q| q.batch.as_mut()).and_then(|b| b.item_mut(DocId(id))).unwrap();
        item.summary = Some(not_searched.clone());
    }
    // Eintrag 2: im Arbeitsbereich verschoben (Plan der Nutzerin)
    let p = Placement { page_index: 0, x: 40.0, y: 300.0, width: 230.0, height: 49.0 };
    s = step(s, Event::BatchItemOpened(DocId(2)), &ctx()).state;
    s = step(s, analyzed(2, 1, None, false), &ctx()).state;
    s = step(s, Event::PlacementChanged(p), &ctx()).state;
    s = step(s, Event::BackToBatch, &ctx()).state;
    assert!(!batch_of(&s).item(DocId(2)).unwrap().auto_plan);
    let on = Context { placeholder_search: true, ..ctx() };
    let t = step(s, Event::SettingsChanged, &on);
    assert_eq!(status(&t.state, 1), BatchItemStatus::Analyzing);
    assert_eq!(t.effects, vec![Effect::Preanalyze(DocId(1))], "nur der automatische Eintrag");
    assert_eq!(status(&t.state, 2), BatchItemStatus::Positioned);
    // Ergebnis noch ohne Suche (Einstellung während der Voranalyse geändert): erneut anfordern
    let t = step(t.state, Event::Preanalyzed { doc: DocId(1), result: Ok(not_searched) }, &on);
    assert_eq!(t.effects, vec![Effect::Preanalyze(DocId(1))]);
    assert_eq!(status(&t.state, 1), BatchItemStatus::Analyzing);
    // mit Suche: QR-Platzhalter → Positioned
    let found = DocSummary { qr: true, placeholder: Some(qr()), ..summary(1) };
    let t = step(t.state, Event::Preanalyzed { doc: DocId(1), result: Ok(found) }, &on);
    assert!(t.effects.is_empty());
    assert_eq!(status(&t.state, 1), BatchItemStatus::Positioned);
    assert!(matches!(batch_of(&t.state).item(DocId(1)).unwrap().plan, Some(PositionChoice::Placeholder(_))));
}

/// Ein unverändert aus der Übersicht geöffnetes und zurückgegebenes Dokument bleibt automatisch: es folgt
/// danach weiter den Einstellungen (wie jeder andere Eintrag) – auch beim erneuten Öffnen.
#[test]
fn untouched_opened_item_keeps_following_the_settings() {
    let fields = Context { placeholder_search: true, placeholder_fields: true, ..ctx() };
    let t = step(State::Idle { queue: Queue::default() }, Event::FilesAdded(vec![doc(1), doc(2)]), &ctx());
    let with_field = DocSummary { fields: 1, first_field: Some(field("Sig0")), ..summary(1) };
    let mut s = step(t.state, Event::Preanalyzed { doc: DocId(1), result: Ok(with_field) }, &ctx()).state;
    s = step(s, Event::Preanalyzed { doc: DocId(2), result: Ok(summary(1)) }, &ctx()).state;
    s = step(s, Event::BatchItemOpened(DocId(1)), &ctx()).state;
    let Event::Analyzed { doc, class, signed, page_count, start, placeholder, warnings, .. } =
        analyzed(1, 1, None, false)
    else {
        unreachable!()
    };
    let a = Event::Analyzed {
        doc,
        class,
        placeholder,
        signed,
        page_count,
        start,
        warnings,
        fields: vec![field("Sig0")],
        new_visible_field_allowed: true,
    };
    s = step(s, a.clone(), &ctx()).state;
    s = step(s, Event::BackToBatch, &ctx()).state;
    let item = batch_of(&s).item(DocId(1)).unwrap();
    assert!(item.auto_plan && item.plan.is_none());
    // Suche „Leere Signaturfelder“: Feld-Plan
    let s = step(s, Event::SettingsChanged, &fields).state;
    assert!(matches!(batch_of(&s).item(DocId(1)).unwrap().plan, Some(PositionChoice::Field(_))));
    // erneut öffnen: automatisch im Feld (wie nach der Analyse), nicht als Plan der Nutzerin
    let s = step(s, Event::BatchItemOpened(DocId(1)), &fields).state;
    let s = step(s, a, &fields).state;
    let State::Positioning { edit: Some(edit), .. } = &s else { panic!("{:?}", s.name()) };
    assert_eq!((edit.target, edit.automatic), (Target::Field(0), true));
}

/// „Unsichtbar signieren“ in einem unverändert geöffneten (automatischen) Eintrag ist ein Plan der Nutzerin:
/// er bleibt bei Einstellungsänderungen und beim erneuten Öffnen erhalten (Befund aus dem Merge E3 – vorher
/// galt der Eintrag weiter als automatisch, der Plan „unsichtbar“ ging verloren).
#[test]
fn invisible_chosen_in_an_untouched_item_is_a_user_plan() {
    let fields = Context { placeholder_search: true, placeholder_fields: true, ..ctx() };
    let s = overview(&[1, 2]);
    let s = step(s, Event::BatchItemOpened(DocId(1)), &ctx()).state;
    let s = step(s, analyzed(1, 3, None, false), &ctx()).state;
    let s = step(s, Event::InvisibleSet(true), &ctx()).state;
    let s = step(s, Event::BackToBatch, &ctx()).state;
    let item = batch_of(&s).item(DocId(1)).unwrap();
    assert_eq!((item.plan.clone(), item.auto_plan), (Some(PositionChoice::Invisible), false));
    // Einstellungen ändern sich: der Plan der Nutzerin bleibt
    let s = step(s, Event::SettingsChanged, &fields).state;
    assert_eq!(batch_of(&s).item(DocId(1)).unwrap().plan, Some(PositionChoice::Invisible));
    // erneut öffnen: unsichtbar
    let s = step(s, Event::BatchItemOpened(DocId(1)), &fields).state;
    let s = step(s, analyzed(1, 3, None, false), &fields).state;
    let State::Positioning { edit: Some(edit), .. } = &s else { panic!("{:?}", s.name()) };
    assert!(edit.invisible && !edit.automatic, "{edit:?}");
}

// ------------------------------------------------------------------------------------------------
// Mehrfachsignatur im Stapel: mehrere Blöcke je Dokument, je Block eine eigene Signatur
// ------------------------------------------------------------------------------------------------

/// Übersicht mit zwei Dokumenten; Dokument 2 im Arbeitsbereich mit drei Blöcken (Seite 1 frei, Seite 0 frei,
/// Seite 2 frei – der zweite ist ausgewählt), zurück in die Übersicht.
fn overview_with_three_blocks() -> (State, [Placement; 3]) {
    let at = |page: usize, x: f32| Placement { page_index: page, x, y: 300.0, width: 230.0, height: 49.0 };
    let (a, b, c) = (at(1, 40.0), at(0, 60.0), at(2, 80.0));
    let t = step(overview(&[1, 2]), Event::BatchItemOpened(DocId(2)), &ctx());
    let s = step(t.state, analyzed(2, 3, None, false), &ctx()).state;
    let s = step(s, Event::PlacementChanged(a), &ctx()).state;
    let s = step(s, Event::BlockAdded(b), &ctx()).state;
    let s = step(s, Event::BlockAdded(c), &ctx()).state;
    let s = step(s, Event::BlockSelected(1), &ctx()).state;
    let t = step(s, Event::BackToBatch, &ctx());
    assert!(matches!(t.state, State::BatchOverview { .. }), "{:?}", t.state);
    (t.state, [a, b, c])
}

#[test]
fn blocks_of_an_opened_item_are_kept_as_its_plan() {
    let (s, [a, b, c]) = overview_with_three_blocks();
    let item = batch_of(&s).item(DocId(2)).unwrap().clone();
    assert_eq!(item.plan, Some(PositionChoice::Manual(b)), "Plan = der ausgewählte Block");
    assert_eq!(item.more, vec![PositionChoice::Manual(a), PositionChoice::Manual(c)]);
    assert_eq!((item.active, item.auto_plan, item.signatures()), (1, false, 3));
    assert_eq!(status(&s, 2), BatchItemStatus::Positioned);
    assert_eq!(batch_of(&s).item(DocId(1)).unwrap().signatures(), 1);
    // erneut öffnen: alle Blöcke wieder da, derselbe ausgewählt
    let t = step(s.clone(), Event::BatchItemOpened(DocId(2)), &ctx());
    let t = step(t.state, analyzed(2, 3, None, false), &ctx());
    let State::Positioning { placement: Some(q), edit: Some(edit), .. } = &t.state else { panic!("{:?}", t.state) };
    assert_eq!(*q, b);
    assert_eq!(edit.active, 1);
    assert_eq!(edit.others.iter().map(|o| o.placement).collect::<Vec<_>>(), vec![Some(a), Some(c)]);
    // zurück ohne Änderung: derselbe Plan
    let again = step(t.state.clone(), Event::BackToBatch, &ctx());
    assert_eq!(batch_of(&again.state).item(DocId(2)).unwrap(), &item);
    // ein Block entfernt: der Plan folgt
    let s2 = step(t.state, Event::BlockRemoved(0), &ctx()).state;
    let back = step(s2, Event::BackToBatch, &ctx());
    let item = batch_of(&back.state).item(DocId(2)).unwrap();
    assert_eq!((item.more.clone(), item.active), (vec![PositionChoice::Manual(c)], 0));
    // „Für diesen Stapel automatisch positionieren“ mit Neusetzen: wieder ein automatischer Block
    let auto = step(s, Event::BatchAutoPositionSet { enabled: true, replace_manual: true }, &ctx());
    let item = batch_of(&auto.state).item(DocId(2)).unwrap();
    assert_eq!((item.more.len(), item.signatures(), item.auto_plan), (0, 1, true));
}

#[test]
fn the_run_signs_once_per_block_in_the_same_session() {
    let (s, [a, b, c]) = overview_with_three_blocks();
    let mut d = Driver::new(started(s, BackendId::Keystore, &[1, 2], ExistingPolicy::KeepBoth));
    d.until(2, BatchPhase::Preparing);
    let run = |d: &Driver| match &d.state {
        State::BatchSigning { run, .. } => (run.round, run.position.clone(), run.positions.len()),
        other => panic!("{}", other.name()),
    };
    assert_eq!(run(&d), (0, Some(PositionChoice::Manual(a)), 3), "Reihenfolge der Blöcke, nicht der Auswahl");
    for (round, position) in [(1, b), (2, c)] {
        d.ok(); // Preparing
        d.ok(); // Confirming
        let effects = d.ok(); // Finishing → nächste Runde
        assert!(
            matches!(&effects[..], [Effect::NextSignature, Effect::Prepare { position: p, .. }] if *p == PositionChoice::Manual(position)),
            "{effects:?}"
        );
        assert_eq!(run(&d), (round, Some(PositionChoice::Manual(position)), 3));
        assert_eq!(d.phase(), BatchPhase::Preparing);
    }
    d.finish();
    assert_eq!(opens(&d.effects), 1, "eine Sitzung");
    assert_eq!(count(&d.effects, |e| *e == Effect::SignDocument), 4, "Dokument 1 einmal, Dokument 2 dreimal");
    assert_eq!(count(&d.effects, |e| *e == Effect::NextSignature), 2);
    assert_eq!(count(&d.effects, |e| matches!(e, Effect::Write(_))), 2, "je Dokument eine Datei");
    assert!(matches!(status(&d.state, 2), BatchItemStatus::Signed { .. }));
}

/// Ein Fehler in einer späteren Runde verwirft das Teilergebnis: „Erneut versuchen“ beginnt wieder bei der ersten
/// Signatur des Dokuments.
#[test]
fn a_failure_in_a_later_round_restarts_the_document() {
    let (s, [a, _, _]) = overview_with_three_blocks();
    let mut d = Driver::new(started(s, BackendId::Keystore, &[2], ExistingPolicy::KeepBoth));
    d.until(2, BatchPhase::Preparing);
    d.ok();
    d.ok();
    d.ok(); // zweite Runde vorbereitet
    d.ok(); // Confirming der zweiten Runde
    assert_eq!(d.phase(), BatchPhase::Confirming);
    d.send(Event::SigningDeclined);
    assert!(matches!(d.pause(), Some(BatchPause::Document { .. })), "{:?}", d.pause());
    let State::BatchSigning { run, .. } = &d.state else { panic!() };
    assert_eq!((run.round, run.position.clone()), (0, Some(PositionChoice::Manual(a))));
    let effects = d.send(Event::Retry);
    assert!(
        matches!(&effects[..], [Effect::Prepare { position, .. }] if *position == PositionChoice::Manual(a)),
        "{effects:?}"
    );
    d.finish();
    assert!(matches!(status(&d.state, 2), BatchItemStatus::Signed { .. }));
}
