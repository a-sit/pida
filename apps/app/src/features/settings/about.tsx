/**
 * Einstellungen › Über PIDA (ADR 0004 Nachtrag 21; Vorlage des Auftraggebers): oben das PIDA-Symbol mit „PIDA“ und
 * „Version 0.2.0 · 17.10.2026“ (Datum in der Oberflächensprache), darunter die Statuszeile der Update-Prüfung, die
 * Linkliste (Versionshinweise, Quellcode, Drittlizenzen, Datenschutz, auf dem Desktop die Logs), der Lizenztext von
 * A-SIT und als letzte Zeile „Urheberrecht © <Jahr> A-SIT · Lizenz EUPL-1.2“ (Jahr aus dem Versionsdatum).
 *
 * Alles kommt aus `ViewState::about` – der Kern entscheidet, ob, wann und mit welchem Ergebnis geprüft wird; die
 * Oberfläche meldet nur, dass der Abschnitt offen ist (`check_for_updates { explicit: false }`), bzw. den Klick
 * auf „Jetzt prüfen“/„Erneut prüfen“ (`explicit: true`). Statusänderungen sagt eine Live-Region an. Auf iOS/Android
 * liefert der Kern keine Prüfung (`update = null`) – dort entfällt die Zeile. Ältere Kerne ohne `about`: Version aus
 * der Shell (`appInfo`), ohne Datum und Prüfung.
 */
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { Link } from 'react-aria-components';
import { useApp } from '../../app/context';
import { isMobile, updateStore, type UpdateCheckView } from '../../core/types';
import { webProfile } from '../../core/web-profile';
import { Button, Spinner } from '../../ui/controls';
import { IconAlert, IconCheck, IconExternal, IconInfo, IconLock, IconRefresh } from '../../ui/Icons';
import { AsitLogo, LogoMark } from '../../ui/Logo';
import { Markup } from '../../ui/Markup';
import { LicensesDialog } from './licenses';

/** `JJJJ-MM-TT` als Datum der Oberflächensprache (de-AT „17.10.2026“, en-GB „17 Oct 2026“); sonst unverändert. */
export function formatReleaseDate(iso: string, lang: string): string {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(iso);
  if (!m) return iso;
  const d = new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]));
  return Number.isNaN(d.getTime()) ? iso : new Intl.DateTimeFormat(lang === 'de' ? 'de-AT' : 'en-GB', { dateStyle: 'medium' }).format(d);
}

type Tone = 'success' | 'info' | 'warning' | 'neutral';

/** Darstellung je Zustand: Ton, Symbol, Text und Aktion (Klick → `check_for_updates { explicit: true }`). */
function statusOf(u: UpdateCheckView, t: ReturnType<typeof useApp>['t']): { tone: Tone; icon: ReactNode; text: string; action: 'check' | 'retry' | 'download' | null } {
  switch (u.state) {
    case 'up_to_date':
      return { tone: 'success', icon: <IconCheck size={16} />, text: t.t('ng-ui-about-update-current'), action: null };
    case 'available':
      return { tone: 'info', icon: <IconInfo size={16} />, text: t.t('ng-ui-about-update-available', { version: u.latest_version ?? '' }), action: 'download' };
    case 'checking':
      return { tone: 'neutral', icon: <Spinner size={16} />, text: t.t('ng-ui-about-update-checking'), action: null };
    case 'failed':
      return { tone: 'warning', icon: <IconAlert size={16} />, text: t.t('ng-ui-about-update-failed', { reason: u.reason ? t.code(u.reason) : '' }), action: 'retry' };
    case 'disabled':
      return { tone: 'neutral', icon: <IconInfo size={16} />, text: t.t('ng-ui-about-update-disabled'), action: 'check' };
    case 'policy_off':
      return { tone: 'neutral', icon: <IconLock size={16} />, text: t.t('ng-ui-about-update-policy'), action: null };
    case 'unchecked':
      return { tone: 'neutral', icon: <IconRefresh size={16} />, text: t.t('ng-ui-about-update-unchecked'), action: 'check' };
  }
}

