/**
 * TS-Mock-Kern für `VITE_MOCK=1`, die Browser-Vorschau und die Komponententests (ADR 0004 §4.8).
 *
 * Er spielt den Arbeitsbereich-Modus des Kerns nach (K1: `CoreOptions::workspace_ui`): Öffnen →
 * automatische Platzierung → „Signieren mit X“ (`sign_with`) → Signierkarte → Fertig, liefert
 * `ViewState`s im serde-Format von `pida-core-api` und simuliert die nativen Dienste (Dialoge, Drag &
 * Drop). Er ist **kein** Ersatz für die Fachlogik des Rust-Kerns: Regeln nur so weit, wie Bedienung und
 * Tests sie brauchen. Keine echten Zugangsdaten, kein Netz. Invariante A1 gilt auch hier: Eine
 * Signiersitzung beginnt nur nach `sign_with`, `choose_backend` (Rückfall), `resume_signature` oder
 * „Erneut versuchen“.
 *
 * Aufbau: `index.ts` (UI-A) hält die Bereiche `{ flow, settings, recent, firstrun, about }` mit je
 * eigenem Zustandsausschnitt (`shared.ts`), verteilt jedes Ereignis an alle, leitet Signale weiter und
 * setzt die Projektionen zusammen. Der Bereich `flow` (Kern, Arbeitsbereich, Signieren, Stapel) steht hier
 * und nutzt `workspace.ts` (Dokumente, Platzierung), `signing.ts` (Signiersitzung) und `batch.ts`
 * (Stapelübersicht und -lauf mit eigenem Zustand, wie `State::BatchOverview`/`BatchSigning` des Kerns neben
 * der Warteschlange; mit `batchOverview` bei ≥ 2 Dokumenten, ADR 0004 §3.8).
 */
import { defaultAccessibility, defaultErrorContext, defaultSigners, defaultSigningProgress, mainBar, noActions, workspaceDefaults } from '../defaults';
import type { Bridge, FileDragEvent, LicenseKind, MobilePick } from '../bridge';
import type {
  ActionsView,
  BackendId,
  DocId,
  DoneItemView,
  ErrorOrigin,
  Lang,
  MainBarView,
  NoticeView,
  ParkedWorkView,
  PlatformView,
  QrPlaceholderSpec,
  ResumeOfferView,
  SaveDialogView,
  Settings,
  SignedSummaryView,
  SignerView,
  SignersView,
  StepView,
  UiEvent,
  ViewState,
} from '../types';
import { aboutArea } from './about';
import { MockBatch, type BatchHost, type BatchPlan } from './batch';
import { firstRunArea } from './firstrun';
import { qrArea, qrPlaceholderSvg } from './qr';
import { recentArea } from './recent';
import { settingsArea } from './settings';
import {
  A4,
  batchFlowFor,
  capabilitiesFor,
  fileNameOf,
  folderOf,
  isoWithOffset,
  joinPath,
  maskAccount,
  outputFileName,
  SANDBOX,
  withStorageQuestion,
  type MockArea,
  type MockCtx,
  type MockOptions,
  type MockSignal,
} from './shared';
import { SigningSession } from './signing';
import {
  autoPlace,
  blockPreviewSvg,
  blockSize,
  changePage,
  constraintsFor,
  docInfo,
  followAutoPosition,
  moveBlock,
  nudgeBlock,
  pageTileSvg,
  projectDocument,
  resizeBlock,
  specForName,
  startEdit,
  toggleNewPage,
  useTarget,
  addBlock,
  selectBlock,
  removeBlock,
  fillTargets,
  targetTaken,
  blockCount,
  contractSpec,
  blankSpec,
  fieldSpec,
  signedSpec,
  offerSpec,
  confirmationSpec,
  lockedSpec,
  type EditState,
  type MockDoc,
  type MockDocSpec,
  emblemPlaceholder,
} from './workspace';

export type { MockOptions, MockScenario, MockSigningFlow, MockUpdateScenario } from './shared';
export { MOCK_SCENARIOS } from './shared';

/* ================================================================ Bereich „flow“ */

type Stage = 'starting' | 'open' | 'loading' | 'warning' | 'editing' | 'choosing' | 'signing' | 'output' | 'error' | 'finished' | 'batch';

interface OutputState {
  path: string | null;
  savePending: boolean;
  askOverwrite: string | null;
  saveDialog: SaveDialogView | null;
}

const EMPTY_OUTPUT: OutputState = { path: null, savePending: false, askOverwrite: null, saveDialog: null };

interface ErrorState {
  code: string;
  params: Record<string, string>;
  retry: boolean;
  origin: ErrorOrigin;
  occurredAt: string;
  backend: BackendId | null;
  onRetry: () => void;
  onCancel: () => void;
}

interface FlowState {
  opts: MockOptions;
  stage: Stage;
  queue: MockDoc[];
  nextDocId: number;
  edit: EditState | null;
  /** Revision der Blockvorschau (steigt mit jeder Änderung der Einstellungen) */
  preview: number;
  signing: SigningSession | null;
  /** Auswahl des Konto-Chips; `explicit` = durch Nutzerin gewählt (gilt bis zum Ende des Dokuments) */
  selected: BackendId | null;
  explicit: boolean;
  /** Passwort im „Merken“-Speicher des Backends (gemerkt nach Anmeldung mit „merken“, F-72 löscht) */
  remembered: boolean;
  output: OutputState;
  summary: SignedSummaryView | null;
  error: ErrorState | null;
  done: DoneItemView[];
  resume: ResumeOfferView | null;
  exitCode: number;
  networkErrorOnce: boolean;
  /** letzte Position/Signiermittel (Tests) */
  last: { position: string; backend: BackendId | null };
  /** Stapel (Übersicht, Lauf; auch während ein Dokument daraus geöffnet ist) */
  batch: MockBatch | null;
  /** Plan des aus dem Stapel geöffneten Dokuments (wird nach der Analyse angewendet) */
  pendingPlan: BatchPlan | null;
  /**
   * Arbeit geparkt („Zur Startseite“, Kern: `Queue::parked`, ADR 0004 Nachtrag 10): Stadium, Dokument, Platzierung
   * und Stapel bleiben; projiziert wird die Startansicht mit der Karte „In Bearbeitung“
   */
  parked: boolean;
}

function mobile(o: MockOptions): boolean {
  return (o.platform ?? 'desktop') !== 'desktop';
}

function backendFor(choice: Settings['backend']['default']): BackendId | null {
  switch (choice) {
    case 'id_austria':
      return 'atrust_native';
    case 'local_bku':
      return 'local_bku';
    case 'keystore':
      return 'keystore';
    default:
      return null;
  }
}

/**
 * Standard-Signiermittel wie `ConfigState::default_backend` im Kern: auf iOS/Android immer ID Austria (das einzige
 * Signiermittel, ADR 0004 Nachtrag 11), sonst die Einstellung.
 */
function defaultBackend(ctx: MockCtx): BackendId | null {
  return ctx.platform().mobile ? 'atrust_native' : backendFor(ctx.settings().backend.default);
}

/** Wahl des Keystores auf Mobilgeräten (dort ohne Wirkung wie `mobile_signers::ignored_on_mobile`)? */
function mobileKeystore(ctx: MockCtx, backend: BackendId | null): boolean {
  return ctx.platform().mobile && backend === 'keystore';
}

/** Wahl im Konto-Chip, die es auf Mobilgeräten nicht gibt (Keystore, Chipkarte – `ignored_on_mobile`)? */
function mobileUnofferedSigner(mobile: boolean, backend: BackendId | null): boolean {
  return mobile && (backend === 'keystore' || backend === 'local_bku');
}

function signerOptions(s: FlowState, ctx: MockCtx): SignerView[] {
  const settings = ctx.settings();
  const isMobile = ctx.platform().mobile;
  const ida = settings.id_austria;
  const ks = settings.keystore;
  const out: Omit<SignerView, 'batch_flow'>[] = [
    {
      backend: 'atrust_native',
      available: true,
      account: ida.remember && ida.username ? maskAccount(ida.username) : null,
      capabilities: capabilitiesFor('atrust_native', isMobile),
      notes: isMobile ? ['ng-fido2-unavailable-mobile'] : [],
      unavailable_reason: null,
    },
  ];
  if (!isMobile) {
    const ok = s.opts.localBkuAvailable ?? true;
    out.push({ backend: 'local_bku', available: ok, account: null, capabilities: ok ? capabilitiesFor('local_bku') : null, notes: [], unavailable_reason: ok ? null : 'ng-signer-localbku-not-running' });
  }
  // Keystore nur am Desktop (iOS/Android: nur ID Austria, ADR 0004 Nachtrag 11)
  if (ks.enabled && !isMobile) {
    const ok = !!ks.file;
    out.push({
      backend: 'keystore',
      available: ok,
      account: ok && ks.file ? [fileNameOf(ks.file), ks.alias].filter(Boolean).join(' · ') : null,
      capabilities: ok ? capabilitiesFor('keystore') : null,
      notes: [],
      unavailable_reason: ok ? null : 'ng-signer-keystore-missing',
    });
  }
  // Richtlinie `allowed_backends` wie im Kern: nicht freigegebene gesperrt mit Grund (bleiben in den Optionen)
  const allowed = s.opts.allowedBackends;
  const byPolicy = out.map((o) => (!allowed || allowed.includes(o.backend) ? o : { ...o, available: false, capabilities: null, unavailable_reason: 'ng-signer-policy' }));
  // Ablauf des Stapels aus den Fähigkeiten wie der Kern (Nachtrag 12)
  return byPolicy.map((o) => ({ ...o, batch_flow: batchFlowFor(o.capabilities) }));
}

/**
 * Signiermittel, die in Frage kommen (wie `eligible_signers` im Kern, ADR 0004 Nachtrag 20): jede Option außer den
 * von der Richtlinie ausgeschlossenen – ein nicht laufendes bzw. nicht eingerichtetes Signiermittel zählt.
 */
