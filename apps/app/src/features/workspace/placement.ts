/**
 * Darstellungsrechnungen des Arbeitsbereichs (ADR 0004 §4.1, §4.3) – reine Funktionen ohne DOM:
 * Seitenraster, pt ↔ px, Einrasten an den Einrastlinien des Kerns (`PageLayoutView::guides`, 7 px),
 * Feld-Treffer beim Ziehen, Klemmen der **Vorschau**, sprechende Lage („rechts unten“), Touch-Greifpunkt
 * und der Satz der Live-Region.
 *
 * Fachlich maßgeblich bleibt der Kern: Er klemmt (`MoveBlock`), rechnet die automatische Position, freie
 * Flächen und die Überdeckung. Koordinaten wie im Kern: pt, `PAGE_ABS` (Ursprung links unten, y nach oben;
 * eine Lage ist die linke obere Ecke `x`/`y` = Oberkante von unten).
 */
import type { AnnounceDetail, BlockSlotView, DocumentView, FieldTargetView, GuideKind, GuideView, PlacementTarget, QrTargetView, RectPt } from '../../core/types';
import type { Translator } from '../../i18n/catalog';

/** 100 % Zoom = 96 px je Zoll (1 pt = 1,333 px) wie im Prototyp. */
export const PT_TO_PX = 96 / 72;
export const ZOOM_STEPS = [50, 67, 75, 90, 100, 110, 125, 150, 175, 200] as const;
export const PAGE_PAD = 24;
export const PAGE_GAP = 28;
/** Einrasten beim Ziehen: Schwelle in Bildschirm-Pixeln (§3.3). */
export const SNAP_PX = 7;
/** Touch: Abstand zwischen Blockunterkante und Finger in pt (Fingerversatz, aus `PositionScreen`). */
export const TOUCH_OFFSET_PT = 12;

/**
 * Seitengrößen der Anzeige inkl. virtueller neuer Seite (`page == page_count`): deren Größe nennt der Kern
 * (`new_page_size` = MediaBox der letzten Seite ohne CropBox, wie PDF-AS sie anhängt; ADR 0004 Nachtrag 16),
 * sonst die der letzten Seite.
 */
export function displayPages(doc: DocumentView): [number, number][] {
  const sizes = doc.doc.page_sizes.map((s) => [s[0], s[1]] as [number, number]);
  const last = doc.new_page_size ?? sizes[sizes.length - 1] ?? [595.28, 841.89];
  return doc.new_page ? [...sizes, [last[0], last[1]]] : sizes;
}

export interface PagesLayout {
  tops: number[];
  lefts: number[];
  width: number;
  height: number;
}

/** Senkrechte Folge aller Seiten (px), waagrecht mittig zur breitesten Seite. */
export function layoutPages(sizes: [number, number][], scale: number): PagesLayout {
  const maxW = Math.max(1, ...sizes.map((s) => s[0]));
  const tops: number[] = [];
  const lefts: number[] = [];
  let y = PAGE_PAD;
  for (const [w, h] of sizes) {
    tops.push(y);
    lefts.push(((maxW - w) * scale) / 2);
    y += h * scale + PAGE_GAP;
  }
  return { tops, lefts, width: maxW * scale, height: Math.max(PAGE_PAD * 2, y - PAGE_GAP + PAGE_PAD) };
}

/** Seiten, die im sichtbaren Bereich (± Rand) liegen. */
export function visiblePages(layout: PagesLayout, sizes: [number, number][], scale: number, scrollTop: number, viewport: number, margin = 200): number[] {
  const out: number[] = [];
  const top = scrollTop - margin;
  const bottom = scrollTop + Math.max(viewport, 1) + margin;
  sizes.forEach(([, h], i) => {
    const t = layout.tops[i] ?? 0;
    if (t + h * scale >= top && t <= bottom) out.push(i);
  });
  return out;
}

/** Rechteck (`PAGE_ABS`) → Box in px relativ zur Seite. */
export function rectToBox(r: RectPt, pageHeight: number, scale: number): { left: number; top: number; width: number; height: number } {
  return { left: r.left * scale, top: (pageHeight - r.top) * scale, width: (r.right - r.left) * scale, height: (r.top - r.bottom) * scale };
}

/** Rechteck der Lage (linke obere Ecke, Größe) in `PAGE_ABS`. */
export function placementRect(x: number, yTop: number, w: number, h: number): RectPt {
  return { left: x, right: x + w, top: yTop, bottom: yTop - h };
}

