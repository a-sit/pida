//! Zustände, Ereignisse, Effekte und Hauptleiste (docs/analyse/01 §3.3–§3.5).
//!
//! Der Reducer selbst steht in [`crate::reducer`]; hier liegen die Datentypen und die
//! Hauptleisten-Matrix.

use std::collections::BTreeMap;
use std::path::PathBuf;

use pida_backend_api::{BackendError, BackendId, Capabilities, CertNeed};
use pida_config::BackendChoice;
use pida_pades::{AcceptanceClass, InvariantViolation, OutputInfo};
use pida_placement::{EmptyFieldCandidate, Placement, QrPlaceholder};

use crate::batch::{BatchRun, DocSummary};
use crate::output::{ExitCode, OutputDecision};
use crate::queue::{DocId, DocOutcome, InputDocument, Queue};

/// Schritte der Hauptleiste (F-100).
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Step {
    /// Öffnen
    Open,
    /// Positionierung
    Positioning,
    /// Signieren
    Signing,
    /// Fertig (nur Anzeige)
    Finish,
}

/// Zustand eines Schritts in der Hauptleiste (Tabelle docs/analyse/01 §3.5).
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct MainBarStep {
    /// Schritt
    pub step: Step,
    /// erreicht („A“)
    pub active: bool,
    /// klickbar („E“)
    pub enabled: bool,
}

/// Wie positioniert wird.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PositionChoice {
    /// `p:auto;x:auto;y:auto`
    Auto,
    /// manuell
    Manual(Placement),
    /// QR-Platzhalter
    Placeholder(QrPlaceholder),
    /// leeres Signaturfeld
    Field(EmptyFieldCandidate),
    /// unsichtbar
    Invisible,
}

impl PositionChoice {
    /// Sichtbarer Signaturblock?
    pub fn is_visible(&self) -> bool {
        !matches!(self, PositionChoice::Invisible)
    }

    /// Manuelle Position, falls gewählt (F-43: bleibt beim Rücksprung erhalten).
    pub fn manual(&self) -> Option<&Placement> {
        match self {
            PositionChoice::Manual(p) => Some(p),
            _ => None,
        }
    }
}

/// Gefundener Platzhalter beim Öffnen (F-20/F-21).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PlaceholderFound {
    /// leere Signaturfelder (`USE_FIELDS`)
    Fields(Vec<EmptyFieldCandidate>),
    /// QR-Platzhalter (`USE_MARKER`, id 1, SORTED)
    Qr(QrPlaceholder),
}

impl PlaceholderFound {
    /// Nichts Verwendbares (leere Feldliste).
    pub fn is_empty(&self) -> bool {
        matches!(self, PlaceholderFound::Fields(f) if f.is_empty())
    }
}

/// Antwort auf „Platzhalter verwenden?“.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PlaceholderAnswer {
    /// Ja
    Yes,
    /// Nein → normale Positionierung
    No,
    /// Abbrechen → Dokument verworfen
    Cancel,
}

/// Herkunft der Platzierung im Arbeitsbereich (ADR 0004 §3.3; Typ für Reducer **und** Bridge, §3.0
/// Regel 9). Der Reducer erzeugt die Werte ab K1; bis dahin dient der Typ nur dem Vertrag.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacementOrigin {
    /// automatische Platzierung (PDF-AS-Regel wie `-a`, „Freie Fläche suchen“)
    Auto,
    /// von der Nutzerin gesetzt (Ziehen, Tastatur, Doppelklick)
    User,
    /// in ein leeres Signaturfeld gesetzt („Ins Feld setzen“)
    Field,
    /// im QR-Platzhalter (automatisch bei eingeschalteter Platzhaltersuche)
    Qr,
}

/// Ziel der Platzierung im Arbeitsbereich (ADR 0004 §3.3; Gegenstück der Bridge:
/// `pida_core_api::PlacementTarget`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Target {
    /// frei platziert (Lage in `State::Positioning::placement`)
    Free,
    /// leeres Signaturfeld (Index in [`WorkspaceEdit::fields`])
    Field(usize),
    /// QR-Platzhalter ([`WorkspaceEdit::qr`])
    Qr,
}

/// Lage beim Verlassen der Positionierung („Signieren“): damit Abbruch und Fehler-„Schließen“ den
/// Arbeitsbereich **unverändert** wiederherstellen (ADR 0004 §3.6), auch wenn ein Feld, ein
/// QR-Platzhalter oder „unsichtbar“ signiert wurde.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ParkedPlacement {
    /// angezeigte Seite
    pub page: usize,
    /// freie Lage (`None` = keine Vorschau)
    pub placement: Option<Placement>,
    /// virtuelle neue Seite
    pub new_page: bool,
}

/// Sperre der Platzierung durch die Einstellung „Automatische Positionierung“ (`signature.auto_position` bzw.
/// `-a`, ADR 0004 Nachtrag 8) – gilt nur für das Dokument dieses Bearbeitungszustands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacementLock {
    /// nicht gesperrt (Einstellung aus, aus der Stapelübersicht geöffnet, schon von Hand platziert, als die
    /// Einstellung eingeschaltet wurde)
    #[default]
    Off,
    /// gesperrt: Ziehen, Tastatur, Seitenwechsel und Zielwahl („Ins Feld setzen“, „Frei platzieren“, „Neu
    /// platzieren“/„Freie Fläche suchen“) bleiben ohne Wirkung; „Unsichtbar signieren“, „Auf neuer Seite“ und
    /// „Signieren“ bleiben bedienbar
    Locked,
    /// „Diesmal selbst platzieren“ ([`Event::PlacementUnlocked`]): für dieses Dokument aufgehoben, auch wenn
    /// die Einstellung erneut eingeschaltet wird
    Released,
}

/// Höchstzahl der Signaturblöcke je Dokument (Mehrfachsignatur: je Block eine eigene Signatur, Entscheidung
/// vom 13.11.2026).
pub const MAX_BLOCKS: usize = 10;

/// Ein weiterer, gerade nicht ausgewählter Signaturblock desselben Dokuments (Mehrfachsignatur). Der
/// ausgewählte Block liegt wie bisher in `State::Positioning` (`page`, `placement`) und in
/// [`WorkspaceEdit::target`]/[`WorkspaceEdit::origin`]; beim Auswählen eines anderen Blocks tauschen beide die
/// Plätze, damit alle Werkzeuge (Ziehen, Tastatur, Zielwahl, automatische Platzierung) unverändert auf den
/// ausgewählten Block wirken. Mit „Auf neuer Seite“ dürfen Blöcke auf der virtuellen neuen Seite liegen
/// (`page == Seitenzahl`); sie stehen dann in der Reihenfolge zuerst, weil nur die erste Signatur die Seite anhängt.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BlockSlot {
    /// Ziel
    pub target: Target,
    /// Herkunft der Lage
    pub origin: PlacementOrigin,
    /// Seite des Blocks
    pub page: usize,
    /// freie Lage (`None` = keine Vorschau)
    pub placement: Option<Placement>,
}

/// Laufende Mehrfachsignatur: alle Positionen in der Reihenfolge der Blöcke und die gerade bearbeitete.
/// Jede Runde signiert das Ergebnis der vorigen (`Effect::NextSignature`); bricht eine Runde ab, wird alles
/// verworfen und „Erneut versuchen“ beginnt wieder bei der ersten.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Rounds {
    /// Positionen aller Blöcke (mindestens zwei)
    pub positions: Vec<PositionChoice>,
    /// laufende Runde (0-basiert)
    pub index: usize,
}

/// Bearbeitungszustand des Dokuments im Arbeitsbereich-Modus (ADR 0004 §3.3, nur mit
/// [`Context::workspace`]). Wandert durch `Signing`, `ChoosingBackend` (Rückfall/Startargument-
/// Automatik), `Error` und `Output` mit.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceEdit {
    /// Ziel
    pub target: Target,
    /// Herkunft der Lage
    pub origin: PlacementOrigin,
    /// unsichtbar signieren (nur dieses Dokument; beim unsichtbaren Profil immer)
    pub invisible: bool,
    /// leere, nicht gesperrte Signaturfelder (aus [`Event::Analyzed::fields`]) – immer angeboten; automatisch
    /// benutzt nur das erste und nur mit Platzhaltersuche „Leere Signaturfelder“ (`placeholder.search`,
    /// `kind = fields`; Einstellungen › Dokument, sonst §8 F2)
    pub fields: Vec<EmptyFieldCandidate>,
    /// angebotener bzw. benutzter QR-Platzhalter (nur mit Platzhaltersuche, `kind = marker`; ein von der
    /// Nutzerin gewähltes QR-Ziel bleibt auch nach dem Ausschalten der Suche)
    pub qr: Option<QrPlaceholder>,
    /// aus der Stapelübersicht geöffnet (ADR 0004 §3.8, ab K2)
    #[serde(default)]
    pub return_to_batch: bool,
    /// Lage beim „Signieren“ (gesetzt beim Verlassen von `Positioning`)
    #[serde(default)]
    pub parked: Option<ParkedPlacement>,
    /// Ergebnis der QR-Platzhaltersuche dieses Dokuments (`None` = noch nicht gesucht bzw. unbekannt;
    /// `Some(None)` = gesucht, keiner gefunden). Bleibt beim Ausschalten der Suche erhalten, damit ein
    /// erneutes Einschalten ohne neue Bildsuche wirkt (Einstellungen › Dokument, K3)
    #[serde(default)]
    pub qr_search: Option<Option<QrPlaceholder>>,
    /// Lage und Ziel stammen aus der Automatik (Startlage nach der Analyse bzw. Nachführung nach geänderten
    /// Einstellungen) und folgen den Einstellungen › Dokument; `false` nach jeder Platzierung durch die
    /// Nutzerin (Ziehen, Tastatur, Seite, neue Seite, „Ins Feld setzen“, „Frei platzieren“/„Freie Fläche
    /// suchen“). Gilt in der Projektion als `DocumentDetectionView::auto_placed`.
    #[serde(default)]
    pub automatic: bool,
    /// „unsichtbar“ nur wegen des unsichtbaren Profils (nicht von der Nutzerin bzw. ihrem Plan gewählt):
    /// wechselt das Profil wieder auf ein sichtbares, wird der Block wieder sichtbar (Einstellungen „sofort
    /// wirksam“, ADR 0004 §3.11, K3). Eine eigene Wahl („Unsichtbar signieren“) bleibt.
    #[serde(default)]
    pub invisible_by_profile: bool,
    /// Einstellung „Automatische Positionierung“: Platzierung gesperrt bzw. für dieses Dokument freigegeben
    /// ([`WorkspaceEdit::follow_auto_position`], ADR 0004 Nachtrag 8)
    #[serde(default)]
    pub lock: PlacementLock,
    /// weitere Signaturblöcke in Reihenfolge, ohne den ausgewählten (Mehrfachsignatur; leer = ein Block wie
    /// bisher)
    #[serde(default)]
    pub others: Vec<BlockSlot>,
    /// Stelle des ausgewählten Blocks in der Reihenfolge aller Blöcke (`0..=others.len()`)
    #[serde(default)]
    pub active: usize,
    /// laufende Mehrfachsignatur (nur in `Signing`, `Error` und `Output`)
    #[serde(default)]
    pub rounds: Option<Rounds>,
}

