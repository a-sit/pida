/**
 * Fertig (Gestalt aus dem Prototyp, ADR 0004 §2.6/§3.7): Prüfsiegel, Vorschau der signierten Seite (Kachel
 * + Blockvorschau aus `ViewState::document`, Phase `done`), Datei und Ordner, Unterzeichner und Zeitpunkt
 * aus `SignedSummaryView`, Aktionen und „Nächstes Dokument“.
 *
 * Wortlaut nach §8 F3: nie „gültig“, sondern „Signatur erstellt und geprüft (Selbstprüfung)“, die
 * Qualifikation „laut Zertifikat“ und immer der Hinweis auf die vollständige Prüfung bei
 * signaturpruefung.gv.at. Desktop: Öffnen / Im Ordner zeigen; Mobile: Öffnen / Teilen / Sichern unter …
 * (`open_result`, `share_result`, `export_result`; kein Ordner, die Datei liegt in den App-Dokumenten).
 *
 * `OutputDialogs` führt die Rückfragen des Ausgabeschritts im neuen Stil: Speichern-Dialog (F-83/F-86,
 * öffnet sich automatisch, Antwort `save{path}` bzw. `save{null}`) und Überschreiben (F-84,
 * `confirm_overwrite`).
 */
import { useEffect, useRef } from 'react';
import { Button as AriaButton } from 'react-aria-components';
import { useApp } from '../../app/context';
import { tileScale } from '../../core/bridge';
import { baseName, dirName } from '../../core/defaults';
import { isMobile, type OutputView, type StepView } from '../../core/types';
import { webProfile } from '../../core/web-profile';
import { Button } from '../../ui/controls';
import { useNarrow } from '../../ui/hooks';
import { ConfirmDialog } from '../../ui/Dialog';
import { IconCheck, IconDownload, IconExternal, IconFileText, IconFolder, IconHelp, IconLayers, IconShare, IconShieldCheck, IconUpload } from '../../ui/Icons';
import { HELP_AVAILABLE } from '../help';
import { displayPages, rectToBox } from '../workspace/placement';
import './done.css';

/** Hilfe-Artikel zu den Signaturarten (`02-signaturarten-recht.md` der Hilfe, in beiden Sprachen dieselbe Kennung). */
export const QUALIFICATION_HELP = 'recht:drei-arten-elektronischer-signaturen-eidas';

const PREVIEW_W = 170;
/** schmal (Telefon): kleinere Vorschau, damit Aktionen und „Nächstes Dokument“ ohne Scrollen erreichbar bleiben */
const PREVIEW_W_NARROW = 112;
/** Vollständige Prüfung (Vertrauenskette, Sperrstatus) – PIDA prüft nur selbst (§8 F3). */
export const VERIFY_URL = 'https://www.signaturpruefung.gv.at';

function joinPath(dir: string, name: string): string {
  if (!dir) return name;
  const sep = dir.includes('\\') && !dir.includes('/') ? '\\' : '/';
  return dir.endsWith(sep) ? dir + name : dir + sep + name;
}

function when(iso: string | null | undefined, lang: 'de' | 'en'): string | null {
  if (!iso) return null;
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : new Intl.DateTimeFormat(lang === 'de' ? 'de-AT' : 'en-GB', { dateStyle: 'medium', timeStyle: 'short' }).format(d);
}

/** Speichern-Dialog öffnen (Vorschlag aus `save_dialog` bzw. dem letzten Vorschlag). */
function useAskSave() {
  const { t, bridge, dispatch, toast } = useApp();
  return async (defaultPath: string | null) => {
    const filters = [
      { name: t.t('common-PDFExtension_Description'), extensions: ['pdf'] },
      { name: t.t('common-AllExtension_Description'), extensions: ['*'] },
    ];
    try {
      const path = await bridge.pickSavePath({ title: t.t('common-Save'), filters, defaultPath });
      void dispatch({ type: 'save', path });
    } catch {
      toast(t.t('ng-ui-dialog-failed'), 'error');
      // Dialog nicht verfügbar: wie abgebrochen (Knopf „Speichern“ bleibt, F-86)
      void dispatch({ type: 'save', path: null });
    }
  };
}

/** Letzter Speichervorschlag (gilt auch für den Knopf „Speichern“ nach abgebrochenem Dialog, F-86). */
const lastSuggestion: { current: string | null } = { current: null };

