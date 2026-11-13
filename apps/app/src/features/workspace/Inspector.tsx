/**
 * Inspektor des Arbeitsbereichs (Gestalt aus dem Prototyp, ADR 0004 §2.4), unter 760 px als Bottom-Sheet
 * (eingeklappt: Konto-Chip, „Signieren“, Überdeckungswarnung). Abschnitte: Signieren mit (Konto-Chip,
 * Kurzhinweis Datenschutz; nur mit Signiermittel-Auswahl, `signers.choice_offered`, ADR 0004 Nachtrag 20 – sonst
 * steht breit nur der Kurzhinweis an dieser Stelle, im Bottom-Sheet direkt über „Signieren“, das dort die volle
 * Breite nimmt), Signaturbild
 * (Vorschau des Kerns, „Bearbeiten“ → Einstellungen ›
 * Signaturbild), Optionen (Unsichtbar, Auf neuer Seite – gesperrt mit Begründung aus `constraints`),
 * Platzierung (sprechende Lage, „Neu platzieren“/„Frei platzieren“, **keine** Zahlenfelder, D1 – darunter
 * „Ohne Vorschau platzieren …“ mit Methodenwahl und Millimeter-Eingabe, `PlacementPanel`; mit „Barrierefreie
 * Bedienung“ steht dieses Panel als „Platzierung der Signatur“ ganz oben, docs/analyse/06),
 * dezente Hinweise auf Erkanntes (Einstellungen › Dokument, `DocumentView::detection`: PDF/A erkannt bzw.
 * erzwungen; QR-Platzhalter bzw. Signaturfeld nur bei automatischer Platzierung),
 * vorhandene Signaturen, primärer Knopf „Signieren ⌘↵“, darunter „Zurück zum Stapel“ (aus der
 * Stapelübersicht geöffnet) bzw. „Zum Stapel“ (weitere Dokumente). In der Phase `warning` steht hier die Warnung
 * der Eingangsanalyse mit „Trotzdem signieren“/„Verwerfen“.
 *
 * iOS: nach dem Empfang über „Teilen“ der Hinweis, dass nur das erste geteilte PDF ankommt (`ShareHint`,
 * `ViewState::share_hint`, ADR 0004 Nachtrag 12).
 *
 * Mehrfachsignatur (`document.blocks`, je Block eine eigene Signatur): Abschnitt „Signaturblöcke“ nach der
 * Platzierung – „Weiteren Block hinzufügen“ (`add_block` auf der gerade sichtbaren Seite), „Alle Platzhalter
 * belegen“ (`fill_targets`), ab zwei Blöcken die nummerierte Liste (auswählen `select_block`, entfernen
 * `remove_block`). Der Knopf heißt dann „n-mal signieren“, darunter steht, wie viele Signaturen das Dokument erhält.
 *
 * Einstellung „Automatische Positionierung“ (`placement_locked`, ADR 0004 Nachtrag 8): Platzierung „Automatisch
 * (laut Einstellung)“ mit „Diesmal selbst platzieren“ (`unlock_placement`) statt „Neu platzieren“/„Frei
 * platzieren“; nach dem Öffnen liegt der Fokus auf „Signieren“ (das Dokument ist bereit). Optionen bleiben
 * bedienbar.
 */
import { useId, useRef, useState } from 'react';
import { Button as AriaButton } from 'react-aria-components';
import { useApp } from '../../app/context';
import { tileScale } from '../../core/bridge';
import { defaultDocumentDetection } from '../../core/defaults';
import { isMobile, type BlockSlotView, type DocumentView, type Lang, type StepView } from '../../core/types';
import { AccessButton, Badge, Button, Keys, Segmented, SwitchField, useModKey } from '../../ui/controls';
import { useFitScale, useNarrow } from '../../ui/hooks';
import { IconAlert, IconChevronDown, IconClose, IconFileText, IconGrip, IconInfo, IconLock, IconPen, IconPlus, IconQr, IconShieldCheck, IconSignature, IconTarget, IconWand } from '../../ui/Icons';
import { accessibilityOf } from '../accessibility';
import { PrivacyNote } from '../settings';
import { PlacementPanel } from './PlacementPanel';
import { blockItems, blockTotal, displayPages, overlapState, placeholderUncovered, zoneText } from './placement';
import { ShareHint } from './ShareHint';
import { SignerChip } from './SignerChip';
import { signerChoice } from './signers';