/** Klemmung wie F-35 mit KR-F11 – nur für die Vorschau beim Ziehen (der Kern klemmt maßgeblich). */
export function clampToPage(x: number, yTop: number, w: number, h: number, pw: number, ph: number): { x: number; y: number } {
  return { x: Math.max(0, Math.min(x, pw - w)), y: Math.min(ph, Math.max(yTop, h)) };
}

export interface SnapResult {
  x: number;
  y: number;
  guides: GuideView[];
  snapped: GuideKind[];
}

/**
 * Einrasten an den Einrastlinien des Kerns (§3.3): `X`-Linien gelten für die linke (`content_left`,
 * `margin_left`) bzw. rechte Blockkante (`content_right`, `margin_right`) bzw. die Blockmitte (`center_x`);
 * `Y`-Linien für die Ober- (`content_top`, `below_content`, `margin_top`) bzw. Unterkante
 * (`content_bottom`, `margin_bottom`) bzw. Mitte (`center_y`). `threshold` in pt.
 */
export function snapToGuides(x: number, yTop: number, w: number, h: number, guides: GuideView[], threshold: number): SnapResult {
  const xTargets = guides
    .filter((g) => g.axis === 'x')
    .map((g) => ({ g, pos: g.kind === 'content_right' || g.kind === 'margin_right' ? g.at - w : g.kind === 'center_x' ? g.at - w / 2 : g.at }));
  const yTargets = guides
    .filter((g) => g.axis === 'y')
    .map((g) => ({ g, pos: g.kind === 'content_bottom' || g.kind === 'margin_bottom' ? g.at + h : g.kind === 'center_y' ? g.at + h / 2 : g.at }));
  const nearest = (targets: { g: GuideView; pos: number }[], v: number) => {
    let best: { g: GuideView; pos: number } | null = null;
    let dist = threshold;
    for (const t of targets) {
      const d = Math.abs(t.pos - v);
      if (d <= dist) {
        best = t;
        dist = d;
      }
    }
    return best;
  };
  const out: SnapResult = { x, y: yTop, guides: [], snapped: [] };
  const bx = nearest(xTargets, x);
  if (bx) {
    out.x = bx.pos;
    out.guides.push(bx.g);
    out.snapped.push(bx.g.kind);
  }
  const by = nearest(yTargets, yTop);
  if (by) {
    out.y = by.pos;
    out.guides.push(by.g);
    out.snapped.push(by.g.kind);
  }
  return out;
}

/** Ziel beim Ziehen: `rect` = wo der Block dort steht, `frame` = Rechteck des Felds bzw. Platzhalters. */
export interface DropTarget {
  target: PlacementTarget;
  rect: RectPt;
  frame: RectPt;
}

/**
 * Als Ziel angebotenes Feld (ADR 0004 Nachtrag 16): wählbar und laut Kern geeignet (`suitable` – ein seitengroßes
 * oder 3 pt schmales Feld nicht), mit Seite und Rechteck.
 */
export function offeredField(f: FieldTargetView): f is FieldTargetView & { page: number; rect: RectPt } {
  return f.selectable && f.suitable && f.page !== null && f.rect !== null;
}

/**
 * Ziel unter der Blockmitte beim Ziehen (angebotenes Signaturfeld bzw. QR-Platzhalter), sonst `null`. Im Feld
 * steht der Block dort, wo ihn der Kern schreibt (`block`: proportional, höchstens natürliche Größe, mittig).
 */
export function targetAt(fields: FieldTargetView[], qr: QrTargetView | null, page: number, cx: number, cy: number): DropTarget | null {
  const inside = (r: RectPt) => cx >= r.left && cx <= r.right && cy >= r.bottom && cy <= r.top;
  for (const f of fields) {
    if (offeredField(f) && f.page === page && inside(f.rect)) return { target: { kind: 'field', index: f.index }, rect: f.block ?? f.rect, frame: f.rect };
  }
  if (qr && qr.page === page && inside(qr.rect)) return { target: { kind: 'qr' }, rect: qr.rect, frame: qr.rect };
  return null;
}

/**
 * Maßstab der Blockvorschau (`blockPreviewUrl`) für die tatsächliche Darstellungsgröße: Bildpunkte je pt
 * (`pxPerPt` = Zoom × Gerätepixel) mal Verkleinerung im Feld (`fit` ≤ 1), auf 0,25 **aufgerundet** – das Bild
 * wird nie hochskaliert (unscharf), nur geringfügig verkleinert.
 */
export function previewScale(pxPerPt: number, fit = 1): number {
  const s = pxPerPt * Math.min(1, Math.max(0, fit));
  if (!Number.isFinite(s) || s <= 0) return 0.25;
  return Math.min(8, Math.max(0.25, Math.ceil(s * 4 - 1e-6) / 4));
}

