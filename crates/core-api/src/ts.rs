//! TypeScript-Typen der Bridge, erzeugt aus den Rust-Typen (Feature `ts`, ts-rs 12).
//!
//! [`typescript_bindings`] liefert den Inhalt von `apps/app/src/core/generated.ts`; der Test
//! `tests/ts_bindings.rs` bricht bei jeder Abweichung ab (neu schreiben mit
//! `PIDA_BLESS=1 cargo test -p pida-core-api --test ts_bindings`). `view.rs` bleibt damit die
//! einzige Quelle des Vertrags Kern ↔ Oberfläche.
//!
//! Typen aus Crates außerhalb der Bridge (`pida-config::Settings`, `pida-placement::Placement`)
//! können `ts_rs::TS` wegen der Orphan-Regel nicht erhalten; für sie stehen hier **Spiegel**, die über
//! `#[ts(as = …)]` eingesetzt werden. Gegen Abweichungen sichern ab:
//! - die vollständige Abbildung `From<&Original>` ohne `..` (ein neues Feld oder eine neue Variante
//!   bricht den Build) und
//! - der Test, dass Original und Spiegel identisch serialisieren (Feldnamen, Tags, Schreibweise).

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::{Config, TS};

use pida_i18n::Lang;

// ------------------------------------------------------------------------------------------------
// Spiegel: pida-placement
// ------------------------------------------------------------------------------------------------

/// Spiegel von `pida_placement::Placement` (PAGE_ABS, linke obere Ecke, y = Oberkante von unten, pt).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
pub struct Placement {
    /// 0-basierter Seitenindex; `== Seitenzahl` = virtuelle neue Seite
    pub page_index: usize,
    /// linke Kante in pt
    pub x: f32,
    /// Oberkante in pt (von unten)
    pub y: f32,
    /// Breite in pt
    pub width: f32,
    /// Höhe in pt
    pub height: f32,
}

impl From<&pida_placement::Placement> for Placement {
    fn from(v: &pida_placement::Placement) -> Self {
        let pida_placement::Placement { page_index, x, y, width, height } = *v;
        Placement { page_index, x, y, width, height }
    }
}

// ------------------------------------------------------------------------------------------------
// Spiegel: pida-config (Settings-Schema, Plan §7.1)
// ------------------------------------------------------------------------------------------------

/// Spiegel von `pida_config::SignatureProfile`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SignatureProfile {
    /// `SIGNATURBLOCK_SMALL`
    SignaturblockSmall,
    /// `AMTSSIGNATURBLOCK`
    Amtssignaturblock,
    /// `BASE_LOGO`
    BaseLogo,
    /// `INVISIBLE`
    Invisible,
}

/// Spiegel von `pida_config::NoteSetting`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "mode", content = "text", rename_all = "snake_case")]
pub enum NoteSetting {
    /// Standardtext
    Default,
    /// kein Hinweis
    None,
    /// eigener Text
    Custom(String),
}

/// Spiegel von `pida_config::WindowSize`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WindowSize {
    /// Breite
    pub width: u32,
    /// Höhe
    pub height: u32,
}

/// Spiegel von `pida_config::GeneralSettings`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct GeneralSettings {
    /// UI-Sprache (`None` = System)
    pub ui_language: Option<Lang>,
    /// Update-Prüfung
    pub update_check: bool,
    /// Fenstergröße
    pub window_size: Option<WindowSize>,
    /// letzter Ordner (Dateidialog, F-11)
    pub last_source_directory: Option<PathBuf>,
    /// „Zuletzt signiert“ merken (ADR 0004 §3.10)
    pub recent_documents: bool,
    /// Ersteinrichtung abgeschlossen (ADR 0004 §3.12)
    pub first_run_completed: bool,
}

/// Spiegel von `pida_config::CmsProfileChoice`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum CmsProfileChoice {
    /// kompatibel zu PDF-AS
    PdfasCompat,
    /// PAdES-BASELINE-B
    PadesBb,
}

