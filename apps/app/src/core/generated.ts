// Automatisch erzeugt aus den Rust-Typen der Bridge (crates/core-api/src/view.rs und abhängige
// Crates) mit ts-rs – NICHT VON HAND ÄNDERN.
// Neu erzeugen: PIDA_BLESS=1 cargo test -p pida-core-api --test ts_bindings
//
// Abbildung (serde_json): Option<T> → T | null, PathBuf → string, u64/usize → number,
// Enums ohne serde-Attribut extern getaggt ({ "Variante": … } bzw. "Variante"),
// #[serde(tag = …)] intern getaggt.

/**
 * Unterstützte Sprachen.
 */
export type Lang = "de" | "en";

/**
 * Kennung eines Dokuments innerhalb einer Sitzung.
 */
export type DocId = number;

/**
 * Schritte der Hauptleiste (F-100).
 */
export type Step = "Open" | "Positioning" | "Signing" | "Finish";

/**
 * Zustand eines Schritts in der Hauptleiste (Tabelle docs/analyse/01 §3.5).
 */
export type MainBarStep = { 
/**
 * Schritt
 */
step: Step, 
/**
 * erreicht („A“)
 */
active: boolean, 
/**
 * klickbar („E“)
 */
enabled: boolean, };

/**
 * Antwort auf „Platzhalter verwenden?“.
 */
export type PlaceholderAnswer = "Yes" | "No" | "Cancel";

/**
 * Spiegel von `pida_pades::AcceptanceClass`.
 */
export type AcceptanceClass = "K1Regular" | "K2Repairable" | "K3Encrypted" | "K4Restricted" | "K5Rejected";

/**
 * Spiegel von `pida_pades::Invariant`.
 */
export type Invariant = "I1Prefix" | "I2ByteRange" | "I3Cms" | "I4PriorSignatures" | "I5Reparse" | "I6AllowedChanges";

/**
 * Spiegel von `pida_pades::InvariantViolation`.
 */
export type InvariantViolation = { 
/**
 * Invariante
 */
invariant: Invariant, 
/**
 * Detail (ohne Dokumentinhalt)
 */
detail: string, };

/**
 * Spiegel von `pida_pades::OutputInfo`.
 */
export type OutputInfo = { 
/**
 * Pfad
 */
path: string, 
/**
 * Länge in Bytes
 */
len: number, 
/**
 * SHA-256 (hex)
 */
sha256: string, };

/**
 * Fehler im Workflow (Fehlercode + Parameter für i18n).
 */
export type WorkflowError = { "Rejected": { 
/**
 * Klasse
 */
class: AcceptanceClass, 
/**
 * Code
 */
code: string, } } | { "Backend": { 
/**
 * `BackendError::code()`
 */
code: string, 
/**
 * Detail
 */
detail: string, } } | { "Invariant": InvariantViolation } | { "CmsTooLarge": { 
/**
 * benötigt
 */
needed: number, 
/**
 * vorhanden
 */
reserve: number, } } | { "Config": string } | { "Save": string } | { "Prepare": { 
/**
 * Code (`sigblock-…`, `placement.…`, `ng-certificate-not-valid`, `error-PrepareDocument`)
 */
code: string, 
/**
 * Detail
 */
detail: string, } } | { "Unexpected": string };

/**
 * Ergebnis eines abgeschlossenen Dokuments.
 */
export type DocResult = { "kind": "signed", 
/**
 * Ausgabe
 */
output: OutputInfo, } | { "kind": "cancelled" } | { "kind": "failed", 
/**
 * Fehler (Code und Parameter für i18n)
 */
error: WorkflowError, };

/**
 * Herkunft der Platzierung im Arbeitsbereich (ADR 0004 §3.3; Typ für Reducer **und** Bridge, §3.0
 * Regel 9). Der Reducer erzeugt die Werte ab K1; bis dahin dient der Typ nur dem Vertrag.
 */
export type PlacementOrigin = "auto" | "user" | "field" | "qr";

/**
 * Umgang mit bereits vorhandenen Zieldateien im Stapellauf (ADR 0004 §3.8, §3.0 Regel 9). Standard im
 * Startdialog: „Beide behalten“ (§8 F11).
 */
export type ExistingPolicy = "overwrite" | "keep_both" | "skip";

/**
 * Ausgabeziel eines Stapellaufs (ADR 0004 §3.8, §3.0 Regel 9): nach den Einstellungen
 * (`output.folder`, F-82; Mobile: App-Dokumente) oder ein vor dem Start gewählter Ordner.
 * Serde: `{ "kind": "configured" }` bzw. `{ "kind": "folder", "path": "…" }`.
 */
export type BatchOutput = { "kind": "configured" } | { "kind": "folder", "path": string };

/**
 * Kennung eines Backends.
 */
export type BackendId = "atrust_native" | "atrust_browser" | "csc" | "local_bku" | "keystore" | "wallet";

/**
 * Spiegel von `pida_cades::DigestAlg`.
 */
export type DigestAlg = "Sha1" | "Sha256" | "Sha384" | "Sha512";

/**
 * Was das Backend signiert.
 */
export type SignInput = "Document" | { "Digest": { 
/**
 * unterstützte Hashverfahren
 */
algs: Array<DigestAlg>, } };

/**
 * Was das Backend liefert.
 */
export type SignOutput = "Cms" | "RawSignature";

/**
 * Wann ein Zertifikat vorab benötigt wird.
 */
export type CertNeed = "Required" | "OnlyForVisibleBlock" | "No";

/**
 * Art der Interaktion.
 */
export type Interaction = "None" | "Secret" | "NativeForms" | "EmbeddedWeb" | "SystemBrowser" | "External" | "ExternalApp";

/**
 * Benötigter Server.
 */
export type ServerNeed = "None" | "DataUrlRelay" | "TokenProxy";

/**
 * Qualifikation der Signatur (Anzeige).
 */
export type Qualification = "Qes" | "AdES" | "Unknown";

/**
 * Faktoren (nur Anzeige).
 */
export type FactorSet = { 
/**
 * ID-Austria-App
 */
app: boolean, 
/**
 * SMS-TAN
 */
sms: boolean, 
/**
 * QR-Code
 */
qr: boolean, 
/**
 * FIDO2
 */
fido2: boolean, 
/**
 * Signaturkarte
 */
card: boolean, };

/**
 * Fähigkeiten eines Backends in einer Umgebung.
 */
export type Capabilities = { 
/**
 * Eingabe
 */
input: SignInput, 
/**
 * Ausgabe
 */
output: SignOutput, 
/**
 * Zertifikat vorab
 */
certificate: CertNeed, 
/**
 * Interaktion
 */
interaction: Interaction, 
/**
 * Server
 */
server: ServerNeed, 
/**
 * Höchstzahl Dokumente je Freigabe (1 = je Dokument eine Freigabe)
 */
max_batch: number, 
/**
 * Eine Anmeldung für den ganzen Batch (Szenario A wie PDF-Over)
 */
login_once_per_batch: boolean, 
/**
 * Dokument verlässt das Gerät (z. B. ID Austria → A-Trust): nicht blockierender Kurzhinweis beim
 * Signiermittel; die automatische Wahl startet es nur nach „Signieren“ (ADR 0003)
 */
document_leaves_device: boolean, 
/**
 * Qualifikation
 */
qualification: Qualification, 
/**
 * Faktoren
 */
factors: FactorSet, 
/**
 * Empfohlene CMS-Reserve in Bytes (Modell B: 16 KiB, Plan §6.2)
 */
cms_reserve: number | null, };

/**
 * Validierungs- bzw. Fehlerhinweis in einer Maske (i18n-Keys aus PDF-Over).
 */
export type FormProblem = "PasswordTooShort" | "PasswordTooLong" | "NoTan" | "TanTooLong" | "EnteredReferenceValue" | "QrImageFailed" | { "Fido2Failed": string } | "LoginRejected" | { "Atrust": string } | { "Fido2Error": { 
/**
 * stabiler Code (`fido2-…`)
 */
code: string, 
/**
 * Diagnosetext
 */
detail: string, } };

/**
 * Gemeinsamer Kontext der A-Trust-Masken.
 */
export type FormContext = { 
/**
 * Link „Signaturdaten anzeigen“ (`ShowSigobj.aspx`), nur wenn A-Trust ihn liefert (F-68).
 */
signature_data_url: string | null, 
/**
 * „SMS anfordern“ möglich (`#SmsButton`).
 */
can_switch_to_sms: boolean, 
/**
 * „FIDO2“ angeboten (`#FidoButton`).
 */
can_switch_to_fido2: boolean, 
/**
 * FIDO2 auf dieser Plattform verfügbar (Button sichtbar).
 */
fido2_visible: boolean, 
/**
 * Meldungszeile (Disclaimer `mobileBKU.aTrustDisclaimer` bzw. Fehler).
 */
problem: FormProblem | null, };

/**
 * QR-Bild (per HTTP von A-Trust geladen, R8).
 */
export type QrImage = { 
/**
 * Bildbytes (typisch PNG)
 */
bytes: Array<number>, 
/**
 * MIME-Typ, falls bekannt
 */
mime: string | null, };

/**
 * Native Maske (Szenario A).
 */
export type NativeForm = { "Login": { 
/**
 * Vorbelegung (gemerkt bzw. `-n`; in der UI nicht normalisiert)
 */
prefill_username: string | null, 
/**
 * Zustand „Merken“ (`MOBILE_REMEMBER_PASSWORD`)
 */
remember: boolean, 
/**
 * Kontext
 */
ctx: FormContext, } } | { "SmsTan": { 
/**
 * `#vergleichswert`
 */
reference_value: string, 
/**
 * Kontext
 */
ctx: FormContext, } } | { "QrCode": { 
/**
 * Vergleichswert
 */
reference_value: string, 
/**
 * Bild (fehlt, wenn das Laden scheiterte)
 */
image: QrImage | null, 
/**
 * Restzeit des Long-Polls in Sekunden beim Anzeigen (Countdown, Plan §4.4; Default 300 s
 * aus `PollConfig`); `None` = unbekannt. Die UI zählt selbst herunter.
 */
timeout_secs: number | null, 
/**
 * Kontext
 */
ctx: FormContext, } } | { "WaitForApp": { 
/**
 * Vergleichswert
 */
reference_value: string | null, 
/**
 * Restzeit des Long-Polls in Sekunden (Countdown, Plan §4.4); `None` = unbekannt.
 */
timeout_secs: number | null, 
/**
 * Kontext
 */
ctx: FormContext, } } | { "Fido2": { 
/**
 * Vorgang läuft (`common.working`, Button deaktiviert)
 */
in_progress: boolean, 
/**
 * Kontext
 */
ctx: FormContext, } };

/**
 * Art der Passwortabfrage (Keystore, Proxy).
 */
export type SecretKind = "KeystorePassword" | "KeyPassword" | "ProxyPassword" | "Fido2Pin";

/**
 * Passwortabfrage.
 */
export type SecretPrompt = { 
/**
 * Art
 */
kind: SecretKind, 
/**
 * Bezug (Keystore-Dateiname, Alias, Proxy-Host) – ohne Pfad zu Nutzerdaten in Logs
 */
subject: string, 
/**
 * Vorheriger Versuch war falsch (`error.KeyStoreStorePass` …)
 */
retry: boolean, };

/**
 * Art einer Meldung.
 */
