/**
 * Arbeitsbereich (ADR 0004 §2.3/§2.4, §4.3, Abnahme E1 Punkte 3–5): Sticker (Tastatur 15/1/50 pt über den
 * Kern, Ziehen mit Einrasten an den Einrastlinien, Feld-Magnet, Doppelklick, Touch), Live-Region,
 * Überdeckungswarnung über „Signieren“, Signaturfelder, Optionen mit Begründung, Konto-Chip, ARIA, axe;
 * Hinweise auf Erkanntes (Einstellungen › Dokument, E3: PDF/A, QR-Platzhalter, Signaturfeld) samt sofortiger
 * Wirkung der Einstellungen (Mock wie K3).
 */
import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { DocumentView, UiEvent } from '../../core/types';
import { expectNoAxeViolations } from '../../test/a11y';
import { emulateMedia, lastEvent, renderApp } from '../../test/render';
import { defaultSettings } from '../../core/defaults';
import { documentView, fieldTarget, IOS, onMobile, settingsView, signersView, viewOf, workspaceView } from '../../test/views';
import { layoutPages, PAGE_GAP, PAGE_PAD } from './placement';

const SCALE = 1.15; // eingepasster Maßstab ohne ResizeObserver (jsdom)
const PH = 841.89;

function sticker() {
  return screen.getByRole('application', { name: 'Signaturblock' });
}

/** Seitenspalte und Sticker ohne Layout: Rechtecke für die Zeigerrechnung setzen. */
function stubRects() {
  const host = document.querySelector('.pages') as HTMLElement;
  host.getBoundingClientRect = () => ({ left: 0, top: 0, right: 0, bottom: 0, width: 0, height: 0, x: 0, y: 0, toJSON: () => ({}) });
  const s = sticker();
  s.getBoundingClientRect = () => ({ left: 0, top: 0, right: 0, bottom: 0, width: 0, height: 0, x: 0, y: 0, toJSON: () => ({}) });
  return { host, s };
}

/** Zeigerposition (px in der Seitenspalte) für eine Lage in pt auf Seite `page` mit Greifpunkt in der Blockmitte. */
function px(page: number, x: number, yTop: number) {
  const top = PAGE_PAD + page * (PH * SCALE + PAGE_GAP);
  return { clientX: (x + 115) * SCALE, clientY: top + (PH - yTop + 41) * SCALE };
}

let restore: (() => void) | null = null;
afterEach(() => {
  restore?.();
  restore = null;
});

describe('Sticker und Seiten', () => {
  it('zeigt Kacheln, Sticker mit Blockvorschau im Rechteck des Kerns, Hilfe und keine Zahlenfelder', async () => {
    renderApp({ view: workspaceView() });
    const s = await screen.findByRole('application', { name: 'Signaturblock' });
    expect(s).toHaveAttribute('aria-roledescription', 'verschiebbarer Signaturblock');
    expect(s).toHaveAttribute('tabindex', '0');
    expect(s).toHaveAccessibleDescription(/Pfeiltasten verschieben, Umschalt\+Pfeil verschiebt fein, Alt\+Pfeil grob/);
    // Rechteck des Kerns (PAGE_ABS) → px: links 182,64 pt, Oberkante 300 pt von unten
    const top = layoutPages([[595.28, PH], [595.28, PH], [595.28, PH]], SCALE).tops[2] ?? 0;
    expect(s.style.left).toBe(`${182.64 * SCALE}px`);
    expect(parseFloat(s.style.top)).toBeCloseTo(top + (PH - 300) * SCALE, 3);
    expect(s.querySelector('img')?.getAttribute('src')).toMatch(/^data:image\/svg\+xml/);
    expect(screen.getByRole('img', { name: 'Seite 3 von 3 – Vertrag.pdf' })).toBeInTheDocument();
    expect(screen.queryByRole('spinbutton')).not.toBeInTheDocument();
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
    // Lage im Inspektor
    expect(screen.getByText('Seite 3 · unten mittig')).toBeInTheDocument();
  });

  it('Pfeiltasten: nudge_block (Schritte rechnet der Kern: 15/1/50 pt), Bild↑/↓ und Pos1/Ende wechseln die Seite', async () => {
    const { mock } = renderApp({ view: workspaceView() });
    const s = await screen.findByRole('application', { name: 'Signaturblock' });
    s.focus();
    fireEvent.keyDown(s, { key: 'ArrowRight' });
    fireEvent.keyDown(s, { key: 'ArrowUp', shiftKey: true });
    fireEvent.keyDown(s, { key: 'ArrowLeft', altKey: true });
    fireEvent.keyDown(s, { key: 'ArrowDown', ctrlKey: true });
    fireEvent.keyDown(s, { key: 'PageUp' });
    fireEvent.keyDown(s, { key: 'Home' });
    fireEvent.keyDown(s, { key: 'End' });
    const sent = mock.dispatched.filter((e) => e.type !== 'inspect_pages');
    expect(sent).toEqual([
      { type: 'nudge_block', dx: 1, dy: 0, fine: false, coarse: false },
      { type: 'nudge_block', dx: 0, dy: 1, fine: true, coarse: false },
      { type: 'nudge_block', dx: -1, dy: 0, fine: false, coarse: true },
      { type: 'nudge_block', dx: 0, dy: -1, fine: false, coarse: true },
      { type: 'change_page', page: 1 },
      { type: 'change_page', page: 0 },
    ]);
  });

  it('Ziehen: Einrasten an den Einrastlinien (rechte Kante, unter dem Text), Bänder freier Flächen, move_block beim Loslassen', async () => {
    const { mock } = renderApp({ view: workspaceView() });
    await screen.findByRole('application', { name: 'Signaturblock' });
    const { s } = stubRects();
    fireEvent.pointerDown(s, { button: 0, pointerId: 1, pointerType: 'mouse', clientX: 0, clientY: 0 });
    fireEvent.pointerMove(s, { pointerId: 1, pointerType: 'mouse', ...px(2, 311, 324) });
    // während des Ziehens: Einrastlinien und freie Flächen aus `layout`
    expect(document.querySelectorAll('.snap-guide')).toHaveLength(2);
    expect(document.querySelectorAll('.free-band')).toHaveLength(1);
    fireEvent.pointerUp(s, { pointerId: 1, pointerType: 'mouse', ...px(2, 311, 324) });
    const move = mock.dispatched.find((e) => e.type === 'move_block') as Extract<UiEvent, { type: 'move_block' }>;
    expect(move.page).toBe(2);
    expect(move.x).toBeCloseTo(309);
    expect(move.y).toBeCloseTo(322);
    expect(document.querySelectorAll('.snap-guide')).toHaveLength(0);
  });

  it('Ziehen über Seitengrenzen und ohne Layoutdaten (kein Einrasten)', async () => {
    const { mock } = renderApp({ view: workspaceView(documentView({ layout: [] })) });
    await screen.findByRole('application', { name: 'Signaturblock' });
    const { s } = stubRects();
    fireEvent.pointerDown(s, { button: 0, pointerId: 1, clientX: 0, clientY: 0 });
    fireEvent.pointerMove(s, { pointerId: 1, ...px(0, 100.5, 700.5) });
    expect(document.querySelectorAll('.snap-guide')).toHaveLength(0);
    fireEvent.pointerUp(s, { pointerId: 1, ...px(0, 100.5, 700.5) });
    const move = mock.dispatched.find((e) => e.type === 'move_block') as Extract<UiEvent, { type: 'move_block' }>;
    expect(move.page).toBe(0);
    expect(move.x).toBeCloseTo(100.5);
    expect(move.y).toBeCloseTo(700.5);
  });

  it('ein zweiter Finger verschiebt nicht; kurzes Antippen ohne Bewegung sendet nichts', async () => {
    const { mock } = renderApp({ view: onMobile(workspaceView()) });
    await screen.findByRole('application', { name: 'Signaturblock' });
    const { s } = stubRects();
    fireEvent.pointerDown(s, { button: 0, pointerId: 1, pointerType: 'touch', clientX: 10, clientY: 10 });
    fireEvent.pointerDown(s, { button: 0, pointerId: 2, pointerType: 'touch', clientX: 400, clientY: 400 });
    fireEvent.pointerMove(s, { pointerId: 2, pointerType: 'touch', clientX: 500, clientY: 500 });
    fireEvent.pointerUp(s, { pointerId: 2, pointerType: 'touch', clientX: 500, clientY: 500 });
    fireEvent.pointerUp(s, { pointerId: 1, pointerType: 'touch', clientX: 10, clientY: 10 });
    expect(mock.dispatched.filter((e) => e.type === 'move_block' || e.type === 'use_target')).toEqual([]);
  });

  it('Doppelklick setzt den Block mittig an den Zeiger; Doppeltippen über den Finger (Fingerversatz)', async () => {
    const { mock } = renderApp({ view: workspaceView() });
    await screen.findByRole('application', { name: 'Signaturblock' });
    stubRects();
    const page1 = screen.getByRole('img', { name: 'Seite 1 von 3 – Vertrag.pdf' });
    // Zeiger bei (300 pt, 400 pt von oben) auf Seite 1
    const at = { clientX: 300 * SCALE, clientY: PAGE_PAD + 400 * SCALE };
    fireEvent.doubleClick(page1, at);
    const move = mock.dispatched.find((e) => e.type === 'move_block') as Extract<UiEvent, { type: 'move_block' }>;
    expect(move.page).toBe(0);
    expect(move.x).toBeCloseTo(300 - 115);
    expect(move.y).toBeCloseTo(PH - (400 - 41));
    fireEvent.pointerUp(page1, { pointerType: 'touch', ...at });
    fireEvent.pointerUp(page1, { pointerType: 'touch', ...at });
    const tap = mock.dispatched.filter((e) => e.type === 'move_block')[1] as Extract<UiEvent, { type: 'move_block' }>;
    // Unterkante 12 pt über dem Finger
    expect(tap.y).toBeCloseTo(PH - (400 - 82 - 12));
  });

  it('Live-Region nach einer Änderung des Kerns: Lage der linken oberen Ecke „von unten“, Einrasten, Prüfergebnis', async () => {
    const { mock } = renderApp({ view: workspaceView() });
    const s = await screen.findByRole('application', { name: 'Signaturblock' });
    stubRects();
    fireEvent.pointerDown(s, { button: 0, pointerId: 1, clientX: 0, clientY: 0 });
    fireEvent.pointerMove(s, { pointerId: 1, ...px(2, 311, 324) });
    fireEvent.pointerUp(s, { pointerId: 1, ...px(2, 311, 324) });
    const next = documentView({
      revision: 2,
      placement: { page_index: 2, x: 309, y: 322, width: 230, height: 82 },
      sticker: { left: 309, right: 539, top: 322, bottom: 240 },
      check: { for_revision: 2, overlaps_content: false, overlaps_widget: false, clamped: false, placeholder_uncovered: false, overlaps_block: false },
      announce: { page: '3', pages: '3', left: '109', top: '114', bottom: '85' },
    });
    act(() => mock.setView(workspaceView(next)));
    await waitFor(() => expect(screen.getByTestId('live-region')).toHaveTextContent('Seite 3 von 3, linke obere Ecke 109 mm von links, 114 mm von unten. Rastet ein: rechts unten. Freie Fläche.'));
  });
});

