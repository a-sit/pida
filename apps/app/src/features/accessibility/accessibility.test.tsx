/**
 * Barrierefreie Bedienung (docs/analyse/06): Schalter in der Kopfzeile (ersetzt keine Symbole), Voreinstellungen
 * (`withMode`), Platzierung ohne Vorschau im Inspektor (Methoden, Millimeter, Platzhalter-Nummer, freie Flächen),
 * ausführliche Ansage und „Vor dem Signieren prüfen“.
 */
import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { defaultAccessibility } from '../../core/defaults';
import type { AccessibilitySettings, UiEvent } from '../../core/types';
import { setWebProfile } from '../../core/web-profile';
import { translator } from '../../i18n/catalog';
import { expectNoAxeViolations } from '../../test/a11y';
import { lastEvent, renderApp } from '../../test/render';
import { documentView, workspaceView } from '../../test/views';
import { announceText } from '../workspace/placement';
import { resetA11yStartForTests } from './A11ySwitch';
import { formatMm, modeRequestedByUrl, parseMm, withMode } from './index';

const ON: AccessibilitySettings = withMode(defaultAccessibility(), true);

function a11yView(a: Partial<AccessibilitySettings> = {}, doc = documentView()) {
  return workspaceView(doc, { accessibility: { ...defaultAccessibility(), ...a } });
}

afterEach(() => {
  resetA11yStartForTests();
  setWebProfile(null);
  window.history.replaceState(null, '', '/');
});

describe('Voreinstellungen und Hilfen', () => {
  it('„Barrierefreie Bedienung“ stellt Methodenwahl, ausführliche Ansagen und Prüfung ein; aus = Standard, Schrittweite bleibt', () => {
    const a = { ...defaultAccessibility(), keyboard_step: 'mm5' as const };
    const on = withMode(a, true);
    expect(on).toEqual({ mode: true, start: 'methods', announce: 'detailed', announce_after_move: true, keyboard_step: 'mm5', check_before_sign: true });
    expect(withMode(on, false)).toEqual({ ...defaultAccessibility(), keyboard_step: 'mm5' });
  });

  it('Millimeter lesen und zeigen; Direktlink', () => {
    expect(parseMm('20,5')).toBe(20.5);
    expect(parseMm(' 120 ')).toBe(120);
    expect(parseMm('-3')).toBeNull();
    expect(parseMm('abc')).toBeNull();
    expect(parseMm('')).toBeNull();
    expect(formatMm(72, 'de')).toBe('25,4');
    expect(formatMm(72, 'en')).toBe('25.4');
    expect(modeRequestedByUrl('?barrierefrei=1')).toBe(true);
    expect(modeRequestedByUrl('?a11y=true')).toBe(true);
    expect(modeRequestedByUrl('?barrierefrei=0')).toBe(false);
    expect(modeRequestedByUrl('')).toBe(false);
  });
});

describe('Schalter in der Kopfzeile', () => {
  it('steht neben Befehlen, Sprache, Hilfe und Einstellungen und setzt die Voreinstellungen', async () => {
    const { mock, user } = renderApp({ view: a11yView() });
    const sw = await screen.findByRole('switch', { name: 'Barrierefreie Bedienung' });
    expect(sw).not.toBeChecked();
    expect(sw).toHaveAccessibleDescription(/Platzierung ohne Vorschau/);
    // ersetzt nichts
    expect(screen.getByRole('button', { name: 'Einstellungen' })).toBeInTheDocument();
    expect(within(screen.getByRole('banner')).getByRole('radio', { name: 'EN' })).toBeInTheDocument();
    await user.click(sw);
    expect(lastEvent(mock)).toEqual({ type: 'set_accessibility', accessibility: ON } satisfies UiEvent);
    await waitFor(() => expect(screen.getByTestId('live-region')).toHaveTextContent('Barrierefreie Bedienung ein.'));
  });

  it('Direktlink ?barrierefrei=1 schaltet beim Start ein; im Web merkt sich der Browser den Schalter', async () => {
    window.history.replaceState(null, '', '/?barrierefrei=1');
    const { mock } = renderApp({ view: a11yView() });
    await waitFor(() => expect(mock.dispatched).toContainEqual({ type: 'set_accessibility', accessibility: ON }));
  });

  it('im Web: gemerkter Schalter gilt beim nächsten Besuch', async () => {
    setWebProfile({ keystore: false });
    window.localStorage.setItem('pida-barrierefrei', '1');
    try {
      const { mock } = renderApp({ view: a11yView() });
      await waitFor(() => expect(mock.dispatched).toContainEqual({ type: 'set_accessibility', accessibility: ON }));
    } finally {
      window.localStorage.removeItem('pida-barrierefrei');
    }
  });
});

