/**
 * Einstieg der Oberfläche: Tauri-Bridge zum Rust-Kern, oder der TS-Mock-Kern bei `VITE_MOCK=1`
 * (`npm run dev:mock`) bzw. außerhalb von Tauri (Browser-Vorschau). Design-System (ADR 0004 §4.2):
 * `styles/tokens.css` → `base.css` → `components.css`; die Ansichten laden ihre eigenen Stile.
 */
// Design-System zuerst: die Stile der Ansichten (aus den Modulen unten) bauen darauf auf und überschreiben es
import './styles/tokens.css';
import './styles/base.css';
import './styles/components.css';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './app/App';
import { createTauriBridge, inTauri, type Bridge } from './core/bridge';
import { defaultSettings } from './core/defaults';
import type { MockCore, MockOptions } from './core/mock';
import type { FirstRunMockOptions } from './core/mock/firstrun';
import type { SettingsMockOptions } from './core/mock/settings';
import type { Settings } from './core/types';

async function bridge(): Promise<Bridge> {
  const forceMock = import.meta.env.VITE_MOCK === '1';
  if (!forceMock && inTauri()) return createTauriBridge();
  // Web-Variante (`npm run build:web`): Rust-Kern als WebAssembly im Browser (eigener Chunk)
  if (import.meta.env.VITE_WEB === '1') return (await import('./core/web')).createWebBridge();
  // Mock-Kern nur bei Bedarf laden (eigener Chunk, nicht im Startpfad der App).
  const { MockCore: Mock, MOCK_SCENARIOS } = await import('./core/mock');
  const mock = new Mock(mockOptionsFromUrl(window.location.search, MOCK_SCENARIOS));
  installBrowserDrop(mock);
  return mock;
}

/**
 * Browser-Vorschau (ADR 0004 §4.8): `?scenario=start|blank|contract|field|signed|batch|error-password|
 * error-atrust` wie im Prototyp (ohne dessen Prüfmodus, ADR 0004 Nachtrag 15), `&platform=ios|android` (Mobile-Oberfläche), `&flow=app|sms|
 * qr|fido2|decline|timeout|none` (Antwort von ID Austria nach der Anmeldung; `none` = Identität ohne zweiten Faktor), `&resume=1` (Angebot „Signatur
 * fortsetzen“), `&lang=en`, `&examples=1` (Beispiele der späteren Etappen). Stapel (ADR 0004 §3.8):
 * `&batch=0` (ohne Stapelübersicht), `&search=0|1` (Platzhaltersuche; Standard an wie im Kern), `&kind=fields`
 * (Platzhaltersuche „Leere Signaturfelder“ statt QR, E3), `&pdfa=1` („PDF/A-kompatibel signieren“, E3),
 * `&policy=recent-off|recent-on|updates` (Richtlinie: „Zuletzt signiert“ bzw. Update-Prüfung festgelegt, E3),
 * `&policy=only-ida` (Richtlinie gibt nur ID Austria frei: keine Signiermittel-Auswahl am Desktop, Nachtrag 20),
 * `&fault=decline|network|session|too-large` (Störung beim zweiten Dokument), `&existing=1` (eine Zieldatei
 * gibt es schon), `&folder=ask|<Pfad>` (Speicherort), `&auto=0` (App-Bestätigung nur über „In der App
 * bestätigt“). Ersteinrichtung (E4): `&firstrun=1` (fehlende Einstellungsdatei), `&legacy=1` (PDF-Over-
 * Einstellungen gefunden; ohne `firstrun` als eigenes Angebot), `&secrets=1` (Klartext-Passwörter darin),
 * `&emblem=1` (eigene Bildmarke in der Altkonfiguration; ohne bleibt das PIDA-Siegel), `&pick=<Name>.pdf`
 * („PDF auswählen“ öffnet dieses Beispiel, z. B. `Signaturfelder.pdf`, ADR 0004 Nachtrag 16), `&update=available|
 * failed|offline|unchecked|checking` (Update-Prüfung in „Über PIDA“, Nachtrag 21; `&policy=updates` = von der
 * Organisation deaktiviert). Nur der Mock-Kern wertet das aus.
 */
