//! `pida-workflow` – expliziter Reducer statt impliziter Re-Entry-Semantik (Plan §3.5;
//! docs/analyse/01 §3).
//!
//! `reduce(state, event, ctx) -> Transition { state, effects }` ist eine **reine Funktion**.
//! Rendern, HTTP und Dateioperationen laufen als [`Effect`] in Tokio-Tasks (`pida-core-api`) und
//! melden sich als [`Event`] zurück. Die Übergangstabelle aus docs/analyse/01 §3.4 ist direkt in
//! Tabellentests übertragen (`tests/transitions.rs`); dabei werden B-03 (INVISIBLE beim ersten
//! Dokument), B-04 (Platzhalter-Flags nicht zurückgesetzt), B-05 (Feldwahl dauerhaft gespeichert),
//! B-06 (Abbruch der Platzhalterfrage ohne Auswahl-Screen), B-10 (Dateiname ohne Erweiterung),
//! B-14 (Batch nach Speicherfehler) und B-15 (`IllegalStateException` als Abbruch: nur
//! `BackendError::Cancelled` gilt als Abbruch) behoben.
//!
//! Zustände (ohne Re-Entry): `Starting → Idle → Analyzing → AnalysisWarning? → PlaceholderQuestion? → Positioning? →
//! ChoosingBackend → Signing{phase} → Output → (nächstes Dokument | Idle | Finished)`;
//! auf Mobile zusätzlich `Idle → Resuming → Signing{phase}` („Signatur fortsetzen“).
//! Einstellungen sind ein Overlay ([`State::settings_open`]) und kein eigener Zustand.
//!
//! # Abbildung der PDF-Over-Übergänge (docs/analyse/01 §3.4)
//!
//! | PDF-Over | hier |
//! |---|---|
//! | PrepareConfiguration → Open | `Starting` + [`Event::Started`] → `Idle` bzw. `Analyzing` (Dateien aus CLI/„Öffnen mit“) |
//! | Init-Fehler, `error.Initialization` (Ja = Werksreset) | [`Event::SettingsUnreadable`] → `Error{Config, ResetSettings}`; Wiederholen → [`Effect::ResetSettings`], Abbrechen → Ende |
//! | Warnung der Analyse (neu, Plan §6.1: `/Perms /UR3`, Hybrid-XFA, Seed Values) | `Analyzed{warnings}` → `AnalysisWarning`; [`Event::WarningAnswered`] `true` → weiter, `false`/Abbrechen → Dokument verworfen; ohne Oberfläche nur Hinweis |
//! | Open: Feld „Ja“ / QR „Ja“ | `PlaceholderQuestion` + `PlaceholderAnswered{Yes}` → `ChoosingBackend` mit [`PositionChoice::Field`] bzw. [`PositionChoice::Placeholder`] (Positionierung entfällt) |
//! | Open: Feldauswahl abgebrochen | `Yes` ohne gültigen Index bei mehreren Feldern → Frage bleibt |
//! | Open: „Abbrechen“ | Dokument verworfen, nächstes Dokument bzw. `Idle` (B-06) |
//! | Open → Positioning | sonst; `invisible` (auch beim ersten Dokument, B-03) bzw. `auto_position` → direkt `ChoosingBackend` |
//! | Positioning: PDF nicht ladbar → Retry/Cancel | [`Event::AnalysisFailed`] → `Error{Load}`; Wiederholen lädt neu, Abbrechen verwirft das Dokument |
//! | Positioning: Composite-Fehler | [`Event::PreviewUnavailable`] → Hinweis `error-PositioningNotPossible`, automatische Position |
//! | Positioning: bestätigt | [`Event::PositionConfirmed`] → `ChoosingBackend` mit [`PositionChoice::Manual`] |
//! | Positioning: „Neue Seite“ auf signiertem Dokument | wie PDF-AS (docs/analyse/04 §4.5) dieselben Koordinaten auf der letzten Seite, zusätzlich Hinweis `placement.notice.new_page_not_allowed` |
//! | „Unsichtbar signieren“ (neu) | [`Event::InvisibleChosen`] → `ChoosingBackend` mit [`PositionChoice::Invisible`] (nur dieses Dokument) |
//! | BKUSelection: Default/automatisch/Auswahl | [`Event::BackendsProbed`] wendet Standard-Backend (F-53) bzw. „nur ID Austria verfügbar“ (F-54) an; [`Event::BackendChosen`] |
//! | Autostart nur nach Nutzeraktion (neu, ADR 0003) | Ein Signiermittel mit `document_leaves_device` (ID Austria) startet die automatische Wahl nur nach „Signieren“ (`PositionConfirmed`, `InvisibleChosen`; `State::ChoosingBackend::sign_requested`), sonst erscheint die Auswahl; kein Zustimmungsschritt, ohne Oberfläche ein einzeiliger Hinweis [`MSG_DOCUMENT_LEAVES_DEVICE`] |
//! | PrepareSigning/LocalBKU/MobileBKU/KS/Signing | `Signing{OpeningSession → ReadingCertificate? → Preparing → Signing → Finishing}` |
//! | Signing: Erfolg → Output | `Finishing` fertig → `Output` + [`Effect::DecideOutput`] |
//! | Signing: Nutzerabbruch → BKUSelection | `BackendError::Cancelled` bzw. [`Event::Cancel`] → `ChoosingBackend` ohne automatische Wahl (F-55, F-72) |
//! | Signing: Fehler → Retry/Cancel | `Error{Backend, Prepare}`; Wiederholen öffnet die Sitzung neu (neu vorbereiten), Abbrechen → `ChoosingBackend` |
//! | CMS zu groß (neu, Plan §6.2) | [`Event::CmsTooLarge`] → `Error{CmsTooLarge, PrepareWithReserve(n)}` |
//! | Output: gespeichert, Queue nicht leer → Open | [`Event::Saved`] → nächstes Dokument (F-88) |
//! | Output: gespeichert, `-s` → Ende | [`Event::Saved`] + `skip_finish` → `Finished` (F-90) |
//! | Output: Dateien abgelegt | [`Event::FilesAdded`] im Ergebnis-Screen nach erfolgreichem Speichern → sofort weiter (F-89) |
//! | Output: Überschreiben/Speichern-Dialog/Schreibfehler | `OutputDecided` → `Write`/`AskOverwrite`/`AskSavePath`; `SaveFailed` → `Error{Save}` |
//! | ConfigurationUI | Overlay in `pida-core-api`; [`Event::SettingsChanged`] baut die Vorschau neu |
//! | Hauptleiste (`jumpToState`) | [`Event::Navigate`] nach der Matrix [`State::main_bar`] |
//!
//! # Mobile-Lebenszyklus (Plan §3.5, §5.2)
//!
//! | Ereignis | hier |
//! |---|---|
//! | App in den Hintergrund / zurück (z. B. Wechsel in die App „ID Austria“) | [`Event::Backgrounded`] / [`Event::Foregrounded`]: **kein Abbruch**, Zustand und Effekte unverändert (den Long-Poll startet `pida-core-api` nach der Rückkehr neu) |
//! | App vom System beendet, Neustart | [`Event::ResumeRequested`] (nur in `Idle`) → [`State::Resuming`]: Dokument laden, dann direkt das gespeicherte Signiermittel („Signatur fortsetzen“ ist die Nutzeraktion); ist es nicht verfügbar, Auswahl mit Hinweis [`MSG_RESUME_BACKEND_UNAVAILABLE`] und [`Effect::DeletePersisted`] |
//! | Erfolg, Abbruch, Fehler | [`Effect::DeletePersisted`] (der persistierte Zustand gilt nur für genau diesen Vorgang) |
//! | Ergebnis teilen / exportieren | [`Event::ShareResultRequested`] / [`Event::ExportResultRequested`] → [`Effect::ShareResult`] / [`Effect::ExportResult`] |
//!
//! # Arbeitsbereich-Modus (ADR 0004 §3.2–§3.7, [`Context::workspace`])
//!
//! | Ereignis / Weg | hier |
//! |---|---|
//! | Analyse fertig (auch unsichtbares Profil, `auto_position`, Platzhalter, ohne Vorschau) | **keine** Platzhalterfrage, **keine** automatische Backend-Wahl: `Positioning { edit }` mit [`WorkspaceEdit`]; QR-Platzhalter (nur Platzhaltersuche, `kind = marker`) → Ziel QR, erstes leeres Signaturfeld (nur Platzhaltersuche, `kind = fields`) → Ziel Feld, sonst frei mit [`Effect::AutoPlace`]; leere Felder sonst nur angeboten; ohne Vorschau `placement = None` mit Hinweis |
//! | [`Event::SettingsChanged`] (Einstellungen › Dokument, „sofort wirksam“) | automatisch platzierte Dokumente ([`WorkspaceEdit::automatic`]) folgen der Platzhaltersuche (QR, erstes Feld, frei mit [`Effect::AutoPlace`]); von Hand platzierte bleiben; fehlt die QR-Suche → [`Effect::SearchPlaceholder`] → [`Event::PlaceholderSearched`] |
//! | Startargument-Automatik ([`Context::cli_auto`]: `-b` und `-a`/`-v`) | klassischer automatischer Weg (D-06, F-88); [`Effect::OpenSession`] bei `BackendsProbed` nur für ein Signiermittel ohne `document_leaves_device`, sonst Arbeitsbereich |
//! | [`Event::InvisibleSet`], [`Event::TargetChosen`], [`Event::AutoPlaced`], [`Event::BlockResized`] | Bearbeitungszustand; Ziehen/Tastatur setzen `target = Free`, `origin = User`; verspätete automatische Lagen werden verworfen |
//! | [`Event::SignRequested`] („Signieren mit X“) | Position aus dem Bearbeitungszustand (unsichtbar, QR, Feld, frei = `Manual`, ohne Vorschau `Auto`), direkt `Signing` (kein Auswahlschritt); nicht verfügbar bzw. DocMDP-unzulässig → bleibt mit Hinweis |
//! | Signieren | [`Effect::SignDocument`] (`sign_one`); Sitzung bleibt bis nach der Selbstprüfung offen |
//! | Abbruch (`Cancel`, eigenes `Cancelled`) | zurück in den Arbeitsbereich mit unverändertem Bearbeitungszustand, [`MSG_SIGNING_CANCELLED`] |
//! | [`Event::SigningDeclined`] (SL 6000/6001 ohne eigenen Abbruch) | Fehler [`MSG_SIGNING_DECLINED`] mit „Erneut versuchen“, Sitzung bleibt offen |
//! | wiederholbare Fehler (Verbindung, Dokument, „CMS zu groß“, Vorbereiten) | [`Effect::ReleaseDocument`] statt `CloseSession`; „Erneut versuchen“ öffnet dieselbe Sitzung wieder (core-api) |
//! | „Schließen“ im Fehler, „Öffnen“, Dateien, Beenden | Arbeitsbereich bzw. Start; offen gehaltene Sitzung wird verworfen (F-72) |
//! | gespeichert mit wartenden Dokumenten | kein F-88: „Fertig“ mit „Nächstes Dokument“ (außer `cli_auto`) |
//! | Hauptleiste Positionieren/Signieren | ohne Wirkung |
//! | Einstellung „Automatische Positionierung“ (`auto_position`, Nachtrag 8) | nach der Analyse und bei geänderten Einstellungen [`WorkspaceEdit::follow_auto_position`]: automatisch platziert und gesperrt ([`PlacementLock::Locked`]) – [`Event::PlacementChanged`], [`Event::PageChanged`] und [`Event::TargetChosen`] bleiben ohne Wirkung, „Unsichtbar“, „Auf neuer Seite“ und „Signieren mit“ wirken; [`Event::PlacementUnlocked`] („Diesmal selbst platzieren“) hebt die Sperre für dieses Dokument auf; nie aus der Stapelübersicht geöffnet; signiert nie selbst (A1) |
//! | [`Event::WentHome`] („Zur Startseite“, Nachtrag 10) | Arbeitsbereich, Analysewarnung, Stapelübersicht und „Fertig“ ohne Speichern werden geparkt ([`Queue::parked`], Zustand unverändert); geparkte Arbeit ist nicht bedienbar (F-54), Ergebnisse von Effekten und Einstellungen wirken weiter; [`Event::ParkedResumed`] setzt fort – ein Stapel ([`State::resumes_to_overview`]) in der Stapelübersicht mit der Platzierung des offenen Dokuments als Plan (Nachtrag 13), sonst unverändert –, [`Event::ParkedDiscarded`] verwirft wie „Öffnen“; neue Dateien kommen zum Stapel (`MoreFilesAdded`, `FilesAdded` mit Hinweis [`MSG_PARKED_FILES_ADDED`]) bzw. ersetzen (`DocumentReplaced`); beim Signieren, im Lauf und im Fehler ohne Wirkung |
//! | [`Event::DocumentReplaced`] („Anderes PDF öffnen“, Nachtrag 9) | in `AnalysisWarning`/`Positioning`: Dokument verwerfen ([`Effect::Discard`], Ergebnis „abgebrochen“ bzw. aus dem Stapel entfernt), dann wie [`Event::FilesAdded`] in der Dokumentauswahl (wartende Dokumente bleiben; aus der Stapelübersicht geöffnet: zurück zur Übersicht mit den neuen Einträgen); sonst wie `FilesAdded`; startet nichts (A1) |
//!
//! Invariante **A1** (Property-Test `workspace_session_only_after_explicit_action`): `Effect::OpenSession`
//! entsteht nur in den Paaren (`SignRequested`, `Positioning`/`ChoosingBackend`), (`BackendChosen`,
//! `ChoosingBackend`), (`BackendsProbed`, `Resuming`), (`Retry`, Fehler des Signierens), (`BatchStarted`,
//! `BatchOverview`), (`Retry`, `BatchSigning` mit Sitzungspause) und – mit `cli_auto` – (`BackendsProbed`,
//! `ChoosingBackend`) für ein Signiermittel ohne `document_leaves_device`.
//!
//! # Stapel (ADR 0004 §3.8, [`Context::batch`], Modul [`batch`])
//!
//! | Ereignis / Weg | hier |
//! |---|---|
//! | ≥ 2 Dokumente (Auswahl, Start, nach dem Speichern) | [`State::BatchOverview`], [`Effect::Preanalyze`] je Dokument → [`Event::Preanalyzed`] (`Ready`, QR → `Positioned`, `Rejected`, `NeedsReview`) |
//! | [`Event::BatchItemOpened`] / [`Event::BackToBatch`] / [`Event::ShowBatch`] | Einzeldokument im Arbeitsbereich mit dem Plan des Eintrags; zurück mit der Platzierung als Plan; einzeln signiert → zurück, `Signed` |
//! | [`Event::MoreFilesAdded`] („Weitere PDFs hinzufügen“, Nachtrag 9) | im Arbeitsbereich (`Positioning`): Dateien anfügen, dann wie `ShowBatch` – alle zusammen in der Übersicht, die Platzierung des offenen Dokuments als dessen Plan; nichts Neues (Duplikat) → unverändert; sonst wie [`Event::FilesAdded`] |
//! | [`Event::BatchStarted`] | [`State::BatchSigning`] mit **einer** Sitzung (`Effect::OpenSession` nur hier und nach „Erneut versuchen“ einer Sitzungspause) |
//! | je Dokument | (Ziel prüfen) → laden → (Zertifikat einmal je Sitzung) → vorbereiten → `SignDocument` → abschließen → [`Effect::DecideBatchOutput`] → schreiben |
//! | [`Event::BatchSkip`] / [`Event::BatchAbort`] | [`Effect::SkipCurrentJob`] (Sitzung bleibt) bzw. `CancelSession` (F-72) |
//! | Fehler | nach `ErrorScope`: Pause Verbindung/Sitzung/Dokument ([`BatchPause`]) bzw. `Failed` und weiter |
//!
//! Ohne grafische Oberfläche ([`Context::headless`], CLI) gelten feste Regeln: automatische
//! Positionierung, QR-Platzhalter bzw. genau ein leeres Feld werden verwendet, kein Speichern-Dialog
//! (neben der Quelle), Fehler werden je Dokument verbucht ([`queue::DocResult`]) und es geht mit dem
//! nächsten Dokument weiter; nach dem letzten Dokument endet der Ablauf mit dem Exit-Code des ersten
//! fehlgeschlagenen Dokuments.
//!
//! Ownership: **Integration**.

