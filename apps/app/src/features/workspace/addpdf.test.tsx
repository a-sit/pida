/**
 * „PDF hinzufügen“ (ADR 0004 Nachtrag 9): Knopf „+ PDF“ in der Kopfzeile mit Menü (Tastatur, axe), „Weitere PDFs
 * hinzufügen“ → Stapel mit der Platzierung als Plan, „Anderes PDF öffnen“ mit Rückfrage nur nach eigener
 * Platzierung (Standard „Abbrechen“), „+ PDF hinzufügen“ in der Miniaturleiste (Klick und Ablegen), Palette
 * („PDF hinzufügen“, „Zum Stapel“/„Zurück zum Stapel“), ⌘/Strg+B samt Kürzelübersicht, schmale Ansicht mit der
 * System-Dateiauswahl. Was daraus folgt, entscheidet der Kern – die Oberfläche sendet nur `open_files { mode }`.
 */
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import type { UiEvent } from '../../core/types';
import { expectNoAxeViolations } from '../../test/a11y';
import { emulateMedia, lastEvent, renderApp } from '../../test/render';
import { batchData, DOC, documentView, editActions, IOS, onMobile, workspaceView } from '../../test/views';

const DIR = '/Users/demo/Dokumente';

let restore: (() => void) | null = null;
afterEach(() => {
  restore?.();
  restore = null;
});

function header() {
  return screen.getByRole('banner');
}

/** Knopf „+ PDF“ in der Kopfzeile. */
async function addButton() {
  return within(header()).findByRole('button', { name: 'PDF hinzufügen oder öffnen' });
}

async function openMenu(user: ReturnType<typeof renderApp>['user']) {
  await user.click(await addButton());
  return screen.findByRole('menu', { name: 'PDF hinzufügen oder öffnen' });
}

const opened = (events: UiEvent[]) => events.filter((e) => e.type === 'open_files');
const sessions = (events: UiEvent[]) => events.filter((e) => e.type === 'sign_with' || e.type === 'start_batch' || e.type === 'choose_backend');

/** Selbst platziert (Kern: `detection.auto_placed == false`). */
const byHand = () => documentView({ origin: 'user', detection: { pdfa: null, pdfa_decision: 'none', auto_placed: false, qr_search: true } });

