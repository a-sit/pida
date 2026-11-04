/**
 * Mock-Kern, Bereich „Über PIDA“ (ADR 0004 Nachtrag 21): `ViewState::about` mit Version, Versionsdatum, Links und
 * der Update-Prüfung nach den Regeln des Kerns (`crates/core-api/src/update.rs`) – ohne Netz:
 *
 * - iOS/Android: keine Prüfung (`update = null`); Richtlinie `update_check = false` → `policy_off` (nie prüfen,
 *   auch nicht auf Klick); Schalter aus → `disabled`, bis „Jetzt prüfen“ einmalig prüft (das Ergebnis gilt, bis
 *   der Schalter wechselt).
 * - Start: das Ergebnis der Prüfung beim Start liegt wie aus dem Zwischenspeicher sofort vor (Szenario), außer bei
 *   `unchecked`/`checking`; `check_for_updates { explicit: false }` (Abschnitt geöffnet) prüft nur ohne Ergebnis –
 *   nach einem Fehler nicht bei jedem Öffnen erneut (der Kern wartet 15 Minuten, beim Rate-Limit eine Stunde) –,
 *   `explicit: true` immer; jeweils nach der simulierten Latenz, mit `checking` dazwischen.
 * - Szenarien (`MockOptions::update`, Browser-Vorschau `?update=`): `up_to_date` (Standard), `available` (0.3.0),
 *   `failed` (erst Rate-Limit, „Erneut prüfen“ gelingt), `offline` (jede Prüfung ohne Verbindung), `unchecked`
 *   (noch kein Ergebnis; das Öffnen prüft), `checking` (die Prüfung endet nie – für Screenshots).
 * - Startseite (Backlog Nr. 33): `update_hint` bei bekannter neuer Version, bis `dismiss_update_hint`.
 */
import type { AboutView, UpdateCheckState, UpdateCheckView, UpdateHintView } from '../types';
import type { PolicyMock } from './settings';
import type { MockArea, MockCtx, MockOptions, MockUpdateScenario } from './shared';

/** Version und Versionsdatum des Mocks (wie ein Build vom 17.10.2026). */
export const MOCK_VERSION = '0.2.0';
export const MOCK_RELEASE_DATE = '2026-10-17';
const REPO = 'https://github.com/a-sit/pida';

type Outcome = { kind: 'checked'; latest: string; at: string } | { kind: 'failed'; reason: string };

interface AboutState {
  scenario: MockUpdateScenario;
  mobile: boolean;
  policy: PolicyMock | undefined;
  running: boolean;
  outcome: Outcome | null;
  /** Ergebnis aus „Jetzt prüfen“ (gilt bei ausgeschalteter Prüfung, bis der Schalter wechselt) */
  explicit: boolean;
  /** zuletzt gesehener Schalter (Richtlinie vor Einstellung) */
  enabled: boolean;
  attempts: number;
  /** Startseite: Hinweis für diese Version ausgeblendet (`dismiss_update_hint`) */
  dismissed: string | null;
}

const AT = '2026-10-17T10:42:17+02:00';

function result(s: AboutState): Outcome {
  switch (s.scenario) {
    case 'available':
      return { kind: 'checked', latest: '0.3.0', at: AT };
    case 'failed':
      // erst Rate-Limit, „Erneut prüfen“ gelingt
      return s.attempts <= 1 ? { kind: 'failed', reason: 'ng-update-failed-rate-limited' } : { kind: 'checked', latest: MOCK_VERSION, at: AT };
    case 'offline':
      return { kind: 'failed', reason: 'ng-update-failed-offline' };
    default:
      return { kind: 'checked', latest: MOCK_VERSION, at: AT };
  }
}

function autoEnabled(s: AboutState, ctx: MockCtx): boolean {
  return s.policy?.update_check ?? ctx.settings().general.update_check;
}

function begin(s: AboutState, ctx: MockCtx): void {
  s.running = true;
  s.attempts += 1;
  if (s.scenario === 'checking') return; // endet nie (Screenshots „Suche nach Updates …“)
  ctx.later(() => {
    s.running = false;
    s.outcome = result(s);
  }, 2);
}

