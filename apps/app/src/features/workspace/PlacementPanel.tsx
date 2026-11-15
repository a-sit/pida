/**
 * „Platzierung der Signatur“ ohne Vorschau (docs/analyse/06, Prototyp `prototypes/a11y-positionierung`): Methode
 * wählen – selbst platzieren, automatisch, Platzhalter im Dokument, Position in Millimetern (linke obere Ecke ab der
 * linken unteren Seitenecke, wie PDF-AS), freie Fläche – und die aktuelle Lage als Text. Die Methoden sind
 * Schaltflächen und wirken sofort (ADR 0004 Nachtrag 27): „Automatisch“, „Platzhalter“ und „Freie Fläche“ setzen
 * alle Blöcke (`place_blocks`), „Rückgängig“ stellt den Stand davor wieder her (`undo_placement`); „Selbst
 * platzieren“ stellt nichts um und wird von selbst gezeigt, sobald der Block gezogen oder mit den Pfeiltasten
 * verschoben wird. Schaltflächen statt Radio-Gruppe, weil dort schon die Pfeiltasten auswählen würden. Für alle da: mit „Barrierefreie Bedienung“ oben im Inspektor und aufgeklappt, sonst unter
 * „Platzierung“ als „Ohne Vorschau platzieren …“ (aufgeklappt, wenn die Platzierung mit der Methodenwahl beginnt).
 *
 * Der Kern bleibt maßgeblich: Ziele über `use_target`, Lage über `move_block` (er klemmt an die Seite),
 * automatisch über `auto_place`; die Prüfung hier sagt nur früh, was nicht passt. Nachtrag zu ADR 0004 D1 („keine
 * Zahlenfelder“): Zahlenfelder gibt es nur in diesem Panel.
 */
import { useEffect, useId, useMemo, useRef, useState, type ReactNode } from 'react';
import { Disclosure, DisclosurePanel, Heading, Button as AriaButton } from 'react-aria-components';
import { useApp } from '../../app/context';
import type { AccessibilitySettings, AnnounceDetail, DocumentView, KeyboardStep, RectPt } from '../../core/types';
import { Button, RadioCards, Segmented, SwitchField, TextInput } from '../../ui/controls';
import { IconChevronRight, IconGrip, IconQr, IconRefresh, IconSettings, IconSliders, IconTarget, IconWand } from '../../ui/Icons';
import { accessibilityOf, formatMm, parseMm, PT_PER_MM } from '../accessibility';
import { announceText, blockTotal, displayPages, isAutoPlaced, offeredField, zoneText } from './placement';

type Method = 'manual' | 'auto' | 'placeholder' | 'coords' | 'free';

function initialMethod(doc: DocumentView): Method {
  if (doc.target.kind !== 'free') return 'placeholder';
  return isAutoPlaced(doc) ? 'auto' : 'manual';
}

/** Zeit, in der eine Änderung der Lage als Folge des eigenen Klicks gilt (Kern rechnet im Hintergrund). */
const OWN_CHANGE_MS = 4000;

export function PlacementPanel({ doc, editable, embedded }: { doc: DocumentView; editable: boolean; embedded: boolean }) {
  const { t, view } = useApp();
  const a = accessibilityOf(view);
  const headingId = useId();
  const body = <PanelBody doc={doc} editable={editable} a={a} focusOnStart={a.start === 'methods'} />;
  if (!embedded) {
    // „Barrierefreie Bedienung“: eigener Abschnitt ganz oben im Inspektor
    return (
      <section className="insp-section insp-pos-panel" aria-labelledby={headingId} data-testid="placement-panel">
        <h2 className="insp-heading" id={headingId}>
          {t.t('ng-ui-insp-pos-title')}
        </h2>
        {body}
      </section>
    );
  }
  return (
    <Disclosure className="details insp-pos-disclosure" defaultExpanded={a.start === 'methods'} data-testid="placement-panel">
      <Heading level={3} className="details-heading">
        <AriaButton slot="trigger" className="details-trigger">
          <IconChevronRight size={16} className="details-caret" />
          {t.t('ng-ui-insp-pos-open')}
        </AriaButton>
      </Heading>
      <DisclosurePanel className="details-panel">{body}</DisclosurePanel>
    </Disclosure>
  );
}

