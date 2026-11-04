/**
 * Web-Variante: Banner ganz oben „PIDA gibt es auch als App“ mit den Plattformen (Windows, macOS, Linux, iOS,
 * Android). Ein Klick öffnet den Dialog mit den Downloads: Desktop direkt mit den Installern der neuesten Release
 * (beim Öffnen von GitHub abgefragt, `releases.ts`; sonst die Release-Seite), iOS und Android
 * mit QR-Code zum Store – vorerst Platzhalter (keine Store-Veröffentlichung geplant). Nur sichtbar, wenn
 * `webProfile()` gesetzt ist; ausblendbar bis zum Neuladen (nichts wird gespeichert).
 */
import { useEffect, useState } from 'react';
import { Button as AriaButton } from 'react-aria-components';
import { IconButton } from '../ui/controls';
import { ModalShell, DialogHeader } from '../ui/Dialog';
import { IconClose, IconDownload, IconExternal } from '../ui/Icons';
import { useApp } from './context';
import { fetchLatestRelease, RELEASES_PAGE, type DesktopPlatform, type LatestRelease } from './releases';
import './download.css';

/** Alle Releases (Rückfall, wenn die Abfrage scheitert; `/releases/latest` kennt keine Pre-releases). */
export const RELEASES_URL = RELEASES_PAGE;
/** Projektseite (Quellcode, alle Releases). */
export const PROJECT_URL = 'https://github.com/a-sit/pida';

type Platform = 'windows' | 'macos' | 'linux' | 'ios' | 'android';
const DESKTOP: readonly DesktopPlatform[] = ['windows', 'macos', 'linux'];
const MOBILE: readonly Platform[] = ['ios', 'android'];
const NAME: Record<Platform, string> = { windows: 'Windows', macos: 'macOS', linux: 'Linux', ios: 'iOS', android: 'Android' };

/**
 * Schlichte Plattform-Symbole (Strichstil wie `ui/Icons`, keine Markenlogos); `colored` färbt sie im Download-Dialog in
 * den Farben der Plattform ein (download.css, `.dl-ico-<plattform>`).
 */
