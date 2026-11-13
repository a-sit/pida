//! Tabellentests des Arbeitsbereich-Modus (ADR 0004 §3.2–§3.7, Paket K1): automatische Wege führen in den
//! Arbeitsbereich, „Signieren mit X“ (`SignRequested`) ist der einzige Weg zur Sitzung (A1), Abbruch und
//! Fehler kehren mit unverändertem Bearbeitungszustand zurück, Wiederholen hält die Sitzung offen.

mod common;

use common::*;
use pida_backend_api::{BackendError, BackendId};
use pida_placement::{EmptyFieldCandidate, Placement};
use pida_workflow::{
    Context, DocFacts, DocId, Effect, Event, PlaceholderFound, PlacementLock, PlacementOrigin, PositionChoice,
    RetryTarget, SigningPhase, State, Step, Target, WorkflowError, WorkspaceEdit,
};

fn ws() -> Context {
    Context { workspace: true, ..Context::default() }
}

fn analyzed_with(id: u64, pages: usize, placeholder: Option<PlaceholderFound>, fields: usize, start: bool) -> Event {
    let Event::Analyzed { doc, class, signed, page_count, .. } = analyzed(id, pages, None, false) else {
        unreachable!()
    };
    Event::Analyzed {
        doc,
        class,
        placeholder,
        signed,
        page_count,
        start: start.then(|| start_placement(pages)),
        warnings: Vec::new(),
        fields: (0..fields).map(|i| field(&format!("Sig{i}"))).collect(),
        new_visible_field_allowed: true,
    }
}

/// Arbeitsbereich wie nach der Analyse (Dokument 1, drei Seiten, ein leeres Feld, QR-Platzhalter bekannt).
fn workspace_state(target: Target, origin: PlacementOrigin, invisible: bool) -> State {
    let edit = WorkspaceEdit {
        target,
        origin,
        invisible,
        fields: vec![field("Sig1")],
        qr: Some(qr()),
        return_to_batch: false,
        parked: None,
        qr_search: Some(Some(qr())),
        automatic: matches!(origin, PlacementOrigin::Auto | PlacementOrigin::Qr),
        invisible_by_profile: false,
        lock: PlacementLock::Off,
        others: Vec::new(),
        active: 0,
        rounds: None,
    };
    State::Positioning {
        queue: queue(&[1]),
        page: 2,
        placement: Some(start_placement(3)),
        new_page: false,
        doc: facts(1, 3),
        edit: Some(edit),
    }
}

fn edit_of(s: &State) -> &WorkspaceEdit {
    match s {
        State::Positioning { edit: Some(e), .. }
        | State::Signing { edit: Some(e), .. }
        | State::ChoosingBackend { edit: Some(e), .. }
        | State::Output { edit: Some(e), .. } => e,
        other => panic!("kein Bearbeitungszustand: {other:?}"),
    }
}

fn opens_session(effects: &[Effect]) -> bool {
    effects.iter().any(|e| matches!(e, Effect::OpenSession(_)))
}

fn sign_with(backend: BackendId) -> Event {
    Event::SignRequested { backend, options: options(true, true) }
}

// ---------------------------------------------------------------------------------------------
// Nach der Analyse: immer Arbeitsbereich (keine Platzhalterfrage, keine automatische Wahl)
// ---------------------------------------------------------------------------------------------

#[test]
fn analysis_leads_to_workspace_with_automatic_start_placement() {
    let t = step(State::Analyzing { queue: queue(&[1]) }, analyzed_with(1, 3, None, 0, true), &ws());
    let State::Positioning { page: 2, placement: Some(p), new_page: false, .. } = &t.state else {
        panic!("{:?}", t.state)
    };
    assert_eq!(*p, start_placement(3), "bis AutoPlaced eintrifft: bisherige Startlage (50 %/25 %)");
    let e = edit_of(&t.state);
    assert_eq!((e.target, e.origin, e.invisible), (Target::Free, PlacementOrigin::Auto, false));
    assert_eq!(
        t.effects,
        vec![
            Effect::RenderPage { doc: DocId(1), page: 2 },
            Effect::BuildBlockPreview(DocId(1)),
            Effect::AutoPlace(DocId(1))
        ]
    );
}

/// Jeder automatische Weg (unsichtbar, `auto_position`, Platzhalter, ohne Vorschau) endet im Arbeitsbereich.
#[test]
fn automatic_paths_end_in_the_workspace() {
    let cases: Vec<(&str, Context, Event)> = vec![
        ("unsichtbar", Context { invisible: true, ..ws() }, analyzed_with(1, 3, None, 0, true)),
        ("auto_position", Context { auto_position: true, ..ws() }, analyzed_with(1, 3, None, 0, true)),
        (
            "QR-Platzhalter",
            Context { placeholder_search: true, ..ws() },
            analyzed_with(1, 3, Some(PlaceholderFound::Qr(qr())), 0, true),
        ),
        (
            "Felder (kind = fields)",
            Context { placeholder_search: true, placeholder_fields: true, ..ws() },
            analyzed_with(1, 3, Some(PlaceholderFound::Fields(vec![field("Sig0")])), 1, true),
        ),
        ("ohne Vorschau", ws(), analyzed_with(1, 3, None, 0, false)),
        (
            "Standard ID Austria",
            Context { default_backend: Some(pida_config::BackendChoice::IdAustria), auto_position: true, ..ws() },
            analyzed_with(1, 3, None, 0, true),
        ),
        (
            "Standard Keystore",
            Context { default_backend: Some(pida_config::BackendChoice::Keystore), auto_position: true, ..ws() },
            analyzed_with(1, 3, None, 0, true),
        ),
    ];
    for (name, ctx, ev) in cases {
        let t = step(State::Analyzing { queue: queue(&[1]) }, ev, &ctx);
        assert!(matches!(t.state, State::Positioning { edit: Some(_), .. }), "{name}: {:?}", t.state);
        assert!(!t.effects.contains(&Effect::ProbeBackends) && !opens_session(&t.effects), "{name}: {:?}", t.effects);
    }
}

#[test]
fn invisible_profile_qr_fields_and_missing_preview() {
    // unsichtbares Profil: unsichtbar, keine automatische Platzierung
    let t = step(
        State::Analyzing { queue: queue(&[1]) },
        analyzed_with(1, 3, None, 0, true),
        &Context { invisible: true, ..ws() },
    );
    assert!(edit_of(&t.state).invisible);
    assert!(!t.effects.contains(&Effect::AutoPlace(DocId(1))));
    // QR-Platzhalter mit Platzhaltersuche (kind = marker): Ziel QR, automatisch, umkehrbar
    let ctx = Context { placeholder_search: true, ..ws() };
    let t = step(
        State::Analyzing { queue: queue(&[1]) },
        analyzed_with(1, 3, Some(PlaceholderFound::Qr(qr())), 0, true),
        &ctx,
    );
    let e = edit_of(&t.state);
    assert_eq!((e.target, e.origin), (Target::Qr, PlacementOrigin::Qr));
    assert!(!t.effects.contains(&Effect::AutoPlace(DocId(1))));
    // ohne Platzhaltersuche kein QR-Ziel
    let t = step(
        State::Analyzing { queue: queue(&[1]) },
        analyzed_with(1, 3, Some(PlaceholderFound::Qr(qr())), 0, true),
        &ws(),
    );
    assert_eq!(edit_of(&t.state).target, Target::Free);
    assert!(edit_of(&t.state).qr.is_none());
    // leere Felder: immer angeboten (auch ohne Platzhaltersuche), nie automatisch benutzt (§8 F2)
    let t = step(State::Analyzing { queue: queue(&[1]) }, analyzed_with(1, 3, None, 2, true), &ws());
    let e = edit_of(&t.state);
    assert_eq!((e.fields.len(), e.target), (2, Target::Free));
    // ohne Vorschau: Arbeitsbereich ohne Lage, Hinweis, keine automatische Platzierung
    let t = step(State::Analyzing { queue: queue(&[1]) }, analyzed_with(1, 3, None, 0, false), &ws());
    assert!(matches!(t.state, State::Positioning { placement: None, .. }), "{:?}", t.state);
    assert_eq!(t.effects, vec![Effect::Notify("error-PositioningNotPossible".into())]);
    // Renderer fällt später aus: bleibt im Arbeitsbereich (A1), signiert automatisch platziert
    let s = workspace_state(Target::Free, PlacementOrigin::Auto, false);
    let t = step(s, Event::PreviewUnavailable { doc: DocId(1) }, &ws());
    assert!(matches!(t.state, State::Positioning { placement: None, edit: Some(_), .. }), "{:?}", t.state);
    let t = step(t.state, sign_with(BackendId::Keystore), &ws());
    assert!(matches!(&t.state, State::Signing { position: PositionChoice::Auto, .. }), "{:?}", t.state);
}