interface MethodOption {
  value: Method;
  label: string;
  description: string;
  icon: ReactNode;
  disabled?: boolean;
}

function PanelBody({ doc, editable, a, focusOnStart }: { doc: DocumentView; editable: boolean; a: AccessibilitySettings; focusOnStart: boolean }) {
  const { t, view, dispatch, openSettings } = useApp();
  const [method, setMethod] = useState<Method>(() => initialMethod(doc));
  const ref = useRef<HTMLDivElement>(null);
  const groupId = useId();
  const hintId = useId();
  const sizes = useMemo(() => displayPages(doc), [doc]);
  const fields = doc.fields.filter(offeredField);
  const placeholders = fields.length + (doc.qr ? 1 : 0);
  const qrSearch = doc.detection?.qr_search ?? true;
  const blocks = blockTotal(doc);
  // Änderungen der Lage, die nicht aus diesem Panel kommen (Ziehen, Pfeiltasten): Anzeige „Selbst platzieren“
  const ownUntil = useRef(0);
  const own = () => {
    ownUntil.current = Date.now() + OWN_CHANGE_MS;
  };
  const revision = useRef(doc.revision);
  useEffect(() => {
    if (revision.current === doc.revision) return;
    revision.current = doc.revision;
    if (Date.now() > ownUntil.current && doc.target.kind === 'free') setMethod((m) => (m === 'coords' ? m : 'manual'));
  }, [doc.revision]);

  // Platzierung beginnt mit der Methodenwahl: Fokus einmal je Dokument auf die gezeigte Methode
  useEffect(() => {
    if (!focusOnStart || !editable) return;
    const button = ref.current?.querySelector<HTMLButtonElement>('.insp-pos-method[aria-pressed="true"]') ?? ref.current?.querySelector<HTMLButtonElement>('.insp-pos-method');
    button?.focus();
    // nur beim Einhängen (je Dokument eine eigene Ansicht)
  }, []);

  // Layout der letzten, der aktuellen und der ersten Seiten anfragen (≤ 8, der Kern rechnet im Hintergrund): für
  // die Liste der freien Flächen und dafür, „Freie Fläche“ mit Grund auszugrauen
  const wanted = useMemo(() => {
    const n = doc.doc.page_count;
    const pages = [doc.placement.page_index, n - 1, ...Array.from({ length: n }, (_, i) => i)].filter((p) => p >= 0 && p < n);
    return [...new Set(pages)].slice(0, 8);
  }, [doc.doc.page_count, doc.placement.page_index]);
  // erst, wenn jemand das Panel benutzt (Fokus bzw. Zeiger auf den Methoden), und beim Wechsel zu „Freie Fläche“
  const asked = useRef(false);
  const inspect = () => {
    if (asked.current || !editable) return;
    asked.current = true;
    void dispatch({ type: 'inspect_pages', pages: wanted });
  };
  useEffect(() => {
    if (method !== 'free' || !editable) return;
    asked.current = true;
    void dispatch({ type: 'inspect_pages', pages: wanted });
  }, [method]);
  const known = wanted.every((p) => doc.layout.some((l) => l.page === p));
  const noFree = known && wanted.length >= doc.doc.page_count && !doc.new_page && doc.layout.every((l) => l.free.length === 0);

  const setA = (patch: Partial<AccessibilitySettings>) => void dispatch({ type: 'set_accessibility', accessibility: { ...a, ...patch } });

  const options: MethodOption[] = [
    { value: 'manual', label: t.t('ng-ui-insp-pos-manual'), description: t.t('ng-ui-insp-pos-manual-desc'), icon: <IconGrip size={16} /> },
    {
      value: 'auto',
      label: t.t('ng-ui-insp-pos-auto'),
      description: t.t(blocks > 1 ? 'ng-ui-insp-pos-auto-desc-all' : 'ng-ui-insp-pos-auto-desc'),
      icon: <IconWand size={16} />,
    },
    {
      value: 'placeholder',
      label: t.t('ng-ui-insp-pos-placeholder'),
      // Suche aus: ehrlich „nicht gesucht“ statt „keiner gefunden“ (Kern: `detection.qr_search`, Nachtrag 30)
      description: placeholders === 0 && !qrSearch ? t.t('ng-ui-insp-pos-placeholder-desc-off') : t.t('ng-ui-insp-pos-placeholder-desc', { n: placeholders }),
      icon: <IconQr size={16} />,
      disabled: placeholders === 0,
    },
    { value: 'coords', label: t.t('ng-ui-insp-pos-coords'), description: t.t('ng-ui-insp-pos-coords-desc'), icon: <IconSliders size={16} /> },
    {
      value: 'free',
      label: t.t('ng-ui-insp-pos-free'),
      description: t.t(noFree ? 'ng-ui-insp-pos-free-desc-none' : 'ng-ui-insp-pos-free-desc'),
      icon: <IconTarget size={16} />,
      disabled: noFree,
    },
  ];
  // Ein Klick wirkt sofort – auf alle Blöcke („Selbst platzieren“ und „Millimeter“ stellen nichts um)
  const choose = (m: Method) => {
    setMethod(m);
    if (m === 'auto' || m === 'placeholder' || m === 'free') {
      own();
      void dispatch({ type: 'place_blocks', method: m === 'placeholder' ? 'placeholders' : m });
    }
  };
  const undo = () => {
    own();
    void dispatch({ type: 'undo_placement' });
    setMethod('manual');
  };

  return (
    <div className="insp-pos" ref={ref}>
      <div className="insp-pos-methods" role="group" aria-labelledby={groupId} aria-describedby={hintId} onFocus={inspect} onPointerEnter={inspect}>
        <p className="field-label" id={groupId}>
          {t.t('ng-ui-insp-pos-method')}
        </p>
        <p className="insp-hint" id={hintId}>
          {blocks > 1 ? t.t('ng-ui-insp-pos-method-hint-all', { n: blocks }) : t.t('ng-ui-insp-pos-method-hint')}
        </p>
        {options.map((o) => (
          <AriaButton
            key={o.value}
            className="insp-pos-method"
            aria-pressed={method === o.value}
            isDisabled={!editable || o.disabled}
            onPress={() => choose(o.value)}
            data-testid={`placement-method-${o.value}`}
          >
            <span className="radio-card-icon">{o.icon}</span>
            <span className="radio-card-text">
              <span className="radio-card-label">{o.label}</span>
              <span className="radio-card-desc">{o.description}</span>
            </span>
          </AriaButton>
        ))}
      </div>

      {doc.can_undo_placement ? (
        <div className="insp-pos-undo">
          <Button variant="secondary" size="sm" onPress={undo} isDisabled={!editable} aria-description={t.t('ng-ui-insp-pos-undo-desc')} data-testid="placement-undo">
            <IconRefresh size={15} />
            {t.t('ng-ui-insp-pos-undo')}
          </Button>
        </div>
      ) : null}

      <div className="insp-pos-detail">
        {method === 'manual' ? (
          <ManualDetail editable={editable} />
        ) : method === 'placeholder' ? (
          <PlaceholderDetail doc={doc} editable={editable} sizes={sizes} onUse={own} />
        ) : method === 'coords' ? (
          <CoordsDetail doc={doc} editable={editable} sizes={sizes} onUse={own} />
        ) : method === 'free' ? (
          <FreeDetail doc={doc} editable={editable} sizes={sizes} onUse={own} />
        ) : null}
      </div>
      {/* ohne Platzhalter ist die Methode ausgegraut: der Weg zur Platzhaltersuche bleibt erreichbar */}
      {placeholders === 0 && method !== 'placeholder' ? <PlaceholderHelp doc={doc} /> : null}

      <div className="insp-pos-current" data-testid="placement-current">
        <p className="insp-pos-current-label">{t.t('ng-ui-insp-pos-current')}</p>
        <p>{announceText(t, doc, [], 'detailed')}</p>
      </div>

      {a.mode ? (
        <Disclosure className="details insp-pos-quick">
          <Heading level={3} className="details-heading">
            <AriaButton slot="trigger" className="details-trigger">
              <IconChevronRight size={16} className="details-caret" />
              {t.t('ng-ui-insp-pos-quick')}
            </AriaButton>
          </Heading>
          <DisclosurePanel className="details-panel stack-3">
            <Segmented<AnnounceDetail>
              label={t.t('ng-ui-set-a11y-announce')}
              size="sm"
              value={a.announce}
              onChange={(announce) => setA({ announce })}
              options={[
                { value: 'short', label: t.t('ng-ui-set-a11y-announce-short') },
                { value: 'detailed', label: t.t('ng-ui-set-a11y-announce-detailed') },
              ]}
            />
            <SwitchField label={t.t('ng-ui-set-a11y-after-move')} isSelected={a.announce_after_move} onChange={(announce_after_move) => setA({ announce_after_move })} />
            {view.platform.mobile ? null : (
              <Segmented<KeyboardStep>
                label={t.t('ng-ui-set-a11y-step')}
                size="sm"
                value={a.keyboard_step}
                onChange={(keyboard_step) => setA({ keyboard_step })}
                options={[
                  { value: 'standard', label: t.t('ng-ui-insp-pos-step-standard') },
                  { value: 'mm1', label: t.t('ng-ui-set-a11y-step-mm', { mm: 1 }) },
                  { value: 'mm5', label: t.t('ng-ui-set-a11y-step-mm', { mm: 5 }) },
                  { value: 'mm10', label: t.t('ng-ui-set-a11y-step-mm', { mm: 10 }) },
                ]}
              />
            )}
          </DisclosurePanel>
        </Disclosure>
      ) : null}
      <Button variant="ghost" size="sm" onPress={() => openSettings('accessibility')} isDisabled={!view.actions.settings}>
        <IconSettings size={15} />
        {t.t('ng-ui-insp-pos-all-settings')}
      </Button>
    </div>
  );
}