impl WorkspaceEdit {
    /// Anzahl der Signaturblöcke (unsichtbar: genau eine Signatur, weitere Blöcke ruhen).
    pub fn block_count(&self) -> usize {
        if self.invisible { 1 } else { 1 + self.others.len() }
    }

    /// Ziel schon von einem anderen (nicht ausgewählten) Block belegt?
    pub fn target_taken(&self, target: Target) -> bool {
        target != Target::Free && self.others.iter().any(|b| b.target == target)
    }

    /// Stelle des ausgewählten Blocks, auf die vorhandenen Blöcke begrenzt.
    pub fn active_index(&self) -> usize {
        self.active.min(self.others.len())
    }

    /// Start nach der Analyse: frei mit automatischer Platzierung bzw. im QR-Platzhalter (Lage aus der
    /// Automatik). `invisible` = unsichtbares Profil ([`Context::invisible`]).
    pub fn new(fields: Vec<EmptyFieldCandidate>, qr: Option<QrPlaceholder>, invisible: bool) -> WorkspaceEdit {
        let (target, origin) =
            if qr.is_some() { (Target::Qr, PlacementOrigin::Qr) } else { (Target::Free, PlacementOrigin::Auto) };
        let qr_search = qr.clone().map(Some);
        WorkspaceEdit {
            target,
            origin,
            invisible,
            fields,
            qr,
            return_to_batch: false,
            parked: None,
            qr_search,
            automatic: true,
            invisible_by_profile: invisible,
            lock: PlacementLock::Off,
            others: Vec::new(),
            active: 0,
            rounds: None,
        }
    }

    /// Einstellung „Automatische Positionierung“ ([`Context::auto_position`]: `signature.auto_position` bzw.
    /// `-a`) anwenden – nach der Analyse und bei jeder Änderung der Einstellungen („sofort wirksam“):
    /// eingeschaltet sperrt sie die Platzierung eines Dokuments, dessen Lage noch aus der Automatik stammt
    /// ([`WorkspaceEdit::automatic`]) und das nicht aus der Stapelübersicht geöffnet wurde; ausgeschaltet hebt
    /// sie die Sperre auf. „Diesmal selbst platzieren“ ([`PlacementLock::Released`]) bleibt für dieses Dokument.
    pub fn follow_auto_position(&mut self, ctx: &Context) {
        let on = ctx.workspace && ctx.auto_position && !ctx.headless;
        self.lock = match self.lock {
            PlacementLock::Released => PlacementLock::Released,
            _ if !on => PlacementLock::Off,
            PlacementLock::Locked => PlacementLock::Locked,
            PlacementLock::Off if self.automatic && !self.return_to_batch => PlacementLock::Locked,
            PlacementLock::Off => PlacementLock::Off,
        };
    }

    /// Platzierung gesperrt (Einstellung „Automatische Positionierung“, noch nicht „Diesmal selbst
    /// platzieren“)?
    pub fn placement_locked(&self) -> bool {
        self.lock == PlacementLock::Locked
    }

    /// Unsichtbares Profil ein bzw. aus (Einstellungen „sofort wirksam“): erzwingt „unsichtbar“; zurück auf
    /// ein sichtbares Profil wird der Block wieder sichtbar, außer die Nutzerin hat „unsichtbar“ selbst
    /// gewählt. Liefert `true`, wenn der Block dadurch wieder sichtbar wurde.
    pub fn follow_profile(&mut self, ctx: &Context) -> bool {
        if ctx.invisible {
            if !self.invisible {
                self.invisible = true;
                self.invisible_by_profile = true;
            }
            false
        } else if self.invisible && self.invisible_by_profile {
            self.invisible = false;
            self.invisible_by_profile = false;
            true
        } else {
            self.invisible_by_profile = false;
            false
        }
    }

    /// „Unsichtbar signieren“ von der Nutzerin bzw. ihrem Plan gewählt (ein- oder ausgeschaltet).
    pub fn choose_invisible(&mut self, invisible: bool) {
        self.invisible = invisible;
        self.invisible_by_profile = false;
    }

    /// Wie [`WorkspaceEdit::new`], dazu das automatische Ziel nach den Einstellungen › Dokument: mit
    /// Platzhaltersuche „Leere Signaturfelder“ das erste leere Signaturfeld, mit „QR-Code-Platzhalter“ der
    /// gefundene QR-Platzhalter, sonst frei (PDF-AS-Regel); nie mit unsichtbarem Profil.
    pub fn after_analysis(
        fields: Vec<EmptyFieldCandidate>,
        qr: Option<QrPlaceholder>,
        invisible: bool,
        ctx: &Context,
    ) -> WorkspaceEdit {
        let mut edit = WorkspaceEdit::new(fields, None, invisible);
        edit.qr_search = qr.map(Some);
        edit.follow_settings(ctx);
        edit
    }

    /// Einstellungen › Dokument (Platzhaltersuche, ADR 0004 §3.11 „sofort wirksam“) auf den
    /// Bearbeitungszustand anwenden: angebotener QR-Platzhalter und – nur solange die Lage aus der Automatik
    /// stammt ([`WorkspaceEdit::automatic`]) – das automatische Ziel. Von Hand gesetzte Lagen und Ziele
    /// bleiben. Liefert, was danach zu tun ist.
    pub fn follow_settings(&mut self, ctx: &Context) -> Follow {
        let want_qr = ctx.placeholder_search && !ctx.placeholder_fields && !ctx.invisible;
        let want_field = ctx.placeholder_search && ctx.placeholder_fields && !ctx.invisible;
        let found = self.qr_search.clone().flatten();
        // ein von Hand gewähltes QR-Ziel bleibt angeboten, auch wenn die Suche aus ist
        let user_qr = (self.target == Target::Qr && !self.automatic).then(|| self.qr.clone()).flatten();
        self.qr = if want_qr { found.or(user_qr) } else { user_qr };
        let search_qr = want_qr && self.qr_search.is_none();
        if !self.automatic {
            return Follow { auto_place: false, search_qr };
        }
        // Platzhaltersuche „Leere Signaturfelder“: das erste für den Block geeignete Feld (Nachtrag 22)
        let auto_field = self.fields.iter().position(|f| f.auto_ok);
        let (target, origin) = if let (true, Some(i)) = (want_field, auto_field) {
            (Target::Field(i), PlacementOrigin::Field)
        } else if want_qr && self.qr.is_some() {
            (Target::Qr, PlacementOrigin::Qr)
        } else {
            (Target::Free, PlacementOrigin::Auto)
        };
        let auto_place = target == Target::Free && !self.auto_free() && !self.invisible;
        self.target = target;
        self.origin = origin;
        Follow { auto_place, search_qr }
    }

    /// Bearbeitungszustand zu einer festen Position (Rückfall ohne Analyseergebnis, z. B. nach „Signatur
    /// fortsetzen“). Nur eine automatische Position gilt als Lage aus der Automatik.
    pub fn from_position(position: &PositionChoice, invisible: bool) -> WorkspaceEdit {
        let mut edit = WorkspaceEdit::new(Vec::new(), None, invisible);
        edit.automatic = matches!(position, PositionChoice::Auto);
        match position {
            PositionChoice::Invisible => edit.invisible = true,
            PositionChoice::Manual(_) => edit.origin = PlacementOrigin::User,
            PositionChoice::Placeholder(qr) => {
                edit.qr = Some(qr.clone());
                edit.target = Target::Qr;
                edit.origin = PlacementOrigin::Qr;
            }
            PositionChoice::Field(f) => {
                edit.fields = vec![f.clone()];
                edit.target = Target::Field(0);
                edit.origin = PlacementOrigin::Field;
            }
            PositionChoice::Auto => {}
        }
        edit
    }

    /// Automatische Platzierung ausstehend bzw. maßgeblich (frei, `origin == Auto`)?
    pub fn auto_free(&self) -> bool {
        self.target == Target::Free && self.origin == PlacementOrigin::Auto
    }
}

/// Folge von [`WorkspaceEdit::follow_settings`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Follow {
    /// neu automatisch platzieren (das Ziel wurde frei: `Effect::AutoPlace`)
    pub auto_place: bool,
    /// QR-Platzhalter dieses Dokuments noch nicht gesucht (`Effect::SearchPlaceholder`)
    pub search_qr: bool,
}

