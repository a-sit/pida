/**
 * Durchgehend scrollbare Seitenansicht mit Signatur-„Sticker“ (Interaktion aus dem Prototyp, ADR 0004
 * §4.3): Seiten sind Kacheln des Kerns (`bridge.pageTileUrl`, nur sichtbare Seiten ± Rand), der Sticker
 * zeigt die Blockvorschau des Kerns (`bridge.blockPreviewUrl`) im Rechteck `document.sticker`.
 *
 * - **Ziehen** (Maus/Touch/Stift): Entwurf lokal, Einrasten an `layout.guides` der Zielseite (7 px), freie
 *   Flächen aus `layout.free` als Bänder, Feld-Treffer (Blockmitte in einem **angebotenen** Feld, `suitable`)
 *   → `use_target`, sonst beim Loslassen `move_block`. Ohne Layoutdaten entfallen Einrasten und Bänder – Ziehen
 *   geht weiter. Im Feld steht der Block, wo ihn der Kern schreibt (`block`: proportional, höchstens natürliche
 *   Größe, mittig; das Feld als Rahmen) – nie über das ganze Feld gestreckt (ADR 0004 Nachtrag 16).
 * - **Blockvorschau** in der tatsächlichen Darstellungsgröße (Zoom × Gerätepixel × Verkleinerung im Feld,
 *   aufgerundet): nie hochskaliert.
 * - **Touch** (aus `PositionScreen`): Greifversatz bleibt, Haptik beim Greifen, ein zweiter Finger verschiebt
 *   nicht, Doppeltippen setzt den Block mittig über den Finger (Fingerversatz), `touch-action: none` nur
 *   auf dem Sticker.
 * - **Doppelklick** auf eine Seite setzt den Block mittig an den Zeiger (`move_block`).
 * - **Tastatur** am Sticker: Pfeile → `nudge_block` (15 pt; Umschalt fein 1 pt, Alt bzw. Strg/⌘ grob 50 pt –
 *   die Schritte rechnet der Kern), Bild↑/↓ und Pos1/Ende → `change_page`.
 * - **Überdeckung** (Rahmen, Plakette) nur, wenn `check.for_revision == revision`.
 * - **Gesperrt** (Einstellung „Automatische Positionierung“, `locked`): weder Ziehen noch Tastatur noch
 *   Feld-Markierungen; der Block nennt sich „automatisch platziert“ und verweist auf „Diesmal selbst platzieren“.
 * - **Mehrfachsignatur** (`document.blocks`, je Block eine eigene Signatur): Der ausgewählte Block ist der Sticker;
 *   die übrigen stehen als nummerierte Knöpfe an ihrer Stelle – ein Klick wählt sie aus (`select_block`). Am
 *   ausgewählten Block entfernt „×“ bzw. Entf den Block (`remove_block`); belegte Felder zeigen keine Markierung.
 */
import { useEffect, useId, useRef, useState, type KeyboardEvent, type MouseEvent, type PointerEvent, type RefObject } from 'react';
import { Button } from 'react-aria-components';
import { useApp } from '../../app/context';
import { tileScale } from '../../core/bridge';
import type { DocumentView, GuideKind, GuideView, RectPt } from '../../core/types';
import { useModKey } from '../../ui/controls';
import { IconAlert, IconCheck, IconGrip, IconLock, IconClose, IconPen, IconQr } from '../../ui/Icons';
import {
  activeBlock,
  blockItems,
  clampToPage,
  grabPoint,
  offeredField,
  overlapState,
  placementRect,
  previewScale,
  rectToBox,
  snapToGuides,
  SNAP_PX,
  targetAt,
  type DropTarget,
  type PagesLayout,
} from './placement';

interface Draft {
  page: number;
  /** linke obere Ecke (`PAGE_ABS`) */
  x: number;
  y: number;
  /** Ziel unter der Blockmitte (angebotenes Feld/QR): Lage des Blocks dort und Rechteck des Ziels */
  target: DropTarget | null;
  guides: GuideView[];
  snapped: GuideKind[];
}

/** Kurze Haptik beim Greifen per Touch (Android-WebView; iOS-WebKit kennt `vibrate` nicht). */
function haptic(): void {
  try {
    if (typeof navigator !== 'undefined' && typeof navigator.vibrate === 'function') navigator.vibrate(8);
  } catch {
    /* nur Komfort */
  }
}

