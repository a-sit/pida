/**
 * Mock-Kern auf Ebene der Projektion (ADR 0004 §4.8, Abnahme E0 „je neuem Typ ein Beispiel“, Invariante A1):
 * Szenarien wie im Prototyp, PDF-AS-Regel der automatischen Platzierung, Raster/Prüfung je Revision,
 * Verschieben in 15/1/50 pt, keine Signiersitzung ohne ausdrückliche Aktion, Bereiche über Signale.
 */
import { afterEach, describe, expect, it } from 'vitest';
import { defaultSettings } from '../defaults';
import type { Settings, UiEvent, ViewState } from '../types';
import { MOCK_SCENARIOS, MockCore, type MockOptions } from './index';
import { fieldFit } from './workspace';

let cores: MockCore[] = [];
afterEach(() => {
  for (const c of cores) c.dispose();
  cores = [];
});

function idAustria(): Settings {
  const s = defaultSettings();
  return { ...s, backend: { default: 'id_austria' }, id_austria: { ...s.id_austria, username: '+43 660 1234123', remember: true }, output: { ...s.output, folder: '.' } };
}

function core(options: MockOptions = {}): MockCore {
  const c = new MockCore({ delayMs: 0, autoCompleteApp: false, ...options });
  cores.push(c);
  return c;
}

/** wartet, bis die Projektion `pred` erfüllt (simulierte Latenz 0, aber über Zeitgeber). */
async function until(c: MockCore, pred: (v: ViewState) => boolean): Promise<ViewState> {
  for (let i = 0; i < 100; i++) {
    const v = await c.view();
    if (pred(v)) return v;
    await new Promise((r) => setTimeout(r, 2));
  }
  throw new Error(`Zustand nicht erreicht: ${JSON.stringify((await c.view()).step).slice(0, 300)}`);
}

const hasDoc = (v: ViewState) => v.document !== null && v.document.phase === 'editing';

describe('Szenarien', () => {
  it('jedes Szenario liefert einen Ausgangszustand', async () => {
    for (const scenario of MOCK_SCENARIOS) {
      const c = core({ scenario });
      const v = await until(c, (x) => x.actions.open_files || x.step.step === 'error' || x.document !== null);
      switch (scenario) {
        case 'start':
          expect(v.document).toBeNull();
          expect(v.actions.open_files).toBe(true);
          expect(v.actions.sign).toBe(false);
          break;
        case 'batch': {
          // Stapelübersicht (ADR 0004 §3.8): kein Dokument im Arbeitsbereich, Voranalyse im Hintergrund
          const b = await until(c, (x) => x.step.step === 'batch' && (x.batch?.items.every((i) => i.status.state !== 'analyzing') ?? false));
          expect(b.document).toBeNull();
          expect(b.batch?.items).toHaveLength(4);
          break;
        }
        case 'error-password': {
          const e = await until(c, (x) => x.step.step === 'error');
          expect(e.step.step === 'error' && e.step.code).toBe('error-PDFPwdProtected');
          break;
        }
        default: {
          const d = await until(c, hasDoc);
          expect(d.document?.doc.page_count).toBeGreaterThan(0);
          expect(d.signers?.options.length).toBeGreaterThan(0);
        }
      }
    }
  });

  it('Vertrag: PDF-AS-Regel (waagrecht mittig, unter dem Inhalt der letzten Seite), Raster und Prüfung je Revision', async () => {
    const v = await until(core({ scenario: 'contract' }), hasDoc);
    const d = v.document!;
    const last = d.doc.page_count - 1;
    const [pw] = d.doc.page_sizes[last] as [number, number];
    expect(d.origin).toBe('auto');
    expect(d.target).toEqual({ kind: 'free' });
    expect(d.placement.page_index).toBe(last);
    expect(Math.abs(d.placement.x + d.placement.width / 2 - pw / 2)).toBeLessThan(0.5);
    const layout = d.layout.find((l) => l.page === last);
    expect(layout?.content).not.toBeNull();
    expect(d.placement.y).toBeLessThanOrEqual((layout?.content?.bottom ?? 0) - 20 + 0.5);
    expect(layout?.free.length).toBeGreaterThan(0);
    expect(layout?.guides.some((g) => g.axis === 'x')).toBe(true);
    expect(d.check.for_revision).toBe(d.revision);
    expect(d.check.overlaps_content).toBe(false);
    expect(d.sticker).toEqual({ left: d.placement.x, top: d.placement.y, right: d.placement.x + d.placement.width, bottom: d.placement.y - d.placement.height });
  });

  it('Signaturfeld: Felder als Ziele, Übernahme setzt Herkunft `field`', async () => {
    const c = core({ scenario: 'field' });
    const v = await until(c, hasDoc);
    const field = v.document!.fields.find((f) => f.selectable);
    expect(field).toBeDefined();
    await c.dispatch({ type: 'use_target', target: { kind: 'field', index: field!.index } });
    const after = await until(c, (x) => x.document?.origin === 'field');
    expect(after.document?.target).toEqual({ kind: 'field', index: field!.index });
    expect(after.document?.check.for_revision).toBe(after.document?.revision);
  });

  it('Formular wie `manySignFields.pdf` (Nachtrag 16): nur plausible Felder angeboten; im seitengroßen Feld behält der Block seine Größe', async () => {
    const c = core();
    await until(c, (x) => x.step.step === 'open');
    await c.dispatch({ type: 'open_files', paths: ['/Users/demo/Dokumente/Signaturfelder.pdf'] });
    const v = await until(c, hasDoc);
    const d = v.document!;
    expect(d.fields.filter((f) => f.suitable).map((f) => f.name)).toEqual(['Signature_0', 'Signature_5', 'Signature_6']);
    expect(d.new_page_size).toEqual([612, 792]);
    // frei auf Seite 2: das seitengroße leere Feld ist kein Hindernis (keine Überdeckungswarnung)
    expect(d.placement.page_index).toBe(1);
    expect(d.check.overlaps_widget).toBe(false);
    const page = d.fields.find((f) => f.name === 'Signature_7')!;
    await c.dispatch({ type: 'use_target', target: { kind: 'field', index: page.index } });
    const after = await until(c, (x) => x.document?.target.kind === 'field');
    const s = after.document!.sticker!;
    expect(s).toEqual(page.block);
    expect(s.right - s.left).toBeCloseTo(d.block.width, 6);
    expect(s.top - s.bottom).toBeCloseTo(d.block.height, 6);
    expect(after.document!.placement).toMatchObject({ page_index: 1, x: s.left, y: s.top, width: d.block.width });
  });

  it('Eignung eines Felds wie im Kern (`pida_placement::field_suitable`): übliche Felder für jedes Profil, absurde für keines', () => {
    const r = (w: number, h: number) => ({ left: 100, bottom: 100, right: 100 + w, top: 100 + h });
    const A4: [number, number] = [595.28, 841.89];
    const small = { width: 230, height: 82 };
    const amts = { width: 495, height: 120 };
    const logo = { width: 40.51, height: 42.52 };
    for (const block of [small, amts, logo]) {
      for (const [w, h] of [[150, 50], [230, 50], [300, 100], [400, 150]] as const) expect(fieldFit(r(w, h), block, A4).suitable, `${block.width} in ${w}×${h}`).toBe(true);
      for (const [w, h] of [[3.2, 224], [13.5, 245.6], [50, 800], [595.28, 841.89]] as const) expect(fieldFit(r(w, h), block, A4).suitable, `${block.width} in ${w}×${h}`).toBe(false);
    }
    // Platzhalterfeld der PDF-AS-Testdaten (63,7 × 61,1): Signaturblock ja (Maßstab 0,28), Amtssignatur nein (0,13)
    expect(fieldFit(r(63.7, 61.1), small, A4).suitable).toBe(true);
    expect(fieldFit(r(63.7, 61.1), amts, A4).suitable).toBe(false);
    // Grenzen: Maßstab ⅕, Form ⅛, halbe Seite
    expect(fieldFit(r(46, 16.4), small, A4).suitable).toBe(true);
    expect(fieldFit(r(230 * 0.19, 82), small, A4).suitable).toBe(false);
    expect(fieldFit(r(920, 41), small, [5000, 5000]).suitable).toBe(true);
    expect(fieldFit(r(943, 41), small, [5000, 5000]).suitable).toBe(false);
    expect(fieldFit(r(595.28, 420.9), small, A4).suitable).toBe(true);
    expect(fieldFit(r(595.28, 430), small, A4).suitable).toBe(false);
  });

  it('bereits signiert: vorhandene Signaturen mit Status, keine neue Seite', async () => {
    const v = await until(core({ scenario: 'signed' }), hasDoc);
    expect(v.document?.existing.length).toBeGreaterThan(0);
    expect(v.document?.existing.every((e) => ['valid', 'invalid', 'not_verifiable'].includes(e.status))).toBe(true);
    expect(v.document?.constraints.new_page_allowed).toBe(false);
  });

  it('Stapel ohne Stapelübersicht (`batchOverview: false`): das erste von vier Dokumenten im Arbeitsbereich', async () => {
    const v = await until(core({ scenario: 'batch', batchOverview: false }), hasDoc);
    expect(v.document?.doc.name).toBe('Angebot_2026-118.pdf');
    expect(v.batch).toBeNull();
    expect(v.actions.show_batch).toBe(false);
  });

  it('Beispiele der späteren Etappen (`examples`): zuletzt signiert, Ersteinrichtung (Stapel: echter Ablauf)', async () => {
    const v = await until(core({ examples: true, firstRun: true }), (x) => x.step.step === 'open');
    expect(v.recent?.length).toBeGreaterThan(0);
    // kein Prüfmodus mehr (ADR 0004 Nachtrag 15): weder Szenario noch Ansicht noch Freigabe
    expect(MOCK_SCENARIOS as readonly string[]).not.toContain('verify');
    expect('verify' in v || 'verify' in v.actions).toBe(false);
    expect(v.first_run).not.toBeNull();
    // der Stapel kommt seit A2 aus dem Ablauf (Szenario `batch`), nicht als festes Beispiel
    expect(v.batch).toBeNull();
    expect(v.actions.show_batch).toBe(false);
    const plain = await until(core(), (x) => x.step.step === 'open');
    expect(plain.recent).toBeNull();
    expect(plain.first_run).toBeNull();
    expect(plain.batch).toBeNull();
  });
});

