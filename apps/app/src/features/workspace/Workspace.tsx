/**
 * Arbeitsbereich (Gestalt aus dem Prototyp, ADR 0004 §2.3/§4.3): Miniaturen | durchgehende Seitenansicht
 * mit Zoom | Inspektor. Ein Bildschirm statt vier Schritten: Öffnen → automatisch platziert →
 * „Signieren“ → Signierkarte als Overlay → Fertig. Inhalte ausschließlich aus `ViewState::document`
 * (Phasen `warning`, `editing`, `signing`), Freigaben aus `ViewState::actions`.
 *
 * - Seiten werden nur in Sichtweite geladen; die sichtbaren Seiten gehen als `inspect_pages` (≤ 8) an den
 *   Kern, der dafür Layout (Einrastlinien, freie Flächen) und Prüfung liefert.
 * - Live-Region nach jeder Änderung der Platzierung (`announce` + Einrasten + Prüfergebnis); Umfang und Verzögerung
 *   nach Einstellungen › Barrierefreiheit (ausführlich, erst nach dem Verschieben).
 * - „Signieren“ (Knopf, ⌘↵, Palette): mit gewähltem Signiermittel `sign_with`, sonst öffnet sich das Menü
 *   des Konto-Chips und die Wahl dort signiert (§3.4, §8 F12). Mit „Vor dem Signieren prüfen“ (Barrierefreiheit)
 *   zuerst die Zusammenfassung `SignCheck`. Ohne Signiermittel-Auswahl (nur ein Signiermittel,
 *   `signers.choice_offered`, ADR 0004 Nachtrag 20) ist es vom Kern gewählt; ein Menü gibt es dann nicht.
 * - ⌘/Strg + Plus/Minus/0 zoomt nur die Seitenansicht.
 * - Einstellung „Automatische Positionierung“ (`document.placement_locked`, ADR 0004 Nachtrag 8): der Block ist
 *   gesperrt (kein Ziehen, keine Tastatur, keine Feld-Angebote), bis „Diesmal selbst platzieren“; „Unsichtbar“ und
 *   „Auf neuer Seite“ bleiben bedienbar. Was gesperrt ist, entscheidet der Kern.
 */
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { setInteractionModality, Toolbar, type PressEvent } from 'react-aria-components';
import { useApp } from '../../app/context';
import type { DocumentView, GuideKind } from '../../core/types';
import { Button, IconButton } from '../../ui/controls';
import { useNarrow } from '../../ui/hooks';
import { IconEyeOff, IconFitWidth, IconQr, IconSidebar, IconSignature, IconZoomIn, IconZoomOut } from '../../ui/Icons';
import { accessibilityOf } from '../accessibility';
import { Inspector } from './Inspector';
import { webProfile } from '../../core/web-profile';
import { SignCheck } from './SignCheck';
import { signerChoice } from './signers';
import { Pages } from './Pages';
import { activeBlock, announceText, displayPages, isAutoPlaced, layoutPages, offeredField, PAGE_PAD, PT_TO_PX, visiblePages, ZOOM_STEPS } from './placement';
import { Thumbnails } from './Thumbnails';
import './workspace.css';

/**
 * Block mittig in die Seitenansicht holen. Nur die Seitenansicht rollt: `scrollIntoView` rollte auch die Vorfahren
 * mit, in WKWebView den Inhaltsbereich `.app-main` des Arbeitsbereichs (`overflow: hidden`) – dann stand alles
 * verschoben, ohne dass man zurückrollen konnte (macOS-Testbuild 30.10.2026).
 */
/** Weicher Bildlauf nur ohne „Bewegung reduzieren“ (Systemeinstellung) – sonst springt die Ansicht direkt. */
function scrollBehavior(smooth: boolean): ScrollBehavior {
  if (!smooth) return 'auto';
  return typeof window.matchMedia === 'function' && window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth';
}

