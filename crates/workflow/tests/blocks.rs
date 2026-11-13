//! Mehrfachsignatur (Entscheidung vom 13.11.2026): mehrere Signaturblöcke in einem Dokument, je Block eine
//! eigene Signatur. Der ausgewählte Block liegt wie bisher im Zustand, die übrigen ruhen in
//! `WorkspaceEdit::others`; signiert wird Runde für Runde in derselben Sitzung.

mod common;

use common::*;
use pida_backend_api::{BackendError, BackendId};
use pida_placement::Placement;
use pida_workflow::{
    Context, DocId, Effect, Event, MAX_BLOCKS, PlacementOrigin, PositionChoice, SigningPhase, State, Target,
    WorkspaceEdit,
};

fn ws() -> Context {
    Context { workspace: true, ..Context::default() }
}

/// Arbeitsbereich: Dokument 1, drei Seiten, zwei leere Felder, QR-Platzhalter, ein frei platzierter Block.
fn workspace() -> State {
    let mut edit = WorkspaceEdit::new(vec![field("Sig1"), field("Sig2")], None, false);
    edit.qr = Some(qr());
    edit.origin = PlacementOrigin::User;
    edit.automatic = false;
    State::Positioning {
        queue: queue(&[1]),
        page: 2,
        placement: Some(start_placement(3)),
        new_page: false,
        doc: facts(1, 3),
        edit: Some(edit),
    }
}

fn at(page: usize, x: f32) -> Placement {
    Placement { page_index: page, x, y: 300.0, width: 200.0, height: 60.0 }
}

fn edit_of(s: &State) -> &WorkspaceEdit {
    match s {
        State::Positioning { edit: Some(e), .. }
        | State::Signing { edit: Some(e), .. }
        | State::Output { edit: Some(e), .. } => e,
        other => panic!("kein Bearbeitungszustand: {other:?}"),
    }
}

fn shown(s: &State) -> (usize, Option<Placement>) {
    match s {
        State::Positioning { page, placement, .. } => (*page, *placement),
        other => panic!("nicht im Arbeitsbereich: {other:?}"),
    }
}

fn sign_with(backend: BackendId) -> Event {
    Event::SignRequested { backend, options: options(true, true) }
}

#[test]
fn adding_a_block_parks_the_selected_one_and_selects_the_new() {
    let t = step(workspace(), Event::BlockAdded(at(0, 40.0)), &ws());
    let e = edit_of(&t.state);
    assert_eq!(e.others.len(), 1);
    assert_eq!(e.active, 1, "der neue Block ist der zweite und ausgewählt");
    assert_eq!(e.others[0].page, 2);
    assert_eq!(e.others[0].placement, Some(start_placement(3)));
    assert_eq!((e.target, e.origin), (Target::Free, PlacementOrigin::User));
    assert_eq!(shown(&t.state), (0, Some(at(0, 40.0))));
    assert_eq!(t.effects, vec![Effect::RenderPage { doc: DocId(1), page: 0 }, Effect::BuildBlockPreview(DocId(1))]);
    assert_eq!(e.block_count(), 2);
}

#[test]
fn selecting_swaps_blocks_and_keeps_their_order() {
    let s = step(workspace(), Event::BlockAdded(at(0, 40.0)), &ws()).state;
    let s = step(s, Event::BlockAdded(at(1, 80.0)), &ws()).state;
    // Reihenfolge: Start (Seite 2), Seite 0, Seite 1 – ausgewählt ist der dritte
    assert_eq!(edit_of(&s).active, 2);
    let t = step(s, Event::BlockSelected(0), &ws());
    let e = edit_of(&t.state);
    assert_eq!(e.active, 0);
    assert_eq!(shown(&t.state), (2, Some(start_placement(3))));
    assert_eq!(e.others.iter().map(|b| b.page).collect::<Vec<_>>(), vec![0, 1]);
    let t = step(t.state, Event::BlockSelected(1), &ws());
    let e = edit_of(&t.state);
    assert_eq!(e.active, 1);
    assert_eq!(shown(&t.state), (0, Some(at(0, 40.0))));
    assert_eq!(e.others.iter().map(|b| b.page).collect::<Vec<_>>(), vec![2, 1]);
    // derselbe bzw. ein unbekannter Block: ohne Wirkung
    for i in [1, 3, 99] {
        let same = step(t.state.clone(), Event::BlockSelected(i), &ws());
        assert_eq!(same.state, t.state);
    }
}