describe('Platzierung', () => {
  it('Verschieben in 15 pt, fein 1 pt, grob 50 pt; Herkunft `user`', async () => {
    const c = core({ scenario: 'blank' });
    const v = await until(c, hasDoc);
    const x0 = v.document!.placement.x;
    await c.dispatch({ type: 'nudge_block', dx: -1, dy: 0, fine: false, coarse: false });
    let d = (await until(c, (x) => x.document!.placement.x !== x0)).document!;
    expect(d.placement.x).toBeCloseTo(x0 - 15);
    expect(d.origin).toBe('user');
    await c.dispatch({ type: 'nudge_block', dx: -1, dy: 0, fine: true, coarse: false });
    d = (await c.view()).document!;
    expect(d.placement.x).toBeCloseTo(x0 - 16);
    await c.dispatch({ type: 'nudge_block', dx: -1, dy: 0, fine: false, coarse: true });
    d = (await c.view()).document!;
    expect(d.placement.x).toBeCloseTo(x0 - 66);
    expect(d.check.for_revision).toBe(d.revision);
  });

  it('Verschieben bleibt auf der Seite (`clamped`)', async () => {
    const c = core({ scenario: 'blank' });
    const v = await until(c, hasDoc);
    await c.dispatch({ type: 'move_block', page: v.document!.placement.page_index, x: -500, y: 99999 });
    const d = (await c.view()).document!;
    expect(d.placement.x).toBe(0);
    expect(d.check.clamped).toBe(true);
  });

  it('unsichtbar und neue Seite folgen den Einschränkungen', async () => {
    const c = core({ scenario: 'blank' });
    await until(c, hasDoc);
    await c.dispatch({ type: 'set_invisible', invisible: true });
    expect((await c.view()).document?.invisible).toBe(true);
    await c.dispatch({ type: 'set_invisible', invisible: false });
    await c.dispatch({ type: 'toggle_new_page' });
    const d = (await c.view()).document!;
    expect(d.new_page).toBe(true);
    expect(d.placement.page_index).toBe(d.doc.page_count);
  });
});

