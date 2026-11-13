/** Bausteine für Test-Zustände (serde-Format wie `pida-core-api`). */
import { defaultErrorContext, defaultSettings, defaultSigningProgress, mainBar, noActions, workspaceDefaults } from '../core/defaults';
import { mockKeystoreFormat } from '../core/mock/settings';
import { batchFlowFor, capabilitiesFor } from '../core/mock/shared';
import { fieldFit } from '../core/mock/workspace';
import type {
  ActionsView,
  BackendOptionView,
  BatchItemView,
  BatchView,
  DocInfo,
  DocumentView,
  ErrorContextView,
  FieldTargetView,
  PlatformView,
  FormContext,
  InteractionView,
  Lang,
  NativeForm,
  OutputView,
  QueueItemView,
  Settings,
  SettingsView,
  SignedSummaryView,
  SignersView,
  SigningProgressView,
  StepView,
  ViewState,
} from '../core/types';

export const DOC: DocInfo = {
  id: 7,
  name: 'Vertrag.pdf',
  page_count: 3,
  page_sizes: [
    [595.28, 841.89],
    [595.28, 841.89],
    [595.28, 841.89],
  ],
  signed: false,
};

export const QUEUE: QueueItemView[] = [
  { id: 7, name: 'Vertrag.pdf', current: true },
  { id: 8, name: 'Angebot.pdf', current: false },
];

const BARS: Record<StepView['step'], [string, string, string, string]> = {
  starting: ['', '', '', ''],
  open: ['AE', '', '', ''],
  loading: ['AE', '', '', ''],
  warning: ['AE', '', '', ''],
  placeholder: ['AE', '', '', ''],
  position: ['AE', 'A', '', ''],
  backend: ['AE', 'AE', 'A', ''],
  signing: ['A', 'A', 'A', ''],
  output: ['AE', 'AE', 'AE', 'A'],
  error: ['AE', '', '', ''],
  finished: ['', '', '', ''],
  // Stapelübersicht (ADR 0004 §3.8, Kern ab K2)
  batch: ['AE', '', '', ''],
};

/** Plattform der Tests (Standard: Desktop). */
export const DESKTOP: PlatformView = { os: 'macos', mobile: false };
export const IOS: PlatformView = { os: 'ios', mobile: true };
export const ANDROID: PlatformView = { os: 'android', mobile: true };

export function viewOf(step: StepView, extra: Partial<ViewState> = {}): ViewState {
  const settingsEnabled = !['starting', 'signing', 'finished'].includes(step.step);
  return {
    step,
    main_bar: mainBar(BARS[step.step], settingsEnabled),
    settings: null,
    language: 'de',
    notices: [],
    migration: null,
    done: [],
    platform: DESKTOP,
    resume: null,
    keystore_import: null,
    privacy_text: null,
    ...workspaceDefaults(),
    ...extra,
  };
}

/** Fehlerkontext des Kerns bis K1 (ADR 0004 §3.6). */
export const NO_ERROR_CONTEXT = defaultErrorContext();

/** Derselbe Zustand auf einer Mobile-Plattform. */
export function onMobile(view: ViewState, platform: PlatformView = IOS): ViewState {
  return { ...view, platform };
}

export function positionView(extra: Partial<Extract<StepView, { step: 'position' }>> = {}, lang: Lang = 'de'): ViewState {
  // Live-Region: linke Kante 297,64 pt ≈ 105 mm, Oberkante 210,47 pt ≈ 74 mm (wie das Feld Y)
  return viewOf(
    {
      step: 'position',
      doc: DOC,
      page: 2,
      block: { width: 232, height: 82, preview_revision: 1, emblem_placeholder: false, emblem_choosable: true, language: 'de' },
      placement: { page_index: 2, x: 297.64, y: 210.47, width: 232, height: 82 },
      new_page: false,
      announce: {},
      ...extra,
    },
    { language: lang },
  );
}

export function ctx(extra: Partial<FormContext> = {}): FormContext {
  return {
    signature_data_url: null,
    can_switch_to_sms: true,
    can_switch_to_fido2: true,
    fido2_visible: true,
    problem: null,
    ...extra,
  };
}

