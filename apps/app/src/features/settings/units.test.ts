/**
 * Reine Hilfen der Einstellungen (Paket UI-B, ADR 0004 E3): Suche mit Synonymen (`.keywords`), Sichtbarkeit
 * der Zeilen je Plattform/Profil, Logo-Größe in cm, Richtlinien-Sperren
 * aus dem vorgeschlagenen Vertragsfeld `locked`, Wertzugriff über Punktpfade.
 */
import { describe, expect, it } from 'vitest';
import { defaultSettings } from '../../core/defaults';
import { translator } from '../../i18n/catalog';
import { settingsView } from '../../test/views';
import { logoCm } from './controls';
import { ROWS, SECTIONS, fold, search, visibleRows } from './rows';
import { lockedReason, valueAt } from './state';

const desktop = visibleRows({ mobile: false, profile: 'SIGNATURBLOCK_SMALL' });
const ids = (rows: { id: string }[]) => rows.map((r) => r.id);

describe('Bereiche und Zeilen', () => {
  it('„Dokument“ direkt nach „Signaturbild“; PDF/A und Platzhalter nicht unter „Erweitert“', () => {
    const order = SECTIONS.map((s) => s.id);
    expect(order).toEqual(['signature', 'document', 'signers', 'save', 'appearance', 'accessibility', 'privacy', 'advanced', 'about']);
    expect(ROWS.filter((r) => r.section === 'document').map((r) => r.id)).toEqual(['document.pdfa', 'document.placeholder', 'document.auto_position']);
    expect(ROWS.filter((r) => r.section === 'advanced').map((r) => r.id)).not.toContain('document.placeholder');
  });

  it('jede Zeile hat eine Beschriftung mit Synonymen in DE und EN', () => {
    for (const lang of ['de', 'en'] as const) {
      const t = translator(lang);
      for (const r of ROWS) {
        expect(t.hasOwn(r.label), `${lang} ${r.label}`).toBe(true);
        expect(t.attr(r.label, 'keywords')?.length ?? 0, `${lang} ${r.label}.keywords`).toBeGreaterThan(2);
      }
    }
  });

  it('Mobile ohne Proxy, Protokolle, Chipkarte, Keystore, Standardwahl und Bildmarke; Profilzeilen je Profil', () => {
    const mobile = ids(visibleRows({ mobile: true, profile: 'SIGNATURBLOCK_SMALL' }));
    const desktopOnly = ['advanced.proxy', 'advanced.migration', 'signers.card', 'signers.keystore', 'signers.default', 'signature.emblem'];
    for (const id of desktopOnly) expect(mobile).not.toContain(id);
    for (const id of desktopOnly) expect(ids(desktop)).toContain(id);
    // Mobile nur ID Austria (ADR 0004 Nachtrag 11): Konto und FIDO2-Hinweis bleiben
    expect(mobile.filter((id) => id.startsWith('signers.'))).toEqual(['signers.id_austria', 'signers.fido2']);
    expect(ids(desktop)).toContain('signature.note');
    expect(ids(desktop)).not.toContain('signature.logo_size');
    const logo = ids(visibleRows({ mobile: false, profile: 'BASE_LOGO' }));
    expect(logo).toContain('signature.logo_size');
    expect(logo).not.toContain('signature.note');
    expect(logo).not.toContain('signature.language');
    const invisible = ids(visibleRows({ mobile: false, profile: 'INVISIBLE' }));
    expect(invisible).not.toContain('signature.emblem');
    expect(invisible).toContain('signature.preview');
  });
});

