/**
 * Schalter „Barrierefreie Bedienung“ in der Kopfzeile (docs/analyse/06) – neben Befehlen, Sprache, Farbschema, Hilfe
 * und Einstellungen, ersetzt nichts. Schmal nur das Symbol (der zugängliche Name bleibt). Setzt die Werte mit
 * `set_accessibility` (`withMode`) und sagt das Ergebnis an.
 *
 * Einschalten von außen: Direktlink `?barrierefrei=1` (z. B. von PDF-AS-WAI). Im Web wird der Schalter nicht gemerkt
 * (dort werden Einstellungen nicht gespeichert, docs/integration/web.md): er gilt bis zum Neuladen (ADR 0004
 * Nachtrag 28). Ein bis 0.8.2 gemerkter Wert wird beim Start entfernt.
 */
import { useEffect, useId } from 'react';
import { Switch } from 'react-aria-components';
import { useApp } from '../../app/context';
import { webProfile } from '../../core/web-profile';
import { IconAccessibility } from '../../ui/Icons';
import { accessibilityOf, modeRequestedByUrl, withMode } from './index';

/** Bis 0.8.2 merkte sich der Browser den Schalter unter diesem Schlüssel – der Rest wird entfernt. */
const LEGACY_WEB_KEY = 'pida-barrierefrei';

function forgetLegacyOnWeb(): void {
  if (!webProfile()) return;
  try {
    window.localStorage.removeItem(LEGACY_WEB_KEY);
  } catch {
    // ohne Speicher (privates Fenster): nichts zu entfernen
  }
}

/** Einmal je Programmstart: Direktlink. */
let startHandled = false;

export function A11ySwitch() {
  const { t, view, dispatch, announce } = useApp();
  const descId = useId();
  const a = accessibilityOf(view);
  const ready = !!view.accessibility;

  useEffect(() => {
    if (!ready || startHandled) return;
    startHandled = true;
    forgetLegacyOnWeb();
    const wanted = typeof window !== 'undefined' && modeRequestedByUrl(window.location.search);
    if (wanted && !a.mode) void dispatch({ type: 'set_accessibility', accessibility: withMode(a, true) });
  }, [ready, a, dispatch]);

  if (!ready) return null;
  const toggle = (on: boolean) => {
    void dispatch({ type: 'set_accessibility', accessibility: withMode(a, on) });
    announce(t.t(on ? 'ng-ui-shell-a11y-on' : 'ng-ui-shell-a11y-off'));
  };
  return (
    <>
      <Switch className="a11y-switch" isSelected={a.mode} onChange={toggle} aria-label={t.t('ng-ui-shell-a11y')} aria-describedby={descId} data-testid="a11y-switch">
        <IconAccessibility size={18} />
        <span className="a11y-switch-text" aria-hidden="true">
          {t.t('ng-ui-shell-a11y')}
        </span>
        <span className="switch-track" aria-hidden="true">
          <span className="switch-thumb" />
        </span>
      </Switch>
      <span id={descId} className="sr-only">
        {t.t('ng-ui-shell-a11y-desc')}
      </span>
    </>
  );
}

/** Nur für Tests: Start erneut behandeln. */
export function resetA11yStartForTests(): void {
  startHandled = false;
}
