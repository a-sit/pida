/**
 * Start/Leerzustand (Gestalt aus dem Prototyp, ADR 0004 §2.1): große Ablagefläche, „PDF auswählen“,
 * daneben der Slot „Zuletzt signiert“ (Paket UI-B, `RecentPanel`; ohne Liste eine Spalte). Einen Prüfmodus gibt
 * es nicht (ADR 0004 Nachtrag 15). Die Ablagefläche ist nur optisch: Dateien kommen über die
 * Fenster-Ereignisse der Shell (Pfade, `bridge.onFileDrag`); hervorgehoben wird sie beim Ziehen
 * (`dragActive`). Die Oberfläche filtert nicht nach Endung – der Kern meldet Nicht-PDFs.
 *
 * Mobile: keine Ablagefläche und keine Tastenkürzel, Knopf „Dokument auswählen“ (Dokument-Picker bzw.
 * SAF) und der Hinweis auf „Teilen“/„Öffnen in“; statt des Kürzels ⌘/Strg+D der Link „Mit Demodokument ausprobieren“.
 *
 * Web-Variante im Browser eines Telefons/Tablets (kein Hover, grober Zeiger): Titel und Demo-Link wie mobil, kein
 * Kürzel- und kein Mehrfachauswahl-Hinweis (die Kürzel-Tasten blendet `base.css` dort ohnehin aus).
 *
 * Unter dem Knopf steht je Plattform, wie die Dateiauswahl mehrere PDFs auf einmal übernimmt (ADR 0004 Nachtrag 17).
 *
 * Neue Version (Backlog Nr. 33): der Hinweis `UpdateHint` unter der Einleitung (nur, wenn der Kern ihn liefert).
 *
 * Geparkte Arbeit („Zur Startseite“, ADR 0004 Nachtrag 10): oben die Karte „In Bearbeitung“ (`ParkedCard`); neue
 * PDFs fragen dann vorher, ob sie zum Stapel kommen (`ParkedJoinDialog`, Shell).
 *
 * Unter der Ablagefläche die Karte „Dokument für andere vorbereiten“ mit „QR-Platzhalter erstellen“ (ADR 0004
 * Nachtrag 24).
 *
 * Web-Variante: unten Impressum und Datenschutz der Website, auf der PIDA läuft (`/impressum/`, `/datenschutz/`;
 * § 5 ECG verlangt ein leicht auffindbares Impressum).
 */
import { Link } from 'react-aria-components';
import { useApp } from '../../app/context';
import { isMobile } from '../../core/types';
import { hintOs, webProfile } from '../../core/web-profile';
import { AccessButton, Button, Keys, useModKey } from '../../ui/controls';
import { useMediaQuery } from '../../ui/hooks';
import { IconLock, IconQr, IconUpload } from '../../ui/Icons';
import { batchFlow, dropHintMsg } from '../batch/flow';
import { RECENT_AVAILABLE, RecentPanel } from '../recent';
import { ParkedCard } from './ParkedCard';
import { UpdateHint } from './UpdateHint';
import './start.css';

