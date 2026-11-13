/**
 * Vertragstest Oberfläche ↔ Kern (serde-Format):
 * - `fixtures/view-states.json` wird vom Rust-Test `apps/app/src-tauri/tests/serde_contract.rs` aus
 *   echten `pida_core_api::ViewState`-Werten erzeugt; hier wird jede Probe gegen ein Laufzeit-Schema
 *   geprüft, dessen Typ der Compiler mit `ViewState` aus `types.ts` gleichsetzt.
 * - `fixtures/ui-events.json` entsteht aus den typisierten TS-Beispielen; Rust liest sie als `UiEvent`.
 */
import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import {
  arr,
  bool,
  ext,
  int,
  lit,
  nullable,
  num,
  obj,
  objOpen,
  omittable,
  optional,
  record,
  str,
  tagged,
  tuple2,
  unknownValue,
  validate,
  type Infer,
  type Same,
} from '../test/schema';
import { UI_EVENT_EXTRA, UI_EVENT_SAMPLES } from './fixtures/ui-events';
import { defaultErrorContext, defaultSettings, defaultSigningProgress, workspaceDefaults } from './defaults';
import type { UiEvent, ViewState } from './types';

const lang = lit('de', 'en');
const step = lit('Open', 'Positioning', 'Signing', 'Finish');
const backendId = lit('atrust_native', 'atrust_browser', 'csc', 'local_bku', 'keystore', 'wallet');

const capabilities = obj({
  input: ext({ Digest: obj({ algs: arr(lit('Sha1', 'Sha256', 'Sha384', 'Sha512')) }) }, ['Document'] as const),
  output: lit('Cms', 'RawSignature'),
  certificate: lit('Required', 'OnlyForVisibleBlock', 'No'),
  interaction: lit('None', 'Secret', 'NativeForms', 'EmbeddedWeb', 'SystemBrowser', 'External', 'ExternalApp'),
  server: lit('None', 'DataUrlRelay', 'TokenProxy'),
  max_batch: int,
  login_once_per_batch: bool,
  document_leaves_device: bool,
  qualification: lit('Qes', 'AdES', 'Unknown'),
  factors: obj({ app: bool, sms: bool, qr: bool, fido2: bool, card: bool }),
  cms_reserve: nullable(int),
});

const formProblem = ext(
  { Fido2Failed: str, Atrust: str, Fido2Error: obj({ code: str, detail: str }) },
  ['PasswordTooShort', 'PasswordTooLong', 'NoTan', 'TanTooLong', 'EnteredReferenceValue', 'QrImageFailed', 'LoginRejected'] as const,
);

const formContext = obj({
  signature_data_url: nullable(str),
  can_switch_to_sms: bool,
  can_switch_to_fido2: bool,
  fido2_visible: bool,
  problem: nullable(formProblem),
});

const nativeForm = ext({
  Login: obj({ prefill_username: nullable(str), remember: bool, ctx: formContext }),
  SmsTan: obj({ reference_value: str, ctx: formContext }),
  QrCode: obj({
    reference_value: str,
    image: nullable(obj({ bytes: arr(int), mime: nullable(str) })),
    timeout_secs: nullable(int),
    ctx: formContext,
  }),
  WaitForApp: obj({ reference_value: nullable(str), timeout_secs: nullable(int), ctx: formContext }),
  Fido2: obj({ in_progress: bool, ctx: formContext }),
});

const progress = ext(
  { Document: obj({ index: int, total: int }) },
  ['Connecting', 'ReadingCertificate', 'WaitingForServer', 'Signing', 'Done'] as const,
);

const notice = obj({
  kind: lit('Info', 'Warning', 'Error'),
  code: ext(
    { Http: obj({ status: int }) },
    [
      'AtrustInterstitial',
      'AtrustError',
      'ProxyAuthRequired',
      'DocumentTooLarge',
      'ConnectionFailed',
      'LocalBkuNotReachable',
      'KeystoreFileMissing',
      'KeystoreAliasMissing',
      'KeystoreError',
      'KeystoreConversionNeeded',
    ] as const,
  ),
  text: nullable(str),
  buttons: lit('Ok', 'OkCancel', 'RetryCancel'),
});

const interaction = tagged('kind', {
  waiting: { progress },
  form: { request: int, form: nativeForm },
  secret: {
    request: int,
    prompt: obj({ kind: lit('KeystorePassword', 'KeyPassword', 'ProxyPassword', 'Fido2Pin'), subject: str, retry: bool }),
    // erste Verwendung des Keystores: „Passwort merken?“ (Nachtrag ADR 0004)
    ask_storage: bool,
    keychain_available: bool,
  },
  notice: { request: int, notice },
});