/**
 * Statuszeile der Update-Prüfung (Karte mit Statussymbol, Text als Live-Region, ggf. Aktion).
 *
 * Fokus: „Jetzt prüfen“/„Erneut prüfen“ verschwindet, sobald der Kern prüft („Suche nach Updates …“). Lag der Fokus
 * darauf, ginge er verloren – React Aria setzte ihn an den Anfang des Dialogs. Deshalb bekommt die Karte selbst den
 * Fokus (`tabIndex=-1`); den neuen Zustand sagt die Live-Region an, ein Tab weiter liegt ggf. der neue Knopf.
 */
export function UpdateStatus({ update }: { update: UpdateCheckView }) {
  const { t, view, dispatch, bridge, toast } = useApp();
  const s = statusOf(update, t);
  const card = useRef<HTMLDivElement>(null);
  const pressed = useRef(false);
  const check = () => {
    pressed.current = true;
    void dispatch({ type: 'check_for_updates', explicit: true });
  };
  useLayoutEffect(() => {
    const el = card.current;
    if (!pressed.current || !el || el.querySelector('.about-update-action')) return;
    pressed.current = false;
    // nur, wenn der Fokus mit dem Knopf verschwunden ist (nie von einer anderen Stelle wegziehen)
    const active = document.activeElement;
    if (active === null || active === document.body) el.focus();
  });
  const url = update.release_url;
  // iOS/Android: aktualisiert wird im Store (Nachtrag 31)
  const store = updateStore(view);
  return (
    <div
      ref={card}
      tabIndex={-1}
      className={`about-update is-${s.tone}`}
      data-testid="about-update"
      data-state={update.state}
      role="group"
      aria-label={t.t('ng-ui-about-update-region')}
    >
      <span className="about-update-badge" aria-hidden="true">
        {s.icon}
      </span>
      {/* dauerhafte Live-Region: jeder neue Zustand wird angesagt (der Knopf liegt außerhalb) */}
      <p className="about-update-text" role="status" aria-live="polite">
        {s.text}
      </p>
      {s.action === 'download' && url ? (
        <Link
          className="link about-update-action"
          aria-label={t.t(store ? `ng-ui-start-update-${store}-label` : 'ng-ui-about-update-download-label', { version: update.latest_version ?? '' })}
          onPress={() => bridge.openUrl(url).catch(() => toast(t.t('ng-ui-open-failed'), 'error'))}
        >
          {t.t(store ? `ng-ui-start-update-${store}` : 'ng-ui-about-update-download')}
          <IconExternal size={14} />
        </Link>
      ) : s.action === 'check' || s.action === 'retry' ? (
        <Button variant="secondary" size="sm" className="about-update-action" onPress={check}>
          {t.t(s.action === 'retry' ? 'ng-ui-about-update-retry' : 'ng-ui-about-update-check-now')}
        </Button>
      ) : null}
    </div>
  );
}

/** Eintrag der Linkliste: extern (Systembrowser, mit Hinweis für Screenreader) oder eine Aktion der App. */
function AboutLink({ label, href, onPress }: { label: string; href?: string; onPress?: () => void }) {
  const { t, bridge, toast } = useApp();
  const press = href ? () => void bridge.openUrl(href).catch(() => toast(t.t('ng-ui-open-failed'), 'error')) : onPress;
  return (
    <li>
      <Link className="link" onPress={press}>
        {label}
        {href ? (
          <>
            <IconExternal size={13} />
            <span className="visually-hidden"> {t.t('ng-ui-link-external')}</span>
          </>
        ) : null}
      </Link>
    </li>
  );
}

/** Website des Herausgebers. */
const ASIT_URL = 'https://www.a-sit.at';

/** Website von PIDA bzw. PDF-Over (Hauptseite für Nutzerinnen und Nutzer). */
export const WEBSITE_URL = 'https://pdf.a-sit.at';

