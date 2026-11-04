//! Serde-Vertrag zwischen `pida-core-api` (Rust) und der Oberfläche (TypeScript).
//!
//! 1. **Rust → TS:** Beispiel-`ViewState`s für alle Varianten (Schritte, Interaktionen, Masken, Meldungen,
//!    Fortschritt, Formularprobleme, Einstellungen mit unbekannten Einträgen) werden serialisiert und mit
//!    `apps/app/src/core/fixtures/view-states.json` verglichen. Die Oberfläche prüft dieselbe Datei gegen
//!    ihre Typen (`src/core/contract.test.ts`). Neu schreiben: `PIDA_BLESS=1 cargo test -p pida-app
//!    --test serde_contract`.
//! 2. **TS → Rust:** Die in TypeScript typisierten Beispiel-Ereignisse
//!    (`src/core/fixtures/ui-events.ts` → `ui-events.json`) müssen sich als [`UiEvent`] lesen lassen und
//!    unverändert zurückschreiben.

use std::collections::BTreeMap;
use std::path::PathBuf;

use pida_backend_api::{
    BackendId, Capabilities, CertNeed, DigestAlg, FactorSet, FormContext, FormProblem, Interaction, NativeForm, Notice,
    NoticeButtons, NoticeCode, NoticeKind, Progress, QrImage, Qualification, SecretKind, SecretPrompt, ServerNeed,
    SignInput, SignOutput,
};
use pida_config::{NoteSetting, Settings};
use pida_core_api::{
    AboutView, ActionsView, BackendOptionView, BatchEstimateView, BatchFlowView, BatchItemStatusView, BatchItemView,
    BatchOutputView, BatchPauseView, BatchSummaryView, BatchView, BlockView, CertificateSource, ConfirmMethod,
    DocConstraintsView, DocInfo, DocRefView, DocumentDetectionView, DocumentPhase, DocumentView, DoneItemView,
    ErrorContextView, ErrorOrigin, ExistingSignatureView, FieldInfo, FieldTargetView, FirstRunView, GuideAxis,
    GuideKind, GuideView, InteractionView, KeystoreEntryView, KeystoreFormatView, KeystoreImportView, MainBarView,
    MigrationItemView, MigrationOfferView, NoticeView, OutputModeView, OutputView, PageLayoutView, ParkedWorkView,
    PauseScope, PdfaDecision, PdfaIdView, PlacementCheckView, PlacementTarget, PlatformOs, PlatformView,
    PositionSummaryView, PriorStatusView, QrInfo, QrTargetView, QueueItemView, RecentItemView, RectPt, ResumeOfferView,
    SaveDialogView, SettingsView, ShareHintView, SignedSummaryView, SignerView, SignersView, SigningProgressView,
    SigningStage, StepView, UiEvent, UpdateCheckState, UpdateCheckView, ViewState,
};
use pida_i18n::Lang;
use pida_pades::{AcceptanceClass, OutputInfo};
use pida_placement::Placement;
use pida_workflow::{DocId, DocResult, MainBarStep, PlacementOrigin, Step, WorkflowError};

const VIEW_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../src/core/fixtures/view-states.json");
const EVENT_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../src/core/fixtures/ui-events.json");

fn bar(cells: [&str; 4], settings_enabled: bool) -> MainBarView {
    let steps = [Step::Open, Step::Positioning, Step::Signing, Step::Finish]
        .into_iter()
        .zip(cells)
        .map(|(step, c)| MainBarStep { step, active: c.contains('A'), enabled: c.contains('E') })
        .collect();
    MainBarView { steps, settings_enabled }
}

fn view(step: StepView, bar: MainBarView) -> ViewState {
    ViewState {
        step,
        main_bar: bar,
        settings: None,
        language: Lang::De,
        accessibility: Some(pida_config::AccessibilitySettings::default()),
        notices: Vec::new(),
        migration: None,
        done: Vec::new(),
        platform: PlatformView { os: PlatformOs::Macos, mobile: false },
        resume: None,
        keystore_import: None,
        privacy_text: None,
        document: None,
        signers: SignersView::default(),
        batch: None,
        recent: None,
        first_run: None,
        actions: ActionsView::default(),
        parked: None,
        share_hint: None,
        about: None,
        update_hint: None,
    }
}

fn doc() -> DocInfo {
    DocInfo {
        id: DocId(7),
        name: "Vertrag.pdf".into(),
        page_count: 2,
        page_sizes: vec![(595.5, 842.0), (842.0, 595.5)],
        signed: false,
    }
}

fn queue() -> Vec<QueueItemView> {
    vec![
        QueueItemView { id: DocId(7), name: "Vertrag.pdf".into(), current: true },
        QueueItemView { id: DocId(8), name: "Angebot.pdf".into(), current: false },
    ]
}

fn ctx(problem: Option<FormProblem>) -> FormContext {
    FormContext {
        signature_data_url: Some("https://www.a-trust.at/mobile/ShowSigobj.aspx?sid=x".into()),
        can_switch_to_sms: true,
        can_switch_to_fido2: false,
        fido2_visible: true,
        problem,
    }
}

fn caps_native() -> Capabilities {
    Capabilities {
        input: SignInput::Document,
        output: SignOutput::Cms,
        certificate: CertNeed::OnlyForVisibleBlock,
        interaction: Interaction::NativeForms,
        server: ServerNeed::None,
        max_batch: 1,
        login_once_per_batch: true,
        document_leaves_device: true,
        qualification: Qualification::Qes,
        factors: FactorSet { app: true, sms: true, qr: true, fido2: true, card: false },
        cms_reserve: Some(16384),
    }
}

fn caps_keystore() -> Capabilities {
    Capabilities {
        input: SignInput::Digest { algs: vec![DigestAlg::Sha256, DigestAlg::Sha512] },
        output: SignOutput::RawSignature,
        certificate: CertNeed::Required,
        interaction: Interaction::Secret,
        server: ServerNeed::None,
        max_batch: 1,
        login_once_per_batch: false,
        document_leaves_device: false,
        qualification: Qualification::Unknown,
        factors: FactorSet::default(),
        cms_reserve: None,
    }
}

fn signing(interaction: InteractionView) -> ViewState {
    // Masken und Meldungen von ID Austria: das Dokument verlässt das Gerät (Kurzhinweis); Passwortabfragen
    // stammen vom Keystore (lokal)
    let document_leaves_device = !matches!(interaction, InteractionView::Secret { .. });
    view(
        StepView::Signing {
            interaction,
            queue: queue(),
            document_leaves_device,
            progress: SigningProgressView::default(),
        },
        bar(["A", "A", "A", ""], false),
    )
}

fn form(request: u64, form: NativeForm) -> ViewState {
    signing(InteractionView::Form { request, form })
}

fn notice(request: u64, kind: NoticeKind, code: NoticeCode, text: Option<&str>, buttons: NoticeButtons) -> ViewState {
    signing(InteractionView::Notice { request, notice: Notice { kind, code, text: text.map(str::to_owned), buttons } })
}