const docInfo = obj({ id: int, name: str, page_count: int, page_sizes: arr(tuple2(num, num)), signed: bool });
const queueItem = obj({ id: int, name: str, current: bool });

// --- ADR 0004 (neue Oberfläche „Arbeitsbereich“, Vertrag E0) -----------------------------------
const rectPt = obj({ left: num, bottom: num, right: num, top: num });
const docRef = obj({ id: int, name: str });
const signatureProfile = lit('SIGNATURBLOCK_SMALL', 'AMTSSIGNATURBLOCK', 'BASE_LOGO', 'INVISIBLE');
const backendChoice = lit('id_austria', 'local_bku', 'keystore');
const placementTarget = tagged('kind', { free: {}, field: { index: int }, qr: {} });
const signingStage = lit('connecting', 'login', 'certificate', 'preparing', 'confirming', 'finishing', 'saving', 'paused');
const guideKind = lit(
  'content_left',
  'content_right',
  'content_top',
  'content_bottom',
  'center_x',
  'center_y',
  'below_content',
  'margin_left',
  'margin_right',
  'margin_top',
  'margin_bottom',
);

const documentView = obj({
  doc: docInfo,
  size_bytes: int,
  phase: lit('warning', 'editing', 'signing', 'done'),
  revision: int,
  block: obj({ width: num, height: num, preview_revision: int, emblem_placeholder: bool, emblem_choosable: bool, language: lit('de', 'en') }),
  placement: obj({ page_index: int, x: num, y: num, width: num, height: num }),
  origin: lit('auto', 'user', 'field', 'qr'),
  target: placementTarget,
  invisible: bool,
  new_page: bool,
  sticker: nullable(rectPt),
  // Nachtrag 16: `suitable` (als Ziel angeboten) und `block` (Lage des Blocks im Feld)
  fields: arr(obj({ index: int, name: str, page: nullable(int), rect: nullable(rectPt), selectable: bool, suitable: bool, block: nullable(rectPt) })),
  qr: nullable(obj({ page: int, rect: rectPt, id: nullable(str) })),
  existing: arr(
    obj({
      field_name: str,
      signer: nullable(str),
      signing_time: nullable(str),
      status: lit('valid', 'invalid', 'not_verifiable'),
      page: nullable(int),
      rect: nullable(rectPt),
    }),
  ),
  constraints: obj({ new_page_allowed: bool, visible_allowed: bool, new_visible_field_allowed: bool, reasons: arr(str) }),
  layout: arr(
    obj({
      page: int,
      content: nullable(rectPt),
      free: arr(rectPt),
      guides: arr(obj({ axis: lit('x', 'y'), at: num, kind: guideKind })),
    }),
  ),
  check: obj({
    for_revision: int,
    overlaps_content: nullable(bool),
    overlaps_widget: bool,
    clamped: bool,
    placeholder_uncovered: bool,
    overlaps_block: bool,
  }),
  announce: record(str),
  // Nachtrag K3 (Einstellungen › Dokument): in TS optional, vom Kern immer geliefert
  detection: optional(
    obj({
      pdfa: nullable(obj({ part: int, conformance: nullable(str) })),
      pdfa_decision: lit('none', 'detected', 'forced'),
      auto_placed: bool,
    }),
  ),
  // Nachtrag 8 (Einstellung „Automatische Positionierung“)
  placement_locked: bool,
  // Nachtrag 16: Größe einer neuen Seite (MediaBox der letzten Seite); in TS optional
  new_page_size: optional(tuple2(num, num)),
  // Mehrfachsignatur (je Block eine eigene Signatur); in TS optional, vom Kern immer geliefert
  blocks: optional(
    obj({
      items: arr(obj({ index: int, active: bool, page: int, rect: nullable(rectPt), target: placementTarget })),
      limit: int,
      can_add: bool,
      free_targets: int,
    }),
  ),
});

const signersView = obj({
  options: arr(
    obj({
      backend: backendId,
      available: bool,
      account: nullable(str),
      capabilities: nullable(capabilities),
      notes: arr(str),
      unavailable_reason: nullable(str),
      // Nachtrag 12 (Stapelwortlaut je Signiermittel)
      batch_flow: optional(lit('login_then_confirm_each', 'confirm_each', 'unlock_once')),
    }),
  ),
  selected: nullable(backendId),
  default: nullable(backendId),
  // Nachtrag 20: Signiermittel-Auswahl nur bei mehr als einem Signiermittel (Kern entscheidet)
  choice_offered: bool,
});