/// Umgang mit bereits vorhandenen Zieldateien im Stapellauf (ADR 0004 §3.8, §3.0 Regel 9). Standard im
/// Startdialog: „Beide behalten“ (§8 F11).
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExistingPolicy {
    /// vorhandene Datei ersetzen
    Overwrite,
    /// neue Datei mit der ersten freien Nummer ab 2 (`Name_signed (2).pdf`)
    #[default]
    KeepBoth,
    /// Dokument überspringen – geprüft **vor** dem Signieren (keine Bestätigung für ein Ergebnis, das nicht
    /// gespeichert würde)
    Skip,
}

/// Ausgabeziel eines Stapellaufs (ADR 0004 §3.8, §3.0 Regel 9): nach den Einstellungen
/// (`output.folder`, F-82; Mobile: App-Dokumente) oder ein vor dem Start gewählter Ordner.
/// Serde: `{ "kind": "configured" }` bzw. `{ "kind": "folder", "path": "…" }`.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Clone, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "path", rename_all = "snake_case")]
pub enum BatchOutput {
    /// wie eingestellt
    #[default]
    Configured,
    /// gewählter Ordner (`StartBatch::output_dir`)
    Folder(PathBuf),
}

impl std::fmt::Debug for BatchOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // ohne Pfad (ADR 0001 Regel 9)
        match self {
            BatchOutput::Configured => f.write_str("Configured"),
            BatchOutput::Folder(_) => f.write_str("Folder(<Pfad>)"),
        }
    }
}

/// Phasen des Signierens (Plan §3.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SigningPhase {
    /// Sitzung öffnen (Keystore entsperren, NullOperation)
    OpeningSession,
    /// Zertifikat lesen (InfoboxRead/Keystore)
    ReadingCertificate,
    /// `prepare()`
    Preparing,
    /// Backend `sign` (A-Trust-Dialoge laufen hier)
    Signing,
    /// `finish_*` + Selbstprüfung I1–I6
    Finishing,
    /// Schreiben (reserviert; geschrieben wird im Zustand [`State::Output`], damit Überschreib-Rückfrage
    /// und Speichern-Dialog dort angezeigt werden)
    Writing,
}

/// Ziel von „Wiederholen“.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum RetryTarget {
    /// neu vorbereiten (F-71: Retry → PrepareSigning)
    Prepare,
    /// neu vorbereiten mit mindestens dieser `/Contents`-Reserve in Bytes – nach
    /// [`WorkflowError::CmsTooLarge`]; „Wiederholen“ ist die zweite Bestätigung (Plan §6.2, R9)
    PrepareWithReserve(usize),
    /// erneut speichern (F-85)
    Save,
    /// Dokument erneut laden (F-15)
    Load,
    /// Einstellungen zurücksetzen (nach `.old` verschieben) und neu starten (F-05, Plan §7.1)
    ResetSettings,
}

/// Fehler im Workflow (Fehlercode + Parameter für i18n).
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, thiserror::Error, serde::Serialize, serde::Deserialize)]
pub enum WorkflowError {
    /// Dokument nicht ladbar/abgelehnt
    #[error("Dokument abgelehnt: {class:?} {code}")]
    Rejected {
        /// Klasse
        #[cfg_attr(feature = "ts", ts(as = "crate::ts_mirror::AcceptanceClass"))]
        class: AcceptanceClass,
        /// Code
        code: String,
    },
    /// Backend-Fehler
    #[error("Signaturfehler: {code}")]
    Backend {
        /// `BackendError::code()`
        code: String,
        /// Detail
        detail: String,
    },
    /// Invariante verletzt – nichts gespeichert
    #[error("Selbstprüfung fehlgeschlagen: {0}")]
    Invariant(#[cfg_attr(feature = "ts", ts(as = "crate::ts_mirror::InvariantViolation"))] InvariantViolation),
    /// CMS größer als die Reserve – nichts gespeichert (`pida_pades::FinishError::CmsTooLarge`)
    #[error("Signatur zu groß: {needed} Bytes, Reserve {reserve}")]
    CmsTooLarge {
        /// benötigt
        needed: usize,
        /// vorhanden
        reserve: usize,
    },
    /// Einstellungen nicht lesbar (`LoadOutcome::Unreadable`, F-05)
    #[error("Einstellungen nicht lesbar: {0}")]
    Config(String),
    /// Schreibfehler
    #[error("Speichern fehlgeschlagen: {0}")]
    Save(String),
    /// Vorbereiten fehlgeschlagen (Signaturblock, Positionierung, Engine; PDF-Over
    /// `error.PrepareDocument`, Wiederholen/Abbrechen)
    #[error("Vorbereitung fehlgeschlagen: {code}")]
    Prepare {
        /// Code (`sigblock-…`, `placement.…`, `ng-certificate-not-valid`, `error-PrepareDocument`)
        code: String,
        /// Detail
        detail: String,
    },
    /// Unerwartet
    #[error("unerwarteter Fehler: {0}")]
    Unexpected(String),
}

/// Meldungs-ID: Invariante verletzt (Parameter `invariant`, `detail`).
pub const MSG_INVARIANT: &str = "ng-invariant-violated";
/// Meldungs-ID: CMS größer als die Reserve (Parameter `needed`, `reserve`).
pub const MSG_CMS_TOO_LARGE: &str = "ng-cms-too-large";
/// Meldungs-ID: Speichern fehlgeschlagen (PDF-Over `output.save_failed`, `arg1` Datei, `arg2` Grund).
pub const MSG_SAVE_FAILED: &str = "output-save_failed";
/// Meldungs-ID: Einstellungen beim Start nicht lesbar, Werksreset anbieten (PDF-Over
/// `error.Initialization`, Ja/Nein).
pub const MSG_INITIALIZATION: &str = "error-Initialization";
/// Meldungs-ID: unerwarteter Fehler (PDF-Over `error.Unexpected`).
pub const MSG_UNEXPECTED: &str = "error-Unexpected";
/// Meldungs-ID: Positionierung nicht möglich, automatische Position (PDF-Over
/// `error.PositioningNotPossible`).
pub const MSG_POSITIONING_NOT_POSSIBLE: &str = "error-PositioningNotPossible";
/// Meldungs-ID: Ohne Oberfläche ist das Signiermittel nicht eindeutig (`-b` angeben).
pub const MSG_BACKEND_CHOICE_REQUIRED: &str = "ng-backend-choice-required";
/// Meldungs-ID: Kein Signiermittel verfügbar.
pub const MSG_NO_BACKEND: &str = "ng-no-backend-available";
/// Meldungs-ID: Das Standard-Signiermittel ist nicht verfügbar.
pub const MSG_DEFAULT_BACKEND_UNAVAILABLE: &str = "ng-default-backend-unavailable";

impl WorkflowError {
    /// Aus einem Backend-Fehler (Code `backend-…`, Detail = Fehlertext ohne Secrets). Die Diagnose einer
    /// unbekannten Seite bzw. beendeten Sitzung steht ohne Präfix im Detail (technische Details, ADR 0004
    /// Nachtrag 19); [`WorkflowError::params`] liefert die Felder der Variante zusätzlich unter ihren Namen.
    pub fn from_backend(e: &BackendError) -> WorkflowError {
        let detail = match e {
            BackendError::UnsupportedKeystoreFormat { format } => format.clone(),
            BackendError::SecurityLayer { code, info } => format!("{code}: {info}"),
            BackendError::Http { status, reason } => format!("{status} {reason}"),
            BackendError::UnknownPage { diagnostic } | BackendError::SessionClosed { diagnostic } => diagnostic.clone(),
            other => other.to_string(),
        };
        WorkflowError::Backend { code: e.code().to_owned(), detail }
    }

    /// Stabile Meldungs-ID (i18n).
    pub fn code(&self) -> String {
        match self {
            WorkflowError::Rejected { code, .. } => code.clone(),
            WorkflowError::Backend { code, .. } => code.clone(),
            WorkflowError::Invariant(_) => MSG_INVARIANT.to_owned(),
            WorkflowError::CmsTooLarge { .. } => MSG_CMS_TOO_LARGE.to_owned(),
            WorkflowError::Config(_) => MSG_INITIALIZATION.to_owned(),
            WorkflowError::Save(_) => MSG_SAVE_FAILED.to_owned(),
            WorkflowError::Prepare { code, .. } => code.clone(),
            WorkflowError::Unexpected(_) => MSG_UNEXPECTED.to_owned(),
        }
    }

    /// Parameter zur Meldungs-ID (Fluent-Variablen).
    pub fn params(&self) -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        match self {
            WorkflowError::Rejected { class, .. } => {
                m.insert("class".into(), format!("{class:?}"));
            }
            WorkflowError::Backend { code, detail } => {
                m.insert("detail".into(), detail.clone());
                backend_message_args(code, detail, &mut m);
            }
            WorkflowError::Prepare { detail, .. } => {
                m.insert("detail".into(), detail.clone());
            }
            WorkflowError::Invariant(v) => {
                m.insert("invariant".into(), format!("{:?}", v.invariant));
                m.insert("detail".into(), v.detail.clone());
            }
            WorkflowError::CmsTooLarge { needed, reserve } => {
                m.insert("needed".into(), needed.to_string());
                m.insert("reserve".into(), reserve.to_string());
            }
            WorkflowError::Config(d) | WorkflowError::Unexpected(d) => {
                m.insert("detail".into(), d.clone());
            }
            WorkflowError::Save(d) => {
                // `output.save_failed`: arg1 = Datei (setzt core-api), arg2 = Grund
                m.insert("arg2".into(), d.clone());
                m.insert("detail".into(), d.clone());
            }
        }
        m
    }