/** Signieren; `leaves` = das Dokument geht an A-Trust (Kurzhinweis, Standard: Masken von ID Austria). */
export function signingView(interaction: InteractionView, lang: Lang = 'de', leaves = interaction.kind !== 'secret'): ViewState {
  return viewOf(
    { step: 'signing', interaction, queue: QUEUE, document_leaves_device: leaves, progress: defaultSigningProgress() },
    { language: lang },
  );
}

export function formView(form: NativeForm, request = 1, lang: Lang = 'de'): ViewState {
  return signingView({ kind: 'form', request, form }, lang);
}

export function backendView(options: BackendOptionView[], extra: Partial<ViewState> = {}): ViewState {
  return viewOf({ step: 'backend', options }, extra);
}

export function settingsView(settings: Settings = defaultSettings(), extra: Partial<SettingsView> = {}, lang: Lang = 'de'): ViewState {
  return viewOf(
    { step: 'open', queue: [] },
    {
      language: lang,
      settings: {
        settings,
        problems: {},
        keychain_available: true,
        fido2_available: true,
        keystore_ready: settings.keystore.enabled && mockKeystoreFormat(settings.keystore.file) === 'pkcs12',
        keystore_format: mockKeystoreFormat(settings.keystore.file),
        keystore_entries: null,
        keystore_inspect_problem: null,
        read_only: false,
        preview_revision: 0,
        applied_revision: 0,
        examples: {},
        emblem_placeholder: settings.signature.profile === 'AMTSSIGNATURBLOCK' && !settings.signature.emblem,
        ...extra,
      },
      main_bar: mainBar(['AE', '', '', ''], false),
    },
  );
}

/** Ergebnis des Ausgabe-Schritts (ohne Rückfragen, sofern nicht angegeben). */
export function outputResult(extra: Partial<OutputView> = {}): OutputView {
  return { path: null, save_pending: false, ask_overwrite: null, save_dialog: null, summary: null, ...extra };
}

/* ---------------------------------------------------------------- Arbeitsbereich (ADR 0004) */

/**
 * Signiermittel wie im Kern (ID Austria mit gemerktem Benutzer, lokale BKU, Keystore); `mobile` = iOS/Android:
 * nur ID Austria (ADR 0004 Nachtrag 11) und damit keine Signiermittel-Auswahl (`choice_offered`, Nachtrag 20).
 */
export function signersView(extra: Partial<SignersView> = {}, mobile = false): SignersView {
  return {
    options: [
      {
        backend: 'atrust_native',
        available: true,
        account: '+43 660 ···123',
        capabilities: capabilitiesFor('atrust_native', mobile),
        notes: mobile ? ['ng-fido2-unavailable-mobile'] : [],
        unavailable_reason: null,
        batch_flow: 'login_then_confirm_each',
      },
      ...(mobile
        ? []
        : [
            { backend: 'local_bku' as const, available: true, account: null, capabilities: capabilitiesFor('local_bku'), notes: [], unavailable_reason: null, batch_flow: batchFlowFor(capabilitiesFor('local_bku')) },
            { backend: 'keystore' as const, available: true, account: 'max.p12 · max', capabilities: capabilitiesFor('keystore'), notes: [], unavailable_reason: null, batch_flow: 'unlock_once' as const },
          ]),
    ],
    selected: 'atrust_native',
    default: 'atrust_native',
    choice_offered: !mobile,
    ...extra,
  };
}

/**
 * Desktop mit Richtlinie, die nur ID Austria freigibt (`allowed_backends`, ADR 0004 Nachtrag 20): Chipkarte und
 * Keystore gesperrt mit Grund, keine Auswahl, ID Austria gewählt – auch bei „Jedes Mal fragen“ (`default` leer).
 */
export function singleSignerView(extra: Partial<SignersView> = {}): SignersView {
  const base = signersView();
  return {
    ...base,
    options: base.options.map((o) => (o.backend === 'atrust_native' ? o : { ...o, available: false, capabilities: null, batch_flow: null, unavailable_reason: 'ng-signer-policy' })),
    selected: 'atrust_native',
    default: null,
    choice_offered: false,
    ...extra,
  };
}

/** Freigaben im Arbeitsbereich (Positionieren). */
export function editActions(extra: Partial<ActionsView> = {}): ActionsView {
  return { ...noActions(), open_files: true, sign: true, edit_placement: true, settings: true, home: true, ...extra };
}