function eligibleSigners(options: SignerView[]): BackendId[] {
  return options.filter((o) => o.unavailable_reason !== 'ng-signer-policy').map((o) => o.backend);
}

/**
 * `ViewState::signers` wie `signers_view` im Kern: kommt nur ein Signiermittel in Frage (iOS/Android: ID Austria;
 * Desktop per Richtlinie), gibt es keine Auswahl (`choice_offered = false`) und es ist gewählt – auch bei „Jedes Mal
 * fragen“ und nach `select_signer` (Nachtrag 20).
 */
function signersProjection(s: FlowState, ctx: MockCtx): SignersView {
  const options = signerOptions(s, ctx);
  const eligible = eligibleSigners(options);
  const choiceOffered = eligible.length > 1;
  return { ...defaultSigners(), options, selected: choiceOffered ? s.selected : (eligible[0] ?? null), default: defaultBackend(ctx), choice_offered: choiceOffered };
}

/**
 * Hinweis, wenn mit `backend` gerade nicht signiert werden kann: ohne Auswahl (nur ein Signiermittel, Nachtrag 20) der
 * Grund der Option (wie `single_signer_unavailable` im Kern) statt „Bitte wählen Sie ein anderes“.
 */
function unavailableNotice(s: FlowState, ctx: MockCtx, backend: BackendId): string {
  const signers = signersProjection(s, ctx);
  const reason = signers.choice_offered ? null : signers.options.find((o) => o.backend === backend)?.unavailable_reason;
  return reason ?? 'ng-signer-unavailable';
}

/** Wirksame Auswahl des Konto-Chips (`signers.selected`). */
function selectedSigner(s: FlowState, ctx: MockCtx): BackendId | null {
  return signersProjection(s, ctx).selected;
}

function available(s: FlowState, ctx: MockCtx, id: BackendId): boolean {
  return signerOptions(s, ctx).some((o) => o.backend === id && o.available);
}

function blockLang(ctx: MockCtx): Lang {
  const s = ctx.settings();
  return s.signature.block_language ?? s.general.ui_language ?? 'de';
}

function current(s: FlowState): MockDoc | null {
  return s.queue[0] ?? null;
}

/** offene Dokumente (Arbeitsbereich, Warteschlange, Stapel; auch geparkt) */
function openDocs(s: FlowState): MockDoc[] {
  const all = [...s.queue, ...(s.batch?.docs() ?? [])];
  return all.filter((d, i) => all.findIndex((x) => x.id === d.id) === i);
}

/**
 * Wie der Kern (`report_selection`, ADR 0004 Nachtrag 17): gewählte Dokumente, die schon offen waren bzw. doppelt in
 * der Auswahl stehen, kommen nicht noch einmal dazu – Hinweis `ng-open-duplicates` statt stillem Verwerfen. Gilt für
 * `open_files` und „Erneut signieren“ (`sign_recent`, Kern: `open_paths`).
 */
function reportDuplicates(s: FlowState, paths: string[], openBefore: Set<DocId>, ctx: MockCtx): void {
  const after = openDocs(s);
  const dups = paths.filter((p, i) => after.some((d) => d.path === p && openBefore.has(d.id)) || paths.indexOf(p) < i);
  if (dups.length === 0) return;
  ctx.notify('ng-open-duplicates', dups.length === 1 ? { count: '1', file: fileNameOf(dups[0] ?? '') } : { count: String(dups.length) });
}

function makeDoc(s: FlowState, path: string, specOverride?: MockDocSpec): MockDoc {
  const name = fileNameOf(path);
  const spec = specOverride ?? specForName(name, s.opts.placeholder ?? 'none');
  return { id: s.nextDocId++, path, name, spec, signed: spec.existing.length > 0 };
}

/** Auswahl des Konto-Chips zu Beginn eines Dokuments: Standard, sofern verfügbar (§3.4, §8 F12). */
function resetSelection(s: FlowState, ctx: MockCtx): void {
  if (s.explicit && s.selected && available(s, ctx, s.selected)) return;
  const def = defaultBackend(ctx);
  s.selected = def && available(s, ctx, def) ? def : null;
  s.explicit = false;
}

function analyze(s: FlowState, ctx: MockCtx): void {
  const d = current(s);
  if (!d) {
    s.stage = 'open';
    return;
  }
  s.stage = 'loading';
  s.edit = null;
  ctx.later(() => {
    if (s.stage !== 'loading' || current(s) !== d) return;
    if (!/\.pdf$/i.test(d.name)) {
      fail(s, ctx, 'error-MayNotBeAPDF', {}, true, 'load', () => analyze(s, ctx), () => dropCurrent(s, ctx));
    } else if (d.spec.locked) {
      fail(s, ctx, 'error-PDFPwdProtected', {}, false, 'load', () => undefined, () => dropCurrent(s, ctx));
    } else {
      s.edit = startEdit(d, ctx.settings(), blockLang(ctx));
      if (s.pendingPlan) s.edit = applyPlan(d, s.edit, s.pendingPlan, blockSize(ctx.settings(), blockLang(ctx)));
      s.pendingPlan = null;
      // „Automatische Positionierung“ sperrt nie einen aus der Stapelübersicht geöffneten Eintrag (Nachtrag 8)
      if (fromBatch(s)) s.edit = { ...s.edit, lock: 'off' };
      // Raster der Seite des Blocks (die übrigen nach `inspect_pages`)
      if (s.edit.placement) s.edit.ready = [s.edit.placement.page_index];
      resetSelection(s, ctx);
      s.stage = (s.opts.warnings ?? []).length > 0 ? 'warning' : 'editing';
    }
  });
}

function fail(s: FlowState, ctx: MockCtx, code: string, params: Record<string, string>, retry: boolean, origin: ErrorOrigin, onRetry: () => void, onCancel: () => void): void {
  void ctx;
  s.stage = 'error';
  s.error = { code, params, retry, origin, occurredAt: isoWithOffset(new Date()), backend: s.signing?.backend ?? null, onRetry, onCancel };
}

function dropCurrent(s: FlowState, ctx: MockCtx): void {
  s.queue.shift();
  s.edit = null;
  s.output = EMPTY_OUTPUT;
  s.summary = null;
  if (s.queue.length > 0) analyze(s, ctx);
  else s.stage = 'open';
}

function finishDoc(s: FlowState, result: DoneItemView['result']): void {
  const d = current(s);
  if (d) s.done.push({ id: d.id, name: d.name, input: d.path, result });
}

/** Stapelübersicht bei ≥ 2 Dokumenten (`batch_overview`, nur im Arbeitsbereich-Modus). */
function batchOn(s: FlowState): boolean {
  return s.opts.batchOverview !== false;
}

/** Dienste des Ablaufs für den Stapel (`batch.ts`). */
function batchHost(s: FlowState, ctx: MockCtx): BatchHost {
  return {
    ctx,
    opts: s.opts,
    available: (backend) => available(s, ctx, backend),
    unavailableNotice: (backend) => unavailableNotice(s, ctx, backend),
    selected: () => selectedSigner(s, ctx),
    remembered: () => s.remembered,
    setRemembered: (value) => {
      s.remembered = value;
    },
    openSingle: (doc, plan) => {
      s.queue = [doc];
      s.pendingPlan = plan;
      s.output = EMPTY_OUTPUT;
      s.summary = null;
      s.error = null;
      analyze(s, ctx);
    },
    runEnded: () => {
      s.explicit = false;
      resetSelection(s, ctx);
    },
  };
}

/** Stapelübersicht mit diesen Dokumenten (Voranalyse im Hintergrund). */
function enterBatch(s: FlowState, ctx: MockCtx, docs: MockDoc[]): void {
  const batch = new MockBatch(batchHost(s, ctx));
  batch.add(docs);
  s.batch = batch;
  s.queue = [];
  s.edit = null;
  s.error = null;
  s.output = EMPTY_OUTPUT;
  s.summary = null;
  s.stage = 'batch';
  resetSelection(s, ctx);
}

/** Zurück in die Übersicht (aus dem geöffneten Einzeldokument). */
function leaveSingle(s: FlowState): void {
  s.signing?.close();
  s.signing = null;
  s.queue = [];
  s.edit = null;
  s.error = null;
  s.output = EMPTY_OUTPUT;
  s.summary = null;
  s.stage = 'batch';
}

/** aus dem Stapel geöffnetes Dokument (`return_to_batch`) */
function fromBatch(s: FlowState): boolean {
  const d = current(s);
  return !!s.batch && !!d && s.batch.openItem === d.id;
}

/**
 * „Zum Stapel“ (`show_batch`): Einzeldokument mit wartenden Dokumenten (Arbeitsbereich, Analysewarnung) → Übersicht;
 * die Platzierung des offenen Dokuments wird dessen Plan, ein Dokument mit Warnung ein Eintrag.
 */
function showBatch(s: FlowState, ctx: MockCtx): void {
  if ((s.stage !== 'editing' && s.stage !== 'warning') || s.batch || !batchOn(s) || s.queue.length < 2) return;
  const [cur, ...rest] = s.queue;
  const batch = new MockBatch(batchHost(s, ctx));
  if (cur && s.edit && s.stage === 'editing') batch.addEdited(cur, planFromEdit(cur, s.edit), followsSettings(s.edit));
  batch.add(s.stage === 'editing' ? rest : s.queue);
  s.batch = batch;
  leaveSingle(s);
}

/** „Zurück zum Stapel“ (`back_to_batch`): aus der Übersicht geöffnetes Dokument zurück, die Platzierung wird Plan. */
function backToBatch(s: FlowState): void {
  const b = s.batch;
  const d = current(s);
  if (!b || !d || !fromBatch(s)) return;
  b.back(d.id, s.stage === 'editing' && s.edit ? planFromEdit(d, s.edit) : undefined, s.edit ? followsSettings(s.edit) : false);
  leaveSingle(s);
}