export function OutputDialogs({ result }: { result: OutputView }) {
  const { t, dispatch, view } = useApp();
  const mobile = isMobile(view);
  const askSave = useAskSave();
  const asked = useRef<string | null>(null);
  const target = result.save_dialog ? joinPath(result.save_dialog.directory, result.save_dialog.file_name) : null;
  const askRef = useRef(askSave);
  askRef.current = askSave;
  useEffect(() => {
    if (target === null) {
      asked.current = null;
      return;
    }
    lastSuggestion.current = target;
    if (asked.current === target) return;
    asked.current = target;
    void askRef.current(target);
  }, [target]);
  if (!result.ask_overwrite) return null;
  return (
    <ConfirmDialog
      title={t.t('ng-ui-fin-overwrite-title')}
      tone="warning"
      onCancel={() => void dispatch({ type: 'confirm_overwrite', overwrite: false })}
      actions={
        <>
          <Button variant="ghost" onPress={() => void dispatch({ type: 'confirm_overwrite', overwrite: false })} autoFocus>
            {t.t('ng-ui-sc-cancel')}
          </Button>
          <Button variant="danger" onPress={() => void dispatch({ type: 'confirm_overwrite', overwrite: true })}>
            {t.t('ng-ui-fin-overwrite')}
          </Button>
        </>
      }
    >
      {/* Mobile: nur der Dateiname – der Sandbox-Pfad ist unlesbar */}
      <p>{t.t('output-file_ask_overwrite', { arg1: mobile ? baseName(result.ask_overwrite) : result.ask_overwrite })}</p>
    </ConfirmDialog>
  );
}