describe('Platzierung ohne Vorschau', () => {
  it('ohne Modus: eingeklappt unter „Platzierung“, keine Zahlenfelder sichtbar', async () => {
    renderApp({ view: a11yView() });
    const trigger = await screen.findByRole('button', { name: 'Ohne Vorschau platzieren …' });
    expect(trigger).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
  });

  it('mit Modus: „Platzierung der Signatur“ ganz oben, Fokus auf der Methode, ohne axe-Fehler', async () => {
    const { container } = renderApp({ view: a11yView(ON) });
    const panel = await screen.findByTestId('placement-panel');
    expect(within(panel).getByRole('heading', { name: 'Platzierung der Signatur' })).toBeInTheDocument();
    const inspector = screen.getByRole('complementary', { name: /./ });
    expect(inspector.querySelector('.insp-section')).toBe(panel);
    const methods = within(panel).getByRole('radiogroup', { name: 'Wie soll die Signatur platziert werden?' });
    await waitFor(() => expect(within(methods).getByRole('radio', { name: /Automatisch/ })).toHaveFocus());
    expect(within(panel).getByTestId('placement-current')).toHaveTextContent('Seite 3 von 3, linke obere Ecke 64 mm von links, 106 mm von unten. Lage:');
    await expectNoAxeViolations(container);
  });

  it('Millimeter: Bereich wird geprüft, gültige Werte gehen als move_block (pt) an den Kern', async () => {
    const { mock, user } = renderApp({ view: a11yView(ON) });
    const panel = await screen.findByTestId('placement-panel');
    await user.click(within(panel).getByRole('radio', { name: /Position in Millimetern/ }));
    const page = within(panel).getByRole('textbox', { name: 'Seite' });
    const x = within(panel).getByRole('textbox', { name: 'Von links (mm)' });
    const y = within(panel).getByRole('textbox', { name: 'Von unten (mm)' });
    // vorbelegt mit der aktuellen Lage
    expect(page).toHaveValue('3');
    expect(x).toHaveValue('64,4');
    expect(y).toHaveValue('105,8');
    expect(within(panel).getByText(/linke obere Ecke des Signaturblocks ab der linken unteren Seitenecke/)).toHaveTextContent('Signaturblock 81,1 × 28,9 mm');
    await user.clear(page);
    await user.type(page, '9');
    await user.clear(x);
    await user.type(x, '200');
    await user.click(within(panel).getByRole('button', { name: 'Position übernehmen' }));
    expect(page).toHaveAccessibleDescription('Seite 1 bis 3.');
    expect(x).toHaveAccessibleDescription(/^0 bis 128,9 mm/);
    expect(lastEvent(mock)?.type).not.toBe('move_block');
    // Fokus springt auf das erste falsche Feld (keine zweite Ansage über die Live-Region)
    await waitFor(() => expect(page).toHaveFocus());
    expect(screen.getByTestId('live-region')).not.toHaveTextContent(/Bitte prüfen/);
    await user.clear(page);
    await user.type(page, '1');
    await user.clear(x);
    await user.type(x, '20,5');
    await user.clear(y);
    await user.type(y, '120');
    await user.click(within(panel).getByRole('button', { name: 'Position übernehmen' }));
    const e = lastEvent(mock);
    expect(e?.type).toBe('move_block');
    if (e?.type !== 'move_block') return;
    expect(e.page).toBe(0);
    expect(e.x).toBeCloseTo((20.5 * 72) / 25.4, 3);
    expect(e.y).toBeCloseTo((120 * 72) / 25.4, 3);
  });

  it('Platzhalter: Auswahlliste mit Nummer, Seite und Lage; Wahl setzt das Ziel; kein Nummernfeld', async () => {
    const doc = documentView({ qr: { page: 2, rect: { left: 320, right: 384, top: 532, bottom: 468 }, id: '0002' } });
    const { mock, user } = renderApp({ view: a11yView(ON, doc) });
    const panel = await screen.findByTestId('placement-panel');
    await user.click(within(panel).getByRole('radio', { name: /Platzhalter im Dokument/ }));
    const found = within(panel).getByRole('radiogroup', { name: 'Im Dokument gefunden' });
    await user.click(within(found).getByRole('radio', { name: /^QR-Code-Platzhalter 0002 ?Seite 3, / }));
    expect(lastEvent(mock)).toEqual({ type: 'use_target', target: { kind: 'qr' } });
    expect(within(panel).queryByRole('textbox')).not.toBeInTheDocument();
  });

  it('ohne QR-Platzhalter: Hinweis auf die Platzhaltersuche', async () => {
    const { user } = renderApp({ view: a11yView(ON) });
    const panel = await screen.findByTestId('placement-panel');
    await user.click(within(panel).getByRole('radio', { name: /Platzhalter im Dokument/ }));
    expect(within(panel).getByText('Kein QR-Code-Platzhalter und kein leeres Signaturfeld gefunden.')).toBeInTheDocument();
    expect(within(panel).getByText(/Signaturplatzhalter automatisch suchen/)).toBeInTheDocument();
    // nichts gefunden: keine Liste und kein Nummernfeld
    expect(within(panel).queryByRole('radiogroup', { name: 'Im Dokument gefunden' })).not.toBeInTheDocument();
    expect(within(panel).queryByRole('textbox')).not.toBeInTheDocument();
  });

  it('freie Flächen als Liste; Wahl setzt den Block in die Fläche', async () => {
    const { mock, user } = renderApp({ view: a11yView(ON) });
    const panel = await screen.findByTestId('placement-panel');
    await user.click(within(panel).getByRole('radio', { name: /Freie Fläche oder selbst verschieben/ }));
    expect(mock.dispatched.some((e) => e.type === 'inspect_pages' && e.pages.includes(2))).toBe(true);
    await user.click(within(panel).getByRole('radio', { name: /Freie Fläche 1: Seite 3, unten mittig, 170,4 × 105,1 mm/ }));
    const e = lastEvent(mock);
    expect(e?.type).toBe('move_block');
    if (e?.type === 'move_block') {
      expect(e.page).toBe(2);
      expect(e.x).toBeCloseTo(56 + (483 - 230) / 2, 3);
      expect(e.y).toBe(334);
    }
  });

  it('automatisch: auto_place', async () => {
    const { mock, user } = renderApp({ view: a11yView(ON) });
    const panel = await screen.findByTestId('placement-panel');
    await user.click(within(panel).getByRole('button', { name: 'Automatisch platzieren' }));
    expect(lastEvent(mock)).toEqual({ type: 'auto_place' });
  });

  it('Ansagen anpassen direkt im Panel (set_accessibility)', async () => {
    const { mock, user } = renderApp({ view: a11yView(ON) });
    const panel = await screen.findByTestId('placement-panel');
    await user.click(within(panel).getByRole('button', { name: 'Ansagen und Bedienung anpassen' }));
    await user.click(within(panel).getByRole('radio', { name: 'Kurz' }));
    expect(lastEvent(mock)).toEqual({ type: 'set_accessibility', accessibility: { ...ON, announce: 'short' } });
    await user.click(within(panel).getByRole('radio', { name: '10 mm' }));
    expect(lastEvent(mock)).toEqual({ type: 'set_accessibility', accessibility: { ...ON, keyboard_step: 'mm10' } });
  });
});