#[test]
fn analysis_warning_then_workspace() {
    let Event::Analyzed { doc, class, placeholder, signed, page_count, start, fields, .. } =
        analyzed_with(1, 3, None, 1, true)
    else {
        unreachable!()
    };
    let ev = Event::Analyzed {
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
    let t = step(State::Analyzing { queue: queue(&[1]) }, ev, &ws());
    assert!(matches!(t.state, State::AnalysisWarning { .. }));
    let t = step(t.state, Event::WarningAnswered(true), &ws());
    assert_eq!(edit_of(&t.state).fields.len(), 1, "Felder gelten nach „Fortfahren“");
}

// ---------------------------------------------------------------------------------------------
// Bearbeiten: Ziele, unsichtbar, automatische Platzierung
// ---------------------------------------------------------------------------------------------

#[test]
fn user_moves_set_free_target_and_user_origin() {
    let s = workspace_state(Target::Field(0), PlacementOrigin::Field, false);
    let p = Placement { page_index: 1, x: 10.0, y: 300.0, width: 230.0, height: 49.0 };
    let t = step(s.clone(), Event::PlacementChanged(p), &ws());
    assert_eq!((edit_of(&t.state).target, edit_of(&t.state).origin), (Target::Free, PlacementOrigin::User));
    let t = step(s.clone(), Event::PageChanged(0), &ws());
    assert_eq!(edit_of(&t.state).origin, PlacementOrigin::User);
    let t = step(s, Event::NewPageToggled, &ws());
    assert_eq!(edit_of(&t.state).target, Target::Free);
}

#[test]
fn auto_placement_applies_only_while_automatic() {
    let s = workspace_state(Target::Free, PlacementOrigin::Auto, false);
    let auto = Placement { page_index: 3, x: 50.0, y: 822.0, width: 230.0, height: 49.0 };
    // automatische Lage auf einer neuen Seite
    let t = step(s.clone(), Event::AutoPlaced(auto), &ws());
    let State::Positioning { page: 3, new_page: true, placement: Some(p), .. } = &t.state else {
        panic!("{:?}", t.state)
    };
    assert_eq!(*p, auto);
    assert_eq!(edit_of(&t.state).origin, PlacementOrigin::Auto);
    // nach einer Bewegung der Nutzerin wird ein verspätetes Ergebnis verworfen
    let moved = Placement { page_index: 0, x: 1.0, y: 100.0, width: 230.0, height: 49.0 };
    let m = step(s.clone(), Event::PlacementChanged(moved), &ws()).state;
    let t = step(m.clone(), Event::AutoPlaced(auto), &ws());
    assert_eq!(t.state, m);
    // „Freie Fläche suchen“ / „Frei platzieren“ fordert sie neu an
    let t = step(m, Event::TargetChosen(Target::Free), &ws());
    assert_eq!(edit_of(&t.state).origin, PlacementOrigin::Auto);
    assert_eq!(t.effects, vec![Effect::AutoPlace(DocId(1))]);
    // signiertes Dokument: neue Seite nicht erlaubt → letzte Seite mit Hinweis
    let State::Positioning { queue, page, placement, new_page, doc, edit } = s else { unreachable!() };
    let signed = State::Positioning { queue, page, placement, new_page, doc: DocFacts { signed: true, ..doc }, edit };
    let t = step(signed, Event::AutoPlaced(auto), &ws());
    assert!(matches!(t.state, State::Positioning { page: 2, new_page: false, .. }), "{:?}", t.state);
    assert!(t.effects.contains(&Effect::Notify("placement.notice.new_page_not_allowed".into())));
}

#[test]
fn block_resize_replaces_automatic_placement_only() {
    let resized = Placement { page_index: 2, x: 297.5, y: 210.5, width: 300.0, height: 60.0 };
    let t = step(workspace_state(Target::Free, PlacementOrigin::Auto, false), Event::BlockResized(resized), &ws());
    assert_eq!(t.effects, vec![Effect::AutoPlace(DocId(1))], "automatisch platziert: neu setzen");
    assert_eq!(edit_of(&t.state).origin, PlacementOrigin::Auto);
    let t = step(workspace_state(Target::Free, PlacementOrigin::User, false), Event::BlockResized(resized), &ws());
    assert!(t.effects.is_empty(), "manuell: Lage behalten (geklemmt)");
    assert_eq!(edit_of(&t.state).origin, PlacementOrigin::User);
    assert!(matches!(&t.state, State::Positioning { placement: Some(p), .. } if *p == resized));
    // Feld bleibt Feld
    let t = step(workspace_state(Target::Field(0), PlacementOrigin::Field, false), Event::BlockResized(resized), &ws());
    assert_eq!(edit_of(&t.state).target, Target::Field(0));
}

#[test]
fn targets_and_invisible_switch() {
    let s = workspace_state(Target::Free, PlacementOrigin::Auto, false);
    let t = step(s.clone(), Event::TargetChosen(Target::Field(0)), &ws());
    assert_eq!((edit_of(&t.state).target, edit_of(&t.state).origin), (Target::Field(0), PlacementOrigin::Field));
    // unbekanntes bzw. gesperrtes Feld: abgelehnt mit Hinweis
    let t = step(s.clone(), Event::TargetChosen(Target::Field(9)), &ws());
    assert_eq!(edit_of(&t.state).target, Target::Free);
    assert_eq!(t.effects, vec![Effect::Notify("ng-field-locked".into())]);
    let t = step(s.clone(), Event::TargetChosen(Target::Qr), &ws());
    assert_eq!(edit_of(&t.state).target, Target::Qr);
    // „Unsichtbar“ als Schalter (nur dieses Dokument)
    let t = step(s.clone(), Event::InvisibleSet(true), &ws());
    assert!(edit_of(&t.state).invisible);
    let t = step(t.state, Event::InvisibleSet(false), &ws());
    assert!(!edit_of(&t.state).invisible);
    // unsichtbares Profil: sichtbar nicht möglich
    let inv = Context { invisible: true, ..ws() };
    let t = step(workspace_state(Target::Free, PlacementOrigin::Auto, true), Event::InvisibleSet(false), &inv);
    assert!(edit_of(&t.state).invisible);
    assert_eq!(t.effects, vec![Effect::Notify("ng-constraint-invisible-profile".into())]);
    // Altereignisse starten im Arbeitsbereich nichts
    for e in [Event::PositionConfirmed, Event::InvisibleChosen] {
        let t = step(s.clone(), e, &ws());
        assert_eq!(t.state, s);
        assert!(t.effects.is_empty());
    }
}

// ---------------------------------------------------------------------------------------------
// „Signieren mit X“ (§3.5)
// ---------------------------------------------------------------------------------------------

#[test]
fn sign_with_takes_the_position_from_the_workspace() {
    let p = start_placement(3);
    let cases = [
        (Target::Free, PlacementOrigin::Auto, false, PositionChoice::Manual(p)),
        (Target::Free, PlacementOrigin::User, false, PositionChoice::Manual(p)),
        (Target::Field(0), PlacementOrigin::Field, false, PositionChoice::Field(field("Sig1"))),
        (Target::Qr, PlacementOrigin::Qr, false, PositionChoice::Placeholder(qr())),
        (Target::Field(0), PlacementOrigin::Field, true, PositionChoice::Invisible),
    ];
    for (target, origin, invisible, expected) in cases {
        let t = step(workspace_state(target, origin, invisible), sign_with(BackendId::AtrustNative), &ws());
        let State::Signing { position, backend, phase: SigningPhase::OpeningSession, flow, .. } = &t.state else {
            panic!("{target:?}: {:?}", t.state)
        };
        assert_eq!(position, &expected, "{target:?}");
        assert_eq!(*backend, BackendId::AtrustNative);
        assert_eq!(flow.needs_certificate, !invisible, "ID Austria braucht das Zertifikat nur sichtbar");
        assert_eq!(t.effects, vec![Effect::OpenSession(BackendId::AtrustNative)], "kein Auswahlschritt");
        let parked = edit_of(&t.state).parked.expect("Lage gemerkt");
        assert_eq!((parked.page, parked.placement, parked.new_page), (2, Some(p), false));
    }
    // neue Seite nur mit „Auf neuer Seite“
    let s = step(workspace_state(Target::Free, PlacementOrigin::User, false), Event::NewPageToggled, &ws()).state;
    let t = step(s, sign_with(BackendId::Keystore), &ws());
    assert!(matches!(&t.state, State::Signing { position: PositionChoice::Manual(p), .. } if p.page_index == 3));
}

#[test]
fn sign_with_unavailable_signer_or_forbidden_position_stays() {
    let s = workspace_state(Target::Free, PlacementOrigin::Auto, false);
    // lokale BKU nicht verfügbar
    let t = step(s.clone(), sign_with(BackendId::LocalBku), &Context { ..ws() });
    let t2 =
        step(s.clone(), Event::SignRequested { backend: BackendId::LocalBku, options: options(false, true) }, &ws());
    assert!(matches!(t.state, State::Signing { .. }), "verfügbar laut Optionen");
    assert_eq!(t2.state, s);
    assert_eq!(t2.effects, vec![Effect::Notify("ng-signer-unavailable".into())]);
    // DocMDP: kein neues sichtbares Feld – vorhandenes Feld bzw. unsichtbar erlaubt
    let certified = |target, invisible| {
        let State::Positioning { queue, page, placement, new_page, doc, edit } =
            workspace_state(target, PlacementOrigin::User, invisible)
        else {
            unreachable!()
        };
        let doc = DocFacts { new_visible_field_allowed: false, signed: true, ..doc };
        State::Positioning { queue, page, placement, new_page, doc, edit }
    };
    let t = step(certified(Target::Free, false), sign_with(BackendId::Keystore), &ws());
    assert!(matches!(t.state, State::Positioning { .. }));
    assert_eq!(t.effects, vec![Effect::Notify("ng-constraint-certified".into())]);
    let t = step(certified(Target::Qr, false), sign_with(BackendId::Keystore), &ws());
    assert!(matches!(t.state, State::Positioning { .. }));
    for (target, invisible) in [(Target::Field(0), false), (Target::Free, true)] {
        let t = step(certified(target, invisible), sign_with(BackendId::Keystore), &ws());
        assert!(matches!(t.state, State::Signing { .. }), "{target:?}/{invisible}: {:?}", t.state);
    }
    // klassischer Ablauf: ohne Wirkung
    let classic = positioning(&[1]);
    let t = step(classic.clone(), sign_with(BackendId::Keystore), &Context::default());
    assert_eq!(t.state, classic);
}

// ---------------------------------------------------------------------------------------------
// Signieren, Abbruch, Fehler, Wiederholen (§3.6)
// ---------------------------------------------------------------------------------------------

fn signing_ws(backend: BackendId) -> State {
    let s = workspace_state(Target::Free, PlacementOrigin::User, false);
    step(s, sign_with(backend), &ws()).state
}

#[test]
fn signing_phases_in_the_workspace() {
    let s = signing_ws(BackendId::AtrustNative);
    let t = step(s, Event::PhaseCompleted(SigningPhase::OpeningSession), &ws());
    assert_eq!(t.effects, vec![Effect::ReadCertificate]);
    let t = step(t.state, Event::PhaseCompleted(SigningPhase::ReadingCertificate), &ws());
    assert!(matches!(t.effects[..], [Effect::Prepare { .. }]));
    let t = step(t.state, Event::PhaseCompleted(SigningPhase::Preparing), &ws());
    assert_eq!(t.effects, vec![Effect::SignDocument], "ein Job mit eigenem Abbruch (sign_one)");
    let t = step(t.state, Event::PhaseCompleted(SigningPhase::Signing), &ws());
    assert_eq!(t.effects, vec![Effect::Finish], "Sitzung bleibt bis nach der Selbstprüfung offen");
    let t = step(t.state, Event::PhaseCompleted(SigningPhase::Finishing), &ws());
    assert!(matches!(t.state, State::Output { edit: Some(_), .. }));
    assert_eq!(t.effects, vec![Effect::CloseSession, Effect::DeletePersisted, Effect::DecideOutput(DocId(1))]);
}

/// Abbruch (Esc/× bzw. Abbruch einer Maske) → Arbeitsbereich mit unveränderter Platzierung und Hinweis.
#[test]
fn cancel_returns_to_the_unchanged_workspace() {
    for target in [Target::Free, Target::Field(0), Target::Qr] {
        let origin = match target {
            Target::Free => PlacementOrigin::User,
            Target::Field(_) => PlacementOrigin::Field,
            Target::Qr => PlacementOrigin::Qr,
        };
        let before = workspace_state(target, origin, false);
        let s = step(before.clone(), sign_with(BackendId::AtrustNative), &ws()).state;
        for ev in [Event::Cancel, Event::BackendFailed(BackendError::Cancelled)] {
            let t = step(s.clone(), ev.clone(), &ws());
            assert_eq!(t.state, before, "{target:?} {ev:?}");
            assert_eq!(
                &t.effects[..3],
                &[Effect::CancelSession, Effect::DeletePersisted, Effect::Notify("ng-signing-cancelled".into())]
            );
            assert!(!opens_session(&t.effects));
        }
    }
}

#[test]
fn declined_in_the_app_keeps_the_session_for_retry() {
    let s = signing_ws(BackendId::AtrustNative);
    let t = step(s, Event::SigningDeclined, &ws());
    let State::Error { error, retry: Some(RetryTarget::Prepare), from: Some(_), .. } = &t.state else {
        panic!("{:?}", t.state)
    };
    assert_eq!(error.code(), "ng-signing-declined");
    assert_eq!(t.effects, vec![Effect::ReleaseDocument, Effect::DeletePersisted], "Sitzung bleibt offen");
    // „Erneut versuchen“: neue Bestätigung in derselben Sitzung (core-api verwendet sie wieder)
    let r = step(t.state.clone(), Event::Retry, &ws());
    assert!(matches!(r.state, State::Signing { phase: SigningPhase::OpeningSession, .. }));
    assert_eq!(r.effects, vec![Effect::OpenSession(BackendId::AtrustNative)]);
    // „Schließen“ = Abbrechen: Arbeitsbereich, Sitzung samt Passwort verworfen (F-72)
    let c = step(t.state, Event::Cancel, &ws());
    assert_eq!(c.state, workspace_state(Target::Free, PlacementOrigin::User, false));
    assert_eq!(c.effects[0], Effect::CancelSession);
}

#[test]
fn errors_keep_the_session_only_when_worth_retrying() {
    let cases: Vec<(Event, bool, Effect)> = vec![
        (Event::BackendFailed(BackendError::Network("x".into())), true, Effect::ReleaseDocument),
        (Event::BackendFailed(BackendError::Timeout), true, Effect::ReleaseDocument),
        (Event::BackendFailed(BackendError::Http { status: 500, reason: "x".into() }), true, Effect::ReleaseDocument),
        (
            Event::BackendFailed(BackendError::SecurityLayer { code: 1, info: "x".into() }),
            true,
            Effect::ReleaseDocument,
        ),
        (Event::BackendFailed(BackendError::WrongPassword), true, Effect::CloseSession),
        (Event::BackendFailed(BackendError::UnknownPage { diagnostic: "x".into() }), false, Effect::CloseSession),
        (Event::BackendFailed(BackendError::DocumentTooLarge), false, Effect::CloseSession),
        (Event::CmsTooLarge { needed: 20_000, reserve: 16_384 }, true, Effect::ReleaseDocument),
        (
            Event::SigningFailed(WorkflowError::Prepare { code: "sigblock-x".into(), detail: String::new() }),
            true,
            Effect::ReleaseDocument,
        ),
        (
            Event::SelfCheckFailed(pida_pades::InvariantViolation {
                invariant: pida_pades::Invariant::I3Cms,
                detail: "x".into(),
            }),
            false,
            Effect::CloseSession,
        ),
    ];
    for (ev, retry, effect) in cases {
        let t = step(signing_ws(BackendId::AtrustNative), ev.clone(), &ws());
        let State::Error { retry: r, .. } = &t.state else { panic!("{ev:?}: {:?}", t.state) };
        assert_eq!(r.is_some(), retry, "{ev:?}");
        assert_eq!(t.effects[0], effect, "{ev:?}");
        // klassisch schließt jeder Fehler die Sitzung
        let classic = signing(&[1], SigningPhase::Signing, BackendId::AtrustNative, PositionChoice::Auto);
        let c = step(classic, ev.clone(), &Context::default());
        if matches!(c.state, State::Error { .. }) {
            assert_eq!(c.effects[0], Effect::CloseSession, "{ev:?}");
        }
    }
}

#[test]
fn error_close_open_and_quit_discard_the_held_session() {
    let err = step(signing_ws(BackendId::AtrustNative), Event::BackendFailed(BackendError::Timeout), &ws()).state;
    let t = step(err.clone(), Event::Navigate(Step::Open), &ws());
    assert!(matches!(t.state, State::Idle { .. }));
    assert_eq!(t.effects[0], Effect::CancelSession);
    let t = step(err.clone(), Event::FilesAdded(vec![doc(7)]), &ws());
    assert!(matches!(t.state, State::Error { .. }));
    assert_eq!(t.effects, vec![Effect::CancelSession]);
    let t = step(err, Event::Quit, &ws());
    assert!(t.effects.starts_with(&[Effect::CancelSession]), "{:?}", t.effects);
}

// ---------------------------------------------------------------------------------------------
// Fertig, Hauptleiste, Startargument-Automatik
// ---------------------------------------------------------------------------------------------

#[test]
fn saved_with_pending_documents_shows_done_in_the_workspace() {
    let mut s = signing_ws(BackendId::Keystore);
    if let Some(q) = s.queue_mut() {
        q.add([doc(2)]);
    }
    for phase in [SigningPhase::OpeningSession, SigningPhase::ReadingCertificate, SigningPhase::Preparing] {
        s = step(s, Event::PhaseCompleted(phase), &ws()).state;
    }
    s = step(s, Event::PhaseCompleted(SigningPhase::Signing), &ws()).state;
    s = step(s, Event::PhaseCompleted(SigningPhase::Finishing), &ws()).state;
    let decided = pida_workflow::OutputDecision::Save { path: "/o.pdf".into(), exists: false };
    s = step(s, Event::OutputDecided(decided), &ws()).state;
    let t = step(s.clone(), Event::Saved(output_info(1)), &ws());
    assert!(matches!(t.state, State::Output { saved: Some(_), .. }), "kein F-88 im Arbeitsbereich: {:?}", t.state);
    // „Nächstes Dokument“
    let n = step(t.state.clone(), Event::Navigate(Step::Open), &ws());
    assert!(matches!(&n.state, State::Analyzing { queue } if queue.current_id() == Some(DocId(2))));
    // Hauptleiste Positionieren/Signieren gibt es im Arbeitsbereich nicht
    for step_ in [Step::Positioning, Step::Signing] {
        let x = step(t.state.clone(), Event::Navigate(step_), &ws());
        assert_eq!(x.state, t.state);
    }
    // mit Startargument-Automatik bleibt F-88 (D-06)
    let cli = Context { cli_auto: true, ..ws() };
    let t = step(s, Event::Saved(output_info(1)), &cli);
    assert!(matches!(&t.state, State::Analyzing { queue } if queue.current_id() == Some(DocId(2))));
}

/// D-06 im Arbeitsbereich: `-b KS -a` signiert automatisch, `-b MOBILE -a` bleibt im Arbeitsbereich.
#[test]
fn cli_auto_signs_only_with_signers_that_keep_the_document_on_the_device() {
    let keystore = Context {
        cli_auto: true,
        auto_position: true,
        default_backend: Some(pida_config::BackendChoice::Keystore),
        ..ws()
    };
    let t = step(State::Analyzing { queue: queue(&[1]) }, analyzed_with(1, 3, None, 0, true), &keystore);
    assert!(matches!(t.state, State::ChoosingBackend { auto_choice: true, edit: Some(_), .. }), "{:?}", t.state);
    let t = step(t.state, Event::BackendsProbed(options(false, true)), &keystore);
    assert_eq!(t.effects, vec![Effect::OpenSession(BackendId::Keystore)]);
    assert!(matches!(t.state, State::Signing { position: PositionChoice::Auto, .. }));

    let mobile = Context { default_backend: Some(pida_config::BackendChoice::IdAustria), ..keystore.clone() };
    let t = step(State::Analyzing { queue: queue(&[1]) }, analyzed_with(1, 3, None, 0, true), &mobile);
    let t = step(t.state, Event::BackendsProbed(options(false, true)), &mobile);
    assert!(matches!(t.state, State::Positioning { edit: Some(_), .. }), "{:?}", t.state);
    assert!(!opens_session(&t.effects));
    assert!(t.effects.contains(&Effect::AutoPlace(DocId(1))), "automatische Platzierung als Startlage");
    // ohne Standard (Wahl nicht eindeutig) ebenso
    let none = Context { default_backend: None, ..keystore };
    let t = step(State::Analyzing { queue: queue(&[1]) }, analyzed_with(1, 3, None, 0, true), &none);
    let t = step(t.state, Event::BackendsProbed(only_id_austria()), &none);
    assert!(matches!(t.state, State::Positioning { .. }), "{:?}", t.state);
}

/// Rückfall nach „Signatur fortsetzen“ mit nicht mehr verfügbarem Signiermittel: Auswahl; Wahl dort
/// signiert, Abbrechen führt in den Arbeitsbereich.
#[test]
fn resume_fallback_choice_and_cancel() {
    let plan = pida_workflow::ResumePlan { position: PositionChoice::Auto, backend: BackendId::Keystore };
    let s = State::Resuming { queue: queue(&[1]), plan, doc: None };
    let t = step(s, analyzed_with(1, 3, None, 0, true), &ws());
    let t = step(t.state, Event::BackendsProbed(options(false, false)), &ws());
    assert!(matches!(t.state, State::ChoosingBackend { auto_choice: false, .. }));
    let c = step(t.state.clone(), sign_with(BackendId::AtrustNative), &ws());
    assert_eq!(c.effects, vec![Effect::OpenSession(BackendId::AtrustNative)]);
    let x = step(t.state, Event::Cancel, &ws());
    assert!(
        matches!(x.state, State::Positioning { edit: Some(_), .. })
            || matches!(x.state, State::Analyzing { .. } | State::Idle { .. }),
        "{:?}",
        x.state
    );
}

/// Invariante A1 als Tabelle (ADR 0004 §3.2): je Zeile das Paar aus Ereignis und Vorzustand, das im
/// Arbeitsbereich eine Sitzung öffnet – und die Gegenprobe, die keine öffnet.
#[test]
fn a1_table_of_session_opening_pairs() {
    let cli = Context { cli_auto: true, auto_position: true, ..ws() };
    let with_default = |choice| Context { default_backend: Some(choice), ..cli.clone() };
    let choosing_auto = || {
        let mut s = choosing(&[1], PositionChoice::Auto, true);
        if let State::ChoosingBackend { edit, .. } = &mut s {
            *edit = Some(WorkspaceEdit::new(Vec::new(), None, false));
        }
        s
    };
    let resuming = || {
        let plan = pida_workflow::ResumePlan { position: PositionChoice::Auto, backend: BackendId::Keystore };
        State::Resuming { queue: queue(&[1]), plan, doc: Some(facts(1, 3)) }
    };
    let error_from_signing =
        || step(signing_ws(BackendId::AtrustNative), Event::BackendFailed(BackendError::Timeout), &ws()).state;
    //   (Zeile, Vorzustand, Ereignis, Kontext, öffnet Sitzung)
    let rows: Vec<(&str, State, Event, Context, bool)> = vec![
        (
            "SignRequested in Positioning",
            workspace_state(Target::Free, PlacementOrigin::Auto, false),
            sign_with(BackendId::AtrustNative),
            ws(),
            true,
        ),
        (
            "SignRequested in ChoosingBackend",
            choosing(&[1], PositionChoice::Auto, false),
            sign_with(BackendId::AtrustNative),
            ws(),
            true,
        ),
        (
            "BackendChosen in ChoosingBackend (Rückfall)",
            step(choosing(&[1], PositionChoice::Auto, false), Event::BackendsProbed(options(true, true)), &ws()).state,
            Event::BackendChosen(BackendId::AtrustNative),
            ws(),
            true,
        ),
        ("BackendsProbed in Resuming", resuming(), Event::BackendsProbed(options(false, true)), ws(), true),
        ("Retry im Fehler des Signierens", error_from_signing(), Event::Retry, ws(), true),
        (
            "BackendsProbed, cli_auto, Keystore",
            choosing_auto(),
            Event::BackendsProbed(options(false, true)),
            with_default(pida_config::BackendChoice::Keystore),
            true,
        ),
        // Gegenproben
        (
            "BackendsProbed, cli_auto, ID Austria",
            choosing_auto(),
            Event::BackendsProbed(options(false, true)),
            with_default(pida_config::BackendChoice::IdAustria),
            false,
        ),
        (
            "BackendsProbed ohne cli_auto",
            choosing_auto(),
            Event::BackendsProbed(options(false, true)),
            Context { default_backend: Some(pida_config::BackendChoice::Keystore), ..ws() },
            false,
        ),
        (
            "PositionConfirmed",
            workspace_state(Target::Free, PlacementOrigin::Auto, false),
            Event::PositionConfirmed,
            ws(),
            false,
        ),
        (
            "SettingsChanged",
            workspace_state(Target::Free, PlacementOrigin::Auto, false),
            Event::SettingsChanged,
            ws(),
            false,
        ),
        (
            "Retry im Fehler der Analyse",
            {
                let s = State::Analyzing { queue: queue(&[1]) };
                let err = WorkflowError::Rejected {
                    class: pida_pades::AcceptanceClass::K1Regular,
                    code: "error-MayNotBeAPDF".into(),
                };
                step(s, Event::AnalysisFailed { doc: DocId(1), error: err }, &ws()).state
            },
            Event::Retry,
            ws(),
            false,
        ),
    ];
    for (row, state, event, ctx, expected) in rows {
        let t = step(state, event, &ctx);
        assert_eq!(opens_session(&t.effects), expected, "{row}: {:?}", t.effects);
    }
}

// ---------------------------------------------------------------------------------------------
// Einstellungen › Dokument (K3, ADR 0004 §3.11 „sofort wirksam“): Platzhaltersuche QR | leere Felder
// ---------------------------------------------------------------------------------------------

fn fields_search() -> Context {
    Context { placeholder_search: true, placeholder_fields: true, ..ws() }
}

fn qr_search() -> Context {
    Context { placeholder_search: true, ..ws() }
}

/// Mit Platzhaltersuche „Leere Signaturfelder“ (`kind = fields`, PDF-Over `USE_FIELDS`) nimmt der Kern das
/// erste leere Signaturfeld automatisch; ohne Felder frei mit automatischer Platzierung; ohne Suche nur
/// angeboten (§8 F2); nie mit unsichtbarem Profil.
#[test]
fn field_search_uses_the_first_empty_field_automatically() {
    let t = step(State::Analyzing { queue: queue(&[1]) }, analyzed_with(1, 3, None, 2, true), &fields_search());
    let e = edit_of(&t.state);
    assert_eq!((e.target, e.origin, e.automatic), (Target::Field(0), PlacementOrigin::Field, true));
    assert!(!t.effects.contains(&Effect::AutoPlace(DocId(1))), "im Feld: keine freie Platzierung");
    // „Signieren“ signiert ins erste Feld
    let t = step(t.state, sign_with(BackendId::Keystore), &fields_search());
    assert!(
        matches!(&t.state, State::Signing { position: PositionChoice::Field(f), .. } if f.qualified_name == "Sig0"),
        "{:?}",
        t.state
    );
    // keine Felder: frei, automatisch platziert
    let t = step(State::Analyzing { queue: queue(&[1]) }, analyzed_with(1, 3, None, 0, true), &fields_search());
    let e = edit_of(&t.state);
    assert_eq!((e.target, e.origin, e.automatic), (Target::Free, PlacementOrigin::Auto, true));
    assert!(t.effects.contains(&Effect::AutoPlace(DocId(1))));
    // ohne Suche nur angeboten, mit unsichtbarem Profil nie
    for ctx in [ws(), Context { placeholder_fields: true, ..ws() }, Context { invisible: true, ..fields_search() }] {
        let t = step(State::Analyzing { queue: queue(&[1]) }, analyzed_with(1, 3, None, 2, true), &ctx);
        assert_eq!(edit_of(&t.state).target, Target::Free, "{ctx:?}");
        assert_eq!(edit_of(&t.state).fields.len(), 2, "Felder bleiben angeboten");
    }
    // Startargument-Automatik (D-06) signiert ebenfalls ins erste Feld (Vorschau = Ergebnis)
    let cli = Context { cli_auto: true, auto_position: true, ..fields_search() };
    let t = step(State::Analyzing { queue: queue(&[1]) }, analyzed_with(1, 3, None, 2, true), &cli);
    assert!(
        matches!(&t.state, State::ChoosingBackend { position: PositionChoice::Field(f), .. } if f.qualified_name == "Sig0"),
        "{:?}",
        t.state
    );
}

/// ADR 0004 Nachtrag 22: Die Automatik nimmt nur Felder, die der Kern nach der Einrastregel als geeignet markiert
/// (`auto_ok`); ungeeignete (seitengroß, 3 pt schmal) bleiben angeboten, werden aber übersprungen. Ohne geeignetes
/// Feld gilt die automatische Position.
#[test]
fn field_search_skips_fields_unsuitable_for_the_block() {
    let analyzed = |flags: &[bool]| {
        let Event::Analyzed {
            doc,
            class,
            placeholder,
            signed,
            page_count,
            start,
            warnings,
            new_visible_field_allowed,
            ..
        } = analyzed_with(1, 3, None, 0, true)
        else {
            unreachable!()
        };
        let fields = flags
            .iter()
            .enumerate()
            .map(|(i, ok)| EmptyFieldCandidate { auto_ok: *ok, ..field(&format!("Sig{i}")) })
            .collect();
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
        }
    };
    // erstes Feld ungeeignet, zweites geeignet: Ziel ist das zweite
    let t = step(State::Analyzing { queue: queue(&[1]) }, analyzed(&[false, true]), &fields_search());
    let e = edit_of(&t.state);
    assert_eq!((e.target, e.origin, e.automatic), (Target::Field(1), PlacementOrigin::Field, true));
    assert_eq!(e.fields.len(), 2, "beide Felder bleiben angeboten");
    let t = step(t.state, sign_with(BackendId::Keystore), &fields_search());
    assert!(
        matches!(&t.state, State::Signing { position: PositionChoice::Field(f), .. } if f.qualified_name == "Sig1"),
        "{:?}",
        t.state
    );
    // kein Feld geeignet: frei mit automatischer Platzierung
    let t = step(State::Analyzing { queue: queue(&[1]) }, analyzed(&[false, false]), &fields_search());
    let e = edit_of(&t.state);
    assert_eq!((e.target, e.origin, e.automatic), (Target::Free, PlacementOrigin::Auto, true));
    assert!(t.effects.contains(&Effect::AutoPlace(DocId(1))));
}