describe('Invariante A1: Sitzung nur nach ausdrücklicher Aktion', () => {
  it('Öffnen, Platzieren und Signer wählen starten keine Sitzung; „Signieren“ (`sign_with`) schon', async () => {
    const c = core({ scenario: 'contract', settings: idAustria() });
    const seen: string[] = [];
    c.onState((v) => seen.push(v.step.step));
    const v = await until(c, hasDoc);
    const events: UiEvent[] = [
      { type: 'select_signer', backend: 'keystore', make_default: false },
      { type: 'select_signer', backend: 'atrust_native', make_default: false },
      { type: 'nudge_block', dx: 1, dy: 0, fine: false, coarse: false },
      { type: 'inspect_pages', pages: [0, 1] },
      { type: 'auto_place' },
    ];
    for (const e of events) await c.dispatch(e);
    await new Promise((r) => setTimeout(r, 10));
    expect(seen).not.toContain('signing');
    expect(v.signers?.selected).toBe('atrust_native');
    await c.dispatch({ type: 'sign_with', backend: 'atrust_native' });
    const s = await until(c, (x) => x.step.step === 'signing');
    expect(s.step.step === 'signing' && s.step.progress.backend).toBe('atrust_native');
    expect(s.step.step === 'signing' && s.step.document_leaves_device).toBe(true);
    // Abbrechen: zurück in den Arbeitsbereich, Platzierung unverändert, Hinweis
    const before = s.document?.placement;
    await c.dispatch({ type: 'cancel' });
    const back = await until(c, (x) => x.step.step === 'position');
    expect(back.document?.placement).toEqual(before);
    expect(back.notices.some((n) => n.code === 'ng-signing-cancelled')).toBe(true);
  });

  it('nicht verfügbarer Signer → Hinweis, keine Sitzung', async () => {
    const c = core({ scenario: 'contract', localBkuAvailable: false });
    await until(c, hasDoc);
    await c.dispatch({ type: 'sign_with', backend: 'local_bku' });
    const v = await c.view();
    expect(v.step.step).toBe('position');
    expect(v.notices.some((n) => n.code === 'ng-signer-unavailable')).toBe(true);
  });
});

describe('Bereiche über Signale', () => {
  it('Einstellungen speichern → neue Blockvorschau (Revision) im Arbeitsbereich', async () => {
    const c = core({ scenario: 'contract' });
    const v = await until(c, hasDoc);
    const rev = v.document!.block.preview_revision;
    await c.dispatch({ type: 'open_settings' });
    expect((await c.view()).actions.settings).toBe(false);
    const s = (await c.view()).settings!.settings;
    await c.dispatch({ type: 'save_settings', settings: { ...s, signature: { ...s.signature, profile: 'AMTSSIGNATURBLOCK' } } });
    const after = await until(c, (x) => x.settings === null);
    expect(after.document!.block.preview_revision).toBeGreaterThan(rev);
    expect(after.document!.block.width).toBeGreaterThan(v.document!.block.width);
  });
});