    /// Exit-Code der CLI (Plan §7.4).
    pub fn exit_code(&self) -> ExitCode {
        match self {
            WorkflowError::Rejected { .. } => ExitCode::Rejected,
            WorkflowError::Backend { code, .. } if code == "backend-cancelled" => ExitCode::Cancelled,
            WorkflowError::Backend { .. } | WorkflowError::CmsTooLarge { .. } => ExitCode::Backend,
            WorkflowError::Invariant(_) => ExitCode::Invariant,
            WorkflowError::Save(_) => ExitCode::Write,
            WorkflowError::Config(_) | WorkflowError::Prepare { .. } | WorkflowError::Unexpected(_) => {
                ExitCode::General
            }
        }
    }
}

/// Variablen der Hauptmeldung eines Backend-Codes (ADR 0004 Nachtrag 19): Die Kataloge nennen sie wie die Felder
/// der Variante (`backend-unknown-page` `{ $diagnostic }`, `backend-http` `{ $status } { $reason }`,
/// `backend-security-layer` `{ $code }: { $info }`, `backend-keystore-format` `{ $format }`); ohne sie stünde in
/// CLI-Ausgabe, Log und Stapelzeile der rohe Platzhalter. `detail` bildet [`WorkflowError::from_backend`]
/// eindeutig. Die Oberfläche zeigt statt der Hauptmeldung den Alltagstext (`.text`) ohne Variablen.
fn backend_message_args(code: &str, detail: &str, m: &mut BTreeMap<String, String>) {
    let mut put = |k: &str, v: &str| {
        m.insert(k.to_owned(), v.to_owned());
    };
    match code {
        "backend-unknown-page" => put("diagnostic", detail),
        "backend-keystore-format" => put("format", detail),
        "backend-http" => {
            let (status, reason) = detail.split_once(' ').unwrap_or((detail, ""));
            put("status", status);
            put("reason", reason);
        }
        "backend-security-layer" => {
            let (code, info) = detail.split_once(": ").unwrap_or((detail, ""));
            put("code", code);
            put("info", info);
        }
        _ => {}
    }
}

/// Option der Backend-Auswahl.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BackendOption {
    /// Backend
    pub id: BackendId,
    /// Fähigkeiten (`None` = nicht verfügbar)
    pub capabilities: Option<Capabilities>,
}

impl BackendOption {
    /// Verfügbar?
    pub fn available(&self) -> bool {
        self.capabilities.is_some()
    }
}

/// Backend zu einer Einstellung (`BKU`).
pub fn backend_for_choice(choice: BackendChoice) -> BackendId {
    match choice {
        BackendChoice::IdAustria => BackendId::AtrustNative,
        BackendChoice::LocalBku => BackendId::LocalBku,
        BackendChoice::Keystore => BackendId::Keystore,
    }
}

/// Unveränderliche Eingaben des Reducers (aus Einstellungen, Overlay, Richtlinie, Plattform).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context {
    /// automatische Positionierung (`SIGNATURE_POSITION=auto` oder `-a`)
    pub auto_position: bool,
    /// unsichtbares Profil (auch beim **ersten** Dokument, B-03 behoben)
    pub invisible: bool,
    /// Platzhaltersuche aktiv
    pub placeholder_search: bool,
    /// `USE_FIELDS` (Vorrang) statt `USE_MARKER`
    pub placeholder_fields: bool,
    /// Standard-Backend
    pub default_backend: Option<BackendChoice>,
    /// `-s`: Ergebnis-Screen überspringen, Ende nach Speichern
    pub skip_finish: bool,
    /// ohne grafische Oberfläche (CLI): automatische Positionierung, Platzhalter nach festen Regeln,
    /// kein Speichern-Dialog, Fehler ohne Wiederholen, Ende nach dem letzten Dokument
    pub headless: bool,
    /// Postfix der Ausgabedatei (`SAVE_FILE_POSTFIX`, Standard `_signed`) für den Speichern-Dialog (F-86)
    pub output_postfix: String,
    /// Arbeitsbereich-Modus (ADR 0004 §3.2, aus `CoreOptions::workspace_ui`, nie zusammen mit `headless`):
    /// keine Platzhalterfrage, keine automatische Backend-Wahl, Abbruch zurück zum Arbeitsbereich;
    /// eine Sitzung öffnet nur nach den Paaren der Invariante A1
    pub workspace: bool,
    /// Startargument-Automatik der GUI (D-06): Overlay mit `-b` **und** (`-a` oder `-v`). Dann gilt im
    /// Arbeitsbereich-Modus der klassische automatische Weg – aber nur für Signiermittel ohne
    /// `document_leaves_device`
    pub cli_auto: bool,
    /// Stapelübersicht bei ≥ 2 Dokumenten (ADR 0004 §3.8, aus `CoreOptions::batch_overview`; nur mit
    /// `workspace`, nie mit `cli_auto` – D-06 bleibt ohne Übersicht)
    pub batch_overview: bool,
}

impl Context {
    /// Stapelübersicht aktiv (Arbeitsbereich, Oberfläche, keine Startargument-Automatik)?
    pub fn batch(&self) -> bool {
        self.workspace && self.batch_overview && !self.cli_auto && !self.headless
    }
}

impl Default for Context {
    fn default() -> Self {
        Self {
            auto_position: false,
            invisible: false,
            placeholder_search: false,
            placeholder_fields: false,
            default_backend: None,
            skip_finish: false,
            headless: false,
            output_postfix: "_signed".into(),
            workspace: false,
            cli_auto: false,
            batch_overview: false,
        }
    }
}

/// Was der Workflow nach der Analyse über das aktuelle Dokument weiß.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DocFacts {
    /// Dokument
    pub id: DocId,
    /// Seitenzahl
    pub page_count: usize,
    /// bereits signiert (keine neue Seite, docs/analyse/04 §4.5)
    pub signed: bool,
    /// Annahmeklasse
    pub class: AcceptanceClass,
    /// Startposition der manuellen Positionierung (letzte Seite, 50 %/25 %, F-32/F-33);
    /// `None` = keine Vorschau möglich
    pub start: Option<Placement>,
    /// Neues sichtbares Signaturfeld erlaubt (`DocReport::allows_new_visible_field`; DocMDP → `false`,
    /// ADR 0004 §3.3)
    #[serde(default = "yes")]
    pub new_visible_field_allowed: bool,
}

fn yes() -> bool {
    true
}

impl DocFacts {
    /// Neue Seite erlaubt? Nicht auf bereits signierten Dokumenten (das schließt DocMDP P=2/3 ein,
    /// denn eine Zertifizierung ist selbst eine Signatur; Plan §6.1).
    pub fn new_page_allowed(&self) -> bool {
        !self.signed
    }
}

/// Ablaufdaten eines Signiervorgangs (aus den Fähigkeiten des gewählten Backends).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct SignFlow {
    /// Zertifikat vorab lesen (Modell A immer; Modell B nur für einen sichtbaren Block)
    pub needs_certificate: bool,
    /// Mindestreserve für `/Contents` nach [`WorkflowError::CmsTooLarge`]
    pub min_reserve: Option<usize>,
}

impl SignFlow {
    /// Aus den Fähigkeiten und der Position.
    pub fn new(caps: &Capabilities, position: &PositionChoice) -> SignFlow {
        let needs_certificate = match caps.certificate {
            CertNeed::Required => true,
            CertNeed::OnlyForVisibleBlock => position.is_visible(),
            CertNeed::No => false,
        };
        SignFlow { needs_certificate, min_reserve: None }
    }
}

/// Persistierter Signiervorgang, der fortgesetzt werden soll (Mobile-Lebenszyklus, Plan §3.5: die App
/// wurde vom System beendet, während die Nutzerin in der App „ID Austria“ bestätigte). Position und
/// Signiermittel stehen fest; das vorbereitete Dokument (`pades::Prepared::persist`) stellt
/// `pida-core-api` beim Effekt [`Effect::Prepare`] wieder her.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ResumePlan {
    /// Position des vorbereiteten Dokuments
    pub position: PositionChoice,
    /// Signiermittel des unterbrochenen Vorgangs
    pub backend: BackendId,
}

/// Meldungs-ID (ADR 0004 §3.6): Signieren abgebrochen, zurück im Arbeitsbereich – „Es wurde nichts
/// signiert.“
pub const MSG_SIGNING_CANCELLED: &str = "ng-signing-cancelled";
/// Meldungs-ID (ADR 0004 §3.6): in der App abgelehnt oder abgelaufen (`Event::SigningDeclined`).
pub const MSG_SIGNING_DECLINED: &str = "ng-signing-declined";
/// Meldungs-ID (ADR 0004 §3.5): Das gewählte Signiermittel ist nicht verfügbar.
pub const MSG_SIGNER_UNAVAILABLE: &str = "ng-signer-unavailable";
/// Meldungs-ID (ADR 0004 §3.3): bereits signiert – keine neue Seite.
pub const MSG_CONSTRAINT_SIGNED: &str = "ng-constraint-signed";
/// Meldungs-ID (ADR 0004 §3.3): DocMDP-zertifiziert – nur vorhandene Felder oder unsichtbar.
pub const MSG_CONSTRAINT_CERTIFIED: &str = "ng-constraint-certified";
/// Meldungs-ID (ADR 0004 §3.3): Profil unsichtbar – kein sichtbarer Block.
pub const MSG_CONSTRAINT_INVISIBLE_PROFILE: &str = "ng-constraint-invisible-profile";
/// Meldungs-ID: gewähltes Signaturfeld gibt es nicht bzw. ist gesperrt (ADR 0004 §3.3 `TargetChosen`).
pub const MSG_FIELD_LOCKED: &str = "ng-field-locked";
/// Meldungs-ID: Höchstzahl der Signaturblöcke erreicht ([`MAX_BLOCKS`]).
pub const MSG_BLOCKS_LIMIT: &str = "ng-blocks-limit";
/// Meldungs-ID: Ziel (Signaturfeld bzw. QR-Platzhalter) ist schon von einem anderen Block belegt.
pub const MSG_BLOCK_TARGET_TAKEN: &str = "ng-block-target-taken";