/// Alle Beispielzustände (Name → ViewState), Reihenfolge stabil.
fn samples() -> Vec<(&'static str, ViewState)> {
    let mut out: Vec<(&'static str, ViewState)> = vec![
        ("starting", view(StepView::Starting, bar(["", "", "", ""], false))),
        ("open_empty", view(StepView::Open { queue: vec![] }, bar(["AE", "", "", ""], true))),
        ("open_queue", view(StepView::Open { queue: queue() }, bar(["AE", "", "", ""], true))),
        ("loading", view(StepView::Loading { queue: queue() }, bar(["AE", "", "", ""], true))),
        (
            "warning",
            view(
                StepView::Warning { doc: doc(), codes: vec!["pdf-usage-rights".into(), "pdf-xfa-hybrid".into()] },
                bar(["AE", "", "", ""], true),
            ),
        ),
    ];
    // Hinweise ohne Rückfrage, Import-Angebot und abgeschlossene Dokumente (Querschnitt)
    let mut open = view(StepView::Open { queue: vec![] }, bar(["AE", "", "", ""], true));
    let mut p = BTreeMap::new();
    p.insert("path".to_owned(), "/Users/demo/settings.toml.old".to_owned());
    open.notices = vec![
        NoticeView { id: 1, code: "error-PositioningNotPossible".into(), params: BTreeMap::new() },
        NoticeView { id: 2, code: "ng-config-reset-done".into(), params: p },
    ];
    open.migration = Some(MigrationOfferView {
        legacy_file: PathBuf::from("/Users/demo/.pdf-over/PDF-Over.config"),
        plaintext_secrets: true,
        items: Vec::new(),
    });
    open.done = vec![
        DoneItemView {
            id: DocId(1),
            name: "a.pdf".into(),
            input: PathBuf::from("/Users/demo/a.pdf"),
            result: DocResult::Signed {
                output: OutputInfo { path: PathBuf::from("/Users/demo/a_signed.pdf"), len: 1234, sha256: "ab".into() },
            },
        },
        DoneItemView {
            id: DocId(2),
            name: "b.pdf".into(),
            input: PathBuf::from("/Users/demo/b.pdf"),
            result: DocResult::Cancelled,
        },
        DoneItemView {
            id: DocId(3),
            name: "c.pdf".into(),
            input: PathBuf::from("/Users/demo/c.pdf"),
            result: DocResult::Failed {
                error: WorkflowError::Rejected {
                    class: AcceptanceClass::K5Rejected,
                    code: "ng-document-rejected".into(),
                },
            },
        },
    ];
    out.push(("open_notices_migration_done", open));
    out.push((
        "placeholder_fields",
        view(
            StepView::Placeholder {
                doc: doc(),
                fields: vec![
                    FieldInfo { name: "Unterschrift 1".into(), page: Some(1) },
                    FieldInfo { name: "Feld.ohne.Seite".into(), page: None },
                ],
                qr: None,
            },
            bar(["AE", "", "", ""], true),
        ),
    ));
    out.push((
        "placeholder_qr",
        view(
            StepView::Placeholder { doc: doc(), fields: vec![], qr: Some(QrInfo { page: 1, id: Some("1".into()) }) },
            bar(["AE", "", "", ""], true),
        ),
    ));
    let mut announce = BTreeMap::new();
    announce.insert("page".to_owned(), "2".to_owned());
    announce.insert("pages".to_owned(), "2".to_owned());
    announce.insert("left".to_owned(), "105".to_owned());
    announce.insert("top".to_owned(), "120".to_owned());
    out.push((
        "position",
        view(
            StepView::Position {
                doc: doc(),
                page: 1,
                block: BlockView {
                    width: 232.5,
                    height: 82.0,
                    preview_revision: 3,
                    emblem_placeholder: false,
                    emblem_choosable: true,
                    language: pida_i18n::Lang::De,
                },
                placement: Placement { page_index: 1, x: 297.5, y: 210.5, width: 232.5, height: 82.0 },
                new_page: false,
                announce,
            },
            bar(["AE", "A", "", ""], true),
        ),
    ));
    out.push((
        "position_new_page",
        view(
            StepView::Position {
                doc: DocInfo { signed: true, ..doc() },
                page: 2,
                block: BlockView {
                    width: 100.0,
                    height: 40.0,
                    preview_revision: 1,
                    emblem_placeholder: false,
                    emblem_choosable: true,
                    language: pida_i18n::Lang::De,
                },
                placement: Placement { page_index: 2, x: 0.0, y: 595.5, width: 100.0, height: 40.0 },
                new_page: true,
                announce: BTreeMap::new(),
            },
            bar(["AE", "A", "", ""], true),
        ),
    ));
    out.push((
        "backend",
        view(
            StepView::Backend {
                options: vec![
                    BackendOptionView {
                        id: BackendId::AtrustNative,
                        available: true,
                        capabilities: Some(caps_native()),
                        notes: Vec::new(),
                    },
                    BackendOptionView {
                        id: BackendId::LocalBku,
                        available: false,
                        capabilities: None,
                        notes: Vec::new(),
                    },
                    BackendOptionView {
                        id: BackendId::Keystore,
                        available: true,
                        capabilities: Some(caps_keystore()),
                        notes: Vec::new(),
                    },
                    BackendOptionView { id: BackendId::Csc, available: false, capabilities: None, notes: Vec::new() },
                ],
            },
            bar(["AE", "AE", "A", ""], true),
        ),
    ));
    for (name, progress) in [
        ("waiting_connecting", Progress::Connecting),
        ("waiting_certificate", Progress::ReadingCertificate),
        ("waiting_server", Progress::WaitingForServer),
        ("waiting_signing", Progress::Signing),
        ("waiting_document", Progress::Document { index: 2, total: 3 }),
        ("waiting_done", Progress::Done),
    ] {
        out.push((name, signing(InteractionView::Waiting { progress })));
    }
    out.push((
        "form_login",
        form(1, NativeForm::Login { prefill_username: Some("+43664123456789".into()), remember: true, ctx: ctx(None) }),
    ));
    for (name, problem) in [
        ("form_login_too_short", FormProblem::PasswordTooShort),
        ("form_login_too_long", FormProblem::PasswordTooLong),
        ("form_login_rejected", FormProblem::LoginRejected),
        ("form_login_atrust", FormProblem::Atrust("Benutzer gesperrt".into())),
        ("form_login_fido2_error", FormProblem::Fido2Error { code: "fido2-timeout".into(), detail: "x".into() }),
    ] {
        out.push((
            name,
            form(2, NativeForm::Login { prefill_username: None, remember: false, ctx: ctx(Some(problem)) }),
        ));
    }
    out.push(("form_sms", form(3, NativeForm::SmsTan { reference_value: "Xk7Pq".into(), ctx: ctx(None) })));
    for (name, problem) in [
        ("form_sms_no_tan", FormProblem::NoTan),
        ("form_sms_too_long", FormProblem::TanTooLong),
        ("form_sms_reference", FormProblem::EnteredReferenceValue),
    ] {
        out.push((name, form(4, NativeForm::SmsTan { reference_value: "Xk7Pq".into(), ctx: ctx(Some(problem)) })));
    }
    out.push((
        "form_qr",
        form(
            5,
            NativeForm::QrCode {
                reference_value: "Xk7Pq".into(),
                image: Some(QrImage { bytes: vec![137, 80, 78, 71], mime: Some("image/png".into()) }),
                timeout_secs: Some(300),
                ctx: ctx(None),
            },
        ),
    ));
    out.push((
        "form_qr_failed",
        form(
            6,
            NativeForm::QrCode {
                reference_value: "Xk7Pq".into(),
                image: None,
                timeout_secs: None,
                ctx: ctx(Some(FormProblem::QrImageFailed)),
            },
        ),
    ));
    out.push((
        "form_app",
        form(
            7,
            NativeForm::WaitForApp { reference_value: Some("Xk7Pq".into()), timeout_secs: Some(300), ctx: ctx(None) },
        ),
    ));
    out.push((
        "form_app_no_reference",
        form(8, NativeForm::WaitForApp { reference_value: None, timeout_secs: None, ctx: FormContext::default() }),
    ));
    out.push(("form_fido2", form(9, NativeForm::Fido2 { in_progress: true, ctx: ctx(None) })));
    out.push((
        "form_fido2_failed",
        form(
            10,
            NativeForm::Fido2 { in_progress: false, ctx: ctx(Some(FormProblem::Fido2Failed("fido2-timeout".into()))) },
        ),
    ));
    // erste Verwendung des Keystores: „Passwort merken?“ unter dem Passwortfeld (Nachtrag ADR 0004)
    for (name, kind, retry, ask_storage) in [
        ("secret_keystore", SecretKind::KeystorePassword, false, true),
        ("secret_key_retry", SecretKind::KeyPassword, true, false),
        ("secret_proxy", SecretKind::ProxyPassword, false, false),
        ("secret_fido2_pin", SecretKind::Fido2Pin, false, false),
    ] {
        out.push((
            name,
            signing(InteractionView::Secret {
                request: 11,
                prompt: SecretPrompt { kind, subject: "keystore.p12".into(), retry },
                ask_storage,
                keychain_available: ask_storage,
            }),
        ));
    }
    out.push((
        "notice_interstitial",
        notice(
            12,
            NoticeKind::Info,
            NoticeCode::AtrustInterstitial,
            Some("Ihr Zertifikat läuft ab."),
            NoticeButtons::Ok,
        ),
    ));
    out.push((
        "notice_atrust_error",
        notice(13, NoticeKind::Error, NoticeCode::AtrustError, Some("Fehler 123"), NoticeButtons::RetryCancel),
    ));
    out.push(("notice_proxy", notice(14, NoticeKind::Error, NoticeCode::ProxyAuthRequired, None, NoticeButtons::Ok)));
    out.push((
        "notice_too_large",
        notice(15, NoticeKind::Error, NoticeCode::DocumentTooLarge, None, NoticeButtons::Ok),
    ));
    out.push((
        "notice_http",
        notice(16, NoticeKind::Error, NoticeCode::Http { status: 503 }, Some("Service Unavailable"), NoticeButtons::Ok),
    ));
    out.push((
        "notice_connection",
        notice(17, NoticeKind::Error, NoticeCode::ConnectionFailed, Some("timeout"), NoticeButtons::Ok),
    ));
    out.push((
        "notice_local_bku",
        notice(18, NoticeKind::Warning, NoticeCode::LocalBkuNotReachable, None, NoticeButtons::RetryCancel),
    ));
    out.push((
        "notice_keystore_missing",
        notice(
            19,
            NoticeKind::Error,
            NoticeCode::KeystoreFileMissing,
            Some("keystore.p12"),
            NoticeButtons::RetryCancel,
        ),
    ));
    out.push((
        "notice_alias_missing",
        notice(20, NoticeKind::Error, NoticeCode::KeystoreAliasMissing, Some("signer"), NoticeButtons::RetryCancel),
    ));
    out.push((
        "notice_keystore",
        notice(21, NoticeKind::Error, NoticeCode::KeystoreError, None, NoticeButtons::OkCancel),
    ));
    out.push((
        "notice_conversion",
        notice(22, NoticeKind::Warning, NoticeCode::KeystoreConversionNeeded, None, NoticeButtons::OkCancel),
    ));
    out.push((
        "output_saved",
        view(
            StepView::Output {
                result: OutputView {
                    path: Some(PathBuf::from("/Users/demo/Vertrag_signed.pdf")),
                    save_pending: false,
                    ask_overwrite: None,
                    save_dialog: None,
                    summary: None,
                },
                next: None,
                pending: 0,
            },
            bar(["AE", "AE", "AE", "A"], true),
        ),
    ));
    out.push((
        "output_pending_next",
        view(
            StepView::Output {
                result: OutputView {
                    path: None,
                    save_pending: true,
                    ask_overwrite: None,
                    save_dialog: None,
                    summary: None,
                },
                next: Some(doc()),
                pending: 1,
            },
            bar(["AE", "AE", "AE", "A"], true),
        ),
    ));
    out.push((
        "output_ask_overwrite",
        view(
            StepView::Output {
                result: OutputView {
                    path: None,
                    save_pending: false,
                    ask_overwrite: Some(PathBuf::from("/Users/demo/Vertrag_signed.pdf")),
                    save_dialog: None,
                    summary: None,
                },
                next: None,
                pending: 0,
            },
            bar(["AE", "AE", "AE", "A"], true),
        ),
    ));
    out.push((
        "output_save_dialog",
        view(
            StepView::Output {
                result: OutputView {
                    path: None,
                    save_pending: false,
                    ask_overwrite: None,
                    save_dialog: Some(SaveDialogView {
                        directory: PathBuf::from("/Users/demo"),
                        file_name: "Vertrag_signed.pdf".into(),
                    }),
                    summary: None,
                },
                next: None,
                pending: 0,
            },
            bar(["AE", "AE", "AE", "A"], true),
        ),
    ));
    let mut params = BTreeMap::new();
    params.insert("arg1".to_owned(), "Vertrag.pdf".to_owned());
    out.push((
        "error",
        view(
            StepView::Error {
                code: "error-MayNotBeAPDF".into(),
                params,
                retry: true,
                context: ErrorContextView::default(),
            },
            bar(["AE", "", "", ""], true),
        ),
    ));
    out.push(("finished", view(StepView::Finished { exit_code: 4 }, bar(["", "", "", ""], false))));

    // Einstellungen als Overlay, mit unbekannten Einträgen (Settings::unknown, flatten)
    let mut settings = Settings::default();
    settings.general.ui_language = Some(Lang::En);
    settings.signature.note = NoteSetting::Custom("Eigener Hinweis".into());
    settings.signature.emblem = Some(PathBuf::from("/Users/demo/logo.png"));
    settings.keystore.enabled = true;
    settings.keystore.file = Some(PathBuf::from("/Users/demo/keystore.p12"));
    settings.keystore.storage_chosen = false;
    settings.output.folder = Some(PathBuf::from("."));
    settings.proxy.port = Some(8080);
    settings.unknown.insert("signature.future_key".into(), toml::Value::String("bleibt".into()));
    settings.unknown.insert("future".into(), toml::Value::Integer(42));
    let mut problems = BTreeMap::new();
    problems.insert("proxy.host".to_owned(), "ng-ui-problem-required".to_owned());
    let mut with_settings = view(StepView::Open { queue: vec![] }, bar(["AE", "", "", ""], false));
    with_settings.settings = Some(SettingsView {
        settings,
        problems,
        keychain_available: true,
        fido2_available: false,
        keystore_ready: true,
        keystore_format: Some(KeystoreFormatView::Pkcs12),
        // Alias-Auswahl (Nachtrag ADR 0004): Schlüssel und ein Zertifikat ohne Schlüssel
        keystore_entries: Some(vec![
            KeystoreEntryView {
                alias: "erster".into(),
                subject_cn: Some("Max Muster (NUR TEST)".into()),
                not_after: Some("2036-10-14T08:00:00Z".into()),
                is_key_entry: true,
            },
            KeystoreEntryView {
                alias: "vertrauensanker".into(),
                subject_cn: Some("Test Root CA (NUR TEST)".into()),
                not_after: Some("2040-01-01T00:00:00Z".into()),
                is_key_entry: false,
            },
            KeystoreEntryView { alias: "ohne-name".into(), subject_cn: None, not_after: None, is_key_entry: true },
        ]),
        keystore_inspect_problem: None,
        read_only: false,
        preview_revision: 3,
        applied_revision: 12,
        examples: BTreeMap::from([("output.postfix".to_owned(), "Vertrag_signed.pdf".to_owned())]),
        // Nachtrag K3: per Richtlinie festgelegt (Pfad bzw. ganzer Abschnitt → Grund)
        locked: Some(BTreeMap::from([
            ("general.recent_documents".to_owned(), "ng-recent-policy".to_owned()),
            ("proxy".to_owned(), "ng-settings-policy".to_owned()),
        ])),
        emblem_placeholder: false,
    });
    with_settings.language = Lang::En;
    // eigener Datenschutztext per Richtlinie (Einstellungen › Datenschutz, ADR 0003)
    with_settings.privacy_text = Some("Eigener Datenschutztext der Organisation".into());
    out.push(("settings_open", with_settings));
    let mut defaults = view(StepView::Open { queue: vec![] }, bar(["AE", "", "", ""], false));
    defaults.settings = Some(SettingsView {
        settings: Settings::default(),
        problems: BTreeMap::new(),
        keychain_available: false,
        fido2_available: true,
        keystore_ready: false,
        keystore_format: Some(KeystoreFormatView::Jks),
        keystore_entries: None,
        keystore_inspect_problem: Some("keystore-wrong-password".into()),
        read_only: true,
        preview_revision: 0,
        applied_revision: 0,
        examples: BTreeMap::new(),
        locked: None,
        emblem_placeholder: false,
    });
    out.push(("settings_defaults", defaults));

    // Mobile (Plan §5.2): Plattform, FIDO2-Hinweis, „Signatur fortsetzen“; das Ergebnis des PKCS#12-Imports setzt
    // der Kern seit ADR 0004 Nachtrag 11 nur noch am Desktop (auf iOS/Android nur ID Austria)
    let android = PlatformView { os: PlatformOs::Android, mobile: true };
    let mut mobile_caps = caps_native();
    mobile_caps.factors.fido2 = false;
    let mut mobile_backend = view(
        StepView::Backend {
            options: vec![BackendOptionView {
                id: BackendId::AtrustNative,
                available: true,
                capabilities: Some(mobile_caps),
                notes: vec!["ng-fido2-unavailable-mobile".into()],
            }],
        },
        bar(["AE", "AE", "A", ""], true),
    );
    mobile_backend.platform = android;
    out.push(("mobile_backend", mobile_backend));
    let mut resume = view(StepView::Open { queue: vec![] }, bar(["AE", "", "", ""], true));
    resume.platform = PlatformView { os: PlatformOs::Ios, mobile: true };
    resume.resume = Some(ResumeOfferView {
        name: "Vertrag.pdf".into(),
        backend: BackendId::AtrustNative,
        age_minutes: 3,
        pending: 1,
        document_leaves_device: true,
    });
    out.push(("mobile_resume_offer", resume));
    let mut imported = view(StepView::Open { queue: vec![] }, bar(["AE", "", "", ""], true));
    imported.keystore_import = Some(KeystoreImportView {
        file_name: "test.p12".into(),
        ok: true,
        code: "ng-keystore-imported".into(),
        password_stored: true,
    });
    out.push(("keystore_imported", imported));
    out.extend(workspace_samples());
    out.extend(about_samples());
    out
}