/// Geänderte Einstellungen › Dokument wirken sofort auf ein automatisch platziertes Dokument: Feld ↔ QR ↔
/// frei; fehlt die QR-Suche, wird sie nachgeholt (`SearchPlaceholder` → `PlaceholderSearched`). Ein von Hand
/// platziertes Dokument bleibt, bekommt den QR-Platzhalter aber angeboten.
#[test]
fn document_settings_replace_automatic_placements_only() {
    // Start ohne Suche: frei, automatisch, ein leeres Feld angeboten, QR unbekannt
    let t = step(State::Analyzing { queue: queue(&[1]) }, analyzed_with(1, 3, None, 1, true), &ws());
    let s = t.state;
    assert_eq!(edit_of(&s).qr_search, None);
    // Suche „Leere Signaturfelder“ an: erstes Feld, keine freie Platzierung nötig
    let t = step(s, Event::SettingsChanged, &fields_search());
    assert_eq!((edit_of(&t.state).target, edit_of(&t.state).origin), (Target::Field(0), PlacementOrigin::Field));
    assert!(!t.effects.contains(&Effect::AutoPlace(DocId(1))));
    assert!(t.effects.contains(&Effect::BuildBlockPreview(DocId(1))));
    // auf „QR-Code-Platzhalter“: QR noch nicht gesucht → frei (automatisch) und Suche nachholen
    let t = step(t.state, Event::SettingsChanged, &qr_search());
    assert_eq!(edit_of(&t.state).target, Target::Free);
    assert!(t.effects.contains(&Effect::AutoPlace(DocId(1))), "{:?}", t.effects);
    assert!(t.effects.contains(&Effect::SearchPlaceholder(DocId(1))), "{:?}", t.effects);
    // Ergebnis der Suche: QR-Platzhalter → Ziel QR (automatisch)
    let t = step(t.state, Event::PlaceholderSearched { doc: DocId(1), qr: Some(qr()) }, &qr_search());
    let e = edit_of(&t.state);
    assert_eq!((e.target, e.origin, e.automatic), (Target::Qr, PlacementOrigin::Qr, true));
    assert_eq!(e.qr, Some(qr()));
    // verspätetes Ergebnis eines anderen Dokuments: ohne Wirkung
    let other = step(t.state.clone(), Event::PlaceholderSearched { doc: DocId(9), qr: None }, &qr_search());
    assert_eq!(other.state, t.state);
    // Suche aus: wieder frei mit automatischer Platzierung, QR nicht mehr angeboten, Ergebnis gemerkt
    let t = step(t.state, Event::SettingsChanged, &ws());
    let e = edit_of(&t.state);
    assert_eq!((e.target, e.origin, e.qr.is_none()), (Target::Free, PlacementOrigin::Auto, true));
    assert_eq!(e.qr_search, Some(Some(qr())));
    assert!(t.effects.contains(&Effect::AutoPlace(DocId(1))));
    // wieder an: ohne neue Suche zurück in den QR-Platzhalter
    let t = step(t.state, Event::SettingsChanged, &qr_search());
    assert_eq!(edit_of(&t.state).target, Target::Qr);
    assert!(!t.effects.contains(&Effect::SearchPlaceholder(DocId(1))));

    // von Hand verschoben: bleibt bei jeder Änderung; der QR-Platzhalter wird nur angeboten
    let p = Placement { page_index: 1, x: 60.0, y: 400.0, width: 230.0, height: 49.0 };
    let t = step(t.state, Event::PlacementChanged(p), &qr_search());
    assert!(!edit_of(&t.state).automatic);
    for ctx in [ws(), fields_search(), qr_search()] {
        let x = step(t.state.clone(), Event::SettingsChanged, &ctx);
        let e = edit_of(&x.state);
        assert_eq!((e.target, e.origin), (Target::Free, PlacementOrigin::User), "{ctx:?}");
        assert!(matches!(x.state, State::Positioning { placement: Some(q), .. } if q == p));
        assert!(!x.effects.contains(&Effect::AutoPlace(DocId(1))), "{ctx:?}");
        assert_eq!(e.qr.is_some(), ctx == qr_search(), "angeboten nur mit QR-Suche");
    }
}