describe('Ansage und Prüfung vor dem Signieren', () => {
  it('ausführlich: zusätzlich die Lage in Worten', () => {
    const t = translator('de');
    expect(announceText(t, documentView(), [], 'detailed')).toBe('Seite 3 von 3, linke obere Ecke 64 mm von links, 106 mm von unten. Lage: unten mittig. Freie Fläche.');
  });

  it('„Vor dem Signieren prüfen“: erst die Zusammenfassung, „Signieren“ darin signiert, „Zurück“ nicht', async () => {
    const doc = documentView({ check: { for_revision: 1, overlaps_content: true, overlaps_widget: false, clamped: false, placeholder_uncovered: false, overlaps_block: false } });
    const { mock, user } = renderApp({ view: a11yView({ check_before_sign: true }, doc) });
    const before = mock.dispatched.length;
    await user.click(await screen.findByRole('button', { name: /^Signieren/ }));
    let dialog = await screen.findByRole('dialog', { name: 'Vor dem Signieren prüfen' });
    expect(within(dialog).getByTestId('sign-check-placement')).toHaveTextContent('Seite 3 von 3, linke obere Ecke 64 mm von links');
    expect(within(dialog).getByTestId('sign-check-overlap')).toHaveTextContent('überdeckt Inhalt');
    expect(within(dialog).getByRole('button', { name: 'Signieren' })).toHaveFocus();
    await user.click(within(dialog).getByRole('button', { name: 'Zurück zur Platzierung' }));
    expect(screen.queryByRole('dialog', { name: 'Vor dem Signieren prüfen' })).not.toBeInTheDocument();
    expect(mock.dispatched.slice(before).some((e) => e.type === 'sign_with')).toBe(false);
    await user.click(screen.getByRole('button', { name: /^Signieren/ }));
    dialog = await screen.findByRole('dialog', { name: 'Vor dem Signieren prüfen' });
    await user.click(within(dialog).getByRole('button', { name: 'Signieren' }));
    expect(lastEvent(mock)?.type).toBe('sign_with');
  });

  it('ohne die Einstellung signiert „Signieren“ wie bisher sofort', async () => {
    const { mock, user } = renderApp({ view: a11yView() });
    await user.click(await screen.findByRole('button', { name: /^Signieren/ }));
    expect(lastEvent(mock)?.type).toBe('sign_with');
    act(() => undefined);
    expect(screen.queryByRole('dialog', { name: 'Vor dem Signieren prüfen' })).not.toBeInTheDocument();
  });
});