describe('Kopfzeile „+ PDF“', () => {
  it('neben dem Dokumentnamen, Menü per Tastatur bedienbar (Eingabe öffnet, Pfeile wählen, Esc schließt); axe', async () => {
    const { mock, user } = renderApp({ view: workspaceView() });
    const button = await addButton();
    expect(within(header()).getByText('Vertrag.pdf')).toBeInTheDocument();
    expect(button).toHaveTextContent('PDF');
    expect(button).toHaveAttribute('aria-haspopup', 'true');
    button.focus();
    await user.keyboard('{Enter}');
    const menu = await screen.findByRole('menu', { name: 'PDF hinzufügen oder öffnen' });
    const items = within(menu).getAllByRole('menuitem');
    expect(items.map((i) => i.textContent)).toEqual([
      'Weitere PDFs hinzufügen …Zusammen mit „Vertrag.pdf“ als Stapel signieren',
      'Anderes PDF öffnen …„Vertrag.pdf“ wird geschlossen',
    ]);
    expect(within(menu).getByRole('menuitem', { name: 'Weitere PDFs hinzufügen …' })).toHaveAccessibleDescription('Zusammen mit „Vertrag.pdf“ als Stapel signieren');
    await waitFor(() => expect(items[0]).toHaveFocus());
    await expectNoAxeViolations();
    await user.keyboard('{ArrowDown}');
    expect(items[1]).toHaveFocus();
    await user.keyboard('{Escape}');
    await waitFor(() => expect(screen.queryByRole('menu')).not.toBeInTheDocument());
    await waitFor(() => expect(button).toHaveFocus());
    // Eingabe auf „Weitere PDFs hinzufügen“: Dateiauswahl mit Mehrfachauswahl → `open_files { mode: 'add' }`
    await user.keyboard('{Enter}');
    await screen.findByRole('menu');
    await user.keyboard('{Enter}');
    await waitFor(() => expect(lastEvent(mock)).toEqual({ type: 'open_files', paths: [`${DIR}/Vertrag.pdf`, `${DIR}/Angebot 2026.pdf`], mode: 'add' }));
    expect(mock.nativeCalls).toContain('pickFiles:multiple');
    expect(sessions(mock.dispatched)).toEqual([]);
  });

  it('nur beim Bearbeiten und mit `actions.open_files`; nicht beim Signieren, nicht ohne Dokument', async () => {
    const { unmount } = renderApp({ view: workspaceView(documentView(), { actions: editActions({ open_files: false }) }) });
    expect(await addButton()).toBeDisabled();
    unmount();
    const signing = renderApp({ view: workspaceView(documentView({ phase: 'signing' })) });
    await within(header()).findByText('Vertrag.pdf');
    expect(within(header()).queryByRole('button', { name: 'PDF hinzufügen oder öffnen' })).not.toBeInTheDocument();
    signing.unmount();
    renderApp();
    await screen.findByRole('heading', { level: 1, name: 'Was möchten Sie signieren?' });
    expect(within(header()).queryByRole('button', { name: 'PDF hinzufügen oder öffnen' })).not.toBeInTheDocument();
  });

  it('Mock-Kern: „Weitere PDFs hinzufügen“ – alle zusammen werden zum Stapel, die eigene Platzierung bleibt der Plan', async () => {
    const { mock, user } = renderApp({ scenario: 'contract', pickFilesResult: [`${DIR}/Angebot.pdf`, `${DIR}/Auftrag.pdf`] });
    const sticker = await screen.findByRole('application', { name: 'Signaturblock' });
    fireEvent.keyDown(sticker, { key: 'ArrowUp' });
    await waitFor(() => expect((async () => (await mock.view()).document?.origin)()).resolves.toBe('user'));
    const menu = await openMenu(user);
    await user.click(within(menu).getByRole('menuitem', { name: 'Weitere PDFs hinzufügen …' }));
    expect(await screen.findByRole('heading', { level: 1, name: '3 Dokumente' })).toBeInTheDocument();
    const list = screen.getByRole('list', { name: 'Dokumente im Stapel' });
    const row = within(list)
      .getAllByRole('listitem', { hidden: true })
      .find((r) => within(r).queryByText('Vertrag.pdf'));
    expect(row).toBeDefined();
    expect(within(row as HTMLElement).getAllByText(/selbst platziert/).length).toBeGreaterThan(0);
    expect(within(list).getByText('Angebot.pdf')).toBeInTheDocument();
    expect(within(list).getByText('Auftrag.pdf')).toBeInTheDocument();
    expect(opened(mock.dispatched)).toEqual([{ type: 'open_files', paths: [`${DIR}/Angebot.pdf`, `${DIR}/Auftrag.pdf`], mode: 'add' }]);
    expect(sessions(mock.dispatched)).toEqual([]);
    await expectNoAxeViolations();
  });

  it('„Anderes PDF öffnen“ ohne eigene Platzierung: ohne Rückfrage eine Datei wählen, das gewählte PDF öffnet', async () => {
    const { mock, user } = renderApp({ scenario: 'contract', pickFilesResult: [`${DIR}/Angebot.pdf`] });
    await screen.findByRole('application', { name: 'Signaturblock' });
    const menu = await openMenu(user);
    await user.click(within(menu).getByRole('menuitem', { name: 'Anderes PDF öffnen …' }));
    await waitFor(() => expect(within(header()).getByText('Angebot.pdf')).toBeInTheDocument());
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();
    expect(mock.nativeCalls).toContain('pickFiles:single');
    expect(opened(mock.dispatched)).toEqual([{ type: 'open_files', paths: [`${DIR}/Angebot.pdf`], mode: 'replace' }]);
    const v = await mock.view();
    expect(v.batch).toBeNull();
    expect(v.done.map((d) => [d.name, d.result.kind])).toEqual([['Vertrag.pdf', 'cancelled']]);
    expect(await screen.findByRole('application', { name: 'Signaturblock' })).toBeInTheDocument();
  });

  it('„Anderes PDF öffnen“ nach eigener Platzierung: Rückfrage mit „Abbrechen“ als Standard; Escape/Abbrechen ändern nichts', async () => {
    const { mock, user } = renderApp({ view: workspaceView(byHand()) });
    let menu = await openMenu(user);
    await user.click(within(menu).getByRole('menuitem', { name: 'Anderes PDF öffnen …' }));
    let ask = await screen.findByRole('alertdialog', { name: 'Anderes PDF öffnen?' });
    expect(ask).toHaveTextContent('Sie haben die Signatur in „Vertrag.pdf“ selbst platziert.');
    expect(within(ask).getByRole('button', { name: 'Abbrechen' })).toHaveFocus();
    await expectNoAxeViolations();
    await user.keyboard('{Escape}');
    await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument());
    menu = await openMenu(user);
    await user.click(within(menu).getByRole('menuitem', { name: 'Anderes PDF öffnen …' }));
    ask = await screen.findByRole('alertdialog', { name: 'Anderes PDF öffnen?' });
    // Eingabe auf dem Standardknopf bricht ab
    await user.keyboard('{Enter}');
    await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument());
    expect(opened(mock.dispatched)).toEqual([]);
    expect(mock.nativeCalls.filter((c) => c.startsWith('pickFiles'))).toEqual([]);
    // bestätigt: erst dann die Dateiauswahl (eine Datei) und `open_files { mode: 'replace' }`
    menu = await openMenu(user);
    await user.click(within(menu).getByRole('menuitem', { name: 'Anderes PDF öffnen …' }));
    ask = await screen.findByRole('alertdialog', { name: 'Anderes PDF öffnen?' });
    await user.click(within(ask).getByRole('button', { name: 'Anderes PDF öffnen' }));
    await waitFor(() => expect(lastEvent(mock)).toEqual({ type: 'open_files', paths: [`${DIR}/Vertrag.pdf`], mode: 'replace' }));
    expect(mock.nativeCalls).toContain('pickFiles:single');
  });

  it('abgebrochene Dateiauswahl sendet nichts', async () => {
    const { mock, user } = renderApp({ view: workspaceView(), pickFilesResult: null });
    const menu = await openMenu(user);
    await user.click(within(menu).getByRole('menuitem', { name: 'Weitere PDFs hinzufügen …' }));
    await waitFor(() => expect(mock.nativeCalls).toContain('pickFiles:multiple'));
    expect(opened(mock.dispatched)).toEqual([]);
  });
});