/// Meldungs-ID: Das Signiermittel des fortgesetzten Vorgangs ist nicht mehr verfügbar; die Auswahl
/// wird gezeigt und das Dokument neu vorbereitet.
pub const MSG_RESUME_BACKEND_UNAVAILABLE: &str = "ng-resume-backend-unavailable";

/// Meldungs-ID (ADR 0004 §3.8): Dokumente zur Warteschlange hinzugefügt (`$count`).
pub const MSG_BATCH_ADDED: &str = "ng-batch-added";
/// Meldungs-ID: ein aus der Stapelübersicht geöffnetes Dokument wurde signiert (`$file`).
pub const MSG_BATCH_ITEM_SIGNED: &str = "ng-batch-item-signed";
/// Meldungs-ID: Stapellauf beendet (`$signed`, `$total`).
pub const MSG_BATCH_DONE: &str = "ng-batch-done";
/// Meldungs-ID: kein Dokument zum Signieren bereit.
pub const MSG_BATCH_NOTHING_TO_SIGN: &str = "ng-batch-nothing-to-sign";
/// Meldungs-ID: vor dem Start einen Ordner wählen („jedes Mal fragen“ bzw. kein vorhandener Ordner).
pub const MSG_BATCH_OUTPUT_FOLDER_REQUIRED: &str = "ng-batch-output-folder-required";
/// Meldungs-ID: Stapel abgebrochen (`$signed`).
pub const MSG_BATCH_ABORTED: &str = "ng-batch-aborted";
/// Meldungs-ID: nach „Dokument überspringen“ kann eine offene Anfrage in der App bis zu ihrem Ablauf
/// sichtbar bleiben (A-Trust erfährt vom Abbruch nichts, ADR 0004 §1.2).
pub const MSG_BATCH_PENDING_APP_REQUEST: &str = "ng-batch-pending-app-request";

/// Meldungs-ID (ADR 0004 Nachtrag 10): Dateien aus „Öffnen mit“ bzw. einer zweiten Instanz kamen ohne Rückfrage
/// zur geparkten Arbeit (`$count`).
pub const MSG_PARKED_FILES_ADDED: &str = "ng-parked-files-added";

/// Meldungs-ID: das Zertifikat aus dem Zertifikats-Cache passt nicht mehr (erneuert, anderes Konto); der
/// Eintrag ist gelöscht, „Wiederholen“ liest das Zertifikat neu (`pida-core-api`).
pub const MSG_CERT_CACHE_MISMATCH: &str = "ng-certcache-mismatch";

/// Meldungs-ID: einzeiliger Hinweis ohne Rückfrage, dass das Dokument zum Signieren an A-Trust geht
/// (`document_leaves_device`). Nur ohne grafische Oberfläche ([`Context::headless`], CLI); die Oberfläche
/// zeigt den Kurzhinweis selbst beim Signiermittel (ADR 0003).
pub const MSG_DOCUMENT_LEAVES_DEVICE: &str = "ng-document-leaves-device";

/// Zustand.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum State {
    /// Initialisierung (Konfiguration, Migration-Angebot)
    Starting,
    /// Dokumentauswahl
    Idle {
        /// Warteschlange
        queue: Queue,
    },
    /// Laden + Analyse + Platzhaltersuche
    Analyzing {
        /// Warteschlange (aktuelles Dokument wird analysiert)
        queue: Queue,
    },
    /// „Signatur fortsetzen“ (Mobile, nach einem App-Kill): das Dokument wird geladen und ohne
    /// Platzhalterfrage, Positionierung oder Auswahl mit dem gespeicherten Signiermittel signiert
    /// ([`Event::ResumeRequested`]).
    Resuming {
        /// Warteschlange (aktuelles Dokument = fortgesetztes)
        queue: Queue,
        /// Position und Signiermittel des unterbrochenen Vorgangs
        plan: ResumePlan,
        /// Dokumentdaten nach der Analyse (`None` = wird noch geladen)
        doc: Option<DocFacts>,
    },
    /// Warnung der Eingangsanalyse mit Abbruchmöglichkeit (Plan §6.1: `/Perms /UR3`, Hybrid-XFA,
    /// inkompatible Seed Values – `pades::Finding::warning`): „Fortfahren“ oder Dokument verwerfen
    AnalysisWarning {
        /// Warteschlange
        queue: Queue,
        /// Codes der Warnungen (i18n-IDs, z. B. `pdf-usage-rights`)
        warnings: Vec<String>,
        /// Dokument
        doc: DocFacts,
        /// Fund der Platzhaltersuche (gilt nach „Fortfahren“)
        placeholder: Option<PlaceholderFound>,
        /// leere Signaturfelder (Arbeitsbereich, gilt nach „Fortfahren“)
        #[serde(default)]
        fields: Vec<EmptyFieldCandidate>,
    },
    /// Frage „Platzhalter verwenden?“ bzw. Feldauswahl
    PlaceholderQuestion {
        /// Warteschlange
        queue: Queue,
        /// Fund
        found: PlaceholderFound,
        /// Dokument
        doc: DocFacts,
    },
    /// Positionierung
    Positioning {
        /// Warteschlange
        queue: Queue,
        /// aktuelle Seite (0-basiert; `== page_count` = virtuelle neue Seite)
        page: usize,
        /// gewählte/zuletzt bekannte Position (F-43)
        placement: Option<Placement>,
        /// virtuelle neue Seite aktiv (F-38)
        new_page: bool,
        /// Dokument
        doc: DocFacts,
        /// Bearbeitungszustand (nur im Arbeitsbereich-Modus, ADR 0004 §3.3)
        #[serde(default)]
        edit: Option<WorkspaceEdit>,
    },
    /// Backend-Auswahl
    ChoosingBackend {
        /// Warteschlange
        queue: Queue,
        /// Position
        position: PositionChoice,
        /// Optionen (leer bis [`Event::BackendsProbed`])
        options: Vec<BackendOption>,
        /// Dokument
        doc: DocFacts,
        /// Standard-Backend bzw. automatische Wahl anwenden (nur auf dem Weg Öffnen/Positionieren →
        /// Signieren, F-53/F-54; nach Abbruch, Fehler oder Hauptleiste wird gefragt, F-55)
        auto_choice: bool,
        /// Die Nutzerin hat ausdrücklich „Signieren“ gewählt (Position bestätigt bzw. „Unsichtbar
        /// signieren“). Nur dann startet die automatische Wahl ein Signiermittel, bei dem das Dokument
        /// das Gerät verlässt (`document_leaves_device`, z. B. ID Austria); sonst erscheint die Auswahl
        /// mit dem Kurzhinweis – nie ohne Nutzeraktion zu A-Trust (Autostart-Befund F-54, ADR 0003).
        #[serde(default)]
        sign_requested: bool,
        /// Bearbeitungszustand (Arbeitsbereich: Rückfall bzw. Startargument-Automatik)
        #[serde(default)]
        edit: Option<WorkspaceEdit>,
    },
    /// Signieren
    Signing {
        /// Warteschlange
        queue: Queue,
        /// Position
        position: PositionChoice,
        /// Backend
        backend: BackendId,
        /// Phase
        phase: SigningPhase,
        /// Dokument
        doc: DocFacts,
        /// Ablaufdaten
        flow: SignFlow,
        /// Bearbeitungszustand (Arbeitsbereich: Abbruch stellt ihn wieder her)
        #[serde(default)]
        edit: Option<WorkspaceEdit>,
    },
    /// Ergebnis
    Output {
        /// Warteschlange
        queue: Queue,
        /// gespeichert unter (sonst Speichern-Knopf, F-86)
        saved: Option<OutputInfo>,
        /// offene Ausgabeentscheidung
        decision: Option<OutputDecision>,
        /// Dokument (Original, F-91: erneut signieren)
        doc: DocFacts,
        /// verwendete Position (F-43/F-91)
        position: PositionChoice,
        /// verwendetes Backend
        backend: BackendId,
        /// Bearbeitungszustand (Arbeitsbereich: „Fertig“ zeigt die signierte Seite)
        #[serde(default)]
        edit: Option<WorkspaceEdit>,
    },
    /// Fehler mit Wiederholmöglichkeit
    Error {
        /// Warteschlange
        queue: Queue,
        /// Fehler
        error: WorkflowError,
        /// Wiederholen möglich
        retry: Option<RetryTarget>,
        /// Zustand, in dem der Fehler auftrat (Ziel von „Wiederholen“ bzw. „Abbrechen“)
        from: Option<Box<State>>,
    },
    /// Programmende
    Finished {
        /// Exit-Code
        exit: ExitCode,
        /// Ergebnisse aller abgeschlossenen Dokumente (CLI `--json`)
        #[serde(default)]
        results: Vec<DocOutcome>,
    },
    // --- ADR 0004 §3.8: Stapel (neue Varianten am Ende)
    /// Stapelübersicht (Arbeitsbereich, ≥ 2 Dokumente); Einträge in `queue.batch`, kein aktuelles Dokument
    BatchOverview {
        /// Warteschlange mit Stapel
        queue: Queue,
    },
    /// Stapellauf mit **einer** Sitzung; aktuelles Dokument = `queue.current`
    BatchSigning {
        /// Warteschlange mit Stapel
        queue: Queue,
        /// Lauf
        run: Box<BatchRun>,
    },
}

impl State {
    /// Ergebnisse der abgeschlossenen Dokumente (auch nach dem Programmende).
    pub fn results(&self) -> &[DocOutcome] {
        match self {
            State::Finished { results, .. } => results,
            other => other.queue().map(|q| q.done.as_slice()).unwrap_or(&[]),
        }
    }

    /// Exit-Code, falls beendet.
    pub fn exit_code(&self) -> Option<ExitCode> {
        match self {
            State::Finished { exit, .. } => Some(*exit),
            _ => None,
        }
    }