export type NoticeKind = "Info" | "Warning" | "Error";

/**
 * Knöpfe einer Meldung.
 */
export type NoticeButtons = "Ok" | "OkCancel" | "RetryCancel";

/**
 * Inhalt einer Meldung (i18n über den Code; Texte von A-Trust roh in [`Notice::text`]).
 */
export type NoticeCode = "AtrustInterstitial" | "AtrustError" | "ProxyAuthRequired" | "DocumentTooLarge" | { "Http": { 
/**
 * Status
 */
status: number, } } | "ConnectionFailed" | "LocalBkuNotReachable" | "KeystoreFileMissing" | "KeystoreAliasMissing" | "KeystoreError" | "KeystoreConversionNeeded";

/**
 * Meldung.
 */
export type Notice = { 
/**
 * Art
 */
kind: NoticeKind, 
/**
 * Code
 */
code: NoticeCode, 
/**
 * Freitext (A-Trust-Meldung, HTTP-Reason)
 */
text: string | null, 
/**
 * Knöpfe
 */
buttons: NoticeButtons, };

/**
 * Antwort auf eine Meldung.
 */
export type NoticeAnswer = "Ok" | "Retry" | "Cancel";

/**
 * Fortschritt (Warte-Screen, Live-Region).
 */
export type Progress = "Connecting" | "ReadingCertificate" | "WaitingForServer" | "Signing" | { "Document": { 
/**
 * 1-basiert
 */
index: number, 
/**
 * gesamt
 */
total: number, } } | "Done";

/**
 * Spiegel von `pida_placement::Placement` (PAGE_ABS, linke obere Ecke, y = Oberkante von unten, pt).
 */
export type Placement = { 
/**
 * 0-basierter Seitenindex; `== Seitenzahl` = virtuelle neue Seite
 */
page_index: number, 
/**
 * linke Kante in pt
 */
x: number, 
/**
 * Oberkante in pt (von unten)
 */
y: number, 
/**
 * Breite in pt
 */
width: number, 
/**
 * Höhe in pt
 */
height: number, };

/**
 * Spiegel von `pida_config::SignatureProfile`.
 */
export type SignatureProfile = "SIGNATURBLOCK_SMALL" | "AMTSSIGNATURBLOCK" | "BASE_LOGO" | "INVISIBLE";

/**
 * Spiegel von `pida_config::NoteSetting`.
 */
export type NoteSetting = { "mode": "default" } | { "mode": "none" } | { "mode": "custom", "text": string };

/**
 * Spiegel von `pida_config::WindowSize`.
 */
export type WindowSize = { 
/**
 * Breite
 */
width: number, 
/**
 * Höhe
 */
height: number, };

/**
 * Spiegel von `pida_config::GeneralSettings`.
 */
export type GeneralSettings = { 
/**
 * UI-Sprache (`None` = System)
 */
ui_language: Lang | null, 
/**
 * Update-Prüfung
 */
update_check: boolean, 
/**
 * Fenstergröße
 */
window_size: WindowSize | null, 
/**
 * letzter Ordner (Dateidialog, F-11)
 */
last_source_directory: string | null, 
/**
 * „Zuletzt signiert“ merken (ADR 0004 §3.10)
 */
recent_documents: boolean, 
/**
 * Ersteinrichtung abgeschlossen (ADR 0004 §3.12)
 */
first_run_completed: boolean, };

/**
 * Spiegel von `pida_config::CmsProfileChoice`.
 */
export type CmsProfileChoice = "pdfas_compat" | "pades_bb";

/**
 * Spiegel von `pida_config::SignatureSettings`.
 */
export type SignatureSettings = { 
/**
 * Profil
 */
profile: SignatureProfile, 
/**
 * Sprache des Signaturblocks (`None` = System)
 */
block_language: Lang | null, 
/**
 * Hinweis
 */
note: NoteSetting, 
/**
 * PDF/A erzwingen
 */
pdfa_force: boolean, 
/**
 * automatische Positionierung
 */
auto_position: boolean, 
/**
 * Bildmarke
 */
emblem: string | null, 
/**
 * Größe „nur Bildmarke“ in mm
 */
logo_only_size_mm: number, 
/**
 * CMS-Profil
 */
cms_profile: CmsProfileChoice, };

/**
 * Spiegel von `pida_config::PlaceholderKind`.
 */
export type PlaceholderKind = "fields" | "marker";

/**
 * Spiegel von `pida_config::PlaceholderSettings`.
 */
export type PlaceholderSettings = { 
/**
 * Suche aktiv
 */
search: boolean, 
/**
 * Art
 */
kind: PlaceholderKind, };

/**
 * Spiegel von `pida_config::BackendChoice`.
 */
export type BackendChoice = "id_austria" | "local_bku" | "keystore";

/**
 * Spiegel von `pida_config::BackendSettings`.
 */
export type BackendSettings = { 
/**
 * Standard-Signiermittel
 */
default: BackendChoice | null, };

/**
 * Spiegel von `pida_config::IdAustriaSettings` (das Passwort wird nie persistiert).
 */
export type IdAustriaSettings = { 
/**
 * Benutzername/Telefonnummer
 */
username: string | null, 
/**
 * „Merken“
 */
remember: boolean, 
/**
 * FIDO2 als Standard
 */
fido2_default: boolean, };

/**
 * Spiegel von `pida_config::PasswordStorage`.
 */
export type PasswordStorage = "none" | "memory" | "keychain";

/**
 * Spiegel von `pida_config::KeystoreSettings`.
 */
export type KeystoreSettings = { 
/**
 * aktiviert
 */
enabled: boolean, 
/**
 * Datei
 */
file: string | null, 
/**
 * angegebener Typ
 */
declared_type: string | null, 
/**
 * Alias
 */
alias: string | null, 
/**
 * Passwort-Speicherart
 */
password_storage: PasswordStorage, 
/**
 * Speicherart ausdrücklich gewählt (`false` = die erste Passworteingabe fragt „Passwort merken?“)
 */
storage_chosen: boolean, };

/**
 * Spiegel von `pida_config::OutputSettings`.
 */
export type OutputSettings = { 
/**
 * Ausgabeordner
 */
folder: string | null, 
/**
 * Dateinamenzusatz
 */
postfix: string, };

/**
 * Spiegel von `pida_config::ProxyMode`.
 */
export type ProxyMode = "system" | "none" | "manual";

/**
 * Spiegel von `pida_config::ProxySettings`.
 */
export type ProxySettings = { 
/**
 * Modus
 */
mode: ProxyMode, 
/**
 * Host
 */
host: string | null, 
/**
 * Port
 */
port: number | null, 
/**
 * Benutzer
 */
user: string | null, };

/**
 * Spiegel von `pida_config::CertificateCacheSettings`.
 */
export type CertificateCacheSettings = { 
/**
 * Opt-in
 */
enabled: boolean, };

/**
 * Spiegel von `pida_config::PlacementStart`.
 */
export type PlacementStart = "preview" | "methods";

/**
 * Spiegel von `pida_config::AnnounceDetail`.
 */
export type AnnounceDetail = "short" | "detailed";

/**
 * Spiegel von `pida_config::KeyboardStep`.
 */
export type KeyboardStep = "standard" | "mm1" | "mm5" | "mm10";

/**
 * Spiegel von `pida_config::AccessibilitySettings`.
 */
export type AccessibilitySettings = { 
/**
 * „Barrierefreie Bedienung“
 */
mode: boolean, 
/**
 * Womit die Platzierung beginnt
 */
start: PlacementStart, 
/**
 * Umfang der Ansagen
 */
announce: AnnounceDetail, 
/**
 * erst nach dem Verschieben ansagen
 */
announce_after_move: boolean, 
/**
 * Schrittweite der Pfeiltasten
 */
keyboard_step: KeyboardStep, 
/**
 * vor dem Signieren prüfen
 */
check_before_sign: boolean, };

/**
 * Spiegel von `pida_config::Settings`. Unbekannte Einträge (`Settings::unknown`, `flatten`) stehen
 * auf oberster Ebene und gehen beim Speichern unverändert zurück.
 */
export type Settings = { 
/**
 * Schema-Version
 */
schema_version: number, 
/**
 * Allgemein
 */
general: GeneralSettings, 
/**
 * Signatur
 */
signature: SignatureSettings, 
/**
 * Platzhalter
 */
placeholder: PlaceholderSettings, 
/**
 * Signiermittel
 */
backend: BackendSettings, 
/**
 * ID Austria
 */
id_austria: IdAustriaSettings, 
/**
 * Keystore
 */
keystore: KeystoreSettings, 
/**
 * Ausgabe
 */
output: OutputSettings, 
/**
 * Proxy
 */
proxy: ProxySettings, 
/**
 * Zertifikats-Cache
 */
certificate_cache: CertificateCacheSettings, 
/**
 * Barrierefreiheit
 */
accessibility: AccessibilitySettings, } & ({ [key in string]: unknown });

/**
 * Signaturbild, das den Platzhalter beim Signieren ersetzt.
 */
export type QrPlaceholderProfile = "standard" | "amtssignatur" | "signer";

/**
 * Dateiformat.
 */
export type QrPlaceholderFormat = "png" | "svg";

/**
 * Was der Platzhalter enthält.
 */
export type QrPlaceholderSpec = { 
/**
 * Nummer (nur Ziffern, 1 bis 10 Stellen; „0001“ und „1“ gelten beim Signieren als gleich)
 */
id: string, 
/**
 * Signaturbild
 */
profile: QrPlaceholderProfile, 
/**
 * Sprache des Signaturblocks und der Beschriftung
 */
language: Lang, };

/**
 * Hinweis ohne Rückfrage (i18n-ID mit Parametern), z. B. `error-PositioningNotPossible`,
 * `placement.notice.*`, `ng-config-reset-done`. Die UI schließt ihn mit [`UiEvent::DismissNotice`].
 */
export type NoticeView = { 
/**
 * Kennung
 */
id: number, 
/**
 * Meldungs-ID
 */
code: string, 
/**
 * Parameter
 */
params: { [key in string]: string }, };

/**
 * Speichern-Dialog (F-83/F-86): Startordner und Namensvorschlag.
 */
export type SaveDialogView = { 
/**
 * Startordner
 */
directory: string, 
/**
 * vorgeschlagener Dateiname
 */
file_name: string, };

/**
 * Angebot, die Einstellungen aus `~/.pdf-over` zu übernehmen (Plan §7.2, einmalig).
 */
export type MigrationOfferView = { 
/**
 * gefundene `PDF-Over.config`
 */
legacy_file: string, 
/**
 * die Altdatei enthält Klartext-Passwörter (Keystore/Proxy): Zustimmung zum Übernehmen in den
 * Schlüsselbund erfragen (`UiEvent::AcceptMigration::store_secrets`)
 */
plaintext_secrets: boolean, 
/**
 * Was übernommen würde (Ersteinrichtung Schritt 1, ADR 0004 §3.12): i18n-IDs `ng-migration-item-*`
 * mit Detail; per Probelauf von `pida_config::migrate` (ohne zu schreiben)
 */
items: Array<MigrationItemView>, };

/**
 * Abgeschlossenes Dokument (Warteschlangen-Panel, CLI-Ergebnis).
 */
export type DoneItemView = { 
/**
 * Kennung
 */
id: DocId, 
/**
 * Name
 */
name: string, 
/**
 * Eingabepfad
 */
input: string, 
/**
 * Ergebnis
 */
result: DocResult, };