describe('Stapel (ADR 0004 §3.8)', () => {
  /** ID Austria, Platzhaltersuche an (QR-Platzhalter → positioniert). */
  function batchSettings(extra: Partial<Settings> = {}): Settings {
    return { ...idAustria(), placeholder: { search: true, kind: 'marker' }, ...extra };
  }
  const ready = (v: ViewState) => v.step.step === 'batch' && !!v.batch && v.batch.items.every((i) => i.status.state !== 'analyzing');
  const DIR = '/Users/demo/Dokumente';

  /** Bestätigungen in der App bis zum Ende des Laufs; zählt Anmeldemasken und Vergleichswerte. */
  async function runAll(c: MockCore, password = 'geheim123') {
    const logins: number[] = [];
    const refs: string[] = [];
    for (let i = 0; i < 40; i++) {
      const v = await c.view();
      if (v.step.step !== 'signing') return { logins, refs, view: v };
      const it = v.step.interaction;
      if (it.kind === 'form' && 'Login' in it.form) {
        logins.push(it.request);
        await c.dispatch({ type: 'submit_form', request: it.request, answer: { kind: 'login', username: '+43 660 1234123', password, remember: false } });
      } else if (it.kind === 'form' && 'WaitForApp' in it.form) {
        refs.push(it.form.WaitForApp.reference_value ?? '');
        c.completeConfirmation();
      }
      await new Promise((r) => setTimeout(r, 3));
    }
    throw new Error('Lauf endet nicht');
  }

  it('Voranalyse: Seiten, Größe, Miniatur, Status; Schätzung für das gewählte Signiermittel; keine Sitzung (A1)', async () => {
    const c = core({ scenario: 'batch', settings: batchSettings() });
    const seen: string[] = [];
    c.onState((v) => seen.push(v.step.step));
    const v = await until(c, ready);
    expect(v.document).toBeNull();
    expect(v.batch?.items.map((i) => [i.name, i.status.state, i.position.kind])).toEqual([
      ['Angebot_2026-118.pdf', 'ready', 'auto'],
      ['Auftragsbestaetigung_4711.pdf', 'positioned', 'qr'],
      ['Lieferschein_Scan.pdf', 'rejected', 'auto'],
      ['Vertrag.pdf', 'ready', 'auto'],
    ]);
    expect(v.batch?.items[2]).toMatchObject({ page_count: 1, thumbnail: false, target_name: null, status: { state: 'rejected', code: 'error-PDFPwdProtected' } });
    expect(v.batch?.items[0]).toMatchObject({ page_count: 2, size_bytes: 96 * 1024, thumbnail: true, target_name: 'Angebot_2026-118_signed.pdf', target_exists: false });
    expect(v.batch?.estimate).toEqual({ backend: 'atrust_native', documents: 3, logins: 1, confirmations: 3, certificate_confirmation_possible: true, large: false });
    expect(v.batch?.output).toEqual({ mode: 'next_to_source', folder_display: null, needs_folder: false });
    expect(v.actions).toMatchObject({ start_batch: true, sign: false, open_files: true });
    expect(c.thumbUrl(1)).toMatch(/^data:image\/svg\+xml/);
    expect(seen).not.toContain('signing');
  });

  it('eine Sitzung: eine Anmeldung, n Bestätigungen mit je eigenem Vergleichswert; danach Zusammenfassung und Hinweis', async () => {
    const c = core({ scenario: 'batch', settings: batchSettings() });
    await until(c, ready);
    await c.dispatch({ type: 'start_batch', backend: 'atrust_native', docs: [1, 2, 3, 4], output_dir: null, on_existing: 'keep_both' });
    const running = await c.view();
    expect(running.batch?.running).toBe(true);
    expect(running.step.step === 'signing' && running.step.progress).toMatchObject({ batch: true, index: 1, total: 3, confirmations_total: 3 });
    expect(running.actions).toMatchObject({ open_files: false, start_batch: false, settings: false, home: false });
    const { logins, refs, view } = await runAll(c);
    expect(logins).toHaveLength(1);
    expect(refs).toHaveLength(3);
    expect(new Set(refs).size).toBe(3);
    expect(view.batch?.summary).toEqual({ total: 4, signed: 3, rejected: 1, failed: 0, skipped: 0, cancelled: 0, not_started: 0, folder_display: null, aborted: false });
    expect(view.batch?.items.map((i) => i.status.state)).toEqual(['signed', 'signed', 'rejected', 'signed']);
    expect(view.notices.some((n) => n.code === 'ng-batch-done' && n.params.signed === '3' && n.params.total === '4')).toBe(true);
  });

  it('Zertifikat höchstens einmal je Sitzung (ab dem zweiten Dokument `session`)', async () => {
    const c = core({ scenario: 'batch', settings: batchSettings() });
    await until(c, ready);
    const certs: string[] = [];
    c.onState((v) => {
      if (v.step.step === 'signing' && v.step.interaction.kind === 'waiting' && v.step.interaction.progress === 'ReadingCertificate') certs.push(String(v.step.progress.index));
    });
    await c.dispatch({ type: 'start_batch', backend: 'atrust_native', docs: [1, 2, 4], output_dir: null, on_existing: 'keep_both' });
    await runAll(c);
    expect([...new Set(certs)]).toEqual(['1']);
  });

  it('vorhandene Dateien: Beide behalten → erste freie Nummer, Überspringen ohne Bestätigung, Überschreiben', async () => {
    const existing = [`${DIR}/Verträge/Vertrag_signed.pdf`, `${DIR}/Verträge/Vertrag_signed (2).pdf`];
    for (const [policy, expected, confirmations] of [
      ['keep_both', { state: 'signed', path: `${DIR}/Verträge/Vertrag_signed (3).pdf` }, 3],
      ['skip', { state: 'skipped', existing: true }, 2],
      ['overwrite', { state: 'signed', path: `${DIR}/Verträge/Vertrag_signed.pdf` }, 3],
    ] as const) {
      const c = core({ scenario: 'batch', settings: batchSettings(), existingFiles: [...existing] });
      const v = await until(c, ready);
      expect(v.batch?.items[3]?.target_exists).toBe(true);
      await c.dispatch({ type: 'start_batch', backend: 'atrust_native', docs: [1, 2, 4], output_dir: null, on_existing: policy });
      const { refs, view } = await runAll(c);
      expect(refs).toHaveLength(confirmations);
      expect(view.batch?.items[3]?.status).toEqual(expected);
    }
  });

  it('„jedes Mal fragen“ bzw. Dateipfad als Ordner: ohne `output_dir` kein Lauf (Hinweis); mit Ordner dorthin', async () => {
    for (const folder of [null, '/Users/demo/Signiert/alle.pdf']) {
      const c = core({ scenario: 'batch', settings: batchSettings({ output: { folder, postfix: '_signed' } }) });
      const v = await until(c, ready);
      expect(v.batch?.output.needs_folder).toBe(true);
      await c.dispatch({ type: 'start_batch', backend: 'atrust_native', docs: [1, 2, 4], output_dir: null, on_existing: 'keep_both' });
      const refused = await c.view();
      expect(refused.batch?.running).toBe(false);
      expect(refused.notices.some((n) => n.code === 'ng-batch-output-folder-required')).toBe(true);
      await c.dispatch({ type: 'start_batch', backend: 'atrust_native', docs: [1, 2, 4], output_dir: '/Users/demo/Signiert', on_existing: 'keep_both' });
      const { view } = await runAll(c);
      expect(view.batch?.items[0]?.status).toEqual({ state: 'signed', path: '/Users/demo/Signiert/Angebot_2026-118_signed.pdf' });
      expect(view.batch?.summary?.folder_display).toBe('/Users/demo/Signiert');
    }
  });

  it('nichts signierbar bzw. Signiermittel nicht verfügbar: Hinweis, keine Sitzung', async () => {
    const c = core({ scenario: 'batch', settings: batchSettings(), localBkuAvailable: false });
    await until(c, ready);
    await c.dispatch({ type: 'start_batch', backend: 'atrust_native', docs: [3], output_dir: null, on_existing: 'keep_both' });
    await c.dispatch({ type: 'start_batch', backend: 'local_bku', docs: [1], output_dir: null, on_existing: 'keep_both' });
    const v = await c.view();
    expect(v.step.step).toBe('batch');
    expect(v.notices.map((n) => n.code)).toEqual(expect.arrayContaining(['ng-batch-nothing-to-sign', 'ng-signer-unavailable']));
  });

  it('Überspringen behält die Anmeldung; Abbruch vergisst das Passwort (F-72), wartende bleiben bereit', async () => {
    const c = core({ scenario: 'batch', settings: batchSettings() });
    await until(c, ready);
    await c.dispatch({ type: 'start_batch', backend: 'atrust_native', docs: [1, 2, 4], output_dir: null, on_existing: 'keep_both' });
    let v = await until(c, (x) => x.step.step === 'signing' && x.step.interaction.kind === 'form' && 'Login' in x.step.interaction.form);
    if (v.step.step === 'signing' && v.step.interaction.kind === 'form') {
      await c.dispatch({ type: 'submit_form', request: v.step.interaction.request, answer: { kind: 'login', username: '+43 660 1234123', password: 'geheim123', remember: true } });
    }
    await until(c, (x) => x.step.step === 'signing' && x.step.interaction.kind === 'form' && 'WaitForApp' in x.step.interaction.form);
    await c.dispatch({ type: 'skip_batch_document' });
    v = await until(c, (x) => x.step.step === 'signing' && x.step.progress.index === 2 && x.step.interaction.kind === 'form' && 'WaitForApp' in x.step.interaction.form);
    expect(v.step.step === 'signing' && v.step.progress.login_done).toBe(true);
    await c.dispatch({ type: 'abort_batch' });
    v = await c.view();
    expect(v.batch?.items.map((i) => i.status.state)).toEqual(['skipped', 'cancelled', 'rejected', 'ready']);
    expect(v.notices.some((n) => n.code === 'ng-batch-aborted' && n.params.signed === '0')).toBe(true);
    // „Daten merken“ gilt nach dem Abbruch nicht mehr: wieder eine Anmeldung
    expect(v.batch?.estimate?.logins).toBe(1);
  });

  it('mit gemerktem Passwort: Schätzung 0 Anmeldungen und kein Login im nächsten Lauf', async () => {
    const c = core({ scenario: 'batch', settings: batchSettings() });
    await until(c, ready);
    await c.dispatch({ type: 'start_batch', backend: 'atrust_native', docs: [1], output_dir: null, on_existing: 'keep_both' });
    const v = await until(c, (x) => x.step.step === 'signing' && x.step.interaction.kind === 'form' && 'Login' in x.step.interaction.form);
    if (v.step.step === 'signing' && v.step.interaction.kind === 'form') {
      await c.dispatch({ type: 'submit_form', request: v.step.interaction.request, answer: { kind: 'login', username: '+43 660 1234123', password: 'geheim123', remember: true } });
    }
    await runAll(c);
    expect((await c.view()).batch?.estimate?.logins).toBe(0);
    await c.dispatch({ type: 'start_batch', backend: 'atrust_native', docs: [2, 4], output_dir: null, on_existing: 'keep_both' });
    const { logins, refs } = await runAll(c);
    expect(logins).toEqual([]);
    expect(refs).toHaveLength(2);
  });

  it('Pausen: Verbindung (gleiche Sitzung), Sitzung (neue Sitzung), Dokument (Ablehnung); nicht wiederholbar → Fehler', async () => {
    const c = core({
      scenario: 'batch',
      settings: batchSettings(),
      batchFaults: { 'Angebot_2026-118.pdf': 'decline', 'Auftragsbestaetigung_4711.pdf': 'network', 'Vertrag.pdf': 'session' },
    });
    await until(c, ready);
    await c.dispatch({ type: 'start_batch', backend: 'atrust_native', docs: [1, 2, 4], output_dir: null, on_existing: 'keep_both' });
    const pauses: unknown[] = [];
    let logins = 0;
    for (let i = 0; i < 80; i++) {
      const v = await c.view();
      if (v.step.step !== 'signing') break;
      const p = v.step.progress;
      if (p.pause) {
        expect(p.stage).toBe('paused');
        pauses.push({ doc: p.pause.doc?.name, scope: p.pause.scope, new_session: p.pause.new_session, code: p.pause.code });
        await c.dispatch({ type: 'retry' });
      } else if (v.step.interaction.kind === 'form' && 'Login' in v.step.interaction.form) {
        logins += 1;
        await c.dispatch({ type: 'submit_form', request: v.step.interaction.request, answer: { kind: 'login', username: '+43 660 1234123', password: 'geheim123', remember: false } });
      } else if (v.step.interaction.kind === 'form' && 'WaitForApp' in v.step.interaction.form) {
        c.completeConfirmation();
      }
      await new Promise((r) => setTimeout(r, 15));
    }
    expect(pauses).toEqual([
      { doc: 'Angebot_2026-118.pdf', scope: 'document', new_session: false, code: 'ng-signing-declined' },
      { doc: 'Auftragsbestaetigung_4711.pdf', scope: 'session', new_session: false, code: 'backend-network' },
      { doc: 'Vertrag.pdf', scope: 'session', new_session: true, code: 'backend-unknown-page' },
    ]);
    // erste Anmeldung und nach dem Sitzungsfehler (ohne „Daten merken“) eine zweite
    expect(logins).toBe(2);
    expect((await c.view()).batch?.summary?.signed).toBe(3);
  }, 10000);

  it('Einzeldokument: öffnen, zurück mit Plan, Plan wird beim erneuten Öffnen angewendet; einzeln signiert → Übersicht + Hinweis', async () => {
    const c = core({ scenario: 'batch', settings: batchSettings() });
    await until(c, ready);
    await c.dispatch({ type: 'open_batch_item', doc: 1 });
    let v = await until(c, hasDoc);
    expect(v.batch?.open_item).toBe(1);
    expect(v.actions.show_batch).toBe(false);
    await c.dispatch({ type: 'set_invisible', invisible: true });
    await c.dispatch({ type: 'back_to_batch' });
    v = await c.view();
    expect(v.step.step).toBe('batch');
    expect(v.batch?.items[0]).toMatchObject({ status: { state: 'positioned' }, position: { kind: 'invisible' } });
    await c.dispatch({ type: 'open_batch_item', doc: 1 });
    v = await until(c, hasDoc);
    expect(v.document?.invisible).toBe(true);
    await c.dispatch({ type: 'sign_with', backend: 'atrust_native' });
    v = await until(c, (x) => x.step.step === 'signing' && x.step.interaction.kind === 'form');
    if (v.step.step === 'signing' && v.step.interaction.kind === 'form') {
      await c.dispatch({ type: 'submit_form', request: v.step.interaction.request, answer: { kind: 'login', username: '+43 660 1234123', password: 'geheim123', remember: false } });
    }
    await until(c, (x) => x.step.step === 'signing' && x.step.interaction.kind === 'form' && 'WaitForApp' in x.step.interaction.form);
    c.completeConfirmation();
    v = await until(c, (x) => x.step.step === 'batch');
    expect(v.document).toBeNull();
    expect(v.batch?.open_item).toBeNull();
    expect(v.batch?.items[0]?.status.state).toBe('signed');
    expect(v.notices.some((n) => n.code === 'ng-batch-item-signed' && n.params.file === 'Angebot_2026-118.pdf')).toBe(true);
  });

  it('Einstellungen wirken sofort: QR-Plan der Voranalyse folgt der Platzhaltersuche, gemerkte Pläne bleiben', async () => {
    const c = core({ scenario: 'batch', settings: batchSettings() });
    await until(c, ready);
    await c.dispatch({ type: 'open_batch_item', doc: 4 });
    await until(c, hasDoc);
    await c.dispatch({ type: 'nudge_block', dx: 0, dy: 1, fine: false, coarse: false });
    await c.dispatch({ type: 'back_to_batch' });
    await c.dispatch({ type: 'open_settings' });
    const s = (await c.view()).settings!.settings;
    await c.dispatch({ type: 'save_settings', settings: { ...s, placeholder: { search: false, kind: 'marker' } } });
    const v = await until(c, (x) => x.settings === null);
    expect(v.batch?.items[1]).toMatchObject({ status: { state: 'ready' }, position: { kind: 'auto' } });
    expect(v.batch?.items[3]).toMatchObject({ status: { state: 'positioned' }, position: { kind: 'manual' } });
  });

  it('Einzeldokument mit hinzugefügten Dateien: `show_batch` → Übersicht mit der Platzierung als Plan', async () => {
    const c = core({ settings: batchSettings() });
    await until(c, (x) => x.step.step === 'open');
    await c.dispatch({ type: 'open_files', paths: [`${DIR}/Vertrag.pdf`] });
    await until(c, hasDoc);
    await c.dispatch({ type: 'open_files', paths: [`${DIR}/Angebot.pdf`, `${DIR}/Vertrag.pdf`] });
    let v = await c.view();
    expect(v.notices.some((n) => n.code === 'ng-batch-added' && n.params.count === '1')).toBe(true);
    expect(v.actions.show_batch).toBe(true);
    await c.dispatch({ type: 'show_batch' });
    v = await until(c, ready);
    expect(v.batch?.items.map((i) => [i.name, i.status.state])).toEqual([
      ['Vertrag.pdf', 'positioned'],
      ['Angebot.pdf', 'ready'],
    ]);
    // Startseite verwirft den Stapel
    await c.dispatch({ type: 'navigate', step: 'Open' });
    v = await c.view();
    expect(v.step.step).toBe('open');
    expect(v.batch).toBeNull();
  });

  it('aus der Übersicht geöffnet: „Start“ bzw. „Weiteres Dokument“ führt wie im Kern zurück zur Übersicht, der Stapel bleibt', async () => {
    const c = core({ scenario: 'batch', settings: batchSettings() });
    await until(c, ready);
    await c.dispatch({ type: 'open_batch_item', doc: 1 });
    await until(c, hasDoc);
    await c.dispatch({ type: 'navigate', step: 'Open' });
    const v = await c.view();
    expect(v.step.step).toBe('batch');
    expect(v.batch?.items).toHaveLength(4);
    expect(v.batch?.open_item).toBeNull();
    expect(v.batch?.items[0]?.status.state).toBe('ready');
  });

  it('Mobile: Ausgabe in die App-Dokumente, keine Ordnerwahl', async () => {
    const c = core({ scenario: 'batch', settings: batchSettings({ output: { folder: null, postfix: '_signed' } }), platform: 'android' });
    const v = await until(c, ready);
    expect(v.batch?.output).toEqual({ mode: 'app_documents', folder_display: null, needs_folder: false });
    await c.dispatch({ type: 'start_batch', backend: 'atrust_native', docs: [1], output_dir: null, on_existing: 'keep_both' });
    const { view } = await runAll(c);
    expect(view.batch?.items[0]?.status).toEqual({ state: 'signed', path: '/var/mobile/Containers/Data/Application/DEMO/Documents/Angebot_2026-118_signed.pdf' });
    expect(view.batch?.summary?.folder_display).toBeNull();
  });

  // „PDF hinzufügen“ (Nachtrag 9) wie der Kern (`Event::MoreFilesAdded`, `Event::DocumentReplaced`)
  it('„Weitere PDFs hinzufügen“: aus dem Stapel geöffnet zurück mit dem neuen Eintrag; nur Duplikate ändern nichts', async () => {
    const c = core({ scenario: 'batch', settings: batchSettings() });
    await until(c, ready);
    await c.dispatch({ type: 'open_batch_item', doc: 1 });
    expect((await until(c, hasDoc)).document?.doc.name).toBe('Angebot_2026-118.pdf');
    // dieselbe Datei: schon im Stapel – nichts ändert sich
    await c.dispatch({ type: 'open_files', paths: [`${DIR}/Angebote/Angebot_2026-118.pdf`], mode: 'add' });
    expect((await c.view()).document?.doc.id).toBe(1);
    await c.dispatch({ type: 'open_files', paths: [`${DIR}/Neu.pdf`], mode: 'add' });
    const back = await until(c, ready);
    expect(back.batch?.items.map((i) => i.name)).toEqual(['Angebot_2026-118.pdf', 'Auftragsbestaetigung_4711.pdf', 'Lieferschein_Scan.pdf', 'Vertrag.pdf', 'Neu.pdf']);
    expect(back.batch?.open_item).toBeNull();
    expect(c.dispatched.some((e) => e.type === 'sign_with' || e.type === 'start_batch')).toBe(false);
  });

  it('„Anderes PDF öffnen“: wartende Dokumente bleiben (zusammen die Übersicht); aus dem Stapel geöffnet ersetzt es den Eintrag', async () => {
    let c = core({ settings: batchSettings() });
    await until(c, (x) => x.step.step === 'open');
    await c.dispatch({ type: 'open_files', paths: [`${DIR}/Vertrag.pdf`] });
    await until(c, hasDoc);
    await c.dispatch({ type: 'open_files', paths: [`${DIR}/Angebot.pdf`] });
    await c.dispatch({ type: 'open_files', paths: [`${DIR}/Auftrag.pdf`], mode: 'replace' });
    let v = await until(c, ready);
    expect(v.batch?.items.map((i) => i.name)).toEqual(['Angebot.pdf', 'Auftrag.pdf']);
    expect(v.done.map((d) => [d.name, d.result.kind])).toEqual([['Vertrag.pdf', 'cancelled']]);

    c = core({ scenario: 'batch', settings: batchSettings() });
    await until(c, ready);
    await c.dispatch({ type: 'open_batch_item', doc: 2 });
    await until(c, hasDoc);
    await c.dispatch({ type: 'open_files', paths: [`${DIR}/Ersatz.pdf`], mode: 'replace' });
    v = await until(c, ready);
    expect(v.batch?.items.map((i) => i.name)).toEqual(['Angebot_2026-118.pdf', 'Lieferschein_Scan.pdf', 'Vertrag.pdf', 'Ersatz.pdf']);
  });

  it('`mode` ohne Dokument im Arbeitsbereich wie `open_files` (Start: zwei Dateien → Übersicht)', async () => {
    const c = core({ settings: batchSettings() });
    await until(c, (x) => x.step.step === 'open');
    await c.dispatch({ type: 'open_files', paths: [`${DIR}/A.pdf`, `${DIR}/B.pdf`], mode: 'replace' });
    const v = await until(c, ready);
    expect(v.batch?.items.map((i) => i.name)).toEqual(['A.pdf', 'B.pdf']);
  });
});