/// Spiegel von `pida_config::SignatureSettings`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct SignatureSettings {
    /// Profil
    pub profile: SignatureProfile,
    /// Sprache des Signaturblocks (`None` = System)
    pub block_language: Option<Lang>,
    /// Hinweis
    pub note: NoteSetting,
    /// PDF/A erzwingen
    pub pdfa_force: bool,
    /// automatische Positionierung
    pub auto_position: bool,
    /// Bildmarke
    pub emblem: Option<PathBuf>,
    /// Größe „nur Bildmarke“ in mm
    pub logo_only_size_mm: f64,
    /// CMS-Profil
    pub cms_profile: CmsProfileChoice,
}

/// Spiegel von `pida_config::PlaceholderKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum PlaceholderKind {
    /// leere Signaturfelder
    Fields,
    /// QR-Platzhalter
    Marker,
}

/// Spiegel von `pida_config::PlaceholderSettings`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PlaceholderSettings {
    /// Suche aktiv
    pub search: bool,
    /// Art
    pub kind: PlaceholderKind,
}

/// Spiegel von `pida_config::BackendChoice`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum BackendChoice {
    /// ID Austria
    IdAustria,
    /// lokale BKU
    LocalBku,
    /// Keystore
    Keystore,
}

/// Spiegel von `pida_config::BackendSettings`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct BackendSettings {
    /// Standard-Signiermittel
    pub default: Option<BackendChoice>,
}

/// Spiegel von `pida_config::IdAustriaSettings` (das Passwort wird nie persistiert).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct IdAustriaSettings {
    /// Benutzername/Telefonnummer
    pub username: Option<String>,
    /// „Merken“
    pub remember: bool,
    /// FIDO2 als Standard
    pub fido2_default: bool,
}

/// Spiegel von `pida_config::PasswordStorage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum PasswordStorage {
    /// nie
    None,
    /// bis Programmende
    Memory,
    /// Schlüsselbund
    Keychain,
}

/// Spiegel von `pida_config::KeystoreSettings`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct KeystoreSettings {
    /// aktiviert
    pub enabled: bool,
    /// Datei
    pub file: Option<PathBuf>,
    /// angegebener Typ
    pub declared_type: Option<String>,
    /// Alias
    pub alias: Option<String>,
    /// Passwort-Speicherart
    pub password_storage: PasswordStorage,
    /// Speicherart ausdrücklich gewählt (`false` = die erste Passworteingabe fragt „Passwort merken?“)
    pub storage_chosen: bool,
}

/// Spiegel von `pida_config::OutputSettings`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct OutputSettings {
    /// Ausgabeordner
    pub folder: Option<PathBuf>,
    /// Dateinamenzusatz
    pub postfix: String,
}

/// Spiegel von `pida_config::ProxyMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ProxyMode {
    /// System
    System,
    /// kein Proxy
    None,
    /// manuell
    Manual,
}

/// Spiegel von `pida_config::ProxySettings`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProxySettings {
    /// Modus
    pub mode: ProxyMode,
    /// Host
    pub host: Option<String>,
    /// Port
    pub port: Option<u16>,
    /// Benutzer
    pub user: Option<String>,
}

/// Spiegel von `pida_config::CertificateCacheSettings`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CertificateCacheSettings {
    /// Opt-in
    pub enabled: bool,
}

/// Spiegel von `pida_config::PlacementStart`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum PlacementStart {
    /// Seitenvorschau
    Preview,
    /// Auswahl der Methode
    Methods,
}

/// Spiegel von `pida_config::AnnounceDetail`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AnnounceDetail {
    /// kurz
    Short,
    /// ausführlich
    Detailed,
}

/// Spiegel von `pida_config::KeyboardStep`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum KeyboardStep {
    /// 15 pt
    Standard,
    /// 1 mm
    Mm1,
    /// 5 mm
    Mm5,
    /// 10 mm
    Mm10,
}

/// Spiegel von `pida_config::AccessibilitySettings`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AccessibilitySettings {
    /// „Barrierefreie Bedienung“
    pub mode: bool,
    /// Womit die Platzierung beginnt
    pub start: PlacementStart,
    /// Umfang der Ansagen
    pub announce: AnnounceDetail,
    /// erst nach dem Verschieben ansagen
    pub announce_after_move: bool,
    /// Schrittweite der Pfeiltasten
    pub keyboard_step: KeyboardStep,
    /// vor dem Signieren prüfen
    pub check_before_sign: bool,
}