function openFiles(s: FlowState, ctx: MockCtx, paths: string[]): void {
  const fresh = paths.filter((p, i) => paths.indexOf(p) === i && !s.queue.some((d) => d.path === p) && !s.batch?.has(p)); // F-89 ohne Duplikate
  if (fresh.length === 0) return;
  if (s.batch) {
    // Übersicht: hinzufügen und voranalysieren; während eines Laufs gehören sie nicht zum Lauf
    s.batch.add(fresh.map((p) => makeDoc(s, p)));
    if (s.stage !== 'batch') ctx.notify('ng-batch-added', { count: String(fresh.length) });
    return;
  }
  const saved = s.stage === 'output' && !!s.output.path;
  const idle = saved || (s.queue.length === 0 && (s.stage === 'open' || s.stage === 'error'));
  if (idle) {
    if (saved && s.output.path) {
      // gespeichert: Dokument verbuchen, dann wie im Leerlauf
      finishDoc(s, { kind: 'signed', output: { path: s.output.path, len: 1024, sha256: '00' } });
      s.queue.shift();
      s.output = EMPTY_OUTPUT;
      s.summary = null;
    }
    const docs = [...s.queue, ...fresh.map((p) => makeDoc(s, p))];
    s.error = null;
    if (batchOn(s) && docs.length >= 2) {
      enterBatch(s, ctx, docs);
      return;
    }
    s.queue = docs;
    analyze(s, ctx);
    return;
  }
  for (const path of fresh) s.queue.push(makeDoc(s, path));
  if (s.stage === 'editing' || s.stage === 'warning' || s.stage === 'loading') ctx.notify('ng-batch-added', { count: String(fresh.length) });
}

/**
 * „Weitere PDFs hinzufügen“ (`open_files { mode: 'add' }`, ADR 0004 Nachtrag 9) wie der Kern
 * (`Event::MoreFilesAdded`): im Arbeitsbereich werden das offene Dokument und die neuen zusammen zum Stapel, die
 * Platzierung des offenen Dokuments bleibt dessen Plan (wie `show_batch`); aus der Übersicht geöffnet zurück mit
 * den neuen Einträgen; nur Duplikate → nichts; sonst (Analysewarnung, Start, Übersicht …) wie `open_files`.
 */
function addFiles(s: FlowState, ctx: MockCtx, paths: string[]): void {
  const d = current(s);
  const edit = s.edit;
  if (s.stage !== 'editing' || !d || !edit || !batchOn(s) || (s.batch && !fromBatch(s))) {
    openFiles(s, ctx, paths);
    return;
  }
  const fresh = paths.filter((p, i) => paths.indexOf(p) === i && !s.queue.some((q) => q.path === p) && !s.batch?.has(p));
  if (fresh.length === 0) return;
  const docs = fresh.map((p) => makeDoc(s, p));
  if (s.batch) {
    s.batch.back(d.id, planFromEdit(d, edit), followsSettings(edit));
    s.batch.add(docs);
  } else {
    const [cur, ...rest] = s.queue;
    const batch = new MockBatch(batchHost(s, ctx));
    if (cur) batch.addEdited(cur, planFromEdit(cur, edit), followsSettings(edit));
    batch.add([...rest, ...docs]);
    s.batch = batch;
  }
  leaveSingle(s);
}

/**
 * „Anderes PDF öffnen“ (`open_files { mode: 'replace' }`, ADR 0004 Nachtrag 9) wie der Kern
 * (`Event::DocumentReplaced`): das offene Dokument der Analysewarnung bzw. des Arbeitsbereichs wird geschlossen
 * (nie signiert, „abgebrochen“), die Dateien öffnen wie in der Dokumentauswahl; wartende Dokumente bleiben (mit
 * ihnen ab zwei Dokumenten die Übersicht). Aus der Übersicht geöffnet ersetzt es den Eintrag. Sonst wie
 * `open_files`.
 */
function replaceDocument(s: FlowState, ctx: MockCtx, paths: string[]): void {
  const d = current(s);
  const unique = paths.filter((p, i) => paths.indexOf(p) === i);
  if (!d || unique.length === 0 || (s.stage !== 'editing' && s.stage !== 'warning')) {
    openFiles(s, ctx, paths);
    return;
  }
  const b = s.batch;
  if (b && fromBatch(s)) {
    b.back(d.id, undefined);
    b.remove(d.id);
    leaveSingle(s);
    if (b.size > 0) {
      b.add(unique.filter((p) => !b.has(p)).map((p) => makeDoc(s, p)));
      return;
    }
    s.batch = null;
  } else {
    finishDoc(s, { kind: 'cancelled' });
    s.queue.shift();
  }
  s.edit = null;
  s.error = null;
  s.output = EMPTY_OUTPUT;
  s.summary = null;
  s.explicit = false;
  const waiting = s.queue;
  const fresh = unique.filter((p) => !waiting.some((q) => q.path === p)).map((p) => makeDoc(s, p));
  if (batchOn(s) && waiting.length + fresh.length >= 2) {
    enterBatch(s, ctx, [...waiting, ...fresh]);
    return;
  }
  const [first, ...more] = fresh;
  s.queue = first ? [first, ...waiting, ...more] : waiting;
  analyze(s, ctx);
}

/** Plan aus dem Bearbeitungszustand (`BackToBatch`/`ShowBatch`; `origin == Auto` → automatisch). */
function planFromEdit(d: MockDoc, e: EditState): BatchPlan | null {
  if (e.invisible && !e.invisibleProfile) return { kind: 'invisible' };
  if (e.target.kind === 'qr') return { kind: 'qr' };
  if (e.target.kind === 'field') return { kind: 'field', index: e.target.index, name: d.spec.fields[e.target.index]?.name ?? '' };
  if (e.origin === 'user' && e.placement) return { kind: 'manual', placement: { ...e.placement }, newPage: e.newPage };
  return null;
}

/**
 * Folgt der Plan aus diesem Bearbeitungszustand weiter den Einstellungen (Kern: `BatchItem::auto_plan`)? Nur
 * solange die Lage aus der Automatik stammt und „Unsichtbar signieren“ nicht von der Nutzerin gewählt ist.
 */
function followsSettings(e: EditState): boolean {
  return e.automatic && !(e.invisible && !e.invisibleProfile);
}

/** Gemerkten Plan auf den Bearbeitungszustand anwenden (Dokument aus dem Stapel geöffnet). */
function applyPlan(d: MockDoc, e: EditState, plan: BatchPlan, block: { width: number; height: number }): EditState {
  switch (plan.kind) {
    case 'qr':
      return useTarget(d, e, { kind: 'qr' }, block) ?? e;
    case 'field':
      return useTarget(d, e, { kind: 'field', index: plan.index }, block) ?? e;
    case 'invisible':
      return { ...e, invisible: true, automatic: false, revision: e.revision + 1 };
    case 'manual':
      return { ...e, placement: { ...plan.placement, width: block.width, height: block.height }, newPage: plan.newPage, origin: 'user', target: { kind: 'free' }, automatic: false, revision: e.revision + 1 };
  }
}

function startSigning(s: FlowState, ctx: MockCtx, backend: BackendId): void {
  const d = current(s);
  const edit = s.edit;
  if (!d || !edit) return;
  if (!available(s, ctx, backend)) {
    ctx.notify(unavailableNotice(s, ctx, backend));
    return;
  }
  const settings = ctx.settings();
  if (!s.explicit || !s.selected) {
    s.selected = backend;
    s.explicit = true;
  }
  s.last = {
    position: edit.invisible ? 'invisible' : edit.target.kind === 'field' ? 'field' : edit.target.kind === 'qr' ? 'placeholder' : edit.origin === 'auto' ? 'auto' : 'manual',
    backend,
  };
  const session = new SigningSession(
    {
      later: (fn, factor) => ctx.later(fn, factor),
      finished: () => toOutput(s, ctx),
      failed: (code, params, retry) =>
        fail(
          s,
          ctx,
          code,
          params,
          retry,
          'signing',
          () => {
            s.stage = 'signing';
            s.error = null;
            session.retry();
          },
          () => {
            session.close();
            s.signing = null;
            s.error = null;
            s.stage = 'editing';
          },
        ),
      cancelled: () => cancelSigning(s, ctx),
      loggedIn: (remember) => {
        s.remembered = remember;
      },
      passwordCleared: () => {
        s.remembered = false;
      },
    },
    {
      backend,
      doc: { id: d.id, name: d.name },
      flow: s.opts.signingFlow ?? 'app',
      visible: !edit.invisible,
      signatures: blockCount(edit),
      fido2Available: s.opts.fido2Available ?? true,
      fido2Default: settings.id_austria.fido2_default,
      mobile: ctx.platform().mobile,
      autoComplete: s.opts.autoCompleteApp ?? true,
      remembered: s.remembered,
      prefillUsername: settings.id_austria.username,
      rememberChecked: settings.id_austria.remember,
      keystoreSubject: fileNameOf(settings.keystore.file ?? 'keystore.p12'),
      networkErrorOnce: s.networkErrorOnce,
    },
  );
  s.networkErrorOnce = false;
  s.signing = session;
  s.stage = 'signing';
  s.error = null;
  session.start();
}

function cancelSigning(s: FlowState, ctx: MockCtx): void {
  s.signing?.close();
  s.signing = null;
  // F-72: gemerktes Passwort vergessen, Benutzername bleibt; zurück zum Arbeitsbereich mit unveränderter Platzierung
  s.remembered = false;
  s.stage = s.edit ? 'editing' : 'open';
  ctx.notify('ng-signing-cancelled');
}