pub mod batch;
mod output;
pub mod queue;
mod reducer;
mod state;
#[cfg(feature = "ts")]
pub mod ts_mirror;

pub use batch::{Batch, BatchItem, BatchItemStatus, BatchPause, BatchPhase, BatchRun, BatchSummary, DocSummary};
pub use output::{
    ExitCode, FsProbe, OutputDecision, PathProbe, chosen_path, decide_output, output_file_name, resolve_without_dialog,
    save_dialog,
};
pub use queue::{DocId, DocOutcome, DocResult, InputDocument, Queue};
pub use reducer::{grown_reserve, reduce};
pub use state::{
    BackendOption, BatchOutput, BlockSlot, Context, DocFacts, Effect, Event, ExistingPolicy, Follow, MAX_BLOCKS,
    MSG_BACKEND_CHOICE_REQUIRED, MSG_BATCH_ABORTED, MSG_BATCH_ADDED, MSG_BATCH_DONE, MSG_BATCH_ITEM_SIGNED,
    MSG_BATCH_NOTHING_TO_SIGN, MSG_BATCH_OUTPUT_FOLDER_REQUIRED, MSG_BATCH_PENDING_APP_REQUEST, MSG_BLOCK_TARGET_TAKEN,
    MSG_BLOCKS_LIMIT, MSG_CERT_CACHE_MISMATCH, MSG_CMS_TOO_LARGE, MSG_CONSTRAINT_CERTIFIED,
    MSG_CONSTRAINT_INVISIBLE_PROFILE, MSG_CONSTRAINT_SIGNED, MSG_DEFAULT_BACKEND_UNAVAILABLE,
    MSG_DOCUMENT_LEAVES_DEVICE, MSG_FIELD_LOCKED, MSG_INITIALIZATION, MSG_INVARIANT, MSG_NO_BACKEND,
    MSG_PARKED_FILES_ADDED, MSG_POSITIONING_NOT_POSSIBLE, MSG_RESUME_BACKEND_UNAVAILABLE, MSG_SAVE_FAILED,
    MSG_SIGNER_UNAVAILABLE, MSG_SIGNING_CANCELLED, MSG_SIGNING_DECLINED, MSG_UNEXPECTED, MainBarStep, ParkedPlacement,
    PlaceholderAnswer, PlaceholderFound, PlacementLock, PlacementOrigin, PositionChoice, ResumePlan, RetryTarget,
    Rounds, SignFlow, SigningPhase, State, Step, Target, Transition, WorkflowError, WorkspaceEdit, backend_for_choice,
};