#[test]
fn tools_act_on_the_selected_block_only() {
    let s = step(workspace(), Event::BlockAdded(at(0, 40.0)), &ws()).state;
    let t = step(s, Event::PlacementChanged(at(0, 300.0)), &ws());
    assert_eq!(shown(&t.state).1, Some(at(0, 300.0)));
    assert_eq!(edit_of(&t.state).others[0].placement, Some(start_placement(3)), "der ruhende Block bleibt");
    // Zielwahl: ein Feld nur für einen Block
    let t = step(t.state, Event::TargetChosen(Target::Field(0)), &ws());
    assert_eq!(edit_of(&t.state).target, Target::Field(0));
    let t = step(t.state, Event::BlockSelected(0), &ws());
    let t = step(t.state, Event::TargetChosen(Target::Field(0)), &ws());
    assert_eq!(edit_of(&t.state).target, Target::Free);
    assert_eq!(t.effects, vec![Effect::Notify("ng-block-target-taken".into())]);
    let t = step(t.state, Event::TargetChosen(Target::Field(1)), &ws());
    assert_eq!(edit_of(&t.state).target, Target::Field(1));
}

#[test]
fn removing_blocks() {
    let s = step(workspace(), Event::BlockAdded(at(0, 40.0)), &ws()).state;
    let s = step(s, Event::BlockAdded(at(1, 80.0)), &ws()).state;
    // einen ruhenden Block davor entfernen: der ausgewählte rückt auf
    let t = step(s.clone(), Event::BlockRemoved(0), &ws());
    let e = edit_of(&t.state);
    assert_eq!((e.others.len(), e.active), (1, 1));
    assert_eq!(shown(&t.state), (1, Some(at(1, 80.0))));
    // den ausgewählten entfernen: der vorige wird ausgewählt
    let t = step(s, Event::BlockRemoved(2), &ws());
    let e = edit_of(&t.state);
    assert_eq!((e.others.len(), e.active), (1, 1));
    assert_eq!(shown(&t.state), (0, Some(at(0, 40.0))));
    // den ersten (ausgewählten) entfernen: der nächste wird ausgewählt
    let t = step(t.state, Event::BlockSelected(0), &ws());
    let t = step(t.state, Event::BlockRemoved(0), &ws());
    let e = edit_of(&t.state);
    assert_eq!((e.others.len(), e.active), (0, 0));
    assert_eq!(shown(&t.state), (0, Some(at(0, 40.0))));
    // der letzte Block bleibt
    let last = step(t.state.clone(), Event::BlockRemoved(0), &ws());
    assert_eq!(last.state, t.state);
}

#[test]
fn limits_and_refusals() {
    // Obergrenze
    let mut s = workspace();
    for i in 1..MAX_BLOCKS {
        s = step(s, Event::BlockAdded(at(0, i as f32)), &ws()).state;
    }
    assert_eq!(edit_of(&s).block_count(), MAX_BLOCKS);
    let t = step(s.clone(), Event::BlockAdded(at(0, 500.0)), &ws());
    assert_eq!(t.state, s);
    assert_eq!(t.effects, vec![Effect::Notify("ng-blocks-limit".into())]);
    // „Auf neuer Seite“ nur mit einem Block – in beiden Richtungen
    let two = step(workspace(), Event::BlockAdded(at(0, 40.0)), &ws());
    let t = step(two.state.clone(), Event::NewPageToggled, &ws());
    assert_eq!(t.state, two.state);
    assert_eq!(t.effects, vec![Effect::Notify("ng-blocks-new-page".into())]);
    let on_new_page = step(workspace(), Event::NewPageToggled, &ws()).state;
    let t = step(on_new_page.clone(), Event::BlockAdded(at(0, 40.0)), &ws());
    assert_eq!(t.state, on_new_page);
    assert_eq!(t.effects, vec![Effect::Notify("ng-blocks-new-page".into())]);
    // unsichtbar, außerhalb des Dokuments, klassischer Ablauf: ohne Wirkung
    let invisible = step(workspace(), Event::InvisibleSet(true), &ws()).state;
    assert_eq!(step(invisible.clone(), Event::BlockAdded(at(0, 40.0)), &ws()).state, invisible);
    assert_eq!(step(workspace(), Event::BlockAdded(at(3, 40.0)), &ws()).state, workspace());
    let classic = positioning(&[1]);
    assert_eq!(step(classic.clone(), Event::BlockAdded(at(0, 40.0)), &Context::default()).state, classic);
}