/**
 * Dokument (Anzeige).
 */
export type DocInfo = { 
/**
 * Kennung
 */
id: DocId, 
/**
 * Name
 */
name: string, 
/**
 * Seitenzahl
 */
page_count: number, 
/**
 * sichtbare Seitengrößen in pt
 */
page_sizes: Array<[number, number]>, 
/**
 * bereits signiert
 */
signed: boolean, };

/**
 * Eintrag der Warteschlange.
 */
export type QueueItemView = { 
/**
 * Kennung
 */
id: DocId, 
/**
 * Name
 */
name: string, 
/**
 * aktuell
 */
current: boolean, };

/**
 * Leeres Feld (Auswahlliste).
 */
export type FieldInfo = { 
/**
 * qualifizierter Name
 */
name: string, 
/**
 * Seite
 */
page: number | null, };

/**
 * QR-Platzhalter.
 */
export type QrInfo = { 
/**
 * Seite
 */
page: number, 
/**
 * id
 */
id: string | null, };

/**
 * Blockgröße und Vorschau.
 */
export type BlockView = { 
/**
 * Breite in pt
 */
width: number, 
/**
 * Höhe in pt
 */
height: number, 
/**
 * Vorschau-Revision (Cache-Busting für `blockPreviewUrl`)
 */
preview_revision: number, 
/**
 * Der Block, der hier signiert wird, zeigt die neutrale Platzhalter-Bildmarke der Amtssignatur statt der
 * Bildmarke der Behörde (Backlog Nr. 10): Profil Amtssignatur ohne ladbare eigene Bildmarke – am QR-Platzhalter
 * das Profil, das der Platzhalter vorgibt (`profile=…`). Die Oberfläche weist inline darauf hin. Additiv.
 */
emblem_placeholder: boolean, 
/**
 * Ändert eine eigene Bildmarke (Einstellungen) diesen Block? `false`, wenn ein QR-Platzhalter ein anderes Profil
 * vorgibt – dessen Block übernimmt die eigene Bildmarke nicht (PDF-Over). Dann kein „Bildmarke wählen“. Additiv.
 */
emblem_choosable: boolean, 
/**
 * Sprache des Signaturblocks (Einstellung `signature.block_language`, ohne Angabe die Systemsprache); der
 * Inspektor schaltet sie mit `set_block_language` um. Additiv.
 */
language: Lang, };

/**
 * Backend-Option.
 */
export type BackendOptionView = { 
/**
 * Backend
 */
id: BackendId, 
/**
 * verfügbar
 */
available: boolean, 
/**
 * Fähigkeiten
 */
capabilities: Capabilities | null, 
/**
 * Hinweise zur Option (i18n-IDs), z. B. `ng-fido2-unavailable-mobile`: ID Austria auf Mobile ohne
 * FIDO2 – die UI zeigt FIDO2 ausgegraut mit diesem Hinweis (Plan §4.7, §5.2)
 */
notes: Array<string>, };

/**
 * Betriebssystem der Shell.
 */
export type PlatformOs = "windows" | "macos" | "linux" | "ios" | "android";

/**
 * Plattform der Oberfläche (Plan §5.1): steuert plattformabhängige Bedienelemente.
 */
export type PlatformView = { 
/**
 * Betriebssystem
 */
os: PlatformOs, 
/**
 * Mobile Oberfläche (iOS/Android, Plan §5.2): Teilen und Exportieren statt „Im Ordner anzeigen“,
 * signiert wird nur mit ID Austria (kein Keystore, ADR 0004 Nachtrag 11; keine lokale BKU), kein FIDO2
 * (ausgegraut mit Hinweis), keine Proxy-Einstellungen (System-Proxy), keine Ausgabeordner-Wahl
 * (App-Dokumente)
 */
mobile: boolean, };

/**
 * Angebot „Signatur fortsetzen“ (Mobile, Plan §3.5): Beim letzten Lauf wurde die App während eines
 * Signiervorgangs vom System beendet (z. B. beim Wechsel in die App „ID Austria“). Antwort über
 * [`UiEvent::ResumeSignature`] bzw. [`UiEvent::DiscardResume`]; nur im Schritt `open` gesetzt.
 */
export type ResumeOfferView = { 
/**
 * Name des Dokuments
 */
name: string, 
/**
 * Signiermittel des unterbrochenen Vorgangs
 */
backend: BackendId, 
/**
 * vor wie vielen Minuten vorbereitet (Anzeige)
 */
age_minutes: number, 
/**
 * weitere Dokumente des unterbrochenen Batches
 */
pending: number, 
/**
 * Das Dokument verlässt mit diesem Signiermittel das Gerät (`document_leaves_device`, z. B. ID Austria):
 * Kurzhinweis „wird an A-Trust übermittelt“ beim Angebot – „Fortsetzen“ startet die Sitzung (ADR 0003)
 */
document_leaves_device: boolean, };

/**
 * Ergebnis des PKCS#12-Imports in die App-Sandbox (früherer Mobile-Import-Assistent; Plan §5.2) – schließen
 * mit [`UiEvent::DismissKeystoreImport`]. Seit ADR 0004 Nachtrag 11 gibt es auf Mobilgeräten keinen Keystore
 * (nur ID Austria): der Kern setzt es dort nie; der Typ bleibt im Vertrag (additiv, §3.0 Regel 2).
 */
export type KeystoreImportView = { 
/**
 * Dateiname (ohne Pfad)
 */
file_name: string, 
/**
 * importiert (Einstellungen › Keystore zeigen auf die Kopie in der Sandbox)
 */
ok: boolean, 
/**
 * Meldungs-ID: `ng-keystore-imported` bzw. der Fehler (`keystore-wrong-password`,
 * `ng-keystore-import-format`, …)
 */
code: string, 
/**
 * Passwort im sicheren Speicher abgelegt (Keychain bzw. Android Keystore)
 */
password_stored: boolean, };

/**
 * Am Inhalt erkanntes Keystore-Format (Einstellungen › Signiermittel › Keystore; ersetzt die Typauswahl).
 */
export type KeystoreFormatView = "pkcs12" | "jks" | "jceks" | "unknown";

/**
 * Eintrag eines PKCS#12-Keystores für die Alias-Auswahl (Einstellungen › Signiermittel › Keystore; nur
 * Schlüsseleinträge sind wählbar). Name und Alias sind Personendaten (ohne friendlyName ist der Alias der
 * Subject-Text) – `Debug` redigiert sie (Regel 9).
 */
export type KeystoreEntryView = { 
/**
 * Alias (Wert für `keystore.alias`)
 */
alias: string, 
/**
 * erster Common Name (CN) des Zertifikats
 */
subject_cn: string | null, 
/**
 * Ende der Gültigkeit des Zertifikats (RFC 3339 mit Offset, UTC)
 */
not_after: string | null, 
/**
 * Schlüsseleintrag (sonst nur ein Zertifikat, z. B. eine Zertifizierungsstelle)
 */
is_key_entry: boolean, };

/**
 * Interaktion während des Signierens.
 */
export type InteractionView = { "kind": "waiting", 
/**
 * Fortschritt
 */
progress: Progress, } | { "kind": "form", 
/**
 * Anfrage-ID (Antwort über [`UiEvent::SubmitForm`])
 */
request: number, 
/**
 * Maske
 */
form: NativeForm, } | { "kind": "secret", 
/**
 * Anfrage-ID
 */
request: number, 
/**
 * Abfrage
 */
prompt: SecretPrompt, 
/**
 * Erste Verwendung des Keystores: unter dem Passwortfeld „Passwort merken?“ anbieten (Antwort in
 * [`UiEvent::SubmitSecret::password_storage`]); nur für das Keystore-Passwort einer Datei aus den
 * Einstellungen, deren Speicherart noch nie gewählt wurde
 */
ask_storage: boolean, 
/**
 * Schlüsselbund bzw. sicherer Speicher verfügbar (sonst ist „Im Schlüsselbund“ nicht wählbar)
 */
keychain_available: boolean, } | { "kind": "notice", 
/**
 * Anfrage-ID
 */
request: number, 
/**
 * Meldung
 */
notice: Notice, };

/**
 * Ergebnis.
 */
export type OutputView = { 
/**
 * gespeichert unter
 */
path: string | null, 
/**
 * Speichern fehlgeschlagen/abgebrochen (Knopf „Speichern“, F-86)
 */
save_pending: boolean, 
/**
 * Rückfrage „Datei existiert, überschreiben?“ (F-84) – Antwort über [`UiEvent::ConfirmOverwrite`]
 */
ask_overwrite: string | null, 
/**
 * Speichern-Dialog öffnen (F-83/F-86) – Antwort über [`UiEvent::Save`]
 */
save_dialog: SaveDialogView | null, 
/**
 * Zusammenfassung für „Fertig“, sobald gespeichert (ADR 0004 §3.7; ab K1, bis dahin `None`)
 */
summary: SignedSummaryView | null, };

/**
 * Hauptleiste.
 */
export type MainBarView = { 
/**
 * Schritte
 */
steps: Array<MainBarStep>, 
/**
 * Einstellungen erreichbar
 */
settings_enabled: boolean, };

/**
 * Schritt-spezifische Sicht.
 */
export type StepView = { "step": "starting" } | { "step": "open", 
/**
 * Warteschlange
 */
queue: Array<QueueItemView>, } | { "step": "loading", 
/**
 * Warteschlange
 */
queue: Array<QueueItemView>, } | { "step": "warning", 
/**
 * Dokument
 */
doc: DocInfo, 
/**
 * Meldungs-IDs der Warnungen (`pades::Finding::code`)
 */
codes: Array<string>, } | { "step": "placeholder", 
/**
 * Dokument
 */
doc: DocInfo, 
/**
 * leere Felder
 */
fields: Array<FieldInfo>, 
/**
 * QR-Platzhalter
 */
qr: QrInfo | null, } | { "step": "position", 
/**
 * Dokument
 */
doc: DocInfo, 
/**
 * Seite
 */
page: number, 
/**
 * Block
 */
block: BlockView, 
/**
 * Position (PAGE_ABS, linke obere Ecke)
 */
placement: Placement, 
/**
 * virtuelle neue Seite
 */
new_page: boolean, 
/**
 * Parameter des Live-Region-Texts `ng-ui-position-announce`: `page`, `pages`, `left` (mm der
 * linken Kante von links), `top` (mm der Oberkante von unten – wie das Zahlenfeld Y), ganzzahlig
 */
announce: { [key in string]: string }, } | { "step": "backend", 
/**
 * Optionen
 */
options: Array<BackendOptionView>, } | { "step": "signing", 
/**
 * Interaktion
 */
interaction: InteractionView, 
/**
 * Warteschlange
 */
queue: Array<QueueItemView>, 
/**
 * Das Dokument verlässt mit dem laufenden Signiermittel das Gerät (`document_leaves_device`, z. B.
 * ID Austria): Kurzhinweis „wird an A-Trust übermittelt“ im Signierbereich (ADR 0003)
 */
document_leaves_device: boolean, 
/**
 * Fortschritt für Stepper und Stapel (ADR 0004 §3.6; ab K1, bis dahin Standardwerte)
 */
progress: SigningProgressView, } | { "step": "output", 
/**
 * Ergebnis
 */
result: OutputView, 
/**
 * nächstes Dokument
 */
next: DocInfo | null, 
/**
 * Zahl der wartenden Dokumente nach diesem (ADR 0004 E1 „Fertig“: „Nächstes Dokument“ mit Anzahl,
 * solange `batch_overview` aus ist; Nachtrag 1 im ADR)
 */
pending: number, } | { "step": "error", 
/**
 * Fehlercode (i18n-ID)
 */
code: string, 
/**
 * Parameter
 */
params: { [key in string]: string }, 
/**
 * Wiederholen möglich
 */
retry: boolean, 
/**
 * Herkunft, Zeitpunkt, Dokument (ADR 0004 §3.6; ab K1, bis dahin Standardwerte)
 */
context: ErrorContextView, } | { "step": "finished", 
/**
 * Exit-Code
 */
exit_code: number, } | { "step": "batch" };