describe('Überdeckung, Felder, Optionen', () => {
  const overlap = (extra: Partial<DocumentView> = {}) => documentView({ check: { for_revision: 1, overlaps_content: true, overlaps_widget: false, clamped: false, placeholder_uncovered: false, overlaps_block: false }, ...extra });

  it('Überdeckung: Rahmen und Warnung direkt über „Signieren“ mit „Freie Fläche suchen“ → auto_place', async () => {
    const { mock, user } = renderApp({ view: workspaceView(overlap()) });
    const s = await screen.findByRole('application', { name: 'Signaturblock' });
    expect(s).toHaveClass('is-overlap');
    const warn = screen.getByTestId('overlap-warning');
    expect(warn).toHaveTextContent('Der Signaturblock überdeckt Inhalt der Seite.');
    // Reihenfolge: Warnung steht vor dem Fuß mit „Signieren“
    const footer = document.querySelector('.inspector-footer') as HTMLElement;
    expect(warn.compareDocumentPosition(footer) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    await user.click(within(warn).getByRole('button', { name: 'Freie Fläche suchen' }));
    expect(lastEvent(mock)).toEqual({ type: 'auto_place' });
  });

  it('Überdeckung einer älteren Revision wird nicht gezeigt', async () => {
    renderApp({ view: workspaceView(overlap({ revision: 2 })) });
    await screen.findByRole('application', { name: 'Signaturblock' });
    expect(screen.queryByTestId('overlap-warning')).not.toBeInTheDocument();
  });

  it('eingeklappter Bottom-Sheet: Konto-Chip, „Signieren“ und die Warnung bleiben sichtbar', async () => {
    restore = emulateMedia({ narrow: true });
    const { user } = renderApp({ view: onMobile(workspaceView(overlap()), IOS) });
    await screen.findByRole('application', { name: 'Signaturblock' });
    const sheet = screen.getByRole('complementary', { name: 'Signatur' });
    expect(within(sheet).getByRole('button', { name: /Optionen einblenden/ })).toHaveAttribute('aria-expanded', 'false');
    expect(within(sheet).getByTestId('signer-chip')).toBeVisible();
    expect(within(sheet).getByRole('button', { name: /^Signieren/ })).toBeInTheDocument();
    expect(within(sheet).getByTestId('overlap-warning')).toBeInTheDocument();
    expect(within(sheet).queryByRole('switch', { name: 'Unsichtbar signieren' })).not.toBeInTheDocument();
    await user.click(within(sheet).getByRole('button', { name: /Optionen einblenden/ }));
    expect(within(sheet).getByRole('switch', { name: 'Unsichtbar signieren' })).toBeInTheDocument();
    await expectNoAxeViolations();
  });

  it('leeres Signaturfeld: „Hier signieren“-Markierung und Hinweisleiste → use_target (auch ohne Platzhaltersuche)', async () => {
    const doc = documentView({ fields: [fieldTarget({ index: 0, name: 'Unterschrift_Antragsteller', page: 1, rect: { left: 299, right: 539, top: 300, bottom: 214 }, selectable: true })] });
    const { mock, user } = renderApp({ view: workspaceView(doc) });
    await user.click(await screen.findByRole('button', { name: 'Hier signieren: Signaturfeld „Unterschrift_Antragsteller“' }));
    expect(lastEvent(mock)).toEqual({ type: 'use_target', target: { kind: 'field', index: 0 } });
    await user.click(screen.getByRole('button', { name: 'Ins Feld setzen' }));
    expect(lastEvent(mock)).toEqual({ type: 'use_target', target: { kind: 'field', index: 0 } });
  });

  it('Ziehen ins Feld rastet im Feld ein → use_target', async () => {
    const doc = documentView({ fields: [fieldTarget({ index: 0, name: 'Feld', page: 2, rect: { left: 300, right: 540, top: 200, bottom: 114 }, selectable: true })] });
    const { mock } = renderApp({ view: workspaceView(doc) });
    await screen.findByRole('application', { name: 'Signaturblock' });
    const { s } = stubRects();
    fireEvent.pointerDown(s, { button: 0, pointerId: 1, clientX: 0, clientY: 0 });
    // Blockmitte bei (420, 157) liegt im Feld
    fireEvent.pointerMove(s, { pointerId: 1, ...px(2, 420 - 115, 157 + 41) });
    expect(s).toHaveClass('is-in-field');
    fireEvent.pointerUp(s, { pointerId: 1, ...px(2, 420 - 115, 157 + 41) });
    expect(mock.dispatched.filter((e) => e.type === 'use_target')).toEqual([{ type: 'use_target', target: { kind: 'field', index: 0 } }]);
  });

  it.each([
    { label: 'Desktop, Maus', narrow: false, pointerType: 'mouse' },
    { label: 'schmal, Touch (iOS)', narrow: true, pointerType: 'touch' },
  ])('unplausible Felder (seitengroß, 3 pt schmal): keine Markierung, kein Einrasten beim Ziehen, Hinweisleiste nennt das passende Feld – $label (Nachtrag 16)', async ({ narrow, pointerType }) => {
    if (narrow) restore = emulateMedia({ narrow: true });
    const page = fieldTarget({ index: 0, name: 'Seite', page: 2, rect: { left: 0, right: 595.28, top: 841.89, bottom: 0 }, selectable: true });
    const sliver = fieldTarget({ index: 1, name: 'Schmal', page: 2, rect: { left: 350, right: 353, top: 581, bottom: 357 }, selectable: true });
    const ok = fieldTarget({ index: 2, name: 'Unterschrift', page: 0, rect: { left: 36, right: 183, top: 766, bottom: 694 }, selectable: true });
    expect([page.suitable, sliver.suitable, ok.suitable]).toEqual([false, false, true]);
    const view = workspaceView(documentView({ fields: [page, sliver, ok] }));
    const { mock } = renderApp({ view: narrow ? onMobile(view, IOS) : view });
    await screen.findByRole('application', { name: 'Signaturblock' });
    expect(screen.getAllByRole('button', { name: /^Hier signieren/ }).map((b) => b.getAttribute('aria-label'))).toEqual(['Hier signieren: Signaturfeld „Unterschrift“']);
    expect(screen.getByText(/„Unterschrift“ auf Seite 1/)).toBeInTheDocument();
    const { s } = stubRects();
    fireEvent.pointerDown(s, { button: 0, pointerId: 1, pointerType, clientX: 0, clientY: 0 });
    // Blockmitte mitten im seitengroßen Feld auf Seite 3: kein Einrasten, Block bleibt so groß wie er ist
    fireEvent.pointerMove(s, { pointerId: 1, pointerType, ...px(2, 150, 500) });
    expect(s).not.toHaveClass('is-in-field');
    expect(parseFloat(s.style.width)).toBeCloseTo(230 * SCALE, 3);
    expect(parseFloat(s.style.height)).toBeCloseTo(82 * SCALE, 3);
    expect(screen.queryByTestId('field-frame')).not.toBeInTheDocument();
    fireEvent.pointerUp(s, { pointerId: 1, pointerType, ...px(2, 150, 500) });
    expect(mock.dispatched.filter((e) => e.type === 'use_target')).toEqual([]);
    const move = mock.dispatched.find((e) => e.type === 'move_block') as Extract<UiEvent, { type: 'move_block' }>;
    expect([move.page, move.x, move.y]).toEqual([2, expect.closeTo(150, 3), expect.closeTo(500, 3)]);
  });

  it.each([
    { label: 'Desktop, Maus', narrow: false, pointerType: 'mouse' },
    { label: 'schmal, Touch (iOS)', narrow: true, pointerType: 'touch' },
  ])('Ziehen über ein passendes Feld: Block proportional und mittig im Feld (`block`), Rahmen des Felds, nicht gestreckt – $label', async ({ narrow, pointerType }) => {
    if (narrow) restore = emulateMedia({ narrow: true });
    // 400 × 200 pt: der Block bleibt 230 × 82 pt groß und steht mittig
    const big = fieldTarget({ index: 0, name: 'Groß', page: 2, rect: { left: 100, right: 500, top: 400, bottom: 200 }, selectable: true });
    expect(big.suitable).toBe(true);
    expect(big.block).toEqual({ left: 185, right: 415, bottom: 259, top: 341 });
    const view = workspaceView(documentView({ fields: [big] }));
    const { mock } = renderApp({ view: narrow ? onMobile(view, IOS) : view });
    await screen.findByRole('application', { name: 'Signaturblock' });
    const { s } = stubRects();
    fireEvent.pointerDown(s, { button: 0, pointerId: 1, pointerType, clientX: 0, clientY: 0 });
    fireEvent.pointerMove(s, { pointerId: 1, pointerType, ...px(2, 150, 330) });
    expect(s).toHaveClass('is-in-field');
    expect(parseFloat(s.style.left)).toBeCloseTo(185 * SCALE, 3);
    expect(parseFloat(s.style.width)).toBeCloseTo(230 * SCALE, 3);
    expect(parseFloat(s.style.height)).toBeCloseTo(82 * SCALE, 3);
    const frame = screen.getByTestId('field-frame');
    expect(parseFloat(frame.style.width)).toBeCloseTo(400 * SCALE, 3);
    expect(document.querySelector('.field-marker.is-hit')).not.toBeNull();
    fireEvent.pointerUp(s, { pointerId: 1, pointerType, ...px(2, 150, 330) });
    expect(mock.dispatched.filter((e) => e.type === 'use_target')).toEqual([{ type: 'use_target', target: { kind: 'field', index: 0 } }]);
  });

  it('im Feld: Vorschaubild in der Darstellungsgröße neu angefordert (verkleinert, nie hochskaliert)', async () => {
    const svgWidth = () => {
      const src = decodeURIComponent(sticker().querySelector('img')?.getAttribute('src') ?? '');
      return Number(/width="(\d+)"/.exec(src)?.[1]);
    };
    // frei: 230 pt × 1,15 px/pt → Maßstab 1,25 (aufgerundet) → 288 px
    const { mock } = renderApp({ view: workspaceView() });
    await screen.findByRole('application', { name: 'Signaturblock' });
    expect(svgWidth()).toBe(288);
    // im halb so großen Feld: 0,575 px/pt → 0,75 → 173 px (dargestellt 132 px, nicht hochskaliert)
    const small = fieldTarget({ index: 0, name: 'Klein', page: 2, rect: { left: 300, right: 415, top: 700, bottom: 659 }, selectable: true });
    expect(small.block).toEqual({ left: 300, right: 415, bottom: 659, top: 700 });
    act(() => mock.setView(workspaceView(documentView({ target: { kind: 'field', index: 0 }, origin: 'field', fields: [small], sticker: small.block }))));
    await waitFor(() => expect(svgWidth()).toBe(173));
    expect(parseFloat(sticker().style.width)).toBeCloseTo(115 * SCALE, 3);
    expect(svgWidth()).toBeGreaterThanOrEqual(115 * SCALE);
    // QR-Platzhalter (Sticker = QR-Bild 80 × 80 pt, nicht proportional): keine Achse hochskaliert – maßgeblich ist
    // die Höhe (80/82), also wieder 1,25 → 288 px
    const qr = { page: 2, id: '1', rect: { left: 300, right: 380, top: 700, bottom: 620 } };
    act(() => mock.setView(workspaceView(documentView({ target: { kind: 'qr' }, origin: 'qr', qr, sticker: qr.rect }))));
    await waitFor(() => expect(svgWidth()).toBe(288));
  });

  it('im Feld: Plakette „Im Signaturfeld“, Lage „Im Signaturfeld …“ und „Frei platzieren“', async () => {
    const doc = documentView({
      target: { kind: 'field', index: 0 },
      origin: 'field',
      fields: [fieldTarget({ index: 0, name: 'Feld', page: 2, rect: { left: 300, right: 540, top: 200, bottom: 114 }, selectable: true })],
      // Nachtrag 16: Lage des Blocks im Feld (proportional, mittig)
      sticker: { left: 305, right: 535, top: 198, bottom: 116 },
    });
    const { mock, user } = renderApp({ view: workspaceView(doc) });
    expect(await screen.findByText('Im Signaturfeld „Feld“')).toBeInTheDocument();
    expect(sticker()).toHaveClass('is-in-field');
    expect(screen.queryByRole('button', { name: /Hier signieren/ })).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Frei platzieren' }));
    expect(lastEvent(mock)).toEqual({ type: 'use_target', target: { kind: 'free' } });
  });

  it('Hinweisleiste erscheint bzw. verschwindet (Block aus dem Feld gezogen bzw. ins Feld gesetzt): Bildlauf um ihre Höhe ausgeglichen', async () => {
    const field = fieldTarget({ index: 0, name: 'Feld', page: 2, rect: { left: 300, right: 540, top: 200, bottom: 114 }, selectable: true });
    const inField = documentView({ target: { kind: 'field', index: 0 }, origin: 'field', fields: [field], sticker: field.block });
    const free = documentView({ fields: [field] });
    // jsdom ohne Layout: die Leiste ist 132 px hoch (Telefon, drei Zeilen und Knopf)
    const original = Element.prototype.getBoundingClientRect;
    Element.prototype.getBoundingClientRect = function (this: Element) {
      const h = this.classList.contains('canvas-banner') ? 132 : 0;
      return { left: 0, top: 0, right: 0, bottom: h, width: 0, height: h, x: 0, y: 0, toJSON: () => ({}) } as DOMRect;
    };
    try {
      const { mock } = renderApp({ view: workspaceView(inField) });
      expect(await screen.findByText('Im Signaturfeld „Feld“')).toBeInTheDocument();
      const scroller = document.querySelector('.canvas-scroll') as HTMLElement;
      scroller.scrollTop = 40;
      act(() => mock.setView(workspaceView(free)));
      expect(await screen.findByText('Signaturfeld gefunden:')).toBeInTheDocument();
      expect(scroller.scrollTop).toBe(40 + 132);
      act(() => mock.setView(workspaceView(inField)));
      await waitFor(() => expect(screen.queryByText('Signaturfeld gefunden:')).not.toBeInTheDocument());
      expect(scroller.scrollTop).toBe(40);
    } finally {
      Element.prototype.getBoundingClientRect = original;
    }
  });

  it('QR-Platzhalter: automatisch positioniert (Hinweisleiste)', async () => {
    const doc = documentView({ target: { kind: 'qr' }, origin: 'qr', qr: { page: 2, id: '1', rect: { left: 309, right: 539, top: 300, bottom: 218 } } });
    renderApp({ view: workspaceView(doc) });
    expect(await screen.findByText('Positioniert im QR-Code-Platzhalter des Dokuments.')).toBeInTheDocument();
    expect(screen.getByText('Im QR-Code-Platzhalter')).toBeInTheDocument();
  });

  it('Amtssignatur ohne eigene Bildmarke (Backlog Nr. 10): Hinweis unter der Vorschau, „Bildmarke wählen“ öffnet die Einstellungen', async () => {
    const base = documentView();
    const { mock, user } = renderApp({ view: workspaceView({ ...base, block: { ...base.block, emblem_placeholder: true } }) });
    const hint = await screen.findByTestId('emblem-placeholder-hint');
    expect(hint).toHaveTextContent('Für die Amtssignatur fehlt die Bildmarke Ihrer Behörde – der Signaturblock zeigt einen Platzhalter.');
    await expectNoAxeViolations();
    await user.click(within(hint).getByRole('button', { name: 'Bildmarke wählen' }));
    expect(lastEvent(mock)).toEqual({ type: 'open_settings' });
  });

  it('Platzhalter-Bildmarke ohne Wahlmöglichkeit: QR-Platzhalter gibt das Profil vor bzw. Mobile – Hinweis ohne „Bildmarke wählen“', async () => {
    const base = documentView();
    const qr = renderApp({ view: workspaceView({ ...base, block: { ...base.block, emblem_placeholder: true, emblem_choosable: false } }) });
    const hint = await screen.findByTestId('emblem-placeholder-hint');
    expect(hint).toHaveTextContent('Der QR-Platzhalter gibt die Amtssignatur vor; sie zeigt einen Platzhalter statt der Bildmarke Ihrer Behörde.');
    expect(within(hint).queryByRole('button')).not.toBeInTheDocument();
    qr.unmount();
    renderApp({ view: onMobile(workspaceView({ ...base, block: { ...base.block, emblem_placeholder: true } }), IOS) });
    const mobile = await screen.findByTestId('emblem-placeholder-hint');
    expect(mobile).toHaveTextContent('In der App lässt sich keine eigene Bildmarke festlegen.');
    expect(within(mobile).queryByRole('button')).not.toBeInTheDocument();
  });

  it('ohne Platzhalter-Bildmarke kein Hinweis', async () => {
    renderApp({ view: workspaceView(documentView()) });
    await screen.findByRole('application', { name: 'Signaturblock' });
    expect(screen.queryByTestId('emblem-placeholder-hint')).not.toBeInTheDocument();
  });

  it('QR-Platzhalter nicht ganz bedeckt (Backlog Nr. 32): Inline-Hinweis und Ansage, nur zur aktuellen Revision und sichtbar', async () => {
    const qr = { page: 2, id: '1', rect: { left: 309, right: 539, top: 300, bottom: 118 } };
    const base = documentView({ target: { kind: 'qr' }, origin: 'qr', qr });
    const check = { ...base.check, for_revision: base.revision, placeholder_uncovered: true };
    // veraltete Prüfung (andere Revision): noch kein Hinweis
    const { mock } = renderApp({ view: workspaceView({ ...base, check: { ...check, for_revision: base.revision - 1 } }) });
    await screen.findByRole('application', { name: 'Signaturblock' });
    expect(screen.queryByTestId('qr-uncovered-hint')).not.toBeInTheDocument();
    act(() => mock.setView(workspaceView({ ...base, check })));
    expect(await screen.findByTestId('qr-uncovered-hint')).toHaveTextContent(
      'Der Signaturblock bedeckt den QR-Platzhalter nicht ganz – ein Teil des Platzhalterbilds bleibt sichtbar.',
    );
    await waitFor(() => expect(screen.getByTestId('live-region')).toHaveTextContent('Der QR-Platzhalter wird nicht ganz bedeckt.'));
    await expectNoAxeViolations();
    act(() => mock.setView(workspaceView({ ...base, invisible: true, check })));
    await waitFor(() => expect(screen.queryByTestId('qr-uncovered-hint')).not.toBeInTheDocument());
  });

  it('Optionen: „Unsichtbar“ als Schalter (set_invisible), „Auf neuer Seite“ (toggle_new_page)', async () => {
    const { mock, user } = renderApp({ view: workspaceView() });
    await user.click(await screen.findByRole('switch', { name: 'Unsichtbar signieren' }));
    expect(lastEvent(mock)).toEqual({ type: 'set_invisible', invisible: true });
    await user.click(screen.getByRole('switch', { name: 'Auf neuer Seite' }));
    expect(lastEvent(mock)).toEqual({ type: 'toggle_new_page' });
  });

  it('Optionen: Sprache des Signaturblocks DE/EN (set_block_language), gesperrt wenn unsichtbar', async () => {
    const { mock, user, unmount } = renderApp({ view: workspaceView() });
    const group = await screen.findByRole('radiogroup', { name: 'Sprache des Signaturblocks' });
    expect(within(group).getByRole('radio', { name: 'DE' })).toBeChecked();
    expect(screen.getByText('Gilt auch für weitere Dokumente (Einstellungen › Signaturbild).')).toBeInTheDocument();
    await user.click(within(group).getByRole('radio', { name: 'EN' }));
    expect(lastEvent(mock)).toEqual({ type: 'set_block_language', language: 'en' });
    await expectNoAxeViolations();
    unmount();
    renderApp({ view: workspaceView(documentView({ invisible: true, sticker: null })) });
    const locked = await screen.findByRole('radiogroup', { name: 'Sprache des Signaturblocks' });
    expect(within(locked).getByRole('radio', { name: 'EN' })).toBeDisabled();
  });

  it('Mock-Kern: DE/EN ändert die Einstellung und den Block', async () => {
    const { user } = renderApp({ pickFilesResult: ['/Users/demo/Vertrag.pdf'] });
    const pick = await screen.findByRole('button', { name: 'PDF auswählen' });
    await waitFor(() => expect(pick).toBeEnabled(), { timeout: 3000 });
    await user.click(pick);
    const group = await screen.findByRole('radiogroup', { name: 'Sprache des Signaturblocks' }, { timeout: 5000 });
    const before = within(group).getByRole('radio', { name: 'DE' });
    expect(before).toBeChecked();
    await user.click(within(group).getByRole('radio', { name: 'EN' }));
    await waitFor(() => expect(within(screen.getByRole('radiogroup', { name: 'Sprache des Signaturblocks' })).getByRole('radio', { name: 'EN' })).toBeChecked());
  });

  it('„Auf neuer Seite“ bei signierten bzw. zertifizierten Dokumenten gesperrt mit Begründung des Kerns', async () => {
    const doc = documentView({
      doc: { ...documentView().doc, signed: true },
      constraints: { new_page_allowed: false, visible_allowed: true, new_visible_field_allowed: true, reasons: ['ng-constraint-signed'] },
      existing: [{ field_name: 'Signature1', signer: 'Dr. Anna Gruber', signing_time: '2026-10-12T14:03:51+02:00', status: 'valid', page: 1, rect: null }],
    });
    renderApp({ view: workspaceView(doc) });
    const sw = await screen.findByRole('switch', { name: 'Auf neuer Seite' });
    expect(sw).toBeDisabled();
    expect(sw).toHaveAccessibleDescription('Das Dokument ist bereits signiert; eine neue Seite würde die vorhandenen Signaturen verändern.');
    // vorhandene Signaturen ohne „gültig“ (§8 F3)
    const list = screen.getByRole('heading', { name: 'Vorhandene Signaturen' }).closest('section') as HTMLElement;
    expect(list).toHaveTextContent('Dr. Anna Gruber');
    expect(list).toHaveTextContent('intakt');
    expect(list).not.toHaveTextContent(/gültig/);
  });

  it('unsichtbares Profil: Schalter fest an mit Begründung; kein Sticker, Hinweisleiste', async () => {
    const doc = documentView({
      invisible: true,
      sticker: null,
      constraints: { new_page_allowed: true, visible_allowed: false, new_visible_field_allowed: true, reasons: ['ng-constraint-invisible-profile'] },
    });
    renderApp({ view: workspaceView(doc) });
    const sw = await screen.findByRole('switch', { name: 'Unsichtbar signieren' });
    expect(sw).toBeChecked();
    expect(sw).toBeDisabled();
    expect(sw).toHaveAccessibleDescription(/unsichtbar \(Einstellungen › Signaturbild\)/);
    expect(screen.queryByRole('application')).not.toBeInTheDocument();
    expect(screen.getByText('Unsichtbare Signatur – auf den Seiten erscheint kein Signaturbild.')).toBeInTheDocument();
  });

  it('Analysewarnung: Seiten sichtbar, Inspektor mit „Trotzdem signieren“/„Dokument verwerfen“', async () => {
    const doc = documentView({ phase: 'warning' });
    const view = viewOf({ step: 'warning', doc: doc.doc, codes: ['pdf-usage-rights'] }, { document: doc, signers: signersView() });
    const { mock, user } = renderApp({ view });
    expect(await screen.findByRole('alert')).toHaveTextContent('erweiterte Adobe-Reader-Rechte');
    expect(sticker()).toHaveAttribute('tabindex', '-1');
    expect(screen.queryByRole('button', { name: /^Signieren/ })).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Trotzdem signieren' }));
    expect(lastEvent(mock)).toEqual({ type: 'answer_warning', proceed: true });
  });
});

describe('Konto-Chip und „Signieren“', () => {
  it('„Signieren“ mit gewähltem Signiermittel → sign_with (keine Kombination confirm_position/choose_backend)', async () => {
    const { mock, user } = renderApp({ view: workspaceView() });
    await user.click(await screen.findByRole('button', { name: /^Signieren/ }));
    expect(mock.dispatched.filter((e) => e.type !== 'inspect_pages')).toEqual([{ type: 'sign_with', backend: 'atrust_native' }]);
  });

  it('Kurzhinweis A-Trust am Konto-Chip (ID Austria), nicht beim Keystore', async () => {
    const { unmount } = renderApp({ view: workspaceView() });
    expect(await screen.findByTestId('privacy-note')).toHaveTextContent('Das Dokument wird zum Signieren an A-Trust übermittelt.');
    unmount();
    renderApp({ view: workspaceView(documentView(), { signers: signersView({ selected: 'keystore' }) }) });
    await screen.findByRole('application', { name: 'Signaturblock' });
    expect(screen.queryByTestId('privacy-note')).not.toBeInTheDocument();
  });

  it('Wechsel im Menü → select_signer (startet nichts); „Als Standard verwenden“ → make_default', async () => {
    const { mock, user } = renderApp({ view: workspaceView() });
    await user.click(await screen.findByRole('button', { name: 'Signiermittel: ID Austria · +43 660 ···123. Wechseln' }));
    const menu = await screen.findByRole('menu');
    expect(within(menu).getByRole('menuitem', { name: /Keystore · max\.p12 · max/ })).toBeInTheDocument();
    expect(within(menu).getByRole('menuitem', { name: /Chipkarte/ })).toBeInTheDocument();
    await user.click(within(menu).getByRole('menuitem', { name: /Keystore/ }));
    expect(lastEvent(mock)).toEqual({ type: 'select_signer', backend: 'keystore', make_default: false });
    expect(mock.dispatched.some((e) => e.type === 'sign_with')).toBe(false);
    act(() => mock.setView(workspaceView(documentView(), { signers: signersView({ selected: 'keystore' }) })));
    await user.click(screen.getByRole('button', { name: /Signiermittel: Keystore/ }));
    await user.click(await screen.findByRole('menuitem', { name: 'Als Standard verwenden' }));
    expect(lastEvent(mock)).toEqual({ type: 'select_signer', backend: 'keystore', make_default: true });
  });

  it('„Einstellungen …“ im Menü öffnet die Einstellungen beim Abschnitt Signiermittel', async () => {
    const { mock, user } = renderApp({ view: workspaceView() });
    await user.click(await screen.findByTestId('signer-chip'));
    const menu = await screen.findByRole('menu');
    expect(within(menu).queryByRole('menuitem', { name: /Konten verwalten/ })).not.toBeInTheDocument();
    await user.click(within(menu).getByRole('menuitem', { name: 'Einstellungen …' }));
    expect(lastEvent(mock)).toEqual({ type: 'open_settings' });
    act(() => mock.setView({ ...workspaceView(), settings: settingsView().settings }));
    // Einstellungen (B3): Abschnitt „Signiermittel“ mit dem ID-Austria-Konto
    const dialog = await screen.findByRole('dialog', { name: 'Einstellungen' });
    expect(within(dialog).getByRole('heading', { level: 3, name: 'Signiermittel' })).toBeInTheDocument();
    expect(within(dialog).getByRole('heading', { name: 'ID Austria' })).toBeInTheDocument();
  });

  it('ohne Auswahl („Jedes Mal fragen“): „Signieren“ öffnet das Menü, die Wahl dort signiert (sign_with)', async () => {
    const view = workspaceView(documentView(), { signers: signersView({ selected: null, default: null }) });
    const { mock, user } = renderApp({ view });
    expect(await screen.findByRole('button', { name: 'Signiermittel: Signiermittel wählen. Wechseln' })).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: /^Signieren/ }));
    const menu = await screen.findByRole('menu');
    expect(menu).toHaveTextContent('Signieren mit');
    expect(mock.dispatched.some((e) => e.type === 'sign_with')).toBe(false);
    await user.click(within(menu).getByRole('menuitem', { name: /ID Austria/ }));
    expect(lastEvent(mock)).toEqual({ type: 'sign_with', backend: 'atrust_native' });
  });

  it('nicht verfügbares Signiermittel: im Menü gesperrt mit Grund des Kerns', async () => {
    const s = signersView();
    const options = s.options.map((o) => (o.backend === 'local_bku' ? { ...o, available: false, capabilities: null, unavailable_reason: 'ng-signer-localbku-not-running' } : o));
    const { user } = renderApp({ view: workspaceView(documentView(), { signers: { ...s, options } }) });
    await user.click(await screen.findByTestId('signer-chip'));
    const item = await screen.findByRole('menuitem', { name: /Chipkarte/ });
    expect(item).toHaveAttribute('aria-disabled', 'true');
    expect(item).toHaveTextContent('Die Chipkarten-Software (lokale BKU) läuft nicht.');
  });

  it('⌘/Strg+↵ signiert (ausdrückliche Aktion), nicht ohne Freigabe', async () => {
    const { mock, user, unmount } = renderApp({ view: workspaceView() });
    await screen.findByRole('application', { name: 'Signaturblock' });
    await user.keyboard('{Control>}{Enter}{/Control}');
    expect(mock.dispatched.filter((e) => e.type === 'sign_with')).toEqual([{ type: 'sign_with', backend: 'atrust_native' }]);
    unmount();
    const r = renderApp({ view: workspaceView(documentView(), { actions: { ...workspaceView().actions, sign: false } }) });
    await screen.findByRole('application', { name: 'Signaturblock' });
    await r.user.keyboard('{Control>}{Enter}{/Control}');
    expect(r.mock.dispatched.filter((e) => e.type === 'sign_with')).toEqual([]);
    expect(screen.getByRole('button', { name: /^Signieren/ })).toBeDisabled();
  });

  it('„Bearbeiten“ am Signaturbild öffnet die Einstellungen (Abschnitt Signaturbild)', async () => {
    const { mock, user } = renderApp({ view: workspaceView() });
    await user.click(await screen.findByRole('button', { name: 'Signaturbild bearbeiten' }));
    expect(lastEvent(mock)).toEqual({ type: 'open_settings' });
  });
});