/// Wert eines unbekannten Eintrags (`Settings::unknown`): beliebiges JSON, unverändert zurückgeben.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(transparent)]
#[ts(type = "unknown")]
pub struct UnknownValue(pub serde_json::Value);

/// Spiegel von `pida_config::Settings`. Unbekannte Einträge (`Settings::unknown`, `flatten`) stehen
/// auf oberster Ebene und gehen beim Speichern unverändert zurück.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Settings {
    /// Schema-Version
    pub schema_version: u32,
    /// Allgemein
    pub general: GeneralSettings,
    /// Signatur
    pub signature: SignatureSettings,
    /// Platzhalter
    pub placeholder: PlaceholderSettings,
    /// Signiermittel
    pub backend: BackendSettings,
    /// ID Austria
    pub id_austria: IdAustriaSettings,
    /// Keystore
    pub keystore: KeystoreSettings,
    /// Ausgabe
    pub output: OutputSettings,
    /// Proxy
    pub proxy: ProxySettings,
    /// Zertifikats-Cache
    pub certificate_cache: CertificateCacheSettings,
    /// Barrierefreiheit
    pub accessibility: AccessibilitySettings,
    /// unbekannte Einträge
    #[serde(flatten)]
    pub unknown: BTreeMap<String, UnknownValue>,
}