describe('Miniaturleiste „+ PDF hinzufügen“', () => {
  it('Klick wie „Weitere PDFs hinzufügen“; axe', async () => {
    const { mock, user } = renderApp({ view: workspaceView() });
    const nav = await screen.findByRole('navigation', { name: 'Seiten' });
    const add = within(nav).getByRole('button', { name: 'PDF hinzufügen' });
    expect(within(nav).getByText('oder hierher ziehen')).toBeInTheDocument();
    await expectNoAxeViolations();
    await user.click(add);
    await waitFor(() => expect(lastEvent(mock)).toEqual({ type: 'open_files', paths: [`${DIR}/Vertrag.pdf`, `${DIR}/Angebot 2026.pdf`], mode: 'add' }));
    expect(mock.nativeCalls).toContain('pickFiles:multiple');
  });

  it('Ablegen auf „+ PDF hinzufügen“ fügt hinzu (`mode: add`), anderswo wie bisher ohne `mode`', async () => {
    const { mock } = renderApp({ view: workspaceView() });
    const nav = await screen.findByRole('navigation', { name: 'Seiten' });
    const add = within(nav).getByRole('button', { name: 'PDF hinzufügen' });
    const original = document.elementFromPoint;
    try {
      document.elementFromPoint = ((x: number) => (x < 100 ? add : document.body)) as typeof document.elementFromPoint;
      mock.simulateFileDrag({ type: 'enter', paths: ['/x/a.pdf'], x: 50, y: 700 });
      await waitFor(() => expect(document.querySelector('.app')).toHaveAttribute('data-drag'));
      mock.simulateFileDrag({ type: 'drop', paths: ['/x/a.pdf', '/x/b.pdf'], x: 50, y: 700 });
      await waitFor(() => expect(lastEvent(mock)).toEqual({ type: 'open_files', paths: ['/x/a.pdf', '/x/b.pdf'], mode: 'add' }));
      mock.simulateFileDrag({ type: 'drop', paths: ['/x/c.pdf'], x: 500, y: 300 });
      await waitFor(() => expect(lastEvent(mock)).toEqual({ type: 'open_files', paths: ['/x/c.pdf'] }));
    } finally {
      document.elementFromPoint = original;
    }
  });

  it('Mock-Kern: abgelegt auf der Leiste → Stapelübersicht', async () => {
    const { mock } = renderApp({ scenario: 'contract' });
    const nav = await screen.findByRole('navigation', { name: 'Seiten' });
    const add = within(nav).getByRole('button', { name: 'PDF hinzufügen' });
    const original = document.elementFromPoint;
    try {
      document.elementFromPoint = (() => add) as typeof document.elementFromPoint;
      mock.simulateFileDrag({ type: 'drop', paths: [`${DIR}/Angebot.pdf`], x: 40, y: 600 });
      expect(await screen.findByRole('heading', { level: 1, name: '2 Dokumente' })).toBeInTheDocument();
    } finally {
      document.elementFromPoint = original;
    }
  });
});