// ------------------------------------------------------------------------------------------------
// Nachtrag 21: „Über PIDA“ und Update-Prüfung – jeder Zustand, dazu Mobile ohne Prüfung
// ------------------------------------------------------------------------------------------------

fn about(update: Option<UpdateCheckView>) -> AboutView {
    AboutView {
        version: "0.2.0".into(),
        release_date: "2026-10-17".into(),
        release_notes_url: "https://github.com/a-sit/pida/releases/tag/v0.2.0".into(),
        source_url: "https://github.com/a-sit/pida".into(),
        update,
    }
}

fn update_view(state: UpdateCheckState) -> UpdateCheckView {
    let checked = matches!(state, UpdateCheckState::UpToDate | UpdateCheckState::Available | UpdateCheckState::Failed);
    UpdateCheckView {
        state,
        latest_version: match state {
            UpdateCheckState::UpToDate => Some("0.2.0".into()),
            UpdateCheckState::Available => Some("0.3.0".into()),
            _ => None,
        },
        release_url: (state == UpdateCheckState::Available)
            .then(|| "https://github.com/a-sit/pida/releases/tag/v0.3.0".into()),
        checked_at: checked.then(|| "2026-10-17T10:42:17+02:00".into()),
        reason: (state == UpdateCheckState::Failed).then(|| "ng-update-failed-rate-limited".into()),
    }
}