export function DoneView({ step }: { step: Extract<StepView, { step: 'output' }> }) {
  const { t, view, dispatch, bridge, toast, openHelp } = useApp();
  const headingRef = useRef<HTMLHeadingElement>(null);
  const mobile = isMobile(view);
  const askSave = useAskSave();
  const { result, next } = step;
  const summary = result.summary;
  const doc = view.document;
  const saved = !!result.path;
  const previewW = useNarrow() ? PREVIEW_W_NARROW : PREVIEW_W;

  useEffect(() => {
    headingRef.current?.focus({ preventScroll: true });
  }, [saved]);

  const fileName = summary?.file_name ?? (result.path ? baseName(result.path) : null);
  const folder = mobile ? null : (summary?.folder_display ?? (result.path ? dirName(result.path) : null));
  const time = when(summary?.signing_time, t.lang);
  const signer = summary?.signer ?? null;

  // Vorschau der signierten Seite: Kachel des Dokuments + Blockvorschau im Rechteck des Stickers
  const sizes = doc ? displayPages(doc) : [];
  const stickerPage = doc && !doc.invisible && doc.sticker ? doc.placement.page_index : null;
  const pageIndex = summary?.page ?? stickerPage ?? (doc ? doc.doc.page_count - 1 : null);
  const size = pageIndex !== null ? sizes[pageIndex] : undefined;
  const scale = size ? previewW / size[0] : 1;
  const box = doc?.sticker && stickerPage === pageIndex && size ? rectToBox(doc.sticker, size[1], scale) : null;

  const reveal = () => bridge.revealOutput().catch(() => toast(t.t('ng-ui-open-failed'), 'error'));
  // Web: kein Ordner – das Ergebnis wird heruntergeladen (`open_result` → Download)
  const web = webProfile() !== null;
  const openVerifyInfo = () => bridge.openUrl(VERIFY_URL).catch(() => toast(t.t('ng-ui-open-failed'), 'error'));
  const qualification = summary?.qualification === 'Qes' ? t.t('ng-ui-fin-qes') : summary?.qualification === 'AdES' ? t.t('ng-ui-fin-ades') : null;

  return (
    <div className="done-view">
      <section className="done-card" aria-labelledby="done-title">
        <div className={`done-seal${saved ? '' : ' is-pending'}`} aria-hidden="true">
          {saved ? <span className="done-seal-ring" /> : null}
          <IconCheck size={30} />
        </div>
        <h1 className="done-title" id="done-title" ref={headingRef} tabIndex={-1} data-main-heading="">
          {t.t('ng-ui-fin-title')}
        </h1>
        {summary?.self_check !== false ? (
          <p className="seal-badge">
            <IconShieldCheck size={18} />
            {/* Mehrfachsignatur: je Block eine Signatur */}
            {(summary?.signatures ?? 1) > 1 ? t.t('ng-ui-fin-seal-n', { n: summary?.signatures ?? 1 }) : t.t('ng-ui-fin-seal')}
          </p>
        ) : null}
        <p className="done-text">
          {saved
            ? t.t((summary?.signatures ?? 1) > 1 ? 'ng-ui-fin-text-n' : 'ng-ui-fin-text')
            : t.t(result.save_pending ? 'ng-ui-fin-not-saved' : 'ng-ui-fin-saving')}
        </p>
        {qualification ? (
          <p className="done-qual">
            {qualification}
            {/* „qualifiziert“/„fortgeschritten“ erklärt die Offline-Hilfe (Abschnitt zu den Signaturarten) */}
            {HELP_AVAILABLE ? (
              <AriaButton className="link help-what" onPress={() => openHelp(QUALIFICATION_HELP)}>
                <IconHelp size={15} />
                {t.t('ng-ui-shell-help-what')}
              </AriaButton>
            ) : null}
          </p>
        ) : null}

        <div className="done-file">
          {size && doc && pageIndex !== null ? (
            <div className="done-preview" role="img" aria-label={t.t('ng-ui-fin-preview')}>
              <div className="done-preview-page" style={{ width: previewW, height: size[1] * scale }}>
                {pageIndex < doc.doc.page_count ? <img src={bridge.pageTileUrl(doc.doc.id, pageIndex, tileScale(scale * 2))} alt="" draggable={false} /> : null}
                {box && doc.block.preview_revision > 0 ? (
                  <div className="done-preview-sticker" style={{ left: box.left, top: box.top, width: box.width, height: box.height }}>
                    <img src={bridge.blockPreviewUrl(tileScale(scale * 2), doc.block.preview_revision)} alt="" draggable={false} />
                  </div>
                ) : null}
              </div>
            </div>
          ) : null}
          <div className="done-file-info">
            <p className="done-file-name">
              <IconFileText size={18} />
              <span>{fileName ?? doc?.doc.name ?? ''}</span>
            </p>
            {saved ? (
              // Mobile: kein `title` – ohne Mauszeiger ohne Nutzen, und VoiceOver läse statt „Gespeichert in den
              // Dokumenten dieser App“ den Pfad im Sandbox-Container vor
              <p className="done-file-meta" title={mobile ? undefined : (result.path ?? undefined)}>
                {web ? t.t('ng-ui-fin-ready-web') : mobile ? t.t('ng-ui-fin-saved-mobile') : t.t('ng-ui-fin-saved-in', { folder: folder ?? '' })}
              </p>
            ) : null}
            {time ? <p className="done-file-meta">{signer ? t.t('ng-ui-fin-signed-by', { name: signer, time }) : t.t('ng-ui-fin-signed-at', { time })}</p> : null}
            <div className="done-actions">
              {saved && web ? (
                <Button variant="primary" onPress={() => void dispatch({ type: 'open_result' })}>
                  <IconDownload size={17} />
                  {t.t('ng-ui-fin-download')}
                </Button>
              ) : saved ? (
                <>
                  <Button variant="secondary" onPress={() => void dispatch({ type: 'open_result' })}>
                    <IconFileText size={17} />
                    {t.t('ng-ui-fin-open')}
                  </Button>
                  {mobile ? (
                    <>
                      <Button variant="secondary" onPress={() => void dispatch({ type: 'share_result' })}>
                        <IconShare size={17} />
                        {t.t('ng-ui-fin-share')}
                      </Button>
                      <Button variant="secondary" onPress={() => void dispatch({ type: 'export_result' })}>
                        <IconUpload size={17} />
                        {t.t('ng-ui-fin-export')}
                      </Button>
                    </>
                  ) : (
                    <Button variant="secondary" onPress={() => void reveal()}>
                      <IconFolder size={17} />
                      {t.t('ng-ui-fin-reveal')}
                    </Button>
                  )}
                </>
              ) : result.save_pending ? (
                <Button variant="primary" onPress={() => void askSave(lastSuggestion.current)}>
                  {t.t('ng-ui-fin-save')}
                </Button>
              ) : null}
            </div>
          </div>
        </div>

        <div className="done-footer">
          <AriaButton className="link" onPress={() => void openVerifyInfo()}>
            {t.t('ng-ui-fin-check')}
            <IconExternal size={14} />
            <span className="sr-only"> {t.t('ng-ui-link-external')}</span>
          </AriaButton>
          <div className="done-next">
            <Button variant="primary" size="lg" onPress={() => void dispatch({ type: 'navigate', step: 'Open' })} isDisabled={!view.actions.home}>
              {next ? t.t('ng-ui-fin-next') : t.t('ng-ui-fin-more')}
            </Button>
            {next ? (
              <span className="done-next-hint">
                {t.t('ng-ui-fin-next-hint', { name: next.name })}
                {step.pending > 0 ? ` · ${t.t('ng-ui-fin-waiting', { n: step.pending })}` : null}
              </span>
            ) : null}
            {/* weitere Dokumente warten (Stapelübersicht, ADR 0004 §3.8 `ShowBatch`): dieses zählt dort als signiert */}
            {view.actions.show_batch ? (
              <Button variant="ghost" size="sm" onPress={() => void dispatch({ type: 'show_batch' })}>
                <IconLayers size={15} />
                {t.t('ng-ui-batch-show')}
              </Button>
            ) : null}
          </div>
        </div>
      </section>
    </div>
  );
}
