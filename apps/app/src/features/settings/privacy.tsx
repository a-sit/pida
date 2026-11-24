/**
 * Datenschutz ohne Zustimmungsdialog (ADR 0003): Statt eines blockierenden Hinweises vor dem ersten
 * Upload zeigt die Oberfläche beim Signiermittel mit `document_leaves_device` (ID Austria) einen
 * **Kurzhinweis** „Das Dokument wird zum Signieren an A-Trust übermittelt.“ mit „Details“. „Details“ öffnet
 * die Einstellungen beim Abschnitt „Datenschutz“; während des Signierens (Einstellungen gesperrt wie in
 * PDF-Over) dieselben Angaben als schließbares Fenster.
 *
 * Der vollständige Text ([`PrivacyInfo`]) ist der Datenschutztext aus PDF-Over (auf Mobilgeräten ohne den Satz zur
 * Chipkarte, `ng-ui-privacy-statement-mobile`) bzw. – per Richtlinie
 * `privacy_notice_text` – der eigene Text der Organisation, dazu der Hinweis auf das lokale Signieren mit
 * Keystore und Chipkarte (Mobilgeräte: dort wird nur mit ID Austria signiert, ADR 0004 Nachtrag 11 – der Hinweis
 * sagt das), am Desktop ein Satz zur Update-Prüfung (Abfrage bei GitHub, IP-Adresse sichtbar, keine Kennungen,
 * abschaltbar; ADR 0004 Nachtrag 21) und der Link zur Datenschutzerklärung von A-Trust. Zum Schluss der Link zur
 * Datenschutzerklärung von PIDA selbst (pdf.a-sit.at/datenschutz/; die Stores verlangen sie aus der App heraus).
 */
import { useState } from 'react';
import { Link } from 'react-aria-components';
import { useApp } from '../../app/context';
import { isMobile } from '../../core/types';
import { webProfile } from '../../core/web-profile';
import { PRIVACY_URL } from './about';
import { AccessButton } from '../../ui/controls';
import { ConfirmDialog } from '../../ui/Dialog';
import { IconShield } from '../../ui/Icons';
import { Markup } from '../../ui/Markup';

/** Vollständige Datenschutzangaben (Einstellungen › Datenschutz und „Details“ im Signierbereich). */
export function PrivacyInfo() {
  const { t, view } = useApp();
  const url = t.t('config-DataProtectionURL');
  const custom = view.privacy_text;
  const web = webProfile() !== null;
  return (
    <>
      <p className="pre-line">
        {custom ? custom : <Markup text={t.t(web ? 'ng-ui-privacy-statement-web' : isMobile(view) ? 'ng-ui-privacy-statement-mobile' : 'ng-ui-privacy-statement')} targets={[{ href: url }]} />}
      </p>
      <p>{t.t(web ? 'ng-ui-privacy-local-web' : isMobile(view) ? 'ng-ui-privacy-local-mobile' : 'ng-ui-privacy-local')}</p>
      {/* Update-Prüfung (Nachtrag 21, iOS/Android: Nachtrag 31) – im Web prüft PIDA nicht */}
      {web ? null : <p>{t.t(isMobile(view) ? 'ng-ui-privacy-update-check-mobile' : 'ng-ui-privacy-update-check')}</p>}
      {custom ? (
        // Der Text aus PDF-Over nennt die Datenschutzerklärung von A-Trust selbst; ein eigener Text evtl. nicht
        <p>
          <Markup text={t.t('ng-ui-privacy-atrust-link')} targets={[{ href: url }]} />
        </p>
      ) : null}
      <p>
        <Markup text={t.t('ng-ui-privacy-pida-link')} targets={[{ href: PRIVACY_URL }]} />
      </p>
    </>
  );
}

/** Datenschutzangaben als schließbares Fenster (wenn die Einstellungen gerade gesperrt sind). */
function PrivacyDialog({ onClose }: { onClose: () => void }) {
  const { t } = useApp();
  return (
    <ConfirmDialog
      title={t.t('config-DataProtection')}
      onCancel={onClose}
      actions={<AccessButton msg="ng-ui-close" variant="primary" onPress={onClose} autoFocus />}
    >
      <PrivacyInfo />
    </ConfirmDialog>
  );
}

/**
 * Kurzhinweis beim Signiermittel (nicht blockierend). `id` kennzeichnet den Satz, damit die Karte des
 * Signiermittels ihn als Beschreibung referenzieren kann (`aria-describedby`, ohne „Details“). `count` =
 * Dokumente (Stapel): ab zwei „Die Dokumente werden …“ statt „Das Dokument wird …“ (Gerätenachweis iPhone 17e
 * und Android-Emulator, 17.10.2026).
 */
export function PrivacyNote({ id, count = 1 }: { id?: string; count?: number }) {
  const { t, view, openSettings } = useApp();
  const [open, setOpen] = useState(false);
  const details = () => {
    // Einstellungen erreichbar (z. B. Signiermittel-Auswahl): dort beim Abschnitt „Datenschutz“
    if (view.main_bar.settings_enabled && !view.settings) openSettings('privacy');
    else setOpen(true);
  };
  return (
    <>
      <p className="privacy-note" data-testid="privacy-note">
        <IconShield size={16} />
        <span>
          <span id={id}>{t.t(count > 1 ? 'ng-ui-backend-leaves-device-many' : 'ng-ui-backend-leaves-device')}</span>{' '}
          <Link className="link" onPress={details} aria-label={t.t('ng-ui-privacy-details-label')}>
            {t.t('ng-ui-privacy-details')}
          </Link>
        </span>
      </p>
      {open ? <PrivacyDialog onClose={() => setOpen(false)} /> : null}
    </>
  );
}
