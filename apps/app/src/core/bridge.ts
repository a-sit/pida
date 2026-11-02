/**
 * Bridge zwischen Oberfläche und Kern (Plan §3.4 `CoreBridge`) plus die nativen Dienste der Shell.
 *
 * Die Oberfläche hält **keinen Fachzustand**: Sie projiziert den `ViewState` und sendet `UiEvent`s.
 * Zwei Implementierungen: `TauriBridge` (hier; Commands `core_view`/`core_dispatch`, Ereignis
 * `pida:view`, Custom Protocol `pida-render://`) und der TS-Mock-Kern (`mock.ts`, für
 * `VITE_MOCK=1`, Browser-Vorschau und Tests).
 */
import type { DocId, QrPlaceholderFormat, QrPlaceholderSpec, UiEvent, ViewState } from './types';

/** Name des Ereignisses mit dem aktuellen ViewState (`pida_app_lib::VIEW_EVENT`). */
export const VIEW_EVENT = 'pida:view';
/** Schema des Custom Protocols (`pida_app_lib::protocol::SCHEME`). */
export const RENDER_SCHEME = 'pida-render';

/** Dateifilter eines Dialogs. */
/** Lizenztexte der App (`license_text`). */
export type LicenseKind = 'eupl' | 'notices' | 'rust_std';

export interface FileFilter {
  name: string;
  extensions: string[];
}

/** Ziehen von Dateien über das Fenster (Tauri `onDragDropEvent`; Position in CSS-Pixeln). */
export type FileDragEvent =
  | { type: 'enter'; paths: string[]; x: number; y: number }
  | { type: 'over'; x: number; y: number }
  | { type: 'drop'; paths: string[]; x: number; y: number }
  | { type: 'leave' };

/** Angaben für den Über-Dialog. */
export interface AppInfo {
  version: string;
  platform: string;
  core_available: boolean;
}

/** Kern-Schnittstelle (Plan §3.4). */
export interface CoreBridge {
  /** `tauri` (Rust-Kern), `web` (Rust-Kern als WebAssembly im Browser) oder `mock` (TS-Mock-Kern). */
  readonly kind: 'tauri' | 'web' | 'mock';
  /** Aktueller Zustand (beim Start). */
  view(): Promise<ViewState>;
  /** Sendet ein Ereignis; Fehler tragen einen stabilen Code (i18n-ID). */
  dispatch(event: UiEvent): Promise<void>;
  /** Abo auf Zustandsänderungen; liefert die Abmeldung. */
  onState(cb: (view: ViewState) => void): () => void;
  /** URL einer Seitenkachel (`scale` relativ zu 72 dpi). */
  pageTileUrl(doc: DocId, page: number, scale: number): string;
  /** URL der Blockvorschau (Revision = Cache-Busting). */
  blockPreviewUrl(scale: number, revision: number): string;
  /**
   * URL der Miniatur eines Stapeleintrags (`pida-render://thumb/<doc>`, Voranalyse, Breite 96 px; ADR 0004
   * §3.8/§3.14). Nur bei `BatchItemView::thumbnail`.
   */
  thumbUrl(doc: DocId): string;
  /** URL eines QR-Platzhalters als PNG bzw. SVG („QR-Platzhalter erstellen“, `pida-render://qr/…`). */
  qrPlaceholderUrl(spec: QrPlaceholderSpec, format: QrPlaceholderFormat): string;
}

/** Ergebnis der Mobile-Dateiauswahl (Command `mobile_pick_files`): Kopien in der Sandbox und nicht übernommene. */
export interface MobilePick {
  /** Kopien in der App-Sandbox, Reihenfolge der Auswahl (`[]` = abgebrochen) */
  paths: string[];
  /** gewählt, aber nicht kopiert (nicht lesbar, zu groß) */
  skipped: number;
}

