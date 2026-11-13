/**
 * Texte und Töne der Stapelübersicht (ADR 0004 §3.8): Status und Detail je Eintrag, Schätzung
 * „3 Dokumente · 1 Anmeldung · 3 Bestätigungen (ggf. +1 für das Zertifikat)“, Speicherort und die
 * Zusammenfassung „2 von 4 signiert, 1 übersprungen, 1 abgelehnt“. Nur Darstellung der Kerndaten – Status,
 * Zahlen, Ausgabenamen und Ordner kommen aus `ViewState::batch`.
 *
 * Statusfarben wie im Prototyp: grau = wartet, blau = läuft, grün = fertig, rot = Fehler; „Wird signiert“
 * bewusst nicht im roten Markenakzent.
 */
import type { BatchEstimateView, BatchFlowView, BatchItemStatusView, BatchItemView, BatchOutputView, BatchSummaryView, SigningStage } from '../../core/types';
import type { Translator } from '../../i18n/catalog';
import type { Tone } from '../../ui/controls';
import { everydayText } from '../signing/texts';
import { confirmsEach } from './flow';

type State = BatchItemStatusView['state'];

const STATUS: Record<State, string> = {
  analyzing: 'ng-ui-batch-status-analyzing',
  ready: 'ng-ui-batch-status-ready',
  positioned: 'ng-ui-batch-status-positioned',
  rejected: 'ng-ui-batch-status-rejected',
  needs_review: 'ng-ui-batch-status-review',
  queued: 'ng-ui-batch-status-queued',
  active: 'ng-ui-batch-status-active',
  signed: 'ng-ui-batch-status-signed',
  failed: 'ng-ui-batch-status-failed',
  skipped: 'ng-ui-batch-status-skipped',
  cancelled: 'ng-ui-batch-status-cancelled',
};

const TONE: Record<State, Tone> = {
  analyzing: 'neutral',
  ready: 'neutral',
  positioned: 'neutral',
  rejected: 'danger',
  needs_review: 'warning',
  queued: 'neutral',
  active: 'info',
  signed: 'success',
  failed: 'danger',
  skipped: 'neutral',
  cancelled: 'neutral',
};

const STAGE: Record<SigningStage, string> = {
  connecting: 'ng-ui-batch-stage-connecting',
  login: 'ng-ui-batch-stage-login',
  certificate: 'ng-ui-batch-stage-certificate',
  preparing: 'ng-ui-batch-stage-preparing',
  confirming: 'ng-ui-batch-stage-confirming',
  finishing: 'ng-ui-batch-stage-finishing',
  saving: 'ng-ui-batch-stage-saving',
  paused: 'ng-ui-batch-stage-paused',
};

/** Ablehnungsgründe mit kurzem Detail (sonst der Text des Codes). */
const REJECTED: Record<string, string> = {
  'error-PDFPwdProtected': 'ng-ui-batch-detail-locked',
  'pdf-user-password': 'ng-ui-batch-detail-locked',
  'error-MayNotBeAPDF': 'ng-ui-batch-detail-notpdf',
};

export function statusLabel(t: Translator, status: BatchItemStatusView): string {
  return t.t(STATUS[status.state]);
}

export function statusTone(status: BatchItemStatusView): Tone {
  return TONE[status.state];
}

/** Phase eines laufenden Eintrags; ohne Bestätigung je Dokument (Keystore) „wird signiert“ statt „Bestätigung läuft“. */
export function stageLabel(t: Translator, stage: SigningStage, flow: BatchFlowView | null = null): string {
  if (stage === 'confirming' && !confirmsEach(flow)) return t.t('ng-ui-batch-stage-signing');
  return t.t(STAGE[stage]);
}

function baseName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

/** Platzierung eines Eintrags („automatisch platziert“, „im QR-Platzhalter“ …). */
export function positionDetail(t: Translator, item: BatchItemView): string {
  const one = singlePositionDetail(t, item);
  // Mehrfachsignatur: je von Hand platziertem Block eine Signatur
  const n = item.signatures ?? 1;
  return n > 1 ? `${one} · ${t.t('ng-ui-batch-detail-signatures', { n })}` : one;
}

function singlePositionDetail(t: Translator, item: BatchItemView): string {
  const p = item.position;
  switch (p.kind) {
    case 'qr':
      return t.t('ng-ui-batch-detail-qr');
    case 'field':
      return t.t('ng-ui-batch-detail-field', { name: p.name });
    case 'manual':
      return t.t('ng-ui-batch-detail-manual', { page: p.page + 1 });
    case 'invisible':
      return t.t('ng-ui-batch-detail-invisible');
    case 'auto':
      return t.t(item.status.state === 'positioned' ? 'ng-ui-batch-detail-checked' : 'ng-ui-batch-detail-auto');
  }
}