/**
 * Einstellungen (Overlay über jedem Schritt).
 */
export type SettingsView = { 
/**
 * Einstellungen (Schema `pida-config`)
 */
settings: Settings, 
/**
 * Validierungsprobleme je Feld: Schlüssel = TOML-Punktpfad des Felds (`proxy.host`,
 * `output.folder`, `keystore.file` …), Wert = i18n-ID
 */
problems: { [key in string]: string }, 
/**
 * Schlüsselbund verfügbar
 */
keychain_available: boolean, 
/**
 * FIDO2 verfügbar
 */
fido2_available: boolean, 
/**
 * Keystore ist eingerichtet (aktiviert, die gewählte Datei existiert und ist PKCS#12); erst dann ist
 * er als Standard-Signiermittel wählbar
 */
keystore_ready: boolean, 
/**
 * Am Dateiinhalt erkanntes Format der gewählten Keystore-Datei (`None` = keine/fehlende Datei). Ersetzt
 * die Typauswahl aus PDF-Over (dort aus der Dateiendung); signiert wird nur mit PKCS#12.
 */
keystore_format: KeystoreFormatView | null, 
/**
 * Einträge der gewählten PKCS#12-Datei für die Alias-Auswahl (nach Alias sortiert), sobald das Passwort
 * bekannt ist: automatisch mit einem gemerkten Passwort (Speicherart), sonst nach
 * [`UiEvent::InspectKeystore`]. `None` = noch unbekannt (bzw. keine PKCS#12-Datei)
 */
keystore_entries: Array<KeystoreEntryView> | null, 
/**
 * Problem der letzten Prüfung über [`UiEvent::InspectKeystore`] (Meldungs-ID, z. B.
 * `keystore-wrong-password`); `None` = keines
 */
keystore_inspect_problem: string | null, 
/**
 * Einstellungen werden nicht gespeichert (Altdatei per `-c`, unlesbare Datei)
 */
read_only: boolean, 
/**
 * Revision der Blockvorschau (Cache-Busting für `blockPreviewUrl` im Einstellungsdialog);
 * der Kern baut die Vorschau beim Öffnen auch ohne Dokument (A4, Beispielzertifikat) wie PDF-Over
 */
preview_revision: number, 
/**
 * Revision des zuletzt übernommenen [`UiEvent::ApplySettings`] (ADR 0004 §3.11). Ändert eine Übernahme das
 * Signaturbild, steigt sie erst zusammen mit der neuen Vorschau (`preview_revision`) in derselben
 * Projektion
 */
applied_revision: number, 
/**
 * Vom Kern berechnete Beispiele (ADR 0004 §3.11): Schlüssel `output.postfix` → Ausgabename für die
 * Beispieldatei `Vertrag.pdf` (F-81)
 */
examples: { [key in string]: string }, 
/**
 * Per Richtlinie festgelegte Einstellungen: TOML-Punktpfad (bzw. ganzer Abschnitt wie `proxy`) → i18n-ID
 * des Grunds (`ng-recent-policy`, `ng-settings-policy`). Die UI zeigt diese Zeilen gesperrt mit Hinweis;
 * `ApplySettings` lehnt sie mit dem Grund als Problem ab. Nachtrag K3 (ADR 0004 §9, Vorschlag aus B3
 * `contract-draft.ts`): additiv, in TypeScript optional; der Kern liefert es immer (`Some`, ggf. leer).
 */
locked?: { [key in string]: string } | null, 
/**
 * Profil Amtssignatur ohne eigene Bildmarke (Arbeitskopie): Standard ist die neutrale Platzhalter-Bildmarke, nicht
 * das PIDA-Siegel (Backlog Nr. 10) – die Bildmarken-Zeile nennt das. Additiv.
 */
emblem_placeholder: boolean, };

/**
 * Gesamtzustand für die UI.
 */
export type ViewState = { 
/**
 * Schritt
 */
step: StepView, 
/**
 * Hauptleiste
 */
main_bar: MainBarView, 
/**
 * Einstellungen, falls geöffnet
 */
settings: SettingsView | null, 
/**
 * UI-Sprache
 */
language: Lang, 
/**
 * Einstellungen › Barrierefreiheit (für Kopfzeile, Inspektor und Ansagen, auch ohne Einstellungsansicht).
 * Additiv, in TypeScript optional; der Kern liefert es immer.
 */
accessibility?: AccessibilitySettings | null, 
/**
 * Hinweise ohne Rückfrage (älteste zuerst)
 */
notices: Array<NoticeView>, 
/**
 * Import-Angebot aus `~/.pdf-over`
 */
migration: MigrationOfferView | null, 
/**
 * abgeschlossene Dokumente dieser Sitzung
 */
done: Array<DoneItemView>, 
/**
 * Plattform (Desktop/Mobile-Bedienelemente)
 */
platform: PlatformView, 
/**
 * Angebot „Signatur fortsetzen“ (nur Mobile, nur im Schritt `open`)
 */
resume: ResumeOfferView | null, 
/**
 * Ergebnis des PKCS#12-Imports (früherer Mobile-Import-Assistent; seit ADR 0004 Nachtrag 11 immer `None`
 * auf Mobilgeräten)
 */
keystore_import: KeystoreImportView | null, 
/**
 * Eigener Datenschutztext der Organisation (Richtlinie `privacy_notice_text`) für Einstellungen ›
 * Datenschutz und die „Details“ des Kurzhinweises; `None` = Text aus PDF-Over
 * (`ng-ui-privacy-statement`, PIDA-Fassung des PDF-Over-Texts `config-DataProtectionStatement`, ADR 0003)
 */
privacy_text: string | null, 
/**
 * Aktuelles Dokument im Arbeitsbereich (Warnung, Positionieren, Signieren, Fertig; ADR 0004 §3.1,
 * §3.3); sonst `None`. Ab K1, bis dahin `None`.
 */
document: DocumentView | null, 
/**
 * Signiermittel für Konto-Chip, Menü und Stapel (ADR 0004 §3.4); ab K1 nach dem Start immer befüllt,
 * bis dahin leer
 */
signers: SignersView, 
/**
 * Stapel (nur mit `batch_overview`, ADR 0004 §3.8); ab K2, bis dahin `None`
 */
batch: BatchView | null, 
/**
 * „Zuletzt signiert“ (neueste zuerst, höchstens zehn); `None` = Funktion abgeschaltet (Einstellung,
 * Richtlinie) bzw. ohne Oberfläche (ADR 0004 §3.10)
 */
recent: Array<RecentItemView> | null, 
/**
 * Ersteinrichtung, nur beim ersten Start (fehlende Einstellungsdatei, Arbeitsbereich; ADR 0004 §3.12)
 */
first_run: FirstRunView | null, 
/**
 * Freigaben für Knöpfe, Befehlspalette und Tastenkürzel (ADR 0004 §3.13), aus dem Zustand berechnet
 */
actions: ActionsView, 
/**
 * Geparkte Arbeit („Zur Startseite“, ADR 0004 Nachtrag 10): gesetzt, solange die Startansicht ein offenes
 * Dokument bzw. einen Stapel verdeckt (Karte „In Bearbeitung“); dann sind `document` und `batch` `None`.
 * Additiv, in TypeScript optional; der Kern liefert es immer (`None` ohne geparkte Arbeit).
 */
parked?: ParkedWorkView | null, 
/**
 * iOS: Hinweis nach dem Empfang über „Teilen“ bzw. „Öffnen in“ (ADR 0004 Nachtrag 12) – die Dateien-App
 * übergibt von mehreren geteilten PDFs nur das erste; weitere für einen Stapel wählt man über „Weitere PDFs
 * hinzufügen …“ (Mehrfachauswahl). Nur, solange das empfangene Dokument allein im Arbeitsbereich offen ist,
 * und nicht mehr nach `dismiss_share_hint` (bis zum Beenden der App). Additiv, in TypeScript optional; sonst
 * `None` (Desktop, Android, Dateiauswahl).
 */
share_hint?: ShareHintView | null, 
/**
 * „Über PIDA“ (ADR 0004 Nachtrag 21): Version, Versionsdatum, Links, Update-Prüfung. Additiv, in TypeScript
 * optional; der Kern liefert es immer (`None` nur ohne Kern, z. B. im Fehlerschritt der Shell).
 */
about?: AboutView | null, 
/**
 * Startseite: Hinweis „PIDA x.y.z ist verfügbar“ mit Link auf die Release-Seite (Backlog Nr. 33). Nur am Desktop,
 * wenn eine neue Version bekannt ist (auch aus dem Zwischenspeicher), die automatische Prüfung an ist und der
 * Hinweis für diese Version nicht ausgeblendet wurde (`dismiss_update_hint`). Additiv, in TypeScript optional.
 */
update_hint?: UpdateHintView | null, };

/**
 * Antwort auf eine Maske (serde-Variante von `FormAnswer`; Passwort wird sofort in `Zeroizing`
 * überführt und nie geloggt).
 */
export type FormAnswerView = { "kind": "login", 
/**
 * Benutzername
 */
username: string, 
/**
 * Passwort
 */
password: string, 
/**
 * Merken
 */
remember: boolean, } | { "kind": "tan", 
/**
 * TAN
 */
tan: string, } | { "kind": "switch_to_sms" } | { "kind": "switch_to_fido2" } | { "kind": "start_fido2" } | { "kind": "continue" };

/**
 * Wie `open_files` die gewählten Dateien zum offenen Dokument nimmt (Kopfzeile „+ PDF“, Miniaturleiste;
 * ADR 0004 Nachtrag 9). Was daraus folgt, entscheidet der Kern nach dem Zustand.
 */
export type OpenFilesMode = "add" | "replace";

/**
 * UI-Ereignis.
 */