function toOutput(s: FlowState, ctx: MockCtx): void {
  const d = current(s);
  if (!d) return;
  const session = s.signing;
  s.signing = null;
  d.signed = true;
  s.stage = 'output';
  s.summary = null;
  const settings = ctx.settings();
  const name = outputFileName(d.path, settings.output.postfix || '_signed');
  const isMobile = ctx.platform().mobile;
  const folder = settings.output.folder ?? (isMobile ? `${SANDBOX}/Documents` : null);
  const backend = session?.backend ?? selectedSigner(s, ctx) ?? 'atrust_native';
  const finish = (path: string) => saved(s, ctx, path, backend);
  if (folder) {
    const target = joinPath(folder === '.' ? folderOf(d.path) : folder, name);
    if ((s.opts.existingFiles ?? []).includes(target)) s.output = { ...EMPTY_OUTPUT, askOverwrite: target };
    else finish(target);
  } else {
    s.output = { ...EMPTY_OUTPUT, saveDialog: { directory: folderOf(d.path), file_name: name } };
  }
}

function saved(s: FlowState, ctx: MockCtx, path: string, backend: BackendId = s.last.backend ?? 'atrust_native'): void {
  const isMobile = ctx.platform().mobile;
  s.output = { ...EMPTY_OUTPUT, path };
  s.stage = 'output';
  const caps = capabilitiesFor(backend, isMobile);
  s.summary = {
    file_name: fileNameOf(path),
    folder_display: isMobile ? null : folderOf(path),
    signer: 'Max Mustermann',
    signing_time: isoWithOffset(new Date()),
    backend,
    qualification: caps.qualification,
    self_check: true,
    page: s.edit && !s.edit.invisible && s.edit.placement ? s.edit.placement.page_index : null,
  };
  ctx.emit({ kind: 'signed', file_name: fileNameOf(path), path });
  const d = current(s);
  if (s.batch && d && s.batch.openItem === d.id) {
    // aus dem Stapel geöffnet: zurück zur Übersicht statt „Fertig“ (§3.6), Hinweis `ng-batch-item-signed`
    d.signed = true;
    s.batch.singleSigned(d.id, path);
    leaveSingle(s);
    s.explicit = false;
    resetSelection(s, ctx);
  }
}

function save(s: FlowState, path: string | null, ctx: MockCtx): void {
  if (s.stage !== 'output') return;
  const d = current(s);
  if (path === null) {
    if (s.output.saveDialog) s.output = { ...EMPTY_OUTPUT, savePending: true };
    else {
      const name = outputFileName(d?.path ?? 'signiert.pdf', ctx.settings().output.postfix || '_signed');
      s.output = { ...EMPTY_OUTPUT, saveDialog: { directory: folderOf(d?.path ?? ''), file_name: name } };
    }
    return;
  }
  if ((s.opts.existingFiles ?? []).includes(path)) {
    s.output = { ...EMPTY_OUTPUT, askOverwrite: path };
    return;
  }
  saved(s, ctx, path);
}

function navigateOpen(s: FlowState, ctx: MockCtx): void {
  if (s.batch?.running) return;
  const d = current(s);
  if (s.batch && d && fromBatch(s) && s.stage !== 'signing') {
    // aus dem Stapel geöffnet (z. B. „Weiteres Dokument“ nach abgebrochenem Speichern-Dialog): wie der Kern
    // (`batch_run::intercept`) zurück zur Übersicht, Eintrag unverändert – der Stapel bleibt
    s.batch.back(d.id, undefined);
    leaveSingle(s);
    s.explicit = false;
    return;
  }
  if (s.batch) {
    // Startseite: Stapel verwerfen (gespeicherte Dokumente bleiben)
    s.batch.dispose();
    s.batch = null;
    s.signing?.close();
    s.signing = null;
    s.queue = [];
    s.edit = null;
    s.error = null;
    s.output = EMPTY_OUTPUT;
    s.summary = null;
    s.explicit = false;
    s.stage = 'open';
    return;
  }
  if (s.stage === 'output') {
    finishDoc(s, s.output.path ? { kind: 'signed', output: { path: s.output.path, len: 1024, sha256: '00' } } : { kind: 'cancelled' });
    s.queue.shift();
    s.output = EMPTY_OUTPUT;
    s.summary = null;
    s.edit = null;
    s.explicit = false;
    if (s.queue.length > 0) analyze(s, ctx);
    else s.stage = 'open';
  } else if (s.stage !== 'open' && s.stage !== 'signing' && s.stage !== 'starting' && s.stage !== 'finished') {
    s.signing?.close();
    s.signing = null;
    s.queue = [];
    s.edit = null;
    s.error = null;
    s.explicit = false;
    s.stage = 'open';
  }
}

/* ---------------------------------------------------------------- geparkte Arbeit (Nachtrag 10) */

/** Ruht offene Arbeit, die geparkt werden kann (wie `parkable` im Kern)? */
function parkable(s: FlowState): boolean {
  if (s.stage === 'editing' || s.stage === 'warning') return !!current(s) && !!s.edit;
  if (s.stage === 'batch') return !!s.batch && !s.batch.running;
  if (s.stage === 'output') return s.output.savePending && !s.output.path;
  return false;
}

/**
 * „Zur Startseite“ (`go_home`) wie der Kern (`Event::WentHome`): Arbeitsbereich, Analysewarnung, Stapelübersicht und
 * „Fertig“ ohne Speichern werden geparkt (nichts verworfen); gespeichert mit wartenden Dokumenten wird der Stapel
 * samt signiertem Eintrag geparkt (wie „Zum Stapel“); ein Stapeleintrag, der gerade lädt, kehrt zur Übersicht zurück,
 * die geparkt wird. Beim Signieren, im Lauf und im Fehlerschritt ohne Wirkung; sonst wie `navigate{Open}`.
 */
function goHome(s: FlowState, ctx: MockCtx): void {
  if (s.parked || ['starting', 'open', 'signing', 'error', 'finished'].includes(s.stage) || s.batch?.running) return;
  if (parkable(s)) {
    s.parked = true;
    return;
  }
  const [cur, ...rest] = s.queue;
  if (s.stage === 'output' && s.output.path && cur && rest.length > 0 && batchOn(s) && !s.batch) {
    finishDoc(s, { kind: 'signed', output: { path: s.output.path, len: 1024, sha256: '00' } });
    const batch = new MockBatch(batchHost(s, ctx));
    batch.addSigned(cur, s.output.path);
    batch.add(rest);
    s.batch = batch;
    leaveSingle(s);
    s.explicit = false;
    s.parked = true;
    return;
  }
  navigateOpen(s, ctx);
  if (parkable(s)) s.parked = true;
}

/** „Schließen“ der Karte „In Bearbeitung“ (`discard_parked`): verwerfen wie „Öffnen“ (Dokumente „abgebrochen“). */
function discardParked(s: FlowState): void {
  s.parked = false;
  if (!s.batch) for (const d of s.queue) s.done.push({ id: d.id, name: d.name, input: d.path, result: { kind: 'cancelled' } });
  s.batch?.dispose();
  s.batch = null;
  s.signing?.close();
  s.signing = null;
  s.queue = [];
  s.edit = null;
  s.error = null;
  s.output = EMPTY_OUTPUT;
  s.summary = null;
  s.explicit = false;
  s.stage = 'open';
}

/**
 * Führt „Weiter bearbeiten“ zur Stapelübersicht (wie `State::resumes_to_overview` im Kern, ADR 0004 Nachtrag 13)?
 * Mit Stapelübersicht: die Übersicht selbst, ein aus ihr geöffnetes Dokument und ein Dokument mit wartenden
 * (Arbeitsbereich, Analysewarnung) – nicht „Fertig“ ohne Speichern.
 */
function resumesToOverview(s: FlowState): boolean {
  if (!batchOn(s)) return false;
  if (s.stage === 'batch') return !!s.batch;
  if (s.stage === 'editing' || s.stage === 'warning') return !!current(s) && (s.queue.length > 1 || fromBatch(s));
  return false;
}

/**
 * „Weiter bearbeiten“ (`resume_parked`) wie der Kern (`Event::ParkedResumed`, Nachtrag 13): ein Stapel kommt in die
 * Stapelübersicht – wie „Zurück zum Stapel“ bzw. „Zum Stapel“, die Platzierung des offenen Dokuments wird dessen
 * Plan –, sonst unverändert zurück.
 */
function resumeParked(s: FlowState, ctx: MockCtx): void {
  s.parked = false;
  if (!resumesToOverview(s) || s.stage === 'batch') return;
  if (fromBatch(s)) backToBatch(s);
  else showBatch(s, ctx);
}

/**
 * Neue Dateien bei geparkter Arbeit wie der Kern: „Zum Stapel hinzufügen“ (`mode: 'add'`) bzw. – `openWith` – „Öffnen
 * mit“/zweite Instanz (ohne Rückfrage, Hinweis `ng-parked-files-added`). Fortsetzen wie „Weiter bearbeiten“ (ein
 * Stapel ist danach die Übersicht), dann hinzufügen: Arbeitsbereich wie „Weitere PDFs hinzufügen“ (Platzierung bleibt
 * Plan), Übersicht mit neuen Einträgen, Analysewarnung als Eintrag.
 */
function joinParked(s: FlowState, ctx: MockCtx, paths: string[], openWith: boolean): void {
  resumeParked(s, ctx);
  const open = () => s.queue.length + (s.batch?.size ?? 0);
  const before = open();
  const d = current(s);
  if (s.stage === 'editing' && batchOn(s)) addFiles(s, ctx, paths);
  else if (s.stage === 'warning' && d && batchOn(s)) {
    const fresh = paths.filter((p, i) => paths.indexOf(p) === i && !s.queue.some((q) => q.path === p) && !s.batch?.has(p));
    if (fresh.length === 0) return;
    const docs = fresh.map((p) => makeDoc(s, p));
    const b = s.batch;
    if (b && fromBatch(s)) {
      b.back(d.id, undefined);
      b.add(docs);
      leaveSingle(s);
    } else enterBatch(s, ctx, [...s.queue, ...docs]);
  } else openFiles(s, ctx, paths);
  const queued = !batchOn(s) && (s.stage === 'editing' || s.stage === 'warning');
  const added = open() - before;
  if (openWith && added > 0 && !queued) ctx.notify('ng-parked-files-added', { count: String(added) });
}