function centerInCanvas(el: HTMLElement | null, scroller: HTMLElement | null, smooth = false): void {
  if (!el || !scroller) return;
  const e = el.getBoundingClientRect();
  const s = scroller.getBoundingClientRect();
  const top = Math.max(0, scroller.scrollTop + (e.top - s.top) - (s.height - e.height) / 2);
  const left = Math.max(0, scroller.scrollLeft + (e.left - s.left) - (s.width - e.width) / 2);
  if (typeof scroller.scrollTo === 'function') scroller.scrollTo({ top, left, behavior: scrollBehavior(smooth) });
  else {
    scroller.scrollTop = top;
    scroller.scrollLeft = left;
  }
}

function clamp(v: number, min: number, max: number): number {
  return Math.min(Math.max(v, min), max);
}

export function Workspace({ doc }: { doc: DocumentView }) {
  return <WorkspaceView key={doc.doc.id} doc={doc} />;
}

function WorkspaceView({ doc }: { doc: DocumentView }) {
  const { t, view, dispatch, announce, registerSign, overlay } = useApp();
  const narrow = useNarrow();
  const sizes = useMemo(() => displayPages(doc), [doc]);
  const maxW = Math.max(...sizes.map((s) => s[0]));
  const [zoom, setZoom] = useState<number | null>(null);
  const [fitScale, setFitScale] = useState(1.15);
  const [fitReady, setFitReady] = useState(typeof ResizeObserver === 'undefined');
  const [current, setCurrent] = useState(doc.placement.page_index);
  const [pagesOpen, setPagesOpen] = useState(!narrow);
  // Sprechblase „automatisch … platziert“ nur, solange die Lage aus der Automatik stammt (frei, QR-Platzhalter
  // bzw. Signaturfeld der Platzhaltersuche; ohne `detection` wie bisher nach `origin`)
  const autoPlaced = isAutoPlaced(doc);
  const [coach, setCoach] = useState(autoPlaced && doc.phase === 'editing');
  const [chip, setChip] = useState<{ open: boolean; sign: boolean }>({ open: false, sign: false });
  const [viewport, setViewport] = useState({ top: 0, height: 0 });
  const scrollRef = useRef<HTMLDivElement>(null);
  const hostRef = useRef<HTMLDivElement>(null);
  const stickerRef = useRef<HTMLDivElement>(null);
  const toolbarRef = useRef<HTMLDivElement>(null);
  const lastSnap = useRef<GuideKind[]>([]);
  const editable = view.actions.edit_placement && doc.phase === 'editing';
  // gesperrt durch „Automatische Positionierung“: Optionen bedienbar, der Block nicht verschiebbar (Kern)
  const movable = editable && !doc.placement_locked;

  const scale = zoom ? (zoom / 100) * PT_TO_PX : fitScale;
  const layout = useMemo(() => layoutPages(sizes, scale), [sizes, scale]);
  const zoomPct = Math.round((scale / PT_TO_PX) * 100);

  // Seitenbreite einpassen
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el || typeof ResizeObserver === 'undefined') return undefined;
    const ro = new ResizeObserver(() => {
      const w = el.clientWidth;
      if (w > 0) {
        setFitScale(clamp((w - 2 * PAGE_PAD - 8) / maxW, 0.35, 1.45));
        setFitReady(true);
      }
      setViewport({ top: el.scrollTop, height: el.clientHeight });
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [maxW]);

  // Hinweisleiste über den Seiten (unsichtbar signieren | Signaturfeld gefunden | QR-Platzhalter); genannt wird nur
  // ein angebotenes Feld (Kern: `suitable`, ADR 0004 Nachtrag 16) – kein seitengroßes oder 3 pt schmales
  const unusedField = movable && doc.target.kind !== 'field' ? doc.fields.find(offeredField) : undefined;
  const banner = doc.invisible ? 'invisible' : unusedField ? 'field' : doc.target.kind === 'qr' ? 'qr' : null;
  // Erscheint oder verschwindet die Leiste (z. B. Block aus dem Signaturfeld gezogen), verschöbe sich die
  // Seitenansicht um ihre Höhe – auf dem Telefon (Leiste bis ~130 px) läge der gerade abgelegte Block dann
  // unter dem Bottom-Sheet. Deshalb den Bildlauf um die Höhenänderung ausgleichen (vor den Effekten, die den
  // Block bewusst in den Blick holen).
  const bannerRef = useRef<HTMLElement | null>(null);
  const setBannerRef = useCallback((el: HTMLElement | null) => {
    bannerRef.current = el;
  }, []);
  const bannerHeight = useRef<number | null>(null);
  useLayoutEffect(() => {
    const h = bannerRef.current?.getBoundingClientRect().height ?? 0;
    const before = bannerHeight.current;
    bannerHeight.current = h;
    const el = scrollRef.current;
    if (before === null || !el || h === before) return;
    el.scrollTop = Math.max(0, el.scrollTop + h - before);
  }, [banner]);

  // Zoomen behält die Lesestelle: die Mitte des sichtbaren Ausschnitts bleibt in der Mitte (sonst stünde nach
  // „Vergrößern“ ein anderer Teil der Seite bzw. eine andere Seite im Bild). Vor dem Effekt, der beim Öffnen den
  // Block in den Blick holt – der gewinnt in der ersten Sekunde.
  const lastScale = useRef(scale);
  useLayoutEffect(() => {
    const before = lastScale.current;
    lastScale.current = scale;
    const el = scrollRef.current;
    if (!el || !fitReady || before === scale || before <= 0) return;
    const ratio = scale / before;
    el.scrollTop = Math.max(0, (el.scrollTop + el.clientHeight / 2) * ratio - el.clientHeight / 2);
    el.scrollLeft = Math.max(0, (el.scrollLeft + el.clientWidth / 2) * ratio - el.clientWidth / 2);
  }, [scale, fitReady]);

  // Beim Öffnen zum Signaturblock scrollen (er steht meist auf der letzten Seite)
  const mountedAt = useRef(Date.now());
  const didScroll = useRef(false);
  useLayoutEffect(() => {
    if (!fitReady) return;
    if (didScroll.current && Date.now() - mountedAt.current > 1000) return;
    didScroll.current = true;
    centerInCanvas(stickerRef.current, scrollRef.current);
  }, [fitReady, fitScale]);

  // Von der Automatik in ein anderes Ziel versetzt (geänderte Platzhaltersuche in den Einstellungen ›
  // Dokument): Block in den Blick holen und die Sprechblase mit dem neuen Ziel erneut zeigen
  const autoKey = autoPlaced && doc.detection ? `${doc.target.kind}:${doc.target.kind === 'field' ? doc.target.index : ''}:${doc.placement.page_index}` : null;
  const lastAutoKey = useRef(autoKey);
  useLayoutEffect(() => {
    if (autoKey === lastAutoKey.current) return;
    lastAutoKey.current = autoKey;
    if (!autoKey || doc.phase !== 'editing') return;
    centerInCanvas(stickerRef.current, scrollRef.current, true);
    setCoach(true);
  }, [autoKey, doc.phase]);

  // Hinweis „automatisch platziert“ nach einigen Sekunden ausblenden – er verdeckt sonst Text
  useEffect(() => {
    if (!coach) return undefined;
    const timer = setTimeout(() => setCoach(false), 8000);
    return () => clearTimeout(timer);
  }, [coach]);

  // Miniaturen folgen der Layoutbreite (schmal: eingeklappt, breit: sichtbar)
  useEffect(() => setPagesOpen(!narrow), [narrow]);

  // Sichtbare Seiten: Kacheln laden (± 1 Seite) und Layout beim Kern anfragen (≤ 8, idempotent)
  const visible = useMemo(() => {
    const v = visiblePages(layout, sizes, scale, viewport.top, viewport.height || 800, 0).filter((i) => i < doc.doc.page_count);
    if (v.length === 0) v.push(Math.min(doc.placement.page_index, doc.doc.page_count - 1));
    return v;
  }, [layout, sizes, scale, viewport, doc.doc.page_count, doc.placement.page_index]);
  const loadPages = useMemo(() => {
    const s = new Set<number>();
    for (const i of visible) for (const j of [i - 1, i, i + 1]) if (j >= 0 && j < doc.doc.page_count) s.add(j);
    if (doc.placement.page_index < doc.doc.page_count) s.add(doc.placement.page_index);
    return s;
  }, [visible, doc.doc.page_count, doc.placement.page_index]);
  const inspectKey = visible.slice(0, 8).join(',');
  useEffect(() => {
    if (!editable) return undefined;
    const pages = inspectKey.split(',').filter(Boolean).map(Number);
    const timer = setTimeout(() => void dispatch({ type: 'inspect_pages', pages }), 120);
    return () => clearTimeout(timer);
  }, [inspectKey, editable, dispatch]);

  const onScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    setViewport({ top: el.scrollTop, height: el.clientHeight });
    const probe = el.scrollTop + el.clientHeight / 3;
    let idx = 0;
    layout.tops.forEach((top, i) => {
      if (top <= probe) idx = i;
    });
    if (idx !== current) setCurrent(idx);
  };

  const scrollToPage = (i: number, pointerType: PressEvent['pointerType']) => {
    const el = scrollRef.current;
    const top = layout.tops[i];
    if (el && top !== undefined) el.scrollTo?.({ top: top - 12, behavior: scrollBehavior(true) });
    setCurrent(i);
    // Schmal liegen die Miniaturen über der Seite – nach der Auswahl schließen, Fokus auf den Umschalter (die
    // gewählte Miniatur verschwindet mit der Leiste). Nur nach Tastatur mit sichtbarem Fokus; nach Finger, Maus bzw.
    // Bildschirmleser ohne Fokusrahmen und ohne Tooltip – der verdeckte sonst die Wortmarke (Mobile-Check
    // 16.10.2026, R10). Dafür die Modalität „Zeiger“ (React Aria: `data-focus-visible`, Tooltip beim Fokus; ein
    // Tippen ohne Kontaktfläche gilt dort als „virtuell“) und `focusVisible: false` (`:focus-visible` des Browsers
    // bei Fokus per Skript). Bildschirmleser zeichnen ihren eigenen Rahmen.
    if (narrow) {
      setPagesOpen(false);
      const toggle = toolbarRef.current?.querySelector<HTMLElement>('button');
      if (pointerType === 'keyboard') toggle?.focus();
      else {
        setInteractionModality('pointer');
        toggle?.focus({ focusVisible: false });
      }
    }
  };

  // Live-Region nach jeder Änderung der Platzierung (nicht beim Öffnen); Umfang und Verzögerung laut Einstellungen ›
  // Barrierefreiheit („erst nach dem Verschieben“: erst, wenn der Block kurz stillsteht)
  const a11y = accessibilityOf(view);
  const text = announceText(t, doc, lastSnap.current, a11y.announce);
  const announceDelay = a11y.announce_after_move ? 900 : 250;
  const firstText = useRef(true);
  const announced = useRef(false);
  useEffect(() => {
    if (firstText.current) {
      firstText.current = false;
      return undefined;
    }
    const timer = setTimeout(() => {
      announced.current = true;
      announce(text);
    }, announceDelay);
    return () => clearTimeout(timer);
  }, [doc.revision, text, announce, announceDelay]);
  // Dokument geschlossen bzw. gewechselt (eigene Ansicht je Dokument): die Lage dieses Dokuments verlässt die
  // Live-Region – sonst stand dort nach dem Öffnen eines anderen Stapeldokuments noch die vorige (Mobile-Check
  // 16.10.2026)
  useEffect(
    () => () => {
      if (announced.current) announce('');
    },
    [announce],
  );

  // Sticker beim Seitenwechsel im Blick halten
  const page = doc.placement.page_index;
  const prevPage = useRef(page);
  // Mehrfachsignatur: auch nach dem Auswählen bzw. Hinzufügen eines Blocks (auf derselben Seite) in den Blick
  const selectedBlock = activeBlock(doc);
  const prevBlock = useRef(selectedBlock);
  useEffect(() => {
    if (prevPage.current !== page || prevBlock.current !== selectedBlock) centerInCanvas(stickerRef.current, scrollRef.current, true);
    prevPage.current = page;
    prevBlock.current = selectedBlock;
  }, [page, selectedBlock]);

  const interact = useCallback(() => setCoach(false), []);
  const onSnap = useCallback((s: GuideKind[]) => {
    lastSnap.current = s;
  }, []);

  // „Signieren“: mit gewähltem Signiermittel sofort, sonst das Menü des Konto-Chips (Wahl signiert) – ohne
  // Signiermittel-Auswahl gibt es kein Menü (Nachtrag 20)
  const choice = signerChoice(view);
  const [checking, setChecking] = useState(false);
  const proceed = useCallback(() => {
    if (!view.actions.sign) return;
    setCoach(false);
    if (view.signers.selected) void dispatch({ type: 'sign_with', backend: view.signers.selected });
    else if (choice) setChip({ open: true, sign: true });
  }, [view.actions.sign, view.signers.selected, choice, dispatch]);
  const check = a11y.check_before_sign && doc.phase === 'editing';
  const sign = useCallback(() => {
    if (!view.actions.sign) return;
    if (check) {
      setCoach(false);
      setChecking(true);
    } else proceed();
  }, [view.actions.sign, check, proceed]);
  useEffect(() => {
    registerSign(sign);
    return () => registerSign(null);
  }, [sign, registerSign]);

  const zoomBy = useCallback(
    (dir: 1 | -1) => {
      const cur = zoomPct;
      const next = dir > 0 ? ZOOM_STEPS.find((z) => z > cur + 1) : [...ZOOM_STEPS].reverse().find((z) => z < cur - 1);
      setZoom(next ?? (dir > 0 ? 200 : 50));
    },
    [zoomPct],
  );

  // Zoom-Kürzel (Strg/⌘ + Plus/Minus/0) – nur ohne offenes Overlay bzw. offenen Dialog (Hilfe, Kurzbefehle, Rückfragen)
  const zoomState = useRef({ zoomBy, blocked: false });
  zoomState.current = { zoomBy, blocked: !!overlay || view.settings !== null || view.step.step === 'signing' };
  useEffect(() => {
    // Im Browser gehören diese Kürzel dem Seitenzoom des Browsers (Barrierefreiheit) – das Dokument zoomt man
    // dort über die Knöpfe der Werkzeugleiste
    if (webProfile() !== null) return undefined;
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.altKey || zoomState.current.blocked || document.querySelector('[data-modal-layer]') !== null) return;
      if (e.key === '+' || e.key === '=') {
        e.preventDefault();
        zoomState.current.zoomBy(1);
      } else if (e.key === '-') {
        e.preventDefault();
        zoomState.current.zoomBy(-1);
      } else if (e.key === '0') {
        e.preventDefault();
        setZoom(null);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  return (
    <div className={`workspace${pagesOpen ? ' pages-open' : ''}`} data-phase={doc.phase}>
      {pagesOpen ? <Thumbnails doc={doc} sizes={sizes} current={current} onSelect={scrollToPage} /> : null}

      <h1 className="sr-only" id="main-heading" tabIndex={-1} data-main-heading="">
        {doc.doc.name}
      </h1>
      <section className="canvas" aria-label={t.t('ng-ui-ws-canvas', { name: doc.doc.name })}>
        {/* React-Aria-Toolbar: Pfeiltasten zwischen den Knöpfen, Tab verlässt die Leiste (ARIA-Muster „toolbar“) */}
        <Toolbar className="canvas-toolbar" aria-label={t.t('ng-ui-ws-toolbar')} ref={toolbarRef}>
          <IconButton label={t.t('ng-ui-ws-toggle-pages')} onPress={() => setPagesOpen((v) => !v)} aria-pressed={pagesOpen}>
            <IconSidebar size={18} />
          </IconButton>
          {/* Telefonbreite: der Dateiname steht hier, in der Kopfzeile fehlt ihm der Platz (shell.css) */}
          <span className="toolbar-doc-name" aria-hidden="true">
            {doc.doc.name}
          </span>
          <span className="page-indicator" aria-live="off">
            <span className="page-indicator-long">{t.t('ng-ui-ws-page-indicator', { n: current + 1, total: sizes.length })}</span>
            {/* Telefonbreite: nur die Zahlen, der Platz gehört dem Dateinamen */}
            <span className="page-indicator-short" aria-hidden="true">
              {current + 1} / {sizes.length}
            </span>
          </span>
          <span className="toolbar-spacer" />
          <IconButton label={t.t('ng-ui-ws-zoom-out')} onPress={() => zoomBy(-1)} isDisabled={zoomPct <= 50}>
            <IconZoomOut size={18} />
          </IconButton>
          {/* Name an einem generischen span ignorieren Screenreader (WCAG 4.1.2): gelesen wird der Text */}
          <span className="zoom-value">
            <span aria-hidden="true">{zoomPct} %</span>
            <span className="sr-only">{t.t('ng-ui-ws-zoom-value', { value: zoomPct })}</span>
          </span>
          <IconButton label={t.t('ng-ui-ws-zoom-in')} onPress={() => zoomBy(1)} isDisabled={zoomPct >= 200}>
            <IconZoomIn size={18} />
          </IconButton>
          <IconButton label={t.t('ng-ui-ws-fit')} onPress={() => setZoom(null)} aria-pressed={zoom === null}>
            <IconFitWidth size={18} />
          </IconButton>
        </Toolbar>

        {banner === 'invisible' ? (
          <p className="canvas-banner" ref={setBannerRef}>
            <IconEyeOff size={16} />
            {t.t('ng-ui-ws-invisible')}
          </p>
        ) : banner === 'field' && unusedField ? (
          <div className="canvas-banner canvas-banner-field" ref={setBannerRef}>
            <IconSignature size={16} />
            <span>
              <strong>{t.t('ng-ui-ws-field-found')}:</strong> {t.t('ng-ui-ws-field-banner', { name: unusedField.name, page: (unusedField.page ?? 0) + 1 })}
            </span>
            <Button
              variant="secondary"
              size="sm"
              onPress={() => {
                setCoach(false);
                lastSnap.current = [];
                void dispatch({ type: 'use_target', target: { kind: 'field', index: unusedField.index } });
              }}
            >
              {t.t('ng-ui-ws-field-use')}
            </Button>
          </div>
        ) : banner === 'qr' ? (
          <p className="canvas-banner" ref={setBannerRef}>
            <IconQr size={16} />
            {t.t('ng-ui-ws-qr-banner')}
          </p>
        ) : null}

        <div className="canvas-scroll" ref={scrollRef} onScroll={onScroll}>
          <Pages
            doc={doc}
            sizes={sizes}
            scale={scale}
            layout={layout}
            loadPages={loadPages}
            editable={movable}
            locked={editable && doc.placement_locked}
            coach={coach}
            hostRef={hostRef}
            stickerRef={stickerRef}
            onInteract={interact}
            onSnap={onSnap}
          />
        </div>
      </section>

      <Inspector
        doc={doc}
        visiblePage={Math.min(current, Math.max(0, doc.doc.page_count - (doc.new_page ? 0 : 1)))}
        chipOpen={chip.open}
        signMode={chip.sign}
        onChipOpenChange={(open) => setChip({ open, sign: open ? chip.sign : false })}
        onSign={sign}
      />
      {checking ? (
        <SignCheck
          doc={doc}
          onCancel={() => setChecking(false)}
          onSign={() => {
            setChecking(false);
            proceed();
          }}
        />
      ) : null}
    </div>
  );
}