export type UiEvent = { "type": "open_files", 
/**
 * Pfade
 */
paths: Array<string>, 
/**
 * Wie die Dateien zum offenen Dokument kommen (Kopfzeile „+ PDF“, Miniaturleiste; ADR 0004 Nachtrag 9);
 * `None` = wie bisher (der Kern entscheidet nach dem Zustand: Auswahl öffnen, sonst an die
 * Warteschlange). Nur im Arbeitsbereich-Modus, sonst ohne Bedeutung. In TypeScript optional.
 */
mode?: OpenFilesMode | null, } | { "type": "answer_warning", 
/**
 * `true` = fortfahren
 */
proceed: boolean, } | { "type": "placeholder_answer", 
/**
 * Antwort
 */
answer: PlaceholderAnswer, 
/**
 * gewähltes Feld
 */
field_index: number | null, } | { "type": "move_block", 
/**
 * Seite
 */
page: number, 
/**
 * x
 */
x: number, 
/**
 * y
 */
y: number, } | { "type": "nudge_block", 
/**
 * dx in Schritten
 */
dx: number, 
/**
 * dy in Schritten
 */
dy: number, 
/**
 * fein (Shift)
 */
fine: boolean, 
/**
 * grob (Strg/⌘)
 */
coarse: boolean, } | { "type": "change_page", 
/**
 * Seite
 */
page: number, } | { "type": "toggle_new_page" } | { "type": "auto_place" } | { "type": "confirm_position" } | { "type": "choose_invisible" } | { "type": "choose_backend", 
/**
 * Backend
 */
backend: BackendId, } | { "type": "submit_form", 
/**
 * Anfrage-ID
 */
request: number, 
/**
 * Antwort
 */
answer: FormAnswerView, } | { "type": "submit_secret", 
/**
 * Anfrage-ID
 */
request: number, 
/**
 * Passwort
 */
secret: string, 
/**
 * Antwort auf „Passwort merken?“ ([`InteractionView::Secret::ask_storage`]): wird als
 * `keystore.password_storage` gespeichert und gilt schon für dieses Passwort; danach fragt PIDA nicht
 * mehr. Ohne Rückfrage ohne Wirkung
 */
password_storage?: PasswordStorage | null, } | { "type": "answer_notice", 
/**
 * Anfrage-ID
 */
request: number, 
/**
 * Antwort
 */
answer: NoticeAnswer, } | { "type": "open_signature_data", 
/**
 * Anfrage-ID der offenen Maske
 */
request: number, } | { "type": "cancel" } | { "type": "retry" } | { "type": "save", 
/**
 * Ziel (`None` = Dialog abgebrochen)
 */
path: string | null, } | { "type": "confirm_overwrite", 
/**
 * ja
 */
overwrite: boolean, } | { "type": "open_result" } | { "type": "share_result" } | { "type": "export_result" } | { "type": "navigate", 
/**
 * Schritt
 */
step: Step, } | { "type": "open_settings" } | { "type": "save_settings", 
/**
 * neue Einstellungen
 */
settings: Settings, } | { "type": "close_settings" } | { "type": "reset_settings" } | { "type": "backgrounded" } | { "type": "foregrounded" } | { "type": "resume_signature" } | { "type": "discard_resume" } | { "type": "import_keystore", 
/**
 * Datei (von der Shell bereits in die Sandbox kopiert, `import_file`)
 */
path: string, 
/**
 * Passwort (nur RAM, sofort in `Zeroizing`; nie geloggt)
 */
password: string | null, 
/**
 * Passwort im sicheren Speicher merken
 */
remember: boolean, } | { "type": "dismiss_keystore_import" } | { "type": "quit" } | { "type": "dismiss_notice", 
/**
 * Kennung ([`NoticeView::id`])
 */
id: number, } | { "type": "accept_migration", 
/**
 * Klartext-Passwörter in den Schlüsselbund übernehmen (Zustimmung)
 */
store_secrets: boolean, } | { "type": "decline_migration" } | { "type": "set_invisible", 
/**
 * unsichtbar
 */
invisible: boolean, } | { "type": "use_target", 
/**
 * Ziel
 */
target: PlacementTarget, } | { "type": "inspect_pages", 
/**
 * Seiten (0-basiert)
 */
pages: Array<number>, } | { "type": "select_signer", 
/**
 * Signiermittel (`None` = keine Auswahl)
 */
backend: BackendId | null, 
/**
 * als Standard speichern
 */
make_default: boolean, } | { "type": "sign_with", 
/**
 * Signiermittel
 */
backend: BackendId, } | { "type": "show_batch" } | { "type": "open_batch_item", 
/**
 * Dokument
 */
doc: DocId, } | { "type": "back_to_batch" } | { "type": "remove_from_batch", 
/**
 * Dokument
 */
doc: DocId, } | { "type": "start_batch", 
/**
 * Signiermittel
 */
backend: BackendId, 
/**
 * genau diese Dokumente (später hinzugefügte gehören nicht zum Lauf)
 */
docs: Array<DocId>, 
/**
 * nur bei `BatchOutputView::needs_folder` (Ordnerwahl einmal vor dem Start)
 */
output_dir: string | null, 
/**
 * Umgang mit vorhandenen Zieldateien
 */
on_existing: ExistingPolicy, } | { "type": "skip_batch_document" } | { "type": "abort_batch" } | { "type": "remove_recent", 
/**
 * Kennung
 */
id: number, } | { "type": "clear_recent" } | { "type": "apply_settings", 
/**
 * geänderte Felder (Pfad → Wert)
 */
changes: Record<string, unknown>, 
/**
 * Revision der UI (zurück in `SettingsView::applied_revision`)
 */
revision: number, } | { "type": "set_ui_language", 
/**
 * Sprache (`None` = System)
 */
language: Lang | null, } | { "type": "set_accessibility", 
/**
 * neue Werte
 */
accessibility: AccessibilitySettings, } | { "type": "export_qr_placeholder", 
/**
 * Inhalt
 */
spec: QrPlaceholderSpec, 
/**
 * Format
 */
format: QrPlaceholderFormat, 
/**
 * Ziel (`None` = teilen bzw. herunterladen)
 */
path: string | null, } | { "type": "update_first_run", 
/**
 * Standard-Signiermittel (`None` = „Jedes Mal fragen“)
 */
default_backend: BackendChoice | null, 
/**
 * Profil des Signaturblocks
 */
profile: SignatureProfile, 
/**
 * Sprache des Signaturblocks (`None` = System)
 */
block_language: Lang | null, } | { "type": "finish_first_run" } | { "type": "offer_migration" } | { "type": "inspect_keystore", 
/**
 * Passwort (nur RAM, sofort in `Zeroizing`; nie geloggt); `None` = ein bekanntes (gemerktes)
 * Passwort verwenden
 */
password: string | null, 
/**
 * Antwort auf „Passwort merken?“ (nur solange `keystore.storage_chosen` falsch ist, sonst ohne
 * Wirkung)
 */
password_storage?: PasswordStorage | null, } | { "type": "unlock_placement" } | { "type": "add_block", 
/**
 * Seite (0-basiert), auf der der Block erscheinen soll
 */
page?: number | null, } | { "type": "select_block", 
/**
 * Stelle in der Reihenfolge der Blöcke (0-basiert)
 */
index: number, } | { "type": "remove_block", 
/**
 * Stelle in der Reihenfolge der Blöcke (0-basiert)
 */
index: number, } | { "type": "fill_targets" } | { "type": "set_batch_auto_position", 
/**
 * ein bzw. aus
 */
enabled: boolean, 
/**
 * von Hand platzierte Einträge ebenfalls automatisch platzieren
 */
replace_manual: boolean, } | { "type": "go_home" } | { "type": "resume_parked" } | { "type": "discard_parked" } | { "type": "postpone_first_run" } | { "type": "files_received", 
/**
 * Pfade (Kopien in der Sandbox)
 */
paths: Array<string>, } | { "type": "dismiss_share_hint" } | { "type": "open_recent", 
/**
 * Kennung ([`RecentItemView::id`])
 */
id: number, } | { "type": "reveal_recent", 
/**
 * Kennung ([`RecentItemView::id`])
 */
id: number, } | { "type": "share_recent", 
/**
 * Kennung ([`RecentItemView::id`])
 */
id: number, } | { "type": "sign_recent", 
/**
 * Kennung ([`RecentItemView::id`])
 */
id: number, 
/**
 * wie [`UiEvent::OpenFiles`] (`None` = der Kern entscheidet nach dem Zustand). In TypeScript optional.
 */
mode?: OpenFilesMode | null, } | { "type": "check_for_updates", 
/**
 * ausdrücklicher Klick („Jetzt prüfen“, „Erneut prüfen“)
 */
explicit: boolean, } | { "type": "set_block_language", 
/**
 * Sprache
 */
language: Lang, } | { "type": "dismiss_update_hint" };

/**
 * Rechteck in pt (`PAGE_ABS`: Ursprung links unten, y nach oben; ADR 0004 §3.0 Regel 3).
 */
export type RectPt = { 
/**
 * linke Kante
 */
left: number, 
/**
 * Unterkante
 */
bottom: number, 
/**
 * rechte Kante
 */
right: number, 
/**
 * Oberkante
 */
top: number, };

/**
 * Kurzverweis auf ein Dokument (§3.1).
 */
export type DocRefView = { 
/**
 * Kennung
 */
id: DocId, 
/**
 * Name
 */
name: string, };

/**
 * Phase des Dokuments im Arbeitsbereich (Tabelle §3.1).
 */
export type DocumentPhase = "warning" | "editing" | "signing" | "done";

/**
 * Ziel der Platzierung (§3.3).
 */
export type PlacementTarget = { "kind": "free" } | { "kind": "field", 
/**
 * Index in [`DocumentView::fields`]
 */
index: number, } | { "kind": "qr" };

/**
 * Leeres, angebotenes Signaturfeld („Hier signieren“; §3.3, §8 F2: nur angeboten, nie automatisch).
 */
export type FieldTargetView = { 
/**
 * Index (für [`PlacementTarget::Field`])
 */
index: number, 
/**
 * qualifizierter Feldname
 */
name: string, 
/**
 * Seite des Widgets
 */
page: number | null, 
/**
 * Widget-Rechteck (`PAGE_ABS`)
 */
rect: RectPt | null, 
/**
 * wählbar (`false` bei gesperrten Feldern: `/Lock`, FieldMDP)
 */
selectable: boolean, 
/**
 * als Ziel angeboten (ADR 0004 Nachtrag 16, `pida_placement::field_suitable`): Das Feld nimmt den Block in
 * aktueller Größe plausibel auf (höchstens auf ein Fünftel verkleinert, Form nicht mehr als achtfach gestreckt,
 * höchstens die halbe Seite). Nur dann rastet der Block beim Ziehen ein, zeigt die Oberfläche „Hier signieren“
 * und nennt die Hinweisleiste das Feld; ein seitengroßes, winziges oder 3 pt schmales Feld wird nicht angeboten.
 */
suitable: boolean, 
/**
 * Rechteck, in dem der Block in **diesem** Feld stünde (`PAGE_ABS`): proportional, höchstens in natürlicher
 * Größe, mittig – genau so, wie ihn der Kern schreibt (KR-P10); nach `use_target` gleich `sticker`. `None`
 * ohne Seite bzw. Rechteck.
 */
block: RectPt | null, };

/**
 * QR-Platzhalter (nur mit `placeholder.search` und `kind = marker`; §3.3).
 */
export type QrTargetView = { 
/**
 * Seite
 */
page: number, 
/**
 * Rechteck des Platzhalters (`PAGE_ABS`)
 */
rect: RectPt, 
/**
 * id des Platzhalters
 */
id: string | null, };

/**
 * Status einer vorhandenen Signatur (aus `pades::PriorStatus`; §3.3).
 */
export type PriorStatusView = "valid" | "invalid" | "not_verifiable";

/**
 * Vorhandene Signatur im Inspektor (§3.3; „Rolle“ ist nicht ermittelbar und entfällt).
 */