impl From<&pida_config::Settings> for Settings {
    fn from(v: &pida_config::Settings) -> Self {
        use pida_config as c;
        let c::Settings {
            schema_version,
            general,
            signature,
            placeholder,
            backend,
            id_austria,
            keystore,
            output,
            proxy,
            certificate_cache,
            accessibility,
            unknown,
        } = v;
        let c::GeneralSettings {
            ui_language,
            update_check,
            window_size,
            last_source_directory,
            recent_documents,
            first_run_completed,
        } = general;
        let c::SignatureSettings {
            profile,
            block_language,
            note,
            pdfa_force,
            auto_position,
            emblem,
            logo_only_size_mm,
            cms_profile,
        } = signature;
        let c::PlaceholderSettings { search, kind } = placeholder;
        let c::BackendSettings { default } = backend;
        let c::IdAustriaSettings { username, remember, fido2_default } = id_austria;
        let c::KeystoreSettings { enabled, file, declared_type, alias, password_storage, storage_chosen } = keystore;
        let c::OutputSettings { folder, postfix } = output;
        let c::ProxySettings { mode, host, port, user } = proxy;
        let c::CertificateCacheSettings { enabled: cache_enabled } = certificate_cache;
        let c::AccessibilitySettings {
            mode: a11y_mode,
            start,
            announce,
            announce_after_move,
            keyboard_step,
            check_before_sign,
        } = accessibility;
        Settings {
            schema_version: *schema_version,
            general: GeneralSettings {
                ui_language: *ui_language,
                update_check: *update_check,
                window_size: window_size.map(|c::WindowSize { width, height }| WindowSize { width, height }),
                last_source_directory: last_source_directory.clone(),
                recent_documents: *recent_documents,
                first_run_completed: *first_run_completed,
            },
            signature: SignatureSettings {
                profile: match profile {
                    c::SignatureProfile::SignaturblockSmall => SignatureProfile::SignaturblockSmall,
                    c::SignatureProfile::Amtssignaturblock => SignatureProfile::Amtssignaturblock,
                    c::SignatureProfile::BaseLogo => SignatureProfile::BaseLogo,
                    c::SignatureProfile::Invisible => SignatureProfile::Invisible,
                },
                block_language: *block_language,
                note: match note {
                    c::NoteSetting::Default => NoteSetting::Default,
                    c::NoteSetting::None => NoteSetting::None,
                    c::NoteSetting::Custom(t) => NoteSetting::Custom(t.clone()),
                },
                pdfa_force: *pdfa_force,
                auto_position: *auto_position,
                emblem: emblem.clone(),
                logo_only_size_mm: *logo_only_size_mm,
                cms_profile: match cms_profile {
                    c::CmsProfileChoice::PdfasCompat => CmsProfileChoice::PdfasCompat,
                    c::CmsProfileChoice::PadesBb => CmsProfileChoice::PadesBb,
                },
            },
            placeholder: PlaceholderSettings {
                search: *search,
                kind: match kind {
                    c::PlaceholderKind::Fields => PlaceholderKind::Fields,
                    c::PlaceholderKind::Marker => PlaceholderKind::Marker,
                },
            },
            backend: BackendSettings { default: default.map(backend_choice) },
            id_austria: IdAustriaSettings {
                username: username.clone(),
                remember: *remember,
                fido2_default: *fido2_default,
            },
            keystore: KeystoreSettings {
                enabled: *enabled,
                file: file.clone(),
                declared_type: declared_type.clone(),
                alias: alias.clone(),
                password_storage: match password_storage {
                    c::PasswordStorage::None => PasswordStorage::None,
                    c::PasswordStorage::Memory => PasswordStorage::Memory,
                    c::PasswordStorage::Keychain => PasswordStorage::Keychain,
                },
                storage_chosen: *storage_chosen,
            },
            output: OutputSettings { folder: folder.clone(), postfix: postfix.clone() },
            proxy: ProxySettings {
                mode: match mode {
                    c::ProxyMode::System => ProxyMode::System,
                    c::ProxyMode::None => ProxyMode::None,
                    c::ProxyMode::Manual => ProxyMode::Manual,
                },
                host: host.clone(),
                port: *port,
                user: user.clone(),
            },
            certificate_cache: CertificateCacheSettings { enabled: *cache_enabled },
            accessibility: AccessibilitySettings {
                mode: *a11y_mode,
                start: match start {
                    c::PlacementStart::Preview => PlacementStart::Preview,
                    c::PlacementStart::Methods => PlacementStart::Methods,
                },
                announce: match announce {
                    c::AnnounceDetail::Short => AnnounceDetail::Short,
                    c::AnnounceDetail::Detailed => AnnounceDetail::Detailed,
                },
                announce_after_move: *announce_after_move,
                keyboard_step: match keyboard_step {
                    c::KeyboardStep::Standard => KeyboardStep::Standard,
                    c::KeyboardStep::Mm1 => KeyboardStep::Mm1,
                    c::KeyboardStep::Mm5 => KeyboardStep::Mm5,
                    c::KeyboardStep::Mm10 => KeyboardStep::Mm10,
                },
                check_before_sign: *check_before_sign,
            },
            unknown: unknown
                .iter()
                .map(|(k, v)| (k.clone(), UnknownValue(serde_json::to_value(v).unwrap_or(serde_json::Value::Null))))
                .collect(),
        }
    }
}

fn backend_choice(v: pida_config::BackendChoice) -> BackendChoice {
    match v {
        pida_config::BackendChoice::IdAustria => BackendChoice::IdAustria,
        pida_config::BackendChoice::LocalBku => BackendChoice::LocalBku,
        pida_config::BackendChoice::Keystore => BackendChoice::Keystore,
    }
}

// ------------------------------------------------------------------------------------------------
// Erzeugung
// ------------------------------------------------------------------------------------------------

/// Kopf der erzeugten Datei.
pub const HEADER: &str = "\
// Automatisch erzeugt aus den Rust-Typen der Bridge (crates/core-api/src/view.rs und abhängige
// Crates) mit ts-rs – NICHT VON HAND ÄNDERN.
// Neu erzeugen: PIDA_BLESS=1 cargo test -p pida-core-api --test ts_bindings
//
// Abbildung (serde_json): Option<T> → T | null, PathBuf → string, u64/usize → number,
// Enums ohne serde-Attribut extern getaggt ({ \"Variante\": … } bzw. \"Variante\"),
// #[serde(tag = …)] intern getaggt.
";

/// Konfiguration: 64-Bit-Zahlen als `number` (Kennungen, Anfrage-IDs; serde_json schreibt Zahlen).
pub fn config() -> Config {
    Config::new().with_large_int("number")
}