describe('Automatische Positionierung (Nachtrag 8, Mock wie der Kern)', () => {
  const autoSettings = (): Settings => ({ ...defaultSettings(), signature: { ...defaultSettings().signature, auto_position: true } });
  const batchSettings = (): Settings => ({ ...idAustria(), placeholder: { search: true, kind: 'marker' } });
  const ready = (v: ViewState) => v.step.step === 'batch' && !!v.batch && v.batch.items.every((i) => i.status.state !== 'analyzing');

  it('Einstellung an: gesperrt, Verschieben und Zielwahl ohne Wirkung, „Diesmal selbst platzieren“ nur für dieses Dokument; keine Sitzung', async () => {
    const c = core({ scenario: 'contract', settings: autoSettings() });
    let v = await until(c, hasDoc);
    const before = v.document!;
    expect(before.placement_locked).toBe(true);
    for (const e of [
      { type: 'move_block', page: 0, x: 40, y: 500 },
      { type: 'nudge_block', dx: 1, dy: 0, fine: false, coarse: true },
      { type: 'change_page', page: 0 },
      { type: 'auto_place' },
      { type: 'use_target', target: { kind: 'free' } },
    ] as UiEvent[]) await c.dispatch(e);
    v = await c.view();
    expect(v.document!.placement).toEqual(before.placement);
    expect(v.document!.revision).toBe(before.revision);
    await c.dispatch({ type: 'set_invisible', invisible: true });
    expect((await c.view()).document!.invisible).toBe(true);
    await c.dispatch({ type: 'set_invisible', invisible: false });
    await c.dispatch({ type: 'unlock_placement' });
    v = await c.view();
    expect(v.document!.placement_locked).toBe(false);
    await c.dispatch({ type: 'nudge_block', dx: 1, dy: 0, fine: false, coarse: false });
    expect((await c.view()).document!.origin).toBe('user');
    expect(c.dispatched.some((e) => e.type === 'sign_with')).toBe(false);
    expect(v.step.step).not.toBe('signing');
  });

  it('Einstellung aus: wie bisher; aus der Stapelübersicht geöffnet nie gesperrt', async () => {
    const c = core({ scenario: 'contract' });
    expect((await until(c, hasDoc)).document!.placement_locked).toBe(false);
    const b = core({ scenario: 'batch', settings: { ...batchSettings(), signature: { ...batchSettings().signature, auto_position: true } } });
    await until(b, ready);
    expect((await b.view()).batch?.auto_position_setting).toBe(true);
    await b.dispatch({ type: 'open_batch_item', doc: 1 });
    expect((await until(b, hasDoc)).document!.placement_locked).toBe(false);
  });

  it('Stapel: „Für diesen Stapel automatisch positionieren“ nur für diesen Stapel, eigene Pläne bleiben außer `replace_manual`', async () => {
    const c = core({ scenario: 'batch', settings: batchSettings() });
    await until(c, ready);
    await c.dispatch({ type: 'open_batch_item', doc: 1 });
    await until(c, hasDoc);
    await c.dispatch({ type: 'nudge_block', dx: 0, dy: 1, fine: false, coarse: false });
    await c.dispatch({ type: 'back_to_batch' });
    let v = await c.view();
    expect(v.batch).toMatchObject({ auto_position_once: false, manual_plans: 1 });
    await c.dispatch({ type: 'set_batch_auto_position', enabled: true, replace_manual: false });
    v = await c.view();
    expect(v.batch).toMatchObject({ auto_position_once: true, auto_position_setting: false, manual_plans: 1 });
    expect(v.batch?.items[0]?.status.state).toBe('positioned');
    expect(v.batch?.items.find((i) => i.position.kind === 'qr')?.status.state).toBe('ready');
    await c.dispatch({ type: 'set_batch_auto_position', enabled: true, replace_manual: true });
    v = await c.view();
    expect(v.batch?.manual_plans).toBe(0);
    expect(v.batch?.items[0]).toMatchObject({ status: { state: 'ready' }, position: { kind: 'auto' } });
    expect(v.step.step).toBe('batch');
    expect(c.dispatched.some((e) => e.type === 'start_batch')).toBe(false);
  });
});