/** Fokus auf den Signaturblock in der Vorschau (dort wirken die Pfeiltasten). */
function focusBlock() {
  const el = document.querySelector<HTMLElement>('[data-testid="sticker"]');
  // nur die Seitenansicht rollen (nicht `.app-main`, siehe Workspace `centerInCanvas`)
  const scroller = el?.closest<HTMLElement>('.canvas-scroll');
  if (el && scroller) {
    const e = el.getBoundingClientRect();
    const s = scroller.getBoundingClientRect();
    scroller.scrollTop = Math.max(0, scroller.scrollTop + (e.top - s.top) - (s.height - e.height) / 2);
  }
  el?.focus({ preventScroll: true });
}

/** „Selbst platzieren“: nichts wird umgestellt; der Weg zum Block in der Vorschau. */
function ManualDetail({ editable }: { editable: boolean }) {
  const { t } = useApp();
  return (
    <div className="stack-3">
      <p className="insp-hint">{t.t('ng-ui-insp-pos-manual-hint')}</p>
      <div>
        <Button variant="secondary" size="sm" onPress={focusBlock} isDisabled={!editable}>
          <IconTarget size={15} />
          {t.t('ng-ui-insp-pos-to-block')}
        </Button>
      </div>
    </div>
  );
}

/** Hinweis auf die Platzhaltersuche und „QR-Platzhalter erstellen“. */
function PlaceholderHelp({ doc }: { doc: DocumentView }) {
  const { t, openSettings, view, setOverlay } = useApp();
  return (
    <>
      {/* QR-Code-Platzhalter findet PIDA nur mit der Platzhaltersuche (Einstellungen › Dokument) */}
      {!doc.qr ? (
        <div className="insp-pos-hint">
          <p className="insp-hint">{t.t('ng-ui-insp-pos-qr-search-hint')}</p>
          <Button variant="ghost" size="sm" onPress={() => openSettings('document')} isDisabled={!view.actions.settings}>
            <IconSettings size={15} />
            {t.t('ng-ui-insp-pos-settings-doc')}
          </Button>
        </div>
      ) : null}
      {/* wer selbst Dokumente vorbereitet: Platzhalter für andere erstellen (ADR 0004 Nachtrag 24) */}
      <div>
        <Button variant="ghost" size="sm" onPress={() => setOverlay('qr')}>
          <IconQr size={15} />
          {t.t('ng-ui-shell-cmd-qr')}
        </Button>
      </div>
    </>
  );
}