/** Karte „In Bearbeitung“ (`ViewState::parked`, wie `parked_view` im Kern). */
function parkedView(s: FlowState): ParkedWorkView | null {
  if (!s.parked) return null;
  const e = s.edit;
  const byHand = !!e && (s.stage === 'editing' || s.stage === 'warning' || s.stage === 'output') && !e.automatic;
  const unsaved = s.stage === 'output';
  // „Weiter bearbeiten“ führt zur Stapelübersicht → „Stapel · n Dokumente“, sonst zum Dokument (Nachtrag 13)
  const doc_name = resumesToOverview(s) ? null : (current(s)?.name ?? null);
  const b = s.batch;
  if (b) return { doc_name, documents: b.size, placed_by_hand: byHand || b.view().manual_plans > 0, unsaved };
  return { doc_name, documents: s.queue.length, placed_by_hand: byHand, unsaved };
}

/** Platzierung zulässig (z. B. kein neues sichtbares Feld auf DocMDP-zertifizierten Dokumenten). */
function placementAllowed(s: FlowState): boolean {
  const d = current(s);
  const e = s.edit;
  if (!d || !e) return false;
  const c = constraintsFor(d, e);
  return e.invisible || e.target.kind !== 'free' || c.new_visible_field_allowed;
}

function editEvent(s: FlowState, ctx: MockCtx, fn: (d: MockDoc, e: EditState) => EditState | null, rejected?: string): void {
  const d = current(s);
  if (s.stage !== 'editing' || !d || !s.edit) return;
  const next = fn(d, s.edit);
  if (next === null) {
    if (rejected) ctx.notify(rejected);
    return;
  }
  // Prüfung liest das Raster nur, wenn es für die Seite schon berechnet ist
  s.edit = next;
}

function scenarioDocs(o: MockOptions): { path: string; spec: MockDocSpec }[] {
  const dir = '/Users/demo/Dokumente';
  switch (o.scenario) {
    case 'blank':
      return [{ path: `${dir}/Leeres_Dokument.pdf`, spec: blankSpec() }];
    case 'contract':
    case 'error-atrust':
      return [{ path: `${dir}/Verträge/Vertrag.pdf`, spec: contractSpec() }];
    case 'field':
      return [{ path: `${dir}/Förderung/Antrag_Foerderung.pdf`, spec: fieldSpec() }];
    case 'signed':
      return [{ path: `${dir}/Wohnung/Mietvertrag_signiert.pdf`, spec: signedSpec() }];
    case 'batch':
      return [
        { path: `${dir}/Angebote/Angebot_2026-118.pdf`, spec: offerSpec() },
        { path: `${dir}/Aufträge/Auftragsbestaetigung_4711.pdf`, spec: confirmationSpec() },
        { path: `${dir}/Lieferschein_Scan.pdf`, spec: lockedSpec() },
        { path: `${dir}/Verträge/Vertrag.pdf`, spec: contractSpec() },
      ];
    case 'error-password':
      return [{ path: `${dir}/Personal/Gehaltszettel_2026-09.pdf`, spec: lockedSpec() }];
    default:
      return [];
  }
}

function bar(s: FlowState): MainBarView {
  switch (s.stage) {
    case 'starting':
    case 'finished':
      return mainBar(['', '', '', ''], false);
    case 'open':
    case 'loading':
    case 'warning':
    case 'error':
      return mainBar(['AE', '', '', ''], true);
    case 'editing':
      return mainBar(['AE', 'A', '', ''], true);
    case 'choosing':
      return mainBar(['AE', 'AE', 'A', ''], true);
    case 'signing':
      return mainBar(['A', 'A', 'A', ''], false);
    case 'output':
      return mainBar(['AE', 'AE', 'AE', 'A'], true);
    case 'batch':
      return s.batch?.running ? mainBar(['A', 'A', 'A', ''], false) : mainBar(['AE', '', '', ''], true);
  }
}

function stepView(s: FlowState, ctx: MockCtx): StepView {
  const d = current(s);
  const queue = s.queue.map((q, i) => ({ id: q.id, name: q.name, current: i === 0 }));
  switch (s.stage) {
    case 'starting':
      return { step: 'starting' };
    case 'open':
      return { step: 'open', queue };
    case 'loading':
      return { step: 'loading', queue };
    case 'warning':
      return d ? { step: 'warning', doc: docInfo(d), codes: [...(s.opts.warnings ?? [])] } : { step: 'open', queue };
    case 'editing': {
      if (!d || !s.edit?.placement) return { step: 'open', queue };
      const p = s.edit.placement;
      const size = blockSize(ctx.settings(), blockLang(ctx));
      return {
        step: 'position',
        doc: docInfo(d),
        page: p.page_index,
        block: { width: size.width, height: size.height, preview_revision: s.preview, emblem_placeholder: emblemPlaceholder(ctx.settings()), emblem_choosable: true, language: blockLang(ctx) },
        placement: { ...p },
        new_page: s.edit.newPage,
        announce: {
          page: String(p.page_index + 1),
          pages: String(d.spec.pages.length + (s.edit.newPage ? 1 : 0)),
          left: String(Math.round((p.x * 25.4) / 72)),
          top: String(Math.round((p.y * 25.4) / 72)),
        },
      };
    }
    case 'choosing':
      return {
        step: 'backend',
        options: signerOptions(s, ctx).map((o) => ({ id: o.backend, available: o.available, capabilities: o.capabilities, notes: o.notes })),
      };
    case 'signing': {
      const session = s.signing;
      const leaves = session ? capabilitiesFor(session.backend).document_leaves_device : false;
      return {
        step: 'signing',
        interaction: session ? withStorageQuestion(session.interaction, ctx.settings(), s.opts.keychainAvailable ?? true) : { kind: 'waiting', progress: 'Connecting' },
        queue,
        document_leaves_device: leaves,
        progress: session ? session.progress() : defaultSigningProgress(),
      };
    }
    case 'output': {
      const next = s.queue[1];
      return {
        step: 'output',
        result: { path: s.output.path, save_pending: s.output.savePending, ask_overwrite: s.output.askOverwrite, save_dialog: s.output.saveDialog ? { ...s.output.saveDialog } : null, summary: s.summary ? { ...s.summary } : null },
        next: next ? docInfo(next) : null,
        pending: Math.max(0, s.queue.length - 1),
      };
    }
    case 'error': {
      const e = s.error;
      return {
        step: 'error',
        code: e?.code ?? 'error-Unexpected',
        params: { ...(e?.params ?? {}) },
        retry: e?.retry ?? false,
        context: e ? { origin: e.origin, occurred_at: e.occurredAt, backend: e.backend, doc: d ? { id: d.id, name: d.name } : null } : defaultErrorContext(),
      };
    }
    case 'finished':
      return { step: 'finished', exit_code: s.exitCode };
    case 'batch': {
      const b = s.batch;
      if (!b?.running) return { step: 'batch' };
      const backend = b.backend();
      return {
        step: 'signing',
        interaction: withStorageQuestion(b.interaction(), ctx.settings(), s.opts.keychainAvailable ?? true),
        queue: b.docs().map((q, i) => ({ id: q.id, name: q.name, current: i === 0 })),
        document_leaves_device: backend ? capabilitiesFor(backend).document_leaves_device : false,
        progress: b.progress(),
      };
    }
  }
}

function actionsView(s: FlowState, b: MainBarView): ActionsView {
  const editing = s.stage === 'editing';
  if (s.stage === 'batch' && s.batch) {
    const running = s.batch.running;
    return { ...noActions(), open_files: !running, start_batch: !running && s.batch.signable().length > 0, settings: !running && b.settings_enabled, home: !running };
  }
  return {
    ...noActions(),
    open_files: !['starting', 'finished', 'signing'].includes(s.stage),
    sign: editing && placementAllowed(s),
    edit_placement: editing,
    // weitere Dokumente in der Warteschlange (Einzeldokument, nicht aus dem Stapel geöffnet)
    show_batch: batchOn(s) && !s.batch && (editing || s.stage === 'warning') && s.queue.length > 1,
    settings: b.settings_enabled,
    home: b.steps[0]?.enabled ?? false,
  };
}