#[test]
fn filling_all_targets() {
    let all = vec![Target::Field(0), Target::Field(1), Target::Qr, Target::Field(7)];
    let t = step(workspace(), Event::TargetsFilled(all.clone()), &ws());
    let e = edit_of(&t.state);
    assert_eq!(e.target, Target::Field(0), "der frei platzierte Block wandert in das erste Ziel");
    assert_eq!(e.others.iter().map(|b| b.target).collect::<Vec<_>>(), vec![Target::Field(1), Target::Qr]);
    assert_eq!(e.active, 0);
    assert_eq!(e.block_count(), 3);
    // erneut: nichts mehr frei
    let again = step(t.state.clone(), Event::TargetsFilled(all.clone()), &ws());
    assert_eq!(again.state, t.state);
    // ein schon gesetzter Block bleibt, die übrigen Ziele kommen dazu
    let s = step(workspace(), Event::TargetChosen(Target::Field(1)), &ws()).state;
    let t = step(s, Event::TargetsFilled(all), &ws());
    let e = edit_of(&t.state);
    assert_eq!(e.target, Target::Field(1));
    assert_eq!(e.others.iter().map(|b| b.target).collect::<Vec<_>>(), vec![Target::Field(0), Target::Qr]);
}

/// Drei Blöcke: frei (Seite 2), Feld 0, frei (Seite 1) – der zweite ist ausgewählt.
fn three_blocks() -> State {
    let s = step(workspace(), Event::BlockAdded(at(0, 40.0)), &ws()).state;
    let s = step(s, Event::TargetChosen(Target::Field(0)), &ws()).state;
    let s = step(s, Event::BlockAdded(at(1, 80.0)), &ws()).state;
    step(s, Event::BlockSelected(1), &ws()).state
}

fn round(s: State, last: bool) -> State {
    let t = step(s, Event::PhaseCompleted(SigningPhase::Preparing), &ws());
    assert_eq!(t.effects, vec![Effect::SignDocument]);
    let t = step(t.state, Event::PhaseCompleted(SigningPhase::Signing), &ws());
    assert_eq!(t.effects, vec![Effect::Finish]);
    let t = step(t.state, Event::PhaseCompleted(SigningPhase::Finishing), &ws());
    if last {
        assert_eq!(t.effects, vec![Effect::CloseSession, Effect::DeletePersisted, Effect::DecideOutput(DocId(1))]);
    } else {
        assert!(matches!(t.effects[..], [Effect::NextSignature, Effect::Prepare { .. }]), "{:?}", t.effects);
    }
    t.state
}

fn position_of(s: &State) -> &PositionChoice {
    match s {
        State::Signing { position, .. } | State::Output { position, .. } => position,
        other => panic!("{other:?}"),
    }
}

