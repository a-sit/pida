//! Stapel im Arbeitsbereich (ADR 0004 §3.8, Paket K2): Datentypen der Stapelübersicht und des
//! Stapellaufs. Der Reducer dazu steht in `reducer/batch_run.rs`.
//!
//! **Eine Sitzung, eine Anmeldung, n Bestätigungen:** `StartBatch` öffnet genau eine Backend-Sitzung für
//! alle Dokumente des Laufs; je Dokument ein eigener Auftrag (`sign_one`) mit eigener Bestätigung. Das
//! Zertifikat wird höchstens einmal je Sitzung gelesen – vor dem ersten Dokument mit sichtbarem Block.
//! Dokumentfehler halten den Lauf nicht auf; nur Verbindungs- und Sitzungsfehler, eine Ablehnung in der App
//! und wiederholbare Dokumentfehler pausieren mit ausdrücklicher Entscheidung ([`BatchPause`]).
//!
//! Die Einträge der Übersicht reisen in [`crate::Queue::batch`] durch alle Zustände – auch durch die
//! Zustände eines aus der Übersicht geöffneten Einzeldokuments (`Analyzing` … `Output`), damit „Zurück zum
//! Stapel“ die Übersicht unverändert wiederfindet (Abweichung von der Skizze `BatchOverview { items }` in
//! ADR 0004 §3.8, reine Workflow-Interna).

use std::path::PathBuf;

use pida_backend_api::{BackendId, Capabilities};
use pida_pades::{AcceptanceClass, OutputInfo};
use pida_placement::{EmptyFieldCandidate, QrPlaceholder};

use crate::queue::DocId;
use crate::state::{
    BatchOutput, Context, DocFacts, ExistingPolicy, PositionChoice, RetryTarget, SignFlow, WorkflowError,
};

/// Voranalyse eines Stapeleintrags (laden, analysieren, Platzhalter zählen; danach freigegeben).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DocSummary {
    /// Seitenzahl
    pub page_count: usize,
    /// Größe der Datei in Bytes
    pub size_bytes: u64,
    /// bereits signiert
    pub signed: bool,
    /// Annahmeklasse
    pub class: AcceptanceClass,
    /// QR-Platzhalter gefunden (nur mit Platzhaltersuche, `kind = marker`)
    pub qr: bool,
    /// leere, nicht gesperrte Signaturfelder
    pub fields: usize,
    /// Warnungen der Analyse mit Abbruchmöglichkeit (Codes) – der Eintrag muss erst einzeln geprüft werden
    pub warnings: Vec<String>,
    /// gefundener QR-Platzhalter (Plan `Placeholder`; ergänzt gegenüber der Skizze, damit der Plan ohne
    /// erneute Suche feststeht)
    #[serde(default)]
    pub placeholder: Option<QrPlaceholder>,
    /// neues sichtbares Signaturfeld erlaubt (`false` bei DocMDP-Zertifizierung)
    #[serde(default = "yes")]
    pub new_visible_field_allowed: bool,
    /// erstes leeres, nicht gesperrtes Signaturfeld (Plan `Field` mit Platzhaltersuche „Leere
    /// Signaturfelder“; Einstellungen › Dokument, K3)
    #[serde(default)]
    pub first_field: Option<EmptyFieldCandidate>,
    /// die Voranalyse hat nach dem QR-Platzhalter gesucht (sonst nach dem Einschalten der Suche erneut
    /// voranalysieren; K3)
    #[serde(default)]
    pub qr_searched: bool,
}

fn yes() -> bool {
    true
}

/// Status eines Stapeleintrags (Spiegel von `pida_core_api::BatchItemStatusView`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum BatchItemStatus {
    /// Voranalyse läuft
    Analyzing,
    /// bereit (automatische Platzierung)
    Ready,
    /// mit Plan (QR-Platzhalter oder im Arbeitsbereich platziert)
    Positioned,
    /// abgelehnt (z. B. `error-PDFPwdProtected`) – wird nie signiert
    Rejected {
        /// Code
        code: String,
    },
    /// Analysewarnung: erst einzeln öffnen und bestätigen
    NeedsReview {
        /// Codes der Warnungen
        codes: Vec<String>,
    },
    /// wartet im Lauf
    Queued,
    /// wird signiert (Phase im [`BatchRun`])
    Active,
    /// signiert und gespeichert
    Signed {
        /// Ausgabe
        output: OutputInfo,
    },
    /// fehlgeschlagen (nicht wiederholbar; der Lauf ging weiter)
    Failed {
        /// Fehler
        error: WorkflowError,
    },
    /// übersprungen: durch die Nutzerin bzw. – `existing` – weil das Ziel bereits existiert
    Skipped {
        /// Ziel existierte (`on_existing = Skip`)
        existing: bool,
    },
    /// beim Abbruch des Stapels in Arbeit
    Cancelled,
}