const signingProgress = obj({
  backend: nullable(backendId),
  stage: signingStage,
  method: nullable(lit('app', 'qr', 'sms', 'fido2', 'password', 'card')),
  doc: nullable(docRef),
  index: int,
  total: int,
  login_done: bool,
  confirmations_done: int,
  confirmations_total: int,
  certificate: lit('read', 'cache', 'session', 'not_needed'),
  batch: bool,
  pause: nullable(
    obj({
      doc: nullable(docRef),
      code: str,
      params: record(str),
      scope: lit('document', 'session'),
      can_retry: bool,
      new_session: bool,
    }),
  ),
});

const errorContext = obj({
  origin: lit('load', 'analysis', 'prepare', 'signing', 'output', 'config', 'other'),
  occurred_at: nullable(str),
  backend: nullable(backendId),
  doc: nullable(docRef),
});

const signedSummary = obj({
  file_name: str,
  folder_display: nullable(str),
  signer: nullable(str),
  signing_time: str,
  backend: backendId,
  qualification: lit('Qes', 'AdES', 'Unknown'),
  self_check: bool,
  // Mehrfachsignatur: Anzahl der erstellten Signaturen; in TS optional
  signatures: optional(int),
  page: nullable(int),
});

const batchView = obj({
  items: arr(
    obj({
      doc: int,
      name: str,
      page_count: nullable(int),
      size_bytes: nullable(int),
      status: tagged('state', {
        analyzing: {},
        ready: {},
        positioned: {},
        rejected: { code: str },
        needs_review: { codes: arr(str) },
        queued: {},
        active: { stage: signingStage },
        signed: { path: str },
        failed: { code: str, params: record(str) },
        skipped: { existing: bool },
        cancelled: {},
      }),
      position: tagged('kind', { auto: {}, manual: { page: int }, field: { name: str }, qr: {}, invisible: {} }),
      target_name: nullable(str),
      target_exists: bool,
      thumbnail: bool,
      // Mehrfachsignatur: Signaturen des Eintrags im Lauf; in TS optional
      signatures: optional(int),
    }),
  ),
  open_item: nullable(int),
  running: bool,
  summary: nullable(
    obj({
      total: int,
      signed: int,
      rejected: int,
      failed: int,
      skipped: int,
      cancelled: int,
      not_started: int,
      folder_display: nullable(str),
      aborted: bool,
    }),
  ),
  output: obj({ mode: lit('next_to_source', 'folder', 'ask', 'app_documents'), folder_display: nullable(str), needs_folder: bool }),
  estimate: nullable(
    obj({
      backend: backendId,
      documents: int,
      logins: int,
      confirmations: int,
      certificate_confirmation_possible: bool,
      large: bool,
    }),
  ),
  // Nachtrag 8: automatische Positionierung im Stapel
  auto_position_setting: bool,
  auto_position_once: bool,
  manual_plans: int,
});

const migrationOffer = obj({ legacy_file: str, plaintext_secrets: bool, items: arr(obj({ code: str, detail: nullable(str) })) });

const firstRun = obj({
  migration: nullable(migrationOffer),
  default_backend: nullable(backendChoice),
  profile: signatureProfile,
  block_language: nullable(lang),
  preview_revision: int,
});

const actions = obj({
  open_files: bool,
  sign: bool,
  edit_placement: bool,
  start_batch: bool,
  show_batch: bool,
  settings: bool,
  home: bool,
});

const stepView = tagged('step', {
  starting: {},
  open: { queue: arr(queueItem) },
  loading: { queue: arr(queueItem) },
  warning: { doc: docInfo, codes: arr(str) },
  placeholder: {
    doc: docInfo,
    fields: arr(obj({ name: str, page: nullable(int) })),
    qr: nullable(obj({ page: int, id: nullable(str) })),
  },
  position: {
    doc: docInfo,
    page: int,
    block: obj({ width: num, height: num, preview_revision: int, emblem_placeholder: bool, emblem_choosable: bool, language: lit('de', 'en') }),
    placement: obj({ page_index: int, x: num, y: num, width: num, height: num }),
    new_page: bool,
    announce: record(str),
  },
  backend: {
    options: arr(obj({ id: backendId, available: bool, capabilities: nullable(capabilities), notes: arr(str) })),
  },
  signing: { interaction, queue: arr(queueItem), document_leaves_device: bool, progress: signingProgress },
  output: {
    result: obj({
      path: nullable(str),
      save_pending: bool,
      ask_overwrite: nullable(str),
      save_dialog: nullable(obj({ directory: str, file_name: str })),
      summary: nullable(signedSummary),
    }),
    next: nullable(docInfo),
    pending: int,
  },
  error: { code: str, params: record(str), retry: bool, context: errorContext },
  finished: { exit_code: int },
  batch: {},
});