    /// Einstellungen sind ein Overlay ohne eigenen Zustand (F-101 ohne Re-Entry): ob sie offen sind,
    /// hält `pida-core-api` (`ViewState::settings`); der Reducer sieht nur
    /// [`Event::SettingsChanged`]. Liefert deshalb immer `false`.
    pub fn settings_open(&self) -> bool {
        false
    }

    /// Warteschlange (außer in `Starting`/`Finished`).
    pub fn queue(&self) -> Option<&Queue> {
        match self {
            State::Starting | State::Finished { .. } => None,
            State::Idle { queue }
            | State::Analyzing { queue }
            | State::Resuming { queue, .. }
            | State::AnalysisWarning { queue, .. }
            | State::PlaceholderQuestion { queue, .. }
            | State::Positioning { queue, .. }
            | State::ChoosingBackend { queue, .. }
            | State::Signing { queue, .. }
            | State::Output { queue, .. }
            | State::Error { queue, .. }
            | State::BatchOverview { queue }
            | State::BatchSigning { queue, .. } => Some(queue),
        }
    }

    /// Veränderbare Warteschlange.
    pub fn queue_mut(&mut self) -> Option<&mut Queue> {
        match self {
            State::Starting | State::Finished { .. } => None,
            State::Idle { queue }
            | State::Analyzing { queue }
            | State::Resuming { queue, .. }
            | State::AnalysisWarning { queue, .. }
            | State::PlaceholderQuestion { queue, .. }
            | State::Positioning { queue, .. }
            | State::ChoosingBackend { queue, .. }
            | State::Signing { queue, .. }
            | State::Output { queue, .. }
            | State::Error { queue, .. }
            | State::BatchOverview { queue }
            | State::BatchSigning { queue, .. } => Some(queue),
        }
    }

    /// Dokumentdaten des aktuellen Dokuments (nach der Analyse).
    pub fn doc(&self) -> Option<&DocFacts> {
        match self {
            State::AnalysisWarning { doc, .. }
            | State::PlaceholderQuestion { doc, .. }
            | State::Positioning { doc, .. }
            | State::ChoosingBackend { doc, .. }
            | State::Signing { doc, .. }
            | State::Output { doc, .. } => Some(doc),
            State::Resuming { doc, .. } => doc.as_ref(),
            State::Error { from: Some(f), .. } => f.doc(),
            State::BatchSigning { run, .. } => run.doc.as_ref(),
            _ => None,
        }
    }

    /// Schritt, der in der Hauptleiste „aktuell“ ist.
    pub fn step(&self) -> Option<Step> {
        match self {
            State::Starting | State::Finished { .. } => None,
            State::Idle { .. }
            | State::Analyzing { .. }
            | State::AnalysisWarning { .. }
            | State::PlaceholderQuestion { .. } => Some(Step::Open),
            // Fortsetzen: Dokument wird geladen, danach direkt Signieren
            State::Resuming { .. } => Some(Step::Signing),
            State::Positioning { .. } => Some(Step::Positioning),
            State::ChoosingBackend { .. } | State::Signing { .. } => Some(Step::Signing),
            State::Output { .. } => Some(Step::Finish),
            State::Error { from, .. } => from.as_deref().and_then(State::step).or(Some(Step::Open)),
            State::BatchOverview { .. } => Some(Step::Open),
            State::BatchSigning { .. } => Some(Step::Signing),
        }
    }

    /// Hauptleisten-Zustände je State (docs/analyse/01 §3.5). Während des Signierens ist nichts
    /// klickbar (PDF-Over: Verhalten von `SigningState` auch während der ID-Austria-Dialoge);
    /// Fehlermeldungen übernehmen die aktiven Schritte des Ursprungszustands, sind aber modal.
    pub fn main_bar(&self) -> [MainBarStep; 4] {
        // (aktiv, klickbar) für Open, Positioning, Signing, Finish
        let flags: [(bool, bool); 4] = match self {
            State::Starting | State::Finished { .. } => [(false, false); 4],
            State::Idle { .. }
            | State::Analyzing { .. }
            | State::AnalysisWarning { .. }
            | State::PlaceholderQuestion { .. }
            | State::BatchOverview { .. } => [(true, true), (false, false), (false, false), (false, false)],
            State::Positioning { .. } => [(true, true), (true, false), (false, false), (false, false)],
            // „Öffnen“ verwirft den fortgesetzten Vorgang; Positionieren entfällt (Position steht fest)
            State::Resuming { .. } => [(true, true), (true, false), (true, false), (false, false)],
            State::ChoosingBackend { .. } => [(true, true), (true, true), (true, false), (false, false)],
            State::Signing { .. } | State::BatchSigning { .. } => {
                [(true, false), (true, false), (true, false), (false, false)]
            }
            State::Output { .. } => [(true, true), (true, true), (true, true), (true, false)],
            State::Error { from, .. } => {
                let base = from.as_deref().map(State::main_bar);
                let mut out = [(true, false), (false, false), (false, false), (false, false)];
                if let Some(base) = base {
                    for (o, b) in out.iter_mut().zip(base) {
                        *o = (b.active, false);
                    }
                }
                out
            }
        };
        let steps = [Step::Open, Step::Positioning, Step::Signing, Step::Finish];
        let mut out = [MainBarStep { step: Step::Open, active: false, enabled: false }; 4];
        for (i, (active, enabled)) in flags.into_iter().enumerate() {
            out[i] = MainBarStep { step: steps[i], active, enabled };
        }
        out
    }

    /// Einstellungen erreichbar (Konfig-Button, Spalte „Config“ in docs/analyse/01 §3.5).
    pub fn settings_enabled(&self) -> bool {
        matches!(
            self,
            State::Idle { .. }
                | State::Analyzing { .. }
                | State::AnalysisWarning { .. }
                | State::PlaceholderQuestion { .. }
                | State::Positioning { .. }
                | State::ChoosingBackend { .. }
                | State::Output { .. }
                | State::BatchOverview { .. }
        )
    }

    /// Klickbarer Hauptleisten-Schritt?
    pub fn step_enabled(&self, step: Step) -> bool {
        self.main_bar().iter().any(|s| s.step == step && s.enabled)
    }

    /// Kurzname (Logs, Diagnose; ohne Inhalte).
    pub fn name(&self) -> &'static str {
        match self {
            State::Starting => "Starting",
            State::Idle { .. } => "Idle",
            State::Analyzing { .. } => "Analyzing",
            State::Resuming { .. } => "Resuming",
            State::AnalysisWarning { .. } => "AnalysisWarning",
            State::PlaceholderQuestion { .. } => "PlaceholderQuestion",
            State::Positioning { .. } => "Positioning",
            State::ChoosingBackend { .. } => "ChoosingBackend",
            State::Signing { .. } => "Signing",
            State::Output { .. } => "Output",
            State::Error { .. } => "Error",
            State::Finished { .. } => "Finished",
            State::BatchOverview { .. } => "BatchOverview",
            State::BatchSigning { .. } => "BatchSigning",
        }
    }

    /// Arbeit geparkt („Zur Startseite“, ADR 0004 Nachtrag 10, [`Queue::parked`])? Dann zeigt die Oberfläche die
    /// Startansicht mit der Karte „In Bearbeitung“; der Zustand selbst bleibt erhalten.
    pub fn parked(&self) -> bool {
        self.queue().is_some_and(|q| q.parked)
    }

    /// Ist diese (ruhende) Arbeit ein Stapel, d. h. führt „Weiter bearbeiten“ geparkter Arbeit zur Stapelübersicht
    /// (ADR 0004 Nachtrag 13)? Nur mit Stapelübersicht ([`Context::batch`]): die Übersicht selbst sowie ein aus ihr
    /// geöffnetes Dokument bzw. ein Dokument mit wartenden Dokumenten im Arbeitsbereich oder mit Analysewarnung.
    /// Nicht „Fertig“ ohne Speichern: dort ginge die Signatur verloren, erst das Speichern führt weiter.
    pub fn resumes_to_overview(&self, ctx: &Context) -> bool {
        if !ctx.batch() {
            return false;
        }
        match self {
            State::BatchOverview { .. } => true,
            State::Positioning { queue, edit: Some(_), .. } | State::AnalysisWarning { queue, .. } => {
                !queue.pending.is_empty() || self.opened_from_batch()
            }
            _ => false,
        }
    }

    /// Stapel, aus dem dieses Einzeldokument geöffnet wurde (`Queue::batch` mit `open` = aktuelles
    /// Dokument) bzw. – nach „Signatur fortsetzen“ eines unterbrochenen Stapels – ein Bearbeitungszustand
    /// mit `return_to_batch`.
    pub fn opened_from_batch(&self) -> bool {
        let state = match self {
            State::Error { from: Some(f), .. } => f.as_ref(),
            s => s,
        };
        let edit = match state {
            State::Positioning { edit, .. }
            | State::ChoosingBackend { edit, .. }
            | State::Signing { edit, .. }
            | State::Output { edit, .. } => edit.as_ref(),
            _ => None,
        };
        if edit.is_some_and(|e| e.return_to_batch) {
            return true;
        }
        let Some(q) = self.queue() else { return false };
        q.current_id().is_some() && q.batch.as_ref().is_some_and(|b| b.open == q.current_id())
    }
}