fn about_samples() -> Vec<(&'static str, ViewState)> {
    let mut out = Vec::new();
    for (name, state) in [
        ("about_unchecked", UpdateCheckState::Unchecked),
        ("about_disabled", UpdateCheckState::Disabled),
        ("about_policy_off", UpdateCheckState::PolicyOff),
        ("about_checking", UpdateCheckState::Checking),
        ("about_up_to_date", UpdateCheckState::UpToDate),
        ("about_available", UpdateCheckState::Available),
        ("about_failed", UpdateCheckState::Failed),
    ] {
        let mut v = view(StepView::Open { queue: vec![] }, bar(["AE", "", "", ""], true));
        v.about = Some(about(Some(update_view(state))));
        out.push((name, v));
    }
    // iOS/Android: keine Update-Prüfung (keine Verteilung über GitHub)
    let mut mobile = view(StepView::Open { queue: vec![] }, bar(["AE", "", "", ""], true));
    mobile.platform = PlatformView { os: PlatformOs::Ios, mobile: true };
    mobile.signers = mobile_signers();
    mobile.about = Some(about(None));
    out.push(("about_mobile", mobile));
    out
}

// ------------------------------------------------------------------------------------------------
// ADR 0004 (neue Oberfläche „Arbeitsbereich“, Vertrag E0): je neuem Typ und jeder Variante eine Probe
// ------------------------------------------------------------------------------------------------

fn rect(left: f32, bottom: f32, right: f32, top: f32) -> RectPt {
    RectPt { left, bottom, right, top }
}

fn doc_ref() -> DocRefView {
    DocRefView { id: DocId(7), name: "Vertrag.pdf".into() }
}

fn ws_actions(sign: bool) -> ActionsView {
    ActionsView {
        open_files: !sign,
        sign,
        edit_placement: sign,
        start_batch: false,
        show_batch: true,
        settings: true,
        home: true,
    }
}

fn signers() -> SignersView {
    SignersView {
        options: vec![
            SignerView {
                backend: BackendId::AtrustNative,
                available: true,
                account: Some("+43 660 ···123".into()),
                capabilities: Some(caps_native()),
                notes: Vec::new(),
                unavailable_reason: None,
                batch_flow: Some(BatchFlowView::LoginThenConfirmEach),
            },
            SignerView {
                backend: BackendId::Keystore,
                available: false,
                account: Some("max.p12 · signer".into()),
                capabilities: None,
                notes: Vec::new(),
                unavailable_reason: Some("ng-signer-keystore-missing".into()),
                batch_flow: None,
            },
            SignerView {
                backend: BackendId::LocalBku,
                available: false,
                account: None,
                capabilities: None,
                notes: vec!["ng-fido2-unavailable-mobile".into()],
                unavailable_reason: Some("ng-signer-localbku-not-running".into()),
                batch_flow: None,
            },
        ],
        selected: Some(BackendId::AtrustNative),
        default: Some(BackendId::AtrustNative),
        choice_offered: true,
    }
}