// Einstellungen › Barrierefreiheit (docs/analyse/06)
const accessibility = obj({
  mode: bool,
  start: lit('preview', 'methods'),
  announce: lit('short', 'detailed'),
  announce_after_move: bool,
  keyboard_step: lit('standard', 'mm1', 'mm5', 'mm10'),
  check_before_sign: bool,
});

const settings = objOpen({
  schema_version: int,
  general: obj({
    ui_language: nullable(lang),
    update_check: bool,
    window_size: nullable(obj({ width: int, height: int })),
    last_source_directory: nullable(str),
    recent_documents: bool,
    first_run_completed: bool,
  }),
  signature: obj({
    profile: signatureProfile,
    block_language: nullable(lang),
    note: tagged('mode', { default: {}, none: {}, custom: { text: str } }),
    pdfa_force: bool,
    auto_position: bool,
    emblem: nullable(str),
    logo_only_size_mm: num,
    cms_profile: lit('pdfas_compat', 'pades_bb'),
  }),
  placeholder: obj({ search: bool, kind: lit('fields', 'marker') }),
  backend: obj({ default: nullable(backendChoice) }),
  id_austria: obj({ username: nullable(str), remember: bool, fido2_default: bool }),
  keystore: obj({
    enabled: bool,
    file: nullable(str),
    declared_type: nullable(str),
    alias: nullable(str),
    password_storage: lit('none', 'memory', 'keychain'),
    storage_chosen: bool,
  }),
  output: obj({ folder: nullable(str), postfix: str }),
  proxy: obj({ mode: lit('system', 'none', 'manual'), host: nullable(str), port: nullable(int), user: nullable(str) }),
  certificate_cache: obj({ enabled: bool }),
  accessibility,
});

const workflowError = ext({
  Rejected: obj({ class: lit('K1Regular', 'K2Repairable', 'K3Encrypted', 'K4Restricted', 'K5Rejected'), code: str }),
  Backend: obj({ code: str, detail: str }),
  Invariant: obj({
    invariant: lit('I1Prefix', 'I2ByteRange', 'I3Cms', 'I4PriorSignatures', 'I5Reparse', 'I6AllowedChanges'),
    detail: str,
  }),
  CmsTooLarge: obj({ needed: int, reserve: int }),
  Config: str,
  Save: str,
  Prepare: obj({ code: str, detail: str }),
  Unexpected: str,
});

const docResult = tagged('kind', {
  signed: { output: obj({ path: str, len: int, sha256: str }) },
  cancelled: {},
  failed: { error: workflowError },
});

const viewState = obj({
  step: stepView,
  main_bar: obj({ steps: arr(obj({ step, active: bool, enabled: bool })), settings_enabled: bool }),
  settings: nullable(
    obj({
      settings,
      problems: record(str),
      keychain_available: bool,
      fido2_available: bool,
      keystore_ready: bool,
      // Nachtrag ADR 0004 (Keystore): Format am Inhalt, Einträge für die Alias-Auswahl
      keystore_format: nullable(lit('pkcs12', 'jks', 'jceks', 'unknown')),
      keystore_entries: nullable(arr(obj({ alias: str, subject_cn: nullable(str), not_after: nullable(str), is_key_entry: bool }))),
      keystore_inspect_problem: nullable(str),
      read_only: bool,
      preview_revision: int,
      applied_revision: int,
      examples: record(str),
      // Nachtrag K3: per Richtlinie festgelegte Pfade (in TS optional)
      locked: optional(record(str)),
      emblem_placeholder: bool,
    }),
  ),
  language: lang,
  accessibility: optional(accessibility),
  notices: arr(obj({ id: int, code: str, params: record(str) })),
  migration: nullable(migrationOffer),
  done: arr(obj({ id: int, name: str, input: str, result: docResult })),
  platform: obj({ os: lit('windows', 'macos', 'linux', 'ios', 'android'), mobile: bool }),
  resume: nullable(obj({ name: str, backend: backendId, age_minutes: int, pending: int, document_leaves_device: bool })),
  keystore_import: nullable(obj({ file_name: str, ok: bool, code: str, password_stored: bool })),
  privacy_text: nullable(str),
  document: nullable(documentView),
  signers: signersView,
  batch: nullable(batchView),
  recent: nullable(arr(obj({ id: int, file_name: str, folder_display: nullable(str), signed_at: str, exists: bool }))),
  first_run: nullable(firstRun),
  actions,
  // Nachtrag 10: geparkte Arbeit („Zur Startseite“) – in TS optional, vom Kern immer geliefert
  parked: optional(obj({ doc_name: nullable(str), documents: int, placed_by_hand: bool, unsaved: bool })),
  // Nachtrag 12: Hinweis zum Teilen (iOS) – in TS optional
  share_hint: optional(obj({ doc: int })),
  // Nachtrag 21: „Über PIDA“ und Update-Prüfung – in TS optional, vom Kern immer geliefert
  about: optional(
    obj({
      version: str,
      release_date: str,
      release_notes_url: str,
      source_url: str,
      update: nullable(
        obj({
          state: lit('unchecked', 'disabled', 'policy_off', 'checking', 'up_to_date', 'available', 'failed'),
          latest_version: nullable(str),
          release_url: nullable(str),
          checked_at: nullable(str),
          reason: nullable(str),
        }),
      ),
    }),
  ),
  // Backlog Nr. 33: Hinweis auf eine neue Version auf der Startseite – in TS optional
  update_hint: optional(obj({ version: str, release_url: str })),
});