/// Ereignis (UI, Effekt-Ergebnis, Lebenszyklus).
///
/// `Analyzed` trägt den gefundenen Platzhalter direkt (QR-Platzhalter mit Matrix und Position);
/// Ereignisse leben nur bis zum nächsten Reducer-Schritt, deshalb wird nicht geboxt.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Start abgeschlossen (CLI-Dateien bereits im Overlay)
    Started {
        /// Eingabedokumente
        documents: Vec<InputDocument>,
    },
    /// Dateien hinzugefügt (Dialog, Drop, „Öffnen mit“, Share)
    FilesAdded(Vec<InputDocument>),
    /// Analyse fertig
    Analyzed {
        /// Dokument
        doc: DocId,
        /// Klasse
        class: AcceptanceClass,
        /// Platzhalter
        placeholder: Option<PlaceholderFound>,
        /// bereits signiert
        signed: bool,
        /// Seitenzahl
        page_count: usize,
        /// Startposition für die manuelle Positionierung (`None` = keine Vorschau)
        start: Option<Placement>,
        /// Warnungen mit Abbruchmöglichkeit (`pades::Finding::warning`, Codes)
        warnings: Vec<String>,
        /// alle leeren, nicht gesperrten Signaturfelder unabhängig von `placeholder.kind` (Arbeitsbereich;
        /// der klassische Ablauf ignoriert sie)
        fields: Vec<EmptyFieldCandidate>,
        /// neues sichtbares Feld erlaubt (`DocReport::allows_new_visible_field`)
        new_visible_field_allowed: bool,
    },
    /// Analyse/Laden fehlgeschlagen
    AnalysisFailed {
        /// Dokument
        doc: DocId,
        /// Fehler
        error: WorkflowError,
    },
    /// Vorschau/Positionierung nicht möglich (Renderer fehlt oder scheitert) → Hinweis
    /// `error.PositioningNotPossible` und automatische Positionierung (docs/analyse/01 §3.4,
    /// `PositioningState.java:171-179`)
    PreviewUnavailable {
        /// Dokument
        doc: DocId,
    },
    /// Antwort auf die Warnung der Analyse (`true` = trotzdem signieren, `false` = Dokument verwerfen)
    WarningAnswered(bool),
    /// Antwort auf die Platzhalterfrage; bei Feldern mit gewähltem Index
    PlaceholderAnswered {
        /// Antwort
        answer: PlaceholderAnswer,
        /// gewähltes Feld
        field_index: Option<usize>,
    },
    /// Block verschoben
    PlacementChanged(Placement),
    /// Seite gewechselt
    PageChanged(usize),
    /// „Neue Seite“ umgeschaltet
    NewPageToggled,
    /// Position bestätigt („Signieren“/Enter)
    PositionConfirmed,
    /// „Unsichtbar signieren“ im Positionierungsschritt (PIDA-Ergänzung): dieses Dokument ohne
    /// sichtbaren Block signieren ([`PositionChoice::Invisible`]); das Profil bleibt unverändert
    InvisibleChosen,
    /// Backends geprüft
    BackendsProbed(Vec<BackendOption>),
    /// Backend gewählt
    BackendChosen(BackendId),
    /// Phase abgeschlossen
    PhaseCompleted(SigningPhase),
    /// Backend-Fehler
    BackendFailed(BackendError),
    /// Invariante verletzt (auch I3-Fehler aus `finish_cms`/`finish_raw`)
    SelfCheckFailed(InvariantViolation),
    /// Sonstiger Fehler beim Vorbereiten/Abschließen (Engine, Signaturblock, Platzierung)
    SigningFailed(WorkflowError),
    /// CMS passt nicht in die Reserve → Fehler mit [`RetryTarget::PrepareWithReserve`]
    CmsTooLarge {
        /// benötigt
        needed: usize,
        /// vorhanden
        reserve: usize,
    },
    /// Ausgabeziel entschieden (Ergebnis von [`Effect::DecideOutput`] bzw. gewählter Pfad aus dem
    /// Speichern-Dialog mit Existenzprüfung)
    OutputDecided(OutputDecision),
    /// Ausgabeziel entschieden bzw. Dialog beantwortet (`None` = abgebrochen). Ein Pfad ohne
    /// Existenzprüfung wird wie ein nicht vorhandenes Ziel behandelt – die Shell sollte
    /// [`Event::OutputDecided`] mit `exists` senden.
    OutputChosen(Option<PathBuf>),
    /// Überschreib-Rückfrage
    OverwriteAnswered(bool),
    /// Knopf „Speichern“ im Ergebnis-Screen (F-86: öffnet immer den Speichern-Dialog)
    SaveRequested,
    /// Link „signiertes Dokument öffnen“ (F-87)
    OpenResultRequested,
    /// „Teilen“ im Ergebnis-Screen (Mobile: Share-Sheet bzw. `ACTION_SEND`)
    ShareResultRequested,
    /// „Exportieren“ im Ergebnis-Screen (Mobile: „Sichern unter“ über Dokument-Picker bzw. SAF
    /// `CREATE_DOCUMENT`; die Ausgabe bleibt zusätzlich in den App-Dokumenten)
    ExportResultRequested,
    /// gespeichert
    Saved(OutputInfo),
    /// Speichern fehlgeschlagen
    SaveFailed(String),
    /// Wiederholen
    Retry,
    /// Abbrechen
    Cancel,
    /// Hauptleiste
    Navigate(Step),
    /// Einstellungen geändert (Blockvorschau neu, Sprache neu)
    SettingsChanged,
    /// Einstellungen beim Start nicht lesbar (F-05) → Fehler mit [`RetryTarget::ResetSettings`]
    SettingsUnreadable(String),
    /// „Signatur fortsetzen“ (Mobile, Plan §3.5): einen persistierten Vorgang wieder aufnehmen – nur im
    /// Auswahl-Screen ([`State::Idle`]); `pending` sind die damals wartenden Dokumente
    ResumeRequested {
        /// fortgesetztes Dokument (Sandbox-Kopie des Originals)
        document: InputDocument,
        /// Position und Signiermittel
        plan: ResumePlan,
        /// wartende Dokumente des unterbrochenen Batches (soweit noch vorhanden)
        pending: Vec<InputDocument>,
    },
    /// App in den Hintergrund (kein Abbruch!)
    Backgrounded,
    /// App wieder im Vordergrund (ggf. „Signatur fortsetzen“)
    Foregrounded,
    /// Beenden
    Quit,
    // --- ADR 0004: Arbeitsbereich-Modus (neue Varianten am Ende)
    /// Schalter „Unsichtbar signieren“ (nur dieses Dokument)
    InvisibleSet(bool),
    /// „Ins Feld setzen“ / QR-Platzhalter / „Frei platzieren“ (Index von core-api geprüft)
    TargetChosen(Target),
    /// Ergebnis von [`Effect::AutoPlace`] (`page_index == Seitenzahl` = neue Seite) → `origin = Auto`
    AutoPlaced(Placement),
    /// Blockgröße geändert (Einstellungen, Seitenwechsel): die von core-api geklemmte Lage; Herkunft und
    /// Ziel bleiben, bei `origin == Auto` wird neu automatisch platziert
    BlockResized(Placement),
    /// „Signieren mit X“ (`UiEvent::SignWith`): ausdrückliche Nutzeraktion mit den aktuellen Optionen
    SignRequested {
        /// Signiermittel
        backend: BackendId,
        /// aktuelle Optionen (Fähigkeiten, Verfügbarkeit)
        options: Vec<BackendOption>,
    },
    /// „Weiteren Block hinzufügen“ (Mehrfachsignatur): neuer, frei platzierter Block an der von core-api
    /// gewählten Stelle; er wird der ausgewählte Block
    BlockAdded(Placement),
    /// Block auswählen (Stelle in der Reihenfolge aller Blöcke)
    BlockSelected(usize),
    /// Block entfernen (Stelle in der Reihenfolge aller Blöcke); der letzte Block bleibt
    BlockRemoved(usize),
    /// „Alle Platzhalter belegen“: je noch freiem Ziel (von core-api nach Eignung gewählt) ein Block; ein frei
    /// platzierter ausgewählter Block wandert in das erste Ziel
    TargetsFilled(Vec<Target>),
    /// `BackendError::Cancelled`, das core-api **nicht** selbst ausgelöst hat (SL 6000/6001, „Abbrechen“
    /// in einer A-Trust-Meldung; ADR 0004 §3.6)
    SigningDeclined,
    // --- ADR 0004 §3.8: Stapel
    /// Ergebnis der Voranalyse eines Stapeleintrags (`Effect::Preanalyze`)
    Preanalyzed {
        /// Dokument
        doc: DocId,
        /// Voranalyse bzw. Grund der Ablehnung
        result: Result<DocSummary, WorkflowError>,
    },
    /// Eintrag der Übersicht im Arbeitsbereich öffnen
    BatchItemOpened(DocId),
    /// zurück zur Übersicht; die Platzierung wird als Plan gemerkt
    BackToBatch,
    /// zur Stapelübersicht (Einzeldokument mit weiteren Dokumenten in der Warteschlange)
    ShowBatch,
    /// Eintrag aus dem Stapel entfernen
    BatchItemRemoved(DocId),
    /// „Alle signieren (n) mit X“
    BatchStarted {
        /// Signiermittel
        backend: BackendId,
        /// genau diese Dokumente
        docs: Vec<DocId>,
        /// Ausgabeziel; `None` = nicht bestimmbar (Ordnerwahl fehlt)
        output: Option<BatchOutput>,
        /// Umgang mit vorhandenen Zieldateien
        on_existing: ExistingPolicy,
        /// aktuelle Optionen (Fähigkeiten, Verfügbarkeit)
        options: Vec<BackendOption>,
    },
    /// aktuelles Dokument überspringen (Sitzung und Zugangsdaten bleiben)
    BatchSkip,
    /// Stapel abbrechen (Sitzung abbrechen, F-72)
    BatchAbort,
    /// Ausgabeziel eines Stapeldokuments (`Effect::DecideBatchOutput`): `path` nach F-81/F-82 und der
    /// Regel für vorhandene Dateien („Beide behalten“: erste freie Nummer), `exists` = das Ziel nach F-81
    /// existierte bereits (`false`, wenn „Überschreiben“ ein Ergebnis desselben Stapels träfe – dann ist
    /// `path` wie bei „Beide behalten“ frei)
    BatchOutputDecided {
        /// Dokument
        doc: DocId,
        /// Ziel
        path: PathBuf,
        /// Ziel existierte
        exists: bool,
    },
    // --- ADR 0004 §3.11 (K3): Einstellungen › Dokument
    /// Ergebnis von [`Effect::SearchPlaceholder`]: QR-Platzhalter des Dokuments (`None` = keiner gefunden
    /// bzw. ohne Renderer nicht suchbar)
    PlaceholderSearched {
        /// Dokument
        doc: DocId,
        /// gefundener QR-Platzhalter
        qr: Option<QrPlaceholder>,
    },
    // --- ADR 0004 Nachtrag 8: Einstellung „Automatische Positionierung“
    /// „Diesmal selbst platzieren“ (`UiEvent::UnlockPlacement`): hebt die Sperre der Platzierung für das
    /// aktuelle Dokument auf ([`PlacementLock::Released`]); startet nichts (A1)
    PlacementUnlocked,
    /// „Für diesen Stapel automatisch positionieren“ (`UiEvent::SetBatchAutoPosition`, nur in der
    /// Stapelübersicht): nur für diesen Stapel ([`crate::Batch::auto_position_once`]); `replace_manual` = auch
    /// von Hand platzierte Einträge neu automatisch platzieren (ausdrücklich bestätigt, sonst behalten sie ihren
    /// Plan); startet nichts (A1)
    BatchAutoPositionSet {
        /// ein bzw. aus
        enabled: bool,
        /// von Hand platzierte Einträge ebenfalls automatisch platzieren
        replace_manual: bool,
    },
    // --- ADR 0004 Nachtrag 9: „PDF hinzufügen“ (Kopfzeile „+ PDF“, Miniaturleiste)
    /// „Weitere PDFs hinzufügen“ (`UiEvent::OpenFiles { mode: add }`): die Dateien kommen zum offenen Dokument;
    /// mit der Stapelübersicht werden im Arbeitsbereich alle zusammen zum Stapel – die Platzierung des offenen
    /// Dokuments bleibt dessen Plan (wie [`Event::ShowBatch`]). Sonst wie [`Event::FilesAdded`]. Startet nichts
    /// (A1).
    MoreFilesAdded(Vec<InputDocument>),
    /// „Anderes PDF öffnen“ (`UiEvent::OpenFiles { mode: replace }`): das offene Dokument (Analysewarnung,
    /// Arbeitsbereich) wird geschlossen – verworfen, nie signiert –, danach öffnen die Dateien wie in der
    /// Dokumentauswahl; wartende Dokumente bleiben. Sonst wie [`Event::FilesAdded`]. Startet nichts (A1).
    DocumentReplaced(Vec<InputDocument>),
    // --- ADR 0004 Nachtrag 10: „Zur Startseite“ parkt offene Arbeit
    /// „Zur Startseite“ (`UiEvent::GoHome`: Haus-Symbol, Wortmarke, Palette): Arbeitsbereich, Analysewarnung,
    /// Stapelübersicht und „Fertig“ ohne Speichern werden geparkt ([`Queue::parked`]) statt verworfen; mit
    /// gespeichertem Ergebnis und wartenden Dokumenten wird der Stapel wie bei „Zum Stapel“ geparkt. Beim
    /// Signieren, im Stapellauf und im Fehlerschritt ohne Wirkung; sonst wie `Navigate(Open)`. Startet nichts (A1).
    WentHome,
    /// „Weiter bearbeiten“ (`UiEvent::ResumeParked`): geparkte Arbeit fortsetzen – ein Stapel
    /// ([`State::resumes_to_overview`]) in der Stapelübersicht wie „Zur Stapelübersicht“ (die Platzierung des offenen
    /// Dokuments wird dessen Plan, Nachtrag 13), sonst unverändert (Platzierung, „Fertig“). Startet nichts (A1).
    ParkedResumed,
    /// „Schließen“ der Karte „In Bearbeitung“ (`UiEvent::DiscardParked`): geparkte Arbeit verwerfen wie „Öffnen“
    /// (Dokumente „abgebrochen“, bereits signierte Ergebnisse bleiben). Startet nichts (A1).
    ParkedDiscarded,
}