export function StartView() {
  const { t, view, dragActive, browse, startDemo, setOverlay } = useApp();
  const { mod, isMac } = useModKey();
  const mobile = isMobile(view);
  // Browser am Telefon/Tablet (Web-Variante): nichts zu ziehen, keine Kürzel, die Dateiauswahl gehört dem Browser
  const touchOnly = useMediaQuery('(hover: none) and (pointer: coarse)') && !mobile;
  const showRecent = RECENT_AVAILABLE && view.recent !== null;
  const single = !showRecent;
  return (
    <div className={`start-view${single ? ' is-single' : ''}`}>
      <div className="start-main">
        <h1 className="view-title start-title" id="main-heading" tabIndex={-1} data-main-heading="">
          {t.t('ng-ui-start-title')}
        </h1>
        <p className="view-lead">{t.t('ng-ui-start-lead')}</p>
        <UpdateHint />
        {view.parked ? <ParkedCard parked={view.parked} /> : null}
        <section className="dropzone start-drop" aria-label={t.t('ng-ui-start-drop-label')} data-drop-target={dragActive || undefined}>
          <div className={`drop-inner${dragActive ? ' is-target' : ''}`}>
            <span className="drop-icon" aria-hidden="true">
              <IconUpload size={30} />
            </span>
            {mobile || touchOnly ? (
              <p className="drop-title">{t.t('ng-ui-start-drop-title-mobile')}</p>
            ) : (
              <p className="drop-title" aria-live="polite">
                {dragActive ? t.t('ng-ui-start-drop-release') : t.t('ng-ui-start-drop-title')}
              </p>
            )}
            <AccessButton msg={mobile ? 'ng-ui-start-pick-mobile' : 'ng-ui-start-pick'} variant="primary" size="lg" onPress={() => void browse()} isDisabled={!view.actions.open_files} />
            {/* mehrere Dateien = Stapel: Wortlaut je Standard-Signiermittel (Kern: `batch_flow`), sonst neutral;
                unter dem Knopf und klein – erst die Handlung, dann der Hinweis */}
            <p className="drop-hint drop-hint-below">{t.t(mobile ? 'ng-ui-start-drop-hint-mobile' : dropHintMsg(batchFlow(view, view.signers.selected ?? view.signers.default)))}</p>
            {/* wie die System-Dateiauswahl mehrere PDFs übernimmt (macOS: Doppelklick nimmt nur eines; Android: Antippen
                nimmt sofort eines; ADR 0004 Nachtrag 17) */}
            {touchOnly ? null : <p className="drop-multi">{t.t('ng-ui-start-multi-hint', { os: hintOs(view.platform.os, isMac) })}</p>}
            <p className="drop-foot">
              <span className="drop-foot-item">
                <IconLock size={14} />
                {t.t('ng-ui-start-local')}
              </span>
              {mobile || touchOnly ? (
                view.actions.open_files ? (
                  <span className="drop-foot-item">
                    <Link className="link" onPress={() => void startDemo()}>
                      {t.t('ng-ui-start-demo')}
                    </Link>
                  </span>
                ) : null
              ) : (
                <span className="drop-foot-item">
                  <Keys keys={[mod, 'O']} label={`${mod}+O`} />
                  {t.t('ng-ui-start-shortcut')}
                </span>
              )}
            </p>
          </div>
        </section>
        {/* für Ersteller:innen: Platzhalter für andere – eigene Karte unter der Ablagefläche */}
        <section className="start-tool" aria-labelledby="start-tool-qr">
          <span className="start-tool-icon" aria-hidden="true">
            <IconQr size={24} />
          </span>
          <div className="start-tool-text">
            <h2 className="start-tool-title" id="start-tool-qr">
              {t.t('ng-ui-start-qr-title')}
            </h2>
            <p className="start-tool-desc">{t.t('ng-ui-start-qr-text')}</p>
          </div>
          <Button variant="secondary" className="start-tool-action" onPress={() => setOverlay('qr')}>
            <IconQr size={16} />
            {t.t('ng-ui-start-qr-action')}
          </Button>
        </section>
        {webProfile() ? <WebLegalLinks /> : null}
      </div>

      {single ? null : (
        <aside className="start-side" aria-label={t.t('ng-ui-start-side')}>
          <RecentPanel items={view.recent} />
        </aside>
      )}
    </div>
  );
}

/** Web-Variante: Impressum und Datenschutz der Website (gleicher Ursprung, neuer Tab – das Dokument bleibt offen). */
function WebLegalLinks() {
  const { t, bridge, toast } = useApp();
  const open = (path: string) => () =>
    void bridge.openUrl(new URL(path, window.location.origin).href).catch(() => toast(t.t('ng-ui-open-failed'), 'error'));
  return (
    <nav className="start-legal" aria-label={t.t('ng-ui-web-legal')}>
      <Link className="link" onPress={open('/impressum/')}>
        {t.t('ng-ui-web-imprint')}
      </Link>
      <span aria-hidden="true">·</span>
      <Link className="link" onPress={open('/datenschutz/')}>
        {t.t('ng-ui-web-privacy')}
      </Link>
    </nav>
  );
}

