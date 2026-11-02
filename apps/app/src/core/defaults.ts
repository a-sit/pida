/**
 * Standardwerte wie in Rust (`pida_config::Settings::default()`, docs/analyse/01 §3.5) und kleine
 * Hilfen zur unveränderlichen Bearbeitung der Einstellungen.
 */
import type {
  AccessibilitySettings,
  ActionsView,
  DocumentDetectionView,
  ErrorContextView,
  MainBarStep,
  MainBarView,
  Settings,
  SignersView,
  SigningProgressView,
  Step,
  ViewState,
} from './types';

/** Entspricht `Settings::default()` (crates/config/src/settings.rs). */
export function defaultSettings(): Settings {
  return {
    schema_version: 1,
    general: {
      ui_language: null,
      update_check: true,
      window_size: null,
      last_source_directory: null,
      recent_documents: true,
      first_run_completed: true,
    },
    signature: {
      profile: 'SIGNATURBLOCK_SMALL',
      block_language: null,
      note: { mode: 'default' },
      pdfa_force: false,
      auto_position: false,
      emblem: null,
      logo_only_size_mm: 23,
      cms_profile: 'pdfas_compat',
    },
    placeholder: { search: true, kind: 'marker' },
    backend: { default: null },
    id_austria: { username: null, remember: false, fido2_default: false },
    keystore: { enabled: false, file: null, declared_type: null, alias: null, password_storage: 'memory', storage_chosen: true },
    output: { folder: null, postfix: '_signed' },
    proxy: { mode: 'system', host: null, port: null, user: null },
    certificate_cache: { enabled: false },
    accessibility: defaultAccessibility(),
  };
}

/** Entspricht `AccessibilitySettings::default()` (Einstellungen › Barrierefreiheit). */
export function defaultAccessibility(): AccessibilitySettings {
  return { mode: false, start: 'preview', announce: 'short', announce_after_move: false, keyboard_step: 'standard', check_before_sign: false };
}

/** Entspricht `SigningProgressView::default()` (ADR 0004 §3.6; bis K1 liefert der Kern nur diesen Wert). */
export function defaultSigningProgress(): SigningProgressView {
  return {
    backend: null,
    stage: 'connecting',
    method: null,
    doc: null,
    index: 0,
    total: 0,
    login_done: false,
    confirmations_done: 0,
    confirmations_total: 0,
    certificate: 'read',
    batch: false,
    pause: null,
  };
}

/** Entspricht `ErrorContextView::default()` (ADR 0004 §3.6). */
export function defaultErrorContext(): ErrorContextView {
  return { origin: 'other', occurred_at: null, backend: null, doc: null };
}

/**
 * Erkennung eines Dokuments, wenn der Kern sie nicht liefert (`DocumentView::detection` ist ein Nachtrag aus
 * K3 und in TypeScript optional): keine PDF/A-Kennung, nichts erzwungen, keine Lage aus der Automatik.
 */
export function defaultDocumentDetection(): DocumentDetectionView {
  return { pdfa: null, pdfa_decision: 'none', auto_placed: false };
}

/** Entspricht `SignersView::default()` (ADR 0004 §3.4): ohne Optionen keine Signiermittel-Auswahl (Nachtrag 20). */
export function defaultSigners(): SignersView {
  return { options: [], selected: null, default: null, choice_offered: false };
}

/** Entspricht `ActionsView::default()`: nichts freigegeben (ADR 0004 §3.13). */
export function noActions(): ActionsView {
  return {
    open_files: false,
    sign: false,
    edit_placement: false,
    start_batch: false,
    show_batch: false,
    settings: false,
    home: false,
  };
}

/** Felder der neuen Oberfläche (ADR 0004) mit den Standardwerten des Kerns bis zur Umsetzung in K1–K4. */
export function workspaceDefaults(): Pick<ViewState, 'document' | 'signers' | 'batch' | 'recent' | 'first_run' | 'actions'> {
  return { document: null, signers: defaultSigners(), batch: null, recent: null, first_run: null, actions: noActions() };
}

/** Bekannte Abschnitte der Einstellungen (alles andere ist `Settings::unknown`). */
export const SETTINGS_SECTIONS = [
  'general',
  'signature',
  'placeholder',
  'backend',
  'id_austria',
  'keystore',
  'output',
  'proxy',
  'certificate_cache',
  'accessibility',
] as const;

/** Abschnittsname. */
export type SettingsSection = (typeof SETTINGS_SECTIONS)[number];

/**
 * Ändert einen Abschnitt; unbekannte Einträge (`Settings::unknown`, flatten) bleiben erhalten
 * (docs/integration/wave1-offene-punkte.md: JSON-Rundlauf über die Bridge).
 */
export function withSection<K extends SettingsSection>(
  settings: Settings,
  section: K,
  patch: Partial<Settings[K]>,
): Settings {
  return { ...settings, [section]: { ...settings[section], ...patch } };
}

/** Reihenfolge der Hauptleiste. */
export const MAIN_BAR_ORDER: Step[] = ['Open', 'Positioning', 'Signing', 'Finish'];

/** Hauptleiste aus „A“/„E“-Kürzeln (Tabelle docs/analyse/01 §3.5), z. B. `bar(['AE','A','',''], true)`. */
export function mainBar(cells: [string, string, string, string], settingsEnabled: boolean): MainBarView {
  const steps: MainBarStep[] = MAIN_BAR_ORDER.map((step, i) => {
    const cell = cells[i] ?? '';
    return { step, active: cell.includes('A'), enabled: cell.includes('E') };
  });
  return { steps, settings_enabled: settingsEnabled };
}

/** pt → mm. */
export function ptToMm(pt: number): number {
  return (pt * 25.4) / 72;
}

/** mm → pt. */
export function mmToPt(mm: number): number {
  return (mm * 72) / 25.4;
}

/** Dateiname eines Pfads (beide Trenner). */
export function baseName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

/** Ordner eines Pfads (beide Trenner). */
export function dirName(path: string): string {
  const i = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  return i > 0 ? path.slice(0, i) : '';
}
