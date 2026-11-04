/**
 * Start und Zustandskarten (ADR 0004 §2.1, §2.11): Ablagefläche nur optisch (Hervorhebung beim Ziehen über
 * die Fenster-Ereignisse), Dateiauswahl, ⌘/Strg+O, Mobile-Variante, Slot „Zuletzt signiert“ leer, Laden,
 * kennwortgeschütztes PDF ohne Kennwortfeld mit „Anderes Dokument öffnen“, Fehlerkarte mit Details, axe.
 */
import { act, screen, waitFor, within } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { setWebProfile } from '../../core/web-profile';
import { defaultErrorContext } from '../../core/defaults';
import { expectNoAxeViolations } from '../../test/a11y';
import { renderApp } from '../../test/render';
import { editActions, IOS, onMobile, QUEUE, signersView, viewOf } from '../../test/views';

const START = () => viewOf({ step: 'open', queue: [] }, { actions: editActions({ sign: false, edit_placement: false }) });

describe('Start', () => {
  it('Titel, Ablagefläche, „PDF auswählen“ (Dateidialog mit Mehrfachauswahl), keine Seitenspalte ohne Inhalte', async () => {
    const { mock, user } = renderApp({ pickFilesResult: ['/Users/demo/a.pdf'] });
    expect(await screen.findByRole('heading', { level: 1, name: 'Was möchten Sie signieren?' })).toBeInTheDocument();
    expect(screen.getByRole('region', { name: 'Ablagefläche für PDF-Dokumente' })).toBeInTheDocument();
    // ohne „Zuletzt signiert“ (Mock ohne Beispiele, wie der Kern bis K3) keine Seitenspalte; kein Hinweis auf einen
    // Prüfmodus mehr (ADR 0004 Nachtrag 15)
    expect(screen.queryByRole('complementary')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Prüfmodus|prüfen/i })).not.toBeInTheDocument();
    expect(document.querySelector('.start-view')).toHaveClass('is-single');
    await user.click(screen.getByRole('button', { name: 'PDF auswählen' }));
    await waitFor(() => expect(mock.dispatched[0]).toEqual({ type: 'open_files', paths: ['/Users/demo/a.pdf'] }));
    expect(mock.nativeCalls).toEqual(['pickFiles:multiple']);
    await expectNoAxeViolations();
  });

  it('Hinweis zu mehreren Dateien je Standard-Signiermittel (Kern: `batch_flow`; Mobile-Check 16.10.2026)', async () => {
    const cases = [
      ['atrust_native', 'Mehrere Dateien werden als Stapel signiert – eine Anmeldung, jedes Dokument bestätigen Sie einzeln.'],
      ['keystore', 'Mehrere Dateien werden als Stapel signiert – der Keystore wird dafür einmal entsperrt.'],
      ['local_bku', 'Mehrere Dateien werden als Stapel signiert – jedes Dokument bestätigen Sie einzeln.'],
      [null, 'Mehrere Dateien werden als Stapel signiert.'],
    ] as const;
    for (const [backend, text] of cases) {
      const { unmount } = renderApp({ view: { ...START(), signers: signersView({ selected: backend, default: backend }) } });
      const zone = await screen.findByRole('region', { name: 'Ablagefläche für PDF-Dokumente' });
      expect(zone.querySelector('.drop-hint')?.textContent).toBe(text);
      unmount();
    }
  });

  it('Ziehen über dem Fenster hebt die Ablagefläche hervor, Ablegen öffnet (Pfade aus der Shell)', async () => {
    const { mock } = renderApp({ view: START() });
    const zone = await screen.findByRole('region', { name: 'Ablagefläche für PDF-Dokumente' });
    act(() => mock.simulateFileDrag({ type: 'enter', paths: ['/x.pdf'], x: 1, y: 1 }));
    expect(zone).toHaveAttribute('data-drop-target', 'true');
    expect(zone).toHaveTextContent('Loslassen zum Öffnen');
    act(() => mock.simulateFileDrag({ type: 'drop', paths: ['/x.pdf', '/y.txt'], x: 1, y: 1 }));
    // keine Filterung nach Endung – der Kern meldet Nicht-PDFs
    expect(mock.dispatched).toEqual([{ type: 'open_files', paths: ['/x.pdf', '/y.txt'] }]);
    expect(zone).not.toHaveAttribute('data-drop-target');
  });

  it('neue Version: Hinweis mit Link auf die Release-Seite und „Ausblenden“ (Backlog Nr. 33); Mobile ohne', async () => {
    const { mock, user } = renderApp({ update: 'available' });
    const hint = await screen.findByTestId('update-hint');
    expect(hint).toHaveAttribute('role', 'note');
    expect(hint).toHaveTextContent('PIDA 0.3.0 ist verfügbar.');
    await user.click(within(hint).getByRole('link', { name: 'Release-Seite von PIDA 0.3.0 auf GitHub öffnen' }));
    expect(mock.openedUrls).toEqual(['https://github.com/a-sit/pida/releases/tag/v0.3.0']);
    await expectNoAxeViolations();
    await user.click(within(hint).getByRole('button', { name: 'Hinweis für diese Version ausblenden' }));
    await waitFor(() => expect(screen.queryByTestId('update-hint')).not.toBeInTheDocument());
    expect(mock.dispatched).toContainEqual({ type: 'dismiss_update_hint' });
  });

  it('kein Hinweis ohne neue Version und auf Mobilgeräten', async () => {
    const { unmount } = renderApp({ update: 'up_to_date' });
    await screen.findByRole('heading', { level: 1 });
    expect(screen.queryByTestId('update-hint')).not.toBeInTheDocument();
    unmount();
    renderApp({ update: 'available', platform: 'ios' });
    await screen.findByRole('heading', { level: 1 });
    expect(screen.queryByTestId('update-hint')).not.toBeInTheDocument();
  });

  it('⌘/Strg+O öffnet die Dateiauswahl (nur mit Freigabe)', async () => {
    const { mock, user } = renderApp({ view: START() });
    await screen.findByRole('heading', { level: 1 });
    await user.keyboard('{Control>}o{/Control}');
    await waitFor(() => expect(mock.nativeCalls).toEqual(['pickFiles:multiple']));
  });

  it('Mobile: keine Ablage-Texte und Kürzel, „Dokument auswählen“ über das Plugin', async () => {
    const { mock, user } = renderApp({ platform: 'ios' });
    const button = await screen.findByRole('button', { name: 'Dokument auswählen' });
    expect(screen.queryByText('PDF hierher ziehen oder auswählen')).not.toBeInTheDocument();
    expect(screen.queryByText('öffnet die Dateiauswahl')).not.toBeInTheDocument();
    await user.click(button);
    await waitFor(() => expect(mock.dispatched[0]).toMatchObject({ type: 'open_files' }));
    expect(mock.nativeCalls).toEqual(['pickMobileDocuments:multiple']);
    await expectNoAxeViolations();
  });

  it('Laden mit Abbrechen', async () => {
    const { mock, user } = renderApp({ view: viewOf({ step: 'loading', queue: QUEUE }) });
    expect(await screen.findByRole('heading', { level: 1, name: 'Vertrag.pdf' })).toBeInTheDocument();
    expect(screen.getByRole('status')).toHaveTextContent('Das Dokument wird gelesen und geprüft');
    await user.click(screen.getByRole('button', { name: 'Abbrechen' }));
    expect(mock.dispatched).toEqual([{ type: 'cancel' }]);
    await expectNoAxeViolations();
  });
});