/// Signiermittel auf iOS/Android: nur ID Austria, FIDO2 mit Hinweis (ADR 0004 Nachtrag 11) – keine Auswahl,
/// kein Konto-Chip (Nachtrag 20).
fn mobile_signers() -> SignersView {
    let mut caps = caps_native();
    caps.factors.fido2 = false;
    SignersView {
        options: vec![SignerView {
            backend: BackendId::AtrustNative,
            available: true,
            account: Some("+43 660 ···123".into()),
            capabilities: Some(caps),
            notes: vec!["ng-fido2-unavailable-mobile".into()],
            unavailable_reason: None,
            batch_flow: Some(BatchFlowView::LoginThenConfirmEach),
        }],
        selected: Some(BackendId::AtrustNative),
        default: Some(BackendId::AtrustNative),
        choice_offered: false,
    }
}

/// Alle Einrastlinien (beide Achsen, jede Art einmal).
fn guides() -> Vec<GuideView> {
    use GuideKind::*;
    [
        (GuideAxis::X, 56.5, ContentLeft),
        (GuideAxis::X, 538.0, ContentRight),
        (GuideAxis::Y, 785.0, ContentTop),
        (GuideAxis::Y, 410.0, ContentBottom),
        (GuideAxis::X, 297.5, CenterX),
        (GuideAxis::Y, 421.0, CenterY),
        (GuideAxis::Y, 392.0, BelowContent),
        (GuideAxis::X, 36.0, MarginLeft),
        (GuideAxis::X, 559.5, MarginRight),
        (GuideAxis::Y, 806.0, MarginTop),
        (GuideAxis::Y, 36.0, MarginBottom),
    ]
    .into_iter()
    .map(|(axis, at, kind)| GuideView { axis, at, kind })
    .collect()
}

fn document(phase: DocumentPhase, origin: PlacementOrigin, target: PlacementTarget) -> DocumentView {
    let mut announce = BTreeMap::new();
    for (k, v) in [("page", "2"), ("pages", "2"), ("left", "105"), ("top", "74"), ("bottom", "45")] {
        announce.insert(k.to_owned(), v.to_owned());
    }
    DocumentView {
        doc: DocInfo { signed: true, ..doc() },
        size_bytes: 248_512,
        phase,
        revision: 4,
        block: BlockView {
            width: 232.5,
            height: 82.0,
            preview_revision: 3,
            emblem_placeholder: false,
            emblem_choosable: true,
            language: pida_i18n::Lang::De,
        },
        placement: Placement { page_index: 1, x: 297.5, y: 210.5, width: 232.5, height: 82.0 },
        origin,
        target,
        invisible: false,
        new_page: false,
        sticker: Some(rect(297.5, 128.5, 530.0, 210.5)),
        fields: vec![
            FieldTargetView {
                index: 0,
                name: "Unterschrift.Auftraggeber".into(),
                page: Some(1),
                rect: Some(rect(56.0, 100.0, 256.0, 160.0)),
                selectable: true,
                // Nachtrag 16: geeignet, Block proportional und mittig im Feld
                suitable: true,
                block: Some(rect(71.0, 100.0, 241.0, 160.0)),
            },
            FieldTargetView {
                index: 1,
                name: "Gesperrt".into(),
                page: None,
                rect: None,
                selectable: false,
                suitable: false,
                block: None,
            },
        ],
        qr: Some(QrTargetView { page: 1, rect: rect(400.0, 40.0, 480.0, 120.0), id: Some("1".into()) }),
        existing: vec![
            ExistingSignatureView {
                field_name: "Signatur1".into(),
                signer: Some("Max Mustermann".into()),
                signing_time: Some("2026-10-14T09:30:00+02:00".into()),
                status: PriorStatusView::Valid,
                page: Some(0),
                rect: Some(rect(300.0, 60.0, 530.0, 140.0)),
            },
            ExistingSignatureView {
                field_name: "Signatur2".into(),
                signer: None,
                signing_time: None,
                status: PriorStatusView::Invalid,
                page: None,
                rect: None,
            },
            ExistingSignatureView {
                field_name: "Zeitstempel".into(),
                signer: None,
                signing_time: None,
                status: PriorStatusView::NotVerifiable,
                page: None,
                rect: None,
            },
        ],
        constraints: DocConstraintsView {
            new_page_allowed: false,
            visible_allowed: true,
            new_visible_field_allowed: false,
            reasons: vec!["ng-constraint-signed".into(), "ng-constraint-certified".into()],
        },
        layout: vec![
            PageLayoutView { page: 0, content: None, free: vec![rect(36.0, 36.0, 559.5, 806.0)], guides: Vec::new() },
            PageLayoutView {
                page: 1,
                content: Some(rect(56.5, 410.0, 538.0, 785.0)),
                free: vec![rect(297.5, 36.0, 559.5, 404.0), rect(36.0, 36.0, 290.0, 404.0)],
                guides: guides(),
            },
        ],
        check: PlacementCheckView {
            for_revision: 4,
            overlaps_content: Some(true),
            overlaps_widget: false,
            clamped: true,
            placeholder_uncovered: false,
        },
        announce,
        // Nachtrag K3 (Einstellungen › Dokument)
        detection: Some(DocumentDetectionView {
            pdfa: Some(PdfaIdView { part: 2, conformance: Some("B".into()) }),
            pdfa_decision: PdfaDecision::Detected,
            auto_placed: origin != PlacementOrigin::User,
        }),
        placement_locked: false,
        // Nachtrag 16: Größe einer neuen Seite (MediaBox der letzten Seite)
        new_page_size: Some((595.28, 841.89)),
    }
}

fn progress(stage: SigningStage, method: Option<ConfirmMethod>, certificate: CertificateSource) -> SigningProgressView {
    SigningProgressView {
        backend: Some(BackendId::AtrustNative),
        stage,
        method,
        doc: Some(doc_ref()),
        index: 1,
        total: 1,
        login_done: true,
        confirmations_done: 0,
        confirmations_total: 1,
        certificate,
        batch: false,
        pause: None,
    }
}

fn signed_summary() -> SignedSummaryView {
    SignedSummaryView {
        file_name: "Vertrag_signed.pdf".into(),
        folder_display: Some("/Users/demo/Dokumente".into()),
        signer: Some("Max Mustermann".into()),
        signing_time: "2026-10-15T10:12:00+02:00".into(),
        backend: BackendId::AtrustNative,
        qualification: Qualification::Qes,
        self_check: true,
        page: Some(1),
    }
}

fn batch_item(doc: u64, name: &str, status: BatchItemStatusView, position: PositionSummaryView) -> BatchItemView {
    BatchItemView {
        doc: DocId(doc),
        name: name.into(),
        page_count: Some(3),
        size_bytes: Some(120_000),
        status,
        position,
        target_name: Some(name.replace(".pdf", "_signed.pdf")),
        target_exists: false,
        thumbnail: true,
    }
}