function mockOptionsFromUrl(search: string, scenarios: readonly string[]): SettingsMockOptions & FirstRunMockOptions {
  const params = new URLSearchParams(search);
  const platform = params.get('platform');
  const scenario = params.get('scenario') ?? params.get('szenario');
  const flow = params.get('flow');
  const lang = params.get('lang');
  const mobile = platform === 'ios' || platform === 'android';
  const fault = params.get('fault');
  const folder = params.get('folder');
  const placeholderSearch = params.get('search') ?? '1';
  // Demo-Konto wie im Prototyp (frei erfunden): ID Austria als Standard mit gemerktem Benutzer, Keystore
  // auf dem Desktop; `?ask=1` = „Jedes Mal fragen“ (Konto-Chip ohne Vorauswahl)
  const base = defaultSettings();
  const settings: Settings = {
    ...base,
    backend: { default: params.get('ask') === '1' ? null : 'id_austria' },
    id_austria: { ...base.id_austria, username: '+43 660 1234123', remember: true },
    keystore: mobile ? base.keystore : { ...base.keystore, enabled: true, file: '/Users/demo/Zertifikate/max.p12', alias: 'max', declared_type: 'PKCS12' },
    output: { ...base.output, folder: folder === 'ask' ? null : (folder ?? '.') },
    placeholder: { ...base.placeholder, search: placeholderSearch === '1', kind: params.get('kind') === 'fields' ? 'fields' : 'marker' },
    signature: { ...base.signature, pdfa_force: params.get('pdfa') === '1' },
  };
  return {
    settings,
    platform: mobile ? platform : 'desktop',
    scenario: scenario && scenarios.includes(scenario) ? (scenario as MockOptions['scenario']) : 'start',
    signingFlow: flow === 'sms' || flow === 'qr' || flow === 'fido2' || flow === 'decline' || flow === 'timeout' || flow === 'none' ? flow : 'app',
    language: lang === 'en' ? 'en' : undefined,
    examples: params.get('examples') === '1',
    resumeOffer: params.get('resume') === '1' ? { name: 'Vertrag.pdf', backend: 'atrust_native', age_minutes: 4, pending: 1, document_leaves_device: true } : null,
    batchOverview: params.get('batch') !== '0',
    autoCompleteApp: params.get('auto') !== '0',
    batchFaults: fault === 'decline' || fault === 'network' || fault === 'session' || fault === 'too-large' ? { 'Auftragsbestaetigung_4711.pdf': fault } : undefined,
    existingFiles: params.get('existing') === '1' ? ['/Users/demo/Dokumente/Verträge/Vertrag_signed.pdf'] : undefined,
    // „PDF auswählen“ liefert diese Datei (z. B. `?pick=Signaturfelder.pdf`, Formular mit Signaturfeldern)
    pickFilesResult: params.get('pick') ? [`/Users/demo/Dokumente/${params.get('pick')}`] : undefined,
    policy: policyFromUrl(params.get('policy')),
    allowedBackends: params.get('policy') === 'only-ida' ? ['atrust_native'] : undefined,
    firstRun: params.get('firstrun') === '1',
    migrationOffer: params.get('legacy') === '1' ? '/Users/demo/.pdf-over/PDF-Over.config' : null,
    migrationSecrets: params.get('secrets') === '1',
    legacyEmblem: params.get('emblem') === '1' ? '/Users/demo/Bilder/eigenes-logo.png' : null,
    update: updateFromUrl(params.get('update')),
  };
}

/** „Über PIDA“ (Nachtrag 21): `?update=up_to_date|available|failed|offline|unchecked|checking`. */
function updateFromUrl(value: string | null): MockOptions['update'] {
  return value === 'available' || value === 'failed' || value === 'offline' || value === 'unchecked' || value === 'checking' || value === 'up_to_date' ? value : undefined;
}

/** Richtlinie im Mock (`SettingsView::locked`): festgelegte Schalter erscheinen gesperrt mit Hinweis. */
function policyFromUrl(value: string | null): SettingsMockOptions['policy'] {
  switch (value) {
    case 'recent-off':
      return { recent_documents: false };
    case 'recent-on':
      return { recent_documents: true };
    case 'updates':
      return { update_check: false };
    default:
      return undefined;
  }
}

/** Browser-Vorschau: HTML5-Drop als Ersatz für die Fenster-Ereignisse von Tauri (nur Dateinamen). */
function installBrowserDrop(mock: MockCore): void {
  let depth = 0;
  window.addEventListener('dragenter', (e) => {
    if (!e.dataTransfer?.types.includes('Files')) return;
    depth += 1;
    mock.simulateFileDrag({ type: 'enter', paths: [], x: e.clientX, y: e.clientY });
  });
  window.addEventListener('dragover', (e) => {
    if (!e.dataTransfer?.types.includes('Files')) return;
    e.preventDefault();
    mock.simulateFileDrag({ type: 'over', x: e.clientX, y: e.clientY });
  });
  window.addEventListener('dragleave', () => {
    depth = Math.max(0, depth - 1);
    if (depth === 0) mock.simulateFileDrag({ type: 'leave' });
  });
  window.addEventListener('drop', (e) => {
    e.preventDefault();
    depth = 0;
    const names = Array.from(e.dataTransfer?.files ?? []).map((f) => `/Browser/${f.name}`);
    mock.simulateFileDrag({ type: 'drop', paths: names, x: e.clientX, y: e.clientY });
  });
}

const root = document.getElementById('root');
if (root) {
  bridge()
    .then((b) => {
      createRoot(root).render(
        <StrictMode>
          <App bridge={b} />
        </StrictMode>,
      );
    })
    .catch(() => {
      root.textContent = 'PIDA: Die Oberfläche konnte nicht gestartet werden. / The user interface could not be started.';
    });
}