#[test]
fn signing_runs_one_round_per_block_in_one_session() {
    let s = step(three_blocks(), sign_with(BackendId::Keystore), &ws());
    assert_eq!(s.effects.iter().filter(|e| matches!(e, Effect::OpenSession(_))).count(), 1);
    let rounds = edit_of(&s.state).rounds.clone().expect("Mehrfachsignatur");
    assert_eq!(rounds.index, 0);
    assert!(matches!(
        rounds.positions[..],
        [PositionChoice::Manual(a), PositionChoice::Field(_), PositionChoice::Manual(b)]
            if a == start_placement(3) && b == at(1, 80.0)
    ));
    assert_eq!(position_of(&s.state), &rounds.positions[0]);
    let s = step(s.state, Event::PhaseCompleted(SigningPhase::OpeningSession), &ws());
    let s = step(s.state, Event::PhaseCompleted(SigningPhase::ReadingCertificate), &ws());
    assert!(matches!(&s.effects[..], [Effect::Prepare { position, .. }] if *position == rounds.positions[0]));
    let s = round(s.state, false);
    assert_eq!((edit_of(&s).rounds.as_ref().unwrap().index, position_of(&s)), (1, &rounds.positions[1]));
    let s = round(s, false);
    assert_eq!((edit_of(&s).rounds.as_ref().unwrap().index, position_of(&s)), (2, &rounds.positions[2]));
    let s = round(s, true);
    assert!(matches!(s, State::Output { .. }));
}

#[test]
fn a_failure_discards_the_partial_result_and_retry_starts_over() {
    let before = three_blocks();
    let s = step(before.clone(), sign_with(BackendId::Keystore), &ws()).state;
    let s = step(s, Event::PhaseCompleted(SigningPhase::OpeningSession), &ws()).state;
    let s = step(s, Event::PhaseCompleted(SigningPhase::ReadingCertificate), &ws()).state;
    let second = round(s, false);
    // Abbruch in der zweiten Runde: Arbeitsbereich wie zuvor, keine laufende Mehrfachsignatur
    let t = step(second.clone(), Event::Cancel, &ws());
    assert_eq!(t.state, before);
    // Fehler in der zweiten Runde, „Erneut versuchen“: wieder ab der ersten Signatur
    let failed = step(second, Event::BackendFailed(BackendError::Network("x".into())), &ws());
    assert!(matches!(failed.state, State::Error { .. }));
    let t = step(failed.state.clone(), Event::Retry, &ws());
    assert!(t.effects.iter().any(|e| matches!(e, Effect::OpenSession(_))));
    let rounds = edit_of(&t.state).rounds.clone().unwrap();
    assert_eq!(rounds.index, 0);
    assert_eq!(position_of(&t.state), &rounds.positions[0]);
    // „Schließen“ im Fehlerschritt: Arbeitsbereich wie zuvor
    let t = step(failed.state, Event::Cancel, &ws());
    assert_eq!(t.state, before);
}

#[test]
fn one_block_and_invisible_sign_once() {
    let s = step(workspace(), sign_with(BackendId::Keystore), &ws());
    assert_eq!(edit_of(&s.state).rounds, None);
    // unsichtbar: genau eine Signatur, die weiteren Blöcke ruhen
    let s = step(three_blocks(), Event::InvisibleSet(true), &ws()).state;
    assert_eq!(edit_of(&s).block_count(), 1);
    let s = step(s, sign_with(BackendId::Keystore), &ws());
    assert_eq!(edit_of(&s.state).rounds, None);
    assert_eq!(position_of(&s.state), &PositionChoice::Invisible);
}

#[test]
fn state_roundtrips_and_reads_older_snapshots() {
    let s = three_blocks();
    let json = serde_json::to_string(&s).unwrap();
    assert_eq!(serde_json::from_str::<State>(&json).unwrap(), s);
    // ältere Zustände ohne die neuen Felder bleiben lesbar
    let mut v: serde_json::Value = serde_json::to_value(workspace()).unwrap();
    let edit = v.pointer_mut("/Positioning/edit").and_then(|e| e.as_object_mut()).unwrap();
    for key in ["others", "active", "rounds"] {
        assert!(edit.remove(key).is_some(), "{key}");
    }
    assert_eq!(serde_json::from_value::<State>(v).unwrap(), workspace());
}