fn batch(mode: OutputModeView) -> BatchView {
    use BatchItemStatusView as S;
    let mut failed = BTreeMap::new();
    failed.insert("detail".to_owned(), "413".to_owned());
    let mut items = vec![
        batch_item(1, "a.pdf", S::Analyzing, PositionSummaryView::Auto),
        batch_item(2, "b.pdf", S::Ready, PositionSummaryView::Manual { page: 2 }),
        batch_item(3, "c.pdf", S::Positioned, PositionSummaryView::Field { name: "Unterschrift".into() }),
        batch_item(4, "d.pdf", S::Rejected { code: "error-PDFPwdProtected".into() }, PositionSummaryView::Qr),
        batch_item(
            5,
            "e.pdf",
            S::NeedsReview { codes: vec!["pdf-usage-rights".into()] },
            PositionSummaryView::Invisible,
        ),
        batch_item(6, "f.pdf", S::Queued, PositionSummaryView::Auto),
        batch_item(7, "g.pdf", S::Active { stage: SigningStage::Confirming }, PositionSummaryView::Auto),
        batch_item(
            8,
            "h.pdf",
            S::Signed { path: PathBuf::from("/Users/demo/h_signed.pdf") },
            PositionSummaryView::Auto,
        ),
        batch_item(
            9,
            "i.pdf",
            S::Failed { code: "backend-document-too-large".into(), params: failed },
            PositionSummaryView::Auto,
        ),
        batch_item(10, "j.pdf", S::Skipped { existing: true }, PositionSummaryView::Auto),
        batch_item(11, "k.pdf", S::Skipped { existing: false }, PositionSummaryView::Auto),
        batch_item(12, "l.pdf", S::Cancelled, PositionSummaryView::Auto),
    ];
    items[0].page_count = None;
    items[0].size_bytes = None;
    items[0].target_name = None;
    items[0].thumbnail = false;
    items[9].target_exists = true;
    let mobile = mode == OutputModeView::AppDocuments;
    BatchView {
        items,
        open_item: Some(DocId(3)),
        running: false,
        summary: Some(BatchSummaryView {
            total: 12,
            signed: 1,
            rejected: 1,
            failed: 1,
            skipped: 2,
            cancelled: 1,
            not_started: 6,
            folder_display: (!mobile).then(|| "/Users/demo".into()),
            aborted: true,
        }),
        output: BatchOutputView {
            mode,
            folder_display: matches!(mode, OutputModeView::Folder).then(|| "/Users/demo/Signiert".into()),
            needs_folder: mode == OutputModeView::Ask,
        },
        estimate: Some(BatchEstimateView {
            backend: BackendId::AtrustNative,
            documents: 3,
            logins: 1,
            confirmations: 3,
            certificate_confirmation_possible: true,
            large: false,
        }),
        auto_position_setting: false,
        auto_position_once: false,
        manual_plans: 0,
    }
}