/// Effekt, den `pida-core-api` ausführt.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Dokument laden, `analyze`, Platzhalter suchen
    Analyze(DocId),
    /// Seitenkachel rendern (Vorschau)
    RenderPage {
        /// Dokument
        doc: DocId,
        /// Seite
        page: usize,
    },
    /// Blockvorschau neu bauen (Profil/Emblem/Hinweis geändert)
    BuildBlockPreview(DocId),
    /// Backends prüfen (TCP-Probe, Keystore-Datei, FIDO2)
    ProbeBackends,
    /// Sitzung öffnen
    OpenSession(BackendId),
    /// Zertifikat lesen
    ReadCertificate,
    /// `prepare()` (persistiert auf Mobile den Zustand)
    Prepare {
        /// Dokument
        doc: DocId,
        /// Position
        position: PositionChoice,
        /// Mindestreserve für `/Contents` in Bytes (`None` = nach Backend-Modell,
        /// `Capabilities::cms_reserve`); gesetzt nach [`Event::CmsTooLarge`]
        min_reserve: Option<usize>,
    },
    /// Mehrfachsignatur: das eben signierte Ergebnis ist die Quelle der nächsten Runde (vor
    /// [`Effect::Prepare`]); [`Effect::OpenSession`] beginnt wieder beim Original
    NextSignature,
    /// Backend `sign`
    Sign,
    /// `finish_*` + Selbstprüfung
    Finish,
    /// Ausgabeziel bestimmen (F-82)
    DecideOutput(DocId),
    /// Speichern-Dialog zeigen
    AskSavePath(OutputDecision),
    /// Überschreib-Rückfrage
    AskOverwrite(PathBuf),
    /// atomar schreiben (eine vorhandene Datei wird **nicht** ersetzt)
    Write(PathBuf),
    /// atomar schreiben und eine vorhandene Datei ersetzen (nach bestätigter Rückfrage, F-84)
    Overwrite(PathBuf),
    /// Ergebnis öffnen (Opener/QuickLook/ACTION_VIEW)
    OpenResult(PathBuf),
    /// Ergebnis teilen (Share-Sheet bzw. `ACTION_SEND`, `ShellHooks::share_path`)
    ShareResult(PathBuf),
    /// Ergebnis exportieren („Sichern unter“: Dokument-Picker bzw. SAF, `ShellHooks::export_path`)
    ExportResult(PathBuf),
    /// Sitzung abbrechen (Passwort löschen, Benutzername bleibt)
    CancelSession,
    /// Sitzung regulär schließen (nach `sign`)
    CloseSession,
    /// Hinweis ohne Rückfrage anzeigen (i18n-ID, z. B. `error-PositioningNotPossible`)
    Notify(String),
    /// Einstellungen zurücksetzen (`Paths::reset`) und Kern neu initialisieren
    ResetSettings,
    /// persistierten Zustand löschen (Mobile: vorbereitetes Dokument und „Signatur fortsetzen“-Datensatz;
    /// nach Erfolg, Abbruch und Fehler – nie bei einem App-Wechsel)
    DeletePersisted,
    /// Dokumentdaten freigeben (Renderer, Kachel-Cache, vorbereitetes PDF)
    Discard(DocId),
    /// letzten Ordner merken
    RememberDirectory(PathBuf),
    /// Programmende
    Exit(ExitCode),
    // --- ADR 0004 (neue Varianten am Ende)
    /// automatische Platzierung (PDF-AS-Regel über `plan::build_plan` mit `PositionChoice::Auto`) →
    /// [`Event::AutoPlaced`]
    AutoPlace(DocId),
    /// Backend `sign_one`: genau ein Job mit eigenem Job-Abbruch (Arbeitsbereich; `Sign` bleibt für den
    /// klassischen Ablauf)
    SignDocument,
    /// vorbereitetes Dokument, Attribute und Ergebnis verwerfen, Sitzung und Kette behalten (Wiederholen
    /// ohne neue Anmeldung, ADR 0004 §3.6)
    ReleaseDocument,
    // --- ADR 0004 §3.8: Stapel
    /// Voranalyse: laden, analysieren, Platzhalter zählen, Miniatur; danach freigeben (nacheinander, niedrige
    /// Priorität) → [`Event::Preanalyzed`]
    Preanalyze(DocId),
    /// nur den laufenden Auftrag abbrechen („Dokument überspringen“), Sitzung und Zugangsdaten behalten
    SkipCurrentJob,
    /// Ausgabeziel eines Stapeldokuments bestimmen → [`Event::BatchOutputDecided`]
    DecideBatchOutput {
        /// Dokument
        doc: DocId,
        /// Ausgabeziel des Laufs
        output: BatchOutput,
        /// Regel für vorhandene Dateien
        on_existing: ExistingPolicy,
    },
    /// Hinweis mit Parametern (Fluent-Variablen)
    NotifyWith(String, BTreeMap<String, String>),
    // --- ADR 0004 §3.11 (K3): Einstellungen › Dokument
    /// QR-Platzhalter des Dokuments suchen (Platzhaltersuche nachträglich eingeschaltet bzw. auf
    /// „QR-Code-Platzhalter“ umgestellt) → [`Event::PlaceholderSearched`]
    SearchPlaceholder(DocId),
}

/// Ergebnis eines Reducer-Schritts.
#[derive(Debug, Clone, PartialEq)]
pub struct Transition {
    /// neuer Zustand
    pub state: State,
    /// Effekte in Ausführungsreihenfolge
    pub effects: Vec<Effect>,
}

impl Transition {
    /// Zustand ohne Effekte.
    pub fn stay(state: State) -> Transition {
        Transition { state, effects: Vec::new() }
    }

    /// Zustand mit Effekten.
    pub fn with(state: State, effects: Vec<Effect>) -> Transition {
        Transition { state, effects }
    }
}