/**
 * Dokument im Arbeitsbereich: Vertrag mit 3 A4-Seiten, Block 230 × 82 pt auf Seite 3 (linke Kante 182,64 pt,
 * Oberkante 300 pt von unten), Layout der Seite 3 mit Inhaltsrahmen, freier Fläche und Einrastlinien.
 */
export function documentView(extra: Partial<DocumentView> = {}): DocumentView {
  const placement = { page_index: 2, x: 182.64, y: 300, width: 230, height: 82 };
  return {
    doc: DOC,
    size_bytes: 214 * 1024,
    phase: 'editing',
    revision: 1,
    block: { width: 230, height: 82, preview_revision: 1, emblem_placeholder: false, emblem_choosable: true, language: 'de' },
    placement,
    origin: 'auto',
    target: { kind: 'free' },
    invisible: false,
    new_page: false,
    sticker: { left: 182.64, right: 412.64, top: 300, bottom: 218 },
    fields: [],
    qr: null,
    existing: [],
    constraints: { new_page_allowed: true, visible_allowed: true, new_visible_field_allowed: true, reasons: [] },
    layout: [
      {
        page: 2,
        content: { left: 56, right: 539, top: 786, bottom: 340 },
        free: [{ left: 56, right: 539, top: 334, bottom: 36 }],
        guides: [
          { axis: 'x', at: 297.64, kind: 'center_x' },
          { axis: 'y', at: 420.95, kind: 'center_y' },
          { axis: 'x', at: 56, kind: 'content_left' },
          { axis: 'x', at: 539, kind: 'content_right' },
          { axis: 'y', at: 786, kind: 'content_top' },
          { axis: 'y', at: 340, kind: 'content_bottom' },
          { axis: 'y', at: 322, kind: 'below_content' },
        ],
      },
    ],
    check: { for_revision: 1, overlaps_content: false, overlaps_widget: false, clamped: false, placeholder_uncovered: false, overlaps_block: false },
    announce: { page: '3', pages: '3', left: '64', top: '106', bottom: '77' },
    placement_locked: false,
    ...extra,
  };
}

/**
 * Leeres Signaturfeld wie vom Kern (ADR 0004 Nachtrag 16): `suitable` und `block` (Lage des Blocks im Feld:
 * proportional, höchstens natürliche Größe, mittig) für den Block 230 × 82 pt aus [`documentView`] auf einer
 * A4-Seite, sofern nicht angegeben.
 */
export function fieldTarget(
  f: Omit<FieldTargetView, 'suitable' | 'block'> & Partial<Pick<FieldTargetView, 'suitable' | 'block'>>,
  block = { width: 230, height: 82 },
  page: [number, number] = [595.28, 841.89],
): FieldTargetView {
  const fit = f.rect ? fieldFit(f.rect, block, page) : null;
  return { suitable: fit?.suitable ?? false, block: fit?.rect ?? null, ...f };
}

/** Arbeitsbereich (Schritt `position` als Phasenmarker, Inhalt in `document`). */
export function workspaceView(doc: DocumentView = documentView(), extra: Partial<ViewState> = {}, lang: Lang = 'de'): ViewState {
  const p = doc.placement;
  return viewOf(
    { step: 'position', doc: doc.doc, page: p.page_index, block: doc.block, placement: p, new_page: doc.new_page, announce: { page: doc.announce.page ?? '1', pages: doc.announce.pages ?? '1', left: doc.announce.left ?? '0', top: doc.announce.top ?? '0' } },
    { language: lang, document: doc, signers: signersView(), actions: editActions(), ...extra },
  );
}

/** Fortschritt der Signierkarte (ID Austria, Einzeldokument). */
export function progress(extra: Partial<SigningProgressView> = {}): SigningProgressView {
  return { ...defaultSigningProgress(), backend: 'atrust_native', stage: 'login', doc: { id: DOC.id, name: DOC.name }, index: 1, total: 1, confirmations_total: 1, ...extra };
}

/** Signieren über dem Arbeitsbereich (Dokument in der Phase `signing`). */
export function cardView(interaction: InteractionView, p: Partial<SigningProgressView> = {}, extra: Partial<ViewState> = {}, lang: Lang = 'de'): ViewState {
  const backend = p.backend ?? 'atrust_native';
  return viewOf(
    { step: 'signing', interaction, queue: [QUEUE[0] as QueueItemView], document_leaves_device: backend === 'atrust_native', progress: progress(p) },
    { language: lang, document: documentView({ phase: 'signing' }), signers: signersView(), actions: { ...noActions() }, ...extra },
  );
}