/// Ein Eintrag der erzeugten Datei.
fn decl<T: TS>(cfg: &Config, out: &mut String) {
    out.push('\n');
    if let Some(docs) = T::docs() {
        out.push_str(&docs);
    }
    out.push_str("export ");
    out.push_str(&T::decl(cfg));
    out.push('\n');
}

macro_rules! declare_all {
    ($cfg:expr, $out:expr, $($t:ty),+ $(,)?) => {{
        let mut names: Vec<String> = Vec::new();
        $(
            decl::<$t>($cfg, $out);
            names.push(<$t as TS>::ident($cfg));
        )+
        names
    }};
}

/// Inhalt von `apps/app/src/core/generated.ts` und die Namen aller deklarierten Typen.
pub fn typescript_bindings_with_names() -> (String, Vec<String>) {
    use crate::view::*;
    use pida_backend_api as b;
    use pida_workflow as w;
    let cfg = config();
    let mut out = String::from(HEADER);
    let names = declare_all!(
        &cfg,
        &mut out,
        // pida-i18n, pida-workflow
        Lang,
        w::DocId,
        w::Step,
        w::MainBarStep,
        w::PlaceholderAnswer,
        w::ts_mirror::AcceptanceClass,
        w::ts_mirror::Invariant,
        w::ts_mirror::InvariantViolation,
        w::ts_mirror::OutputInfo,
        w::WorkflowError,
        w::DocResult,
        w::PlacementOrigin,
        w::ExistingPolicy,
        w::BatchOutput,
        // pida-backend-api
        b::BackendId,
        b::ts_mirror::DigestAlg,
        b::SignInput,
        b::SignOutput,
        b::CertNeed,
        b::Interaction,
        b::ServerNeed,
        b::Qualification,
        b::FactorSet,
        b::Capabilities,
        b::FormProblem,
        b::FormContext,
        b::QrImage,
        b::NativeForm,
        b::SecretKind,
        b::SecretPrompt,
        b::NoticeKind,
        b::NoticeButtons,
        b::NoticeCode,
        b::Notice,
        b::NoticeAnswer,
        b::Progress,
        // pida-placement
        Placement,
        // pida-config
        SignatureProfile,
        NoteSetting,
        WindowSize,
        GeneralSettings,
        CmsProfileChoice,
        SignatureSettings,
        PlaceholderKind,
        PlaceholderSettings,
        BackendChoice,
        BackendSettings,
        IdAustriaSettings,
        PasswordStorage,
        KeystoreSettings,
        OutputSettings,
        ProxyMode,
        ProxySettings,
        CertificateCacheSettings,
        PlacementStart,
        AnnounceDetail,
        KeyboardStep,
        AccessibilitySettings,
        Settings,
        // pida-core-api (qr_placeholder.rs)
        crate::qr_placeholder::QrPlaceholderProfile,
        crate::qr_placeholder::QrPlaceholderFormat,
        crate::qr_placeholder::QrPlaceholderSpec,
        // pida-core-api (view.rs)
        NoticeView,
        SaveDialogView,
        MigrationOfferView,
        DoneItemView,
        DocInfo,
        QueueItemView,
        FieldInfo,
        QrInfo,
        BlockView,
        BackendOptionView,
        PlatformOs,
        PlatformView,
        ResumeOfferView,
        KeystoreImportView,
        KeystoreFormatView,
        KeystoreEntryView,
        InteractionView,
        OutputView,
        MainBarView,
        StepView,
        SettingsView,
        ViewState,
        FormAnswerView,
        OpenFilesMode,
        UiEvent,
        // ADR 0004 (neue Oberfläche, Vertrag E0)
        RectPt,
        DocRefView,
        DocumentPhase,
        PlacementTarget,
        FieldTargetView,
        QrTargetView,
        PriorStatusView,
        ExistingSignatureView,
        DocConstraintsView,
        GuideAxis,
        GuideKind,
        GuideView,
        PageLayoutView,
        PlacementCheckView,
        BlockSlotView,
        BlocksView,
        PdfaIdView,
        PdfaDecision,
        DocumentDetectionView,
        DocumentView,
        SignersView,
        SignerView,
        SigningStage,
        ConfirmMethod,
        CertificateSource,
        PauseScope,
        BatchPauseView,
        SigningProgressView,
        ErrorOrigin,
        ErrorContextView,
        SignedSummaryView,
        BatchItemStatusView,
        PositionSummaryView,
        BatchItemView,
        OutputModeView,
        BatchOutputView,
        BatchEstimateView,
        BatchSummaryView,
        BatchView,
        RecentItemView,
        MigrationItemView,
        FirstRunView,
        ActionsView,
        // Nachtrag 10
        ParkedWorkView,
        // Nachtrag 12
        BatchFlowView,
        ShareHintView,
        // Nachtrag 21
        AboutView,
        UpdateCheckState,
        UpdateCheckView,
        // Backlog Nr. 33
        UpdateHintView,
    );
    (out, names)
}