/// Mobile-Check 16.10.2026 (`qr.pdf`): ohne Suche automatisch auf der neuen Seite platziert, dann QR-Suche an →
/// im QR-Platzhalter; „Auf neuer Seite“ entfällt und die Ansicht wechselt von der virtuellen Seite auf die Seite
/// des Platzhalters (vorher blieb „Seite 1 / 2“ mit leerer neuer Seite). Ebenso nach Wahl eines Felds bzw. des QR
/// durch die Nutzerin und mit der Suche „Leere Signaturfelder“. Suche wieder aus: die Automatik bestimmt die Seite
/// neu (`AutoPlace`).
#[test]
fn target_on_an_existing_page_ends_the_new_page() {
    let one_page = |fields: usize| {
        let t = step(State::Analyzing { queue: queue(&[1]) }, analyzed_with(1, 1, None, fields, true), &ws());
        // die PDF-AS-Regel fand keinen Platz: neue Seite
        let auto = Placement { page_index: 1, x: 180.0, y: 820.0, width: 230.0, height: 82.0 };
        let s = step(t.state, Event::AutoPlaced(auto), &ws()).state;
        assert_eq!(positioning_parts(&s), (1, Some(auto), true), "auf neuer Seite");
        s
    };
    let on_page_zero = |s: &State| {
        let (page, placement, new_page) = positioning_parts(s);
        assert_eq!((page, new_page), (0, false), "{s:?}");
        assert_eq!(placement.map(|p| p.page_index), Some(0), "keine Lage auf der virtuellen Seite");
    };
    // Suche „QR-Code-Platzhalter“ eingeschaltet → nachgeholte Suche findet ihn
    let t = step(one_page(0), Event::SettingsChanged, &qr_search());
    assert!(t.effects.contains(&Effect::SearchPlaceholder(DocId(1))));
    let t = step(t.state, Event::PlaceholderSearched { doc: DocId(1), qr: Some(qr()) }, &qr_search());
    assert_eq!(edit_of(&t.state).target, Target::Qr);
    on_page_zero(&t.state);
    assert!(t.effects.contains(&Effect::RenderPage { doc: DocId(1), page: 0 }), "{:?}", t.effects);
    // wieder an, nachdem die Suche schon lief: direkt beim Einschalten
    let off = step(t.state, Event::SettingsChanged, &ws());
    assert!(off.effects.contains(&Effect::AutoPlace(DocId(1))), "Automatik bestimmt die Seite neu");
    let auto = Placement { page_index: 1, x: 180.0, y: 820.0, width: 230.0, height: 82.0 };
    let s = step(off.state, Event::AutoPlaced(auto), &ws()).state;
    assert!(positioning_parts(&s).2, "wieder auf neuer Seite");
    let t = step(s, Event::SettingsChanged, &qr_search());
    assert_eq!(edit_of(&t.state).target, Target::Qr);
    on_page_zero(&t.state);
    // Suche „Leere Signaturfelder“
    let t = step(one_page(1), Event::SettingsChanged, &fields_search());
    assert_eq!(edit_of(&t.state).target, Target::Field(0));
    on_page_zero(&t.state);
    // „Ins Feld setzen“ von Hand
    let t = step(one_page(1), Event::TargetChosen(Target::Field(0)), &ws());
    assert_eq!(edit_of(&t.state).target, Target::Field(0));
    on_page_zero(&t.state);
    // frei gewählt: die Seite bleibt Sache der Automatik
    let t = step(one_page(0), Event::TargetChosen(Target::Free), &ws());
    assert_eq!(positioning_parts(&t.state).0, 1);
    assert!(t.effects.contains(&Effect::AutoPlace(DocId(1))));
}

