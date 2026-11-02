/**
 * Shell und Kernpfad mit dem Mock-Kern (ADR 0004 Abnahme E1 Punkt 1, 5, 6): Datei ablegen → automatisch
 * platziert → „Signieren“ → Passwort → in der App bestätigen → Fertig, ohne Wechsel der Ansicht bis „Fertig“;
 * Abbrechen zurück in den Arbeitsbereich mit unveränderter Platzierung; Ablehnung in der App; Netzfehler
 * mit „Erneut versuchen“ ohne zweite Anmeldung; Keystore; Kopfzeile, Kürzel, Zugriffstasten, Hinweise.
 * Dazu die Regel aus E0: `App.tsx` importiert nichts direkt aus `settings/**` oder dem Migrationsdialog.
 */
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { defaultSettings } from '../core/defaults';
import type { Settings, UiEvent } from '../core/types';
import { expectNoAxeViolations } from '../test/a11y';
import { lastEvent, renderApp } from '../test/render';
import { editActions, viewOf, workspaceView } from '../test/views';
import { accessKeyFromEvent, mainView, triggerAccessKey } from './App';
import { buildCommands, buildShortcuts, COMMANDS } from './commands';
import { translator } from '../i18n/catalog';

/** Einstellungen mit ID Austria als Standard (gemerkter Benutzer). */
function idAustria(extra: Partial<Settings> = {}): Settings {
  const s = defaultSettings();
  return { ...s, backend: { default: 'id_austria' }, id_austria: { ...s.id_austria, username: '+43 660 1234123', remember: true }, output: { ...s.output, folder: '.' }, ...extra };
}

async function openOne(user: ReturnType<typeof renderApp>['user']) {
  await user.click(await screen.findByRole('button', { name: 'PDF auswählen' }));
  return screen.findByRole('application', { name: 'Signaturblock' });
}

async function login(user: ReturnType<typeof renderApp>['user'], password = 'geheim123') {
  const dialog = await screen.findByRole('dialog', { name: 'Signieren mit ID Austria' });
  await user.type(await within(dialog).findByLabelText('Signatur-Passwort'), password);
  await user.click(within(dialog).getByRole('button', { name: 'Anmelden' }));
  return dialog;
}