impl BatchItemStatus {
    /// Darf `StartBatch` diesen Eintrag signieren (ADR 0004 §3.8: nur `Ready`/`Positioned`)?
    pub fn signable(&self) -> bool {
        matches!(self, BatchItemStatus::Ready | BatchItemStatus::Positioned)
    }

    /// Noch nicht begonnen (für die Zusammenfassung).
    pub fn not_started(&self) -> bool {
        matches!(
            self,
            BatchItemStatus::Analyzing
                | BatchItemStatus::Ready
                | BatchItemStatus::Positioned
                | BatchItemStatus::NeedsReview { .. }
                | BatchItemStatus::Queued
        )
    }
}

/// Eintrag der Stapelübersicht.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BatchItem {
    /// Dokument
    pub doc: DocId,
    /// Voranalyse (`None` = läuft noch)
    pub summary: Option<DocSummary>,
    /// Plan der Platzierung; `None` = automatisch (QR-Platzhalter bzw. PDF-AS-Regel beim Vorbereiten)
    pub plan: Option<PositionChoice>,
    /// Status
    pub status: BatchItemStatus,
    /// Analysewarnungen bestätigt (einzeln geöffnet und „Fortfahren“ bzw. im Arbeitsbereich platziert)
    #[serde(default)]
    pub reviewed: bool,
    /// Plan (bzw. kein Plan) aus der Automatik: folgt den Einstellungen › Dokument (Platzhaltersuche, Profil
    /// „unsichtbar“); `false` = Plan der Nutzerin (im Arbeitsbereich platziert), der bleibt (K3)
    #[serde(default = "yes")]
    pub auto_plan: bool,
    /// Mehrfachsignatur: Positionen der weiteren Signaturblöcke in ihrer Reihenfolge, ohne den Block von `plan`
    /// (leer = ein Block); nur mit einem Plan der Nutzerin
    #[serde(default)]
    pub more: Vec<PositionChoice>,
    /// Mehrfachsignatur: Stelle des Blocks von `plan` in der Reihenfolge aller Blöcke
    #[serde(default)]
    pub active: usize,
}

impl BatchItem {
    /// Anzahl der Signaturen, die das Dokument im Lauf erhält (von Hand platzierte Blöcke; automatisch eine).
    pub fn signatures(&self) -> usize {
        if self.auto_plan || matches!(self.plan, Some(PositionChoice::Invisible)) { 1 } else { 1 + self.more.len() }
    }

    /// Positionen aller Blöcke in ihrer Reihenfolge; `first` ist die Position des Blocks von `plan`.
    pub fn positions(&self, first: PositionChoice) -> Vec<PositionChoice> {
        let mut all = self.more.clone();
        all.insert(self.active.min(all.len()), first);
        all
    }

    /// Neuer Eintrag, Voranalyse ausstehend.
    pub fn analyzing(doc: DocId) -> BatchItem {
        BatchItem {
            doc,
            summary: None,
            plan: None,
            status: BatchItemStatus::Analyzing,
            reviewed: false,
            auto_plan: true,
            more: Vec::new(),
            active: 0,
        }
    }

    /// Status nach der Voranalyse bzw. nach „Zurück zum Stapel“: mit Plan `Positioned`, sonst `Ready`
    /// (Warnungen ohne Bestätigung: `NeedsReview`).
    pub fn settle(&mut self) {
        if let Some(s) = &self.summary
            && !s.warnings.is_empty()
            && !self.reviewed
        {
            self.status = BatchItemStatus::NeedsReview { codes: s.warnings.clone() };
            return;
        }
        self.status = if self.plan.is_some() { BatchItemStatus::Positioned } else { BatchItemStatus::Ready };
    }

    /// Von Hand platziert: eigener Plan der Nutzerin (im Arbeitsbereich platziert, nicht aus der Automatik).
    pub fn manual_plan(&self) -> bool {
        !self.auto_plan && self.plan.is_some()
    }