export type ExistingSignatureView = { 
/**
 * Feldname
 */
field_name: string, 
/**
 * CN des Signerzertifikats
 */
signer: string | null, 
/**
 * RFC 3339 (`signing-time` des CMS, kein geprüfter Zeitstempel)
 */
signing_time: string | null, 
/**
 * Status
 */
status: PriorStatusView, 
/**
 * Seite des Signaturbilds
 */
page: number | null, 
/**
 * Rechteck des Signaturbilds (`None` = unsichtbar)
 */
rect: RectPt | null, };

/**
 * Einschränkungen des Dokuments mit Begründung (§3.3).
 */
export type DocConstraintsView = { 
/**
 * „Auf neuer Seite“ erlaubt (`false` bei bereits signierten Dokumenten)
 */
new_page_allowed: boolean, 
/**
 * sichtbarer Block erlaubt (`false` beim unsichtbaren Profil)
 */
visible_allowed: boolean, 
/**
 * neues sichtbares Feld erlaubt (`false` bei DocMDP-Zertifizierung: nur vorhandene Felder/unsichtbar)
 */
new_visible_field_allowed: boolean, 
/**
 * Begründungen (i18n-IDs): `ng-constraint-signed`, `ng-constraint-certified`,
 * `ng-constraint-invisible-profile`
 */
reasons: Array<string>, };

/**
 * Achse einer Einrastlinie (§3.3).
 */
export type GuideAxis = "x" | "y";

/**
 * Art einer Einrastlinie (§3.3): `X`-Linien gelten für die linke (`ContentLeft`, `MarginLeft`) bzw.
 * rechte Blockkante (`ContentRight`, `MarginRight`) bzw. die Blockmitte (`CenterX`); `Y`-Linien für die
 * Ober- (`ContentTop`, `BelowContent`, `MarginTop`) bzw. Unterkante (`ContentBottom`, `MarginBottom`)
 * bzw. Mitte (`CenterY`).
 */
export type GuideKind = "content_left" | "content_right" | "content_top" | "content_bottom" | "center_x" | "center_y" | "below_content" | "margin_left" | "margin_right" | "margin_top" | "margin_bottom";

/**
 * Einrastlinie (§3.3).
 */
export type GuideView = { 
/**
 * Achse
 */
axis: GuideAxis, 
/**
 * Lage in pt
 */
at: number, 
/**
 * Art
 */
kind: GuideKind, };

/**
 * Layout einer Seite (§3.3): Inhaltsrahmen, freie Flächen, Einrastlinien.
 */
export type PageLayoutView = { 
/**
 * Seite
 */
page: number, 
/**
 * Begrenzungsrahmen des Inhalts (`None` = leere Seite)
 */
content: RectPt | null, 
/**
 * freie Rechtecke, in die der Block in aktueller Größe passt (6 pt Abstand zum Inhalt), absteigend
 * nach Eignung, höchstens 16
 */
free: Array<RectPt>, 
/**
 * Einrastlinien
 */
guides: Array<GuideView>, };

/**
 * Ergebnis der Live-Prüfung der Platzierung (§3.3).
 */
export type PlacementCheckView = { 
/**
 * Revision der Platzierung, auf die sich die Prüfung bezieht ([`DocumentView::revision`])
 */
for_revision: number, 
/**
 * überdeckt Inhalt; `None` = unbekannt (kein Renderer, Raster noch nicht berechnet, neue Seite)
 */
overlaps_content: boolean | null, 
/**
 * überdeckt ein Formularfeld
 */
overlaps_widget: boolean, 
/**
 * vom Kern an die Seite geklemmt
 */
clamped: boolean, 
/**
 * Ziel QR-Platzhalter: der Block, der dort signiert wird (Breite KR-F14, Höhe aus dem Layout bei dieser
 * Breite), bedeckt das Platzhalterbild nicht ganz – es schaut unten bzw. rechts hervor (Entscheidung
 * 20.10.2026, Backlog Nr. 32: Block bleibt PDF-AS-gleich, die Oberfläche weist inline darauf hin). Additiv.
 */
placeholder_uncovered: boolean, 
/**
 * Mehrfachsignatur: der ausgewählte Block überdeckt einen anderen Signaturblock desselben Dokuments. Additiv.
 */
overlaps_block: boolean, };

/**
 * Ein Signaturblock der Mehrfachsignatur ([`BlocksView::items`]).
 */
export type BlockSlotView = { 
/**
 * Stelle in der Reihenfolge der Blöcke (0-basiert) = Reihenfolge der Signaturen
 */
index: number, 
/**
 * der ausgewählte Block (seine Lage steht auch in [`DocumentView::placement`] und [`DocumentView::sticker`])
 */
active: boolean, 
/**
 * Seite des Blocks (0-basiert)
 */
page: number, 
/**
 * Rechteck des Blocks auf `page` (`None` = keine Vorschau)
 */
rect: RectPt | null, 
/**
 * Ziel
 */
target: PlacementTarget, };

/**
 * Mehrfachsignatur: alle Signaturblöcke des Dokuments – je Block eine eigene Signatur, in dieser Reihenfolge
 * ([`DocumentView::blocks`]).
 */
export type BlocksView = { 
/**
 * alle Blöcke in ihrer Reihenfolge (mindestens einer; unsichtbar: nur der ausgewählte)
 */
items: Array<BlockSlotView>, 
/**
 * Höchstzahl der Blöcke je Dokument
 */
limit: number, 
/**
 * „Weiteren Block hinzufügen“ möglich (nicht unsichtbar, nicht auf neuer Seite, nicht gesperrt, Höchstzahl
 * nicht erreicht, neues sichtbares Feld erlaubt, Vorschau vorhanden)
 */
can_add: boolean, 
/**
 * Anzahl der Ziele, die „Alle Platzhalter belegen“ noch belegen würde (geeignete leere Signaturfelder und
 * der QR-Platzhalter, die noch kein Block benutzt)
 */
free_targets: number, };

/**
 * PDF/A-Kennung eines Dokuments (XMP `pdfaid:part`, `pdfaid:conformance`).
 */
export type PdfaIdView = { 
/**
 * Teil (1, 2, 3, 4)
 */
part: number, 
/**
 * Konformitätsstufe (A, B, U …), falls angegeben
 */
conformance: string | null, };

/**
 * Wirksame PDF/A-Entscheidung beim Signieren (Einstellungen › Dokument „PDF/A-kompatibel signieren“).
 */
export type PdfaDecision = "none" | "detected" | "forced";

/**
 * Erkanntes und automatisch Entschiedenes zum Dokument (Nachtrag K3, Einstellungen › Dokument).
 */
export type DocumentDetectionView = { 
/**
 * PDF/A-Kennung des Dokuments (`None` = keine)
 */
pdfa: PdfaIdView | null, 
/**
 * wirksame PDF/A-Entscheidung
 */
pdfa_decision: PdfaDecision, 
/**
 * Lage stammt aus der Automatik (Startlage nach der Analyse bzw. Nachführung nach geänderten
 * Einstellungen). Zusammen mit [`DocumentView::target`]: frei = „automatisch platziert“, `qr` =
 * „QR-Platzhalter gefunden – Signatur dort platziert“, `field` = „Signaturfeld gefunden – Signatur im
 * Feld platziert“ (Platzhaltersuche „Leere Signaturfelder“). `false`, sobald die Nutzerin selbst platziert
 * (Ziehen, Tastatur, Seite, „Ins Feld setzen“, „Frei platzieren“/„Freie Fläche suchen“).
 */
auto_placed: boolean, };

/**
 * Aktuelles Dokument im Arbeitsbereich (§3.3) – einzige Inhaltsquelle der neuen Oberfläche;
 * `StepView::Position` bleibt Phasenmarker.
 */
export type DocumentView = { 
/**
 * Dokument (id, name, page_count, page_sizes, signed)
 */
doc: DocInfo, 
/**
 * Größe der Datei in Bytes
 */
size_bytes: number, 
/**
 * Phase
 */
phase: DocumentPhase, 
/**
 * Revision der Platzierung (steigt bei jeder Änderung)
 */
revision: number, 
/**
 * Blockgröße und Vorschau-Revision
 */
block: BlockView, 
/**
 * maßgebliche Lage (`PAGE_ABS`, linke obere Ecke)
 */
placement: Placement, 
/**
 * Herkunft der Lage
 */
origin: PlacementOrigin, 
/**
 * Ziel
 */
target: PlacementTarget, 
/**
 * unsichtbar signieren (nur dieses Dokument)
 */
invisible: boolean, 
/**
 * virtuelle neue Seite
 */
new_page: boolean, 
/**
 * tatsächliches Rechteck des Blocks auf `placement.page_index` (beim QR-Platzhalter dessen Rechteck, im
 * Signaturfeld `FieldTargetView::block` – proportional und mittig im Feld, Nachtrag 16);
 * `None` = unsichtbar bzw. keine Vorschau
 */
sticker: RectPt | null, 
/**
 * leere, nicht gesperrte Signaturfelder – im Arbeitsbereich immer angeboten; automatisch benutzt wird nur
 * das erste und nur mit Platzhaltersuche „Leere Signaturfelder“ (Einstellungen › Dokument, Nachtrag 5)
 */
fields: Array<FieldTargetView>, 
/**
 * QR-Platzhalter (nur mit Platzhaltersuche)
 */
qr: QrTargetView | null, 
/**
 * vorhandene Signaturen
 */
existing: Array<ExistingSignatureView>, 
/**
 * Einschränkungen
 */
constraints: DocConstraintsView, 
/**
 * Layout der zuletzt angefragten Seiten (`InspectPages`, ≤ 8) und der Seite des Stickers, aufsteigend
 * nach `page`, höchstens 9; Seiten ohne fertiges Raster fehlen
 */
layout: Array<PageLayoutView>, 
/**
 * Live-Prüfung
 */
check: PlacementCheckView, 
/**
 * Live-Region: `page`, `pages`, `left`, `top` (wie `StepView::Position`) und `bottom` (mm der
 * Unterkante von unten)
 */
announce: { [key in string]: string }, 
/**
 * Was der Kern erkannt bzw. automatisch entschieden hat (Einstellungen › Dokument: PDF/A,
 * Platzhaltersuche) – für die dezenten Hinweise im Arbeitsbereich. Nachtrag K3 (ADR 0004 §9, Vorschlag):
 * additiv; in TypeScript optional (`detection?: … | null`), damit bestehende UI-Literale gültig bleiben.
 * Der Kern liefert es im Arbeitsbereich immer (`Some`).
 */
detection?: DocumentDetectionView | null, 
/**
 * Platzierung gesperrt durch die Einstellung „Automatische Positionierung“ (`signature.auto_position`
 * bzw. `-a`; ADR 0004 Nachtrag 8): automatisch platziert und bereit zum Signieren – der Kern ignoriert
 * `move_block`, `nudge_block`, `change_page`, `use_target` und `auto_place`; `set_invisible`,
 * `toggle_new_page` und `sign_with` wirken. `unlock_placement` („Diesmal selbst platzieren“) hebt die
 * Sperre für dieses Dokument auf. Nie für Einträge der Stapelübersicht.
 */
placement_locked: boolean, 
/**
 * Sichtbare Größe einer neu angehängten Seite (pt, wie `DocInfo::page_sizes`): MediaBox und `/Rotate` der
 * letzten Seite ohne deren CropBox (`pida_placement::new_page_geometry`, wie `new PDPage(lastPage.getMediaBox())`
 * in PDF-AS). Die Oberfläche zeigt die virtuelle neue Seite in dieser Größe (Nachtrag 16). Additiv; in
 * TypeScript optional, `None` ohne Seiten.
 */
new_page_size?: [number, number] | null, 
/**
 * Mehrfachsignatur: alle Signaturblöcke (je Block eine eigene Signatur). Additiv; in TypeScript optional.
 * Der Kern liefert es im Arbeitsbereich immer (`Some`, mindestens ein Block).
 */
blocks?: BlocksView | null, };