/** Impressum und Datenschutzerklärung von PIDA (Webspace von pdf.a-sit.at). */
export const IMPRINT_URL = `${WEBSITE_URL}/impressum/`;
export const PRIVACY_URL = `${WEBSITE_URL}/datenschutz/`;

export function AboutRow({ onPrivacy }: { onPrivacy: () => void }) {
  const { t, view, bridge, toast, dispatch } = useApp();
  const about = view.about ?? null;
  const update = about?.update ?? null;
  const mobile = isMobile(view);
  // „Lizenzen anzeigen“: eigener Dialog über den Einstellungen (Backlog Nr. 2)
  const [licenses, setLicenses] = useState(false);
  // ältere Kerne ohne `about`: Version aus der Shell
  const [shellVersion, setShellVersion] = useState<string | null>(null);
  useEffect(() => {
    if (about) return undefined;
    let alive = true;
    bridge
      .appInfo()
      .then((i) => alive && setShellVersion(i.version))
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [about, bridge]);
  // Abschnitt offen: der Kern prüft, wenn die automatische Prüfung an ist und das letzte Ergebnis älter als eine Stunde
  const offersCheck = update !== null;
  useEffect(() => {
    if (offersCheck) void dispatch({ type: 'check_for_updates', explicit: false });
  }, [offersCheck, dispatch]);

  const version = about
    ? t.t('ng-ui-about-version-date', { version: about.version, date: formatReleaseDate(about.release_date, t.lang) })
    : t.t('ng-ui-about-version', { version: shellVersion ?? '…' });
  const year = about && /^\d{4}/.test(about.release_date) ? about.release_date.slice(0, 4) : String(new Date().getFullYear());
  return (
    <div className="about" data-testid="about">
      <div className="about-head">
        <LogoMark size={64} className="about-logo" />
        <div className="about-title">
          <h4 className="about-name">PIDA</h4>
          <p className="about-version">{version}</p>
        </div>
      </div>
      {update ? <UpdateStatus update={update} /> : null}
      <ul className="about-links" aria-label={t.t('ng-ui-about-links')}>
        <AboutLink label={t.t('ng-ui-about-website')} href={WEBSITE_URL} />
        <AboutLink label={t.t('ng-ui-about-imprint')} href={IMPRINT_URL} />
        {about ? <AboutLink label={t.t('ng-ui-about-release-notes')} href={about.release_notes_url} /> : null}
        {about ? <AboutLink label={t.t('ng-ui-about-source')} href={about.source_url} /> : null}
        <AboutLink label={t.t('ng-ui-about-notices')} onPress={() => setLicenses(true)} />
        <AboutLink label={t.t('ng-ui-about-privacy')} onPress={onPrivacy} />
        {mobile || webProfile() ? null : <AboutLink label={t.t('config-ShowLogDirectory')} onPress={() => void bridge.openLogDir().catch(() => toast(t.t('ng-ui-open-failed'), 'error'))} />}
      </ul>
      <p className="about-license pre-line">
        <Markup text={t.t('ng-ui-about-license')} />
      </p>
      <p className="about-license">{t.t('ng-ui-about-not-official')}</p>
      {/* Herausgeber (Wunsch des Auftraggebers): Logo mit Link zur Website von A-SIT */}
      <Link
        className="about-publisher"
        aria-label={`${t.t('ng-ui-about-publisher')} ${t.t('ng-ui-link-external')}`}
        onPress={() => void bridge.openUrl(ASIT_URL).catch(() => toast(t.t('ng-ui-open-failed'), 'error'))}
      >
        <AsitLogo height={32} />
      </Link>
      <p className="about-based-on">{t.t('ng-ui-about-based-on')}</p>
      <p className="about-copyright">{t.t('ng-ui-about-copyright', { year })}</p>
      {licenses ? <LicensesDialog onClose={() => setLicenses(false)} /> : null}
    </div>
  );
}