fn workspace_samples() -> Vec<(&'static str, ViewState)> {
    let mut out: Vec<(&'static str, ViewState)> = Vec::new();
    let position_bar = || bar(["AE", "A", "", ""], true);
    let position_step = || {
        let mut announce = BTreeMap::new();
        announce.insert("page".to_owned(), "2".to_owned());
        StepView::Position {
            doc: doc(),
            page: 1,
            block: BlockView {
                width: 232.5,
                height: 82.0,
                preview_revision: 3,
                emblem_placeholder: false,
                emblem_choosable: true,
                language: pida_i18n::Lang::De,
            },
            placement: Placement { page_index: 1, x: 297.5, y: 210.5, width: 232.5, height: 82.0 },
            new_page: false,
            announce,
        }
    };

    // Arbeitsbereich: Phasen, Ziele und Herkunft der Platzierung, Konto-Chip, Freigaben
    let mut editing = view(position_step(), position_bar());
    editing.document = Some(document(DocumentPhase::Editing, PlacementOrigin::Auto, PlacementTarget::Free));
    editing.signers = signers();
    editing.actions = ws_actions(true);
    // iOS nach dem Empfang über „Teilen“ (Nachtrag 12): Hinweis, dass nur das erste geteilte PDF ankommt
    let mut shared = editing.clone();
    shared.platform = PlatformView { os: PlatformOs::Ios, mobile: true };
    shared.share_hint = Some(ShareHintView { doc: DocId(7) });
    // auf iOS nur ID Austria (Nachtrag 11)
    shared.signers = mobile_signers();
    out.push(("ws_editing", editing));
    out.push(("ws_share_hint_ios", shared));

    // Einstellung „Automatische Positionierung“ (Nachtrag 8): automatisch platziert und gesperrt
    let mut locked = view(position_step(), position_bar());
    let mut d = document(DocumentPhase::Editing, PlacementOrigin::Auto, PlacementTarget::Free);
    d.placement_locked = true;
    d.check = PlacementCheckView {
        for_revision: 4,
        overlaps_content: Some(false),
        overlaps_widget: false,
        clamped: false,
        placeholder_uncovered: false,
    };
    locked.document = Some(d);
    locked.signers = signers();
    locked.actions = ws_actions(true);
    out.push(("ws_editing_locked", locked));

    let mut field = view(position_step(), position_bar());
    let mut d = document(DocumentPhase::Editing, PlacementOrigin::Field, PlacementTarget::Field { index: 0 });
    d.check = PlacementCheckView {
        for_revision: 3,
        overlaps_content: None,
        overlaps_widget: true,
        clamped: false,
        placeholder_uncovered: false,
    };
    // Platzhaltersuche „Leere Signaturfelder“: automatisch im ersten Feld; PDF/A erzwungen (ohne Kennung)
    d.detection = Some(DocumentDetectionView { pdfa: None, pdfa_decision: PdfaDecision::Forced, auto_placed: true });
    field.document = Some(d);
    field.signers = SignersView { selected: None, default: None, ..signers() };
    field.actions = ws_actions(true);
    out.push(("ws_field_no_signer", field));

    let mut invisible = view(position_step(), position_bar());
    let mut d = document(DocumentPhase::Editing, PlacementOrigin::User, PlacementTarget::Free);
    d.invisible = true;
    d.new_page = true;
    d.sticker = None;
    d.layout = Vec::new();
    d.fields = Vec::new();
    d.qr = None;
    d.existing = Vec::new();
    d.constraints = DocConstraintsView {
        visible_allowed: false,
        reasons: vec!["ng-constraint-invisible-profile".into()],
        ..DocConstraintsView::default()
    };
    d.check = PlacementCheckView::default();
    // ohne Erkennung (älterer Kern): `null`
    d.detection = None;
    invisible.document = Some(d);
    invisible.actions = ws_actions(true);
    out.push(("ws_invisible", invisible));

    let mut warning =
        view(StepView::Warning { doc: doc(), codes: vec!["pdf-usage-rights".into()] }, bar(["AE", "", "", ""], true));
    let mut d = document(DocumentPhase::Warning, PlacementOrigin::Auto, PlacementTarget::Free);
    d.detection = Some(DocumentDetectionView { pdfa: None, pdfa_decision: PdfaDecision::None, auto_placed: true });
    warning.document = Some(d);
    warning.actions = ws_actions(false);
    out.push(("ws_warning", warning));

    // Signierkarte: jede Phase, jeder Bestätigungsweg, jede Zertifikatsherkunft
    use CertificateSource as C;
    use ConfirmMethod as M;
    use SigningStage as St;
    for (name, stage, method, cert) in [
        ("ws_signing_connecting", St::Connecting, None, C::Read),
        ("ws_signing_login", St::Login, None, C::Read),
        ("ws_signing_certificate", St::Certificate, None, C::Read),
        ("ws_signing_preparing", St::Preparing, None, C::Cache),
        ("ws_signing_confirming_app", St::Confirming, Some(M::App), C::Session),
        ("ws_signing_confirming_qr", St::Confirming, Some(M::Qr), C::Session),
        ("ws_signing_confirming_sms", St::Confirming, Some(M::Sms), C::Session),
        ("ws_signing_confirming_fido2", St::Confirming, Some(M::Fido2), C::Session),
        ("ws_signing_confirming_password", St::Confirming, Some(M::Password), C::NotNeeded),
        ("ws_signing_confirming_card", St::Confirming, Some(M::Card), C::NotNeeded),
        ("ws_signing_finishing", St::Finishing, None, C::Session),
        ("ws_signing_saving", St::Saving, None, C::Session),
    ] {
        let mut v = signing(InteractionView::Waiting { progress: Progress::Signing });
        if let StepView::Signing { progress: p, .. } = &mut v.step {
            *p = progress(stage, method, cert);
        }
        v.document = Some(document(DocumentPhase::Signing, PlacementOrigin::Qr, PlacementTarget::Qr));
        out.push((name, v));
    }

    // Stapelpausen (Dokument, Sitzung mit/ohne neue Sitzung)
    for (name, scope, doc, new_session) in [
        ("ws_batch_pause_document", PauseScope::Document, Some(doc_ref()), false),
        ("ws_batch_pause_connection", PauseScope::Session, Some(doc_ref()), false),
        ("ws_batch_pause_session", PauseScope::Session, None, true),
    ] {
        let mut v = signing(InteractionView::Waiting { progress: Progress::Document { index: 2, total: 3 } });
        let mut params = BTreeMap::new();
        params.insert("detail".to_owned(), "timeout".to_owned());
        if let StepView::Signing { progress: p, .. } = &mut v.step {
            *p = SigningProgressView {
                index: 2,
                total: 3,
                confirmations_done: 1,
                confirmations_total: 3,
                batch: true,
                pause: Some(BatchPauseView {
                    doc,
                    code: "backend-network".into(),
                    params,
                    scope,
                    can_retry: true,
                    new_session,
                }),
                ..progress(St::Paused, None, C::Session)
            };
        }
        v.batch = Some(BatchView { running: true, summary: None, ..batch(OutputModeView::NextToSource) });
        out.push((name, v));
    }

    // Fehler mit Kontext (jede Herkunft)
    for (name, origin) in [
        ("ws_error_load", ErrorOrigin::Load),
        ("ws_error_analysis", ErrorOrigin::Analysis),
        ("ws_error_prepare", ErrorOrigin::Prepare),
        ("ws_error_signing", ErrorOrigin::Signing),
        ("ws_error_output", ErrorOrigin::Output),
        ("ws_error_config", ErrorOrigin::Config),
        ("ws_error_other", ErrorOrigin::Other),
    ] {
        let signing_error = origin == ErrorOrigin::Signing;
        let mut v = view(
            StepView::Error {
                code: if signing_error { "ng-signing-declined".into() } else { "error-Unexpected".into() },
                params: BTreeMap::new(),
                retry: signing_error,
                context: ErrorContextView {
                    origin,
                    occurred_at: signing_error.then(|| "2026-10-15T10:12:00+02:00".into()),
                    backend: signing_error.then_some(BackendId::AtrustNative),
                    doc: signing_error.then(doc_ref),
                },
            },
            bar(["AE", "", "", ""], true),
        );
        if signing_error {
            v.document = Some(document(DocumentPhase::Signing, PlacementOrigin::Auto, PlacementTarget::Free));
        }
        out.push((name, v));
    }

    // Fertig mit Zusammenfassung (Desktop und Mobile)
    for (name, folder, qualification) in
        [("ws_done", Some("/Users/demo/Dokumente"), Qualification::Qes), ("ws_done_mobile", None, Qualification::AdES)]
    {
        let mut v = view(
            StepView::Output {
                result: OutputView {
                    path: Some(PathBuf::from("/Users/demo/Dokumente/Vertrag_signed.pdf")),
                    save_pending: false,
                    ask_overwrite: None,
                    save_dialog: None,
                    summary: Some(SignedSummaryView {
                        folder_display: folder.map(str::to_owned),
                        qualification,
                        signer: folder.map(|_| "Max Mustermann".to_owned()),
                        page: folder.map(|_| 1),
                        ..signed_summary()
                    }),
                },
                next: Some(doc()),
                pending: 1,
            },
            bar(["AE", "AE", "AE", "A"], true),
        );
        v.document = Some(document(DocumentPhase::Done, PlacementOrigin::User, PlacementTarget::Free));
        if folder.is_none() {
            v.platform = PlatformView { os: PlatformOs::Ios, mobile: true };
        }
        out.push((name, v));
    }

    // Stapelübersicht (jede Ausgabeart)
    for (name, mode) in [
        ("ws_batch", OutputModeView::NextToSource),
        ("ws_batch_folder", OutputModeView::Folder),
        ("ws_batch_ask", OutputModeView::Ask),
        ("ws_batch_mobile", OutputModeView::AppDocuments),
    ] {
        let mut v = view(StepView::Batch, bar(["AE", "", "", ""], true));
        v.batch = Some(batch(mode));
        v.signers = signers();
        v.actions = ActionsView { start_batch: true, ..ws_actions(false) };
        if mode == OutputModeView::AppDocuments {
            v.platform = PlatformView { os: PlatformOs::Android, mobile: true };
            v.signers = mobile_signers();
        }
        out.push((name, v));
    }
    let mut empty = view(StepView::Batch, bar(["AE", "", "", ""], true));
    empty.batch = Some(BatchView {
        items: Vec::new(),
        open_item: None,
        running: false,
        summary: None,
        output: BatchOutputView { mode: OutputModeView::Ask, folder_display: None, needs_folder: true },
        estimate: None,
        auto_position_setting: false,
        auto_position_once: false,
        manual_plans: 0,
    });
    out.push(("ws_batch_empty", empty));

    // Nachtrag 8: „Für diesen Stapel automatisch positionieren“ (ein Eintrag von Hand platziert) bzw. laut
    // Einstellung
    for (name, setting, once) in [("ws_batch_auto_once", false, true), ("ws_batch_auto_setting", true, false)] {
        let mut v = view(StepView::Batch, bar(["AE", "", "", ""], true));
        v.batch = Some(BatchView {
            auto_position_setting: setting,
            auto_position_once: once,
            manual_plans: 1,
            ..batch(OutputModeView::NextToSource)
        });
        v.signers = signers();
        v.actions = ActionsView { start_batch: true, ..ws_actions(false) };
        out.push((name, v));
    }

    // „Zuletzt signiert“ (an, leer) und Ersteinrichtung
    let mut recent = view(StepView::Open { queue: vec![] }, bar(["AE", "", "", ""], true));
    recent.recent = Some(vec![
        RecentItemView {
            id: 2,
            file_name: "Vertrag_signed.pdf".into(),
            folder_display: Some("/Users/demo/Dokumente".into()),
            signed_at: "2026-10-15T10:12:00+02:00".into(),
            exists: true,
        },
        RecentItemView {
            id: 1,
            file_name: "Angebot_signed.pdf".into(),
            folder_display: None,
            signed_at: "2026-10-14T16:01:00+02:00".into(),
            exists: false,
        },
    ]);
    recent.actions = ws_actions(false);
    out.push(("ws_recent", recent));
    let mut recent_empty = view(StepView::Open { queue: vec![] }, bar(["AE", "", "", ""], true));
    recent_empty.recent = Some(Vec::new());
    out.push(("ws_recent_empty", recent_empty));
    // geparkte Arbeit (ADR 0004 Nachtrag 10): Startansicht mit Karte „In Bearbeitung“
    let mut parked = view(StepView::Open { queue: vec![] }, bar(["AE", "", "", ""], true));
    parked.actions = ActionsView { show_batch: false, ..ws_actions(false) };
    parked.parked = Some(ParkedWorkView {
        doc_name: Some("Vertrag.pdf".into()),
        documents: 1,
        placed_by_hand: true,
        unsaved: false,
    });
    out.push(("ws_start_parked", parked));
    let mut parked_batch = view(StepView::Open { queue: vec![] }, bar(["AE", "", "", ""], true));
    parked_batch.actions = ActionsView { show_batch: false, ..ws_actions(false) };
    parked_batch.parked = Some(ParkedWorkView { doc_name: None, documents: 4, placed_by_hand: false, unsaved: false });
    out.push(("ws_start_parked_batch", parked_batch));

    let offer = MigrationOfferView {
        legacy_file: PathBuf::from("/Users/demo/.pdf-over/PDF-Over.config"),
        plaintext_secrets: false,
        items: [
            ("ng-migration-item-block", None),
            ("ng-migration-item-emblem", Some("logo.png")),
            ("ng-migration-item-username", None),
            ("ng-migration-item-output", None),
            ("ng-migration-item-keystore", Some("max.p12")),
            ("ng-migration-item-proxy", None),
            ("ng-migration-item-placeholder", None),
            ("ng-migration-item-language", None),
        ]
        .into_iter()
        .map(|(code, detail)| MigrationItemView { code: code.into(), detail: detail.map(str::to_owned) })
        .collect(),
    };
    // Ersteinrichtung am Desktop: die Signiermittel stehen schon fest (Schritt „Standard-Signiermittel“ nur mit
    // Auswahl, Nachtrag 20)
    let mut first_run = view(StepView::Open { queue: vec![] }, bar(["AE", "", "", ""], true));
    first_run.signers = SignersView { selected: None, default: None, ..signers() };
    first_run.migration = Some(offer.clone());
    first_run.first_run = Some(FirstRunView {
        migration: Some(offer),
        default_backend: Some(pida_config::BackendChoice::IdAustria),
        profile: pida_config::SignatureProfile::Amtssignaturblock,
        block_language: Some(Lang::En),
        preview_revision: 2,
    });
    out.push(("ws_first_run", first_run));
    let mut first_run_plain = view(StepView::Open { queue: vec![] }, bar(["AE", "", "", ""], true));
    first_run_plain.signers = SignersView { selected: None, default: None, ..signers() };
    first_run_plain.first_run = Some(FirstRunView {
        migration: None,
        default_backend: None,
        profile: pida_config::SignatureProfile::SignaturblockSmall,
        block_language: None,
        preview_revision: 0,
    });
    out.push(("ws_first_run_without_migration", first_run_plain));
    out
}

fn to_fixture() -> serde_json::Value {
    let list: Vec<serde_json::Value> = samples()
        .into_iter()
        .map(|(name, view)| serde_json::json!({ "name": name, "view": serde_json::to_value(&view).expect("serialisierbar") }))
        .collect();
    serde_json::Value::Array(list)
}

#[test]
fn view_states_match_the_typescript_fixture() {
    let actual = to_fixture();
    let text = serde_json::to_string_pretty(&actual).expect("json") + "\n";
    if std::env::var_os("PIDA_BLESS").is_some() {
        std::fs::write(VIEW_FIXTURE, &text).expect("Fixture schreiben");
    }
    let expected: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(VIEW_FIXTURE).expect("Fixture lesen (PIDA_BLESS=1 erzeugt sie)"))
            .expect("Fixture ist JSON");
    assert_eq!(
        actual, expected,
        "view-states.json ist veraltet: PIDA_BLESS=1 cargo test -p pida-app --test serde_contract"
    );
}