describe('Kernpfad mit dem Mock-Kern', () => {
  it('Ablegen → automatisch platziert → Signieren → Passwort → App → Fertig (Ansicht bleibt eingehängt)', async () => {
    const { mock, user } = renderApp({ settings: idAustria(), pickFilesResult: ['/Users/demo/Dokumente/Vertrag.pdf'] });
    await screen.findByRole('heading', { level: 1, name: 'Was möchten Sie signieren?' });
    act(() => mock.simulateFileDrag({ type: 'drop', paths: ['/Users/demo/Dokumente/Vertrag.pdf'], x: 1, y: 1 }));
    const sticker = await screen.findByRole('application', { name: 'Signaturblock' });
    // automatisch auf freier Fläche der letzten Seite (PDF-AS-Regel) mit Einblendung
    expect(screen.getByText('Signaturblock automatisch auf freier Fläche platziert')).toBeInTheDocument();
    expect(screen.getByText(/^Seite 3 · /)).toBeInTheDocument();
    const workspace = document.querySelector('.workspace');
    await user.click(screen.getByRole('button', { name: /^Signieren/ }));
    expect(mock.dispatched.filter((e) => e.type === 'sign_with')).toEqual([{ type: 'sign_with', backend: 'atrust_native' }]);
    const dialog = await login(user);
    expect(await within(dialog).findByRole('heading', { name: 'Benachrichtigung an die ID-Austria-App gesendet' })).toBeInTheDocument();
    // die Seitenansicht ist dieselbe (kein Wechsel der Ansicht)
    expect(document.querySelector('.workspace')).toBe(workspace);
    expect(sticker.isConnected).toBe(true);
    act(() => mock.completeConfirmation());
    expect(await screen.findByRole('heading', { level: 1, name: 'Signiert' })).toBeInTheDocument();
    expect(screen.getByText('Vertrag_signed.pdf')).toBeInTheDocument();
    expect(screen.getByText('Signatur erstellt und geprüft (Selbstprüfung)')).toBeInTheDocument();
    expect(mock.choices).toEqual({ position: 'auto', backend: 'atrust_native' });
    // A1: nur ein `sign_with` – keine Sitzung ohne ausdrückliche Aktion
    expect(mock.dispatched.some((e) => e.type === 'confirm_position' || e.type === 'choose_backend')).toBe(false);
  });

  it('Passwort per Augen-Symbol im Klartext zeigen und wieder verbergen', async () => {
    const { mock, user } = renderApp({ settings: idAustria(), pickFilesResult: ['/Users/demo/Vertrag.pdf'] });
    await openOne(user);
    await user.click(screen.getByRole('button', { name: /^Signieren/ }));
    const dialog = await screen.findByRole('dialog', { name: 'Signieren mit ID Austria' });
    const field = within(dialog).getByLabelText('Signatur-Passwort');
    await user.type(field, 'geheim123');
    expect(field).toHaveAttribute('type', 'password');
    await user.click(within(dialog).getByRole('button', { name: 'Passwort anzeigen' }));
    expect(field).toHaveAttribute('type', 'text');
    expect(field).toHaveValue('geheim123');
    expect(field).toHaveAttribute('autocorrect', 'off');
    const hide = within(dialog).getByRole('button', { name: 'Passwort verbergen' });
    expect(hide).toHaveAttribute('aria-pressed', 'true');
    await user.click(hide);
    expect(field).toHaveAttribute('type', 'password');
    // das Augen-Symbol schickt das Formular nicht ab
    expect(mock.dispatched.some((e) => e.type === 'submit_form')).toBe(false);
    await user.click(within(dialog).getByRole('button', { name: 'Anmelden' }));
    expect(await within(dialog).findByRole('heading', { name: 'Benachrichtigung an die ID-Austria-App gesendet' })).toBeInTheDocument();
  });

  it('zweites Dokument (ohne Stapelübersicht): gemerkte Anmeldung → direkt die App-Bestätigung; „Nächstes Dokument“', async () => {
    const { mock, user } = renderApp({ settings: idAustria(), batchOverview: false, pickFilesResult: ['/Users/demo/Vertrag.pdf', '/Users/demo/Angebot.pdf'] });
    await openOne(user);
    await user.click(screen.getByRole('button', { name: /^Signieren/ }));
    const dialog = await screen.findByRole('dialog', { name: 'Signieren mit ID Austria' });
    // „Daten merken“ ist vorbelegt (gemerkter Benutzer)
    expect(within(dialog).getByRole('checkbox', { name: /Daten merken/ })).toBeChecked();
    await user.type(within(dialog).getByLabelText('Signatur-Passwort'), 'geheim123');
    await user.click(within(dialog).getByRole('button', { name: 'Anmelden' }));
    await within(dialog).findByRole('heading', { name: 'Benachrichtigung an die ID-Austria-App gesendet' });
    act(() => mock.completeConfirmation());
    expect(await screen.findByText('Weiter mit „Angebot.pdf“ · 1 Dokument wartet')).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Nächstes Dokument' }));
    await screen.findByRole('application', { name: 'Signaturblock' });
    expect(screen.getByRole('banner')).toHaveTextContent('Angebot.pdf');
    await user.click(screen.getByRole('button', { name: /^Signieren/ }));
    const second = await screen.findByRole('dialog', { name: 'Signieren mit ID Austria' });
    expect(await within(second).findByRole('heading', { name: 'Benachrichtigung an die ID-Austria-App gesendet' })).toBeInTheDocument();
  });

  it('Abbrechen → Arbeitsbereich mit unveränderter Platzierung und Hinweis „es wurde nichts signiert“', async () => {
    const { mock, user } = renderApp({ settings: idAustria(), pickFilesResult: ['/Users/demo/Vertrag.pdf'] });
    const sticker = await openOne(user);
    fireEvent.keyDown(sticker, { key: 'ArrowRight' });
    await waitFor(() => expect(mock.dispatched.some((e) => e.type === 'nudge_block')).toBe(true));
    const before = (screen.getByRole('application', { name: 'Signaturblock' }) as HTMLElement).style.left;
    await user.click(screen.getByRole('button', { name: /^Signieren/ }));
    const dialog = await screen.findByRole('dialog', { name: 'Signieren mit ID Austria' });
    await user.click(within(dialog).getByRole('button', { name: 'Abbrechen' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(await screen.findByText('Signieren abgebrochen – es wurde nichts signiert.')).toBeInTheDocument();
    expect((screen.getByRole('application', { name: 'Signaturblock' }) as HTMLElement).style.left).toBe(before);
  });

  it('ID Austria ohne zweiten Faktor (Test-Identitäten): nach der Anmeldung gleich signiert, kein Bestätigungsschritt', async () => {
    const { mock, user } = renderApp({ settings: idAustria(), signingFlow: 'none', pickFilesResult: ['/Users/demo/Vertrag.pdf'] });
    await openOne(user);
    await user.click(screen.getByRole('button', { name: /^Signieren/ }));
    const dialog = await login(user);
    // Abschluss ohne Bestätigungsmaske: der Stepper hakt kein „In der App bestätigen“ ab
    await waitFor(() => expect(within(dialog).queryByText('In der App bestätigen')).not.toBeInTheDocument());
    expect(await screen.findByRole('heading', { level: 1, name: 'Signiert' }, { timeout: 3000 })).toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: 'Benachrichtigung an die ID-Austria-App gesendet' })).not.toBeInTheDocument();
    expect(mock.dispatched.filter((e) => e.type === 'submit_form')).toHaveLength(1);
  });

  it('Ablehnung in der App → Fehlerschritt „abgelehnt“, Erneut versuchen ohne neue Anmeldung', async () => {
    const { user } = renderApp({ settings: idAustria(), signingFlow: 'decline', pickFilesResult: ['/Users/demo/Vertrag.pdf'] });
    await openOne(user);
    await user.click(screen.getByRole('button', { name: /^Signieren/ }));
    const dialog = await login(user);
    expect(await within(dialog).findByRole('heading', { name: 'In der App abgelehnt oder abgelaufen' })).toBeInTheDocument();
    await expectNoAxeViolations();
    await user.click(within(dialog).getByRole('button', { name: 'Erneut versuchen' }));
    expect(await within(dialog).findByRole('heading', { name: 'Benachrichtigung an die ID-Austria-App gesendet' })).toBeInTheDocument();
    expect(within(dialog).queryByLabelText('Signatur-Passwort')).not.toBeInTheDocument();
  });

  it('A-Trust nicht erreichbar → Fehlerschritt mit Details; Erneut versuchen in derselben Sitzung (keine zweite Login-Maske)', async () => {
    const { mock, user } = renderApp({ settings: idAustria(), scenario: 'error-atrust' });
    await screen.findByRole('application', { name: 'Signaturblock' });
    await user.click(screen.getByRole('button', { name: /^Signieren/ }));
    const dialog = await login(user);
    expect(await within(dialog).findByRole('heading', { name: 'Der Signaturdienst ist gerade nicht erreichbar' })).toBeInTheDocument();
    await user.click(within(dialog).getByRole('button', { name: 'Erneut versuchen' }));
    expect(await within(dialog).findByRole('heading', { name: 'Benachrichtigung an die ID-Austria-App gesendet' })).toBeInTheDocument();
    expect(mock.dispatched.filter((e) => e.type === 'submit_form' && e.answer.kind === 'login')).toHaveLength(1);
  });

  it('Keystore mit falschem Passwort, dann richtig; unsichtbar signiert', async () => {
    const s = defaultSettings();
    const settings = { ...s, backend: { default: 'keystore' as const }, keystore: { ...s.keystore, enabled: true, file: '/Users/demo/id.p12' }, output: { folder: '/Users/demo/out', postfix: '_signed' } };
    const { mock, user } = renderApp({ settings, localBkuAvailable: false, pickFilesResult: ['/Users/demo/Vertrag.pdf'] });
    await openOne(user);
    await user.click(screen.getByRole('switch', { name: 'Unsichtbar signieren' }));
    await user.click(screen.getByRole('button', { name: /^Signieren/ }));
    const dialog = await screen.findByRole('dialog', { name: 'Signieren mit Keystore' });
    await user.type(await within(dialog).findByLabelText('Keystore-Passwort'), 'falsch{Enter}');
    expect(await within(dialog).findByText(/Keystore-Passwort nicht akzeptiert/)).toBeInTheDocument();
    await user.type(within(dialog).getByLabelText('Keystore-Passwort'), 'richtig{Enter}');
    expect(await screen.findByRole('heading', { level: 1, name: 'Signiert' }, { timeout: 3000 })).toBeInTheDocument();
    expect(screen.getByText('Gespeichert in /Users/demo/out')).toBeInTheDocument();
    expect(mock.choices).toEqual({ position: 'invisible', backend: 'keystore' });
  });

  it('Nicht-PDF: Zustandskarte mit Erneut versuchen/Schließen; zurück zum Start', async () => {
    const { user } = renderApp({ pickFilesResult: ['/Users/demo/bild.png'] });
    await user.click(await screen.findByRole('button', { name: 'PDF auswählen' }));
    expect(await screen.findByRole('heading', { level: 1, name: 'Diese Datei ist kein PDF' })).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Schließen' }));
    expect(await screen.findByRole('heading', { level: 1, name: 'Was möchten Sie signieren?' })).toBeInTheDocument();
  });

  it('kennwortgeschütztes PDF aus dem Szenario', async () => {
    renderApp({ scenario: 'error-password' });
    expect(await screen.findByRole('heading', { level: 1, name: 'Dieses PDF ist mit einem Kennwort geschützt' })).toBeInTheDocument();
  });

  it('Ablehnen eines Ereignisses durch den Kern erscheint als Fehlerhinweis', async () => {
    const { mock, user } = renderApp();
    mock.dispatch = async () => {
      throw new Error('ng-ui-event-invalid');
    };
    await user.click(await screen.findByRole('button', { name: 'PDF auswählen' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('noch nicht unterstützt');
  });
});

describe('Kopfzeile und Kürzel', () => {
  it('Wortmarke ohne „Prototyp“, Dokumentkontext, kein „Prüfen“ (Nachtrag 15), DE/EN, Einstellungen', async () => {
    const { mock, user } = renderApp({ view: workspaceView() });
    const header = await screen.findByRole('banner');
    expect(header).not.toHaveTextContent(/Prototyp/i);
    expect(within(header).getByRole('button', { name: 'PIDA – Startseite' })).toHaveTextContent('PIDA');
    expect(header).toHaveTextContent('Vertrag.pdf');
    expect(header).toHaveTextContent('3 Seiten · 214 KB');
    // kein Prüfmodus mehr (ADR 0004 Nachtrag 15): weder Knopf noch Kontext in der Kopfzeile
    expect(within(header).queryByRole('button', { name: /prüfen/i })).not.toBeInTheDocument();
    expect(header).not.toHaveTextContent(/Prüf/);
    await user.click(within(header).getByRole('radio', { name: 'EN' }));
    expect(lastEvent(mock)).toEqual({ type: 'set_ui_language', language: 'en' });
    await user.click(within(header).getByRole('button', { name: 'Einstellungen' }));
    expect(lastEvent(mock)).toEqual({ type: 'open_settings' });
    // „Zur Startseite“ parkt offene Arbeit (Nachtrag 10)
    await user.click(within(header).getByRole('button', { name: 'PIDA – Startseite' }));
    expect(lastEvent(mock)).toEqual({ type: 'go_home' });
    await expectNoAxeViolations();
  });

  it('Sprache wechselt sofort (Mock: set_ui_language), Farbschema lokal', async () => {
    const { user } = renderApp();
    await screen.findByRole('heading', { level: 1, name: 'Was möchten Sie signieren?' });
    await user.click(screen.getByRole('radio', { name: 'EN' }));
    expect(await screen.findByRole('heading', { level: 1, name: 'What would you like to sign?' })).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Colour scheme' }));
    await user.click(await screen.findByRole('menuitemradio', { name: 'Dark' }));
    expect(document.documentElement.getAttribute('data-theme')).toBe('dark');
    expect(localStorage.getItem('pida.theme')).toBe('dark');
  });

  it('⌘/Strg+, öffnet die Einstellungen; Sprunglink zum Inhalt', async () => {
    const { mock, user } = renderApp({ view: workspaceView() });
    await screen.findByRole('application', { name: 'Signaturblock' });
    await user.keyboard('{Control>},{/Control}');
    expect(lastEvent(mock)).toEqual({ type: 'open_settings' });
    expect(screen.getByRole('link', { name: 'Zum Inhalt springen' })).toHaveAttribute('href', '#main');
  });

  it('Ansicht je Zustand (klassischer Zustand ohne `document`: Startansicht, keine alten Bildschirme)', () => {
    const doc = workspaceView();
    expect(mainView(doc)).toBe('workspace');
    expect(mainView({ ...doc, document: null })).toBe('start');
    expect(mainView(viewOf({ step: 'open', queue: [] }))).toBe('start');
  });

  it('Befehlsliste: alle Befehle des Prototyps mit Freigaben aus `actions`, Kürzel 15/1/50 pt', () => {
    const t = translator('de');
    const view = workspaceView();
    const deps = { view, t, isMac: true, dispatch: async () => true, browse: async () => undefined, startDemo: async () => undefined, requestSign: () => undefined, openSettings: () => undefined, setTheme: () => undefined, setOverlay: () => undefined };
    const cmds = buildCommands(deps);
    expect(cmds.map((c) => c.id)).toEqual(COMMANDS.map((c) => c.id));
    expect(cmds.find((c) => c.id === 'sign')).toMatchObject({ label: 'Signieren', keys: ['⌘', '↵'], isDisabled: false });
    // kein Befehl „Signatur prüfen“ mehr (Nachtrag 15)
    expect(cmds.map((c) => c.id)).not.toContain('verify');
    expect(cmds.some((c) => /prüf/i.test(c.label) || /verify|prüfen/i.test(c.keywords))).toBe(false);
    expect(cmds.find((c) => c.id === 'signature')?.keywords).toContain('logo');
    const keys = buildShortcuts(t, false);
    expect(keys.find((k) => k.id === 'move')?.label).toBe('Signaturblock verschieben (15 pt)');
    expect(keys.find((k) => k.id === 'move-coarse')?.label).toBe('Grob verschieben (50 pt)');
  });
});

describe('Zugriffstasten', () => {
  it('Alt+Taste (Windows/Linux) bzw. Ctrl+Option (macOS); AltGr wird ignoriert', () => {
    const ev = (init: KeyboardEventInit) => new KeyboardEvent('keydown', init);
    expect(accessKeyFromEvent(ev({ code: 'KeyB', altKey: true }), false)).toBe('b');
    expect(accessKeyFromEvent(ev({ code: 'KeyB', altKey: true, ctrlKey: true }), false)).toBeNull();
    expect(accessKeyFromEvent(ev({ code: 'KeyB', altKey: true }), true)).toBeNull();
    expect(accessKeyFromEvent(ev({ code: 'KeyB', altKey: true, ctrlKey: true }), true)).toBe('b');
    expect(accessKeyFromEvent(ev({ code: 'Digit3', altKey: true }), false)).toBe('3');
    expect(accessKeyFromEvent(ev({ code: 'Space', altKey: true }), false)).toBeNull();
  });

  it('nur die oberste Ebene und keine gesperrten Elemente', () => {
    document.body.innerHTML = `
      <button id="page" data-accesskey="x">Seite</button>
      <div data-modal-layer><button id="dlg-off" disabled data-accesskey="x">Aus</button><button id="dlg" data-accesskey="x">Dialog</button></div>`;
    let clicked = '';
    for (const b of Array.from(document.querySelectorAll('button'))) b.addEventListener('click', () => (clicked = b.id));
    expect(triggerAccessKey('x')).toBe(true);
    expect(clicked).toBe('dlg');
    expect(triggerAccessKey('q')).toBe(false);
    document.body.innerHTML = '';
  });

  it('doppelte Zugriffstaste: Fokus wandert reihum, nichts wird ausgelöst', () => {
    document.body.innerHTML = `
      <div data-modal-layer>
        <div class="field"><label data-accesskey="r">Sprache</label><button id="lang" aria-haspopup="listbox">DE</button></div>
        <button id="reset" data-accesskey="r">Logo zurücksetzen</button>
        <button id="other" data-accesskey="s">Speichern</button>
      </div>`;
    let clicked = '';
    for (const b of Array.from(document.querySelectorAll('button'))) b.addEventListener('click', () => (clicked = b.id));
    expect(triggerAccessKey('r')).toBe(true);
    expect(document.activeElement?.id).toBe('lang');
    expect(triggerAccessKey('r')).toBe(true);
    expect(document.activeElement?.id).toBe('reset');
    expect(triggerAccessKey('r')).toBe(true);
    expect(document.activeElement?.id).toBe('lang');
    expect(clicked).toBe('');
    expect(triggerAccessKey('s')).toBe(true);
    expect(clicked).toBe('other');
    document.body.innerHTML = '';
  });

  it('„Signieren“ trägt die Zugriffstaste S; Unterstreichung nur bei gedrückter Alt-Taste', async () => {
    const { mock } = renderApp({ view: workspaceView() });
    const sign = await screen.findByRole('button', { name: /^Signieren/ });
    expect(sign).toHaveAttribute('aria-keyshortcuts', 'Alt+S');
    expect(document.documentElement.hasAttribute('data-accesskeys')).toBe(false);
    fireEvent.keyDown(window, { key: 'Alt', altKey: true });
    expect(document.documentElement.hasAttribute('data-accesskeys')).toBe(true);
    fireEvent.keyDown(window, { key: 's', code: 'KeyS', altKey: true });
    fireEvent.keyUp(window, { key: 'Alt' });
    expect(document.documentElement.hasAttribute('data-accesskeys')).toBe(false);
    await waitFor(() => expect(mock.dispatched.some((e: UiEvent) => e.type === 'sign_with')).toBe(true));
  });
});

describe('Hinweise (Toasts)', () => {
  it('Info nach 6 s weg (eigene Frist je Hinweis), Fehler bleiben; Kernhinweise schließt dismiss_notice', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const view = viewOf({ step: 'open', queue: [] }, { notices: [{ id: 1, code: 'ng-signing-cancelled', params: {} }], actions: editActions() });
      const { mock } = renderApp({ view });
      expect(await screen.findByText('Signieren abgebrochen – es wurde nichts signiert.')).toBeInTheDocument();
      vi.spyOn(mock, 'dispatch').mockRejectedValueOnce(new Error('ng-ui-bridge-error'));
      act(() => mock.simulateFileDrag({ type: 'drop', paths: ['/a.pdf'], x: 1, y: 1 }));
      await screen.findByRole('alert');
      await act(async () => {
        await vi.advanceTimersByTimeAsync(6100);
      });
      expect(mock.dispatched.some((e) => e.type === 'dismiss_notice' && e.id === 1)).toBe(true);
      expect(document.querySelectorAll('.toast-error')).toHaveLength(1);
    } finally {
      vi.useRealTimers();
    }
  });
});

describe('Regel E0: App.tsx bindet UI-B nur über die stabilen Exporte ein', () => {
  it('keine direkten Importe aus settings/** oder dem Migrationsdialog', () => {
    const source = readFileSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), 'App.tsx'), 'utf8');
    const imports = [...source.matchAll(/from '([^']+)'/g)].map((m) => m[1] ?? '');
    expect(imports.filter((p) => p.startsWith('../settings/') || p.includes('MigrationDialog') || p.includes('/components/'))).toEqual([]);
    expect(imports).toContain('../features/settings');
    expect(imports).toContain('../features/onboarding');
  });
});