function compare(a: string, b: string): number {
  const pa = a.split('.').map(Number);
  const pb = b.split('.').map(Number);
  for (let i = 0; i < 3; i += 1) {
    const d = (pa[i] ?? 0) - (pb[i] ?? 0);
    if (d !== 0) return d;
  }
  return 0;
}

function view(s: AboutState, ctx: MockCtx): UpdateCheckView | null {
  if (s.mobile) return null;
  const checkedAt = s.outcome?.kind === 'checked' ? s.outcome.at : null;
  const base = (state: UpdateCheckState): UpdateCheckView => ({ state, latest_version: null, release_url: null, checked_at: checkedAt, reason: null });
  if (s.policy?.update_check === false) return base('policy_off');
  if (s.running) return base('checking');
  if (!autoEnabled(s, ctx) && !s.explicit) return base('disabled');
  const o = s.outcome;
  if (!o) return base('unchecked');
  if (o.kind === 'failed') return { ...base('failed'), reason: o.reason };
  if (compare(o.latest, MOCK_VERSION) > 0) return { ...base('available'), latest_version: o.latest, release_url: `${REPO}/releases/tag/v${o.latest}` };
  return { ...base('up_to_date'), latest_version: o.latest };
}

/** Startseite (Backlog Nr. 33): neue Version bekannt, automatische Prüfung an, nicht ausgeblendet – wie der Kern. */
function hint(s: AboutState, ctx: MockCtx): UpdateHintView | null {
  if (s.mobile || s.policy?.update_check === false || !autoEnabled(s, ctx)) return null;
  const o = s.outcome;
  if (o?.kind !== 'checked' || compare(o.latest, MOCK_VERSION) <= 0 || s.dismissed === o.latest) return null;
  return { version: o.latest, release_url: `${REPO}/releases/tag/v${o.latest}` };
}

export const aboutArea: MockArea<AboutState> = {
  init(o: MockOptions & { policy?: PolicyMock }) {
    const scenario = o.update ?? 'up_to_date';
    const mobile = (o.platform ?? 'desktop') !== 'desktop';
    const policy = o.policy;
    const enabledAtStart = policy?.update_check ?? o.settings?.general.update_check ?? true;
    const s: AboutState = { scenario, mobile, policy, running: false, outcome: null, explicit: false, enabled: enabledAtStart, attempts: 0, dismissed: null };
    // Prüfung beim Start (wie aus dem Zwischenspeicher): nur am Desktop, erlaubt und eingeschaltet
    if (!mobile && policy?.update_check !== false && enabledAtStart && scenario !== 'unchecked' && scenario !== 'checking') {
      s.attempts = 1;
      s.outcome = result(s);
    }
    return s;
  },

  handle(event, s, ctx) {
    if (event.type === 'dismiss_update_hint') {
      const h = hint(s, ctx);
      if (h) s.dismissed = h.version;
      return true;
    }
    if (event.type !== 'check_for_updates') return false;
    if (s.mobile || s.policy?.update_check === false || s.running) return true;
    // der Mock kennt keine Zeit: ein Ergebnis (Erfolg oder Fehler) gilt als frisch
    if (!event.explicit && (!autoEnabled(s, ctx) || s.outcome !== null)) return true;
    s.explicit = event.explicit;
    begin(s, ctx);
    return true;
  },

  signal(signal, s, ctx) {
    if (signal.kind !== 'settings-changed') return;
    const enabled = autoEnabled(s, ctx);
    if (s.enabled !== enabled) s.explicit = false;
    s.enabled = enabled;
  },

  project(s, ctx) {
    const about: AboutView = {
      version: MOCK_VERSION,
      release_date: MOCK_RELEASE_DATE,
      release_notes_url: `${REPO}/releases/tag/v${MOCK_VERSION}`,
      source_url: REPO,
      update: view(s, ctx),
    };
    return { about, update_hint: hint(s, ctx) };
  },
};