/** Je Adresse eine eigene Kachel: der Ladezustand gehört zum Bild, nicht zur Seite (kein „geladen“ vom vorigen Maßstab). */
function KeyedPageTile({ url }: { url: string }) {
  return <PageTile key={url} url={url} alt="" />;
}

function PageTile({ url, alt }: { url: string; alt: string }) {
  const [state, setState] = useState<'loading' | 'ok' | 'failed'>('loading');
  const { t } = useApp();
  return (
    <>
      <img className="page-tile" src={url} alt={alt} draggable={false} onLoad={() => setState('ok')} onError={() => setState('failed')} data-state={state} />
      {state === 'failed' ? (
        <span className="page-failed" role="status">
          {t.t('ng-ui-ws-page-failed')}
        </span>
      ) : null}
    </>
  );
}

/** Lage der Einblendung „automatisch platziert“ zum Block: links-, mittig- oder rechtsbündig, damit sie auch in
 *  schmalen Ansichten auf der Seite bleibt (Mitte des Blocks relativ zur Seitenbreite). */
function coachAnchor(center: number): string {
  if (center < 0.35) return ' is-start';
  if (center <= 0.65) return ' is-center';
  return '';
}

export function Pages({
  doc,
  sizes,
  scale,
  layout,
  loadPages,
  editable,
  locked = false,
  coach,
  hostRef,
  stickerRef,
  onInteract,
  onSnap,
}: {
  doc: DocumentView;
  sizes: [number, number][];
  scale: number;
  layout: PagesLayout;
  /** Seiten, deren Kachel geladen wird (sichtbar ± Rand) */
  loadPages: Set<number>;
  /** Block verschiebbar (Ziehen, Tastatur, Doppelklick, Feld-Markierungen) */
  editable: boolean;
  /** gesperrt durch die Einstellung „Automatische Positionierung“ (Kern: `placement_locked`) */
  locked?: boolean;
  coach: boolean;
  hostRef: RefObject<HTMLDivElement | null>;
  stickerRef: RefObject<HTMLDivElement | null>;
  onInteract: () => void;
  /** Einrasten des letzten Ziehens (für die Live-Region) */
  onSnap: (snapped: GuideKind[]) => void;
}) {
  const { t, bridge, dispatch } = useApp();
  const { mod, enter } = useModKey();
  const helpId = useId();
  const [draft, setDraft] = useState<Draft | null>(null);
  const [active, setActive] = useState(false);
  const draftRef = useRef<Draft | null>(null);
  const grab = useRef<{ dx: number; dy: number; sx: number; sy: number; moved: boolean; pointer: number } | null>(null);
  const lastTap = useRef<{ time: number; x: number; y: number } | null>(null);
  const settle = useRef<ReturnType<typeof setTimeout> | null>(null);
  const dpr = typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1;
  const tile = tileScale(scale * dpr);
  const block = doc.block;
  const placementPage = doc.placement.page_index;
  const realPages = doc.doc.page_count;
  // Mehrfachsignatur: die übrigen Blöcke (nummeriert, wählbar) und die Nummer des ausgewählten
  const items = blockItems(doc);
  const total = Math.max(1, items.length);
  const activeNo = activeBlock(doc) + 1;
  const others = items.filter((b) => !b.active && b.rect && b.page < sizes.length);
  const takenFields = new Set(others.flatMap((b) => (b.target.kind === 'field' ? [b.target.index] : [])));
  const qrTaken = others.some((b) => b.target.kind === 'qr');
  const removable = editable && !locked && total > 1;
  const removeActive = () => {
    onInteract();
    onSnap([]);
    void dispatch({ type: 'remove_block', index: activeNo - 1 });
  };

  // Neue Projektion des Kerns ersetzt den Entwurf (seine Lage ist maßgeblich – auch wenn er anders klemmt)
  useEffect(() => {
    if (!grab.current) {
      draftRef.current = null;
      setDraft(null);
    }
  }, [doc.revision, doc.placement.page_index, doc.placement.x, doc.placement.y]);
  useEffect(
    () => () => {
      if (settle.current) clearTimeout(settle.current);
    },
    [],
  );

  const top = (i: number) => layout.tops[i] ?? 0;
  const left = (i: number) => layout.lefts[i] ?? 0;
  const pageH = (i: number) => sizes[i]?.[1] ?? sizes[sizes.length - 1]?.[1] ?? 841.89;
  const pageW = (i: number) => sizes[i]?.[0] ?? sizes[sizes.length - 1]?.[0] ?? 595.28;
  const pageIndexAt = (py: number) => {
    for (let i = 0; i < sizes.length; i++) {
      if (py < top(i) + pageH(i) * scale + 14) return i;
    }
    return sizes.length - 1;
  };

  /** Entwurf aus Zeigerposition (px relativ zur Seitenspalte) mit Feld-Magnet bzw. Einrasten. */
  const compute = (px: number, py: number, dx: number, dy: number): Draft => {
    const i = pageIndexAt(py);
    const [pw, ph] = [pageW(i), pageH(i)];
    const x = (px - left(i)) / scale - dx;
    const yTop = ph - ((py - top(i)) / scale - dy);
    const hit = targetAt(doc.fields, doc.qr, i, x + block.width / 2, yTop - block.height / 2);
    if (hit) return { page: i, x: hit.rect.left, y: hit.rect.top, target: hit, guides: [], snapped: [] };
    const guides = doc.layout.find((l) => l.page === i)?.guides ?? [];
    const s = snapToGuides(x, yTop, block.width, block.height, guides, SNAP_PX / scale);
    const c = clampToPage(s.x, s.y, block.width, block.height, pw, ph);
    return { page: i, x: c.x, y: c.y, target: null, guides: s.guides, snapped: s.snapped };
  };

  const send = (d: Draft) => {
    onSnap(d.snapped);
    if (d.target) void dispatch({ type: 'use_target', target: d.target.target });
    else void dispatch({ type: 'move_block', page: d.page, x: d.x, y: d.y });
    // Antwort des Kerns ersetzt den Entwurf; meldet er keine neue Lage, nach kurzer Zeit zurückfallen
    if (settle.current) clearTimeout(settle.current);
    settle.current = setTimeout(() => {
      settle.current = null;
      if (!grab.current) {
        draftRef.current = null;
        setDraft(null);
      }
    }, 400);
  };

  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    if (!editable || e.button !== 0) return;
    // ein zweiter Finger (Pinch, Scrollen) verschiebt den Block nicht
    if (grab.current && grab.current.pointer !== e.pointerId) return;
    onInteract();
    const el = e.currentTarget;
    try {
      el.setPointerCapture?.(e.pointerId);
    } catch {
      /* synthetischer Zeiger – ohne Capture weiter */
    }
    const r = el.getBoundingClientRect();
    // Greifversatz relativ zur Blockgröße (aus einem Feld heraus wird wieder die normale Größe gezogen)
    const fx = r.width > 0 ? (e.clientX - r.left) / r.width : 0.5;
    const fy = r.height > 0 ? (e.clientY - r.top) / r.height : 0.5;
    if ((e.pointerType || 'mouse') === 'touch') haptic();
    grab.current = { dx: fx * block.width, dy: fy * block.height, sx: e.clientX, sy: e.clientY, moved: false, pointer: e.pointerId };
    el.focus({ preventScroll: true });
    e.preventDefault();
  };

  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    const g = grab.current;
    if (!g || g.pointer !== e.pointerId || !hostRef.current) return;
    if (!g.moved && Math.hypot(e.clientX - g.sx, e.clientY - g.sy) < 3) return;
    if (!g.moved) setActive(true);
    g.moved = true;
    const host = hostRef.current.getBoundingClientRect();
    const next = compute(e.clientX - host.left, e.clientY - host.top, g.dx, g.dy);
    draftRef.current = next;
    setDraft(next);
  };

  const finish = (e: PointerEvent<HTMLDivElement>) => {
    const g = grab.current;
    if (!g || g.pointer !== e.pointerId) return;
    grab.current = null;
    setActive(false);
    try {
      e.currentTarget.releasePointerCapture?.(e.pointerId);
    } catch {
      /* ignorieren */
    }
    const d = draftRef.current;
    if (g.moved && d && e.type !== 'pointercancel') {
      send(d);
    } else {
      draftRef.current = null;
      setDraft(null);
    }
  };

  /** Block an eine Stelle der Seite setzen (Doppelklick: mittig am Zeiger; Doppeltippen: über dem Finger). */
  const placeAt = (clientX: number, clientY: number, pointerType: string) => {
    if (!editable || doc.invisible || !hostRef.current) return;
    onInteract();
    const host = hostRef.current.getBoundingClientRect();
    const g = grabPoint(false, pointerType, { x: 0, top: 0 }, { x: 0, y: 0, width: block.width, height: block.height }, 0);
    send(compute(clientX - host.left, clientY - host.top, g.grabX, g.grabTop));
  };

  const onDoubleClick = (e: MouseEvent<HTMLDivElement>) => placeAt(e.clientX, e.clientY, 'mouse');

  const onPageTap = (e: PointerEvent<HTMLDivElement>) => {
    if (e.pointerType !== 'touch') return;
    const now = Date.now();
    const last = lastTap.current;
    if (last && now - last.time < 350 && Math.hypot(e.clientX - last.x, e.clientY - last.y) < 30) {
      lastTap.current = null;
      placeAt(e.clientX, e.clientY, 'touch');
    } else {
      lastTap.current = { time: now, x: e.clientX, y: e.clientY };
    }
  };

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (!editable) return;
    const fine = e.shiftKey;
    const coarse = e.altKey || e.ctrlKey || e.metaKey;
    const nudge = (dx: number, dy: number) => {
      e.preventDefault();
      onInteract();
      onSnap([]);
      void dispatch({ type: 'nudge_block', dx, dy, fine, coarse });
    };
    const page = (p: number) => {
      e.preventDefault();
      onInteract();
      const target = Math.max(0, Math.min(sizes.length - 1, p));
      if (target !== placementPage) void dispatch({ type: 'change_page', page: target });
    };
    switch (e.key) {
      case 'ArrowLeft':
        return nudge(-1, 0);
      case 'ArrowRight':
        return nudge(1, 0);
      case 'ArrowUp':
        return nudge(0, 1);
      case 'ArrowDown':
        return nudge(0, -1);
      case 'PageUp':
        return page(placementPage - 1);
      case 'PageDown':
        return page(placementPage + 1);
      case 'Home':
        return page(0);
      case 'End':
        return page(sizes.length - 1);
      case 'Delete':
      case 'Backspace':
        // Mehrfachsignatur: den ausgewählten Block entfernen (der letzte bleibt)
        if (removable) {
          e.preventDefault();
          removeActive();
        }
        return;
      default:
    }
  };

  // --- Darstellung --------------------------------------------------------------------------
  const dragging = !!draft && active;
  const stickerPage = draft ? draft.page : placementPage;
  const stickerRect: RectPt | null = draft ? (draft.target?.rect ?? placementRect(draft.x, draft.y, block.width, block.height)) : doc.sticker;
  const inField = draft ? !!draft.target : doc.target.kind !== 'free';
  const overlap = draft ? null : overlapState(doc);
  const box = stickerRect ? rectToBox(stickerRect, pageH(stickerPage), scale) : null;
  // Vorschau in der Darstellungsgröße: im Feld verkleinert der Kern den Block proportional (nie vergrößert).
  // Maßgeblich ist das größere der beiden Achsenverhältnisse (die geringere Verkleinerung), damit keine Achse
  // hochskaliert wird (QR-Platzhalter: Sticker = Rechteck des QR-Bildes, nicht proportional zum Block)
  const fit = box && block.width > 0 && block.height > 0 ? Math.max(box.width / (block.width * scale), box.height / (block.height * scale)) : 1;
  const previewUrl = bridge.blockPreviewUrl(previewScale(scale * dpr, fit), block.preview_revision);
  const currentField = doc.target.kind === 'field' ? doc.target.index : null;
  // Rahmen des Felds, in dem der Block steht bzw. beim Ziehen landen würde
  const current = currentField !== null ? doc.fields.find((f) => f.index === currentField) : undefined;
  const frame: { page: number; rect: RectPt } | null = draft
    ? draft.target && draft.target.target.kind === 'field'
      ? { page: draft.page, rect: draft.target.frame }
      : null
    : current && current.rect && current.page !== null && current.page < sizes.length
      ? { page: current.page, rect: current.rect }
      : null;
  const frameBox = frame ? rectToBox(frame.rect, pageH(frame.page), scale) : null;
  const hitField = draft?.target?.target.kind === 'field' ? draft.target.target.index : null;

  return (
    <div className={`pages${dragging ? ' is-dragging' : ''}`} ref={hostRef} style={{ width: layout.width, height: layout.height }}>
      {sizes.map(([w, h], i) => {
        const isNew = i >= realPages;
        return (
          <div
            key={i}
            className={`page-frame${isNew ? ' is-new' : ''}${placementPage === i ? ' has-sticker' : ''}`}
            style={{ top: top(i), left: left(i), width: w * scale, height: h * scale }}
            role="img"
            aria-label={isNew ? t.t('ng-ui-ws-new-page') : t.t('ng-ui-ws-page-alt', { n: i + 1, total: sizes.length, name: doc.doc.name })}
            data-page={i}
            onDoubleClick={onDoubleClick}
            onPointerUp={onPageTap}
          >
            {isNew ? (
              <div className="new-page-label" aria-hidden="true">
                {t.t('ng-ui-ws-new-page')}
              </div>
            ) : loadPages.has(i) ? (
              <KeyedPageTile url={bridge.pageTileUrl(doc.doc.id, i, tile)} />
            ) : null}
          </div>
        );
      })}

      <div className="pages-overlay">
        {dragging
          ? doc.layout.flatMap((l) =>
              l.page < sizes.length
                ? l.free.map((r, bi) => {
                    const b = rectToBox(r, pageH(l.page), scale);
                    return <div key={`band-${l.page}-${bi}`} className="free-band" style={{ top: top(l.page) + b.top, left: left(l.page) + b.left, width: b.width, height: b.height }} aria-hidden="true" />;
                  })
                : [],
            )
          : null}

        {dragging && draft
          ? draft.guides.map((g, gi) =>
              g.axis === 'x' ? (
                <div key={`g${gi}`} className="snap-guide snap-guide-v" style={{ left: left(draft.page) + g.at * scale, top: top(draft.page), height: pageH(draft.page) * scale }} aria-hidden="true" />
              ) : (
                <div key={`g${gi}`} className="snap-guide snap-guide-h" style={{ top: top(draft.page) + (pageH(draft.page) - g.at) * scale, left: left(draft.page), width: pageW(draft.page) * scale }} aria-hidden="true" />
              ),
            )
          : null}

        {frame && frameBox && !doc.invisible ? (
          <div
            className="field-frame"
            style={{ top: top(frame.page) + frameBox.top, left: left(frame.page) + frameBox.left, width: frameBox.width, height: frameBox.height }}
            aria-hidden="true"
            data-testid="field-frame"
          />
        ) : null}

        {editable
          ? doc.fields.map((f) => {
              // nur angebotene Felder (Kern: `suitable`); ein seitengroßes Feld verdeckte sonst die ganze Seite
              if (!offeredField(f) || f.page >= sizes.length || f.index === currentField || takenFields.has(f.index)) return null;
              const b = rectToBox(f.rect, pageH(f.page), scale);
              return (
                <Button
                  key={`field-${f.index}`}
                  className={`field-marker field-marker-field${dragging ? ' is-target' : ''}${hitField === f.index ? ' is-hit' : ''}`}
                  style={{ top: top(f.page) + b.top, left: left(f.page) + b.left, width: b.width, height: b.height }}
                  aria-label={t.t('ng-ui-ws-marker-field', { name: f.name })}
                  onPress={() => {
                    onInteract();
                    onSnap([]);
                    void dispatch({ type: 'use_target', target: { kind: 'field', index: f.index } });
                  }}
                >
                  <span className="field-marker-pill">
                    <IconPen size={16} />
                    {t.t('ng-ui-ws-marker')}
                  </span>
                </Button>
              );
            })
          : null}
        {editable && doc.qr && doc.target.kind !== 'qr' && !qrTaken && doc.qr.page < sizes.length
          ? (() => {
              const qr = doc.qr;
              const b = rectToBox(qr.rect, pageH(qr.page), scale);
              return (
                <Button
                  className={`field-marker field-marker-qr${dragging ? ' is-target' : ''}`}
                  style={{ top: top(qr.page) + b.top, left: left(qr.page) + b.left, width: b.width, height: b.height }}
                  aria-label={t.t('ng-ui-ws-marker-qr')}
                  onPress={() => {
                    onInteract();
                    onSnap([]);
                    void dispatch({ type: 'use_target', target: { kind: 'qr' } });
                  }}
                >
                  <span className="field-marker-pill">
                    <IconQr size={16} />
                    {t.t('ng-ui-ws-marker')}
                  </span>
                </Button>
              );
            })()
          : null}

        {/* Mehrfachsignatur: die nicht ausgewählten Blöcke an ihrer Stelle – ein Klick wählt den Block aus */}
        {others.map((b) => {
          if (!b.rect) return null;
          const bb = rectToBox(b.rect, pageH(b.page), scale);
          return (
            <Button
              key={`block-${b.index}`}
              className="sticker-other"
              style={{ top: top(b.page) + bb.top, left: left(b.page) + bb.left, width: bb.width, height: bb.height }}
              aria-label={t.t('ng-ui-ws-block-select', { n: b.index + 1, total })}
              isDisabled={!editable || locked}
              data-testid="block-other"
              data-block={b.index}
              onPress={() => {
                onInteract();
                onSnap([]);
                void dispatch({ type: 'select_block', index: b.index });
              }}
            >
              {block.preview_revision > 0 ? <img className="sticker-img" src={previewUrl} alt="" draggable={false} /> : null}
              <span className="sticker-number" aria-hidden="true">
                {b.index + 1}
              </span>
            </Button>
          );
        })}

        {box && !doc.invisible ? (
          <div
            ref={stickerRef}
            className={`sticker${dragging ? ' is-dragging' : ''}${overlap ? ' is-overlap' : ''}${inField ? ' is-in-field' : ''}${locked ? ' is-locked' : ''}`}
            style={{ top: top(stickerPage) + box.top, left: left(stickerPage) + box.left, width: box.width, height: box.height }}
            role="application"
            aria-roledescription={t.t(locked ? 'ng-ui-ws-sticker-role-locked' : 'ng-ui-ws-sticker-role')}
            aria-label={total > 1 ? t.t('ng-ui-ws-sticker-label-n', { n: activeNo, total }) : t.t('ng-ui-ws-sticker-label')}
            aria-describedby={helpId}
            aria-disabled={editable ? undefined : true}
            tabIndex={editable ? 0 : -1}
            data-testid="sticker"
            data-locked={locked || undefined}
            onPointerDown={onPointerDown}
            onPointerMove={onPointerMove}
            onPointerUp={finish}
            onPointerCancel={finish}
            onKeyDown={onKeyDown}
          >
            <span className="sticker-tab" aria-hidden="true">
              {locked ? <IconLock size={14} /> : <IconGrip size={14} />}
              {total > 1 ? t.t('ng-ui-ws-sticker-label-n', { n: activeNo, total }) : t.t('ng-ui-ws-sticker-label')}
            </span>
            {total > 1 ? (
              <span className="sticker-number is-active" aria-hidden="true" data-testid="sticker-number">
                {activeNo}
              </span>
            ) : null}
            {block.preview_revision > 0 ? <img className="sticker-img" src={previewUrl} alt="" draggable={false} /> : null}
            {overlap ? (
              <span className="sticker-flag sticker-flag-warn" aria-hidden="true">
                <IconAlert size={13} />
                {t.t(overlap === 'block' ? 'ng-ui-ws-sticker-block' : overlap === 'widget' ? 'ng-ui-ws-sticker-widget' : 'ng-ui-ws-sticker-overlap')}
              </span>
            ) : inField ? (
              <span className="sticker-flag sticker-flag-ok" aria-hidden="true">
                <IconCheck size={13} />
                {t.t(doc.target.kind === 'qr' && !draft ? 'ng-ui-ws-sticker-in-qr' : 'ng-ui-ws-sticker-in-field')}
              </span>
            ) : null}
            {/* nur beim einen, automatisch platzierten Block – nicht an einem hinzugefügten */}
            {coach && !dragging && total === 1 ? (
              <span className={`sticker-coach${coachAnchor((box.left + box.width / 2) / (pageW(stickerPage) * scale))}`} aria-hidden="true">
                {/* nennt das Ziel der Automatik (Einstellungen › Dokument: QR-Platzhalter bzw. Signaturfeld) */}
                {t.t(doc.target.kind === 'qr' ? 'ng-ui-ws-placed-qr' : doc.target.kind === 'field' ? 'ng-ui-ws-placed-field' : 'ng-ui-ws-placed')}
              </span>
            ) : null}
            <span id={helpId} className="sr-only">
              {t.t(locked ? 'ng-ui-ws-sticker-locked-help' : 'ng-ui-ws-sticker-help', { sign: `${mod}+${enter}` })}
              {removable ? ` ${t.t('ng-ui-ws-sticker-help-remove')}` : ''}
            </span>
          </div>
        ) : null}
        {/* „×“ am ausgewählten Block (Geschwister des Stickers: kein Knopf in der `application`-Rolle) */}
        {box && !doc.invisible && removable && !dragging ? (
          <Button
            className="sticker-remove"
            style={{ top: top(stickerPage) + box.top, left: left(stickerPage) + box.left + box.width }}
            aria-label={t.t('ng-ui-ws-block-remove', { n: activeNo })}
            data-testid="block-remove"
            onPress={removeActive}
          >
            <IconClose size={14} />
          </Button>
        ) : null}
      </div>
    </div>
  );
}