#[test]
fn view_states_round_trip() {
    for (name, view) in samples() {
        let json = serde_json::to_value(&view).expect("serialisierbar");
        let back: ViewState = serde_json::from_value(json).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(back, view, "{name}");
    }
}

#[test]
fn every_step_and_interaction_variant_is_covered() {
    let fixture = to_fixture();
    let list = fixture.as_array().expect("Liste");
    let steps: std::collections::BTreeSet<&str> =
        list.iter().filter_map(|s| s["view"]["step"]["step"].as_str()).collect();
    for step in [
        "starting",
        "open",
        "loading",
        "warning",
        "placeholder",
        "position",
        "backend",
        "signing",
        "output",
        "error",
        "finished",
        "batch",
    ] {
        assert!(steps.contains(step), "Schritt {step} fehlt");
    }
    let kinds: std::collections::BTreeSet<&str> =
        list.iter().filter_map(|s| s["view"]["step"]["interaction"]["kind"].as_str()).collect();
    for kind in ["waiting", "form", "secret", "notice"] {
        assert!(kinds.contains(kind), "Interaktion {kind} fehlt");
    }
}

/// Zahlen einheitlich als f64 (JavaScript schreibt `23`, serde `23.0` für f32/f64).
fn normalize(v: serde_json::Value) -> serde_json::Value {
    match v {
        serde_json::Value::Number(n) => serde_json::json!(n.as_f64().unwrap_or_default()),
        serde_json::Value::Array(a) => serde_json::Value::Array(a.into_iter().map(normalize).collect()),
        serde_json::Value::Object(o) => {
            serde_json::Value::Object(o.into_iter().map(|(k, v)| (k, normalize(v))).collect())
        }
        other => other,
    }
}

#[test]
fn typescript_events_deserialize_into_ui_event() {
    let text = std::fs::read_to_string(EVENT_FIXTURE).expect("ui-events.json lesen (npm test erzeugt sie)");
    let samples: Vec<serde_json::Value> = serde_json::from_str(&text).expect("JSON-Liste");
    assert!(samples.len() >= UI_EVENT_VARIANTS, "alle UiEvent-Varianten erwartet, gefunden {}", samples.len());
    let mut types = std::collections::BTreeSet::new();
    for sample in samples {
        let event: UiEvent = serde_json::from_value(sample.clone()).unwrap_or_else(|e| panic!("{sample}: {e}"));
        let back = serde_json::to_value(&event).expect("serialisierbar");
        assert_eq!(normalize(back), normalize(sample.clone()), "Rundlauf");
        types.insert(sample["type"].as_str().unwrap_or_default().to_owned());
        // Regel 9: Debug ohne Passwörter/TANs
        let debug = format!("{event:?}");
        assert!(!debug.contains("geheim-123"), "Debug enthält das Passwort: {debug}");
        assert!(!debug.contains("917253"), "Debug enthält die TAN: {debug}");
    }
    assert_eq!(types.len(), UI_EVENT_VARIANTS, "alle UiEvent-Varianten: {types:?}");
}

/// Anzahl der `UiEvent`-Varianten in den TypeScript-Typen (die TS-Beispiele müssen jede abdecken);
/// ohne das veraltete `acknowledge_privacy` (`#[ts(skip)]`, ADR 0003).
const UI_EVENT_VARIANTS: usize = 72;

/// Das veraltete `acknowledge_privacy` (früherer Datenschutzdialog) wird weiter gelesen – ältere Aufrufer
/// scheitern nicht –, steht aber nicht mehr in den TypeScript-Typen (ADR 0003).
#[test]
fn deprecated_acknowledge_privacy_still_deserializes() {
    for json in [
        serde_json::json!({ "type": "acknowledge_privacy", "accepted": true }),
        serde_json::json!({ "type": "acknowledge_privacy" }),
    ] {
        let event: UiEvent = serde_json::from_value(json.clone()).unwrap_or_else(|e| panic!("{json}: {e}"));
        assert!(matches!(event, UiEvent::AcknowledgePrivacy { .. }));
    }
    let generated = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../src/core/generated.ts"))
        .expect("generated.ts lesen");
    assert!(!generated.contains("acknowledge_privacy"), "veraltetes Ereignis in den TS-Typen");
}

#[test]
fn unknown_events_are_rejected() {
    let unknown = serde_json::json!({ "type": "frobnicate" });
    assert!(serde_json::from_value::<UiEvent>(unknown).is_err(), "unbekanntes Ereignis");
    let missing = serde_json::json!({ "type": "move_block", "page": 0 });
    assert!(serde_json::from_value::<UiEvent>(missing).is_err());
}