describe('Einstellungen › Barrierefreiheit', () => {
  it('Bereich mit Schalter, Beginn, Prüfung, Ansagen und Schrittweite; der Schalter setzt alles gemeinsam', async () => {
    const { mock, user } = renderApp();
    await user.click(await screen.findByRole('button', { name: 'Einstellungen' }));
    const dialog = await screen.findByRole('dialog', { name: 'Einstellungen' });
    await user.click(within(within(dialog).getByRole('navigation', { name: 'Bereiche' })).getByRole('button', { name: 'Barrierefreiheit' }));
    expect(within(dialog).getByRole('radiogroup', { name: 'Platzierung beginnt mit' })).toBeInTheDocument();
    expect(within(dialog).getByRole('switch', { name: 'Vor dem Signieren prüfen' })).not.toBeChecked();
    expect(within(dialog).getByRole('radiogroup', { name: 'Ansagen für Screenreader' })).toBeInTheDocument();
    expect(within(dialog).getByText(/richten sich nach den Einstellungen Ihres Systems/)).toBeInTheDocument();
    await user.click(within(dialog).getByRole('switch', { name: 'Barrierefreie Bedienung' }));
    const apply = mock.dispatched.filter((e) => e.type === 'apply_settings').at(-1);
    expect(apply?.type === 'apply_settings' ? apply.changes : null).toEqual({
      'accessibility.mode': true,
      'accessibility.start': 'methods',
      'accessibility.announce': 'detailed',
      'accessibility.announce_after_move': true,
      'accessibility.check_before_sign': true,
    });
    await waitFor(() => expect(within(dialog).getByRole('switch', { name: 'Vor dem Signieren prüfen' })).toBeChecked());
  });

  it('Suche: „screenreader“ und „blind“ finden den Bereich', async () => {
    const { user } = renderApp();
    await user.click(await screen.findByRole('button', { name: 'Einstellungen' }));
    const dialog = await screen.findByRole('dialog', { name: 'Einstellungen' });
    await user.type(within(dialog).getByRole('searchbox', { name: 'Einstellungen durchsuchen' }), 'blind');
    expect(within(dialog).getByRole('switch', { name: 'Barrierefreie Bedienung' })).toBeInTheDocument();
  });
});

describe('Prüfung 30.10.2026 (WCAG)', () => {
  it('2.1.4: die Einzeltaste „?“ ist im barrierefreien Modus aus, sonst öffnet sie die Kürzel', async () => {
    renderApp({ view: a11yView(ON) });
    await screen.findByTestId('placement-panel');
    fireEvent.keyDown(window, { key: '?' });
    expect(screen.queryByRole('dialog', { name: 'Tastenkürzel' })).not.toBeInTheDocument();
  });

  it('4.1.3: die Ansage-Region bleibt bei offenen Dialogen erreichbar (React Aria macht sie nicht inert)', async () => {
    const { user } = renderApp();
    await user.click(await screen.findByRole('button', { name: /QR-Platzhalter erstellen/ }));
    await screen.findByRole('dialog', { name: 'QR-Platzhalter erstellen' });
    const region = screen.getByTestId('live-region');
    expect(region).toHaveAttribute('data-live-announcer', 'true');
    expect(region.closest('[inert], [aria-hidden="true"]')).toBeNull();
  });
});