/** Native Dienste (Dialoge, Drag & Drop, Öffnen im System). */
export interface PlatformServices {
  /** Dateidialog mit Mehrfachauswahl (F-11); `null` = abgebrochen. */
  pickFiles(options: { title: string; filters: FileFilter[]; defaultPath?: string | null; multiple: boolean }): Promise<string[] | null>;
  /** Speichern-Dialog (F-83); `null` = abgebrochen. */
  pickSavePath(options: { title: string; filters: FileFilter[]; defaultPath?: string | null }): Promise<string | null>;
  /** Ordnerdialog; `null` = abgebrochen. */
  pickDirectory(options: { title: string; defaultPath?: string | null }): Promise<string | null>;
  /**
   * Mobile: System-Dateiauswahl über das Plugin `pida-mobile` (Dokument-Picker bzw. SAF); die Dateien
   * liegen danach als Kopien in der App-Sandbox. `[]` = abgebrochen. Fehler tragen einen Code
   * (`ng-mobile-not-implemented`, `ng-mobile-unsupported` …). Nur PDFs: einen PKCS#12-Import gibt es seit ADR
   * 0004 Nachtrag 11 nicht mehr (auf Mobilgeräten nur ID Austria).
   */
  pickMobileFiles(options: { kind: 'pdf'; multiple: boolean }): Promise<string[]>;
  /**
   * Mobile: PDF-Dokumente wählen wie `pickMobileFiles({ kind: 'pdf' })`, dazu die Zahl der gewählten Dateien, die
   * die native Seite nicht kopieren konnte (nicht lesbar, zu groß; ADR 0004 Nachtrag 17). Reihenfolge der Auswahl.
   */
  pickMobileDocuments(options: { multiple: boolean }): Promise<MobilePick>;
  /** Drag & Drop von Dateien über das Fenster. */
  onFileDrag(cb: (event: FileDragEvent) => void): () => void;
  /** Öffnet eine `https:`/`mailto:`-URL im Systembrowser. */
  openUrl(url: string): Promise<void>;
  /**
   * Zeigt die Ergebnisdatei im Dateimanager: ohne `doc` die des Ergebnis-Schritts, mit `doc` die Ausgabe
   * dieses Stapeleintrags (nur Pfade, die der Kern geschrieben hat; ADR 0004 §3.14).
   */
  revealOutput(doc?: DocId | null): Promise<void>;
  /** Öffnet den Log-Ordner. */
  openLogDir(): Promise<void>;
  /**
   * Lizenztext für die Anzeige in der App (Backlog Nr. 2): `eupl` (EUPL-1.2 von PIDA), `notices`
   * (THIRD-PARTY-NOTICES, Markdown) bzw. `rust_std` (Hinweise der Rust-Standardbibliothek, HTML) – eingebettet in das
   * Programm, auch auf Android. Fehlen die Hinweise (Entwicklungsbuild): Fehler `ng-ui-licenses-missing`.
   */
  licenseText(kind: LicenseKind): Promise<string>;
  /**
   * Demodokument („Demo starten“, ⌘/Strg+D, im Web ⌥⇧D): eine Seite mit QR-Platzhalter Nr. 1 (`public/PIDA-Demo.pdf`,
   * `tools/make-demo-pdf.py`). Liefert den Pfad einer frischen, beschreibbaren Kopie, die die Oberfläche wie eine
   * gewählte Datei öffnet. Fehler: `ng-ui-open-failed`.
   */
  demoDocument(): Promise<string>;
  /** Version, Plattform, Kern verfügbar. */
  appInfo(): Promise<AppInfo>;
}

/** Bridge + Dienste. */
export type Bridge = CoreBridge & PlatformServices;

/** Läuft die Oberfläche in Tauri? */
export function inTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

/** Nur `https:` und `mailto:` gehen an den Systembrowser (Plan §7.7). */
export function isAllowedExternalUrl(url: string): boolean {
  try {
    const u = new URL(url);
    return u.protocol === 'https:' || u.protocol === 'mailto:';
  } catch {
    return false;
  }
}

/** Maßstab für Kacheln: auf 0,25 gerundet (weniger Neuberechnungen), begrenzt wie im Protokoll. */
export function tileScale(scale: number): number {
  const rounded = Math.round(scale * 4) / 4;
  return Math.min(8, Math.max(0.25, rounded));
}

/** Basis-URL des Custom Protocols je Plattform aus dem User-Agent (Rückfall, wie `convertFileSrc`). */
export function renderBase(userAgent: string): string {
  const windowsLike = /Windows|Android/i.test(userAgent);
  return windowsLike ? `http://${RENDER_SCHEME}.localhost/` : `${RENDER_SCHEME}://localhost/`;
}

/**
 * Basis-URL des Custom Protocols: maßgeblich ist `convertFileSrc` der Tauri-Laufzeit (kennt Plattform
 * und `useHttpsScheme`); nur wenn sie fehlt oder Unerwartetes liefert, der User-Agent.
 */
export function resolveRenderBase(convert: ((path: string, protocol: string) => string) | null, userAgent: string): string {
  try {
    const base = convert?.('', RENDER_SCHEME);
    if (typeof base === 'string' && base.endsWith('/') && base.includes(RENDER_SCHEME)) return base;
  } catch {
    /* Rückfall */
  }
  return renderBase(userAgent);
}

/** URL einer Seitenkachel relativ zur Basis. */
export function pageTilePath(doc: DocId, page: number, scale: number): string {
  return `page/${encodeURIComponent(String(doc))}/${encodeURIComponent(String(page))}?scale=${tileScale(scale)}`;
}

/** URL der Miniatur eines Stapeleintrags relativ zur Basis. */
export function thumbPath(doc: DocId): string {
  return `thumb/${encodeURIComponent(String(doc))}`;
}

/** URL eines QR-Platzhalters relativ zur Basis (Nummer nur Ziffern, sonst leer – der Kern prüft ebenfalls). */
export function qrPlaceholderPath(spec: QrPlaceholderSpec, format: QrPlaceholderFormat): string {
  return `qr/${encodeURIComponent(spec.id)}.${format}?profile=${spec.profile}&lang=${spec.language}`;
}

