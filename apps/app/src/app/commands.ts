/**
 * Befehlsliste der Shell (ADR 0004 §2.10, §6.1): alle Befehle des Prototyps mit ihren Freigaben aus
 * `ViewState::actions` (§3.13). Beschriftungen und Suchbegriffe (`.keywords`) stehen in
 * `ui-workspace.ftl` (Bereich `shell`). Die Befehlspalette und die Kürzelübersicht (UI-B, B4) zeigen genau
 * diese Liste; die Tastenkürzel der Shell (`App.tsx`) lösen dieselben Befehle aus.
 *
 * Befehle senden nur `UiEvent`s bzw. öffnen UI-Overlays – „Signieren“ ist eine ausdrückliche Nutzeraktion
 * (A1): mit gewähltem Signiermittel `sign_with`, sonst öffnet der Arbeitsbereich das Menü des Konto-Chips.
 */
import type { ComponentType } from 'react';
import type { Translator } from '../i18n/catalog';
import type { OpenFilesMode, UiEvent, ViewState } from '../core/types';
import { webProfile } from '../core/web-profile';
import { IconFilePlus, IconFolder, IconGlobe, IconHome, IconKeyboard, IconLayers, IconMonitor, IconMoon, IconPlay, IconQr, IconSettings, IconSignature, IconSun } from '../ui/Icons';
import type { SettingsSectionId, ShellOverlay } from './context';
import type { Theme } from './theme';

export type CommandId =
  | 'open'
  | 'add'
  | 'batch'
  | 'sign'
  | 'settings'
  | 'signature'
  | 'qr'
  | 'demo'
  | 'home'
  | 'theme-light'
  | 'theme-dark'
  | 'theme-system'
  | 'lang-de'
  | 'lang-en'
  | 'shortcuts';

export type CommandGroup = 'actions' | 'view' | 'help';

/** Tastenkürzel eines Befehls: `mod` = ⌘ (macOS) bzw. Strg; `alt`/`shift` = zusätzlich ⌥/Alt bzw. ⇧/Umschalt. */
export interface KeyCombo {
  mod: boolean;
  alt?: boolean;
  shift?: boolean;
  key: 'O' | 'B' | 'D' | 'Enter' | ',' | 'K' | '?';
}

/**
 * „Demo starten“: ⌘/Strg+D in den Apps; im Browser ist das „Lesezeichen setzen“, dort deshalb ⌥⇧D bzw. Alt+Umschalt+D
 * (erkannt über die physische Taste, weil ⌥⇧D unter macOS ein Sonderzeichen tippt).
 */
export function demoKeys(): KeyCombo {
  return webProfile() !== null ? { mod: false, alt: true, shift: true, key: 'D' } : { mod: true, key: 'D' };
}

/** Statische Beschreibung eines Befehls. */
export interface CommandSpec {
  id: CommandId;
  group: CommandGroup;
  /** Meldungs-ID der Beschriftung (`ui-workspace.ftl`; Attribut `.keywords` = Suchbegriffe) */
  label: string;
  keys?: KeyCombo;
  icon: ComponentType<{ size?: number }>;
}

/** Alle Befehle in der Reihenfolge der Palette. */
export const COMMANDS: readonly CommandSpec[] = [
  { id: 'open', group: 'actions', label: 'ng-ui-shell-cmd-open', keys: { mod: true, key: 'O' }, icon: IconFolder },
  // „PDF hinzufügen“ und Stapel (ADR 0004 Nachtrag 9)
  { id: 'add', group: 'actions', label: 'ng-ui-shell-cmd-add', icon: IconFilePlus },
  { id: 'batch', group: 'actions', label: 'ng-ui-shell-cmd-batch', keys: { mod: true, key: 'B' }, icon: IconLayers },
  { id: 'sign', group: 'actions', label: 'ng-ui-shell-cmd-sign', keys: { mod: true, key: 'Enter' }, icon: IconSignature },
  { id: 'settings', group: 'actions', label: 'ng-ui-shell-cmd-settings', keys: { mod: true, key: ',' }, icon: IconSettings },
  { id: 'signature', group: 'actions', label: 'ng-ui-shell-cmd-signature', icon: IconSignature },
  // Nachtrag 24: „QR-Platzhalter erstellen“
  { id: 'qr', group: 'actions', label: 'ng-ui-shell-cmd-qr', icon: IconQr },
  // Demodokument mit QR-Platzhalter (`public/PIDA-Demo.pdf`); Kürzel je nach Variante (`demoKeys`)
  { id: 'demo', group: 'actions', label: 'ng-ui-shell-cmd-demo', icon: IconPlay },
  { id: 'home', group: 'actions', label: 'ng-ui-shell-cmd-home', icon: IconHome },
  { id: 'theme-light', group: 'view', label: 'ng-ui-shell-cmd-theme-light', icon: IconSun },
  { id: 'theme-dark', group: 'view', label: 'ng-ui-shell-cmd-theme-dark', icon: IconMoon },
  { id: 'theme-system', group: 'view', label: 'ng-ui-shell-cmd-theme-system', icon: IconMonitor },
  { id: 'lang-de', group: 'view', label: 'ng-ui-shell-cmd-lang-de', icon: IconGlobe },
  { id: 'lang-en', group: 'view', label: 'ng-ui-shell-cmd-lang-en', icon: IconGlobe },
  { id: 'shortcuts', group: 'help', label: 'ng-ui-shell-cmd-shortcuts', keys: { mod: false, key: '?' }, icon: IconKeyboard },
];