/** Zweite Zeile unter dem Status (`flow`: Ablauf des laufenden Stapels, Kern `SignerView::batch_flow`). */
export function statusDetail(t: Translator, item: BatchItemView, flow: BatchFlowView | null = null): string {
  const s = item.status;
  switch (s.state) {
    case 'analyzing':
    case 'queued':
      return '';
    case 'ready':
    case 'positioned':
      return item.target_exists && item.target_name ? `${positionDetail(t, item)} · ${t.t('ng-ui-batch-detail-exists', { file: item.target_name })}` : positionDetail(t, item);
    case 'rejected': {
      const id = REJECTED[s.code];
      return id ? t.t(id) : t.code(s.code);
    }
    case 'needs_review':
      return t.t('ng-ui-batch-detail-review');
    case 'active':
      return stageLabel(t, s.stage, flow);
    case 'signed':
      return t.t('ng-ui-batch-detail-signed', { file: baseName(s.path) });
    case 'failed':
      // Alltagstext ohne technische Ursache (Nachtrag 19); die steht in der Pause bzw. im Log
      return everydayText(t, s.code, s.params);
    case 'skipped':
      return t.t(s.existing ? 'ng-ui-batch-detail-skipped-existing' : 'ng-ui-batch-detail-skipped');
    case 'cancelled':
      return t.t('ng-ui-batch-detail-cancelled');
  }
}

/** Signierbar (die Oberfläche bietet nur `Ready`/`Positioned` an; der Kern prüft erneut). */
export function isSignable(item: BatchItemView): boolean {
  return item.status.state === 'ready' || item.status.state === 'positioned';
}

/** Einzeln öffnen (Bereit, Positioniert, Analysewarnung). */
export function isOpenable(item: BatchItemView): boolean {
  return isSignable(item) || item.status.state === 'needs_review';
}

/**
 * Schätzung: „3 Dokumente · 1 Anmeldung · 3 Bestätigungen (ggf. +1 für das Zertifikat)“; Keystore:
 * „3 Dokumente · Keystore-Passwort einmal“. Eine Erwartung, keine Zusage (§1.2).
 */
export function estimateText(t: Translator, e: BatchEstimateView): string {
  const docs = t.t('ng-ui-batch-docs', { n: e.documents });
  if (e.backend === 'keystore') return e.logins > 0 ? `${docs} · ${t.t('ng-ui-batch-estimate-keystore')}` : docs;
  const parts = [docs, t.t('ng-ui-batch-estimate-logins', { n: e.logins }), t.t('ng-ui-batch-estimate-confirmations', { n: e.confirmations })];
  const text = parts.join(' · ');
  return e.certificate_confirmation_possible ? `${text} ${t.t('ng-ui-batch-estimate-cert')}` : text;
}

/** Speicherort in der Übersicht. */
export function outputText(t: Translator, o: BatchOutputView): string {
  switch (o.mode) {
    case 'app_documents':
      return t.t('ng-ui-batch-output-app');
    case 'ask':
      return t.t('ng-ui-batch-output-ask');
    case 'folder':
      return o.needs_folder ? t.t('ng-ui-batch-output-missing', { folder: o.folder_display ?? '' }) : t.t('ng-ui-batch-output-folder', { folder: o.folder_display ?? '' });
    case 'next_to_source':
      return o.folder_display ? t.t('ng-ui-batch-output-folder', { folder: o.folder_display }) : t.t('ng-ui-batch-output-next');
  }
}

/** „2 von 4 signiert, 1 übersprungen, 1 abgelehnt“. */
export function summaryCounts(t: Translator, s: BatchSummaryView): string {
  const parts = [t.t('ng-ui-batch-sum-signed', { signed: s.signed, total: s.total })];
  if (s.skipped > 0) parts.push(t.t('ng-ui-batch-sum-skipped', { n: s.skipped }));
  if (s.rejected > 0) parts.push(t.t('ng-ui-batch-sum-rejected', { n: s.rejected }));
  if (s.failed > 0) parts.push(t.t('ng-ui-batch-sum-failed', { n: s.failed }));
  if (s.cancelled > 0) parts.push(t.t('ng-ui-batch-sum-cancelled', { n: s.cancelled }));
  if (s.not_started > 0) parts.push(t.t('ng-ui-batch-sum-not-started', { n: s.not_started }));
  return parts.join(', ');
}

/** Speicherort der Zusammenfassung. */
export function summaryPlace(t: Translator, s: BatchSummaryView, o: BatchOutputView): string | null {
  if (s.signed === 0) return null;
  if (o.mode === 'app_documents') return t.t('ng-ui-batch-sum-app');
  if (s.folder_display) return t.t('ng-ui-batch-sum-folder', { folder: s.folder_display });
  if (o.mode === 'next_to_source') return t.t('ng-ui-batch-sum-next');
  return null;
}