    /// Status für die Anzeige: mit automatischer Positionierung im Stapel ([`Batch::auto_position`]) gilt ein
    /// automatischer Plan (QR-Platzhalter bzw. erstes leeres Feld der Platzhaltersuche) als „Bereit“ – nur von
    /// Hand platzierte Einträge heißen „Positioniert“ (ADR 0004 Nachtrag 8).
    pub fn shown_status(&self, auto_position: bool) -> BatchItemStatus {
        match &self.status {
            BatchItemStatus::Positioned if auto_position && self.auto_plan => BatchItemStatus::Ready,
            other => other.clone(),
        }
    }
}

/// Ergebnis des letzten Laufs (die Zahlen der Zusammenfassung zählt die Projektion über **alle** Einträge
/// – vorher einzeln signierte zählen als signiert).
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BatchSummary {
    /// Dokumente des Laufs
    pub total: u32,
    /// im Lauf signiert
    pub signed: u32,
    /// abgebrochen
    pub aborted: bool,
    /// gewählter Ausgabeordner des Laufs (`None` = nach den Einstellungen)
    pub folder: Option<PathBuf>,
}

impl std::fmt::Debug for BatchSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // ohne Pfad (ADR 0001 Regel 9)
        f.debug_struct("BatchSummary")
            .field("total", &self.total)
            .field("signed", &self.signed)
            .field("aborted", &self.aborted)
            .field("folder", &self.folder.as_ref().map(|_| "<Pfad>"))
            .finish()
    }
}

/// Stapel: Einträge in Anzeigereihenfolge, geöffnetes Einzeldokument und letzter Lauf.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Batch {
    /// Einträge
    pub items: Vec<BatchItem>,
    /// aus der Übersicht im Arbeitsbereich geöffnetes Dokument (`OpenBatchItem`)
    pub open: Option<DocId>,
    /// Ergebnis des letzten Laufs
    pub last_run: Option<BatchSummary>,
    /// „Stapel abbrechen“ während des Schreibens: Dokument, dessen Ergebnis noch geschrieben wird (das Schreiben
    /// läuft ohne Epoche zu Ende; „gespeichert ist gespeichert“ – `Saved` verbucht es in der Übersicht als
    /// signiert, `SaveFailed` bzw. eine noch ausstehende Zielentscheidung lassen es „Abgebrochen“)
    #[serde(default)]
    pub writing: Option<DocId>,
    /// „Für diesen Stapel automatisch positionieren“ (ADR 0004 Nachtrag 8): gilt nur für diesen Stapel – bis zum
    /// Ende bzw. Abbruch des nächsten Laufs bzw. bis der Stapel geschlossen wird; die Einstellung
    /// `signature.auto_position` bleibt unverändert
    #[serde(default)]
    pub auto_position_once: bool,
}

impl Batch {
    /// Automatische Positionierung im Stapel: laut Einstellung ([`Context::auto_position`]) oder nur für diesen
    /// Stapel ([`Batch::auto_position_once`]).
    pub fn auto_position(&self, ctx: &Context) -> bool {
        ctx.auto_position || self.auto_position_once
    }

    /// Noch nicht signierte, von Hand platzierte Einträge ([`BatchItem::manual_plan`]) – sie behalten beim
    /// Einschalten der automatischen Positionierung ihren Plan, außer die Nutzerin bestätigt das Neusetzen.
    pub fn manual_plans(&self) -> usize {
        self.items.iter().filter(|i| i.status.signable() && i.manual_plan()).count()
    }

    /// Eintrag zum Dokument.
    pub fn item(&self, doc: DocId) -> Option<&BatchItem> {
        self.items.iter().find(|i| i.doc == doc)
    }

    /// Veränderbarer Eintrag zum Dokument.
    pub fn item_mut(&mut self, doc: DocId) -> Option<&mut BatchItem> {
        self.items.iter_mut().find(|i| i.doc == doc)
    }

    /// Signierbare Einträge (`Ready`/`Positioned`).
    pub fn signable(&self) -> usize {
        self.items.iter().filter(|i| i.status.signable()).count()
    }
}

/// Phase des aktuellen Dokuments im Stapellauf.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum BatchPhase {
    /// Sitzung öffnen (einmal je Lauf bzw. nach „Erneut versuchen“ einer Sitzungspause)
    OpeningSession,
    /// Zertifikat lesen (einmal je Sitzung, vor dem ersten Dokument mit sichtbarem Block)
    ReadingCertificate,
    /// Ziel prüfen (`on_existing = Skip`, **vor** dem Signieren – keine Bestätigung für ein Ergebnis, das
    /// nicht gespeichert würde)
    CheckingTarget,
    /// Dokument laden und analysieren
    Loading,
    /// `prepare()`
    Preparing,
    /// Bestätigung (`sign_one`)
    Confirming,
    /// `finish_*` + Selbstprüfung
    Finishing,
    /// Ziel bestimmen und schreiben
    Writing,
}