/// Jede Platzierung durch die Nutzerin beendet die Automatik: „Ins Feld setzen“, QR wählen, „Frei
/// platzieren“/„Freie Fläche suchen“, Seite, neue Seite, Ziehen. Ein von Hand gewähltes QR-Ziel bleibt auch
/// nach dem Ausschalten der Suche.
#[test]
fn user_placements_end_the_automatic() {
    let s = workspace_state(Target::Free, PlacementOrigin::Auto, false);
    assert!(edit_of(&s).automatic);
    let p = Placement { page_index: 1, x: 60.0, y: 400.0, width: 230.0, height: 49.0 };
    for event in [
        Event::TargetChosen(Target::Field(0)),
        Event::TargetChosen(Target::Qr),
        Event::TargetChosen(Target::Free),
        Event::PageChanged(1),
        Event::NewPageToggled,
        Event::PlacementChanged(p),
    ] {
        let t = step(s.clone(), event.clone(), &qr_search());
        assert!(!edit_of(&t.state).automatic, "{event:?}");
    }
    // automatische Ergebnisse und Blockgröße lassen die Automatik bestehen
    for event in [Event::AutoPlaced(p), Event::BlockResized(p), Event::InvisibleSet(true)] {
        let t = step(s.clone(), event.clone(), &ws());
        assert!(edit_of(&t.state).automatic, "{event:?}");
    }
    // QR von Hand gewählt, dann Suche aus: Ziel und Angebot bleiben
    let t = step(
        workspace_state(Target::Free, PlacementOrigin::User, false),
        Event::TargetChosen(Target::Qr),
        &qr_search(),
    );
    let t = step(t.state, Event::SettingsChanged, &ws());
    let e = edit_of(&t.state);
    assert_eq!((e.target, e.qr.is_some()), (Target::Qr, true));
    assert!(!t.effects.contains(&Effect::AutoPlace(DocId(1))));
}

