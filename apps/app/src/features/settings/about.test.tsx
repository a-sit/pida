/**
 * Einstellungen › Über PIDA (ADR 0004 Nachtrag 21): Symbol, Name, „Version 0.2.0 · 17.10.2026“ (Datum in der
 * Oberflächensprache), die Statuszeile der Update-Prüfung in **jedem** Zustand (Zustände des echten Kerns aus
 * `core/fixtures/real-core-about-views.json`, erzeugt von `crates/core-api/tests/about.rs`), Live-Region,
 * „Jetzt prüfen“/„Erneut prüfen“ (`check_for_updates { explicit: true }`), „Herunterladen“ (Release-Seite im
 * Browser, nichts wird geladen), Linkliste, Urheberrecht als letzte Zeile, axe; Mobile ohne Prüfung.
 */
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { screen, waitFor, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { defaultSettings } from '../../core/defaults';
import type { UiEvent, ViewState } from '../../core/types';
import { expectNoAxeViolations } from '../../test/a11y';
import { lastEvent, renderApp } from '../../test/render';
import { formatReleaseDate } from './about';

const FIXTURE = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../core/fixtures/real-core-about-views.json');
const states = JSON.parse(readFileSync(FIXTURE, 'utf8')) as Record<string, ViewState>;

function state(name: string): ViewState {
  const v = states[name];
  if (!v) throw new Error(`Zustand ${name} fehlt in real-core-about-views.json`);
  return v;
}

function nav(dialog: HTMLElement, name: string | RegExp) {
  return within(within(dialog).getByRole('navigation', { name: /Bereiche|Sections/ })).getByRole('button', { name });
}

/** Einstellungen offen (Zustand des echten Kerns) → „Über PIDA“. */
async function openAbout(view: ViewState, lang: 'de' | 'en' = 'de') {
  const r = renderApp({ view });
  const dialog = await screen.findByRole('dialog', { name: lang === 'en' ? 'Settings' : 'Einstellungen' });
  await r.user.click(nav(dialog, lang === 'en' ? 'About PIDA' : 'Über PIDA'));
  const about = within(dialog).getByTestId('about');
  return { ...r, dialog, about };
}

/** Mock-Kern: Einstellungen über die Kopfzeile → „Über PIDA“. */
async function openMockAbout(options: Parameters<typeof renderApp>[0] = {}) {
  const r = renderApp(options);
  const settings = await screen.findByRole('button', { name: 'Einstellungen' });
  // erst nach dem Start des Mock-Kerns erreichbar (simulierte Latenz)
  await waitFor(() => expect(settings).toBeEnabled(), { timeout: 3000 });
  await r.user.click(settings);
  const dialog = await screen.findByRole('dialog', { name: 'Einstellungen' });
  await r.user.click(nav(dialog, 'Über PIDA'));
  return { ...r, dialog, about: within(dialog).getByTestId('about') };
}

const checks = (dispatched: UiEvent[]) => dispatched.filter((e) => e.type === 'check_for_updates');

describe('Über PIDA: Kopf, Links, Urheberrecht', () => {
  it('Symbol, Name, Version mit Datum; Linkliste; letzte Zeile Urheberrecht A-SIT mit dem Jahr des Versionsdatums', async () => {
    const { about, mock, user, dialog } = await openAbout(state('about_up_to_date'));
    expect(about.querySelector('svg.about-logo')).not.toBeNull();
    expect(within(about).getByRole('heading', { level: 4, name: 'PIDA' })).toBeInTheDocument();
    expect(within(about).getByText('Version 0.2.0 · 17.10.2026')).toBeInTheDocument();
    const links = within(within(about).getByRole('list', { name: 'Links' })).getAllByRole('link');
    expect(links.map((l) => l.textContent)).toEqual([
      'Website (pdf.a-sit.at) (öffnet im Browser)',
      'Impressum (öffnet im Browser)',
      'Versionshinweise (öffnet im Browser)',
      'Quellcode (öffnet im Browser)',
      'Lizenzen anzeigen',
      'Datenschutz',
      'Logs anzeigen',
    ]);
    await user.click(within(about).getByRole('link', { name: /Website/ }));
    await user.click(within(about).getByRole('link', { name: /Versionshinweise/ }));
    await user.click(within(about).getByRole('link', { name: /Quellcode/ }));
    expect(mock.openedUrls).toEqual(['https://pdf.a-sit.at', 'https://github.com/a-sit/pida/releases/tag/v0.2.0', 'https://github.com/a-sit/pida']);
    await user.click(within(about).getByRole('link', { name: 'Logs anzeigen' }));
    await waitFor(() => expect(mock.nativeCalls).toContain('openLogDir'));
    expect(within(about).getByText('Basiert auf PDF-Over von A-SIT.')).toBeInTheDocument();
    expect(within(about).getByText(/kein offizielles Produkt der Republik Österreich oder der A-Trust GmbH/)).toBeInTheDocument();
    const last = about.lastElementChild;
    expect(last).toHaveTextContent('Urheberrecht © 2026 A-SIT · Lizenz EUPL-1.2');
    expect(within(about).getByText(/Diese Software wird von A-SIT unter den Bedingungen der EUPL/)).toBeInTheDocument();
    await user.click(within(about).getByRole('link', { name: 'Datenschutz' }));
    expect(nav(dialog, 'Datenschutz')).toHaveAttribute('aria-current', 'true');
    // Datenschutz: ein Satz zur Update-Prüfung (Desktop)
    expect(within(dialog).getByText(/Die Update-Prüfung fragt bei GitHub \(api\.github\.com\) nach der neuesten Version/)).toHaveTextContent(
      'IP-Adresse dieses Geräts; Kennungen werden nicht übertragen',
    );
  });

  it('Englisch: Datum und Urheberrecht in der Oberflächensprache', async () => {
    const en = { ...state('about_up_to_date'), language: 'en' as const };
    const { about } = await openAbout(en, 'en');
    expect(within(about).getByText('Version 0.2.0 · 17 Oct 2026')).toBeInTheDocument();
    expect(within(about).getByRole('status')).toHaveTextContent('PIDA is up to date');
    expect(about.lastElementChild).toHaveTextContent('Copyright © 2026 A-SIT · Licence EUPL-1.2');
    expect(within(about).getByRole('link', { name: /Release notes/ })).toBeInTheDocument();
  });

  it('Versionsdatum: JJJJ-MM-TT in der Oberflächensprache, sonst unverändert', () => {
    expect(formatReleaseDate('2026-10-17', 'de')).toBe('17.10.2026');
    expect(formatReleaseDate('2026-09-08', 'de')).toBe('08.09.2026');
    expect(formatReleaseDate('2026-10-17', 'en')).toBe('17 Oct 2026');
    expect(formatReleaseDate('unbekannt', 'de')).toBe('unbekannt');
  });

  it('iOS: keine Update-Zeile (keine Verteilung über GitHub), keine Logs; Links und Urheberrecht bleiben; axe', async () => {
    const { about, mock } = await openAbout(state('about_ios'));
    expect(within(about).queryByRole('group', { name: 'Updates' })).toBeNull();
    expect(within(about).queryByRole('status')).toBeNull();
    expect(within(about).queryByRole('link', { name: /Logs/ })).toBeNull();
    expect(within(about).getByRole('link', { name: /Versionshinweise/ })).toBeInTheDocument();
    expect(about.lastElementChild).toHaveTextContent('Urheberrecht © 2026 A-SIT · Lizenz EUPL-1.2');
    expect(checks(mock.dispatched)).toEqual([]);
    await expectNoAxeViolations();
  });
});

describe('Über PIDA: Statuszeile in jedem Zustand des echten Kerns', () => {
  const cases: [string, string, string | null][] = [
    ['about_up_to_date', 'PIDA ist auf dem neuesten Stand', null],
    ['about_available', 'Version 0.3.0 ist verfügbar', 'Herunterladen'],
    ['about_checking', 'Suche nach Updates …', null],
    ['about_failed', 'Prüfung nicht möglich: zu viele Anfragen an GitHub – bitte später erneut prüfen', 'Erneut prüfen'],
    ['about_disabled', 'Automatische Prüfung ist ausgeschaltet', 'Jetzt prüfen'],
    ['about_policy_off', 'Von der Organisation deaktiviert', null],
    ['about_unchecked', 'Noch nicht nach Updates gesucht', 'Jetzt prüfen'],
  ];

  it('alle Zustände sind in den Proben', () => {
    expect(Object.keys(states).sort()).toEqual([...cases.map((c) => c[0]), 'about_ios'].sort());
  });

  it.each(cases)('%s: „%s“, Aktion %s; axe', async (name, text, action) => {
    const { about, mock, user } = await openAbout(state(name));
    const card = within(about).getByRole('group', { name: 'Updates' });
    expect(card).toHaveAttribute('data-state', state(name).about?.update?.state);
    expect(within(card).getByRole('status')).toHaveTextContent(text);
    // nie „auf dem neuesten Stand“ ohne erfolgreiche Prüfung
    if (name !== 'about_up_to_date') expect(card).not.toHaveTextContent('auf dem neuesten Stand');
    const buttons = within(card).queryAllByRole('button');
    const links = within(card).queryAllByRole('link');
    if (action === 'Herunterladen') {
      expect(buttons).toHaveLength(0);
      await user.click(within(card).getByRole('link', { name: 'Version 0.3.0 herunterladen (Release-Seite auf GitHub)' }));
      expect(mock.openedUrls).toEqual(['https://github.com/a-sit/pida/releases/tag/v0.3.0']);
    } else if (action) {
      expect(links).toHaveLength(0);
      await user.click(within(card).getByRole('button', { name: action }));
      expect(lastEvent(mock)).toEqual({ type: 'check_for_updates', explicit: true });
    } else {
      expect([...buttons, ...links]).toHaveLength(0);
    }
    if (name === 'about_checking') expect(card.querySelector('.spinner')).not.toBeNull();
    await expectNoAxeViolations();
  });

  it('beim Öffnen meldet die Oberfläche nur „Abschnitt offen“ – der Kern entscheidet, ob er prüft', async () => {
    const { mock } = await openAbout(state('about_disabled'));
    expect(checks(mock.dispatched)).toEqual([{ type: 'check_for_updates', explicit: false }]);
  });

  it('neue Version nach der Prüfung beim Start: Hinweis auf der Startseite, keine Meldung, kein Dialog', async () => {
    const v = state('about_available');
    expect(v.notices).toEqual([]);
    renderApp({ view: v });
    expect(await screen.findByTestId('update-hint')).toHaveTextContent('PIDA 0.3.0 ist verfügbar.');
    expect(screen.queryByRole('alertdialog')).toBeNull();
  });
});

describe('Über PIDA mit dem Mock-Kern: Abläufe und Live-Region', () => {
  it('noch nicht geprüft: Öffnen prüft – „Suche nach Updates …“, dann „auf dem neuesten Stand“ in derselben Live-Region', async () => {
    // simulierte Latenz 150 ms (Prüfung 300 ms): „prüft“ ist sicher zu sehen
    const { about, mock } = await openMockAbout({ update: 'unchecked', delayMs: 150 });
    const live = within(about).getByRole('status');
    expect(checks(mock.dispatched)).toEqual([{ type: 'check_for_updates', explicit: false }]);
    await waitFor(() => expect(live).toHaveTextContent('Suche nach Updates …'));
    await waitFor(() => expect(live).toHaveTextContent('PIDA ist auf dem neuesten Stand'), { timeout: 5000 });
    expect(within(about).getByRole('status')).toBe(live);
  });

  it('Schalter aus: „Jetzt prüfen“ prüft einmalig; Fehler → „Erneut prüfen“ → Erfolg', async () => {
    const base = defaultSettings();
    const settings = { ...base, general: { ...base.general, update_check: false } };
    const { about, mock, user } = await openMockAbout({ settings, update: 'failed' });
    const live = within(about).getByRole('status');
    expect(live).toHaveTextContent('Automatische Prüfung ist ausgeschaltet');
    expect(checks(mock.dispatched)).toEqual([{ type: 'check_for_updates', explicit: false }]);
    await user.click(within(about).getByRole('button', { name: 'Jetzt prüfen' }));
    await waitFor(() => expect(live).toHaveTextContent('Prüfung nicht möglich: zu viele Anfragen an GitHub'));
    await user.click(within(about).getByRole('button', { name: 'Erneut prüfen' }));
    await waitFor(() => expect(live).toHaveTextContent('PIDA ist auf dem neuesten Stand'));
    expect(checks(mock.dispatched).map((e) => e.type === 'check_for_updates' && e.explicit)).toEqual([false, true, true]);
  });

  it('Tastatur: „Jetzt prüfen“ verschwindet während der Prüfung – der Fokus bleibt in der Statuskarte (nicht am Anfang des Dialogs)', async () => {
    const base = defaultSettings();
    const settings = { ...base, general: { ...base.general, update_check: false } };
    const { about, user } = await openMockAbout({ settings, update: 'failed', delayMs: 150 });
    const card = within(about).getByRole('group', { name: 'Updates' });
    const live = within(card).getByRole('status');
    within(card).getByRole('button', { name: 'Jetzt prüfen' }).focus();
    await user.keyboard('{Enter}');
    await waitFor(() => expect(live).toHaveTextContent('Suche nach Updates …'));
    expect(within(card).queryByRole('button')).toBeNull();
    // React Aria hält den Fokus im Dialog; ohne Ziel spränge er an den Anfang (Schließen bzw. Suche)
    await new Promise((r) => setTimeout(r, 50));
    expect(document.activeElement).toBe(card);
    // Fehler: „Erneut prüfen“ erscheint, der Fokus bleibt auf der Karte (ein Tab weiter liegt der Knopf)
    await waitFor(() => expect(live).toHaveTextContent('Prüfung nicht möglich'));
    expect(document.activeElement).toBe(card);
    await user.tab();
    expect(document.activeElement).toBe(within(card).getByRole('button', { name: 'Erneut prüfen' }));
    await user.keyboard('{Enter}');
    await waitFor(() => expect(live).toHaveTextContent('PIDA ist auf dem neuesten Stand'));
    expect(document.activeElement).toBe(card);
    await expectNoAxeViolations();
  });

  it('Richtlinie: von der Organisation deaktiviert, keine Aktion; neue Version: Herunterladen', async () => {
    const { about } = await openMockAbout({ policy: { update_check: false } } as Parameters<typeof renderApp>[0]);
    expect(within(about).getByRole('status')).toHaveTextContent('Von der Organisation deaktiviert');
    expect(within(within(about).getByRole('group', { name: 'Updates' })).queryByRole('button')).toBeNull();
  });

  it('neue Version (Mock): „Version 0.3.0 ist verfügbar“ mit Link auf die Release-Seite', async () => {
    const { about, mock, user } = await openMockAbout({ update: 'available' });
    expect(within(about).getByRole('status')).toHaveTextContent('Version 0.3.0 ist verfügbar');
    await user.click(within(about).getByRole('link', { name: /Version 0\.3\.0 herunterladen/ }));
    expect(mock.openedUrls).toEqual(['https://github.com/a-sit/pida/releases/tag/v0.3.0']);
  });

  it('Android (Mock): keine Update-Zeile, keine Prüfung', async () => {
    const { about, mock } = await openMockAbout({ platform: 'android' });
    expect(within(about).queryByRole('group', { name: 'Updates' })).toBeNull();
    expect(checks(mock.dispatched)).toEqual([]);
    expect(within(about).getByText('Version 0.2.0 · 17.10.2026')).toBeInTheDocument();
  });
});
