import { describe, expect, it, vi } from 'vitest';
import { fetchLatestRelease, pickRelease, RELEASES_API, RELEASES_PAGE } from './releases';

const dl = (tag: string, name: string) => ({ name, browser_download_url: `https://github.com/a-sit/pida/releases/download/${tag}/${name}` });

/** Dateien wie in der Release v0.2.5 (plus Web-Pakete, die der Dialog nicht anbietet). */
function release(tag: string, extra: Record<string, unknown> = {}) {
  const v = tag.slice(1);
  return {
    tag_name: tag,
    html_url: `https://github.com/a-sit/pida/releases/tag/${tag}`,
    draft: false,
    prerelease: true,
    assets: [
      `pida-cli_${v}_macos_arm64`,
      `PIDA_${v}_android_arm64.apk`,
      `PIDA_${v}_linux_amd64.deb`,
      `PIDA_${v}_linux_x86_64.AppImage`,
      `PIDA_${v}_linux_x86_64.rpm`,
      `PIDA_${v}_macos_arm64.dmg`,
      `PIDA_${v}_windows_x64-portable.zip`,
      `PIDA_${v}_windows_x64-setup.exe`,
      `PIDA_${v}_windows_x64.msi`,
      `pida-web_${v}_amd64.deb`,
      'SHA256SUMS.txt',
    ].map((n) => dl(tag, n)),
    ...extra,
  };
}

describe('Download-Dialog: neueste Release von GitHub', () => {
  it('Installer je Plattform, empfohlener zuerst; Pre-release als Testversion', () => {
    const r = pickRelease([release('v0.2.5')]);
    expect(r?.version).toBe('0.2.5');
    expect(r?.prerelease).toBe(true);
    expect(r?.page).toBe('https://github.com/a-sit/pida/releases/tag/v0.2.5');
    expect(r?.installers.windows.map((i) => i.format)).toEqual(['.exe', 'MSI', 'ZIP']);
    expect(r?.installers.windows[0]?.url).toBe('https://github.com/a-sit/pida/releases/download/v0.2.5/PIDA_0.2.5_windows_x64-setup.exe');
    expect(r?.installers.macos.map((i) => i.format)).toEqual(['DMG']);
    // das Web-Paket (pida-web_…deb) ist kein Desktop-Installer
    expect(r?.installers.linux.map((i) => i.url.split('/').pop())).toEqual(['PIDA_0.2.5_linux_amd64.deb', 'PIDA_0.2.5_linux_x86_64.rpm', 'PIDA_0.2.5_linux_x86_64.AppImage']);
  });

  it('neueste veröffentlichte Release, Entwürfe übersprungen', () => {
    const r = pickRelease([release('v0.2.7', { draft: true }), release('v0.2.6', { prerelease: false }), release('v0.2.5')]);
    expect(r?.version).toBe('0.2.6');
    expect(r?.prerelease).toBe(false);
  });

  it('fremde Adressen aus der Antwort werden nicht übernommen', () => {
    const r = pickRelease([
      {
        tag_name: 'v1.0.0',
        html_url: 'https://evil.example/releases',
        assets: [{ name: 'PIDA_1.0.0_macos_arm64.dmg', browser_download_url: 'https://evil.example/x.dmg' }, dl('v1.0.0', 'PIDA_1.0.0_windows_x64-setup.exe')],
      },
    ]);
    expect(r?.page).toBe(RELEASES_PAGE);
    expect(r?.installers.macos).toEqual([]);
    expect(r?.installers.windows).toHaveLength(1);
  });

  it('unbrauchbare Antworten ergeben keine Release', () => {
    expect(pickRelease({ message: 'API rate limit exceeded' })).toBeNull();
    expect(pickRelease([])).toBeNull();
    expect(pickRelease([{ draft: true, tag_name: 'v1' }])).toBeNull();
  });

  it('Abfrage: Fehler und HTTP-Fehler ergeben null', async () => {
    const ok = vi.fn(async () => new Response(JSON.stringify([release('v0.2.5')]), { status: 200 }));
    expect((await fetchLatestRelease(undefined, ok))?.tag).toBe('v0.2.5');
    expect(ok).toHaveBeenCalledWith(RELEASES_API, expect.objectContaining({ credentials: 'omit' }));
    expect(await fetchLatestRelease(undefined, async () => new Response('{}', { status: 403 }))).toBeNull();
    expect(
      await fetchLatestRelease(undefined, async () => {
        throw new TypeError('offline');
      }),
    ).toBeNull();
  });
});