/** URL der Blockvorschau relativ zur Basis. */
export function blockPreviewPath(scale: number, revision: number): string {
  return `block?scale=${tileScale(scale)}&rev=${encodeURIComponent(String(revision))}`;
}

/** Fehlercode aus einer abgelehnten Tauri-Promise (Commands liefern stabile Codes als String). */
export function errorCode(error: unknown): string {
  if (typeof error === 'string' && /^[a-z0-9][a-z0-9_.-]*$/i.test(error)) return error;
  return 'ng-ui-bridge-error';
}

/** Bridge zum Rust-Kern über Tauri. */
export async function createTauriBridge(): Promise<Bridge> {
  const [{ invoke, convertFileSrc }, { listen }, { getCurrentWebview }, dialog, opener] = await Promise.all([
    import('@tauri-apps/api/core'),
    import('@tauri-apps/api/event'),
    import('@tauri-apps/api/webview'),
    import('@tauri-apps/plugin-dialog'),
    import('@tauri-apps/plugin-opener'),
  ]);
  const base = resolveRenderBase(convertFileSrc, navigator.userAgent);
  const listeners = new Set<(view: ViewState) => void>();
  await listen<ViewState>(VIEW_EVENT, (event) => {
    for (const cb of listeners) cb(event.payload);
  });

  const asPaths = (value: string | string[] | null): string[] | null => {
    if (value === null) return null;
    return Array.isArray(value) ? value : [value];
  };
  const pickMobile = async (kind: 'pdf' | 'pkcs12', multiple: boolean): Promise<MobilePick> => {
    try {
      const r = await invoke<MobilePick>('mobile_pick_files', { kind, multiple });
      return { paths: r.paths, skipped: r.skipped };
    } catch (e) {
      throw new Error(errorCode(e));
    }
  };

  return {
    kind: 'tauri',
    view: () => invoke<ViewState>('core_view'),
    async dispatch(event) {
      try {
        await invoke('core_dispatch', { event });
      } catch (e) {
        throw new Error(errorCode(e));
      }
    },
    onState(cb) {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
    pageTileUrl: (doc, page, scale) => base + pageTilePath(doc, page, scale),
    blockPreviewUrl: (scale, revision) => base + blockPreviewPath(scale, revision),
    thumbUrl: (doc) => base + thumbPath(doc),
    qrPlaceholderUrl: (spec, format) => base + qrPlaceholderPath(spec, format),

    async pickFiles({ title, filters, defaultPath, multiple }) {
      const result = await dialog.open({ title, filters, multiple, directory: false, defaultPath: defaultPath ?? undefined });
      return asPaths(result);
    },
    async pickSavePath({ title, filters, defaultPath }) {
      return dialog.save({ title, filters, defaultPath: defaultPath ?? undefined });
    },
    async pickDirectory({ title, defaultPath }) {
      const result = await dialog.open({ title, directory: true, multiple: false, defaultPath: defaultPath ?? undefined });
      return typeof result === 'string' ? result : null;
    },
    async pickMobileFiles({ kind, multiple }) {
      return (await pickMobile(kind, multiple)).paths;
    },
    async pickMobileDocuments({ multiple }) {
      return pickMobile('pdf', multiple);
    },
    onFileDrag(cb) {
      let unlisten: (() => void) | null = null;
      let cancelled = false;
      const ratio = () => window.devicePixelRatio || 1;
      void getCurrentWebview()
        .onDragDropEvent((event) => {
          const p = event.payload;
          switch (p.type) {
            case 'enter':
              cb({ type: 'enter', paths: p.paths, x: p.position.x / ratio(), y: p.position.y / ratio() });
              break;
            case 'over':
              cb({ type: 'over', x: p.position.x / ratio(), y: p.position.y / ratio() });
              break;
            case 'drop':
              cb({ type: 'drop', paths: p.paths, x: p.position.x / ratio(), y: p.position.y / ratio() });
              break;
            default:
              cb({ type: 'leave' });
          }
        })
        .then((fn) => {
          if (cancelled) fn();
          else unlisten = fn;
        });
      return () => {
        cancelled = true;
        unlisten?.();
      };
    },
    async openUrl(url) {
      if (!isAllowedExternalUrl(url)) throw new Error('ng-ui-open-failed');
      await opener.openUrl(url);
    },
    async revealOutput(doc) {
      try {
        await invoke('reveal_output', doc === undefined || doc === null ? {} : { doc });
      } catch (e) {
        throw new Error(errorCode(e));
      }
    },
    async openLogDir() {
      try {
        await invoke('open_log_dir');
      } catch (e) {
        throw new Error(errorCode(e));
      }
    },
    async licenseText(kind) {
      try {
        return await invoke<string>('license_text', { kind });
      } catch (e) {
        throw new Error(errorCode(e));
      }
    },
    async demoDocument() {
      try {
        return await invoke<string>('demo_document');
      } catch (e) {
        throw new Error(errorCode(e));
      }
    },
    appInfo: () => invoke<AppInfo>('app_info'),
  };
}