const formAnswer = tagged('kind', {
  login: { username: str, password: str, remember: bool },
  tan: { tan: str },
  switch_to_sms: {},
  switch_to_fido2: {},
  start_fido2: {},
  continue: {},
});

const uiEvent = tagged('type', {
  // Nachtrag 9: „+ PDF“ – in TS optional, ohne `mode` wie bisher
  open_files: { paths: arr(str), mode: omittable(lit('add', 'replace')) },
  answer_warning: { proceed: bool },
  placeholder_answer: { answer: lit('Yes', 'No', 'Cancel'), field_index: nullable(int) },
  move_block: { page: int, x: num, y: num },
  nudge_block: { dx: num, dy: num, fine: bool, coarse: bool },
  change_page: { page: int },
  toggle_new_page: {},
  auto_place: {},
  confirm_position: {},
  choose_invisible: {},
  choose_backend: { backend: backendId },
  submit_form: { request: int, answer: formAnswer },
  submit_secret: { request: int, secret: str, password_storage: omittable(lit('none', 'memory', 'keychain')) },
  answer_notice: { request: int, answer: lit('Ok', 'Retry', 'Cancel') },
  open_signature_data: { request: int },
  cancel: {},
  retry: {},
  save: { path: nullable(str) },
  confirm_overwrite: { overwrite: bool },
  open_result: {},
  share_result: {},
  export_result: {},
  navigate: { step },
  open_settings: {},
  save_settings: { settings },
  close_settings: {},
  reset_settings: {},
  backgrounded: {},
  foregrounded: {},
  resume_signature: {},
  discard_resume: {},
  import_keystore: { path: str, password: nullable(str), remember: bool },
  dismiss_keystore_import: {},
  quit: {},
  dismiss_notice: { id: int },
  accept_migration: { store_secrets: bool },
  decline_migration: {},
  // ADR 0004 (Vertrag E0)
  set_invisible: { invisible: bool },
  use_target: { target: placementTarget },
  inspect_pages: { pages: arr(int) },
  select_signer: { backend: nullable(backendId), make_default: bool },
  sign_with: { backend: backendId },
  show_batch: {},
  open_batch_item: { doc: int },
  back_to_batch: {},
  remove_from_batch: { doc: int },
  start_batch: { backend: backendId, docs: arr(int), output_dir: nullable(str), on_existing: lit('overwrite', 'keep_both', 'skip') },
  skip_batch_document: {},
  abort_batch: {},
  remove_recent: { id: int },
  clear_recent: {},
  apply_settings: { changes: record(unknownValue), revision: int },
  set_ui_language: { language: nullable(lang) },
  set_accessibility: { accessibility },
  // Nachtrag 24: „QR-Platzhalter erstellen“
  export_qr_placeholder: {
    spec: obj({ id: str, profile: lit('standard', 'amtssignatur', 'signer'), language: lit('de', 'en') }),
    format: lit('png', 'svg'),
    path: nullable(str),
  },
  update_first_run: { default_backend: nullable(backendChoice), profile: signatureProfile, block_language: nullable(lang) },
  finish_first_run: {},
  offer_migration: {},
  inspect_keystore: { password: nullable(str), password_storage: omittable(lit('none', 'memory', 'keychain')) },
  // Nachtrag 8 (Einstellung „Automatische Positionierung“)
  unlock_placement: {},
  set_batch_auto_position: { enabled: bool, replace_manual: bool },
  // Mehrfachsignatur: je Block eine eigene Signatur
  add_block: { page: omittable(num) },
  select_block: { index: num },
  remove_block: { index: num },
  fill_targets: {},
  // Nachtrag 10: „Zur Startseite“ parkt offene Arbeit
  go_home: {},
  resume_parked: {},
  discard_parked: {},
  // Nachtrag 12: Ersteinrichtung „Später einrichten“; von außen übergebene Dateien (Shell), Hinweis zum Teilen
  postpone_first_run: {},
  files_received: { paths: arr(str) },
  dismiss_share_hint: {},
  // Nachtrag 15: „Zuletzt signiert“ statt des entfernten Prüfmodus (`mode` in TS optional, wie `open_files`)
  open_recent: { id: int },
  reveal_recent: { id: int },
  share_recent: { id: int },
  sign_recent: { id: int, mode: omittable(lit('add', 'replace')) },
  // Nachtrag 21: „Über PIDA“ – beim Öffnen bzw. „Jetzt prüfen“
  check_for_updates: { explicit: bool },
  // Backlog Nr. 33: Hinweis auf der Startseite ausblenden
  dismiss_update_hint: {},
  // Inspektor › Optionen: Sprache des Signaturblocks
  set_block_language: { language: lit('de', 'en') },
});