/**
 * Signiermittel für Konto-Chip, Menü und Stapel (§3.4).
 */
export type SignersView = { 
/**
 * Optionen (ohne reservierte Backends; auf iOS/Android nur ID Austria, ADR 0004 Nachtrag 11)
 */
options: Array<SignerView>, 
/**
 * Auswahl des Konto-Chips (Start = Standard, sofern verfügbar; bei Richtlinie `forced_backend` fest);
 * mit „Jedes Mal fragen“ nur bis zum Ende des Dokuments bzw. Stapellaufs (§8 F12)
 */
selected: BackendId | null, 
/**
 * Standard aus den Einstellungen (`None` = „Jedes Mal fragen“); auf iOS/Android immer ID Austria (das
 * einzige Signiermittel, sofern die Richtlinie es erlaubt)
 */
default: BackendId | null, 
/**
 * Signiermittel-Auswahl anbieten (ADR 0004 Nachtrag 20). `false`, wenn höchstens ein Signiermittel in Frage
 * kommt: auf iOS/Android immer (nur ID Austria, Nachtrag 11), am Desktop bei einer Richtlinie
 * (`forced_backend` bzw. `allowed_backends` mit einem Signiermittel) oder fest vorgegebenen Backends mit
 * einem. Von der Richtlinie ausgeschlossene Optionen zählen nicht; ein nicht eingerichteter Keystore bzw.
 * eine nicht laufende Chipkarten-Software zählt (wählbar, sobald eingerichtet bzw. gestartet). Ohne Auswahl
 * ist [`SignersView::selected`] das eine Signiermittel (auch bei „Jedes Mal fragen“ – es gibt nichts zu
 * fragen) und „Signieren“ signiert damit; die Oberfläche zeigt dann keinen Konto-Chip mit Menü, keinen
 * Abschnitt „Signieren mit“ und keine Wahl in Stapel, Ersteinrichtung und Signierkarte. Additiv: fehlt das
 * Feld (ältere Daten), gilt `true` wie bisher; ohne Optionen ([`Default`]) `false`.
 */
choice_offered: boolean, };

/**
 * Ein Signiermittel im Konto-Chip (§3.4).
 */
export type SignerView = { 
/**
 * Backend
 */
backend: BackendId, 
/**
 * verfügbar
 */
available: boolean, 
/**
 * Kontoanzeige: ID Austria „+43 660 ···123“ (nur bei gemerktem Benutzer, im Kern maskiert),
 * Keystore „max.p12 · alias“; lokale BKU `None`
 */
account: string | null, 
/**
 * Fähigkeiten (u. a. `document_leaves_device`, Faktoren, Qualifikation)
 */
capabilities: Capabilities | null, 
/**
 * Hinweise (i18n-IDs) wie [`BackendOptionView::notes`]
 */
notes: Array<string>, 
/**
 * Grund, falls nicht verfügbar (i18n-ID): `ng-signer-localbku-not-running`,
 * `ng-signer-keystore-missing`, `ng-signer-policy` (der frühere Mobile-Grund „PKCS#12-Datei importieren“ entfiel
 * mit ADR 0004 Nachtrag 11 – auf Mobilgeräten gibt es keinen Keystore mehr)
 */
unavailable_reason: string | null, 
/**
 * Ablauf eines Stapellaufs mit diesem Signiermittel (aus den Fähigkeiten; `None` ohne Fähigkeiten, z. B.
 * Keystore nicht eingerichtet): Wortlaut von Startfläche, Stapelübersicht, Startdialog und Stapelkarte (ADR 0004
 * Nachtrag 12). Additiv, in TypeScript optional; der Kern liefert es immer.
 */
batch_flow?: BatchFlowView | null, };

/**
 * Phase der Signierkarte (§3.6).
 */
export type SigningStage = "connecting" | "login" | "certificate" | "preparing" | "confirming" | "finishing" | "saving" | "paused";

/**
 * Bestätigungsweg (§3.6).
 */
export type ConfirmMethod = "app" | "qr" | "sms" | "fido2" | "password" | "card";

/**
 * Herkunft des Signaturzertifikats in dieser Sitzung (§3.6).
 */
export type CertificateSource = "read" | "cache" | "session" | "not_needed";

/**
 * Wirkungsbereich einer Stapelpause (§3.8).
 */
export type PauseScope = "document" | "session";

/**
 * Pause eines Stapellaufs mit ausdrücklicher Entscheidung (§3.8).
 */
export type BatchPauseView = { 
/**
 * Dokument (`None` = Sitzungsfehler vor dem ersten Dokument)
 */
doc: DocRefView | null, 
/**
 * Fehlercode (i18n-ID)
 */
code: string, 
/**
 * Parameter
 */
params: { [key in string]: string }, 
/**
 * Wirkungsbereich
 */
scope: PauseScope, 
/**
 * „Erneut versuchen“ möglich
 */
can_retry: boolean, 
/**
 * „Erneut versuchen“ öffnet eine neue Sitzung (ggf. neue Anmeldung)
 */
new_session: boolean, };

/**
 * Fortschritt der Signierkarte (Stepper „Anmelden → Bestätigen k/n → Signiert“; §3.6).
 */
export type SigningProgressView = { 
/**
 * Signiermittel
 */
backend: BackendId | null, 
/**
 * Phase
 */
stage: SigningStage, 
/**
 * Bestätigungsweg der offenen Maske bzw. des Backends
 */
method: ConfirmMethod | null, 
/**
 * Dokument der laufenden Bestätigung
 */
doc: DocRefView | null, 
/**
 * 1-basiert; Einzeldokument 1
 */
index: number, 
/**
 * Einzeldokument 1
 */
total: number, 
/**
 * Anmeldung dieser Sitzung erfolgreich; wird wieder `false`, sobald das Backend erneut die
 * Login-Maske zeigt
 */
login_done: boolean, 
/**
 * abgeschlossene Bestätigungen
 */
confirmations_done: number, 
/**
 * Bestätigungen gesamt (je Dokument eine)
 */
confirmations_total: number, 
/**
 * Herkunft des Zertifikats
 */
certificate: CertificateSource, 
/**
 * Stapellauf
 */
batch: boolean, 
/**
 * Stapelpause; bei Einzeldokumenten immer `None` (dort `StepView::Error`)
 */
pause: BatchPauseView | null, };

/**
 * Herkunft eines Fehlers (§3.6).
 */
export type ErrorOrigin = "load" | "analysis" | "prepare" | "signing" | "output" | "config" | "other";

/**
 * Kontext eines Fehlers für „Technische Details“ (§3.6).
 */
export type ErrorContextView = { 
/**
 * Herkunft
 */
origin: ErrorOrigin, 
/**
 * RFC 3339, lokale Zeit mit Offset
 */
occurred_at: string | null, 
/**
 * Signiermittel
 */
backend: BackendId | null, 
/**
 * Dokument
 */
doc: DocRefView | null, };

/**
 * Zusammenfassung „Fertig“ (§3.7). Wortlaut nach §8 F3: nie „gültig“, sondern „Signatur erstellt und
 * geprüft (Selbstprüfung)“, Qualifikation „laut Zertifikat“.
 */
export type SignedSummaryView = { 
/**
 * Dateiname der Ausgabe
 */
file_name: string, 
/**
 * Desktop: Ordner der Ausgabe; Mobile: `None` („in den Dokumenten dieser App“)
 */
folder_display: string | null, 
/**
 * CN des verwendeten Signaturzertifikats
 */
signer: string | null, 
/**
 * RFC 3339 wie `/M`
 */
signing_time: string, 
/**
 * Signiermittel
 */
backend: BackendId, 
/**
 * laut qcStatements des Signerzertifikats (QcCompliance und QcSSCD → `Qes`, sonst `AdES`; nicht gegen die
 * Vertrauensliste geprüft)
 */
qualification: Qualification, 
/**
 * Selbstprüfung I1–I6 bestanden (ohne Bestehen wird nie gespeichert – dient der Anzeige)
 */
self_check: boolean, 
/**
 * Seite des sichtbaren Blocks (`None` = unsichtbar)
 */
page: number | null, 
/**
 * Mehrfachsignatur: Anzahl der in diesem Vorgang erstellten Signaturen (je Block eine; sonst 1). Additiv; in
 * TypeScript optional, der Kern liefert es immer.
 */
signatures?: number | null, };

/**
 * Status eines Stapeleintrags (§3.8).
 */
export type BatchItemStatusView = { "state": "analyzing" } | { "state": "ready" } | { "state": "positioned" } | { "state": "rejected", 
/**
 * Code
 */
code: string, } | { "state": "needs_review", 
/**
 * Codes der Warnungen
 */
codes: Array<string>, } | { "state": "queued" } | { "state": "active", 
/**
 * Phase
 */
stage: SigningStage, } | { "state": "signed", 
/**
 * Ausgabe
 */
path: string, } | { "state": "failed", 
/**
 * Code
 */
code: string, 
/**
 * Parameter
 */
params: { [key in string]: string }, } | { "state": "skipped", 
/**
 * Ziel existierte
 */
existing: boolean, } | { "state": "cancelled" };

/**
 * Kurzfassung der Platzierung eines Stapeleintrags (§3.8).
 */
export type PositionSummaryView = { "kind": "auto" } | { "kind": "manual", 
/**
 * Seite
 */
page: number, } | { "kind": "field", 
/**
 * Feldname
 */
name: string, } | { "kind": "qr" } | { "kind": "invisible" };

/**
 * Eintrag der Stapelübersicht (§3.8).
 */
export type BatchItemView = { 
/**
 * Dokument
 */
doc: DocId, 
/**
 * Name
 */
name: string, 
/**
 * Seitenzahl (nach der Voranalyse)
 */
page_count: number | null, 
/**
 * Größe in Bytes
 */
size_bytes: number | null, 
/**
 * Status
 */
status: BatchItemStatusView, 
/**
 * Platzierung
 */
position: PositionSummaryView, 
/**
 * Ausgabedateiname nach F-81
 */
target_name: string | null, 
/**
 * Ziel existiert bereits (Vorabprüfung)
 */
target_exists: boolean, 
/**
 * Miniatur über `pida-render://thumb/<doc>` verfügbar
 */
thumbnail: boolean, 
/**
 * Mehrfachsignatur: Anzahl der Signaturen, die das Dokument im Lauf erhält (je von Hand platziertem Block
 * eine; sonst 1). Additiv; in TypeScript optional, der Kern liefert es immer.
 */
signatures?: number | null, };

/**
 * Ausgabeart des Stapels (§3.8).
 */
export type OutputModeView = "next_to_source" | "folder" | "ask" | "app_documents";

/**
 * Ausgabe des Stapels (§3.8).
 */
export type BatchOutputView = { 
/**
 * Art
 */
mode: OutputModeView, 
/**
 * Ordner zur Anzeige
 */
folder_display: string | null, 
/**
 * `StartBatch` braucht `output_dir` („jedes Mal fragen“ oder kein vorhandener Ordner)
 */
needs_folder: boolean, };

