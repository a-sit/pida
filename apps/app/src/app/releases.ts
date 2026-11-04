/**
 * Web-Variante, Download-Dialog: neueste Release von GitHub (auch Testversionen = Pre-releases) und daraus die
 * Installer je Plattform. Abgefragt wird erst beim Öffnen des Dialogs (anonym, GitHub erlaubt 60 Abfragen pro Stunde
 * und Adresse); scheitert die Abfrage oder fehlt eine Datei, verlinkt der Dialog die Release-Seite.
 *
 * Dateinamen wie in den Releases: `PIDA_<Version>_windows_x64-setup.exe`, `…_windows_x64.msi`,
 * `…_windows_x64-portable.zip`, `…_macos_arm64.dmg`, `…_linux_amd64.deb`, `…_linux_x86_64.rpm`,
 * `…_linux_x86_64.AppImage` (siehe Release-Ablauf).
 */

/** Releases-Abfrage (neueste zuerst). */
export const RELEASES_API = 'https://api.github.com/repos/a-sit/pida/releases?per_page=10';
/** Alle Releases (`/releases/latest` kennt keine Pre-releases). */
export const RELEASES_PAGE = 'https://github.com/a-sit/pida/releases';

export type DesktopPlatform = 'windows' | 'macos' | 'linux';

/** Ein Installer: Format (Kurzname) und Download-Adresse. */
export interface Installer {
  format: string;
  url: string;
}

export interface LatestRelease {
  /** Tag, z. B. `v0.2.5` */
  tag: string;
  /** Version ohne `v` */
  version: string;
  prerelease: boolean;
  /** Release-Seite */
  page: string;
  /** Installer je Plattform, der erste ist der empfohlene */
  installers: Record<DesktopPlatform, Installer[]>;
}

/** Erkennung der Dateien je Plattform, in der Reihenfolge der Anzeige (erster = Hauptlink). */
const PATTERNS: Record<DesktopPlatform, { format: string; test: RegExp }[]> = {
  windows: [
    { format: '.exe', test: /^PIDA_[^_]+_windows_x64-setup\.exe$/ },
    { format: 'MSI', test: /^PIDA_[^_]+_windows_x64\.msi$/ },
    { format: 'ZIP', test: /^PIDA_[^_]+_windows_x64-portable\.zip$/ },
  ],
  macos: [{ format: 'DMG', test: /^PIDA_[^_]+_macos_arm64\.dmg$/ }],
  linux: [
    { format: '.deb', test: /^PIDA_[^_]+_linux_amd64\.deb$/ },
    { format: '.rpm', test: /^PIDA_[^_]+_linux_x86_64\.rpm$/ },
    { format: 'AppImage', test: /^PIDA_[^_]+_linux_x86_64\.AppImage$/ },
  ],
};

interface ApiAsset {
  name?: unknown;
  browser_download_url?: unknown;
}
interface ApiRelease {
  tag_name?: unknown;
  html_url?: unknown;
  draft?: unknown;
  prerelease?: unknown;
  assets?: unknown;
}

/** Nur Adressen auf GitHub übernehmen (die Antwort ist fremde Eingabe). */
function githubUrl(value: unknown): string | null {
  if (typeof value !== 'string') return null;
  try {
    const u = new URL(value);
    return u.protocol === 'https:' && (u.hostname === 'github.com' || u.hostname.endsWith('.github.com')) ? u.href : null;
  } catch {
    return null;
  }
}

/** Neueste veröffentlichte Release (kein Entwurf) aus der Antwort der API; `null` = keine brauchbare. */
export function pickRelease(json: unknown): LatestRelease | null {
  if (!Array.isArray(json)) return null;
  const release = (json as ApiRelease[]).find((r) => r && r.draft !== true && typeof r.tag_name === 'string');
  if (!release) return null;
  const tag = release.tag_name as string;
  const assets = (Array.isArray(release.assets) ? (release.assets as ApiAsset[]) : []).flatMap((a) => {
    const url = githubUrl(a?.browser_download_url);
    return typeof a?.name === 'string' && url ? [{ name: a.name, url }] : [];
  });
  const installers = {} as Record<DesktopPlatform, Installer[]>;
  for (const platform of Object.keys(PATTERNS) as DesktopPlatform[]) {
    installers[platform] = PATTERNS[platform].flatMap(({ format, test }) => {
      const asset = assets.find((a) => test.test(a.name));
      return asset ? [{ format, url: asset.url }] : [];
    });
  }
  return {
    tag,
    version: tag.replace(/^v/, ''),
    prerelease: release.prerelease === true,
    page: githubUrl(release.html_url) ?? RELEASES_PAGE,
    installers,
  };
}

/** Abfrage bei GitHub; `null` bei Fehler, Zeitüberschreitung oder Abbruch. */
export async function fetchLatestRelease(signal?: AbortSignal, fetchFn: typeof fetch = fetch): Promise<LatestRelease | null> {
  try {
    const r = await fetchFn(RELEASES_API, { headers: { Accept: 'application/vnd.github+json' }, signal, cache: 'no-cache', credentials: 'omit', referrerPolicy: 'no-referrer' });
    if (!r.ok) return null;
    return pickRelease(await r.json());
  } catch {
    return null;
  }
}