describe('Seitenansicht, Zoom und Barrierefreiheit', () => {
  it('Werkzeugleiste: Zoom-Stufen, An Breite anpassen, Seitenleiste; ⌘/Strg+Plus zoomt', async () => {
    const { user } = renderApp({ view: workspaceView() });
    await screen.findByRole('application', { name: 'Signaturblock' });
    const toolbar = screen.getByRole('toolbar', { name: 'Ansicht' });
    expect(within(toolbar).getByText('Zoom 86 %')).toBeInTheDocument();
    await user.click(within(toolbar).getByRole('button', { name: 'Vergrößern' }));
    expect(within(toolbar).getByText('Zoom 90 %')).toBeInTheDocument();
    await user.keyboard('{Control>}={/Control}');
    expect(within(toolbar).getByText('Zoom 100 %')).toBeInTheDocument();
    await user.click(within(toolbar).getByRole('button', { name: 'An Breite anpassen' }));
    expect(within(toolbar).getByText('Zoom 86 %')).toBeInTheDocument();
    expect(screen.getByRole('navigation', { name: 'Seiten' })).toBeInTheDocument();
    await user.click(within(toolbar).getByRole('button', { name: 'Seitenleiste ein-/ausblenden' }));
    expect(screen.queryByRole('navigation', { name: 'Seiten' })).not.toBeInTheDocument();
  });

  it('schmal: nach der Wahl einer Miniatur Fokus auf dem Umschalter – sichtbar nur nach Tastatur, ohne Tooltip nach Zeiger', async () => {
    // Mobile-Check 16.10.2026 (R10): Fokusrahmen und Tooltip des Umschalters verdeckten nach einem Tippen die Wortmarke
    restore = emulateMedia({ narrow: true });
    const { user } = renderApp({ view: workspaceView() });
    await screen.findByRole('application', { name: 'Signaturblock' });
    const toggle = () => within(screen.getByRole('toolbar', { name: 'Ansicht' })).getByRole('button', { name: 'Seitenleiste ein-/ausblenden' });
    const thumb = (n: number) => within(screen.getByRole('navigation', { name: 'Seiten' })).getByRole('button', { name: new RegExp(`^Seite ${n}`) });
    // `focusVisible: false` unterdrückt auch das `:focus-visible` des Browsers bei Fokus per Skript (WebKit)
    const focus = vi.spyOn(HTMLElement.prototype, 'focus');
    const focusedQuietly = () => focus.mock.contexts.some((el, i) => el === toggle() && (focus.mock.calls[i]?.[0] as FocusOptions | undefined)?.focusVisible === false);
    const quiet = () => {
      expect(screen.queryByRole('navigation', { name: 'Seiten' })).not.toBeInTheDocument();
      expect(toggle()).toHaveFocus();
      expect(toggle()).not.toHaveAttribute('data-focus-visible');
      expect(screen.queryByRole('tooltip')).not.toBeInTheDocument();
      expect(focusedQuietly()).toBe(true);
      focus.mockClear();
    };
    try {
      // Maus bzw. Finger
      await user.click(toggle());
      await user.click(thumb(2));
      quiet();
      // Tippen ohne Kontaktfläche bzw. Bildschirmleser („virtuell“, `detail = 0`)
      await user.click(toggle());
      fireEvent.click(thumb(1));
      quiet();
      // Tastatur: der Fokus bleibt sichtbar
      await user.click(toggle());
      thumb(3).focus();
      focus.mockClear();
      await user.keyboard('{Enter}');
      expect(toggle()).toHaveFocus();
      expect(toggle()).toHaveAttribute('data-focus-visible');
      expect(focusedQuietly()).toBe(false);
    } finally {
      focus.mockRestore();
    }
  });

  it('Live-Region: nach dem Wechsel zu einem anderen Dokument nicht mehr die Lage des vorigen', async () => {
    // Mobile-Check 16.10.2026 (Android): nach dem Öffnen eines anderen Stapeldokuments stand dort zunächst noch die Lage
    // des vorigen
    const { mock } = renderApp({ view: workspaceView() });
    await screen.findByRole('application', { name: 'Signaturblock' });
    const moved = documentView({
      revision: 2,
      placement: { page_index: 2, x: 309, y: 322, width: 230, height: 82 },
      sticker: { left: 309, right: 539, top: 322, bottom: 240 },
      check: { for_revision: 2, overlaps_content: false, overlaps_widget: false, clamped: false, placeholder_uncovered: false, overlaps_block: false },
      announce: { page: '3', pages: '3', left: '109', top: '114', bottom: '85' },
    });
    act(() => mock.setView(workspaceView(moved)));
    const live = screen.getByTestId('live-region');
    await waitFor(() => expect(live).toHaveTextContent(/^Seite 3 von 3, linke obere Ecke 109 mm von links/));
    const other = documentView({ doc: { ...moved.doc, id: 8, name: 'Angebot.pdf' }, revision: 1 });
    act(() => mock.setView(workspaceView(other)));
    await screen.findByRole('img', { name: 'Seite 3 von 3 – Angebot.pdf' });
    expect(live).toBeEmptyDOMElement();
    // auch eine noch ausstehende Ansage des vorigen landet nicht mehr
    await act(() => new Promise((r) => setTimeout(r, 350)));
    expect(live).toBeEmptyDOMElement();
  });

  it('sichtbare Seiten gehen als inspect_pages an den Kern', async () => {
    const { mock } = renderApp({ view: workspaceView() });
    await screen.findByRole('application', { name: 'Signaturblock' });
    await waitFor(() => expect(mock.dispatched.some((e) => e.type === 'inspect_pages')).toBe(true));
    const ev = mock.dispatched.find((e) => e.type === 'inspect_pages') as Extract<UiEvent, { type: 'inspect_pages' }>;
    expect(ev.pages.length).toBeGreaterThan(0);
    expect(ev.pages.length).toBeLessThanOrEqual(8);
  });

  it('axe: Arbeitsbereich, Felder, Überdeckung, Englisch', async () => {
    const doc = documentView({
      fields: [fieldTarget({ index: 0, name: 'Feld', page: 1, rect: { left: 300, right: 540, top: 200, bottom: 114 }, selectable: true })],
      check: { for_revision: 1, overlaps_content: true, overlaps_widget: false, clamped: false, placeholder_uncovered: false, overlaps_block: false },
    });
    const { unmount } = renderApp({ view: workspaceView(doc) });
    await screen.findByRole('application', { name: 'Signaturblock' });
    await expectNoAxeViolations();
    unmount();
    renderApp({ view: workspaceView(documentView(), {}, 'en') });
    expect(await screen.findByRole('application', { name: 'Signature block' })).toBeInTheDocument();
    await expectNoAxeViolations();
  });
});

