// @vitest-environment node
/** Darstellungsrechnungen des Arbeitsbereichs (reine Funktionen, ADR 0004 §4.1/§4.3). */
import { describe, expect, it } from 'vitest';
import { translator } from '../../i18n/catalog';
import { documentView, fieldTarget } from '../../test/views';
import {
  announceText,
  clampToPage,
  describeZone,
  displayPages,
  grabPoint,
  layoutPages,
  overlapState,
  previewScale,
  rectToBox,
  snapToGuides,
  targetAt,
  TOUCH_OFFSET_PT,
  visiblePages,
} from './placement';

describe('Seitenraster', () => {
  it('Seiten untereinander, neue Seite in der Größe der letzten', () => {
    const doc = documentView({ new_page: true });
    const sizes = displayPages(doc);
    expect(sizes).toHaveLength(4);
    expect(sizes[3]).toEqual(sizes[2]);
    const l = layoutPages(sizes, 1);
    expect(l.tops[0]).toBe(24);
    expect(l.tops[1]).toBeCloseTo(24 + 841.89 + 28);
    expect(l.width).toBeCloseTo(595.28);
  });

  it('sichtbare Seiten aus der Scrollposition', () => {
    const sizes = displayPages(documentView());
    const l = layoutPages(sizes, 1);
    expect(visiblePages(l, sizes, 1, 0, 800, 0)).toEqual([0]);
    expect(visiblePages(l, sizes, 1, 850, 400, 0)).toEqual([0, 1]);
    expect(visiblePages(l, sizes, 1, 5000, 400, 0)).toEqual([]);
  });

  it('Rechteck (PAGE_ABS) → Box in px', () => {
    expect(rectToBox({ left: 100, right: 330, top: 300, bottom: 218 }, 842, 2)).toEqual({ left: 200, top: 1084, width: 460, height: 164 });
  });

  it('Klemmen der Vorschau wie F-35/KR-F11', () => {
    expect(clampToPage(-20, 900, 230, 82, 595, 842)).toEqual({ x: 0, y: 842 });
    expect(clampToPage(500, 10, 230, 82, 595, 842)).toEqual({ x: 365, y: 82 });
  });
});

describe('Einrasten an den Einrastlinien des Kerns', () => {
  const guides = documentView().layout[0]?.guides ?? [];

  it('rechte Blockkante an content_right, Oberkante an below_content', () => {
    const s = snapToGuides(311, 324, 230, 82, guides, 6);
    expect(s.x).toBe(539 - 230);
    expect(s.y).toBe(322);
    expect(s.snapped).toEqual(['content_right', 'below_content']);
  });

  it('Mitte an center_x, Unterkante an content_bottom', () => {
    const s = snapToGuides(297.64 - 115 + 2, 340 + 82 + 3, 230, 82, guides, 6);
    expect(s.x).toBeCloseTo(297.64 - 115);
    expect(s.y).toBeCloseTo(340 + 82);
    expect(s.snapped).toEqual(['center_x', 'content_bottom']);
  });

  it('außerhalb der Schwelle kein Einrasten', () => {
    const s = snapToGuides(250, 600, 230, 82, guides, 6);
    expect(s).toEqual({ x: 250, y: 600, guides: [], snapped: [] });
  });
});