describe('„Zur Startseite“ parkt offene Arbeit (Nachtrag 10, Mock wie der Kern)', () => {
  const DIR = '/Users/demo/Dokumente';
  const parkedStart = (v: ViewState) => v.step.step === 'open' && !!v.parked && v.document === null && v.batch === null;
  const ready = (v: ViewState) => v.step.step === 'batch' && !!v.batch && v.batch.items.every((i) => i.status.state !== 'analyzing');

  it('Arbeitsbereich: parken, verdeckt nicht signierbar, fortsetzen mit unveränderter Platzierung', async () => {
    const c = core({ scenario: 'contract', settings: idAustria() });
    await until(c, hasDoc);
    await c.dispatch({ type: 'nudge_block', dx: 0, dy: 1, fine: false, coarse: false });
    const before = (await c.view()).document!;
    await c.dispatch({ type: 'go_home' });
    const v = await until(c, parkedStart);
    expect(v.parked).toEqual({ doc_name: 'Vertrag.pdf', documents: 1, placed_by_hand: true, unsaved: false });
    expect(v.actions).toMatchObject({ sign: false, edit_placement: false, open_files: true });
    // verdeckt (F-54): weder Signieren noch Verschieben wirken
    await c.dispatch({ type: 'sign_with', backend: 'atrust_native' });
    await c.dispatch({ type: 'nudge_block', dx: 1, dy: 0, fine: false, coarse: false });
    expect((await c.view()).parked).not.toBeNull();
    await c.dispatch({ type: 'resume_parked' });
    const back = await until(c, hasDoc);
    expect(back.parked).toBeNull();
    expect(back.document!.placement).toEqual(before.placement);
  });

  it('neue PDFs: „Zum Stapel hinzufügen“ bildet mit dem geparkten Dokument den Stapel, „Öffnen mit“ mit Hinweis', async () => {
    const c = core({ scenario: 'contract', settings: idAustria() });
    await until(c, hasDoc);
    await c.dispatch({ type: 'set_invisible', invisible: true });
    await c.dispatch({ type: 'go_home' });
    await until(c, parkedStart);
    await c.dispatch({ type: 'open_files', paths: [`${DIR}/Angebot.pdf`], mode: 'add' });
    let v = await until(c, ready);
    expect(v.batch?.items.map((i) => [i.name, i.position.kind])).toEqual([
      ['Vertrag.pdf', 'invisible'],
      ['Angebot.pdf', 'auto'],
    ]);
    expect(v.notices.some((n) => n.code === 'ng-parked-files-added')).toBe(false);
    // Stapel parken; „Öffnen mit“ (ohne `mode`): ohne Rückfrage dazu, Hinweis
    await c.dispatch({ type: 'go_home' });
    v = await until(c, parkedStart);
    expect(v.parked).toMatchObject({ doc_name: null, documents: 2, placed_by_hand: true });
    await c.dispatch({ type: 'open_files', paths: [`${DIR}/Auftrag.pdf`] });
    v = await until(c, ready);
    expect(v.batch?.items).toHaveLength(3);
    expect(v.notices.some((n) => n.code === 'ng-parked-files-added' && n.params.count === '1')).toBe(true);
    // „Offene schließen und neu beginnen“: Stapel verworfen, das neue Dokument öffnet
    await c.dispatch({ type: 'go_home' });
    await until(c, parkedStart);
    await c.dispatch({ type: 'open_files', paths: [`${DIR}/Neu.pdf`], mode: 'replace' });
    v = await until(c, hasDoc);
    expect([v.document?.doc.name, v.batch, v.parked]).toEqual(['Neu.pdf', null, null]);
  });

  it('„Schließen“ verwirft; beim Signieren und ohne offene Arbeit ohne Wirkung; „Neuer Stapel“ wie bisher', async () => {
    const c = core({ scenario: 'contract', settings: idAustria() });
    await until(c, hasDoc);
    await c.dispatch({ type: 'go_home' });
    await until(c, parkedStart);
    await c.dispatch({ type: 'discard_parked' });
    let v = await c.view();
    expect([v.step.step, v.parked, v.document]).toEqual(['open', null, null]);
    expect(v.done.map((d) => d.result.kind)).toEqual(['cancelled']);
    await c.dispatch({ type: 'go_home' });
    expect((await c.view()).parked).toBeNull();
    // beim Signieren gesperrt
    await c.dispatch({ type: 'open_files', paths: [`${DIR}/Vertrag.pdf`] });
    await until(c, hasDoc);
    await c.dispatch({ type: 'sign_with', backend: 'atrust_native' });
    await until(c, (x) => x.step.step === 'signing');
    await c.dispatch({ type: 'go_home' });
    v = await c.view();
    expect([v.step.step, v.parked]).toEqual(['signing', null]);
  });

  it('Stapel (Nachtrag 13): ein geöffneter Eintrag kehrt mit „Weiter bearbeiten“ als Plan in die Übersicht zurück', async () => {
    const c = core({ scenario: 'batch', settings: idAustria() });
    let v = await until(c, ready);
    const doc = v.batch!.items[0]!.doc;
    await c.dispatch({ type: 'open_batch_item', doc });
    await until(c, hasDoc);
    await c.dispatch({ type: 'nudge_block', dx: 0, dy: 1, fine: false, coarse: false });
    await c.dispatch({ type: 'go_home' });
    v = await until(c, parkedStart);
    expect(v.parked).toMatchObject({ doc_name: null, documents: 4 });
    await c.dispatch({ type: 'resume_parked' });
    v = await c.view();
    expect([v.step.step, v.document, v.batch?.open_item, v.parked]).toEqual(['batch', null, null, null]);
    expect(v.batch?.items[0]).toMatchObject({ doc, status: { state: 'positioned' }, position: { kind: 'manual' } });
  });

});