// --- Typgleichheit Schema ↔ types.ts (Kompilierzeit) ------------------------------------------
const viewStateMatchesTypes: Same<Infer<typeof viewState>, ViewState> = true;
const uiEventMatchesTypes: Same<Infer<typeof uiEvent>, UiEvent> = true;

const fixtureDir = path.join(path.dirname(fileURLToPath(import.meta.url)), 'fixtures');
const fixtureUrl = (name: string) => path.join(fixtureDir, name);

describe('Vertrag ViewState (Rust → TS)', () => {
  const fixture = JSON.parse(readFileSync(fixtureUrl('view-states.json'), 'utf8')) as { name: string; view: unknown }[];

  it('Schema entspricht den TS-Typen', () => {
    expect(viewStateMatchesTypes).toBe(true);
    expect(uiEventMatchesTypes).toBe(true);
  });

  it('enthält Proben', () => {
    expect(fixture.length).toBeGreaterThan(40);
  });

  it.each(fixture.map((f) => [f.name, f.view] as const))('%s passt zu ViewState', (_name, view) => {
    expect(validate(viewState, view)).toEqual([]);
  });

  it('deckt alle Schritte, Interaktionen und Masken ab', () => {
    const views = fixture.map((f) => f.view as ViewState);
    const steps = new Set(views.map((v) => v.step.step));
    expect([...steps].sort()).toEqual([
      'backend',
      'batch',
      'error',
      'finished',
      'loading',
      'open',
      'output',
      'placeholder',
      'position',
      'signing',
      'starting',
      'warning',
    ]);
    const forms = new Set(
      views.flatMap((v) => (v.step.step === 'signing' && v.step.interaction.kind === 'form' ? [Object.keys(v.step.interaction.form)[0]] : [])),
    );
    expect([...forms].sort()).toEqual(['Fido2', 'Login', 'QrCode', 'SmsTan', 'WaitForApp']);
  });

  it('deckt jede Variante der Typen aus ADR 0004 ab', () => {
    const views = fixture.map((f) => f.view as ViewState);
    const docs = views.flatMap((v) => (v.document ? [v.document] : []));
    const progress = views.flatMap((v) => (v.step.step === 'signing' ? [v.step.progress] : []));
    const items = views.flatMap((v) => v.batch?.items ?? []);
    const sorted = (xs: (string | null | undefined)[]) => [...new Set(xs.filter((x): x is string => typeof x === 'string'))].sort();
    expect(sorted(docs.map((d) => d.phase))).toEqual(['done', 'editing', 'signing', 'warning']);
    expect(sorted(docs.map((d) => d.origin))).toEqual(['auto', 'field', 'qr', 'user']);
    expect(sorted(docs.map((d) => d.target.kind))).toEqual(['field', 'free', 'qr']);
    expect(sorted(docs.flatMap((d) => d.existing.map((e) => e.status)))).toEqual(['invalid', 'not_verifiable', 'valid']);
    expect(sorted(docs.flatMap((d) => d.layout.flatMap((l) => l.guides.map((g) => g.kind))))).toHaveLength(11);
    expect(sorted(progress.map((p) => p.stage))).toHaveLength(8);
    expect(sorted(progress.map((p) => p.method))).toEqual(['app', 'card', 'fido2', 'password', 'qr', 'sms']);
    expect(sorted(progress.map((p) => p.certificate))).toEqual(['cache', 'not_needed', 'read', 'session']);
    expect(sorted(progress.map((p) => p.pause?.scope))).toEqual(['document', 'session']);
    expect(sorted(views.map((v) => (v.step.step === 'error' ? v.step.context.origin : null)))).toHaveLength(7);
    expect(views.some((v) => v.step.step === 'output' && v.step.result.summary !== null)).toBe(true);
    expect(sorted(items.map((i) => i.status.state))).toHaveLength(11);
    expect(sorted(items.map((i) => i.position.kind))).toEqual(['auto', 'field', 'invisible', 'manual', 'qr']);
    expect(sorted(views.map((v) => v.batch?.output.mode))).toEqual(['app_documents', 'ask', 'folder', 'next_to_source']);
    expect(views.some((v) => (v.recent?.length ?? 0) > 0)).toBe(true);
    // Nachtrag 15: kein Prüfmodus mehr – weder Ansicht noch Freigabe im Vertrag
    expect(views.every((v) => !('verify' in v) && !('verify' in v.actions))).toBe(true);
    expect(views.some((v) => (v.first_run?.migration?.items.length ?? 0) === 8)).toBe(true);
    expect(views.some((v) => v.signers.options.length > 0 && v.actions.sign)).toBe(true);
    expect(views.some((v) => v.settings !== null && Object.keys(v.settings.examples).length > 0)).toBe(true);
    // Nachtrag K3: Erkennung (PDF/A, automatische Platzierung) und Richtliniensperren
    expect(sorted(docs.map((d) => d.detection?.pdfa_decision))).toEqual(['detected', 'forced', 'none']);
    expect(docs.some((d) => d.detection === null)).toBe(true);
    expect(sorted(docs.map((d) => (d.detection ? String(d.detection.auto_placed) : null)))).toEqual(['false', 'true']);
    expect(views.some((v) => Object.keys(v.settings?.locked ?? {}).length > 0)).toBe(true);
    expect(views.some((v) => v.settings !== null && v.settings.locked === null)).toBe(true);
    // Nachtrag 10: geparkte Arbeit (Einzeldokument von Hand platziert, Stapel)
    expect(sorted(views.map((v) => (v.parked ? (v.parked.doc_name ?? 'stapel') : null)))).toEqual(['Vertrag.pdf', 'stapel']);
    expect(views.some((v) => v.parked?.placed_by_hand === true)).toBe(true);
    // Nachtrag 21: jeder Zustand der Update-Prüfung; Mobile ohne Prüfung
    expect(sorted(views.map((v) => v.about?.update?.state))).toEqual(['available', 'checking', 'disabled', 'failed', 'policy_off', 'unchecked', 'up_to_date']);
    for (const v of views.filter((x) => x.platform.mobile && x.about)) expect(v.about?.update).toBeNull();
    expect(views.filter((v) => v.about?.update?.state === 'up_to_date').every((v) => v.about?.update?.checked_at !== null)).toBe(true);
  });

  it('defaultSettings() entspricht Settings::default() aus Rust (Mock-Kern, Tests, Zurücksetzen)', () => {
    const v = fixture.find((f) => f.name === 'settings_defaults')?.view as ViewState;
    expect(v.settings?.settings).toEqual(defaultSettings());
  });

  it('Standardwerte der neuen Typen entsprechen den Rust-Defaults (ADR 0004, Stand E0)', () => {
    const at = (name: string) => fixture.find((f) => f.name === name)?.view as ViewState;
    const waiting = at('waiting_connecting');
    expect(waiting.step.step === 'signing' ? waiting.step.progress : null).toEqual(defaultSigningProgress());
    const error = at('error');
    expect(error.step.step === 'error' ? error.step.context : null).toEqual(defaultErrorContext());
    const starting = at('starting');
    const { document, signers, batch, recent, first_run, actions } = starting;
    expect({ document, signers, batch, recent, first_run, actions }).toEqual(workspaceDefaults());
  });

  it('unbekannte Einstellungen stehen auf oberster Ebene (flatten)', () => {
    const v = fixture.find((f) => f.name === 'settings_open')?.view as ViewState;
    expect(v.settings?.settings['signature.future_key']).toBe('bleibt');
    expect(v.settings?.settings.future).toBe(42);
  });

  it('Schema erkennt Abweichungen', () => {
    expect(
      validate(viewState, {
        step: { step: 'open' },
        main_bar: { steps: [], settings_enabled: true },
        settings: null,
        language: 'fr',
        accessibility: null,
        notices: [],
        migration: null,
        done: [],
        platform: { os: 'ios', mobile: true },
        resume: null,
        keystore_import: null,
        privacy_text: null,
        document: null,
        signers: { options: [], selected: null, default: null, choice_offered: false },
        batch: null,
        recent: null,
        first_run: null,
        actions: {
          open_files: true,
          sign: false,
          edit_placement: false,
          start_batch: false,
          show_batch: false,
          settings: true,
          home: true,
        },
        parked: null,
        share_hint: null,
        about: null,
        update_hint: null,
      }),
    ).toEqual(['$.step<open>.queue: fehlt', '$.language: eines von de|en erwartet, "fr" gefunden']);
    expect(validate(uiEvent, { type: 'move_block', page: 1, x: 1, y: 2, z: 3 })).toEqual(['$<move_block>.z: unbekanntes Feld']);
    expect(validate(uiEvent, { type: 'frobnicate' })).toHaveLength(1);
  });
});