/// Profil „unsichtbar“ in den Einstellungen ein und wieder aus („sofort wirksam“): das offene Dokument wird
/// unsichtbar und danach wieder sichtbar (automatisch platziert); eine eigene Wahl „Unsichtbar signieren“
/// bleibt auch nach dem Wechsel auf ein sichtbares Profil. Startet das Dokument mit dem unsichtbaren Profil,
/// wird es mit einem sichtbaren Profil ebenfalls sichtbar.
#[test]
fn invisible_profile_switch_is_reversible_unless_chosen_by_the_user() {
    let inv = Context { invisible: true, ..ws() };
    let t = step(State::Analyzing { queue: queue(&[1]) }, analyzed_with(1, 3, None, 1, true), &ws());
    let t = step(t.state, Event::SettingsChanged, &inv);
    let e = edit_of(&t.state);
    assert!(e.invisible && e.invisible_by_profile);
    assert!(!t.effects.contains(&Effect::AutoPlace(DocId(1))), "unsichtbar: keine Platzierung");
    let t = step(t.state, Event::SettingsChanged, &ws());
    let e = edit_of(&t.state);
    assert!(!e.invisible && !e.invisible_by_profile, "wieder sichtbar");
    assert!(e.automatic && e.auto_free());
    assert!(t.effects.contains(&Effect::AutoPlace(DocId(1))), "{:?}", t.effects);
    // „Leere Signaturfelder“ unter dem unsichtbaren Profil: kein Feld; wieder sichtbar → erstes Feld
    let fields_inv = Context { invisible: true, ..fields_search() };
    let t = step(t.state, Event::SettingsChanged, &fields_inv);
    assert_eq!((edit_of(&t.state).target, edit_of(&t.state).invisible), (Target::Free, true));
    let t = step(t.state, Event::SettingsChanged, &fields_search());
    assert_eq!((edit_of(&t.state).target, edit_of(&t.state).invisible), (Target::Field(0), false));

    // eigene Wahl bleibt
    let t = step(t.state, Event::InvisibleSet(true), &fields_search());
    let t = step(t.state, Event::SettingsChanged, &fields_inv);
    let t = step(t.state, Event::SettingsChanged, &fields_search());
    let e = edit_of(&t.state);
    assert!(e.invisible && !e.invisible_by_profile, "von der Nutzerin gewählt");

    // Start mit dem unsichtbaren Profil, dann sichtbares Profil
    let t = step(State::Analyzing { queue: queue(&[1]) }, analyzed_with(1, 3, None, 0, true), &inv);
    assert!(edit_of(&t.state).invisible_by_profile);
    let t = step(t.state, Event::SettingsChanged, &ws());
    assert!(!edit_of(&t.state).invisible);
    assert!(t.effects.contains(&Effect::AutoPlace(DocId(1))), "{:?}", t.effects);
    let t = step(t.state, sign_with(BackendId::Keystore), &ws());
    assert!(matches!(&t.state, State::Signing { position: PositionChoice::Manual(_), .. }), "{:?}", t.state);
}