describe('Stapel per Palette und ⌘/Strg+B', () => {
  const openPalette = async () => {
    fireEvent.keyDown(window, { key: 'k', ctrlKey: true });
    return screen.findByRole('dialog', { name: 'Befehle' });
  };

  it('Palette: „PDF hinzufügen …“ und „Zum Stapel“ (Strg+B) mit wartenden Dokumenten', async () => {
    const view = workspaceView(documentView(), { actions: editActions({ show_batch: true }) });
    const { mock, user } = renderApp({ view });
    await screen.findByRole('application', { name: 'Signaturblock' });
    let palette = await openPalette();
    const batch = within(palette).getByRole('menuitem', { name: /^Zum Stapel/ });
    expect(batch).toHaveTextContent('StrgB');
    await user.click(batch);
    await waitFor(() => expect(lastEvent(mock)).toEqual({ type: 'show_batch' }));
    palette = await openPalette();
    await user.click(within(palette).getByRole('menuitem', { name: /^PDF hinzufügen …/ }));
    await waitFor(() => expect(lastEvent(mock)).toMatchObject({ type: 'open_files', mode: 'add' }));
  });

  it('Palette: aus dem Stapel geöffnet „Zurück zum Stapel“; ohne Stapel und ohne wartende Dokumente kein Stapelbefehl', async () => {
    const view = workspaceView(documentView(), { batch: batchData({ open_item: DOC.id }) });
    const { mock, user, unmount } = renderApp({ view, isMac: true });
    await screen.findByRole('application', { name: 'Signaturblock' });
    fireEvent.keyDown(window, { key: 'k', metaKey: true });
    const palette = await screen.findByRole('dialog', { name: 'Befehle' });
    const back = within(palette).getByRole('menuitem', { name: /^Zurück zum Stapel/ });
    expect(back).toHaveTextContent('⌘B');
    expect(within(palette).queryByRole('menuitem', { name: /^Zum Stapel/ })).not.toBeInTheDocument();
    await user.click(back);
    await waitFor(() => expect(lastEvent(mock)).toEqual({ type: 'back_to_batch' }));
    unmount();
    renderApp({ view: workspaceView() });
    await screen.findByRole('application', { name: 'Signaturblock' });
    const plain = await openPalette();
    expect(within(plain).queryByRole('menuitem', { name: /Stapel/ })).not.toBeInTheDocument();
    expect(within(plain).getByRole('menuitem', { name: /^PDF hinzufügen …/ })).toBeInTheDocument();
  });

  it('⌘/Strg+B: „Zum Stapel“ bzw. „Zurück zum Stapel“, sonst ohne Wirkung; nicht über Dialogen', async () => {
    const show = renderApp({ view: workspaceView(documentView(), { actions: editActions({ show_batch: true }) }) });
    await screen.findByRole('application', { name: 'Signaturblock' });
    fireEvent.keyDown(window, { key: 'b', ctrlKey: true });
    expect(lastEvent(show.mock)).toEqual({ type: 'show_batch' });
    // über einer Rückfrage wirkt das Kürzel nicht
    fireEvent.keyDown(window, { key: 'k', ctrlKey: true });
    await screen.findByRole('dialog', { name: 'Befehle' });
    const before = show.mock.dispatched.length;
    fireEvent.keyDown(window, { key: 'b', ctrlKey: true });
    expect(show.mock.dispatched.length).toBe(before);
    show.unmount();

    const back = renderApp({ view: workspaceView(documentView(), { batch: batchData({ open_item: DOC.id }) }), isMac: true });
    await screen.findByRole('application', { name: 'Signaturblock' });
    fireEvent.keyDown(window, { key: 'b', metaKey: true });
    expect(lastEvent(back.mock)).toEqual({ type: 'back_to_batch' });
    back.unmount();

    const none = renderApp({ view: workspaceView() });
    await screen.findByRole('application', { name: 'Signaturblock' });
    fireEvent.keyDown(window, { key: 'b', ctrlKey: true });
    expect(none.mock.dispatched.filter((e) => e.type === 'show_batch' || e.type === 'back_to_batch')).toEqual([]);
  });

  it('Kürzelübersicht nennt „Zum Stapel“ mit Strg+B', async () => {
    renderApp({ view: workspaceView() });
    await screen.findByRole('application', { name: 'Signaturblock' });
    fireEvent.keyDown(window, { key: '?' });
    const dialog = await screen.findByRole('dialog', { name: 'Tastenkürzel' });
    const row = [...dialog.querySelectorAll<HTMLElement>('.shortcuts-row')].find((r) => r.dataset.shortcut === 'batch');
    expect(row?.querySelector('dt')).toHaveTextContent('Zum Stapel');
    expect([...(row?.querySelectorAll('.kbd') ?? [])].map((k) => k.textContent)).toEqual(['Strg', 'B']);
  });
});