const flowArea: MockArea<FlowState> = {
  init(o) {
    const s: FlowState = {
      opts: o,
      stage: 'starting',
      queue: [],
      nextDocId: 1,
      edit: null,
      preview: 1,
      signing: null,
      selected: null,
      explicit: false,
      remembered: false,
      output: EMPTY_OUTPUT,
      summary: null,
      error: null,
      done: [],
      resume: o.resumeOffer ?? null,
      exitCode: 0,
      networkErrorOnce: o.scenario === 'error-atrust',
      last: { position: 'manual', backend: null },
      batch: null,
      pendingPlan: null,
      parked: false,
    };
    return s;
  },

  handle(e, s, ctx) {
    // geparkte Arbeit (Nachtrag 10): verdeckt – nur Fortsetzen, Verwerfen, neue Dateien und Beenden wirken (F-54)
    if (s.parked) {
      switch (e.type) {
        case 'resume_parked':
          resumeParked(s, ctx);
          return true;
        case 'discard_parked':
          discardParked(s);
          return true;
        case 'open_files':
          if (e.mode === 'replace') {
            discardParked(s);
            openFiles(s, ctx, e.paths);
          } else joinParked(s, ctx, e.paths, e.mode !== 'add');
          return true;
        case 'quit':
        case 'select_signer':
          break;
        default:
          return true;
      }
    }
    if (e.type === 'go_home') {
      goHome(s, ctx);
      return true;
    }
    const d = current(s);
    const settings = ctx.settings();
    const lang = blockLang(ctx);
    const b = s.batch;
    // Stapellauf: Masken, Pausen, Überspringen, Abbruch (§3.8); `Cancel` im Lauf = `AbortBatch`
    if (s.stage === 'batch' && b?.running) {
      switch (e.type) {
        case 'submit_form':
          b.session()?.submitForm(e.request, e.answer);
          return true;
        case 'submit_secret':
          b.session()?.submitSecret(e.request, e.secret);
          return true;
        case 'answer_notice':
          b.session()?.answerNotice(e.request, e.answer);
          return true;
        case 'retry':
          b.retry();
          return true;
        case 'skip_batch_document':
          b.skip();
          return true;
        case 'abort_batch':
        case 'cancel':
          b.abort();
          return true;
        case 'quit':
          b.dispose();
          s.stage = 'finished';
          return true;
        case 'open_files':
          openFiles(s, ctx, e.paths);
          return true;
        case 'select_signer':
        case 'dismiss_notice':
          break;
        default:
          return e.type === 'open_signature_data';
      }
    }
    switch (e.type) {
      case 'open_files':
        // „+ PDF“ (Nachtrag 9): was „hinzufügen“ bzw. „anderes öffnen“ bedeutet, entscheidet der Kern – der Mock folgt
        if (e.mode === 'add') addFiles(s, ctx, e.paths);
        else if (e.mode === 'replace') replaceDocument(s, ctx, e.paths);
        else openFiles(s, ctx, e.paths);
        return true;
      case 'show_batch':
        if (!b && d && s.queue.length > 1) showBatch(s, ctx);
        return true;
      case 'open_batch_item':
        if (s.stage === 'batch' && b) b.open(e.doc);
        return true;
      case 'back_to_batch':
        if (s.stage === 'editing' || s.stage === 'warning' || s.stage === 'loading') backToBatch(s);
        return true;
      case 'remove_from_batch':
        if (s.stage === 'batch' && b) {
          b.remove(e.doc);
          if (b.size === 0) {
            s.batch = null;
            s.stage = 'open';
          }
        }
        return true;
      case 'start_batch':
        if (mobileKeystore(ctx, e.backend)) return true;
        if (s.stage === 'batch' && b) b.start(e);
        return true;
      case 'set_batch_auto_position':
        // „Für diesen Stapel automatisch positionieren“ (Nachtrag 8): startet nichts
        if (s.stage === 'batch' && b) b.setAutoPosition(e.enabled, e.replace_manual);
        return true;
      case 'skip_batch_document':
      case 'abort_batch':
        return true;
      case 'answer_warning':
        if (s.stage !== 'warning') return true;
        if (e.proceed) s.stage = 'editing';
        else if (b && d && fromBatch(s)) {
          // „Dokument verwerfen“ aus dem Stapel: Eintrag entfernen, zurück zur Übersicht
          b.back(d.id, undefined);
          b.remove(d.id);
          leaveSingle(s);
          if (b.size === 0) {
            s.batch = null;
            s.stage = 'open';
          }
        } else {
          finishDoc(s, { kind: 'cancelled' });
          dropCurrent(s, ctx);
        }
        return true;
      // Einstellung „Automatische Positionierung“ (Kern: `WorkspaceEdit::placement_locked`, Nachtrag 8):
      // gesperrt verschieben weder Ziehen noch Tastatur, Seitenwechsel oder Zielwahl den Block
      case 'move_block':
      case 'nudge_block':
      case 'change_page':
      case 'auto_place':
      case 'use_target':
      case 'add_block':
      case 'select_block':
      case 'remove_block':
      case 'fill_targets':
        if (s.edit?.lock === 'locked') return true;
        break;
      case 'unlock_placement':
        // „Diesmal selbst platzieren“: nur dieses Dokument, Lage unverändert
        editEvent(s, ctx, (_doc, ed) => (ed.lock === 'locked' ? { ...ed, lock: 'released' } : ed));
        return true;
      default:
        break;
    }
    switch (e.type) {
      case 'move_block':
        editEvent(s, ctx, (doc, ed) => moveBlock(doc, ed, e.page, e.x, e.y));
        return true;
      case 'nudge_block':
        editEvent(s, ctx, (doc, ed) => nudgeBlock(doc, ed, e.dx, e.dy, e.fine, e.coarse));
        return true;
      case 'change_page':
        editEvent(s, ctx, (doc, ed) => changePage(doc, ed, e.page));
        return true;
      case 'toggle_new_page':
        editEvent(s, ctx, (doc, ed) => toggleNewPage(doc, ed), (s.edit?.others?.length ?? 0) > 0 ? 'ng-blocks-new-page' : 'placement.notice.new_page_not_allowed');
        return true;
      // Mehrfachsignatur: je Block eine eigene Signatur (Kern: `BlockAdded`, `BlockSelected`, `BlockRemoved`, `TargetsFilled`)
      case 'add_block': {
        const d0 = current(s);
        const refused = d0 && s.edit && s.stage === 'editing' ? addBlock(d0, s.edit, e.page ?? null, blockSize(settings, lang)) : null;
        if (typeof refused === 'string') ctx.notify(refused);
        else if (refused) s.edit = refused;
        return true;
      }
      case 'select_block':
        editEvent(s, ctx, (doc, ed) => selectBlock(doc, ed, e.index));
        return true;
      case 'remove_block':
        editEvent(s, ctx, (doc, ed) => removeBlock(doc, ed, e.index));
        return true;
      case 'fill_targets':
        editEvent(s, ctx, (doc, ed) =>
          fillTargets(doc, ed, blockSize(settings, lang), (settings.placeholder.search && settings.placeholder.kind === 'marker') || (ed.target.kind === 'qr' && !ed.automatic)),
        );
        return true;
      case 'auto_place':
        editEvent(s, ctx, (doc, ed) => {
          // „Neu platzieren“/„Freie Fläche suchen“ (Kern: `TargetChosen(Free)`): folgt danach nicht mehr der
          // Platzhaltersuche
          const next = { ...autoPlace(doc, ed, blockSize(settings, lang)), automatic: false };
          if (next.placement && !next.ready.includes(next.placement.page_index)) next.ready = [...next.ready, next.placement.page_index];
          return next;
        });
        return true;
      case 'set_invisible':
        editEvent(s, ctx, (_doc, ed) => (ed.invisibleProfile && !e.invisible ? null : { ...ed, invisible: e.invisible, revision: ed.revision + 1 }), 'ng-constraint-invisible-profile');
        return true;
      case 'use_target':
        editEvent(s, ctx, (doc, ed) => useTarget(doc, ed, e.target, blockSize(settings, lang)), s.edit && targetTaken(s.edit, e.target) ? 'ng-block-target-taken' : 'ng-document-rejected');
        return true;
      case 'inspect_pages': {
        if (!s.edit || !d) return true;
        const pages = [...new Set(e.pages.filter((p) => Number.isInteger(p) && p >= 0 && p < d.spec.pages.length))].slice(0, 8);
        s.edit = { ...s.edit, inspected: pages };
        // Raster asynchron (niedrige Priorität), danach neue Projektion mit derselben Revision
        ctx.later(() => {
          if (s.edit && current(s) === d) s.edit = { ...s.edit, ready: [...new Set([...s.edit.ready, ...pages])] };
        }, 0.5);
        return true;
      }
      case 'select_signer':
        if (mobileUnofferedSigner(ctx.platform().mobile, e.backend)) return true;
        s.selected = e.backend && available(s, ctx, e.backend) ? e.backend : null;
        s.explicit = e.backend !== null;
        return true;
      case 'sign_with':
        if (mobileKeystore(ctx, e.backend)) return true;
        if (s.stage === 'choosing') startSigning(s, ctx, e.backend);
        else if (s.stage === 'editing' && placementAllowed(s)) startSigning(s, ctx, e.backend);
        return true;
      case 'confirm_position': {
        // Alias im Arbeitsbereich-Modus (§3.2): nur mit gewähltem Signiermittel
        const backend = selectedSigner(s, ctx);
        if (s.stage === 'editing' && backend) startSigning(s, ctx, backend);
        return true;
      }
      case 'choose_invisible': {
        const backend = selectedSigner(s, ctx);
        if (s.stage === 'editing' && s.edit && backend) {
          s.edit = { ...s.edit, invisible: true, revision: s.edit.revision + 1 };
          startSigning(s, ctx, backend);
        }
        return true;
      }
      case 'choose_backend':
        if (mobileKeystore(ctx, e.backend)) return true;
        if (s.stage === 'choosing') startSigning(s, ctx, e.backend);
        return true;
      case 'submit_form':
        s.signing?.submitForm(e.request, e.answer);
        return true;
      case 'submit_secret':
        s.signing?.submitSecret(e.request, e.secret);
        return true;
      case 'answer_notice':
        s.signing?.answerNotice(e.request, e.answer);
        return true;
      case 'open_signature_data':
        return true;
      case 'cancel':
        if (b && fromBatch(s) && (s.stage === 'loading' || s.stage === 'warning' || s.stage === 'editing')) {
          // aus dem Stapel geöffnet: zurück zur Übersicht, Status unverändert
          if (d) b.back(d.id, undefined);
          leaveSingle(s);
        } else if (s.stage === 'signing') cancelSigning(s, ctx);
        else if (s.stage === 'choosing') {
          s.stage = s.edit ? 'editing' : 'open';
          ctx.notify('ng-signing-cancelled');
        } else if (s.stage === 'error') {
          const cancel = s.error?.onCancel;
          s.error = null;
          cancel?.();
        } else if (s.stage === 'loading' || s.stage === 'warning' || s.stage === 'editing') {
          finishDoc(s, { kind: 'cancelled' });
          dropCurrent(s, ctx);
        }
        return true;
      case 'retry':
        if (s.stage === 'error') {
          const retry = s.error?.onRetry;
          if (s.error?.origin !== 'signing') s.error = null;
          retry?.();
        }
        return true;
      case 'save':
        save(s, e.path, ctx);
        return true;
      case 'confirm_overwrite':
        if (!s.output.askOverwrite) return true;
        if (e.overwrite) saved(s, ctx, s.output.askOverwrite);
        else s.output = { ...EMPTY_OUTPUT, savePending: true };
        return true;
      case 'navigate':
        if (e.step === 'Open') navigateOpen(s, ctx);
        return true;
      case 'resume_signature': {
        const offer = s.resume;
        if (!offer || s.stage !== 'open') return true;
        s.resume = null;
        const doc = makeDoc(s, `${SANDBOX}/Library/tmp/resume/document/${offer.name}`, contractSpec());
        s.queue.unshift(doc);
        s.edit = startEdit(doc, settings, lang);
        if (available(s, ctx, offer.backend)) startSigning(s, ctx, offer.backend);
        else s.stage = 'choosing';
        return true;
      }
      case 'discard_resume':
        s.resume = null;
        return true;
      case 'quit':
        s.signing?.close();
        s.signing = null;
        s.batch?.dispose();
        s.stage = 'finished';
        s.exitCode = 0;
        return true;
      default:
        return false;
    }
  },

  signal(sig, s, ctx) {
    if (sig.kind === 'settings-changed') {
      s.preview += 1;
      const d = current(s);
      if (d && s.edit && (s.stage === 'editing' || s.stage === 'warning')) {
        s.edit = resizeBlock(d, s.edit, sig.settings, sig.settings.signature.block_language ?? sig.settings.general.ui_language ?? 'de');
        s.edit = { ...s.edit, lock: followAutoPosition(s.edit, sig.settings, fromBatch(s)) };
      }
      if (!s.explicit) resetSelection(s, ctx);
      s.batch?.settingsChanged();
    } else if (sig.kind === 'open-files') {
      // „Erneut signieren“ aus „Zuletzt signiert“ (`sign_recent`, Nachtrag 15): wie `open_files` mit demselben `mode`
      // (bei geparkter Arbeit ohne `mode` wie „Öffnen mit“ dazu, Kern: `FilesAdded`)
      const openBefore = new Set(openDocs(s).map((d) => d.id));
      flowArea.handle(sig.mode ? { type: 'open_files', paths: sig.paths, mode: sig.mode } : { type: 'open_files', paths: sig.paths }, s, ctx);
      reportDuplicates(s, sig.paths, openBefore, ctx);
    }
  },

  project(s, ctx) {
    if (s.parked) {
      // Startansicht über der geparkten Arbeit: Schritt, Hauptleiste und Freigaben wie im Leerlauf
      const bp = mainBar(['AE', '', '', ''], true);
      return {
        step: { step: 'open', queue: [] },
        main_bar: bp,
        done: s.done.map((x) => structuredClone(x)),
        resume: null,
        document: null,
        signers: signersProjection(s, ctx),
        batch: null,
        actions: { ...noActions(), open_files: true, settings: bp.settings_enabled, home: true },
        parked: parkedView(s),
      };
    }
    const d = current(s);
    const b = bar(s);
    const lang = blockLang(ctx);
    const phase = s.stage === 'warning' ? 'warning' : s.stage === 'editing' ? 'editing' : s.stage === 'signing' || s.stage === 'choosing' ? 'signing' : s.stage === 'output' ? 'done' : s.stage === 'error' && s.error?.origin === 'signing' ? 'signing' : null;
    const signers = signersProjection(s, ctx);
    const actions = actionsView(s, b);
    // wie der Kern (Nachtrag 20): ohne Auswahl und ohne Signiermittel (Richtlinie schließt jedes aus) nichts zu signieren
    if (!signers.choice_offered && signers.selected === null) {
      actions.sign = false;
      actions.start_batch = false;
    }
    return {
      step: stepView(s, ctx),
      main_bar: b,
      done: s.done.map((x) => structuredClone(x)),
      resume: s.stage === 'open' && s.resume ? { ...s.resume } : null,
      document: d && s.edit && phase ? projectDocument(d, s.edit, phase, ctx.settings(), lang, s.preview) : null,
      signers,
      batch: s.batch ? s.batch.view() : null,
      actions,
      parked: null,
    };
  },
};