/// Pause eines Stapellaufs – nie automatisch weiter, ausdrückliche Entscheidung („Erneut versuchen“,
/// „Überspringen“, „Stapel abbrechen“).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum BatchPause {
    /// betrifft nur dieses Dokument (Ablehnung in der App, „CMS zu groß“, Speicherfehler, SL-Fehler,
    /// Zertifikats-Cache passt nicht); Sitzung und Zugangsdaten bleiben
    Document {
        /// Dokument
        doc: DocId,
        /// Fehler
        error: WorkflowError,
        /// Ziel von „Erneut versuchen“
        retry: RetryTarget,
        /// Phase, die „Erneut versuchen“ wieder aufnimmt
        resume: BatchPhase,
    },
    /// Verbindung (`new_session == false`: Sitzung bleibt offen) bzw. Sitzung (`true`: geschlossen,
    /// „Erneut versuchen“ öffnet eine neue – ggf. mit neuer Anmeldung)
    Session {
        /// aktuelles Dokument (`None` = vor dem ersten Dokument)
        doc: Option<DocId>,
        /// Fehler
        error: WorkflowError,
        /// „Erneut versuchen“ öffnet eine neue Sitzung
        new_session: bool,
        /// Phase, die „Erneut versuchen“ in derselben Sitzung wieder aufnimmt
        resume: BatchPhase,
    },
}

impl BatchPause {
    /// Dokument der Pause.
    pub fn doc(&self) -> Option<DocId> {
        match self {
            BatchPause::Document { doc, .. } => Some(*doc),
            BatchPause::Session { doc, .. } => *doc,
        }
    }

    /// Fehler der Pause.
    pub fn error(&self) -> &WorkflowError {
        match self {
            BatchPause::Document { error, .. } | BatchPause::Session { error, .. } => error,
        }
    }
}

/// Laufender Stapel mit **einer** Sitzung.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BatchRun {
    /// Signiermittel
    pub backend: BackendId,
    /// Fähigkeiten des Signiermittels beim Start
    pub caps: Capabilities,
    /// Dokumente des Laufs in Reihenfolge der Übersicht (später hinzugefügte gehören nicht dazu, A1)
    pub order: Vec<DocId>,
    /// Index des aktuellen Dokuments in `order`
    pub index: usize,
    /// Phase des aktuellen Dokuments
    pub phase: BatchPhase,
    /// Ablaufdaten des aktuellen Dokuments
    pub flow: SignFlow,
    /// Zertifikat in dieser Sitzung gelesen
    pub certificate_read: bool,
    /// Dokument, vor dem das Zertifikat gelesen wurde (Signierkarte: ab dem nächsten „aus der Sitzung“)
    pub certificate_doc: Option<DocId>,
    /// Ausgabeziel
    pub output: BatchOutput,
    /// Umgang mit vorhandenen Zieldateien
    pub on_existing: ExistingPolicy,
    /// Pause mit ausdrücklicher Entscheidung
    pub pause: Option<BatchPause>,
    /// abgeschlossene Bestätigungen
    pub confirmations_done: u32,
    /// in diesem Lauf signiert
    pub signed: u32,
    /// Sitzung offen (nach `PhaseCompleted(OpeningSession)`, bis zu einem Sitzungsfehler)
    pub session_open: bool,
    /// Dokumentdaten des aktuellen Dokuments nach der Analyse
    pub doc: Option<DocFacts>,
    /// Position des aktuellen Dokuments
    pub position: Option<PositionChoice>,
    /// „Dokument überspringen“ angefordert: wartet auf das Ende des laufenden Schritts (die Sitzung kommt
    /// mit dem abgebrochenen Auftrag zurück in den Kern)
    pub skipping: bool,
    /// Mehrfachsignatur: Positionen aller Blöcke des aktuellen Dokuments (leer = eine Signatur)
    #[serde(default)]
    pub positions: Vec<PositionChoice>,
    /// Mehrfachsignatur: laufende Runde (0-basiert); `position` ist die Position dieser Runde
    #[serde(default)]
    pub round: usize,
}

impl BatchRun {
    /// Aktuelles Dokument.
    pub fn current(&self) -> Option<DocId> {
        self.order.get(self.index).copied()
    }
}
