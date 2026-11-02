/**
 * Web-Brücke (Konzept Webdienst, Variante E, Etappe 1): dieselbe Oberfläche wie in der App, der Kern läuft als
 * WebAssembly im Browser (`apps/web`, Rust). PDFs (Bildmarke, im Testbuild Keystore) liegen im Dateisystem im Speicher
 * des Kerns (`put_file`), Ergebnisse kommen als Download heraus (ein Stapel als ZIP). Gespeichert wird nichts: jede
 * Sitzung beginnt mit denselben Einstellungen (`apps/web`, `SESSION_SETTINGS`), siehe auch `web-profile.ts`.
 *
 * Laden (nur bei `VITE_WEB=1`, `npm run build:web`): `pkg/pida_web.js` (wasm-bindgen) und PDFium als eigenes
 * WebAssembly-Modul (`pdfium/pdfium.js`, Emscripten) liegen neben der Seite.
 * - PDFium vor dem Kern laden und binden (`initialize_pdfium_render`); der Initialisierer heißt im Paket von
 *   bblanchon/pdfium-binaries `_FPDF_InitLibrary`, pdfium-render ruft `PDFium_Init` → Alias.
 * - Pause-Rückruf für das progressive Rendern in PDFiums eigener Funktionstabelle (`addFunction`).
 * - Bilder (`pageTileUrl` …): synchron gerendert und als Blob-URL zwischengespeichert (der Kern läuft im selben Thread).
 *
 * Signiermittel: ID Austria über das A-Trust-Overlay (`ida-overlay.ts`), als verstecktes Testmerkmal
 * (`?test-keystore`) zusätzlich der Keystore.
 */
import type { AppInfo, Bridge, FileDragEvent, LicenseKind } from './bridge';
import { blockPreviewPath, qrPlaceholderPath, tileScale } from './bridge';
import type { DocId, UiEvent, ViewState } from './types';
import { createSlTransport } from './ida-overlay';
import { setWebProfile } from './web-profile';
import { zipStored } from './zip';

/** Name des ZIP mit den Ergebnissen eines Stapels. */
const BATCH_ZIP = 'PIDA-signiert.zip';

interface PidaWebModule {
  default(input?: unknown): Promise<unknown>;
  WebCore: new (
    language: string,
    version: string,
    onView: (json: string) => void,
    shell: (kind: string, value: string) => void,
    sl: (kind: string, id: number, payload: string) => void,
    testKeystore: boolean,
  ) => {
    view(): string;
    dispatch(event: string): void;
    page_tile(doc: bigint, page: number, scale: number): Uint8Array<ArrayBuffer>;
    block_preview(scale: number): Uint8Array<ArrayBuffer>;
    qr_placeholder(spec: string, format: string): Uint8Array<ArrayBuffer>;
    thumbnail(doc: bigint): Uint8Array<ArrayBuffer>;
  };
  put_file(name: string, bytes: Uint8Array): string;
  read_file(path: string): Uint8Array<ArrayBuffer>;
  sl_resolve(id: number, ok: boolean, response: Uint8Array, error: string): void;
  license_eupl(): string;
  output_dir(): string;
  version(): string;
  initialize_pdfium_render(pdfium: unknown, rust: unknown, debug: boolean): boolean;
  set_pdfium_never_pause(index: number): void;
}

interface EmscriptenPdfium {
  _FPDF_InitLibrary?: unknown;
  _PDFium_Init?: unknown;
  addFunction(fn: (...args: number[]) => number, signature: string): number;
}

/** Leeres Bild, solange eine Kachel (noch) nicht gerendert werden kann. */
const EMPTY_PNG = 'data:image/gif;base64,R0lGODlhAQABAAAAACH5BAEKAAEALAAAAAABAAEAAAICTAEAOw==';

function loadPdfium(base: string): Promise<EmscriptenPdfium> {
  return new Promise((resolve, reject) => {
    const w = window as unknown as { Module?: unknown };
    w.Module = { onRuntimeInitialized: () => resolve(w.Module as EmscriptenPdfium), locateFile: (f: string) => `${base}pdfium/${f}` };
    const s = document.createElement('script');
    s.src = `${base}pdfium/pdfium.js`;
    s.onerror = () => reject(new Error('ng-ui-bridge-error'));
    document.head.append(s);
  });
}

function download(name: string, bytes: Uint8Array<ArrayBuffer>): void {
  const lower = name.toLowerCase();
  const type = lower.endsWith('.pdf') ? 'application/pdf' : lower.endsWith('.zip') ? 'application/zip' : 'application/octet-stream';
  const url = URL.createObjectURL(new Blob([bytes], { type }));
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 60_000);
}