describe('Erkennung beim Öffnen: PDF/A und Platzhalter (Einstellungen › Dokument, E3)', () => {
  const inspector = () => screen.getByRole('complementary', { name: 'Signatur' });
  const FIELD = 'Unterschrift_Antragsteller';
  const withSearch = (kind: 'marker' | 'fields', search = true) => ({ ...defaultSettings(), placeholder: { search, kind } });

  async function documentSettings(user: ReturnType<typeof renderApp>['user']) {
    await user.click(screen.getByRole('button', { name: 'Einstellungen' }));
    const dialog = await screen.findByRole('dialog', { name: 'Einstellungen' });
    await user.click(within(within(dialog).getByRole('navigation', { name: 'Bereiche' })).getByRole('button', { name: 'Dokument' }));
    return dialog;
  }

  async function closeSettings(user: ReturnType<typeof renderApp>['user']) {
    await user.keyboard('{Escape}');
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
  }

  it('nennt ein erkanntes PDF/A-Dokument bzw. die erzwungene PDF/A-Option (sofort wirksam)', async () => {
    const { unmount } = renderApp({ scenario: 'contract' });
    await screen.findByRole('application', { name: 'Signaturblock' });
    expect(within(inspector()).getByText('PDF/A-Dokument erkannt – wird PDF/A-konform signiert')).toBeInTheDocument();
    expect(within(inspector()).queryByText(/erzwungen/)).not.toBeInTheDocument();
    unmount();

    const r = renderApp({ scenario: 'blank' });
    await screen.findByRole('application', { name: 'Signaturblock' });
    expect(within(inspector()).queryByText(/PDF\/A/)).not.toBeInTheDocument();
    const dialog = await documentSettings(r.user);
    await r.user.click(within(dialog).getByRole('switch', { name: 'PDF/A-kompatibel signieren' }));
    await closeSettings(r.user);
    expect(within(inspector()).getByText('PDF/A-kompatible Signatur (erzwungen)')).toBeInTheDocument();
  });

  it('„Leere Signaturfelder“: Signatur beim Öffnen im ersten leeren Feld, Hinweis und Sprechblase nennen das Ziel', async () => {
    const { user } = renderApp({ scenario: 'field', settings: withSearch('fields') });
    await screen.findByRole('application', { name: 'Signaturblock' });
    expect(within(inspector()).getByText(`Im Signaturfeld „${FIELD}“`)).toBeInTheDocument();
    expect(within(inspector()).getByText('Signaturfeld gefunden – Signatur im Feld platziert')).toBeInTheDocument();
    expect(sticker()).toHaveClass('is-in-field');
    expect(screen.getByText('Signaturblock automatisch im Signaturfeld platziert')).toBeInTheDocument();
    // Feld belegt: keine Hinweisleiste, keine „Hier signieren“-Markierung
    expect(screen.queryByText('Signaturfeld gefunden:')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Hier signieren/ })).not.toBeInTheDocument();
    await expectNoAxeViolations();

    // „Frei platzieren“: der Hinweis entfällt, das Feld wird wieder angeboten
    await user.click(within(inspector()).getByRole('button', { name: 'Frei platzieren' }));
    await waitFor(() => expect(within(inspector()).queryByText(/Signaturfeld gefunden/)).not.toBeInTheDocument());
    expect(screen.getByRole('button', { name: `Hier signieren: Signaturfeld „${FIELD}“` })).toBeInTheDocument();
    // von Hand ins Feld: Lage genannt, aber nicht als automatisch erkannt
    await user.click(screen.getByRole('button', { name: 'Ins Feld setzen' }));
    expect(await within(inspector()).findByText(`Im Signaturfeld „${FIELD}“`)).toBeInTheDocument();
    expect(within(inspector()).queryByTestId('detect-note')).not.toBeInTheDocument();
  });

  it('Umschalten in den Einstellungen wirkt sofort; ohne Suche wird das Feld nur angeboten', async () => {
    const { user } = renderApp({ scenario: 'field' });
    await screen.findByRole('application', { name: 'Signaturblock' });
    // Standard (Suche nach QR-Platzhaltern): das Signaturfeld wird nur angeboten
    expect(screen.getByText('Signaturfeld gefunden:')).toBeInTheDocument();
    expect(within(inspector()).getByText(/^Seite \d · /)).toBeInTheDocument();
    expect(within(inspector()).queryByTestId('detect-note')).not.toBeInTheDocument();

    let dialog = await documentSettings(user);
    await user.click(within(dialog).getByRole('radio', { name: /Leere Signaturfelder/ }));
    await closeSettings(user);
    expect(await within(inspector()).findByText('Signaturfeld gefunden – Signatur im Feld platziert')).toBeInTheDocument();
    expect(screen.getByText('Signaturblock automatisch im Signaturfeld platziert')).toBeInTheDocument();
    expect(screen.queryByText('Signaturfeld gefunden:')).not.toBeInTheDocument();

    dialog = await documentSettings(user);
    await user.click(within(dialog).getByRole('switch', { name: 'Signaturplatzhalter automatisch suchen' }));
    await closeSettings(user);
    await waitFor(() => expect(within(inspector()).queryByText(/Signaturfeld gefunden/)).not.toBeInTheDocument());
    expect(within(inspector()).getByText(/^Seite \d · /)).toBeInTheDocument();
    // Markierung und Hinweisleiste bleiben als Angebot
    expect(screen.getByText('Signaturfeld gefunden:')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: `Hier signieren: Signaturfeld „${FIELD}“` })).toBeInTheDocument();
  });

  it('von Hand platziert bleibt beim Umschalten der Platzhaltersuche liegen', async () => {
    const { user, mock } = renderApp({ scenario: 'field' });
    const s = await screen.findByRole('application', { name: 'Signaturblock' });
    s.focus();
    await user.keyboard('{ArrowLeft}');
    await waitFor(() => expect(mock.dispatched.some((e) => e.type === 'nudge_block')).toBe(true));
    const before = (await mock.view()).document?.placement;
    const dialog = await documentSettings(user);
    await user.click(within(dialog).getByRole('radio', { name: /Leere Signaturfelder/ }));
    await closeSettings(user);
    const after = (await mock.view()).document;
    expect(after?.target).toEqual({ kind: 'free' });
    expect(after?.placement).toEqual(before);
    expect(within(inspector()).queryByTestId('detect-note')).not.toBeInTheDocument();
    expect(screen.getByText('Signaturfeld gefunden:')).toBeInTheDocument();
  });

  it('QR-Code-Platzhalter: automatisch genutzt, nicht bei „Leere Signaturfelder“ oder ausgeschalteter Suche', async () => {
    const open = async () => {
      const list = await screen.findByRole('list', { name: 'Dokumente im Stapel' });
      await waitFor(() => expect(within(list).queryAllByText('Wird geprüft')).toHaveLength(0));
      await userOf.current!.click(screen.getByRole('button', { name: '„Auftragsbestaetigung_4711.pdf“ öffnen und positionieren' }));
      await screen.findByRole('application', { name: 'Signaturblock' });
    };
    const userOf: { current: ReturnType<typeof renderApp>['user'] | null } = { current: null };
    let r = renderApp({ scenario: 'batch', settings: withSearch('marker') });
    userOf.current = r.user;
    await open();
    expect(within(inspector()).getByText('Im QR-Code-Platzhalter')).toBeInTheDocument();
    expect(within(inspector()).getByText('QR-Platzhalter gefunden – Signatur dort platziert')).toBeInTheDocument();
    expect(screen.getByText('Signaturblock automatisch im QR-Platzhalter platziert')).toBeInTheDocument();
    r.unmount();

    for (const settings of [withSearch('fields'), withSearch('marker', false)]) {
      r = renderApp({ scenario: 'batch', settings });
      userOf.current = r.user;
      await open();
      expect(within(inspector()).queryByText(/QR-Platzhalter gefunden/)).not.toBeInTheDocument();
      expect(screen.getByText('Signaturblock automatisch auf freier Fläche platziert')).toBeInTheDocument();
      r.unmount();
    }
  });
});

