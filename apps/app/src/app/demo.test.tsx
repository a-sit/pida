/**
 * „Demo starten“: Kürzel ⌘/Strg+D (Apps) bzw. ⌥⇧D/Alt+Umschalt+D (Web, ⌘D ist dort „Lesezeichen“), Befehlspalette,
 * Kürzelübersicht und der Link auf der Startseite der Mobilgeräte. Die Plattform liefert eine frische Kopie des
 * Demodokuments (`demoDocument`), die Oberfläche öffnet sie wie eine gewählte Datei (`open_files`).
 */
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { setWebProfile } from '../core/web-profile';
import { renderApp } from '../test/render';

const DESKTOP_DEMO = '/Users/demo/Library/Caches/at.asit.pida/demo/PIDA-Demo.pdf';

afterEach(() => setWebProfile(null));

const demoOpened = (mock: ReturnType<typeof renderApp>['mock']) =>
  mock.dispatched.filter((e) => e.type === 'open_files' && e.paths.some((p) => p.endsWith('PIDA-Demo.pdf')));

describe('Demo starten', () => {
  it('⌘D öffnet das Demodokument; der QR-Platzhalter wird zum Ziel', async () => {
    const { mock } = renderApp({ isMac: true });
    await screen.findByRole('heading', { level: 1 });
    fireEvent.keyDown(window, { key: 'd', code: 'KeyD', metaKey: true });
    await waitFor(() => expect(demoOpened(mock)).toEqual([{ type: 'open_files', paths: [DESKTOP_DEMO] }]));
    expect(mock.nativeCalls).toContain('demoDocument');
    await screen.findByRole('application', { name: 'Signaturblock' });
  });

  it('Strg+D unter Windows/Linux; Alt+Umschalt+D und Tippen in einem Feld lösen nichts aus', async () => {
    const { mock } = renderApp({ isMac: false });
    await screen.findByRole('heading', { level: 1 });
    fireEvent.keyDown(window, { key: 'D', code: 'KeyD', altKey: true, shiftKey: true });
    const input = document.createElement('input');
    document.body.append(input);
    fireEvent.keyDown(input, { key: 'd', code: 'KeyD', ctrlKey: true });
    input.remove();
    expect(mock.nativeCalls).not.toContain('demoDocument');
    fireEvent.keyDown(window, { key: 'd', code: 'KeyD', ctrlKey: true });
    await waitFor(() => expect(demoOpened(mock)).toHaveLength(1));
  });

  it('Web: ⌥⇧D (physische Taste D, unter macOS tippt sie „Î“), ⌘D bleibt dem Browser', async () => {
    setWebProfile({ keystore: false });
    const { mock } = renderApp({ isMac: true });
    await screen.findByRole('heading', { level: 1 });
    const bookmark = fireEvent.keyDown(window, { key: 'd', code: 'KeyD', metaKey: true });
    expect(bookmark).toBe(true); // nicht verhindert
    expect(mock.nativeCalls).not.toContain('demoDocument');
    fireEvent.keyDown(window, { key: 'Î', code: 'KeyD', altKey: true, shiftKey: true });
    await waitFor(() => expect(demoOpened(mock)).toHaveLength(1));
  });

  it('Befehlspalette und Kürzelübersicht nennen das Kürzel der Variante', async () => {
    const { user, mock } = renderApp({ isMac: false });
    await screen.findByRole('heading', { level: 1 });
    fireEvent.keyDown(window, { key: 'k', ctrlKey: true });
    const palette = await screen.findByRole('dialog', { name: 'Befehle' });
    const option = within(palette).getByRole('menuitem', { name: /Demo starten/ });
    expect(option).toHaveTextContent('Strg');
    expect(option).toHaveTextContent('D');
    await user.click(option);
    await waitFor(() => expect(demoOpened(mock)).toHaveLength(1));
  });

  it('Kürzelübersicht im Web: Alt+Umschalt+D', async () => {
    setWebProfile({ keystore: false });
    renderApp({ isMac: false });
    await screen.findByRole('heading', { level: 1 });
    fireEvent.keyDown(window, { key: '?' });
    const dialog = await screen.findByRole('dialog', { name: /Tastenkürzel/ });
    const row = within(dialog).getByText('Demo starten').closest('tr, li, div') as HTMLElement;
    expect(row).toHaveTextContent(/Alt.*Umschalt.*D/);
  });

  it('Mobil: Link „Mit Demodokument ausprobieren“ auf der Startseite', async () => {
    const { user, mock } = renderApp({ platform: 'android' });
    await user.click(await screen.findByRole('link', { name: 'Mit Demodokument ausprobieren' }));
    await waitFor(() => expect(demoOpened(mock)).toHaveLength(1));
    expect(demoOpened(mock)[0]).toMatchObject({ paths: [expect.stringContaining('/Library/Caches/demo/PIDA-Demo.pdf')] });
  });
});