function pageSize(sizes: [number, number][], page: number): [number, number] {
  return sizes[page] ?? sizes[sizes.length - 1] ?? [595.28, 841.89];
}

function where(t: ReturnType<typeof useApp>['t'], sizes: [number, number][], page: number, rect: RectPt): string {
  const [pw, ph] = pageSize(sizes, page);
  return t.t('ng-ui-insp-pos-item-where', { page: page + 1, zone: zoneText(t, rect, pw, ph) });
}

/**
 * Gefundene Platzhalter als Auswahlliste (Nummer, Seite, Lage); ohne Fund nur der Hinweis – eine Nummer einzugeben,
 * ergäbe nichts, was die Liste nicht schon zeigt.
 */
function PlaceholderDetail({ doc, editable, sizes, onUse }: { doc: DocumentView; editable: boolean; sizes: [number, number][]; onUse: () => void }) {
  const { t, dispatch } = useApp();
  const fields = doc.fields.filter(offeredField);
  const qr = doc.qr;
  const current = doc.target.kind === 'field' ? `field:${doc.target.index}` : doc.target.kind === 'qr' ? 'qr' : null;
  const options = [
    ...(qr
      ? [{ value: 'qr', label: qr.id ? t.t('ng-ui-insp-pos-qr-item', { id: qr.id }) : t.t('ng-ui-insp-pos-qr-item-noid'), description: where(t, sizes, qr.page, qr.rect) }]
      : []),
    ...fields.map((f) => ({ value: `field:${f.index}`, label: t.t('ng-ui-insp-pos-field-item', { name: f.name }), description: where(t, sizes, f.page, f.rect) })),
  ].map((o) => ({ ...o, isDisabled: !editable }));
  const use = (v: string) => {
    onUse();
    if (v === 'qr') void dispatch({ type: 'use_target', target: { kind: 'qr' } });
    else void dispatch({ type: 'use_target', target: { kind: 'field', index: Number(v.slice('field:'.length)) } });
  };
  return (
    <div className="stack-3">
      {options.length > 0 ? (
        <RadioCards label={t.t(blockTotal(doc) > 1 ? 'ng-ui-insp-pos-placeholder-pick' : 'ng-ui-insp-pos-found')} value={current} onChange={use} options={options} />
      ) : (
        <p className="insp-hint">{t.t('ng-ui-insp-pos-none')}</p>
      )}
      <PlaceholderHelp doc={doc} />
    </div>
  );
}