/**
 * Schätzung „n Dokumente · 1 Anmeldung · n Bestätigungen“ für das gewählte Signiermittel (§3.8).
 */
export type BatchEstimateView = { 
/**
 * Signiermittel
 */
backend: BackendId, 
/**
 * Dokumente
 */
documents: number, 
/**
 * erwartete Anmeldungen (0 oder 1; eine Erwartung, keine Zusage)
 */
logins: number, 
/**
 * Bestätigungen (= `documents` bei `max_batch == 1`)
 */
confirmations: number, 
/**
 * ggf. eine weitere Bestätigung für das Zertifikat
 */
certificate_confirmation_possible: boolean, 
/**
 * ab 20 Dokumenten mit ID Austria: Hinweis `ng-batch-large` (§8 F9)
 */
large: boolean, };

/**
 * Zusammenfassung über **alle** Einträge der Übersicht nach dem letzten Lauf (§3.8).
 */
export type BatchSummaryView = { 
/**
 * Einträge gesamt
 */
total: number, 
/**
 * signiert (auch vorher einzeln signierte)
 */
signed: number, 
/**
 * abgelehnt
 */
rejected: number, 
/**
 * fehlgeschlagen
 */
failed: number, 
/**
 * übersprungen
 */
skipped: number, 
/**
 * abgebrochen
 */
cancelled: number, 
/**
 * nicht begonnen
 */
not_started: number, 
/**
 * Speicherort zur Anzeige
 */
folder_display: string | null, 
/**
 * der letzte Lauf wurde abgebrochen („Stapel abbrechen“, Esc) – Titel der Zusammenfassung; `cancelled`
 * allein genügt nicht (Abbruch vor dem ersten Dokument bzw. Einträge aus einem früheren Lauf;
 * Nachtrag 4 im ADR 0004)
 */
aborted: boolean, };

/**
 * Stapel (§3.8; nur mit `batch_overview`).
 */
export type BatchView = { 
/**
 * Einträge
 */
items: Array<BatchItemView>, 
/**
 * gerade im Arbeitsbereich geöffnetes Dokument (`OpenBatchItem`)
 */
open_item: DocId | null, 
/**
 * Lauf aktiv (Details in `StepView::Signing::progress`)
 */
running: boolean, 
/**
 * nach dem letzten Lauf
 */
summary: BatchSummaryView | null, 
/**
 * Ausgabe
 */
output: BatchOutputView, 
/**
 * für `signers.selected` (`None` = kein Signiermittel gewählt)
 */
estimate: BatchEstimateView | null, 
/**
 * Einstellung „Automatische Positionierung“ an (`signature.auto_position` bzw. `-a`): jeder Stapel wird
 * automatisch positioniert – die Übersicht zeigt „Automatische Positionierung (laut Einstellung)“
 * (Nachtrag 8)
 */
auto_position_setting: boolean, 
/**
 * „Für diesen Stapel automatisch positionieren“ (`set_batch_auto_position`): nur für diesen Stapel bis zum
 * Ende bzw. Abbruch des Laufs bzw. bis der Stapel geschlossen wird; die Einstellung bleibt unverändert.
 * Mit automatischer Positionierung (Einstellung oder diese Option) gelten automatisch platzierte Einträge
 * als „Bereit“ (Nachtrag 8)
 */
auto_position_once: boolean, 
/**
 * noch nicht signierte, von Hand platzierte Einträge (eigener Plan): sie behalten beim Einschalten ihren
 * Plan, außer `set_batch_auto_position { replace_manual: true }` nach ausdrücklicher Bestätigung
 */
manual_plans: number, };

/**
 * Eintrag „Zuletzt signiert“ (§3.10; gespeichert werden nur Dateiname, Pfad und Zeitpunkt).
 */
export type RecentItemView = { 
/**
 * Kennung
 */
id: number, 
/**
 * Dateiname
 */
file_name: string, 
/**
 * Desktop: Ordner; Mobile: `None`
 */
folder_display: string | null, 
/**
 * RFC 3339, lokale Zeit mit Offset
 */
signed_at: string, 
/**
 * Datei noch vorhanden (beim Projizieren geprüft, gecacht)
 */
exists: boolean, };

/**
 * Eintrag der Übernahmeliste (§3.12): i18n-ID `ng-migration-item-{block,emblem,username,output,keystore,
 * proxy,placeholder,position,language}` mit Detail (z. B. Dateiname der Bildmarke); `position` =
 * „Automatische Positionierung“ (`SIGNATURE_POSITION=auto`, Nachtrag 8).
 */
export type MigrationItemView = { 
/**
 * i18n-ID
 */
code: string, 
/**
 * Detail
 */
detail: string | null, };

/**
 * Ersteinrichtung (§3.12; nur beim ersten Start).
 */
export type FirstRunView = { 
/**
 * Migrationsangebot (gleich [`ViewState::migration`]); `None` = Schritt 1 entfällt
 */
migration: MigrationOfferView | null, 
/**
 * Standard-Signiermittel (`None` = „Jedes Mal fragen“)
 */
default_backend: BackendChoice | null, 
/**
 * Profil des Signaturblocks
 */
profile: SignatureProfile, 
/**
 * Sprache des Signaturblocks (`None` = System)
 */
block_language: Lang | null, 
/**
 * Revision der Blockvorschau
 */
preview_revision: number, };

/**
 * Freigaben für Knöpfe, Befehlspalette und Tastenkürzel (§3.13), berechnet aus dem Zustand
 * (`actions::actions_for`).
 */
export type ActionsView = { 
/**
 * Dateidialog/Ablegen sinnvoll (nicht während des Signierens)
 */
open_files: boolean, 
/**
 * `sign_with` möglich (ohne `signers.selected` öffnet die UI das Signiermittel-Menü); ohne
 * Signiermittel-Auswahl und ohne Signiermittel (`signers.choice_offered = false`, `selected = None`: die
 * Richtlinie schließt jedes aus, ADR 0004 Nachtrag 20) `false`
 */
sign: boolean, 
/**
 * Sticker und Optionen bedienbar
 */
edit_placement: boolean, 
/**
 * Stapelübersicht mit mindestens einem signierbaren Dokument (und einem Signiermittel, siehe [`Self::sign`])
 */
start_batch: boolean, 
/**
 * weitere Dokumente in der Warteschlange (`show_batch`)
 */
show_batch: boolean, 
/**
 * Einstellungen erreichbar (= `main_bar.settings_enabled`)
 */
settings: boolean, 
/**
 * „Zur Startseite“ (`go_home`; Fertig: `navigate{Open}`) erlaubt
 */
home: boolean, };

/**
 * Geparkte Arbeit (ADR 0004 Nachtrag 10): Karte „In Bearbeitung“ der Startansicht mit „Weiter bearbeiten“
 * (`resume_parked`) und „Schließen“ (`discard_parked`).
 */
export type ParkedWorkView = { 
/**
 * Name des geparkten Dokuments; `None` = Stapel („Stapel · n Dokumente“): „Weiter bearbeiten“ führt dann zur
 * Stapelübersicht (Nachtrag 13; „Fertig“ ohne Speichern eines Stapeleintrags nennt das Dokument)
 */
doc_name: string | null, 
/**
 * offene Dokumente (Einträge des Stapels bzw. das Dokument samt wartenden)
 */
documents: number, 
/**
 * Signatur bzw. Stapeleinträge von Hand platziert (`DocumentDetectionView::auto_placed == false` bzw.
 * `BatchView::manual_plans > 0`): „Schließen“ fragt vorher nach
 */
placed_by_hand: boolean, 
/**
 * signiert, aber noch nicht gespeichert („Fertig“ mit „Speichern“): „Schließen“ verwirft die Signatur und fragt
 * vorher nach
 */
unsaved: boolean, };

/**
 * Ablauf eines Stapellaufs mit einem Signiermittel (ADR 0004 §3.8, Nachtrag 12). Der Kern leitet ihn aus den
 * Fähigkeiten des Backends ab ([`BatchFlowView::from_capabilities`]); die Oberfläche wählt danach ihre Texte
 * („einmal anmelden, dann jedes Dokument einzeln bestätigen“ nur bei ID Austria, „Bestätigung k von n“ nur mit
 * einer Bestätigung je Dokument).
 */
export type BatchFlowView = "login_then_confirm_each" | "confirm_each" | "unlock_once";

/**
 * Hinweis zum Teilen auf iOS (ADR 0004 Nachtrag 12, [`ViewState::share_hint`]): ein kurzer, nicht blockierender
 * Hinweis im Arbeitsbereich mit „Weitere PDFs hinzufügen …“ und „Ausblenden“ (`dismiss_share_hint`).
 */
export type ShareHintView = { 
/**
 * das über „Teilen“ empfangene Dokument (das offene im Arbeitsbereich)
 */
doc: DocId, };

/**
 * „Über PIDA“ (ADR 0004 Nachtrag 21, [`ViewState::about`]): Version mit Versionsdatum, Links und die
 * Update-Prüfung. Alles entscheidet der Kern; die Oberfläche formatiert nur (Datum in der Oberflächensprache).
 */
export type AboutView = { 
/**
 * Version der App (`0.2.0`)
 */
version: string, 
/**
 * Versionsdatum `JJJJ-MM-TT` – Commit-Datum des gebauten Stands, beim Bauen eingebettet (`SOURCE_DATE_EPOCH`,
 * sonst Git, ohne Git die Dateizeit von `Cargo.toml`, zuletzt das Datum des Bauens; nie vor 2026); sein Jahr steht
 * im Urheberrechtsvermerk
 */
release_date: string, 
/**
 * Versionshinweise dieser Version (GitHub-Release `v<version>`)
 */
release_notes_url: string, 
/**
 * Quellcode (Repository)
 */
source_url: string, 
/**
 * Update-Prüfung; `None` = keine (iOS/Android: dort keine Verteilung über GitHub; ohne Oberfläche bzw. ohne
 * Netz-Zugang des Kerns)
 */
update: UpdateCheckView | null, };

/**
 * Zustand der Update-Prüfung ([`UpdateCheckView::state`]).
 */
export type UpdateCheckState = "unchecked" | "disabled" | "policy_off" | "checking" | "up_to_date" | "available" | "failed";

/**
 * Update-Prüfung für „Über PIDA“ (ADR 0004 Nachtrag 21).
 */
export type UpdateCheckView = { 
/**
 * Zustand
 */
state: UpdateCheckState, 
/**
 * `available`: die neue Version; `up_to_date`: die neueste in Frage kommende (`None` = keine); sonst `None`
 */
latest_version: string | null, 
/**
 * Release-Seite der neuen Version (nur `available`)
 */
release_url: string | null, 
/**
 * letzte erfolgreiche Prüfung (RFC 3339 mit Offset), auch aus dem Zwischenspeicher; `None` = noch nie
 */
checked_at: string | null, 
/**
 * Grund (i18n-ID `ng-update-failed-*`), nur `failed`
 */
reason: string | null, };

/**
 * Hinweis auf der Startseite ([`ViewState::update_hint`]): neue Version und ihre Release-Seite.
 */
export type UpdateHintView = { 
/**
 * die neue Version (`0.2.4`)
 */
version: string, 
/**
 * Release-Seite der neuen Version
 */
release_url: string, };