// ---------------------------------------------------------------------------------------------
// Einstellung „Automatische Positionierung“ (ADR 0004 Nachtrag 8)
// ---------------------------------------------------------------------------------------------

fn auto_pos() -> Context {
    Context { auto_position: true, ..ws() }
}

fn positioning_parts(s: &State) -> (usize, Option<Placement>, bool) {
    match s {
        State::Positioning { page, placement, new_page, .. } => (*page, *placement, *new_page),
        other => panic!("nicht im Arbeitsbereich: {other:?}"),
    }
}

/// Option an: nach der Analyse automatisch platziert und gesperrt („bereit zum Signieren“). Ziehen,
/// Tastatur, Seitenwechsel und jede Zielwahl bleiben ohne Wirkung (keine Effekte); „Unsichtbar“, „Auf neuer
/// Seite“ und „Signieren mit“ wirken. Es öffnet sich nie eine Sitzung ohne „Signieren mit“ (A1, F-54).
#[test]
fn auto_position_locks_the_automatic_placement() {
    let t = step(State::Analyzing { queue: queue(&[1]) }, analyzed_with(1, 3, None, 1, true), &auto_pos());
    let e = edit_of(&t.state);
    assert_eq!(e.lock, PlacementLock::Locked);
    assert!(e.placement_locked() && e.automatic && e.auto_free());
    assert!(t.effects.contains(&Effect::AutoPlace(DocId(1))), "automatisch platziert wie bisher: {:?}", t.effects);
    assert!(!opens_session(&t.effects) && !t.effects.contains(&Effect::ProbeBackends));
    // die automatische Lage trifft ein und gilt (gesperrt heißt nicht: ohne Automatik)
    let auto = Placement { page_index: 2, x: 50.0, y: 260.0, width: 230.0, height: 49.0 };
    let t = step(t.state, Event::AutoPlaced(auto), &auto_pos());
    assert_eq!(positioning_parts(&t.state), (2, Some(auto), false));
    let locked = t.state;
    let moved = Placement { page_index: 0, x: 80.0, y: 500.0, width: 230.0, height: 49.0 };
    for event in [
        Event::PlacementChanged(moved),
        Event::PageChanged(0),
        Event::TargetChosen(Target::Free),
        Event::TargetChosen(Target::Field(0)),
        Event::TargetChosen(Target::Qr),
    ] {
        let t = step(locked.clone(), event.clone(), &auto_pos());
        assert_eq!(t.state, locked, "{event:?} verschiebt den gesperrten Block nicht");
        assert!(t.effects.is_empty(), "{event:?}: {:?}", t.effects);
    }
    // „Unsichtbar signieren“ und „Auf neuer Seite“ bleiben bedienbar, die Sperre bleibt
    let t = step(locked.clone(), Event::InvisibleSet(true), &auto_pos());
    assert!(edit_of(&t.state).invisible && edit_of(&t.state).placement_locked());
    let t = step(locked.clone(), Event::NewPageToggled, &auto_pos());
    let (page, placement, new_page) = positioning_parts(&t.state);
    assert!(new_page && page == 3 && placement.is_some_and(|p| p.page_index == 3), "{:?}", t.state);
    assert!(edit_of(&t.state).placement_locked(), "neue Seite: weiterhin gesperrt");
    let t = step(t.state, Event::PlacementChanged(moved), &auto_pos());
    assert!(positioning_parts(&t.state).2, "auf der neuen Seite gesperrt");
    // „Signieren mit“ ist die ausdrückliche Aktion: signiert die automatische Lage
    let t = step(locked, sign_with(BackendId::Keystore), &auto_pos());
    assert!(
        matches!(&t.state, State::Signing { position: PositionChoice::Manual(p), .. } if *p == auto),
        "{:?}",
        t.state
    );
    assert!(opens_session(&t.effects));
}