describe('Mehrfachsignatur (je Block eine eigene Signatur)', () => {
  const blocks = (v: ViewState) => v.document!.blocks!;

  it('hinzufügen, auswählen, entfernen: die Werkzeuge wirken auf den ausgewählten Block', async () => {
    const c = core({ scenario: 'contract' });
    let v = await until(c, hasDoc);
    expect(blocks(v)).toMatchObject({ limit: 10, can_add: true, items: [{ index: 0, active: true }] });
    const first = blocks(v).items[0]!.rect;
    await c.dispatch({ type: 'add_block', page: 0 });
    v = await c.view();
    expect(blocks(v).items.map((b) => [b.index, b.active, b.page])).toEqual([[0, false, v.document!.blocks!.items[0]!.page], [1, true, 0]]);
    expect(blocks(v).items[0]!.rect).toEqual(first);
    expect(v.document!.sticker).toEqual(blocks(v).items[1]!.rect);
    expect(v.document!.check.overlaps_block).toBe(false);
    // dritter Block auf derselben Seite: überdeckt keinen anderen
    await c.dispatch({ type: 'add_block', page: 0 });
    v = await c.view();
    expect(blocks(v).items).toHaveLength(3);
    expect(v.document!.check.overlaps_block).toBe(false);
    // auswählen und verschieben: nur der ausgewählte Block bewegt sich
    await c.dispatch({ type: 'select_block', index: 1 });
    const before = blocks(await c.view()).items;
    await c.dispatch({ type: 'nudge_block', dx: 1, dy: 0, fine: false, coarse: false });
    const after = blocks(await c.view()).items;
    expect(after[1]!.rect).not.toEqual(before[1]!.rect);
    expect([after[0], after[2]]).toEqual([before[0], before[2]]);
    // auf den dritten geschoben: die Prüfung meldet es
    const r = after[2]!.rect!;
    await c.dispatch({ type: 'move_block', page: after[2]!.page, x: r.left + 4, y: r.top - 4 });
    expect((await c.view()).document!.check.overlaps_block).toBe(true);
    // entfernen: der ausgewählte geht, der vorige ist ausgewählt; der letzte Block bleibt
    await c.dispatch({ type: 'remove_block', index: 1 });
    v = await c.view();
    expect(blocks(v).items.map((b) => b.active)).toEqual([true, false]);
    await c.dispatch({ type: 'remove_block', index: 1 });
    await c.dispatch({ type: 'remove_block', index: 0 });
    v = await c.view();
    expect(blocks(v).items).toHaveLength(1);
    expect(c.dispatched.some((e) => e.type === 'sign_with')).toBe(false);
  });

  it('Grenzen: höchstens 10 Blöcke, „Auf neuer Seite“ nur mit einem Block, unsichtbar eine Signatur', async () => {
    const c = core({ scenario: 'contract' });
    await until(c, hasDoc);
    for (let i = 0; i < 9; i++) await c.dispatch({ type: 'add_block', page: 0 });
    let v = await c.view();
    expect(blocks(v)).toMatchObject({ can_add: false });
    expect(blocks(v).items).toHaveLength(10);
    await c.dispatch({ type: 'add_block', page: 0 });
    v = await c.view();
    expect(blocks(v).items).toHaveLength(10);
    expect(v.notices.some((n) => n.code === 'ng-blocks-limit')).toBe(true);
    await c.dispatch({ type: 'toggle_new_page' });
    v = await c.view();
    expect(v.document!.new_page).toBe(false);
    expect(v.notices.some((n) => n.code === 'ng-blocks-new-page')).toBe(true);
    await c.dispatch({ type: 'set_invisible', invisible: true });
    v = await c.view();
    expect(blocks(v).items).toHaveLength(1);
    // auf neuer Seite: kein weiterer Block
    const d = core({ scenario: 'contract' });
    await until(d, hasDoc);
    await d.dispatch({ type: 'toggle_new_page' });
    await d.dispatch({ type: 'add_block', page: 0 });
    v = await d.view();
    expect([v.document!.new_page, blocks(v).items.length, blocks(v).can_add]).toEqual([true, 1, false]);
  });

  it('„Alle Platzhalter belegen“: der freie Block wandert ins erste Feld, je weiterem Feld ein Block; ein Feld nur einmal', async () => {
    const c = core({ scenario: 'field' });
    let v = await until(c, hasDoc);
    const offered = v.document!.fields.filter((f) => f.suitable).length;
    expect(offered).toBeGreaterThanOrEqual(1);
    expect(blocks(v).free_targets).toBe(offered);
    await c.dispatch({ type: 'fill_targets' });
    v = await c.view();
    expect(v.document!.target).toEqual({ kind: 'field', index: 0 });
    expect(blocks(v).items).toHaveLength(offered);
    expect(blocks(v).free_targets).toBe(0);
    // ein zweiter Block kann nicht in dasselbe Feld
    await c.dispatch({ type: 'add_block', page: 0 });
    await c.dispatch({ type: 'use_target', target: { kind: 'field', index: 0 } });
    v = await c.view();
    expect(v.document!.target).toEqual({ kind: 'free' });
    expect(v.notices.some((n) => n.code === 'ng-block-target-taken')).toBe(true);
  });

  it('Signieren: je Block eine Bestätigung im Fortschritt', async () => {
    const c = core({ scenario: 'contract', settings: idAustria() });
    await until(c, hasDoc);
    await c.dispatch({ type: 'add_block', page: 0 });
    await c.dispatch({ type: 'add_block', page: 1 });
    await c.dispatch({ type: 'sign_with', backend: 'atrust_native' });
    const v = await until(c, (x) => x.step.step === 'signing');
    expect(v.step.step === 'signing' && v.step.progress.confirmations_total).toBe(3);
  });
});