/** Fehlerschritt in der Karte (Herkunft Signieren). */
export function cardErrorView(code: string, params: Record<string, string> = {}, retry = true, context: Partial<ErrorContextView> = {}): ViewState {
  return viewOf(
    { step: 'error', code, params, retry, context: { ...defaultErrorContext(), origin: 'signing', occurred_at: '2026-10-15T10:42:17+02:00', backend: 'atrust_native', doc: { id: DOC.id, name: DOC.name }, ...context } },
    { document: documentView({ phase: 'signing' }), signers: signersView(), actions: { ...noActions(), open_files: true, settings: true, home: true } },
  );
}

/** Zusammenfassung „Fertig“. */
export function summary(extra: Partial<SignedSummaryView> = {}): SignedSummaryView {
  return {
    file_name: 'Vertrag_signed.pdf',
    folder_display: '/Users/demo/Dokumente',
    signer: 'Max Mustermann',
    signing_time: '2026-10-15T10:42:17+02:00',
    backend: 'atrust_native',
    qualification: 'Qes',
    self_check: true,
    page: 2,
    ...extra,
  };
}

/** Fertig (gespeichert). */
export function doneView(result: Partial<OutputView> = {}, next: DocInfo | null = null, extra: Partial<ViewState> = {}, pending = next ? 1 : 0): ViewState {
  return viewOf(
    { step: 'output', result: outputResult({ path: '/Users/demo/Dokumente/Vertrag_signed.pdf', summary: summary(), ...result }), next, pending },
    { document: documentView({ phase: 'done' }), signers: signersView(), actions: { ...noActions(), open_files: true, settings: true, home: true }, ...extra },
  );
}

/* ---------------------------------------------------------------- Stapel (ADR 0004 §3.8) */

/** Eintrag der Stapelübersicht (Vertrag mit 3 Seiten, bereit). */
export function batchItem(extra: Partial<BatchItemView> = {}): BatchItemView {
  return {
    doc: 7,
    name: 'Vertrag.pdf',
    page_count: 3,
    size_bytes: 214 * 1024,
    status: { state: 'ready' },
    position: { kind: 'auto' },
    target_name: 'Vertrag_signed.pdf',
    target_exists: false,
    thumbnail: true,
    ...extra,
  };
}

/** Stapel wie im Abnahmekriterium E2.1: drei signierbar, einer kennwortgeschützt. */
export function batchData(extra: Partial<BatchView> = {}): BatchView {
  return {
    items: [
      batchItem({ doc: 1, name: 'Angebot.pdf', page_count: 2, size_bytes: 96 * 1024, target_name: 'Angebot_signed.pdf' }),
      batchItem({ doc: 2, name: 'Auftrag.pdf', page_count: 1, size_bytes: 74 * 1024, status: { state: 'positioned' }, position: { kind: 'qr' }, target_name: 'Auftrag_signed.pdf' }),
      batchItem({ doc: 3, name: 'Scan.pdf', page_count: null, size_bytes: 184 * 1024, status: { state: 'rejected', code: 'error-PDFPwdProtected' }, target_name: null, thumbnail: false }),
      batchItem({ doc: 4 }),
    ],
    open_item: null,
    running: false,
    summary: null,
    output: { mode: 'next_to_source', folder_display: null, needs_folder: false },
    estimate: { backend: 'atrust_native', documents: 3, logins: 1, confirmations: 3, certificate_confirmation_possible: true, large: false },
    auto_position_setting: false,
    auto_position_once: false,
    manual_plans: 0,
    ...extra,
  };
}

/** Stapelübersicht (`StepView::Batch`). */
export function batchOverview(batch: BatchView = batchData(), extra: Partial<ViewState> = {}): ViewState {
  return viewOf(
    { step: 'batch' },
    { batch, signers: signersView(), actions: { ...noActions(), open_files: true, start_batch: batch.items.some((i) => i.status.state === 'ready' || i.status.state === 'positioned'), settings: true, home: true }, ...extra },
  );
}