describe('Zustandskarten', () => {
  it('kennwortgeschütztes PDF: ohne Kennwortfeld, „Anderes Dokument öffnen“ = cancel + Dateiauswahl', async () => {
    const view = viewOf({ step: 'error', code: 'error-PDFPwdProtected', params: {}, retry: false, context: { ...defaultErrorContext(), origin: 'load' } });
    const { mock, user } = renderApp({ view, pickFilesResult: ['/Users/demo/b.pdf'] });
    expect(await screen.findByRole('heading', { level: 1, name: 'Dieses PDF ist mit einem Kennwort geschützt' })).toBeInTheDocument();
    expect(screen.getByRole('main').querySelector('input')).toBeNull();
    await expectNoAxeViolations();
    await user.click(screen.getByRole('button', { name: 'Anderes Dokument öffnen' }));
    await waitFor(() => expect(mock.dispatched).toEqual([{ type: 'cancel' }, { type: 'open_files', paths: ['/Users/demo/b.pdf'] }]));
  });

  it('Fehler beim Öffnen (kein PDF) mit Erneut versuchen, Schließen und technischen Details', async () => {
    const view = viewOf({ step: 'error', code: 'error-MayNotBeAPDF', params: {}, retry: true, context: { ...defaultErrorContext(), origin: 'load', occurred_at: '2026-10-15T10:42:17+02:00' } });
    const { mock, user } = renderApp({ view });
    expect(await screen.findByRole('heading', { level: 1, name: 'Diese Datei ist kein PDF' })).toBeInTheDocument();
    expect(screen.getByRole('alert')).toHaveTextContent('Dies ist möglicherweise keine PDF-Datei');
    await user.click(screen.getByRole('button', { name: 'Technische Details' }));
    expect(screen.getByRole('main')).toHaveTextContent('error-MayNotBeAPDF');
    await expectNoAxeViolations();
    await user.click(screen.getByRole('button', { name: 'Erneut versuchen' }));
    const card = screen.getByRole('region', { name: 'Diese Datei ist kein PDF' });
    await user.click(within(card).getByRole('button', { name: 'Schließen' }));
    expect(mock.dispatched).toEqual([{ type: 'retry' }, { type: 'cancel' }]);
  });

  it('Mobile: Startansicht im schmalen Layout ohne Verstöße', async () => {
    renderApp({ view: onMobile(START(), IOS) });
    await screen.findByRole('heading', { level: 1 });
    await expectNoAxeViolations();
  });
});

describe('Start: Impressum und Datenschutz (Web-Variante)', () => {
  afterEach(() => setWebProfile(null));

  it('Web: Fußzeile mit Impressum und Datenschutz der eigenen Website, je in neuem Tab', async () => {
    setWebProfile({ keystore: false });
    const { mock, user } = renderApp();
    const legal = await screen.findByRole('navigation', { name: 'Rechtliches' });
    await user.click(within(legal).getByRole('link', { name: 'Impressum' }));
    await user.click(within(legal).getByRole('link', { name: 'Datenschutz' }));
    expect(mock.openedUrls).toEqual([`${window.location.origin}/impressum/`, `${window.location.origin}/datenschutz/`]);
  });

  it('App: keine Fußzeile (Impressum unter Einstellungen › Über PIDA)', async () => {
    renderApp();
    await screen.findByRole('heading', { level: 1 });
    expect(screen.queryByRole('navigation', { name: 'Rechtliches' })).toBeNull();
  });
});