/// „Diesmal selbst platzieren“ hebt die Sperre nur für dieses Dokument auf: danach normales Ziehen und
/// Einrasten, auch nach weiteren Einstellungsänderungen; das nächste Dokument ist wieder gesperrt. Abbruch
/// des Signierens stellt den Bearbeitungszustand samt Sperre unverändert wieder her.
#[test]
fn unlock_applies_only_to_this_document() {
    let t = step(State::Analyzing { queue: queue(&[1, 2]) }, analyzed_with(1, 3, None, 0, true), &auto_pos());
    assert!(edit_of(&t.state).placement_locked());
    let before = t.state.clone();
    let t = step(t.state, Event::PlacementUnlocked, &auto_pos());
    assert!(t.effects.is_empty(), "startet nichts: {:?}", t.effects);
    assert_eq!(edit_of(&t.state).lock, PlacementLock::Released);
    assert_eq!(positioning_parts(&t.state), positioning_parts(&before), "Lage unverändert");
    let moved = Placement { page_index: 0, x: 80.0, y: 500.0, width: 230.0, height: 49.0 };
    let t = step(t.state, Event::PlacementChanged(moved), &auto_pos());
    assert_eq!(positioning_parts(&t.state), (0, Some(moved), false), "normal verschiebbar");
    assert_eq!(edit_of(&t.state).origin, PlacementOrigin::User);
    // Einstellungen erneut übernommen (Option weiter an bzw. aus und wieder an): bleibt freigegeben
    let t = step(t.state, Event::SettingsChanged, &auto_pos());
    assert_eq!(edit_of(&t.state).lock, PlacementLock::Released);
    // nächstes Dokument: wieder gesperrt
    let t = step(t.state, Event::Cancel, &auto_pos());
    assert!(matches!(&t.state, State::Analyzing { queue } if queue.current_id() == Some(DocId(2))), "{:?}", t.state);
    let t = step(t.state, analyzed_with(2, 3, None, 0, true), &auto_pos());
    assert_eq!(edit_of(&t.state).lock, PlacementLock::Locked);
    // Signieren abbrechen → zurück mit Sperre
    let t = step(t.state, sign_with(BackendId::Keystore), &auto_pos());
    let t = step(t.state, Event::Cancel, &auto_pos());
    assert!(edit_of(&t.state).placement_locked(), "{:?}", t.state);
    // ohne Sperre ist die Freigabe ohne Wirkung
    let s = workspace_state(Target::Free, PlacementOrigin::User, false);
    let t = step(s.clone(), Event::PlacementUnlocked, &ws());
    assert_eq!(t.state, s);
}

/// Option aus: Verhalten wie bisher (nicht gesperrt, Ziehen wirkt). Die Einstellung wirkt sofort: an →
/// gesperrt, solange die Lage aus der Automatik stammt; aus → wieder frei. Von Hand platziert bleibt frei.
/// Der klassische Ablauf (ohne Arbeitsbereich) bleibt unverändert.
#[test]
fn auto_position_off_and_switched_in_the_settings() {
    let t = step(State::Analyzing { queue: queue(&[1]) }, analyzed_with(1, 3, None, 0, true), &ws());
    assert_eq!(edit_of(&t.state).lock, PlacementLock::Off);
    let moved = Placement { page_index: 0, x: 80.0, y: 500.0, width: 230.0, height: 49.0 };
    let m = step(t.state.clone(), Event::PlacementChanged(moved), &ws());
    assert_eq!(positioning_parts(&m.state).1, Some(moved), "Option aus: wie bisher");
    // eingeschaltet: das automatisch platzierte Dokument ist sofort gesperrt
    let t = step(t.state, Event::SettingsChanged, &auto_pos());
    assert!(edit_of(&t.state).placement_locked());
    assert!(!opens_session(&t.effects));
    // ausgeschaltet: frei
    let t = step(t.state, Event::SettingsChanged, &ws());
    assert_eq!(edit_of(&t.state).lock, PlacementLock::Off);
    // von Hand platziert, dann eingeschaltet: bleibt frei
    let t = step(m.state, Event::SettingsChanged, &auto_pos());
    assert_eq!(edit_of(&t.state).lock, PlacementLock::Off);
    // klassischer Ablauf (ohne Arbeitsbereich): unverändert direkt zur Signiermittel-Auswahl
    let classic = Context { auto_position: true, ..Context::default() };
    let t = step(State::Analyzing { queue: queue(&[1]) }, analyzed(1, 3, None, false), &classic);
    assert!(matches!(t.state, State::ChoosingBackend { .. }), "{:?}", t.state);
}

/// Platzhaltersuche hat Vorrang: der QR-Platzhalter wird automatisch benutzt und ist gesperrt; die
/// Startargument-Automatik (D-06, `-b` + `-a`) bleibt der klassische automatische Weg.
#[test]
fn auto_position_with_placeholder_search_and_start_arguments() {
    let ctx = Context { auto_position: true, ..qr_search() };
    let t = step(
        State::Analyzing { queue: queue(&[1]) },
        analyzed_with(1, 3, Some(PlaceholderFound::Qr(qr())), 0, true),
        &ctx,
    );
    let e = edit_of(&t.state);
    assert_eq!((e.target, e.origin, e.lock), (Target::Qr, PlacementOrigin::Qr, PlacementLock::Locked));
    let s = t.state.clone();
    let t = step(s.clone(), Event::TargetChosen(Target::Free), &ctx);
    assert_eq!(t.state, s, "„Frei platzieren“ gesperrt");
    // D-06: -b KS -a → klassischer automatischer Weg (Signiermittel-Prüfung), unverändert
    let cli = Context { cli_auto: true, ..auto_pos() };
    let t = step(State::Analyzing { queue: queue(&[1]) }, analyzed_with(1, 3, None, 0, true), &cli);
    assert!(matches!(t.state, State::ChoosingBackend { auto_choice: true, .. }), "{:?}", t.state);
    assert_eq!(t.effects, vec![Effect::ProbeBackends]);
}