describe('Suche', () => {
  const de = translator('de');
  const en = translator('en');
  const hit = (t: typeof de, q: string) => ids(search(t, desktop, q));

  it('findet „Logo“, „Handy“, „Proxy“ (Abnahme E3 Punkt 3)', () => {
    expect(hit(de, 'Logo')).toEqual(['signature.emblem']);
    expect(hit(de, 'Handy')).toEqual(['signers.id_austria']);
    expect(hit(de, 'PDF-Over')).toEqual(['advanced.migration']);
    expect(hit(en, 'import')).toEqual(['advanced.migration']);
    expect(hit(de, 'Proxy')).toEqual(['advanced.proxy']);
  });

  it('„Keystore“ findet nur Keystore und Standard-Signiermittel, „Aktualisierung“ die Updates (keine doppelten `.keywords`)', () => {
    expect(hit(de, 'keystore')).toEqual(['signers.default', 'signers.keystore']);
    expect(hit(en, 'keystore')).toEqual(['signers.default', 'signers.keystore']);
    expect(hit(de, 'aktualisierung')).toEqual(['advanced.updates']);
    expect(hit(en, 'updates')).toEqual(['advanced.updates']);
  });

  it('„Über PIDA“ ist ein Block (Nachtrag 21): Logs, Versionshinweise, Quellcode und Urheberrecht finden ihn', () => {
    expect(ROWS.filter((r) => r.section === 'about').map((r) => r.id)).toEqual(['about.info']);
    for (const q of ['protokolle', 'logs', 'versionshinweise', 'quellcode', 'urheberrecht', 'drittlizenzen']) expect(hit(de, q), q).toEqual(['about.info']);
    for (const q of ['logs', 'release notes', 'source code', 'copyright', 'licenses']) expect(hit(en, q), q).toEqual(['about.info']);
  });

  it('Dokument: „pdf/a“, „platzhalter“, „qr“, „signaturfeld“; englisch „signature field“', () => {
    expect(hit(de, 'pdf/a')).toEqual(['document.pdfa']);
    expect(hit(de, 'PDF/A')).toEqual(['document.pdfa']);
    expect(hit(de, 'platzhalter')).toEqual(['document.placeholder']);
    expect(hit(de, 'qr')).toEqual(['document.placeholder']);
    expect(hit(de, 'signaturfeld')).toEqual(['document.placeholder']);
    expect(hit(en, 'signature field')).toEqual(['document.placeholder']);
    expect(hit(en, 'pdf/a')).toEqual(['document.pdfa']);
  });

  it('alle Wörter, ohne Groß-/Kleinschreibung und Akzente; Bereichsname zählt; leer = nichts', () => {
    expect(fold('Größe ÄÖÜ')).toBe('große aou');
    expect(hit(de, 'bürgerkarte')).toEqual(expect.arrayContaining(['signers.card']));
    expect(hit(de, 'BURGERKARTE')).toEqual(expect.arrayContaining(['signers.card']));
    expect(hit(de, 'datenschutz')).toEqual(['privacy.info', 'privacy.certcache', 'privacy.recent']);
    expect(hit(de, 'zuletzt signiert')).toEqual(['privacy.recent']);
    expect(hit(de, 'xyz')).toEqual([]);
    expect(hit(de, '   ')).toEqual([]);
  });
});

describe('Hilfen', () => {
  it('Logo-Größe: mm → cm mit Grenzen 1–20 cm', () => {
    expect(logoCm(23)).toBe(2.3);
    expect(logoCm(200)).toBe(20);
    expect(logoCm(500)).toBe(20);
    expect(logoCm(3)).toBe(1);
    expect(logoCm(Number.NaN)).toBe(2.3);
  });

  it('Richtlinien-Sperren aus `locked` (Pfad oder ganzer Abschnitt), ohne Feld frei', () => {
    const view = settingsView().settings!;
    expect(lockedReason(view, 'general.recent_documents')).toBeNull();
    const locked = { ...view, locked: { 'general.recent_documents': 'ng-recent-policy', proxy: 'ng-ui-set-policy' } };
    expect(lockedReason(locked, 'general.recent_documents')).toBe('ng-recent-policy');
    expect(lockedReason(locked, 'proxy.port')).toBe('ng-ui-set-policy');
    expect(lockedReason(locked, 'general.update_check')).toBeNull();
  });

  it('Punktpfade', () => {
    const s = defaultSettings();
    expect(valueAt(s, 'signature.profile')).toBe('SIGNATURBLOCK_SMALL');
    expect(valueAt(s, 'placeholder.kind')).toBe('marker');
    expect(valueAt(s, 'nichts.da')).toBeUndefined();
  });
});

describe('Einstellungen im Web', () => {
  it('nur Signaturbild, Dokument, Barrierefreiheit, Datenschutz, Über PIDA; Keystore nur im Testbuild', () => {
    const sections = (keystore: boolean) => [...new Set(visibleRows({ mobile: false, profile: 'SIGNATURBLOCK_SMALL', web: { keystore } }).map((r) => r.section))];
    expect(sections(false)).toEqual(['signature', 'document', 'accessibility', 'privacy', 'about']);
    const ids = (keystore: boolean) => visibleRows({ mobile: false, profile: 'SIGNATURBLOCK_SMALL', web: { keystore } }).map((r) => r.id);
    expect(ids(false)).not.toContain('signers.keystore');
    expect(ids(true).filter((id) => id.startsWith('signers.'))).toEqual(['signers.keystore']);
    expect(ids(true)).not.toContain('privacy.recent');
    // App unverändert
    expect(visibleRows({ mobile: false, profile: 'SIGNATURBLOCK_SMALL' }).map((r) => r.id)).toContain('advanced.reset');
  });
});