// K4 (ADR 0004 §6.2 Punkt 4): Zustände des echten Kerns (Ersteinrichtung; die des Prüfmodus sind mit ihm entfallen,
// Nachtrag 15) für die Oberfläche (B4); erzeugt von `PIDA_BLESS=1 cargo test -p pida-core-api --test first_run
// real_core_tools_views`.
describe('Zustände des echten Kerns passen zum Vertrag', () => {
  const load = (name: string) => Object.entries(JSON.parse(readFileSync(fixtureUrl(name), 'utf8')) as Record<string, unknown>);

  it.each(['real-core-views.json', 'real-core-batch-views.json', 'real-core-tools-views.json', 'real-core-about-views.json'].flatMap((file) =>
    load(file).map(([name, view]) => [`${file} ${name}`, view] as const),
  ))('%s', (_name, view) => {
    expect(validate(viewState, view)).toEqual([]);
  });

  it('Ersteinrichtung: Übernahmeliste, nach der Übernahme; kein Prüfmodus (Nachtrag 15)', () => {
    const views = load('real-core-tools-views.json').map(([, v]) => v as ViewState);
    expect(views.every((v) => !('verify' in v) && !('verify' in v.actions))).toBe(true);
    const items = views.flatMap((v) => v.first_run?.migration?.items ?? []);
    expect(items.map((i) => i.code.replace('ng-migration-item-', ''))).toEqual([
      'block',
      'emblem',
      'username',
      'output',
      'keystore',
      'proxy',
      'placeholder',
      'language',
    ]);
    expect(views.some((v) => v.first_run !== null && v.first_run.migration === null && v.first_run.preview_revision > 0)).toBe(true);
  });
});