export type ZoneH = 'left' | 'center' | 'right';
export type ZoneV = 'top' | 'middle' | 'bottom';

/** Beschreibende Lage auf der Seite (Inspektor, Ansage), z. B. „rechts unten“. */
export function describeZone(r: RectPt, pw: number, ph: number): { h: ZoneH; v: ZoneV } {
  const cx = (r.left + r.right) / 2;
  const cyFromTop = ph - (r.top + r.bottom) / 2;
  const h: ZoneH = cx < pw / 3 ? 'left' : cx > (pw * 2) / 3 ? 'right' : 'center';
  const v: ZoneV = cyFromTop < ph * 0.33 ? 'top' : cyFromTop > ph * 0.55 ? 'bottom' : 'middle';
  return { h, v };
}

export function zoneText(t: Translator, r: RectPt, pw: number, ph: number): string {
  const z = describeZone(r, pw, ph);
  return t.t(`ng-ui-ws-zone-${z.v}-${z.h}`);
}

/**
 * Greifpunkt im Block in pt (von der linken oberen Ecke; `grabTop` nach unten, aus `PositionScreen`). Auf
 * dem Block bleibt der Versatz zum Zeiger; außerhalb (Doppeltippen/Doppelklick) steht der Block bei Touch
 * mittig über dem Finger (Unterkante [`TOUCH_OFFSET_PT`] über dem Berührpunkt), bei Maus/Stift mittig am Zeiger.
 */
export function grabPoint(
  onBlock: boolean,
  pointerType: string,
  pointer: { x: number; top: number },
  block: { x: number; y: number; width: number; height: number },
  pageHeight: number,
): { grabX: number; grabTop: number } {
  if (onBlock) return { grabX: pointer.x - block.x, grabTop: pointer.top - (pageHeight - block.y) };
  if (pointerType === 'touch') return { grabX: block.width / 2, grabTop: block.height + TOUCH_OFFSET_PT };
  return { grabX: block.width / 2, grabTop: block.height / 2 };
}

function snapWord(t: Translator, kinds: GuideKind[]): string {
  const right = kinds.some((k) => k === 'content_right' || k === 'margin_right');
  const bottom = kinds.some((k) => k === 'content_bottom' || k === 'margin_bottom' || k === 'below_content');
  if (right && bottom) return t.t('ng-ui-ws-snap-corner');
  return kinds
    .map((k) => {
      switch (k) {
        case 'content_left':
        case 'margin_left':
          return t.t('ng-ui-ws-snap-left');
        case 'content_right':
        case 'margin_right':
          return t.t('ng-ui-ws-snap-right');
        case 'center_x':
          return t.t('ng-ui-ws-snap-hcenter');
        case 'content_top':
        case 'margin_top':
          return t.t('ng-ui-ws-snap-top');
        case 'content_bottom':
        case 'margin_bottom':
          return t.t('ng-ui-ws-snap-bottom');
        case 'center_y':
          return t.t('ng-ui-ws-snap-vcenter');
        case 'below_content':
          return t.t('ng-ui-ws-snap-below');
      }
    })
    .join(', ');
}

/**
 * Satz der Live-Region, z. B. „Seite 3 von 3, linke obere Ecke 112 mm von links, 67 mm von unten. Rastet ein:
 * rechts unten. Freie Fläche.“ – Lage aus `announce` (Kern), „rastet ein“ kennt nur die Oberfläche (Ziehen), das
 * Prüfergebnis aus `check` (nur, wenn es zur aktuellen Revision gehört). `detailed` (Einstellungen ›
 * Barrierefreiheit) ergänzt die Lage in Worten („Lage: rechts unten.“).
 */