function baseName(path: string): string {
  return path.split('/').filter(Boolean).pop() ?? 'dokument.pdf';
}

/** Bridge zum Kern als WebAssembly. */
export async function createWebBridge(): Promise<Bridge> {
  const base = new URL('./', document.baseURI).href;
  const pdfiumLoaded = loadPdfium(base);
  const pkg = (await import(/* @vite-ignore */ `${base}pkg/pida_web.js`)) as PidaWebModule;
  const rust = await pkg.default();
  const pdfium = await pdfiumLoaded;
  if (!pdfium._PDFium_Init && pdfium._FPDF_InitLibrary) pdfium._PDFium_Init = pdfium._FPDF_InitLibrary;
  pkg.initialize_pdfium_render(pdfium, rust, false);
  pkg.set_pdfium_never_pause(pdfium.addFunction(() => 0, 'ii'));
  // verstecktes Testmerkmal (internes Testen ohne A-Trust): Keystore nur mit `?test-keystore` in der Adresse
  const testKeystore = new URLSearchParams(window.location.search).has('test-keystore');
  setWebProfile({ keystore: testKeystore });

  const listeners = new Set<(view: ViewState) => void>();
  let current: ViewState | null = null;
  const shell = (kind: string, value: string) => {
    if (kind === 'url') {
      window.open(value, '_blank', 'noopener,noreferrer');
    } else if (value.startsWith('/pida/')) {
      download(baseName(value), pkg.read_file(value));
    }
  };
  const core = new pkg.WebCore(navigator.language || 'de', pkg.version(), (json) => {
    current = JSON.parse(json) as ViewState;
    for (const cb of listeners) cb(current);
  }, shell, createSlTransport(
    base,
    (id, ok, response, error) => pkg.sl_resolve(id, ok, response, error),
    () => current?.language ?? 'de',
    () => {
      // wie das × der Signierkarte: Stapel abbrechen bzw. Signieren beenden
      const step = current?.step;
      const batch = step?.step === 'signing' && !!step.progress?.batch;
      core.dispatch(JSON.stringify({ type: batch ? 'abort_batch' : 'cancel' }));
    },
  ), testKeystore);

  const images = new Map<string, string>();
  const image = (key: string, render: () => Uint8Array<ArrayBuffer>, type = 'image/png'): string => {
    const hit = images.get(key);
    if (hit) return hit;
    try {
      const url = URL.createObjectURL(new Blob([render()], { type }));
      images.set(key, url);
      if (images.size > 400) {
        const [oldKey, oldUrl] = images.entries().next().value as [string, string];
        images.delete(oldKey);
        URL.revokeObjectURL(oldUrl);
      }
      return url;
    } catch {
      return EMPTY_PNG;
    }
  };

  const outputPath = (view: ViewState | null, doc?: DocId | null): string | null => {
    if (!view) return null;
    if (doc === undefined || doc === null) return view.step.step === 'output' ? view.step.result.path : null;
    const item = view.done.find((d) => d.id === doc);
    return item && item.result.kind === 'signed' ? item.result.output.path : null;
  };

  const pick = (accept: string, multiple: boolean): Promise<string[] | null> =>
    new Promise((resolve) => {
      const input = document.createElement('input');
      input.type = 'file';
      input.accept = accept;
      input.multiple = multiple;
      input.addEventListener('cancel', () => resolve(null));
      input.addEventListener('change', async () => {
        const files = [...(input.files ?? [])];
        if (files.length === 0) {
          resolve(null);
          return;
        }
        const paths: string[] = [];
        for (const f of files) paths.push(pkg.put_file(f.name, new Uint8Array(await f.arrayBuffer())));
        resolve(paths);
      });
      input.click();
    });

  return {
    kind: 'web',
    view: async () => current ?? (JSON.parse(core.view()) as ViewState),
    async dispatch(event: UiEvent) {
      try {
        core.dispatch(JSON.stringify(event));
      } catch (e) {
        throw new Error(e instanceof Error ? e.message : 'ng-ui-bridge-error');
      }
    },
    onState(cb) {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
    pageTileUrl: (doc, page, scale) => {
      const s = tileScale(scale);
      return image(`page/${doc}/${page}/${s}`, () => core.page_tile(BigInt(doc), page, s));
    },
    blockPreviewUrl: (scale, revision) => image(blockPreviewPath(scale, revision), () => core.block_preview(tileScale(scale))),
    thumbUrl: (doc) => image(`thumb/${doc}/${current?.batch?.items.length ?? 0}`, () => core.thumbnail(BigInt(doc))),
    qrPlaceholderUrl: (spec, format) =>
      image(qrPlaceholderPath(spec, format), () => core.qr_placeholder(JSON.stringify(spec), format), format === 'svg' ? 'image/svg+xml' : 'image/png'),

    async pickFiles({ filters, multiple }) {
      const accept = filters.flatMap((f) => f.extensions.map((e) => `.${e}`)).join(',');
      return pick(accept, multiple);
    },
    async pickSavePath({ defaultPath }) {
      return `${pkg.output_dir()}/${baseName(defaultPath ?? 'dokument_signiert.pdf')}`;
    },
    async pickDirectory() {
      return pkg.output_dir();
    },
    async pickMobileFiles() {
      throw new Error('ng-mobile-unsupported');
    },
    async pickMobileDocuments() {
      throw new Error('ng-mobile-unsupported');
    },
    onFileDrag(cb: (event: FileDragEvent) => void) {
      const over = (e: DragEvent) => {
        if (!e.dataTransfer?.types.includes('Files')) return;
        e.preventDefault();
        cb({ type: 'over', x: e.clientX, y: e.clientY });
      };
      const enter = (e: DragEvent) => {
        if (!e.dataTransfer?.types.includes('Files')) return;
        e.preventDefault();
        cb({ type: 'enter', paths: [], x: e.clientX, y: e.clientY });
      };
      const leave = (e: DragEvent) => {
        if (e.relatedTarget === null) cb({ type: 'leave' });
      };
      const drop = async (e: DragEvent) => {
        if (!e.dataTransfer?.files.length) return;
        e.preventDefault();
        const paths: string[] = [];
        for (const f of [...e.dataTransfer.files]) paths.push(pkg.put_file(f.name, new Uint8Array(await f.arrayBuffer())));
        cb({ type: 'drop', paths, x: e.clientX, y: e.clientY });
      };
      window.addEventListener('dragenter', enter);
      window.addEventListener('dragover', over);
      window.addEventListener('dragleave', leave);
      window.addEventListener('drop', drop);
      return () => {
        window.removeEventListener('dragenter', enter);
        window.removeEventListener('dragover', over);
        window.removeEventListener('dragleave', leave);
        window.removeEventListener('drop', drop);
      };
    },
    async openUrl(url) {
      // E-Mail-Adressen öffnen das E-Mail-Programm ohne leeren Tab
      if (url.startsWith('mailto:')) window.location.href = url;
      else window.open(url, '_blank', 'noopener,noreferrer');
    },
    // Web: „Im Ordner zeigen“ des Stapels lädt alle signierten Dokumente als ZIP (Einzelergebnis: das PDF)
    async revealOutput(doc) {
      const batch = doc !== undefined && doc !== null ? current?.batch : null;
      if (batch) {
        const files = batch.items.flatMap((i) => (i.status.state === 'signed' ? [i.status.path] : []));
        if (files.length === 0) throw new Error('ng-ui-open-failed');
        download(BATCH_ZIP, zipStored(files.map((p) => ({ name: baseName(p), data: pkg.read_file(p) }))));
        return;
      }
      const path = outputPath(current, doc);
      if (!path) throw new Error('ng-ui-open-failed');
      download(baseName(path), pkg.read_file(path));
    },
    async openLogDir() {
      throw new Error('ng-ui-open-failed');
    },
    // Drittlizenzen liegen neben der Seite (`apps/web/build.sh` mit PIDA_WEB_NOTICES); ohne sie wie ein Entwicklungsbuild
    async licenseText(kind: LicenseKind) {
      if (kind === 'eupl') return pkg.license_eupl();
      const file = kind === 'notices' ? 'THIRD-PARTY-NOTICES.md' : 'RUST-STD-COPYRIGHT.html';
      const r = await fetch(new URL(`notices/${file}`, base).href, { cache: 'no-cache' }).catch(() => null);
      if (!r || !r.ok) throw new Error('ng-ui-licenses-missing');
      return r.text();
    },
    // Demodokument liegt neben der Seite (Vite kopiert `public/`); in das Dateisystem des Kerns wie eine gewählte Datei
    async demoDocument() {
      const r = await fetch(new URL('PIDA-Demo.pdf', base).href).catch(() => null);
      if (!r || !r.ok) throw new Error('ng-ui-open-failed');
      return pkg.put_file('PIDA-Demo.pdf', new Uint8Array(await r.arrayBuffer()));
    },
    async appInfo(): Promise<AppInfo> {
      return { version: pkg.version(), platform: 'web', core_available: true };
    },
  };
}