describe('Vertrag UiEvent (TS → Rust)', () => {
  const samples: UiEvent[] = [...UI_EVENT_SAMPLES, ...UI_EVENT_EXTRA];

  it.each(samples.map((s) => [s.type, s] as const))('%s passt zum Schema', (_t, sample) => {
    expect(validate(uiEvent, sample)).toEqual([]);
  });

  // ohne das veraltete `acknowledge_privacy` (nicht mehr in den TS-Typen, ADR 0003); 26 davon aus ADR 0004
  // (19 aus E0 – ohne die vier des entfernten Prüfmodus –, `inspect_keystore` aus Nachtrag 7, `unlock_placement` und
  // `set_batch_auto_position` aus Nachtrag 8, `open_recent`/`reveal_recent`/`share_recent`/`sign_recent` aus
  // Nachtrag 15; `check_for_updates` aus Nachtrag 21; `add_block`, `select_block`, `remove_block` und
  // `fill_targets` der Mehrfachsignatur)
  it('alle 68 Varianten sind vertreten', () => {
    expect(new Set(UI_EVENT_SAMPLES.map((s) => s.type)).size).toBe(68);
  });

  it('enthält die Mobile-Proben (Plattform, FIDO2-Hinweis, Fortsetzen) und den Import nur am Desktop', () => {
    const fixture = JSON.parse(readFileSync(fixtureUrl('view-states.json'), 'utf8')) as { name: string; view: ViewState }[];
    const names = new Set(fixture.map((f) => f.name));
    for (const n of ['mobile_backend', 'mobile_resume_offer', 'keystore_imported']) expect(names.has(n)).toBe(true);
    const backend = fixture.find((f) => f.name === 'mobile_backend')?.view;
    expect(backend?.platform).toEqual({ os: 'android', mobile: true });
    // iOS/Android nur mit ID Austria (ADR 0004 Nachtrag 11): keine Probe mit Keystore auf Mobilgeräten – und
    // damit keine Signiermittel-Auswahl (Nachtrag 20)
    for (const f of fixture.filter((x) => x.view.platform.mobile)) {
      expect(f.view.keystore_import, f.name).toBeNull();
      expect(f.view.signers.options.map((o) => o.backend), f.name).not.toContain('keystore');
      expect(f.view.signers.choice_offered, f.name).toBe(false);
    }
    // Desktop mit mehreren Signiermitteln: Auswahl wie bisher
    expect(fixture.some((x) => !x.view.platform.mobile && x.view.signers.options.length > 1 && x.view.signers.choice_offered)).toBe(true);
  });

  it('ui-events.json ist aktuell (für den Rust-Test)', () => {
    const text = `${JSON.stringify(samples, null, 2)}\n`;
    const file = fixtureUrl("ui-events.json");
    if (process.env.PIDA_BLESS) writeFileSync(file, text);
    expect(readFileSync(file, "utf8")).toBe(text);
  });
});