export function announceText(t: Translator, doc: DocumentView, snapped: GuideKind[] = [], detail: AnnounceDetail = 'short'): string {
  const a = doc.announce;
  if (doc.invisible) return t.t('ng-ui-ws-announce-invisible');
  if (!a.page) return '';
  // linke obere Ecke wie die Millimeter-Eingabe und PDF-AS (docs/analyse/06 R-6)
  const parts = [t.t('ng-ui-ws-announce', { page: a.page, pages: a.pages ?? a.page, left: a.left ?? '0', top: a.top ?? '0' })];
  // Mehrfachsignatur: welcher Block gemeint ist
  const total = blockTotal(doc);
  if (total > 1) parts.unshift(t.t('ng-ui-ws-announce-blocks', { n: activeBlock(doc) + 1, total }));
  // ausführlich (Einstellungen › Barrierefreiheit): Lage in Worten
  if (detail === 'detailed' && doc.sticker) {
    const sizes = displayPages(doc);
    const [pw, ph] = sizes[doc.placement.page_index] ?? sizes[sizes.length - 1] ?? [595.28, 841.89];
    parts.push(t.t('ng-ui-ws-announce-zone', { zone: zoneText(t, doc.sticker, pw, ph) }));
  }
  const field = doc.target.kind === 'field' ? doc.fields.find((f) => doc.target.kind === 'field' && f.index === doc.target.index) : undefined;
  if (field) parts.push(t.t('ng-ui-ws-announce-field', { name: field.name }));
  else if (doc.target.kind === 'qr') parts.push(t.t('ng-ui-ws-announce-qr'));
  if (snapped.length > 0) parts.push(t.t('ng-ui-ws-announce-snap', { where: snapWord(t, snapped) }));
  const c = doc.check;
  if (placeholderUncovered(doc)) parts.push(t.t('ng-ui-ws-announce-qr-uncovered'));
  if (c.for_revision === doc.revision && c.overlaps_block === true && !doc.invisible) parts.push(t.t('ng-ui-ws-announce-block'));
  if (c.for_revision === doc.revision && doc.target.kind === 'free') {
    if (c.overlaps_content === true) parts.push(t.t('ng-ui-ws-announce-overlap'));
    else if (c.overlaps_widget) parts.push(t.t('ng-ui-ws-announce-widget'));
    else if (c.overlaps_content === false) parts.push(t.t('ng-ui-ws-announce-free'));
  }
  return parts.join(' ');
}

/**
 * Bedeckt der Block den QR-Platzhalter nicht ganz (Backlog Nr. 32)? Der Kern entscheidet
 * (`check.placeholder_uncovered`); nur für die aktuelle Revision und sichtbare Signaturen.
 */
export function placeholderUncovered(doc: DocumentView): boolean {
  const c = doc.check;
  return !doc.invisible && doc.target.kind === 'qr' && c.for_revision === doc.revision && c.placeholder_uncovered === true;
}

/**
 * Mehrfachsignatur (Kern: `DocumentView::blocks`): alle Signaturblöcke in ihrer Reihenfolge – je Block eine eigene
 * Signatur. Unsichtbar bzw. ohne Angabe des Kerns: genau ein Block.
 */
export function blockItems(doc: DocumentView): BlockSlotView[] {
  if (doc.invisible) return [];
  return doc.blocks?.items ?? [];
}

/** Anzahl der Signaturen, die das Dokument erhält (unsichtbar: eine). */
export function blockTotal(doc: DocumentView): number {
  return Math.max(1, blockItems(doc).length);
}

/** Stelle des ausgewählten Blocks (0-basiert). */
export function activeBlock(doc: DocumentView): number {
  return blockItems(doc).find((b) => b.active)?.index ?? 0;
}

/** Überdeckung gilt nur, wenn die Prüfung zur aktuellen Revision gehört (§4.3). */
export function overlapState(doc: DocumentView): 'block' | 'content' | 'widget' | null {
  const c = doc.check;
  if (doc.invisible || c.for_revision !== doc.revision) return null;
  // Mehrfachsignatur: ein anderer Signaturblock desselben Dokuments
  if (c.overlaps_block === true) return 'block';
  if (c.overlaps_content === true) return 'content';
  if (c.overlaps_widget) return 'widget';
  return null;
}

/**
 * Lage aus der Automatik? Der Kern entscheidet (`detection.auto_placed` = `WorkspaceEdit::automatic`: `false` nach
 * jeder Platzierung durch die Nutzerin); ohne `detection` nach `origin`.
 */
export function isAutoPlaced(doc: DocumentView): boolean {
  return doc.detection ? doc.detection.auto_placed : doc.origin === 'auto';
}

/**
 * Hat die Nutzerin die Signatur in diesem Dokument selbst platziert? Dann fragt „Anderes PDF öffnen“ vorher nach
 * (ADR 0004 Nachtrag 9) – die Platzierung ginge verloren.
 */
export function placedByHand(doc: DocumentView): boolean {
  return doc.phase === 'editing' && !isAutoPlaced(doc);
}

/** Dateigröße zur Anzeige („214 KB“, „1,2 MB“). */
export function formatSize(bytes: number, lang: 'de' | 'en'): string {
  const locale = lang === 'de' ? 'de-AT' : 'en-GB';
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024)).toLocaleString(locale)} KB`;
  return `${(bytes / (1024 * 1024)).toLocaleString(locale, { maximumFractionDigits: 1 })} MB`;
}