interface CoordErrors {
  page?: string;
  x?: string;
  y?: string;
}

function CoordsDetail({ doc, editable, sizes, onUse }: { doc: DocumentView; editable: boolean; sizes: [number, number][]; onUse: () => void }) {
  const { t, dispatch } = useApp();
  const formRef = useRef<HTMLFormElement>(null);
  const lang = t.lang;
  const pages = doc.doc.page_count;
  const fromDoc = () => ({
    page: String(Math.min(doc.placement.page_index, pages - 1) + 1),
    x: formatMm(doc.placement.x, lang),
    y: formatMm(doc.placement.y, lang),
  });
  const [values, setValues] = useState(fromDoc);
  const [dirty, setDirty] = useState(false);
  const [errors, setErrors] = useState<CoordErrors>({});
  // Lage geändert (Pfeiltasten, Ziehen, übernommen): die Felder folgen, solange nichts eingetippt ist
  useEffect(() => {
    if (!dirty) setValues(fromDoc());
  }, [doc.revision]);
  const pageNo = Number(values.page);
  const shownPage = Number.isInteger(pageNo) && pageNo >= 1 && pageNo <= pages ? pageNo - 1 : Math.min(doc.placement.page_index, pages - 1);
  const [pw, ph] = pageSize(sizes, shownPage);
  const bw = doc.block.width;
  const bh = doc.block.height;
  const edit = (k: keyof typeof values) => (v: string) => {
    setDirty(true);
    setValues((old) => ({ ...old, [k]: v }));
    // Fehler gilt bis zur nächsten Eingabe in diesem Feld
    setErrors((old) => ({ ...old, [k]: undefined }));
  };
  const apply = () => {
    const e: CoordErrors = {};
    const p = /^\d+$/.test(values.page.trim()) ? Number(values.page.trim()) : NaN;
    const x = parseMm(values.x);
    const y = parseMm(values.y);
    if (!Number.isInteger(p) || p < 1 || p > pages) e.page = t.t('ng-ui-insp-pos-err-page', { max: pages });
    const maxX = Math.max(0, pw - bw);
    if (x === null || x * PT_PER_MM > maxX + 0.05) e.x = t.t('ng-ui-insp-pos-err-x', { max: formatMm(maxX, lang) });
    if (y === null || y * PT_PER_MM < bh - 0.05 || y * PT_PER_MM > ph + 0.05) e.y = t.t('ng-ui-insp-pos-err-y', { min: formatMm(bh, lang), max: formatMm(ph, lang) });
    setErrors(e);
    if (e.page || e.x || e.y) {
      // Fokus auf das erste falsche Feld (WCAG 3.3.1): Screenreader lesen Bezeichnung und Fehler (aria-describedby);
      // keine zusätzliche Ansage, die `role="alert"` der Felder melden die Fehler schon
      requestAnimationFrame(() => formRef.current?.querySelector<HTMLInputElement>('input[aria-invalid="true"]')?.focus());
      return;
    }
    setDirty(false);
    onUse();
    void dispatch({ type: 'move_block', page: p - 1, x: (x as number) * PT_PER_MM, y: (y as number) * PT_PER_MM });
  };
  return (
    <form
      ref={formRef}
      className="stack-3"
      onSubmit={(ev) => {
        ev.preventDefault();
        apply();
      }}
    >
      <div className="insp-pos-coords">
        <TextInput label={t.t('ng-ui-insp-pos-page')} value={values.page} onChange={edit('page')} inputMode="numeric" autoComplete="off" error={errors.page} isDisabled={!editable} />
        <TextInput label={t.t('ng-ui-insp-pos-x')} value={values.x} onChange={edit('x')} inputMode="decimal" autoComplete="off" error={errors.x} isDisabled={!editable} />
        <TextInput label={t.t('ng-ui-insp-pos-y')} value={values.y} onChange={edit('y')} inputMode="decimal" autoComplete="off" error={errors.y} isDisabled={!editable} />
      </div>
      <p className="insp-hint">
        {t.t('ng-ui-insp-pos-coords-hint', { page: shownPage + 1, pw: formatMm(pw, lang), ph: formatMm(ph, lang), bw: formatMm(bw, lang), bh: formatMm(bh, lang) })}
      </p>
      <div>
        <Button type="submit" variant="secondary" size="sm" isDisabled={!editable}>
          {t.t('ng-ui-insp-pos-coords-apply')}
        </Button>
      </div>
    </form>
  );
}