function PlatformIcon({ platform, size = 18, colored = false }: { platform: Platform; size?: number; colored?: boolean }) {
  const common = { width: size, height: size, viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', strokeWidth: 1.75, strokeLinecap: 'round' as const, strokeLinejoin: 'round' as const, 'aria-hidden': true, focusable: false, className: colored ? `icon dl-ico dl-ico-${platform}` : 'icon' };
  switch (platform) {
    case 'windows':
      return (
        <svg {...common}>
          <path d="M4 5.5 11 4.5v7H4z" />
          <path d="M13 4.2 20 3v8.5h-7z" />
          <path d="M4 12.5h7v7l-7-1z" />
          <path d="M13 12.5h7V21l-7-1.2z" />
        </svg>
      );
    case 'macos':
      return (
        <svg {...common}>
          <path d="M15.5 7.5c-1.4 0-2.2.8-3.5.8s-2-.8-3.4-.8C6.4 7.5 4.5 9.3 4.5 12.4c0 3.6 2.4 7.6 4.3 7.6 1 0 1.6-.6 3.2-.6s2.1.6 3.2.6c1.4 0 2.8-2 3.6-4.1-1.6-.7-2.4-2-2.4-3.6 0-1.4.7-2.6 1.9-3.3-.8-1-1.8-1.5-2.8-1.5z" />
          <path d="M12.2 7.4c0-1.8 1.2-3.3 2.9-3.6.1 1.9-1.2 3.4-2.9 3.6z" />
        </svg>
      );
    case 'linux':
      return (
        <svg {...common}>
          <path d="M12 3.5c-2 0-3.2 1.6-3.2 3.8 0 1.6-.5 2.6-1.4 3.9-1 1.4-1.9 3.1-1.9 5 0 2.7 2.9 4.3 6.5 4.3s6.5-1.6 6.5-4.3c0-1.9-.9-3.6-1.9-5-.9-1.3-1.4-2.3-1.4-3.9 0-2.2-1.2-3.8-3.2-3.8z" />
          <path d="M10.5 8.2h.01M13.5 8.2h.01" />
          <path d="M11 10.2h2l-1 .9z" />
        </svg>
      );
    case 'ios':
      return (
        <svg {...common}>
          <rect x="6.5" y="2.5" width="11" height="19" rx="2.5" />
          <path d="M11 18.5h2" />
        </svg>
      );
    case 'android':
      return (
        <svg {...common}>
          <path d="M5 15.5a7 7 0 0 1 14 0z" />
          <path d="M8.5 9.8 6.8 7.2M15.5 9.8l1.7-2.6" />
          <path d="M9.5 13h.01M14.5 13h.01" />
          <path d="M5 18h14" />
        </svg>
      );
  }
}

/** Platzhalter für den QR-Code zum Store (bis zur Veröffentlichung). */
function QrPlaceholder({ label }: { label: string }) {
  return (
    <div className="dl-qr" role="img" aria-label={label}>
      <svg viewBox="0 0 21 21" width="96" height="96" aria-hidden="true" focusable="false">
        {/* nur die drei Ortungsmuster eines QR-Codes, kein echter Code */}
        {(
          [
            [0, 0],
            [14, 0],
            [0, 14],
          ] as const
        ).map(([x, y]) => (
          <g key={`${x}-${y}`} fill="currentColor">
            <path d={`M${x} ${y}h7v7h-7z M${x + 1} ${y + 1}v5h5v-5z`} fillRule="evenodd" />
            <rect x={x + 2} y={y + 2} width="3" height="3" />
          </g>
        ))}
      </svg>
    </div>
  );
}

/** Stand der Abfrage: `undefined` = läuft, `null` = gescheitert. */
type ReleaseState = LatestRelease | null | undefined;

function DownloadsDialog({ onClose }: { onClose: () => void }) {
  const { t, bridge, toast } = useApp();
  const open = (url: string) => void bridge.openUrl(url).catch(() => toast(t.t('ng-ui-open-failed'), 'error'));
  const [release, setRelease] = useState<ReleaseState>(undefined);
  useEffect(() => {
    const abort = new AbortController();
    let alive = true;
    const timer = setTimeout(() => abort.abort(), 8000);
    void fetchLatestRelease(abort.signal).then((r) => {
      if (alive) setRelease(r);
    });
    return () => {
      alive = false;
      clearTimeout(timer);
      abort.abort();
    };
  }, []);
  const page = release?.page ?? RELEASES_URL;
  return (
    <ModalShell onClose={onClose} size="md" className="dl-modal">
      <DialogHeader title={t.t('ng-ui-shell-dl-title')} subtitle={t.t('ng-ui-shell-dl-subtitle')} onClose={onClose} />
      <div className="dl-body">
        <section aria-labelledby="dl-desktop">
          <h3 className="dl-section" id="dl-desktop">
            {t.t('ng-ui-shell-dl-desktop')}
          </h3>
          <p className="dl-release" role="status">
            {release === undefined
              ? t.t('ng-ui-shell-dl-loading')
              : release
                ? t.t(release.prerelease ? 'ng-ui-shell-dl-version-test' : 'ng-ui-shell-dl-version', { version: release.version })
                : t.t('ng-ui-shell-dl-unavailable')}
          </p>
          <ul className="dl-desktop">
            {DESKTOP.map((p) => {
              const [main, ...more] = release?.installers[p] ?? [];
              const label = (format: string) => `${NAME[p]}: ${t.t('ng-ui-shell-dl-download')} (${format})`;
              return (
                <li key={p} className="dl-platform">
                  <PlatformIcon platform={p} size={32} colored />
                  <span className="dl-platform-name">{NAME[p]}</span>
                  {main ? (
                    <>
                      <AriaButton className="btn btn-primary btn-sm dl-get" onPress={() => open(main.url)} aria-label={label(main.format)}>
                        <IconDownload size={15} />
                        {main.format}
                      </AriaButton>
                      {more.length > 0 ? (
                        <span className="dl-more">
                          {more.map((i) => (
                            <AriaButton key={i.format} className="link" onPress={() => open(i.url)} aria-label={label(i.format)}>
                              {i.format}
                            </AriaButton>
                          ))}
                        </span>
                      ) : null}
                    </>
                  ) : (
                    <AriaButton className="link dl-platform-hint" onPress={() => open(page)} isDisabled={release === undefined}>
                      {t.t('ng-ui-shell-dl-github')}
                      <IconExternal size={13} />
                    </AriaButton>
                  )}
                </li>
              );
            })}
          </ul>
          <p className="dl-note">{t.t('ng-ui-shell-dl-from-github')}</p>
        </section>
        <section aria-labelledby="dl-mobile">
          <h3 className="dl-section" id="dl-mobile">
            {t.t('ng-ui-shell-dl-mobile')}
          </h3>
          <ul className="dl-mobile">
            {MOBILE.map((p) => (
              <li key={p} className="dl-store">
                <QrPlaceholder label={t.t('ng-ui-shell-dl-qr', { platform: NAME[p] })} />
                <p className="dl-platform-name">
                  <PlatformIcon platform={p} size={20} colored />
                  {NAME[p]}
                </p>
                <p className="dl-platform-hint">{t.t(p === 'ios' ? 'ng-ui-shell-dl-appstore' : 'ng-ui-shell-dl-play')}</p>
              </li>
            ))}
          </ul>
        </section>
        <p className="dl-github">
          {t.t('ng-ui-shell-dl-source')}{' '}
          <AriaButton className="link" onPress={() => open(PROJECT_URL)}>
            github.com/a-sit/pida
            <IconExternal size={13} />
            <span className="sr-only"> {t.t('ng-ui-link-external')}</span>
          </AriaButton>
        </p>
      </div>
    </ModalShell>
  );
}

export function DownloadBanner() {
  const { t } = useApp();
  const [hidden, setHidden] = useState(false);
  const [open, setOpen] = useState(false);
  if (hidden) return null;
  return (
    <div className="dl-banner" role="region" aria-label={t.t('ng-ui-shell-dl-region')}>
      <AriaButton className="dl-banner-main" onPress={() => setOpen(true)}>
        <span className="dl-banner-icons" aria-hidden="true">
          {[...DESKTOP, ...MOBILE].map((p) => (
            <PlatformIcon key={p} platform={p} size={22} />
          ))}
        </span>
        <span className="dl-banner-text">{t.t('ng-ui-shell-dl-banner')}</span>
        <span className="dl-banner-cta">{t.t('ng-ui-shell-dl-cta')}</span>
      </AriaButton>
      <IconButton label={t.t('ng-ui-shell-dl-hide')} onPress={() => setHidden(true)} className="dl-banner-close" tooltip={false}>
        <IconClose size={16} />
      </IconButton>
      {open ? <DownloadsDialog onClose={() => setOpen(false)} /> : null}
    </div>
  );
}
