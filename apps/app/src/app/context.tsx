/**
 * App-Kontext: Bridge, aktueller `ViewState`, Übersetzer, Live-Region, Hinweise und die Dienste der Shell
 * (Dateiauswahl, Einstellungen bei einem Abschnitt öffnen, Overlays, „Signieren“-Befehl).
 * Die Oberfläche hält **keinen Fachzustand** – nur UI-Zustand (Eingaben, Ziehen, Theme, Overlays;
 * ADR 0004 §4.1).
 */
import { createContext, useContext } from 'react';
import type { Bridge } from '../core/bridge';
import type { OpenFilesMode, UiEvent, ViewState } from '../core/types';
import type { Translator } from '../i18n/catalog';
import type { Theme } from './theme';

/** Kurzer Hinweis der Oberfläche (Toast). */
export interface Toast {
  id: number;
  kind: 'info' | 'success' | 'error';
  text: string;
  /** zweite Zeile (optional) */
  detail?: string;
}

/**
 * Abschnitte der Einstellungen für `openSettings(section)` (ADR 0004 §6.1, in E0 festgelegt). Welcher
 * Abschnitt gezeigt wird, ist reiner UI-Zustand; der Kern öffnet nur das Overlay (`open_settings`).
 */
export type SettingsSectionId = 'signature' | 'document' | 'signers' | 'save' | 'appearance' | 'accessibility' | 'privacy' | 'advanced' | 'about';

/** Alle Abschnitte in der Reihenfolge der Einstellungen („Dokument“ seit E3 direkt nach „Signaturbild“). */
export const SETTINGS_SECTION_IDS: readonly SettingsSectionId[] = ['signature', 'document', 'signers', 'save', 'appearance', 'accessibility', 'privacy', 'advanced', 'about'];

/** Overlays der Shell, die die Oberfläche selbst öffnet (Palette und Kürzelübersicht kommen mit B4). */
export type ShellOverlay = 'palette' | 'shortcuts' | 'qr' | null;

/** Werte des Kontexts. */
export interface AppContextValue {
  bridge: Bridge;
  view: ViewState;
  /**
   * sendet ein Ereignis; Fehler erscheinen als Hinweis. Das Ergebnis meldet, ob der Kern das Ereignis
   * angenommen hat (Masken geben ihren Doppelklickschutz bei `false` wieder frei).
   */
  dispatch: (event: UiEvent) => Promise<boolean>;
  /** Übersetzer der UI-Sprache (`ViewState::language`) */
  t: Translator;
  /** Text für Screenreader (Live-Region, höflich); `''` leert sie (veraltete Ansage) */
  announce: (text: string) => void;
  /** Hinweis anzeigen */
  toast: (text: string, kind?: Toast['kind'], detail?: string) => void;
  /** Ziehen von Dateien über dem Fenster aktiv */
  dragActive: boolean;
  /** macOS (Kürzel mit ⌘, Zugriffstasten mit ⌃⌥) */
  isMac: boolean;
  /** Farbschema (UI-lokal je Gerät, `pida.theme`); Kopfzeile und Einstellungen › Darstellung teilen es */
  theme: Theme;
  setTheme: (theme: Theme) => void;
  /** Einstellungen öffnen, optional bei einem Abschnitt (z. B. `privacy` aus dem Kurzhinweis „Details“) */
  openSettings: (section?: SettingsSectionId) => void;
  /**
   * Offline-Hilfe öffnen (`HELP_AVAILABLE`), optional bei einem Artikel (z. B. `fehler:<code>` aus einer
   * Fehlerkarte); ohne Hilfe ein No-op
   */
  openHelp: (target?: string | null) => void;
  /** zuletzt angeforderter Abschnitt der Einstellungen (für `SettingsPanel`) */
  settingsSection: SettingsSectionId;
  /**
   * Dateiauswahl des Systems (Desktop: Dialog, Mobile: Dokument-Picker) → `open_files`. Mit `mode` aus „+ PDF“
   * (ADR 0004 Nachtrag 9): `add` = „Weitere PDFs hinzufügen“ (Mehrfachauswahl), `replace` = „Anderes PDF öffnen“
   * (eine Datei); was daraus folgt, entscheidet der Kern.
   */
  browse: (mode?: OpenFilesMode) => Promise<void>;
  /**
   * „Erneut signieren“ aus „Zuletzt signiert“ (ADR 0004 Nachtrag 15): öffnet die Datei des Eintrags wie „Öffnen“
   * (`sign_recent`); bei geparkter Arbeit fragt die Shell vorher wie bei neuen PDFs (`ParkedJoinDialog`).
   */
  signRecent: (id: number) => void;
  /**
   * „Demo starten“ (⌘/Strg+D, im Web ⌥⇧D; Palette, Hilfe, Startseite auf Mobilgeräten): öffnet eine frische Kopie des
   * Demodokuments mit QR-Platzhalter wie eine gewählte Datei.
   */
  startDemo: () => Promise<void>;
  /** offenes Shell-Overlay (Palette, Kürzel) */
  overlay: ShellOverlay;
  setOverlay: (overlay: ShellOverlay) => void;
  /**
   * „Signieren“ (Knopf, ⌘↵, Palette): mit gewähltem Signiermittel `sign_with`, sonst öffnet der
   * Arbeitsbereich das Menü des Konto-Chips (ADR 0004 §3.4, §8 F12). Der Arbeitsbereich meldet dafür
   * seinen Handler an (`registerSign`); ohne Handler geschieht nichts.
   */
  requestSign: () => void;
  registerSign: (handler: (() => void) | null) => void;
}

export const AppContext = createContext<AppContextValue | null>(null);

/** Zugriff auf den App-Kontext. */
export function useApp(): AppContextValue {
  const ctx = useContext(AppContext);
  if (!ctx) throw new Error('AppContext fehlt');
  return ctx;
}

/** Übersetzer. */
export function useT(): Translator {
  return useApp().t;
}