function FreeDetail({ doc, editable, sizes, onUse }: { doc: DocumentView; editable: boolean; sizes: [number, number][]; onUse: () => void }) {
  const { t, dispatch } = useApp();
  const lang = t.lang;
  const bw = doc.block.width;
  const [chosen, setChosen] = useState<string | null>(null);
  // je Seite höchstens drei (der Kern sortiert nach Eignung), zusammen höchstens acht
  const areas = doc.layout
    .filter((l) => l.page < doc.doc.page_count)
    .flatMap((l) => l.free.slice(0, 3).map((r) => ({ page: l.page, rect: r })))
    .slice(0, 8);
  const choose = (v: string) => {
    const area = areas[Number(v)];
    if (!area) return;
    setChosen(v);
    onUse();
    const { rect } = area;
    const x = rect.left + Math.max(0, (rect.right - rect.left - bw) / 2);
    void dispatch({ type: 'move_block', page: area.page, x, y: rect.top });
  };
  return (
    <div className="stack-3">
      {areas.length > 0 ? (
        <RadioCards
          label={t.t('ng-ui-insp-pos-free-list')}
          value={chosen}
          onChange={choose}
          options={areas.map((a, i) => {
            const [pw, ph] = pageSize(sizes, a.page);
            return {
              value: String(i),
              label: t.t('ng-ui-insp-pos-free-item', {
                n: i + 1,
                page: a.page + 1,
                zone: zoneText(t, a.rect, pw, ph),
                w: formatMm(a.rect.right - a.rect.left, lang),
                h: formatMm(a.rect.top - a.rect.bottom, lang),
              }),
              isDisabled: !editable,
            };
          })}
        />
      ) : (
        <p className="insp-hint">{t.t('ng-ui-insp-pos-free-none')}</p>
      )}
      {areas.length > 0 ? <p className="insp-hint">{t.t('ng-ui-insp-pos-free-hint')}</p> : null}
    </div>
  );
}