describe('Automatische Positionierung (Einstellung, ADR 0004 Nachtrag 8)', () => {
  const inspector = () => screen.getByRole('complementary', { name: 'Signatur' });
  const FIELD = fieldTarget({ index: 0, name: 'Unterschrift_Antragsteller', page: 1, rect: { left: 299, right: 539, top: 300, bottom: 214 }, selectable: true });
  const lockedDoc = (extra: Partial<DocumentView> = {}) =>
    documentView({ placement_locked: true, detection: { pdfa: null, pdfa_decision: 'none', auto_placed: true }, ...extra });
  const SESSION: UiEvent['type'][] = ['sign_with', 'choose_backend', 'retry', 'confirm_position', 'choose_invisible', 'start_batch'];
  const moves = (dispatched: UiEvent[]) => dispatched.filter((e) => ['move_block', 'nudge_block', 'change_page', 'use_target', 'auto_place'].includes(e.type));
  const withAutoPosition = () => ({ ...defaultSettings(), signature: { ...defaultSettings().signature, auto_position: true } });

  it('gesperrter Block: kein Ziehen, keine Tastatur, kein Doppelklick, keine Feld-Angebote; Fokus auf „Signieren“; axe', async () => {
    const { mock } = renderApp({ view: workspaceView(lockedDoc({ fields: [FIELD] })) });
    const s = await screen.findByRole('application', { name: 'Signaturblock' });
    expect(s).toHaveAttribute('aria-roledescription', 'automatisch platzierter Signaturblock');
    expect(s).toHaveAttribute('aria-disabled', 'true');
    expect(s).toHaveAttribute('tabindex', '-1');
    expect(s).toHaveClass('is-locked');
    expect(s).toHaveAccessibleDescription(/Automatisch platziert laut Einstellung „Automatische Positionierung“.*„Diesmal selbst platzieren“/);
    // nach dem Öffnen bereit: Fokus auf „Signieren“
    await waitFor(() => expect(screen.getByRole('button', { name: /^Signieren/ })).toHaveFocus());
    stubRects();
    fireEvent.pointerDown(s, { button: 0, pointerId: 1, clientX: 0, clientY: 0 });
    fireEvent.pointerMove(s, { pointerId: 1, ...px(2, 311, 324) });
    fireEvent.pointerUp(s, { pointerId: 1, ...px(2, 311, 324) });
    fireEvent.keyDown(s, { key: 'ArrowRight' });
    fireEvent.keyDown(s, { key: 'PageUp' });
    fireEvent.doubleClick(screen.getByRole('img', { name: 'Seite 2 von 3 – Vertrag.pdf' }), px(1, 200, 400));
    expect(moves(mock.dispatched)).toEqual([]);
    // leeres Feld: weder Markierung noch Hinweisleiste „Ins Feld setzen“
    expect(screen.queryByRole('button', { name: /Hier signieren/ })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Ins Feld setzen' })).not.toBeInTheDocument();
    // Inspektor: „Automatisch (laut Einstellung)“, kein „Neu platzieren“
    expect(within(inspector()).getByTestId('placement-auto')).toHaveTextContent('Automatisch (laut Einstellung)');
    expect(within(inspector()).getByText('Bereit zum Signieren. Die Lage bestimmt die Einstellung „Automatische Positionierung“.')).toBeInTheDocument();
    expect(within(inspector()).queryByRole('button', { name: 'Neu platzieren' })).not.toBeInTheDocument();
    expect(mock.dispatched.filter((e) => SESSION.includes(e.type))).toEqual([]);
    await expectNoAxeViolations();
  });

  it('„Unsichtbar signieren“ und „Auf neuer Seite“ bleiben bedienbar; „Diesmal selbst platzieren“ → unlock_placement', async () => {
    const { mock, user } = renderApp({ view: workspaceView(lockedDoc()) });
    await screen.findByRole('application', { name: 'Signaturblock' });
    await user.click(screen.getByRole('switch', { name: 'Unsichtbar signieren' }));
    expect(lastEvent(mock)).toEqual({ type: 'set_invisible', invisible: true });
    await user.click(screen.getByRole('switch', { name: 'Auf neuer Seite' }));
    expect(lastEvent(mock)).toEqual({ type: 'toggle_new_page' });
    await user.click(within(inspector()).getByRole('button', { name: 'Diesmal selbst platzieren' }));
    expect(lastEvent(mock)).toEqual({ type: 'unlock_placement' });
    expect(mock.dispatched.filter((e) => SESSION.includes(e.type))).toEqual([]);
  });

  it('Überdeckung bei gesperrtem Block: „Diesmal selbst platzieren“ statt „Freie Fläche suchen“', async () => {
    const doc = lockedDoc({ check: { for_revision: 1, overlaps_content: true, overlaps_widget: false, clamped: false, placeholder_uncovered: false, overlaps_block: false } });
    const { mock, user } = renderApp({ view: workspaceView(doc) });
    await screen.findByRole('application', { name: 'Signaturblock' });
    const warn = screen.getByTestId('overlap-warning');
    expect(within(warn).queryByRole('button', { name: 'Freie Fläche suchen' })).not.toBeInTheDocument();
    await user.click(within(warn).getByRole('button', { name: 'Diesmal selbst platzieren' }));
    expect(lastEvent(mock)).toEqual({ type: 'unlock_placement' });
  });

  it('Option aus: wie bisher (verschiebbar, „Neu platzieren“, kein Fokussprung auf „Signieren“)', async () => {
    const { mock } = renderApp({ view: workspaceView() });
    const s = await screen.findByRole('application', { name: 'Signaturblock' });
    expect(s).toHaveAttribute('tabindex', '0');
    expect(s).not.toHaveClass('is-locked');
    expect(within(inspector()).queryByTestId('placement-auto')).not.toBeInTheDocument();
    expect(within(inspector()).queryByRole('button', { name: 'Diesmal selbst platzieren' })).not.toBeInTheDocument();
    expect(within(inspector()).getByRole('button', { name: 'Neu platzieren' })).toBeInTheDocument();
    await new Promise((r) => setTimeout(r, 50));
    expect(screen.getByRole('button', { name: /^Signieren/ })).not.toHaveFocus();
    fireEvent.keyDown(s, { key: 'ArrowRight' });
    expect(moves(mock.dispatched)).toEqual([{ type: 'nudge_block', dx: 1, dy: 0, fine: false, coarse: false }]);
  });

  it('Mock wie der Kern: Option an → gesperrt; „Diesmal selbst platzieren“ gibt nur dieses Dokument frei', async () => {
    const { mock, user } = renderApp({ scenario: 'contract', settings: withAutoPosition() });
    const s = await screen.findByRole('application', { name: 'Signaturblock' });
    expect(s).toHaveAttribute('aria-disabled', 'true');
    await waitFor(() => expect(screen.getByRole('button', { name: /^Signieren/ })).toHaveFocus());
    const before = (await mock.view()).document!;
    expect(before.placement_locked).toBe(true);
    expect(before.detection?.auto_placed).toBe(true);
    // Verschieben wird vom Kern ignoriert (Mock folgt): auch ein ausdrücklich gesendetes Ereignis
    await mock.dispatch({ type: 'move_block', page: 0, x: 40, y: 500 });
    expect((await mock.view()).document!.placement).toEqual(before.placement);
    await user.click(within(inspector()).getByRole('button', { name: 'Diesmal selbst platzieren' }));
    await waitFor(() => expect(sticker()).toHaveAttribute('tabindex', '0'));
    expect(sticker()).toHaveAttribute('aria-roledescription', 'verschiebbarer Signaturblock');
    expect(within(inspector()).getByRole('button', { name: 'Neu platzieren' })).toBeInTheDocument();
    sticker().focus();
    fireEvent.keyDown(sticker(), { key: 'ArrowLeft' });
    await waitFor(async () => expect((await mock.view()).document!.placement.x).toBeLessThan(before.placement.x));
    expect((await mock.view()).document!.placement_locked).toBe(false);
    expect(mock.dispatched.filter((e) => SESSION.includes(e.type))).toEqual([]);
  });

  it('Mock wie der Kern: Einstellung sofort wirksam – aus gibt den Block frei, an sperrt ihn wieder', async () => {
    const { user } = renderApp({ scenario: 'contract', settings: withAutoPosition() });
    await screen.findByRole('application', { name: 'Signaturblock' });
    expect(sticker()).toHaveAttribute('aria-disabled', 'true');
    const toggleSetting = async () => {
      await user.click(screen.getByRole('button', { name: 'Einstellungen' }));
      const dialog = await screen.findByRole('dialog', { name: 'Einstellungen' });
      await user.click(within(within(dialog).getByRole('navigation', { name: 'Bereiche' })).getByRole('button', { name: 'Dokument' }));
      await user.click(within(dialog).getByRole('switch', { name: 'Automatische Positionierung' }));
      await user.keyboard('{Escape}');
      await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    };
    await toggleSetting();
    await waitFor(() => expect(sticker()).toHaveAttribute('tabindex', '0'));
    expect(within(inspector()).queryByTestId('placement-auto')).not.toBeInTheDocument();
    await toggleSetting();
    await waitFor(() => expect(sticker()).toHaveAttribute('aria-disabled', 'true'));
    expect(within(inspector()).getByTestId('placement-auto')).toBeInTheDocument();
  });

  it('englisch und axe', async () => {
    renderApp({ view: workspaceView(lockedDoc(), {}, 'en') });
    const s = await screen.findByRole('application', { name: 'Signature block' });
    expect(s).toHaveAttribute('aria-roledescription', 'automatically placed signature block');
    expect(within(screen.getByRole('complementary', { name: 'Signature' })).getByText('Automatic (per setting)')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Place it myself this time' })).toBeInTheDocument();
    await expectNoAxeViolations();
  });
});

describe('Inhaltsbereich im Arbeitsbereich', () => {
  it('rollt nie: wird `.app-main` trotzdem gerollt (Fokus, scrollIntoView in WKWebView), springt er zurück', async () => {
    renderApp({ view: workspaceView() });
    await screen.findByRole('application', { name: 'Signaturblock' });
    const main = document.getElementById('main') as HTMLElement;
    main.scrollTop = 64;
    fireEvent.scroll(main);
    expect(main.scrollTop).toBe(0);
  });
});

describe('Mehrfachsignatur (je Block eine eigene Signatur)', () => {
  const inspector = () => screen.getByRole('complementary', { name: 'Signatur' });
  const FIELD = fieldTarget({ index: 0, name: 'Unterschrift_Antragsteller', page: 1, rect: { left: 299, right: 539, top: 300, bottom: 214 }, selectable: true });
  const single = (extra: Partial<DocumentView> = {}) =>
    documentView({
      blocks: { items: [{ index: 0, active: true, page: 2, rect: { left: 182.64, right: 412.64, top: 300, bottom: 218 }, target: { kind: 'free' } }], limit: 10, can_add: true, free_targets: 0 },
      ...extra,
    });
  /** drei Blöcke: Seite 1 frei, im Feld (Seite 2), Seite 3 frei – der dritte ist ausgewählt */
  const three = (extra: Partial<DocumentView> = {}) =>
    documentView({
      fields: [FIELD],
      blocks: {
        items: [
          { index: 0, active: false, page: 0, rect: { left: 329, right: 559, top: 118, bottom: 36 }, target: { kind: 'free' } },
          { index: 1, active: false, page: 1, rect: FIELD.block, target: { kind: 'field', index: 0 } },
          { index: 2, active: true, page: 2, rect: { left: 182.64, right: 412.64, top: 300, bottom: 218 }, target: { kind: 'free' } },
        ],
        limit: 10,
        can_add: true,
        free_targets: 0,
      },
      ...extra,
    });

  it('ein Block: „Weiteren Block hinzufügen“ im Inspektor, sonst alles wie bisher', async () => {
    const { mock, user } = renderApp({ view: workspaceView(single()) });
    await screen.findByRole('application', { name: 'Signaturblock' });
    const section = within(inspector()).getByTestId('blocks-section');
    expect(within(section).getByRole('heading', { name: 'Signaturblöcke' })).toBeInTheDocument();
    expect(within(section).queryByRole('list')).not.toBeInTheDocument();
    expect(screen.queryByTestId('sign-count')).not.toBeInTheDocument();
    expect(screen.queryByTestId('block-remove')).not.toBeInTheDocument();
    expect(within(inspector()).getByRole('button', { name: /^Signieren/ })).toBeInTheDocument();
    await user.click(within(section).getByRole('button', { name: 'Weiteren Block hinzufügen' }));
    expect(lastEvent(mock)).toMatchObject({ type: 'add_block' });
    expect(mock.dispatched.some((e) => e.type === 'sign_with')).toBe(false);
  });

  it('drei Blöcke: nummeriert auf der Seite und in der Liste, „3-mal signieren“ mit Hinweis', async () => {
    const { mock, user } = renderApp({ view: workspaceView(three()) });
    const s = await screen.findByRole('application', { name: 'Signaturblock 3 von 3' });
    expect(within(s).getByTestId('sticker-number')).toHaveTextContent('3');
    expect(s).toHaveAccessibleDescription(/Entf entfernt diesen Block/);
    // die anderen Blöcke stehen an ihrer Stelle und sind wählbar
    const others = screen.getAllByTestId('block-other');
    expect(others.map((o) => o.getAttribute('aria-label'))).toEqual(['Signaturblock 1 von 3 auswählen', 'Signaturblock 2 von 3 auswählen']);
    await user.click(others[0]!);
    expect(lastEvent(mock)).toEqual({ type: 'select_block', index: 0 });
    // das belegte Feld zeigt keine Markierung „Ins Feld setzen“
    expect(document.querySelector('.field-marker-field')).toBeNull();
    // Liste im Inspektor
    const list = within(inspector()).getByRole('list', { name: 'Signaturblöcke' });
    const rows = within(list).getAllByRole('listitem');
    expect(rows.map((r) => r.textContent)).toEqual(['1Seite 1 · rechts unten', '2Signaturfeld „Unterschrift_Antragsteller“', '3Seite 3 · unten mittig']);
    expect(within(rows[2]!).getByRole('button', { name: /Block 3 auswählen/ })).toHaveAttribute('aria-current', 'true');
    await user.click(within(rows[1]!).getByRole('button', { name: /Block 2 auswählen/ }));
    expect(lastEvent(mock)).toEqual({ type: 'select_block', index: 1 });
    await user.click(within(rows[0]!).getByRole('button', { name: 'Block 1 entfernen' }));
    expect(lastEvent(mock)).toEqual({ type: 'remove_block', index: 0 });
    // „×“ am ausgewählten Block und Entf
    await user.click(screen.getByRole('button', { name: 'Signaturblock 3 entfernen' }));
    expect(lastEvent(mock)).toEqual({ type: 'remove_block', index: 2 });
    mock.dispatched.length = 0;
    fireEvent.keyDown(s, { key: 'Delete' });
    expect(lastEvent(mock)).toEqual({ type: 'remove_block', index: 2 });
    // Signieren: Anzahl im Knopf und als Hinweis davor (ID Austria: evtl. je eine Bestätigung)
    expect(within(inspector()).getByRole('button', { name: /^3-mal signieren/ })).toBeInTheDocument();
    expect(screen.getByTestId('sign-count')).toHaveTextContent('Dieses Dokument erhält 3 Signaturen. Mit ID Austria kann für jede Signatur eine eigene Bestätigung nötig sein.');
    // „Auf neuer Seite“ bleibt auch mit mehreren Blöcken bedienbar
    expect(screen.getByRole('switch', { name: 'Auf neuer Seite' })).toBeEnabled();
    expect(mock.dispatched.some((e) => e.type === 'sign_with')).toBe(false);
    await expectNoAxeViolations();
  });

  it('Grenze erreicht bzw. Überdeckung eines anderen Blocks: Begründung statt Knopf', async () => {
    const full = three();
    const doc = { ...full, blocks: { ...full.blocks!, can_add: false, limit: 3 }, check: { ...full.check, overlaps_block: true } };
    renderApp({ view: workspaceView(doc) });
    await screen.findByRole('application', { name: 'Signaturblock 3 von 3' });
    expect(within(inspector()).getByRole('button', { name: 'Weiteren Block hinzufügen' })).toBeDisabled();
    expect(screen.getByTestId('blocks-add-reason')).toHaveTextContent('Mehr als 3 Signaturblöcke je Dokument sind nicht möglich.');
    const warn = screen.getByTestId('overlap-warning');
    expect(warn).toHaveTextContent('Der Signaturblock überdeckt einen anderen Signaturblock.');
    expect(within(warn).queryByRole('button')).not.toBeInTheDocument();
  });

  it('„Auf neuer Seite“ mit mehreren Blöcken: weiterer Block möglich, Hinweis zur Reihenfolge', async () => {
    const base = three();
    // der ausgewählte Block liegt auf der virtuellen neuen Seite (Seite 4) und ist Nummer 1
    const doc = {
      ...base,
      new_page: true,
      placement: { ...base.placement, page_index: 3 },
      blocks: {
        ...base.blocks!,
        items: [
          { index: 0, active: true, page: 3, rect: { left: 182.64, right: 412.64, top: 300, bottom: 218 }, target: { kind: 'free' as const } },
          { index: 1, active: false, page: 0, rect: { left: 329, right: 559, top: 118, bottom: 36 }, target: { kind: 'free' as const } },
        ],
      },
    };
    const { mock, user } = renderApp({ view: workspaceView(doc) });
    await screen.findByRole('application', { name: 'Signaturblock 1 von 2' });
    expect(within(inspector()).getByText(/Blöcke auf der neuen Seite werden zuerst signiert und stehen deshalb vorn\./)).toBeInTheDocument();
    const add = within(inspector()).getByRole('button', { name: 'Weiteren Block hinzufügen' });
    expect(add).toBeEnabled();
    await user.click(add);
    expect(lastEvent(mock)).toMatchObject({ type: 'add_block' });
    expect(screen.getByRole('switch', { name: 'Auf neuer Seite' })).toBeChecked();
  });

  it('„Alle Platzhalter belegen“ ab zwei freien Zielen', async () => {
    const doc = single({ fields: [FIELD, { ...FIELD, index: 1, name: 'Unterschrift_Zweite' }] });
    const { mock, user } = renderApp({ view: workspaceView({ ...doc, blocks: { ...doc.blocks!, free_targets: 2 } }) });
    await screen.findByRole('application', { name: 'Signaturblock' });
    await user.click(within(inspector()).getByRole('button', { name: 'Alle 2 Platzhalter belegen' }));
    expect(lastEvent(mock)).toEqual({ type: 'fill_targets' });
  });

  it('gesperrt, unsichtbar bzw. beim Signieren: kein Abschnitt bzw. nichts bedienbar', async () => {
    const { unmount } = renderApp({ view: workspaceView(single({ invisible: true, sticker: null })) });
    await screen.findByRole('complementary', { name: 'Signatur' });
    expect(screen.queryByTestId('blocks-section')).not.toBeInTheDocument();
    unmount();
    renderApp({ view: workspaceView(three({ phase: 'signing' }), { actions: { ...workspaceView().actions, edit_placement: false, sign: false } }) });
    await screen.findByRole('complementary', { name: 'Signatur' });
    for (const o of screen.getAllByTestId('block-other')) expect(o).toBeDisabled();
    expect(screen.queryByTestId('block-remove')).not.toBeInTheDocument();
    expect(screen.queryByTestId('sign-count')).not.toBeInTheDocument();
  });
});