function formatTime(iso: string | null, lang: 'de' | 'en'): string | null {
  if (!iso) return null;
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return null;
  return new Intl.DateTimeFormat(lang === 'de' ? 'de-AT' : 'en-GB', { dateStyle: 'medium', timeStyle: 'short' }).format(d);
}

export function Inspector({
  doc,
  chipOpen,
  onChipOpenChange,
  signMode,
  onSign,
  visiblePage,
}: {
  /** gerade sichtbare Seite (0-basiert): dort erscheint ein weiterer Signaturblock */
  visiblePage?: number;
  doc: DocumentView;
  chipOpen: boolean;
  onChipOpenChange: (open: boolean, signMode?: boolean) => void;
  signMode: boolean;
  onSign: () => void;
}) {
  const { t, view, dispatch, openSettings, bridge } = useApp();
  const { mod, enter } = useModKey();
  const a11y = accessibilityOf(view);
  // Bottom-Sheet: beginnt die Platzierung mit der Methodenwahl, ist es gleich aufgeklappt
  const [expanded, setExpanded] = useState(a11y.start === 'methods');
  const narrow = useNarrow();
  const bodyId = useId();
  const previewRef = useRef<HTMLDivElement>(null);
  const previewScale = useFitScale(previewRef, doc.block.width, { min: 0.4, max: 1.2, pad: 28, initial: doc.block.width > 300 ? 0.6 : 1.05 });
  const editable = view.actions.edit_placement && doc.phase === 'editing';
  // „Automatische Positionierung“: automatisch platziert, bereit zum Signieren; verschieben erst nach „Diesmal
  // selbst platzieren“ (der Kern ignoriert Verschieben und Zielwahl bis dahin)
  const locked = editable && doc.placement_locked;
  const unlock = () => void dispatch({ type: 'unlock_placement' });
  const warning = doc.phase === 'warning' && view.step.step === 'warning' ? (view.step as Extract<StepView, { step: 'warning' }>) : null;
  const sizes = displayPages(doc);
  const page = doc.placement.page_index;
  const [pw, ph] = sizes[page] ?? sizes[sizes.length - 1] ?? [595.28, 841.89];
  const overlap = overlapState(doc);
  const c = doc.constraints;
  const selected = view.signers.options.find((o) => o.backend === view.signers.selected);
  const leaves = selected?.capabilities?.document_leaves_device === true;
  // nur ein Signiermittel (iOS/Android: ID Austria; Desktop z. B. per Richtlinie): keine Auswahl, kein Konto-Chip –
  // das Konto zeigt die Signierkarte beim Anmelden, geändert wird es unter Einstellungen › Signiermittel (Nachtrag 20)
  const choice = signerChoice(view);
  const field = doc.target.kind === 'field' ? doc.fields.find((f) => doc.target.kind === 'field' && f.index === doc.target.index) : undefined;
  const reason = (id: string) => (c.reasons.includes(id) ? t.code(id) : null);
  // Einstellungen › Dokument (E3): Erkanntes nur nennen, solange die Automatik platziert hat (nicht nach „Ins
  // Feld setzen“ bzw. „Frei platzieren“ von Hand); PDF/A wie beim Signieren entschieden (Kern)
  const detection = doc.detection ?? defaultDocumentDetection();
  const detected = detection.auto_placed && (doc.target.kind === 'qr' || doc.target.kind === 'field') ? doc.target.kind : null;
  const pdfaNote =
    detection.pdfa_decision === 'detected' ? t.t('ng-ui-insp-detect-pdfa') : detection.pdfa_decision === 'forced' ? t.t('ng-ui-insp-detect-pdfa-forced') : null;
  // aus der Stapelübersicht geöffnet: „Zurück zum Stapel“ (Platzierung wird als Plan gemerkt, §3.8)
  const backToBatch = view.batch?.open_item === doc.doc.id;
  const newPageReason = !c.new_page_allowed ? (reason('ng-constraint-signed') ?? reason('ng-constraint-certified') ?? t.t('ng-ui-insp-new-page-locked')) : null;
  const invisibleReason = !c.visible_allowed ? (reason('ng-constraint-invisible-profile') ?? null) : null;
  // Platzierung ohne Vorschau (docs/analyse/06): mit „Barrierefreie Bedienung“ als erster Abschnitt, sonst eingeklappt
  // unter „Platzierung“; nicht bei gesperrter Platzierung („Automatische Positionierung“) und unsichtbarer Signatur
  const panelShown = !warning && !doc.invisible && !!doc.sticker && !locked;
  const panelOnTop = panelShown && a11y.mode;
  // Mehrfachsignatur: je Block eine eigene Signatur
  const blocks = doc.blocks ?? null;
  const items = blockItems(doc);
  const signatures = blockTotal(doc);
  const blocksShown = !warning && !doc.invisible && !!doc.sticker && !!blocks && !locked;
  const fillCount = blocks?.free_targets ?? 0;
  // „Alle Platzhalter belegen“ erst, wenn es mehr bringt als „Ins Feld setzen“ für den einen Block
  const fillShown = editable && (fillCount >= 2 || (fillCount >= 1 && doc.target.kind !== 'free'));
  const addReason = !blocks || blocks.can_add ? null : doc.new_page ? t.t('ng-ui-insp-blocks-new-page') : items.length >= blocks.limit ? t.t('ng-ui-insp-blocks-limit', { n: blocks.limit }) : null;
  const where = (b: BlockSlotView): string => {
    if (b.target.kind === 'field') {
      const index = b.target.index;
      return t.t('ng-ui-insp-blocks-item-field', { name: doc.fields.find((f) => f.index === index)?.name ?? String(index + 1) });
    }
    if (b.target.kind === 'qr') return t.t('ng-ui-insp-blocks-item-qr');
    const [bw, bh] = sizes[b.page] ?? sizes[sizes.length - 1] ?? [595.28, 841.89];
    return t.t('ng-ui-insp-blocks-item-free', { page: b.page + 1, zone: b.rect ? zoneText(t, b.rect, bw, bh) : '' });
  };
  const chip = (compact: boolean) => (
    <SignerChip compact={compact} isOpen={chipOpen} onOpenChange={(open) => onChipOpenChange(open, false)} signMode={signMode} />
  );

  return (
    <aside className="inspector" data-expanded={expanded || undefined} data-narrow={narrow || undefined} aria-label={t.t('ng-ui-insp-label')}>
      {narrow ? (
        <AriaButton className="sheet-handle" aria-expanded={expanded} aria-controls={bodyId} onPress={() => setExpanded((v) => !v)}>
          <span className="sheet-grip" aria-hidden="true" />
          <span className="sheet-handle-text">{expanded ? t.t('ng-ui-insp-sheet-collapse') : t.t('ng-ui-insp-sheet-expand')}</span>
          <IconChevronDown size={16} className="sheet-caret" />
        </AriaButton>
      ) : null}

      <div className="inspector-scroll" id={bodyId} hidden={narrow && !expanded}>
        {panelOnTop ? <PlacementPanel doc={doc} editable={editable} embedded={false} /> : null}
        {warning ? (
          <section className="insp-section insp-warning" aria-labelledby={`${bodyId}-warn`}>
            <h2 className="insp-heading" id={`${bodyId}-warn`}>
              {t.t('ng-ui-insp-warning')}
            </h2>
            <div className="callout callout-warning" role="alert">
              <IconAlert size={18} />
              <div>
                <p className="callout-title">{t.t('ng-ui-warning-lead')}</p>
                <ul className="state-list">
                  {warning.codes.map((code) => (
                    <li key={code} data-code={code}>
                      {t.code(code)}
                    </li>
                  ))}
                </ul>
                <p>{t.t('ng-ui-warning-question')}</p>
              </div>
            </div>
            <div className="button-row">
              <Button variant="primary" onPress={() => void dispatch({ type: 'answer_warning', proceed: true })}>
                {t.t('ng-ui-insp-warning-proceed')}
              </Button>
              <Button variant="ghost" onPress={() => void dispatch({ type: 'answer_warning', proceed: false })}>
                {t.t('ng-ui-insp-warning-discard')}
              </Button>
            </div>
          </section>
        ) : null}

        {!narrow && !warning && choice ? (
          <section className="insp-section" aria-labelledby={`${bodyId}-signer`}>
            <h2 className="insp-heading" id={`${bodyId}-signer`}>
              {t.t('ng-ui-insp-sign-with')}
            </h2>
            {chip(false)}
            {leaves ? <PrivacyNote /> : null}
          </section>
        ) : null}
        {/* breit ohne Auswahl (Nachtrag 20): kein Abschnitt „Signieren mit“ – der Kurzhinweis steht, wo er stand (oben
            im rollbaren Teil), nicht im festen Fuß: im Querformat bzw. mit großer Schrift bliebe sonst kaum Platz für
            Signaturbild, Optionen und Platzierung */}
        {!narrow && !warning && !choice && leaves ? (
          <div className="insp-section" data-testid="insp-privacy">
            <PrivacyNote />
          </div>
        ) : null}

        <section className="insp-section" aria-labelledby={`${bodyId}-block`}>
          <div className="insp-heading-row">
            <h2 className="insp-heading" id={`${bodyId}-block`}>
              {t.t('ng-ui-insp-block')}
            </h2>
            <Button variant="ghost" size="sm" onPress={() => openSettings('signature')} isDisabled={!view.actions.settings} aria-label={t.t('ng-ui-insp-block-edit')}>
              <IconPen size={15} />
              {t.t('ng-ui-insp-edit')}
            </Button>
          </div>
          <div className={`insp-block-preview${doc.invisible ? ' is-muted' : ''}`} ref={previewRef}>
            <div className="insp-block-scroller" style={{ width: doc.block.width * previewScale, height: doc.block.height * previewScale }}>
              {/* Revision 0 = der Kern hat (noch) keine Vorschau gebaut, z. B. Startargument-Automatik ohne
                  Positionieren – kein Abruf, der nur mit „keine Blockvorschau“ scheitert */}
              {doc.block.preview_revision > 0 ? (
                <img
                className="insp-preview-img"
                src={bridge.blockPreviewUrl(tileScale(previewScale * 2), doc.block.preview_revision)}
                alt={t.t('ng-ui-insp-block-alt')}
                width={Math.round(doc.block.width * previewScale)}
                height={Math.round(doc.block.height * previewScale)}
                draggable={false}
                />
              ) : null}
            </div>
          </div>
          {/* Amtssignatur ohne eigene Bildmarke (Backlog Nr. 10): neutraler Platzhalter statt der Bildmarke der Behörde */}
          {/* „Bildmarke wählen“ nur, wo eine eigene Bildmarke diesen Block ändert: nicht auf iOS/Android (keine eigene
              Bildmarke) und nicht, wenn ein QR-Platzhalter das Profil vorgibt (der Kern entscheidet: `emblem_choosable`) */}
          {doc.block.emblem_placeholder ? (
            <div className="callout callout-info" data-testid="emblem-placeholder-hint">
              <IconInfo size={18} />
              <div>
                {isMobile(view) ? (
                  <p>{t.t('ng-ui-insp-emblem-placeholder-mobile')}</p>
                ) : doc.block.emblem_choosable ? (
                  <>
                    <p>{t.t('ng-ui-insp-emblem-placeholder')}</p>
                    <Button variant="secondary" size="sm" onPress={() => openSettings('signature')} isDisabled={!view.actions.settings}>
                      {t.t('ng-ui-insp-emblem-placeholder-choose')}
                    </Button>
                  </>
                ) : (
                  <p>{t.t('ng-ui-insp-emblem-placeholder-qr')}</p>
                )}
              </div>
            </div>
          ) : null}
        </section>

        {!warning ? (
          <section className="insp-section" aria-labelledby={`${bodyId}-opts`}>
            <h2 className="insp-heading" id={`${bodyId}-opts`}>
              {t.t('ng-ui-insp-options')}
            </h2>
            {/* Schnellzugriff auf die Einstellung `signature.block_language` (gilt auch für weitere Dokumente) */}
            <div className={`switch-field insp-block-lang${!editable || doc.invisible ? ' is-disabled' : ''}`}>
              <div className="insp-block-lang-row">
                <span className="switch-label" id={`${bodyId}-lang`}>
                  {t.t('ng-ui-insp-block-lang')}
                </span>
                <Segmented<Lang>
                  label={t.t('ng-ui-insp-block-lang')}
                  hideLabel
                  size="sm"
                  value={doc.block.language}
                  onChange={(language) => void dispatch({ type: 'set_block_language', language })}
                  options={[
                    { value: 'de', label: 'DE', isDisabled: !editable || doc.invisible },
                    { value: 'en', label: 'EN', isDisabled: !editable || doc.invisible },
                  ]}
                />
              </div>
              <p className="switch-hint">{t.t('ng-ui-insp-block-lang-hint')}</p>
            </div>
            <SwitchField
              label={t.t('ng-ui-insp-invisible')}
              hint={invisibleReason ?? t.t('ng-ui-insp-invisible-hint')}
              isSelected={doc.invisible}
              isDisabled={!editable || !c.visible_allowed}
              onChange={(v) => void dispatch({ type: 'set_invisible', invisible: v })}
            />
            <SwitchField
              label={t.t('ng-ui-insp-new-page')}
              hint={newPageReason ?? (signatures > 1 ? t.t('ng-ui-insp-new-page-blocks') : t.t('ng-ui-insp-new-page-hint'))}
              isSelected={doc.new_page}
              isDisabled={!editable || doc.invisible || (!c.new_page_allowed && !doc.new_page) || (signatures > 1 && !doc.new_page)}
              onChange={() => void dispatch({ type: 'toggle_new_page' })}
            />
            {pdfaNote ? (
              <p className="insp-note" data-testid="pdfa-note">
                <IconFileText size={15} />
                {pdfaNote}
              </p>
            ) : null}
          </section>
        ) : null}

        {!warning && !doc.invisible && doc.sticker && !panelOnTop ? (
          <section className="insp-section" aria-labelledby={`${bodyId}-place`}>
            <div className="insp-heading-row">
              <h2 className="insp-heading" id={`${bodyId}-place`}>
                {t.t('ng-ui-insp-placement')}
              </h2>
              {doc.target.kind === 'free' && !locked ? (
                <Button variant="ghost" size="sm" onPress={() => void dispatch({ type: 'auto_place' })} isDisabled={!editable}>
                  <IconWand size={15} />
                  {t.t('ng-ui-insp-auto-place')}
                </Button>
              ) : null}
            </div>
            {locked && detection.auto_placed ? (
              <p className="insp-placement" data-testid="placement-auto">
                <IconLock size={16} />
                <span>{t.t('ng-ui-insp-placement-auto')}</span>
              </p>
            ) : (
              <p className="insp-placement">
                {locked ? <IconLock size={16} /> : <IconTarget size={16} />}
                <span>
                  {field
                    ? t.t('ng-ui-insp-placement-field', { name: field.name })
                    : doc.target.kind === 'qr'
                      ? t.t('ng-ui-insp-placement-qr')
                      : t.t('ng-ui-insp-placement-text', { page: page + 1, zone: zoneText(t, doc.sticker, pw, ph) })}
                </span>
              </p>
            )}
            {detected ? (
              <p className="insp-note" data-testid="detect-note">
                {detected === 'qr' ? <IconQr size={15} /> : <IconPen size={15} />}
                {t.t(detected === 'qr' ? 'ng-ui-insp-detect-qr' : 'ng-ui-insp-detect-field')}
              </p>
            ) : null}
            {locked ? (
              <>
                <p className="insp-hint">{t.t('ng-ui-insp-placement-locked-hint')}</p>
                <Button variant="secondary" size="sm" onPress={unlock} data-testid="placement-unlock">
                  <IconGrip size={15} />
                  {t.t('ng-ui-insp-placement-unlock')}
                </Button>
              </>
            ) : doc.target.kind !== 'free' ? (
              <Button variant="secondary" size="sm" onPress={() => void dispatch({ type: 'use_target', target: { kind: 'free' } })} isDisabled={!editable}>
                {t.t('ng-ui-insp-placement-free')}
              </Button>
            ) : (
              <p className="insp-hint">{t.t('ng-ui-insp-placement-hint')}</p>
            )}
            {panelShown ? <PlacementPanel doc={doc} editable={editable} embedded /> : null}
          </section>
        ) : null}

        {blocksShown && blocks ? (
          <section className="insp-section" aria-labelledby={`${bodyId}-blocks`} data-testid="blocks-section">
            <div className="insp-heading-row">
              <h2 className="insp-heading" id={`${bodyId}-blocks`}>
                {t.t('ng-ui-insp-blocks')}
              </h2>
              {signatures > 1 ? <Badge tone="neutral">{signatures}</Badge> : null}
            </div>
            {signatures > 1 ? (
              <>
                <ol className="blocks-list" aria-label={t.t('ng-ui-insp-blocks')}>
                  {items.map((b) => (
                    <li key={b.index} className={`blocks-item${b.active ? ' is-active' : ''}`}>
                      <AriaButton
                        className="blocks-select"
                        aria-current={b.active ? 'true' : undefined}
                        aria-label={t.t('ng-ui-insp-blocks-select', { n: b.index + 1, where: where(b) })}
                        isDisabled={!editable}
                        onPress={() => void dispatch({ type: 'select_block', index: b.index })}
                      >
                        <span className="blocks-number" aria-hidden="true">
                          {b.index + 1}
                        </span>
                        <span className="blocks-where">{where(b)}</span>
                      </AriaButton>
                      <AriaButton
                        className="blocks-remove"
                        aria-label={t.t('ng-ui-insp-blocks-remove', { n: b.index + 1 })}
                        isDisabled={!editable}
                        onPress={() => void dispatch({ type: 'remove_block', index: b.index })}
                      >
                        <IconClose size={14} />
                      </AriaButton>
                    </li>
                  ))}
                </ol>
                <p className="insp-hint">{t.t('ng-ui-insp-blocks-hint')}</p>
              </>
            ) : (
              <p className="insp-hint">{t.t('ng-ui-insp-blocks-add-hint')}</p>
            )}
            <div className="blocks-actions">
              <Button
                variant="secondary"
                size="sm"
                isDisabled={!editable || !blocks.can_add}
                onPress={() => void dispatch({ type: 'add_block', page: visiblePage ?? null })}
                data-testid="blocks-add"
              >
                <IconPlus size={15} />
                {t.t('ng-ui-insp-blocks-add')}
              </Button>
              {fillShown ? (
                <Button variant="secondary" size="sm" onPress={() => void dispatch({ type: 'fill_targets' })} data-testid="blocks-fill">
                  <IconPen size={15} />
                  {doc.target.kind === 'free' ? t.t('ng-ui-insp-blocks-fill', { n: fillCount }) : t.t('ng-ui-insp-blocks-fill-more', { n: fillCount })}
                </Button>
              ) : null}
            </div>
            {addReason ? <p className="insp-hint" data-testid="blocks-add-reason">{addReason}</p> : null}
          </section>
        ) : null}

        {doc.existing.length > 0 ? (
          <section className="insp-section" aria-labelledby={`${bodyId}-existing`}>
            <h2 className="insp-heading" id={`${bodyId}-existing`}>
              {t.t('ng-ui-insp-existing')}
            </h2>
            <ul className="existing-list">
              {doc.existing.map((s) => {
                const tone = s.status === 'valid' ? 'success' : s.status === 'invalid' ? 'danger' : 'warning';
                const time = formatTime(s.signing_time, t.lang);
                return (
                  <li key={s.field_name} className="existing-item">
                    <IconShieldCheck size={18} className={`existing-icon is-${s.status === 'valid' ? 'valid' : s.status === 'invalid' ? 'invalid' : 'unknown'}`} />
                    <div className="existing-text">
                      <span className="existing-signer">{s.signer ?? t.t('ng-ui-insp-existing-unknown')}</span>
                      <span className="existing-meta">{time ? t.t('ng-ui-insp-existing-meta', { time }) : s.field_name}</span>
                    </div>
                    <Badge tone={tone}>{t.t(`ng-ui-insp-status-${s.status === 'not_verifiable' ? 'unknown' : s.status}`)}</Badge>
                  </li>
                );
              })}
            </ul>
          </section>
        ) : null}
      </div>

      {/* Überdeckung direkt über „Signieren“ (auch im eingeklappten Bottom-Sheet), damit sie vor dem
          Signieren nicht unten in der Seitenleiste verschwindet; angesagt über die Live-Region. */}
      {overlap && !warning ? (
        <div className="callout callout-warning inspector-warn" data-testid="overlap-warning">
          <IconAlert size={18} />
          <div>
            <p>{t.t(overlap === 'block' ? 'ng-ui-insp-overlap-block' : overlap === 'widget' ? 'ng-ui-insp-overlap-widget' : 'ng-ui-insp-overlap')}</p>
            {overlap === 'block' ? null : locked ? (
              <Button variant="secondary" size="sm" onPress={unlock}>
                {t.t('ng-ui-insp-placement-unlock')}
              </Button>
            ) : (
              <Button variant="secondary" size="sm" onPress={() => void dispatch({ type: 'auto_place' })} isDisabled={!editable}>
                {t.t('ng-ui-insp-overlap-fix')}
              </Button>
            )}
          </div>
        </div>
      ) : null}

      {/* QR-Platzhalter höher bzw. breiter als der Block (Backlog Nr. 32): Block bleibt PDF-AS-gleich, nur ein Hinweis */}
      {placeholderUncovered(doc) && !warning ? (
        <div className="callout callout-info inspector-warn" data-testid="qr-uncovered-hint">
          <IconInfo size={18} />
          <div>
            <p>{t.t('ng-ui-insp-qr-uncovered')}</p>
          </div>
        </div>
      ) : null}

      {/* iOS nach dem Empfang über „Teilen“ (Nachtrag 12): nur das erste geteilte PDF kommt an – wie die Warnung über
          „Signieren“, damit der Hinweis auch im eingeklappten Bottom-Sheet sichtbar ist (der Kern entscheidet) */}
      {!warning ? <ShareHint /> : null}

      {/* Schmal (Bottom-Sheet): der Konto-Chip steht unten – der Kurzhinweis „wird an A-Trust übermittelt“
          (ADR 0003, „bei der Signiermittel-Auswahl“) deshalb direkt darüber, auch eingeklappt (ohne Auswahl über
          „Signieren“, Nachtrag 20) */}
      {narrow && leaves && !warning ? (
        <div className="inspector-privacy">
          <PrivacyNote />
        </div>
      ) : null}

      {/* Mehrfachsignatur: vor dem Start sagen, wie viele Signaturen entstehen (mit ID Austria evtl. je eine Bestätigung) */}
      {!warning && signatures > 1 && doc.phase === 'editing' ? (
        <p className="inspector-count" data-testid="sign-count">
          {t.t('ng-ui-insp-sign-count', { n: signatures })}
          {leaves ? ` ${t.t('ng-ui-insp-sign-count-confirm')}` : ''}
        </p>
      ) : null}

      {!warning ? (
        <div className={`inspector-footer${choice ? '' : ' is-single'}`}>
          {narrow && choice ? <div className="inspector-footer-chip">{chip(true)}</div> : null}
          <AccessButton
            msg={signatures > 1 ? 'ng-ui-insp-sign-times' : 'ng-ui-insp-sign'}
            args={signatures > 1 ? { n: signatures } : undefined}
            variant="primary"
            size="lg"
            className="sign-button"
            onPress={onSign}
            // ohne Signiermittel (die Richtlinie schließt jedes aus) sperrt der Kern `actions.sign` (Nachtrag 20)
            isDisabled={!view.actions.sign}
            // beim Öffnen gesperrt = bereit: Fokus auf „Signieren“ (React Aria wertet `autoFocus` nur beim Einhängen
            // aus – nach der Analysewarnung bzw. beim Öffnen, nie bei späteren Änderungen)
            autoFocus={locked && view.actions.sign}
            icon={<IconSignature size={18} />}
          >
            <Keys keys={[mod, enter]} decorative />
          </AccessButton>
          {backToBatch || view.actions.show_batch ? (
            <Button variant="ghost" size="sm" className="insp-back" onPress={() => void dispatch({ type: backToBatch ? 'back_to_batch' : 'show_batch' })} isDisabled={!editable}>
              {t.t(backToBatch ? 'ng-ui-batch-back' : 'ng-ui-batch-show')}
            </Button>
          ) : null}
        </div>
      ) : null}
    </aside>
  );
}