/** Freigabe eines Befehls aus `ViewState::actions` (die Oberfläche verknüpft nur mit offenen Overlays). */
export function isCommandEnabled(id: CommandId, view: ViewState): boolean {
  const a = view.actions;
  switch (id) {
    case 'open':
    case 'demo':
      return a.open_files;
    case 'add':
      return a.open_files && addAvailable(view);
    case 'batch':
      return batchNav(view) !== null;
    case 'sign':
      // im Stapel: „Alle signieren“ (ausdrückliche Aktion, A1)
      return a.sign || a.start_batch;
    case 'settings':
    case 'signature':
      return a.settings && view.settings === null;
    case 'home':
      return a.home;
    default:
      return true;
  }
}

/**
 * „PDF hinzufügen“ (Palette, ADR 0004 Nachtrag 9): zu einem Dokument im Arbeitsbereich (wie „Weitere PDFs
 * hinzufügen“ in der Kopfzeile) bzw. in die Stapelübersicht; nicht im Lauf.
 */
function addAvailable(view: ViewState): boolean {
  const doc = view.document;
  if (doc) return doc.phase === 'editing' || doc.phase === 'warning';
  return view.step.step === 'batch' && !!view.batch && !view.batch.running;
}

/**
 * „Zum Stapel“ bzw. „Zurück zum Stapel“ (Kopfzeile, Palette, ⌘/Strg+B): `back` für ein aus der Übersicht geöffnetes
 * Dokument (`batch.open_item`, Arbeitsbereich bzw. Analysewarnung), `show` mit der Freigabe `actions.show_batch`
 * (weitere Dokumente warten), sonst `null`. Was dabei mit der Platzierung geschieht, entscheidet der Kern.
 */
export function batchNav(view: ViewState): 'back' | 'show' | null {
  const doc = view.document;
  const batch = view.batch;
  if (doc && batch && batch.open_item === doc.doc.id && (doc.phase === 'editing' || doc.phase === 'warning')) return 'back';
  return view.actions.show_batch ? 'show' : null;
}

/**
 * „Zur Startseite“ (Wortmarke, Haus-Symbol, Palette): `go_home`, sofern freigegeben. Der Kern parkt dabei das offene
 * Dokument bzw. den Stapel (ADR 0004 Nachtrag 10); die Startansicht zeigt es unter „In Bearbeitung“. „Fertig“ →
 * „Nächstes Dokument“ und „Neuer Stapel“ senden weiter `navigate{Open}`.
 */
export async function goHome(view: ViewState, dispatch: (event: UiEvent) => Promise<boolean>): Promise<void> {
  if (view.actions.home) await dispatch({ type: 'go_home' });
}

/** Was ein Befehl auslöst (von der Shell bereitgestellt). */
export interface CommandDeps {
  view: ViewState;
  t: Translator;
  isMac: boolean;
  dispatch: (event: UiEvent) => Promise<boolean>;
  browse: (mode?: OpenFilesMode) => Promise<void>;
  startDemo: () => Promise<void>;
  requestSign: () => void;
  openSettings: (section?: SettingsSectionId) => void;
  setTheme: (theme: Theme) => void;
  setOverlay: (overlay: ShellOverlay) => void;
}

/** Aufbereiteter Befehl für Palette und Kürzelübersicht. */
export interface CommandEntry {
  id: CommandId;
  group: CommandGroup;
  label: string;
  /** Suchbegriffe (Synonyme) */
  keywords: string;
  /** Tastenkappen zur Anzeige, z. B. `['⌘', 'O']` */
  keys: string[];
  icon: ComponentType<{ size?: number }>;
  isDisabled: boolean;
  run: () => void;
}

/** Tastenkappen eines Kürzels. */
export function keyCaps(keys: KeyCombo, isMac: boolean, t: Translator): string[] {
  const de = t.lang === 'de';
  const mod = isMac ? '⌘' : de ? 'Strg' : 'Ctrl';
  const key = keys.key === 'Enter' ? (isMac ? '↵' : de ? 'Eingabe' : 'Enter') : keys.key;
  return [
    ...(keys.mod ? [mod] : []),
    ...(keys.alt ? [isMac ? '⌥' : 'Alt'] : []),
    ...(keys.shift ? [isMac ? '⇧' : de ? 'Umschalt' : 'Shift'] : []),
    key,
  ];
}

