/**
 * „Vor dem Signieren prüfen“ (Einstellungen › Barrierefreiheit, docs/analyse/06 R-1): Platzierung als Text,
 * Signaturbild, Signiermittel und – deutlich – eine Überdeckung, bevor „Signieren“ wirklich startet. Der Fokus
 * liegt auf „Signieren“; Escape bzw. „Zurück zur Platzierung“ schließt ohne Signieren.
 */
import { useApp } from '../../app/context';
import type { DocumentView } from '../../core/types';
import { Button } from '../../ui/controls';
import { DialogHeader, ModalShell } from '../../ui/Dialog';
import { IconAlert, IconSignature } from '../../ui/Icons';
import { formatMm } from '../accessibility';
import { announceText, blockTotal, overlapState } from './placement';
import { chipLabel } from './signers';

export function SignCheck({ doc, onCancel, onSign }: { doc: DocumentView; onCancel: () => void; onSign: () => void }) {
  const { t, view } = useApp();
  const signer = view.signers.options.find((o) => o.backend === view.signers.selected);
  const overlap = overlapState(doc);
  return (
    <ModalShell onClose={onCancel} size="md" className="sign-check">
      <DialogHeader title={t.t('ng-ui-insp-check-title')} icon={<IconSignature size={20} />} onClose={onCancel} />
      <div className="confirm-body">
        <dl className="sign-check-list">
          <dt>{t.t('ng-ui-insp-check-placement')}</dt>
          <dd data-testid="sign-check-placement">{announceText(t, doc, [], 'detailed')}</dd>
          <dt>{t.t('ng-ui-insp-check-block')}</dt>
          <dd>
            {doc.invisible
              ? t.t('ng-ui-insp-check-invisible')
              : t.t('ng-ui-insp-check-block-size', { w: formatMm(doc.block.width, t.lang), h: formatMm(doc.block.height, t.lang) })}
          </dd>
          {blockTotal(doc) > 1 ? (
            <>
              <dt>{t.t('ng-ui-insp-blocks')}</dt>
              <dd data-testid="sign-check-count">{t.t('ng-ui-insp-sign-count', { n: blockTotal(doc) })}</dd>
            </>
          ) : null}
          {signer ? (
            <>
              <dt>{t.t('ng-ui-insp-check-signer')}</dt>
              <dd>{chipLabel(t, signer)}</dd>
            </>
          ) : null}
        </dl>
        {overlap ? (
          <div className="callout callout-warning" data-testid="sign-check-overlap">
            <IconAlert size={18} />
            <p>{t.t('ng-ui-insp-check-overlap')}</p>
          </div>
        ) : null}
      </div>
      <div className="confirm-actions dialog-actions">
        <Button variant="ghost" onPress={onCancel}>
          {t.t('ng-ui-insp-check-back')}
        </Button>
        <Button variant="primary" onPress={onSign} autoFocus>
          {t.t('ng-ui-insp-check-sign')}
        </Button>
      </div>
    </ModalShell>
  );
}