/// Inhalt von `apps/app/src/core/generated.ts`.
pub fn typescript_bindings() -> String {
    typescript_bindings_with_names().0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn same<A: Serialize, B: Serialize>(a: &A, b: &B) {
        assert_eq!(serde_json::to_value(a).unwrap(), serde_json::to_value(b).unwrap());
    }

    #[test]
    fn placement_mirror_serializes_like_the_original() {
        let p = pida_placement::Placement { page_index: 3, x: 1.5, y: 700.0, width: 230.0, height: 49.0 };
        same(&p, &Placement::from(&p));
    }

    #[test]
    fn settings_mirror_serializes_like_the_original() {
        use pida_config as c;
        let mut s = c::Settings::default();
        same(&s, &Settings::from(&s));
        // alle Varianten und optionalen Werte belegt, dazu unbekannte Einträge (flatten)
        s.general.ui_language = Some(Lang::En);
        s.general.window_size = Some(c::WindowSize { width: 800, height: 600 });
        s.general.last_source_directory = Some("/tmp".into());
        s.general.recent_documents = false;
        s.general.first_run_completed = false;
        s.signature.block_language = Some(Lang::De);
        s.signature.note = c::NoteSetting::Custom("Hinweis".into());
        s.signature.emblem = Some("/logo.png".into());
        s.signature.cms_profile = c::CmsProfileChoice::PadesBb;
        s.placeholder.kind = c::PlaceholderKind::Fields;
        s.backend.default = Some(c::BackendChoice::LocalBku);
        s.id_austria.username = Some("+43664".into());
        s.keystore.password_storage = c::PasswordStorage::Keychain;
        s.keystore.storage_chosen = false;
        s.keystore.file = Some("/k.p12".into());
        s.proxy.mode = c::ProxyMode::Manual;
        s.proxy.port = Some(3128);
        s.unknown.insert("future".into(), toml::Value::Integer(5));
        let mut table = toml::map::Map::new();
        table.insert("x".into(), toml::Value::String("y".into()));
        s.unknown.insert("zukunft".into(), toml::Value::Table(table));
        same(&s, &Settings::from(&s));
        for p in [
            c::SignatureProfile::SignaturblockSmall,
            c::SignatureProfile::Amtssignaturblock,
            c::SignatureProfile::BaseLogo,
            c::SignatureProfile::Invisible,
        ] {
            s.signature.profile = p;
            same(&s, &Settings::from(&s));
        }
        for n in [c::NoteSetting::Default, c::NoteSetting::None] {
            s.signature.note = n;
            same(&s, &Settings::from(&s));
        }
        for (b, m, ps) in [
            (c::BackendChoice::IdAustria, c::ProxyMode::System, c::PasswordStorage::None),
            (c::BackendChoice::Keystore, c::ProxyMode::None, c::PasswordStorage::Memory),
        ] {
            s.backend.default = Some(b);
            s.proxy.mode = m;
            s.keystore.password_storage = ps;
            same(&s, &Settings::from(&s));
        }
        // der Spiegel liest das Original-JSON und schreibt es unverändert zurück
        let json = serde_json::to_value(&s).unwrap();
        let mirror: Settings = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(serde_json::to_value(&mirror).unwrap(), json);
    }
}
