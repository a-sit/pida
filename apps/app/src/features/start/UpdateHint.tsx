/**
 * Hinweis auf eine neue Version auf der Startseite (Backlog Nr. 33, `ViewState::update_hint`): dezent und nicht
 * blockierend über der Ablagefläche, mit Link auf die Release-Seite (nichts wird heruntergeladen) und „Ausblenden“
 * (`dismiss_update_hint`, dauerhaft für diese Version). Wann er erscheint – mit eingeschalteter automatischer
 * Prüfung und für eine nicht ausgeblendete Version –, entscheidet der Kern. Auf iOS/Android (Nachtrag 31) meldet der
 * Kern nur Versionen, die im Store bereitstehen; der Link führt dann in den Play Store bzw. zu TestFlight.
 */
import { Link } from 'react-aria-components';
import { useApp } from '../../app/context';
import { updateStore } from '../../core/types';
import { IconButton } from '../../ui/controls';
import { IconClose, IconExternal, IconInfo } from '../../ui/Icons';

export function UpdateHint() {
  const { t, view, dispatch, bridge, toast } = useApp();
  const hint = view.update_hint;
  if (!hint) return null;
  const url = hint.release_url;
  const store = updateStore(view);
  return (
    <div className="callout callout-info start-update" role="note" data-testid="update-hint">
      <IconInfo size={18} />
      <div>
        <p>{t.t('ng-ui-start-update', { version: hint.version })}</p>
        <Link
          className="link"
          aria-label={t.t(store ? `ng-ui-start-update-${store}-label` : 'ng-ui-start-update-open-label', { version: hint.version })}
          onPress={() => bridge.openUrl(url).catch(() => toast(t.t('ng-ui-open-failed'), 'error'))}
        >
          {t.t(store ? `ng-ui-start-update-${store}` : 'ng-ui-start-update-open')}
          <IconExternal size={14} />
        </Link>
      </div>
      <IconButton className="start-update-close" size="sm" label={t.t('ng-ui-start-update-dismiss')} tooltip={false} onPress={() => void dispatch({ type: 'dismiss_update_hint' })}>
        <IconClose size={16} />
      </IconButton>
    </div>
  );
}