describe('schmale Ansicht und Mobilgeräte', () => {
  it('derselbe Knopf in der Kopfzeile öffnet die System-Dateiauswahl (mehrere bzw. eine Datei); axe', async () => {
    restore = emulateMedia({ narrow: true, coarse: true });
    const { mock, user } = renderApp({ view: onMobile(workspaceView(), IOS), platform: 'ios' });
    await screen.findByRole('application', { name: 'Signaturblock' });
    let menu = await openMenu(user);
    await expectNoAxeViolations();
    await user.click(within(menu).getByRole('menuitem', { name: 'Weitere PDFs hinzufügen …' }));
    await waitFor(() => expect(lastEvent(mock)).toMatchObject({ type: 'open_files', mode: 'add' }));
    expect(mock.nativeCalls).toContain('pickMobileDocuments:multiple');
    menu = await openMenu(user);
    await user.click(within(menu).getByRole('menuitem', { name: 'Anderes PDF öffnen …' }));
    await waitFor(() => expect(lastEvent(mock)).toMatchObject({ type: 'open_files', mode: 'replace' }));
    expect(mock.nativeCalls).toContain('pickMobileDocuments:single');
    const sent = lastEvent(mock) as Extract<UiEvent, { type: 'open_files' }>;
    expect(sent.paths.every((p) => p.includes('/Library/Caches/import/'))).toBe(true);
    // Miniaturen sind schmal eingeklappt – die Kopfzeile bleibt der Weg
    expect(screen.queryByRole('navigation', { name: 'Seiten' })).not.toBeInTheDocument();
  });
});