/* ================================================================ MockCore */

interface Area {
  area: MockArea<unknown>;
  state: unknown;
}

/** Mock-Kern samt nativer Dienste. */
export class MockCore implements Bridge {
  readonly kind = 'mock' as const;

  /** alle gesendeten Ereignisse (Tests) */
  readonly dispatched: UiEvent[] = [];
  /** im Systembrowser geöffnete URLs (Tests) */
  readonly openedUrls: string[] = [];
  /** Aufrufe nativer Dienste (Tests) */
  readonly nativeCalls: string[] = [];
  /** Optionen der Dateidialoge (`pickFiles`; Tests: Titel bzw. Hinweiszeile, Mehrfachauswahl) */
  readonly pickFilesCalls: Array<{ title: string; multiple: boolean }> = [];

  private readonly opts: MockOptions;
  private readonly listeners = new Set<(v: ViewState) => void>();
  private readonly dragListeners = new Set<(e: FileDragEvent) => void>();
  private forced: ViewState | null = null;
  private readonly timers = new Set<ReturnType<typeof setTimeout>>();
  private readonly flow: FlowState;
  private readonly areas: Area[];
  private notices: NoticeView[] = [];
  private noticeId = 0;
  /** Hinweis zum Teilen (iOS, Nachtrag 12): über „Teilen“ empfangenes Dokument bzw. ausgeblendet – wie der Kern */
  private shareHint: DocId | null = null;
  private shareHintDismissed = false;
  private readonly ctx: MockCtx;

  constructor(options: MockOptions = {}) {
    this.opts = { ...options };
    this.flow = flowArea.init(this.opts);
    this.areas = [
      { area: flowArea as MockArea<unknown>, state: this.flow },
      { area: settingsArea as MockArea<unknown>, state: settingsArea.init(this.opts) },
      { area: recentArea as MockArea<unknown>, state: recentArea.init(this.opts) },
      { area: firstRunArea as MockArea<unknown>, state: firstRunArea.init(this.opts) },
      // Nachtrag 21: „Über PIDA“ und Update-Prüfung
      { area: aboutArea as MockArea<unknown>, state: aboutArea.init(this.opts) },
      // Nachtrag 24: „QR-Platzhalter erstellen“
      { area: qrArea as MockArea<unknown>, state: qrArea.init(this.opts) },
    ];
    const settingsState = this.areas[1]?.state as ReturnType<typeof settingsArea.init>;
    this.ctx = {
      emit: (signal: MockSignal) => {
        for (const a of this.areas) a.area.signal?.(signal, a.state, this.ctx);
      },
      native: (call) => this.nativeCalls.push(call),
      settings: () => settingsState.settings,
      platform: () => this.platformView(),
      notify: (code, params = {}) => this.notify(code, params),
      later: (fn, factor = 1) => this.later(fn, factor),
    };
    if (options.initialView) {
      this.forced = options.initialView;
    } else {
      this.later(() => {
        this.flow.stage = 'open';
        const docs = scenarioDocs(this.opts).map(({ path, spec }) => makeDoc(this.flow, path, spec));
        if (docs.length >= 2 && batchOn(this.flow)) enterBatch(this.flow, this.ctx, docs);
        else if (docs.length > 0) {
          this.flow.queue.push(...docs);
          analyze(this.flow, this.ctx);
        }
      });
    }
  }

  // ------------------------------------------------------------------------------------------
  // CoreBridge
  // ------------------------------------------------------------------------------------------

  async view(): Promise<ViewState> {
    return this.current();
  }

  onState(cb: (v: ViewState) => void): () => void {
    this.listeners.add(cb);
    return () => this.listeners.delete(cb);
  }

  private doc(doc: DocId): MockDoc | undefined {
    return this.flow.queue.find((q) => q.id === doc) ?? this.flow.batch?.docs().find((q) => q.id === doc);
  }

  pageTileUrl(doc: DocId, page: number, scale: number): string {
    const d = this.doc(doc);
    const p = d?.spec.pages[page] ?? null;
    const last = d?.spec.pages[d.spec.pages.length - 1];
    const size: [number, number] = p ? [p.w, p.h] : last ? [last.w, last.h] : A4;
    return pageTileSvg(p, size, scale);
  }

  /** Miniatur der ersten Seite (Voranalyse des Stapels, `pida-render://thumb/<doc>`; Breite 96 px). */
  thumbUrl(doc: DocId): string {
    const d = this.doc(doc);
    const p = d?.spec.pages[0] ?? null;
    const size: [number, number] = p ? [p.w, p.h] : A4;
    return pageTileSvg(p, size, 96 / size[0]);
  }

  qrPlaceholderUrl(spec: QrPlaceholderSpec): string {
    return qrPlaceholderSvg(spec);
  }

  blockPreviewUrl(scale: number, revision: number): string {
    const settings = this.ctx.settings();
    return blockPreviewSvg(settings, settings.signature.block_language ?? settings.general.ui_language ?? this.opts.language ?? 'de', scale, revision);
  }