describe('Feld-Treffer, Lage, Touch', () => {
  it('Blockmitte im wählbaren Feld bzw. QR-Platzhalter', () => {
    const fields = [
      fieldTarget({ index: 0, name: 'A', page: 1, rect: { left: 300, right: 540, top: 200, bottom: 114 }, selectable: true }),
      fieldTarget({ index: 1, name: 'B', page: 1, rect: { left: 50, right: 280, top: 200, bottom: 114 }, selectable: false }),
    ];
    expect(targetAt(fields, null, 1, 400, 150)?.target).toEqual({ kind: 'field', index: 0 });
    expect(targetAt(fields, null, 1, 100, 150)).toBeNull();
    expect(targetAt(fields, null, 0, 400, 150)).toBeNull();
    expect(targetAt([], { page: 0, id: '1', rect: { left: 0, right: 100, top: 100, bottom: 0 } }, 0, 50, 50)?.target).toEqual({ kind: 'qr' });
  });

  it('nur angebotene Felder (Kern: `suitable`); der Block landet in `block`, das Feld ist der Rahmen (Nachtrag 16)', () => {
    const page = fieldTarget({ index: 0, name: 'Seite', page: 1, rect: { left: 0, right: 612, top: 792, bottom: 0 }, selectable: true });
    const ok = fieldTarget({ index: 1, name: 'Groß', page: 1, rect: { left: 100, right: 500, top: 400, bottom: 200 }, selectable: true });
    expect(page.suitable).toBe(false);
    expect(targetAt([page], null, 1, 306, 396)).toBeNull();
    const hit = targetAt([page, ok], null, 1, 300, 300);
    expect(hit).toEqual({ target: { kind: 'field', index: 1 }, rect: { left: 185, right: 415, bottom: 259, top: 341 }, frame: ok.rect });
  });

  it('Vorschau-Maßstab: Darstellungsgröße × Verkleinerung im Feld, auf 0,25 aufgerundet, begrenzt', () => {
    expect(previewScale(1.15)).toBe(1.25);
    expect(previewScale(3 * 0.56)).toBe(1.75);
    expect(previewScale(1.15, 0.5)).toBe(0.75);
    expect(previewScale(2, 1.7)).toBe(2);
    expect(previewScale(0.01)).toBe(0.25);
    expect(previewScale(40)).toBe(8);
    expect(previewScale(Number.NaN)).toBe(0.25);
  });

  it('virtuelle neue Seite in der Größe des Kerns (`new_page_size`, MediaBox der letzten Seite)', () => {
    const doc = documentView({ new_page: true, new_page_size: [612, 792] });
    expect(displayPages(doc).at(-1)).toEqual([612, 792]);
    expect(displayPages(documentView({ new_page: true })).at(-1)).toEqual([595.28, 841.89]);
    expect(displayPages(documentView({ new_page_size: [612, 792] }))).toHaveLength(3);
  });

  it('sprechende Lage („rechts unten“)', () => {
    expect(describeZone({ left: 400, right: 560, top: 120, bottom: 40 }, 595, 842)).toEqual({ h: 'right', v: 'bottom' });
    expect(describeZone({ left: 182, right: 412, top: 420, bottom: 340 }, 595, 842)).toEqual({ h: 'center', v: 'middle' });
    expect(describeZone({ left: 20, right: 120, top: 820, bottom: 760 }, 595, 842)).toEqual({ h: 'left', v: 'top' });
  });

  it('Greifpunkt: Versatz auf dem Block, Touch mittig über dem Finger, Maus mittig am Zeiger', () => {
    const block = { x: 100, y: 500, width: 200, height: 60 };
    expect(grabPoint(true, 'touch', { x: 150, top: 352 }, block, 842)).toEqual({ grabX: 50, grabTop: 10 });
    expect(grabPoint(false, 'touch', { x: 0, top: 0 }, block, 842)).toEqual({ grabX: 100, grabTop: 60 + TOUCH_OFFSET_PT });
    expect(grabPoint(false, 'mouse', { x: 0, top: 0 }, block, 842)).toEqual({ grabX: 100, grabTop: 30 });
  });
});

describe('Live-Region und Überdeckung', () => {
  const t = translator('de');

  it('Lage aus `announce` (linke obere Ecke, `top`), Einrasten der Oberfläche, Prüfergebnis', () => {
    expect(announceText(t, documentView(), ['content_right', 'below_content'])).toBe(
      'Seite 3 von 3, linke obere Ecke 64 mm von links, 106 mm von unten. Rastet ein: rechts unten. Freie Fläche.',
    );
    const overlap = documentView({ check: { for_revision: 1, overlaps_content: true, overlaps_widget: false, clamped: false, placeholder_uncovered: false, overlaps_block: false } });
    expect(announceText(t, overlap)).toBe('Seite 3 von 3, linke obere Ecke 64 mm von links, 106 mm von unten. Achtung: überdeckt Inhalt.');
    expect(overlapState(overlap)).toBe('content');
  });

  it('Prüfergebnis einer älteren Revision zählt nicht; unbekannt = keine Aussage', () => {
    const stale = documentView({ revision: 2, check: { for_revision: 1, overlaps_content: true, overlaps_widget: false, clamped: false, placeholder_uncovered: false, overlaps_block: false } });
    expect(overlapState(stale)).toBeNull();
    expect(announceText(t, stale)).toBe('Seite 3 von 3, linke obere Ecke 64 mm von links, 106 mm von unten.');
    const unknown = documentView({ check: { for_revision: 1, overlaps_content: null, overlaps_widget: false, clamped: false, placeholder_uncovered: false, overlaps_block: false } });
    expect(announceText(t, unknown)).not.toContain('Fläche');
  });

  it('im Signaturfeld bzw. unsichtbar', () => {
    const field = documentView({
      target: { kind: 'field', index: 0 },
      fields: [fieldTarget({ index: 0, name: 'Unterschrift', page: 2, rect: { left: 300, right: 540, top: 200, bottom: 114 }, selectable: true })],
    });
    expect(announceText(t, field)).toContain('Im Signaturfeld „Unterschrift“.');
    expect(announceText(t, documentView({ invisible: true }))).toBe('Unsichtbare Signatur, kein Signaturbild.');
  });
});