/** Befehle mit Freigaben und Aktionen. */
export function buildCommands(deps: CommandDeps): CommandEntry[] {
  const { view, t, isMac } = deps;
  const run = (id: CommandId): (() => void) => {
    switch (id) {
      case 'open':
        return () => void deps.browse();
      case 'add':
        return () => void deps.browse('add');
      case 'batch':
        return () => {
          const nav = batchNav(deps.view);
          if (nav) void deps.dispatch({ type: nav === 'back' ? 'back_to_batch' : 'show_batch' });
        };
      case 'sign':
        return deps.requestSign;
      case 'settings':
        return () => deps.openSettings();
      case 'signature':
        return () => deps.openSettings('signature');
      case 'qr':
        return () => deps.setOverlay('qr');
      case 'demo':
        return () => void deps.startDemo();
      case 'home':
        return () => void goHome(deps.view, deps.dispatch);
      case 'theme-light':
        return () => deps.setTheme('light');
      case 'theme-dark':
        return () => deps.setTheme('dark');
      case 'theme-system':
        return () => deps.setTheme('system');
      case 'lang-de':
        return () => void deps.dispatch({ type: 'set_ui_language', language: 'de' });
      case 'lang-en':
        return () => void deps.dispatch({ type: 'set_ui_language', language: 'en' });
      case 'shortcuts':
        return () => deps.setOverlay('shortcuts');
    }
  };
  // aus der Übersicht geöffnet: „Zurück zum Stapel“ statt „Zum Stapel“ (gleiches Kürzel)
  const label = (c: CommandSpec) => (c.id === 'batch' && batchNav(view) === 'back' ? 'ng-ui-shell-cmd-batch-back' : c.label);
  return COMMANDS.map((c) => ({
    id: c.id,
    group: c.group,
    label: t.t(label(c)),
    keywords: t.attr(label(c), 'keywords') ?? '',
    keys: c.id === 'demo' ? keyCaps(demoKeys(), isMac, t) : c.keys ? keyCaps(c.keys, isMac, t) : [],
    icon: c.icon,
    isDisabled: !isCommandEnabled(c.id, view),
    run: run(c.id),
  }));
}

/** Kürzel des Arbeitsbereichs für die Kürzelübersicht (Schrittweiten des Kerns: 15/1/50 pt, §2.3). */
export interface ShortcutEntry {
  id: string;
  group: 'general' | 'workspace';
  label: string;
  keys: string[];
}

export function buildShortcuts(t: Translator, isMac: boolean): ShortcutEntry[] {
  const de = t.lang === 'de';
  const mod = isMac ? '⌘' : de ? 'Strg' : 'Ctrl';
  const shift = isMac ? '⇧' : de ? 'Umschalt' : 'Shift';
  const alt = isMac ? '⌥' : 'Alt';
  const general: ShortcutEntry[] = COMMANDS.filter((c) => c.keys || c.id === 'demo').map((c) => ({
    id: c.id,
    group: 'general',
    label: t.t(c.label),
    keys: keyCaps(c.id === 'demo' ? demoKeys() : (c.keys as KeyCombo), isMac, t),
  }));
  general.push({ id: 'close', group: 'general', label: t.t('ng-ui-shell-key-close'), keys: ['Esc'] });
  general.push({ id: 'accesskeys', group: 'general', label: t.t('ng-ui-shell-key-accesskeys'), keys: isMac ? ['⌃', '⌥', 'A–Z'] : ['Alt', 'A–Z'] });
  const workspace: ShortcutEntry[] = [
    { id: 'move', group: 'workspace', label: t.t('ng-ui-shell-key-move'), keys: ['←', '↑', '→', '↓'] },
    { id: 'move-fine', group: 'workspace', label: t.t('ng-ui-shell-key-move-fine'), keys: [shift, '←↑→↓'] },
    { id: 'move-coarse', group: 'workspace', label: t.t('ng-ui-shell-key-move-coarse'), keys: [alt, '←↑→↓'] },
    { id: 'page', group: 'workspace', label: t.t('ng-ui-shell-key-page'), keys: ['Bild ↑', 'Bild ↓'].map((k) => (de ? k : k.replace('Bild', 'Page'))) },
    { id: 'place', group: 'workspace', label: t.t('ng-ui-shell-key-place'), keys: [t.t('ng-ui-shell-key-dblclick')] },
    { id: 'zoom', group: 'workspace', label: t.t('ng-ui-shell-key-zoom'), keys: [mod, '+', '−'] },
    { id: 'fit', group: 'workspace', label: t.t('ng-ui-shell-key-fit'), keys: [mod, '0'] },
  ];
  return [...general, ...workspace];
}