  async dispatch(event: UiEvent): Promise<void> {
    this.dispatched.push(event);
    if (this.forced) return; // feste Testzustände reagieren nicht; Tests prüfen `dispatched`
    if (event.type === 'dismiss_notice') this.notices = this.notices.filter((n) => n.id !== event.id);
    // von außen übergebene Dateien (Shell, Nachtrag 12): wie `open_files` ohne `mode`; auf iOS danach der Hinweis
    // zum Teilen für das empfangene Dokument, sofern nicht ausgeblendet (wie der Kern)
    const routed: UiEvent = event.type === 'files_received' ? { type: 'open_files', paths: event.paths } : event;
    const openBefore = routed.type === 'open_files' ? new Set(openDocs(this.flow).map((d) => d.id)) : null;
    const firstNew = this.flow.nextDocId;
    for (const a of this.areas) a.area.handle(routed, a.state, this.ctx);
    if (routed.type === 'open_files' && openBefore) reportDuplicates(this.flow, routed.paths, openBefore, this.ctx);
    if (event.type === 'files_received' && this.platformView().os === 'ios' && !this.shareHintDismissed && this.flow.nextDocId > firstNew) this.shareHint = firstNew;
    if (event.type === 'dismiss_share_hint') {
      this.shareHint = null;
      this.shareHintDismissed = true;
    }
    const session = this.flow.signing ?? this.flow.batch?.session() ?? null;
    if (event.type === 'open_signature_data' && session) {
      const url = session.signatureDataUrl(event.request);
      if (url) this.openedUrls.push(url);
    }
    if (event.type === 'open_result' && this.flow.output.path) this.nativeCalls.push(`openResult:${this.flow.output.path}`);
    if (event.type === 'share_result' && this.flow.output.path) this.nativeCalls.push(`shareResult:${this.flow.output.path}`);
    if (event.type === 'export_result' && this.flow.output.path) {
      this.nativeCalls.push(`exportResult:${this.flow.output.path}`);
      this.notify('ng-export-done', { name: fileNameOf(this.flow.output.path) });
    }
    this.emit();
  }

  // ------------------------------------------------------------------------------------------
  // PlatformServices
  // ------------------------------------------------------------------------------------------

  async pickFiles(options: { title: string; multiple: boolean }): Promise<string[] | null> {
    this.nativeCalls.push(`pickFiles:${options.multiple ? 'multiple' : 'single'}`);
    this.pickFilesCalls.push({ title: options.title, multiple: options.multiple });
    const result = this.opts.pickFilesResult === undefined ? ['/Users/demo/Dokumente/Vertrag.pdf', '/Users/demo/Dokumente/Angebot 2026.pdf'] : this.opts.pickFilesResult;
    if (result === null) return null;
    return options.multiple ? [...result] : result.slice(0, 1);
  }

  async pickSavePath(options: { defaultPath?: string | null }): Promise<string | null> {
    this.nativeCalls.push(`pickSavePath:${options.defaultPath ?? ''}`);
    const r = this.opts.pickSaveResult === undefined ? '' : this.opts.pickSaveResult;
    if (r === null) return null;
    return r || options.defaultPath || '/Users/demo/Dokumente/signiert.pdf';
  }

  async pickDirectory(): Promise<string | null> {
    this.nativeCalls.push('pickDirectory');
    return this.opts.pickDirectoryResult === undefined ? '/Users/demo/Signiert' : this.opts.pickDirectoryResult;
  }

  async pickMobileFiles(options: { kind: 'pdf'; multiple: boolean }): Promise<string[]> {
    this.nativeCalls.push(`pickMobileFiles:${options.kind}:${options.multiple ? 'multiple' : 'single'}`);
    if (!mobile(this.opts)) throw new Error('ng-mobile-unsupported');
    const sandbox = `${SANDBOX}/Library/Caches/import/1`;
    const result = this.opts.pickMobileFilesResult ?? [`${sandbox}/Vertrag.pdf`, `${sandbox}/Angebot.pdf`];
    return options.multiple ? [...result] : result.slice(0, 1);
  }

  async pickMobileDocuments(options: { multiple: boolean }): Promise<MobilePick> {
    this.nativeCalls.push(`pickMobileDocuments:${options.multiple ? 'multiple' : 'single'}`);
    if (!mobile(this.opts)) throw new Error('ng-mobile-unsupported');
    const sandbox = `${SANDBOX}/Library/Caches/import/1`;
    const result = this.opts.pickMobileFilesResult ?? [`${sandbox}/Vertrag.pdf`, `${sandbox}/Angebot.pdf`];
    return { paths: options.multiple ? [...result] : result.slice(0, 1), skipped: this.opts.pickMobileSkipped ?? 0 };
  }

  private platformView(): PlatformView {
    switch (this.opts.platform) {
      case 'ios':
        return { os: 'ios', mobile: true };
      case 'android':
        return { os: 'android', mobile: true };
      default:
        return { os: 'macos', mobile: false };
    }
  }

  onFileDrag(cb: (e: FileDragEvent) => void): () => void {
    this.dragListeners.add(cb);
    return () => this.dragListeners.delete(cb);
  }

  /** Simuliert Drag & Drop über das Fenster (Tests, Browser-Vorschau). */
  simulateFileDrag(event: FileDragEvent): void {
    for (const cb of this.dragListeners) cb(event);
  }

  async openUrl(url: string): Promise<void> {
    this.openedUrls.push(url);
  }

  async revealOutput(doc?: DocId | null): Promise<void> {
    this.nativeCalls.push(doc === undefined || doc === null ? 'revealOutput' : `revealOutput:${doc}`);
  }

  async openLogDir(): Promise<void> {
    this.nativeCalls.push('openLogDir');
  }

  async licenseText(kind: LicenseKind): Promise<string> {
    this.nativeCalls.push(`licenseText:${kind}`);
    if (kind === 'eupl') return 'EUROPEAN UNION PUBLIC LICENCE v. 1.2\nEUPL © the European Union 2007, 2016\n';
    if (this.opts.notices === false) throw new Error('ng-ui-licenses-missing');
    if (kind === 'notices') return '# THIRD-PARTY-NOTICES – PIDA\n\n## PDFium\n\nBSD-3-Clause …\n\n## Android-Bibliotheken (Android libraries)\n';
    return '<!DOCTYPE html><html><head><title>Copyright notices for The Rust Standard Library</title></head><body><h1>Copyright notices for The Rust Standard Library</h1><p>The Rust Standard Library is dual-licensed under Apache 2.0 and MIT terms.</p><ul><li>core</li><li>alloc</li></ul><script>window.__pwned = true</script></body></html>';
  }

  async demoDocument(): Promise<string> {
    this.nativeCalls.push('demoDocument');
    return mobile(this.opts) ? `${SANDBOX}/Library/Caches/demo/PIDA-Demo.pdf` : '/Users/demo/Library/Caches/at.asit.pida/demo/PIDA-Demo.pdf';
  }

  async appInfo() {
    return { version: '0.2.0', platform: mobile(this.opts) ? (this.opts.platform ?? 'mock') : 'mock', core_available: true };
  }

  // ------------------------------------------------------------------------------------------
  // Steuerung für Tests
  // ------------------------------------------------------------------------------------------

  /** Setzt einen festen Zustand (Ereignisse werden nur protokolliert). */
  setView(view: ViewState): void {
    this.forced = view;
    this.emit();
  }

  /** Long-Poll meldet „bestätigt“ (App/QR) bzw. die Chipkarten-Software ist fertig. */
  completeConfirmation(): void {
    if (this.flow.stage === 'signing' && this.flow.signing?.completeConfirmation()) this.emit();
    else if (this.flow.stage === 'batch' && this.flow.batch?.session()?.completeConfirmation()) this.emit();
  }

  /** Gewählte Position und Signiermittel des letzten Signiervorgangs (Tests). */
  get choices(): { position: string; backend: BackendId | null } {
    return { ...this.flow.last };
  }

  /** Alle Zeitgeber stoppen. */
  dispose(): void {
    this.flow.batch?.dispose();
    for (const t of this.timers) clearTimeout(t);
    this.timers.clear();
    this.listeners.clear();
    this.dragListeners.clear();
  }

  // ------------------------------------------------------------------------------------------
  // Zustand → ViewState
  // ------------------------------------------------------------------------------------------

  private current(): ViewState {
    if (this.forced) return this.forced;
    const settings = this.ctx.settings();
    const base: ViewState = {
      step: { step: 'starting' },
      main_bar: mainBar(['', '', '', ''], false),
      settings: null,
      language: settings.general.ui_language ?? this.opts.language ?? 'de',
      accessibility: { ...(settings.accessibility ?? defaultAccessibility()) },
      notices: this.notices.map((n) => ({ ...n, params: { ...n.params } })),
      migration: null,
      done: [],
      platform: this.platformView(),
      resume: null,
      keystore_import: null,
      privacy_text: this.opts.privacyText ?? null,
      ...workspaceDefaults(),
    };
    const view = this.areas.reduce<ViewState>((v, a) => ({ ...v, ...a.area.project(a.state, this.ctx) }), base);
    // Hinweis zum Teilen (Nachtrag 12): nur, solange das empfangene Dokument allein offen ist (wie der Kern)
    const alone = this.shareHint !== null && view.document?.doc.id === this.shareHint && !view.batch && this.flow.queue.length === 1;
    view.share_hint = alone && this.shareHint !== null ? { doc: this.shareHint } : null;
    // Einstellungen offen: Einstellungen nicht erneut erreichbar (wie `main_bar.settings_enabled` im Kern)
    if (view.settings) {
      view.main_bar = { ...view.main_bar, settings_enabled: false };
      view.actions = { ...view.actions, settings: false };
    }
    return view;
  }

  private emit(): void {
    const view = this.current();
    for (const cb of this.listeners) cb(view);
  }

  private later(fn: () => void, factor = 1): void {
    const t = setTimeout(
      () => {
        this.timers.delete(t);
        fn();
        this.emit();
      },
      (this.opts.delayMs ?? 350) * factor,
    );
    this.timers.add(t);
  }

  private notify(code: string, params: Record<string, string> = {}): void {
    // wie der Kern: gleiche Meldung nicht doppelt stapeln
    if (this.notices.some((n) => n.code === code && JSON.stringify(n.params) === JSON.stringify(params))) return;
    this.noticeId += 1;
    this.notices.push({ id: this.noticeId, code, params });
  }
}

/** Mock-Kern als Bridge. */
export function createMockBridge(options?: MockOptions): MockCore {
  return new MockCore(options);
}
