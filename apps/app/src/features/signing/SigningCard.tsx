/**
 * Signieren als Statuskarte über dem Arbeitsbereich (Gestalt aus dem Prototyp, ADR 0004 §4.4): kein
 * Wechsel der Ansicht, das Dokument bleibt eingehängt. Die Schritte kommen aus dem Kern –
 * `StepView::Signing { interaction, progress }` (Masken, Fortschritt), `StepView::Backend` (Rückfall
 * „Signiermittel wählen“ nach „Signatur fortsetzen“), `StepView::Error` mit Herkunft `signing`
 * (Fehlerschritt mit Details und „Erneut versuchen“ in derselben Sitzung) und `StepView::Output` ohne
 * Pfad („Signiert“, wird gespeichert). Validierung zeigt die Karte vorab wie PDF-Over; maßgeblich ist das
 * `FormProblem` des Kerns.
 *
 * Modal mit Fokusfalle, Esc/×/„Abbrechen“ = `cancel`, kein Schließen durch Klick daneben. Kurzhinweis
 * „wird an A-Trust übermittelt“ (ADR 0003) bei `document_leaves_device`. Passwörter, PINs und TANs
 * bleiben nur im Formularzustand, werden nie geloggt und nach dem Senden verworfen.
 *
 * Stapel (`progress.batch`, ADR 0004 §3.8/§4.4): Kopf „Dokument k von n · Name“ bzw. „Bestätigung k von n ·
 * Name“, Zähler „Bestätigungen k von n“ (Keystore: „Dokument k von n“, „Signiert k von n“ – Wortlaut je
 * Signiermittel nach `SignerView::batch_flow`, Nachtrag 12), aufklappbare Liste, „Dokument überspringen“ (nur dieser Job, die
 * Anmeldung bleibt) und „Stapel abbrechen“; nach dem Überspringen der Hinweis auf eine offene Anfrage in der
 * App (`ng-batch-pending-app-request`). Pausen (`progress.pause`) mit „Erneut versuchen“ (bei neuer Sitzung mit
 * Hinweis auf eine mögliche neue Anmeldung), „Überspringen“ (nur mit Dokument) und „Stapel abbrechen“. Esc/×
 * fragen im Stapel erst nach, bevor der Stapel abgebrochen wird.
 */
import { createContext, useContext, useEffect, useRef, useState, type CSSProperties, type FormEvent, type ReactNode } from 'react';
import { Button as AriaButton } from 'react-aria-components';
import { useApp } from '../../app/context';
import { HelpLink } from '../help/HelpLink';
import type {
  BackendId,
  BatchItemStatusView,
  BatchPauseView,
  ConfirmMethod,
  FormAnswerView,
  FormContext,
  InteractionView,
  NativeForm,
  Notice,
  NoticeAnswer,
  PasswordStorage,
  SecretPrompt,
  SigningProgressView,
  SigningStage,
  StepView,
  UiEvent,
  ViewState,
} from '../../core/types';
import { isMobile } from '../../core/types';
import { Badge, Button, CheckboxField, Details, Progress, RadioCards, Spinner, TextInput } from '../../ui/controls';
import { DialogHeader, ModalShell } from '../../ui/Dialog';
import { IconAlert, IconAlertCircle, IconCheck, IconClock, IconInfo, IconMessage, IconPhone, IconQr, IconUsbKey } from '../../ui/Icons';
import { abortTextMsg, batchFlow, confirmsEach, newSessionMsg } from '../batch/flow';
import { statusLabel, statusTone } from '../batch/texts';
import { DEFAULT_STORAGE_CHOICE, PasswordStorageQuestion, PrivacyNote } from '../settings';
import { isShownBackend, SignerGlyph, SignerIcon, signerChoice, signerClass, signerName } from '../workspace/signers';
import { CountdownRing, useCountdown } from './CountdownRing';
import { friendlyError, noticeTexts, problemText, progressText, qrDataUrl } from './texts';
import './signing.css';

/** Welcher Teil der Karte gerade gilt (aus dem Schritt des Kerns); `pause` = Stapelpause (§3.8). */
export type CardMode = 'interaction' | 'choose' | 'error' | 'finishing' | 'pause';

/** Karte gehört zu einem Stapellauf („Abbrechen“ heißt dort „Stapel abbrechen“). */
const BatchCard = createContext(false);

/** Status der Einträge eines laufenden Stapels (die Liste der Karte zeigt nur diese). */
const RUN_STATES: readonly BatchItemStatusView['state'][] = ['queued', 'active', 'signed', 'failed', 'skipped', 'cancelled'];

/** Gehört ein Fehler in die Karte (Herkunft Signieren bzw. Dokument in der Phase `signing`)? */
export function errorInCard(view: ViewState): boolean {
  return view.step.step === 'error' && (view.step.context.origin === 'signing' || view.document?.phase === 'signing');
}

/** Teil der Karte für den aktuellen Zustand; `null` = keine Karte. */
export function cardMode(view: ViewState): CardMode | null {
  const s = view.step;
  if (s.step === 'signing') return s.progress.batch && s.progress.pause ? 'pause' : 'interaction';
  if (s.step === 'backend') return 'choose';
  if (errorInCard(view)) return 'error';
  if (s.step === 'output' && !s.result.path && !s.result.save_pending && !s.result.ask_overwrite) return 'finishing';
  return null;
}

/** Phase aus der Maske (Kern ohne Fortschritt, z. B. K0) */
function stageFromInteraction(i: InteractionView): SigningStage {
  if (i.kind === 'waiting') return i.progress === 'Signing' || i.progress === 'Done' ? 'finishing' : i.progress === 'ReadingCertificate' ? 'certificate' : 'connecting';
  if (i.kind === 'form') return 'Login' in i.form ? 'login' : 'confirming';
  if (i.kind === 'secret') return 'confirming';
  return 'confirming';
}

function methodFromInteraction(i: InteractionView): ConfirmMethod | null {
  if (i.kind === 'form') {
    const f = i.form;
    if ('SmsTan' in f) return 'sms';
    if ('Fido2' in f) return 'fido2';
    if ('QrCode' in f) return 'qr';
    if ('WaitForApp' in f) return 'app';
    return null;
  }
  if (i.kind === 'secret') return i.prompt.kind === 'Fido2Pin' ? 'fido2' : 'password';
  return null;
}

type StepKey = 'login' | 'confirm' | 'unlock' | 'card' | 'done';

/** `noConfirm`: ID Austria ohne zweiten Faktor (z. B. Test-Identitäten) – nach der Anmeldung gleich signiert. */
function stepsFor(backend: BackendId | null, noConfirm = false): StepKey[] {
  if (backend === 'keystore') return ['unlock', 'done'];
  if (backend === 'local_bku') return ['card', 'done'];
  return noConfirm ? ['login', 'done'] : ['login', 'confirm', 'done'];
}

function stepIndex(backend: BackendId | null, stage: SigningStage, loginDone: boolean): number {
  const last = stepsFor(backend).length - 1;
  if (stage === 'finishing' || stage === 'saving') return last;
  if (backend === 'keystore' || backend === 'local_bku') return 0;
  if (stage === 'connecting' || stage === 'login') return 0;
  if (stage === 'confirming') return 1;
  return loginDone ? 1 : 0;
}

function stepLabel(key: StepKey, method: ConfirmMethod | null): string {
  switch (key) {
    case 'login':
      return 'ng-ui-sc-step-login';
    case 'confirm':
      return method === 'sms' ? 'ng-ui-sc-step-sms' : method === 'fido2' ? 'ng-ui-sc-step-fido2' : 'ng-ui-sc-step-app';
    case 'unlock':
      return 'ng-ui-sc-step-unlock';
    case 'card':
      return 'ng-ui-sc-step-card';
    case 'done':
      return 'ng-ui-sc-step-done';
  }
}

interface Memo {
  backend: BackendId | null;
  docName: string;
  stage: SigningStage;
  method: ConfirmMethod | null;
  loginDone: boolean;
}

/** Statuskarte; rendert nichts, wenn gerade nicht signiert wird. */
export function SigningCard() {
  const { view } = useApp();
  const mode = cardMode(view);
  const memo = useRef<Memo | null>(null);
  if (!mode) {
    memo.current = null;
    return null;
  }
  const step = view.step;
  const progress: SigningProgressView | null = step.step === 'signing' && step.progress.backend ? step.progress : null;
  const prev = memo.current;
  const docName = progress?.doc?.name ?? view.document?.doc.name ?? (step.step === 'error' ? step.context.doc?.name : null) ?? prev?.docName ?? '';
  // Rückfall „Signiermittel wählen“ ohne Auswahl (nur ein Signiermittel, Nachtrag 20): das eine, vom Kern gewählte
  const backend: BackendId | null =
    mode === 'choose'
      ? signerChoice(view)
        ? null
        : view.signers.selected
      : (progress?.backend ?? (step.step === 'error' ? step.context.backend : null) ?? prev?.backend ?? view.signers.selected ?? 'atrust_native');
  let stage: SigningStage = prev?.stage ?? 'connecting';
  let method: ConfirmMethod | null = prev?.method ?? null;
  let loginDone = prev?.loginDone ?? false;
  if (step.step === 'signing') {
    stage = progress ? progress.stage : stageFromInteraction(step.interaction);
    // eine erneute Login-Maske setzt den Stepper sichtbar zurück (A-Trust hat das Passwort verworfen)
    if (!progress && step.interaction.kind === 'form' && 'Login' in step.interaction.form) loginDone = false;
    else if (!progress && stage === 'confirming') loginDone = true;
    if (progress) loginDone = progress.login_done;
    method = progress?.method ?? methodFromInteraction(step.interaction) ?? method;
  } else if (mode === 'finishing') {
    stage = 'saving';
  }
  memo.current = { backend, docName, stage, method, loginDone };
  const batch = progress?.batch ? progress : null;
  return <SigningDialog mode={mode} backend={backend} docName={docName} stage={stage} method={method} loginDone={loginDone} batch={batch} />;
}

function SigningDialog({
  mode,
  backend,
  docName,
  stage,
  method,
  loginDone,
  batch,
}: {
  mode: CardMode;
  backend: BackendId | null;
  docName: string;
  stage: SigningStage;
  method: ConfirmMethod | null;
  loginDone: boolean;
  /** Fortschritt eines Stapellaufs (`progress.batch`), sonst `null` */
  batch: SigningProgressView | null;
}) {
  const { t, view, dispatch } = useApp();
  const step = view.step;
  const bodyRef = useRef<HTMLDivElement>(null);
  const [askAbort, setAskAbort] = useState(false);
  const [skipHint, setSkipHint] = useState(false);
  // Der Hinweis gilt der gerade übersprungenen Anfrage: er bleibt beim nächsten Dokument stehen und verschwindet
  // mit dem übernächsten (sonst stand er bis zum Ende des Stapels)
  const batchDoc = batch?.doc?.id ?? null;
  const hintDocs = useRef<{ skipped: typeof batchDoc; next: typeof batchDoc } | null>(null);
  useEffect(() => {
    const h = hintDocs.current;
    if (!h || batchDoc === h.skipped) return;
    if (h.next === null) h.next = batchDoc;
    else if (batchDoc !== h.next) {
      hintDocs.current = null;
      setSkipHint(false);
    }
  }, [batchDoc]);
  const saving = stage === 'finishing' || stage === 'saving';
  const finishing = mode === 'finishing' || saving;
  // Stapel: Esc/× fragen nach, „Stapel abbrechen“ in der Karte bricht sofort ab (`abort_batch`, F-72)
  const abort = () => void dispatch({ type: 'abort_batch' });
  const cancel = () => {
    if (batch) {
      setAskAbort((v) => !v);
      return;
    }
    if (finishing) return;
    void dispatch({ type: 'cancel' });
  };
  const skip = () => {
    // eine schon angezeigte Anfrage bleibt in der App sichtbar, bis sie dort abläuft (§1.2)
    if (stage === 'confirming' && (method === 'app' || method === 'qr')) {
      hintDocs.current = { skipped: batchDoc, next: null };
      setSkipHint(true);
    }
    setAskAbort(false);
    void dispatch({ type: 'skip_batch_document' });
  };

  const interaction = step.step === 'signing' ? step.interaction : null;
  const paneKey = `${mode}:${askAbort ? 'abort' : ''}:${batch?.doc?.id ?? ''}:${interaction ? `${interaction.kind}:${'request' in interaction ? interaction.request : ''}:${interaction.kind === 'waiting' ? String(typeof interaction.progress === 'string' ? interaction.progress : 'doc') : ''}` : ''}`;
  // Fokus beim Schrittwechsel auf das erste Eingabefeld bzw. die Überschrift des Schritts
  useEffect(() => {
    const el = bodyRef.current;
    if (!el) return;
    const target = el.querySelector<HTMLElement>('[data-autofocus] input, input[data-autofocus]') ?? el.querySelector<HTMLElement>('.step-title');
    target?.focus({ preventScroll: true });
  }, [paneKey]);

  // ohne Signiermittel-Auswahl (Nachtrag 20) kein „Signiermittel wählen“, auch nicht im Rückfall
  const single = mode === 'choose' && !signerChoice(view);
  const title =
    mode === 'choose' && !single
      ? t.t('ng-ui-sc-choose-title')
      : backend
        ? t.t('ng-ui-sc-title', { signer: signerName(t, backend) })
        : t.t('ng-ui-insp-sign');
  // Einzeldokument fertig, ohne dass je eine Bestätigung (App, SMS, QR, FIDO2) verlangt wurde: kein abgehakter Schritt
  // „In der App bestätigen“, den es nicht gab
  const noConfirm = !batch && finishing && backend === 'atrust_native' && method === null;
  const steps = stepsFor(backend, noConfirm);
  const errorNow = mode === 'error' || mode === 'pause';
  // Stapel: „Signiert“ erst am Ende des Laufs; zwischen den Dokumenten bleibt der Schritt „Bestätigen“ aktiv
  const index = batch ? (saving ? Math.min(1, steps.length - 2) : stepIndex(backend, stage, loginDone)) : finishing ? steps.length - 1 : stepIndex(backend, stage, loginDone);
  const leaves = step.step === 'signing' && step.document_leaves_device;
  // Stapel: Untertitel „3 Dokumente“, Dokument und Zähler im Kopf der Stapelkarte (§2.5, §4.4)
  const subtitle = batch ? t.t('ng-ui-batch-docs', { n: batch.total }) : docName || undefined;

  return (
    <ModalShell onClose={cancel} isDismissable={false} isKeyboardDismissDisabled={finishing && !batch} className={batch ? 'signing-modal signing-modal-batch' : 'signing-modal'} size="md">
      <div className={`signing-card${finishing && !batch ? ' is-busy' : ''}`} data-testid="signing-card" data-mode={mode} data-batch={batch ? '' : undefined}>
        <DialogHeader title={title} subtitle={subtitle} onClose={finishing && !batch ? undefined : cancel} icon={<SignerGlyph backend={backend} />} />
        {mode !== 'choose' ? (
          <ol className="stepper" aria-label={t.t('ng-ui-sc-steps')}>
            {steps.map((k, i) => {
              const allDone = mode === 'finishing';
              const isDone = i < index || allDone;
              const isCurrent = i === index && !allDone;
              // Fehler nicht nur über die Farbe zeigen: Ausrufezeichen statt Nummer und eigener Screenreader-Text
              const isError = errorNow && isCurrent;
              return (
                <li key={k} className={`step${isDone ? ' is-done' : ''}${isCurrent ? ' is-current' : ''}${isError ? ' is-error' : ''}`} aria-current={isCurrent ? 'step' : undefined}>
                  <span className="step-dot" aria-hidden="true">
                    {isDone ? <IconCheck size={14} /> : isError ? '!' : i + 1}
                  </span>
                  <span className="step-label">{t.t(stepLabel(k, method))}</span>
                  <span className="sr-only">
                    , {t.t(isDone ? 'ng-ui-sc-state-done' : isError ? 'ng-ui-sc-state-error' : isCurrent ? 'ng-ui-sc-state-current' : 'ng-ui-sc-state-todo')}
                  </span>
                </li>
              );
            })}
          </ol>
        ) : null}
        <div className="signing-body" ref={bodyRef}>
          <BatchCard.Provider value={!!batch}>
            {batch ? <BatchStrip progress={batch} onSkip={skip} canSkip={mode === 'interaction' && !askAbort && !saving} skipHint={skipHint} /> : null}
            {askAbort && batch ? (
              <AbortPane key="abort" onKeep={() => setAskAbort(false)} onAbort={abort} backend={batch?.backend ?? backend} />
            ) : (
              <>
                {mode === 'interaction' && interaction ? <InteractionPane interaction={interaction} backend={backend} onCancel={batch ? abort : cancel} /> : null}
                {mode === 'pause' && batch?.pause ? <PausePane pause={batch.pause} onAbort={abort} onSkip={skip} backend={batch.backend ?? backend} /> : null}
                {mode === 'choose' && step.step === 'backend' ? (
                  single ? <SinglePane step={step} backend={backend} onCancel={cancel} /> : <ChoosePane step={step} onCancel={cancel} />
                ) : null}
                {mode === 'error' && step.step === 'error' ? <ErrorPane step={step} beforeConfirm={index === 0} /> : null}
                {mode === 'finishing' ? <DonePane /> : null}
              </>
            )}
          </BatchCard.Provider>
          {leaves && !finishing ? (
            <div className="sc-privacy">
              <PrivacyNote count={batch?.total ?? 1} />
            </div>
          ) : null}
        </div>
      </div>
    </ModalShell>
  );
}

/**
 * Kopf der Stapelkarte: Dokument bzw. Bestätigung k von n, Zähler, Liste der Dokumente des Laufs,
 * „Dokument überspringen“ und der Hinweis auf eine offene Anfrage in der App nach dem Überspringen.
 */
function BatchStrip({ progress, onSkip, canSkip, skipHint }: { progress: SigningProgressView; onSkip: () => void; canSkip: boolean; skipHint: boolean }) {
  const { t, view } = useApp();
  const name = progress.doc?.name ?? '';
  const done = progress.confirmations_done;
  const total = progress.total;
  const items = (view.batch?.items ?? []).filter((i) => RUN_STATES.includes(i.status.state));
  // „Bestätigung k von n“ und „Bestätigungen“ nur mit einer Bestätigung je Dokument (Kern: `batch_flow`); beim
  // Keystore (einmal entsperrt) „Dokument k von n“ und „Signiert“ (Mobile-Check 16.10.2026)
  const each = confirmsEach(batchFlow(view, progress.backend));
  const head = progress.stage === 'confirming' && each ? 'ng-ui-sc-batch-confirm' : 'ng-ui-sc-batch-doc';
  const count = each ? 'ng-ui-sc-batch-count' : 'ng-ui-sc-batch-count-signed';
  return (
    <div className="sc-batch" data-testid="batch-strip">
      <p className="sc-batch-doc" role="status">
        {name ? t.t(head, { index: progress.index, total, name }) : null}
      </p>
      <Progress className="sc-batch-progress" label={t.t(count)} value={total ? (done / total) * 100 : 0} valueLabel={t.t('ng-ui-sc-batch-count-value', { done, total })} />
      <div className="sc-batch-foot">
        {items.length > 0 ? (
          <Details label={t.t('ng-ui-sc-batch-list')} level={3}>
            <ul className="sc-batch-list">
              {items.map((i) => (
                <li key={i.doc} className="sc-batch-item" aria-current={i.doc === progress.doc?.id ? 'step' : undefined}>
                  <span className="sc-batch-name">{i.name}</span>
                  <Badge tone={statusTone(i.status)}>{statusLabel(t, i.status)}</Badge>
                </li>
              ))}
            </ul>
          </Details>
        ) : (
          <span />
        )}
        {canSkip && progress.doc ? (
          <Button variant="secondary" size="sm" className="sc-batch-skip" onPress={onSkip}>
            {t.t('ng-ui-sc-batch-skip')}
          </Button>
        ) : null}
      </div>
      {skipHint ? (
        <p className="callout callout-info sc-batch-hint" role="status">
          <IconInfo size={18} />
          <span>{t.code('ng-batch-pending-app-request')}</span>
        </p>
      ) : null}
    </div>
  );
}

/** Rückfrage vor dem Abbruch des Stapels (Esc/×): Gespeichertes bleibt, das Passwort wird vergessen. */
function AbortPane({ onKeep, onAbort, backend }: { onKeep: () => void; onAbort: () => void; backend: BackendId | null }) {
  const { t, view } = useApp();
  return (
    <div className="step-pane">
      <span className="step-hero step-hero-info" aria-hidden="true">
        <IconAlert size={28} />
      </span>
      <div role="alert">
        <StepTitle>{t.t('ng-ui-sc-batch-abort-title')}</StepTitle>
        <p className="step-text">{t.t(abortTextMsg(batchFlow(view, backend)))}</p>
      </div>
      <div className="step-actions">
        <Button variant="ghost" onPress={onKeep}>
          {t.t('ng-ui-sc-batch-abort-keep')}
        </Button>
        <Button variant="danger" onPress={onAbort}>
          {t.t('ng-ui-sc-batch-abort')}
        </Button>
      </div>
    </div>
  );
}

/**
 * Stapelpause (§3.8): Ablehnung bzw. wiederholbarer Dokumentfehler (Dokument), Verbindungsfehler (Sitzung
 * bleibt) oder Sitzungsfehler (neue Sitzung, ggf. neue Anmeldung). Nie automatisch weiter.
 */
function PausePane({ pause, onAbort, onSkip, backend }: { pause: BatchPauseView; onAbort: () => void; onSkip: () => void; backend: BackendId | null }) {
  const { t, dispatch, view } = useApp();
  const friendly = friendlyError(t, pause.code);
  const generic = friendly.title === t.t('ng-ui-sc-error-title');
  return (
    <div className="step-pane" data-testid="batch-pause" data-scope={pause.scope}>
      <span className="step-hero step-hero-danger" aria-hidden="true">
        <IconAlertCircle size={28} />
      </span>
      <div role="alert" className="step-prose">
        <StepTitle>{generic ? t.t('ng-ui-sc-batch-pause-title') : friendly.title}</StepTitle>
        <p className="step-text pre-line">{friendly.text ?? t.code(pause.code, pause.params)}</p>
        {/* ohne eigene Überschrift steht „Der Stapel ist angehalten“ schon im Titel */}
        <p className="step-text">{generic ? t.t('ng-ui-sc-batch-pause-decide') : t.code('ng-batch-paused')}</p>
        {pause.new_session ? <p className="step-footnote">{t.t(newSessionMsg(batchFlow(view, backend)))}</p> : null}
      </div>
      <Details label={t.t('ng-ui-sc-details')} level={4}>
        <p className="mono-small">{[pause.code, pause.doc?.name].filter(Boolean).join(' · ')}</p>
        {pause.params.detail ? <p className="mono-small">{pause.params.detail}</p> : null}
      </Details>
      <div className="step-actions">
        <Button variant="ghost" onPress={onAbort}>
          {t.t('ng-ui-sc-batch-abort')}
        </Button>
        {pause.doc ? (
          <Button variant="secondary" onPress={onSkip}>
            {t.t('ng-ui-sc-batch-pause-skip')}
          </Button>
        ) : null}
        {pause.can_retry ? (
          <Button variant="primary" onPress={() => void dispatch({ type: 'retry' })}>
            {friendly.action ?? t.t('ng-ui-ws-retry')}
          </Button>
        ) : null}
      </div>
    </div>
  );
}

/* ------------------------------------------------------------------ Bausteine */

function StepTitle({ children }: { children: ReactNode }) {
  return (
    <h3 className="step-title" tabIndex={-1}>
      {children}
    </h3>
  );
}

function ReferenceValue({ value, size = 'lg', onShowData }: { value: string; size?: 'lg' | 'sm'; onShowData?: () => void }) {
  const { t } = useApp();
  // Ein Kästchen je Zeichen, Leerzeichen der A-Trust-Gliederung nicht als leeres Kästchen (der Kern liefert
  // den Wert bereits kompakt; das schützt auch ältere Kernstände)
  const chars = Array.from(value).filter((c) => c.trim() !== '');
  return (
    <div className={`refvalue refvalue-${size}`}>
      <span className="refvalue-label">{t.t('ng-ui-sc-ref')}</span>
      <span className="refvalue-chars" aria-hidden="true" style={{ '--n': chars.length } as CSSProperties}>
        {chars.map((c, i) => (
          <span key={i} className="refvalue-char">
            {c}
          </span>
        ))}
      </span>
      <span className="sr-only">{chars.join(' ')}</span>
      {onShowData ? (
        <AriaButton className="link refvalue-data" onPress={onShowData}>
          {t.t('ng-ui-sc-data')}
        </AriaButton>
      ) : null}
    </div>
  );
}

/** Problemzeile des Kerns bzw. der Vorabprüfung als dauerhafte `alert`-Region. */
function ProblemLine({ ctx, local }: { ctx: FormContext | null; local?: string | null }) {
  const { t } = useApp();
  const text = local ?? (ctx?.problem ? problemText(t, ctx.problem) : null);
  return (
    <div className="sc-problem" role="alert">
      {text ? (
        <p className="message message-error">
          <IconAlertCircle size={16} />
          <span>{text}</span>
        </p>
      ) : null}
    </div>
  );
}

/**
 * Doppelklickschutz wie PDF-Over (der Knopf sperrt nach dem Klick). Lehnt die Bridge das Ereignis ab
 * (Hinweis erscheint), wird die Maske wieder freigegeben.
 */
function useSubmit(request: number) {
  const { dispatch } = useApp();
  const [sent, setSent] = useState(false);
  const send = (answer: FormAnswerView) => {
    setSent(true);
    void dispatch({ type: 'submit_form', request, answer }).then((ok) => {
      if (!ok) setSent(false);
    });
  };
  return { sent, send };
}

/**
 * „Stattdessen: FIDO2-Schlüssel · SMS-TAN“ nach `FormContext`. Mobile: bietet A-Trust FIDO2 an, gibt es auf
 * dem Gerät aber keinen Authenticator, entfällt der Link; es bleibt nur der Hinweis, dass FIDO2 hier nicht geht.
 */
function Alternatives({ ctx, send, sent, sms = true, fido2 = true, app }: { ctx: FormContext; send: (a: FormAnswerView) => void; sent: boolean; sms?: boolean; fido2?: boolean; app?: () => void }) {
  const { t, view } = useApp();
  const mobile = isMobile(view);
  const fido2Greyed = fido2 && !ctx.fido2_visible && ctx.can_switch_to_fido2 && mobile;
  const items: ReactNode[] = [];
  if (app) {
    items.push(
      <AriaButton key="app" className="link" onPress={app} isDisabled={sent}>
        <IconPhone size={15} />
        {t.t('ng-ui-sc-alt-app')}
      </AriaButton>,
    );
  }
  if (fido2 && ctx.fido2_visible && ctx.can_switch_to_fido2) {
    items.push(
      <AriaButton key="fido2" className="link" onPress={() => send({ kind: 'switch_to_fido2' })} isDisabled={sent}>
        <IconUsbKey size={15} />
        {t.t('ng-ui-sc-alt-fido2')}
      </AriaButton>,
    );
  }
  if (sms && ctx.can_switch_to_sms) {
    items.push(
      <AriaButton key="sms" className="link" onPress={() => send({ kind: 'switch_to_sms' })} isDisabled={sent}>
        <IconMessage size={15} />
        {t.t('ng-ui-sc-alt-sms')}
      </AriaButton>,
    );
  }
  if (items.length === 0 && !fido2Greyed) return null;
  return (
    <>
      {items.length > 0 ? (
        <div className="alt-row">
          <span>{t.t('ng-ui-sc-alternatives')}</span>
          {items.flatMap((item, i) => (i === 0 ? [item] : [<span key={`sep-${i}`} aria-hidden="true">·</span>, item]))}
        </div>
      ) : null}
      {fido2Greyed ? (
        <p className="step-footnote">
          {t.t('ng-fido2-unavailable-mobile')}
        </p>
      ) : null}
    </>
  );
}

function CancelButton({ onCancel, isDisabled }: { onCancel: () => void; isDisabled?: boolean }) {
  const { t } = useApp();
  const batch = useContext(BatchCard);
  return (
    <Button variant="ghost" onPress={onCancel} isDisabled={isDisabled}>
      {t.t(batch ? 'ng-ui-sc-batch-abort' : 'ng-ui-sc-cancel')}
    </Button>
  );
}

/* ------------------------------------------------------------------ Schritte */

function InteractionPane({ interaction, backend, onCancel }: { interaction: InteractionView; backend: BackendId | null; onCancel: () => void }) {
  switch (interaction.kind) {
    case 'waiting':
      if (backend === 'local_bku' && interaction.progress === 'Signing') return <CardPane onCancel={onCancel} />;
      if (interaction.progress === 'Signing' || interaction.progress === 'Done') return <BusyPane progress={interaction.progress} backend={backend} />;
      return <BusyPane progress={interaction.progress} backend={backend} onCancel={onCancel} />;
    case 'form':
      return <FormPane key={interaction.request} request={interaction.request} form={interaction.form} onCancel={onCancel} />;
    case 'secret':
      return (
        <SecretPane
          key={interaction.request}
          request={interaction.request}
          prompt={interaction.prompt}
          askStorage={interaction.ask_storage}
          keychainAvailable={interaction.keychain_available}
          onCancel={onCancel}
        />
      );
    case 'notice':
      return <NoticePane key={interaction.request} request={interaction.request} notice={interaction.notice} />;
  }
}

function BusyPane({ progress, backend, onCancel }: { progress: Extract<InteractionView, { kind: 'waiting' }>['progress']; backend: BackendId | null; onCancel?: () => void }) {
  const { t } = useApp();
  return (
    <div className="step-pane step-pane-center">
      <div className="busy" role="status">
        <Spinner size={36} className="step-spinner" />
        <p className="step-title" tabIndex={-1}>
          {progressText(t, progress, backend)}
        </p>
      </div>
      {onCancel ? (
        <div className="step-actions">
          <CancelButton onCancel={onCancel} />
        </div>
      ) : null}
    </div>
  );
}

function FormPane({ request, form, onCancel }: { request: number; form: NativeForm; onCancel: () => void }) {
  if ('Login' in form) return <LoginPane request={request} data={form.Login} onCancel={onCancel} />;
  if ('WaitForApp' in form) return <AppPane request={request} data={form.WaitForApp} onCancel={onCancel} />;
  if ('QrCode' in form) return <QrPane request={request} data={form.QrCode} onCancel={onCancel} />;
  if ('SmsTan' in form) return <SmsPane request={request} data={form.SmsTan} onCancel={onCancel} />;
  return <Fido2Pane request={request} data={form.Fido2} onCancel={onCancel} />;
}

/** „Signaturdaten anzeigen“ (F-68), nur wenn A-Trust den Link liefert. */
function useShowData(request: number, ctx: FormContext): (() => void) | undefined {
  const { dispatch } = useApp();
  if (!ctx.signature_data_url) return undefined;
  return () => void dispatch({ type: 'open_signature_data', request });
}

function LoginPane({ request, data, onCancel }: { request: number; data: Extract<NativeForm, { Login: unknown }>['Login']; onCancel: () => void }) {
  const { t } = useApp();
  const { sent, send } = useSubmit(request);
  const [user, setUser] = useState(data.prefill_username ?? '');
  const [pw, setPw] = useState('');
  const [remember, setRemember] = useState(data.remember);
  const [errUser, setErrUser] = useState<string | null>(null);
  const [errPw, setErrPw] = useState<string | null>(null);
  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (sent) return;
    // Vorabprüfung wie MobileBKUEnterNumberComposite (Passwort 5–200 Zeichen); der Kern prüft erneut.
    const u = user.trim();
    const eu = u ? null : t.t('ng-ui-username-required');
    const ep = pw.length < 5 ? t.t('exception-PasswordTooShort') : pw.length > 200 ? t.t('exception-PasswordTooLong') : null;
    setErrUser(eu);
    setErrPw(ep);
    if (eu || ep) return;
    send({ kind: 'login', username: u, password: pw, remember });
    setPw('');
  };
  return (
    <form className="step-pane" onSubmit={submit} noValidate>
      <StepTitle>{t.t('ng-ui-sc-step-login')}</StepTitle>
      <div data-autofocus={user ? undefined : true}>
        <TextInput
          label={t.t('ng-ui-sc-login-user')}
          value={user}
          onChange={(v) => {
            setUser(v);
            setErrUser(null);
          }}
          error={errUser}
          autoComplete="username"
          // Telefonnummer **oder** Benutzername: Standardtastatur (die Telefon-Tastatur von iOS hat keine Buchstaben)
          autoCapitalize="none"
          spellCheck={false}
        />
      </div>
      <div data-autofocus={user ? true : undefined}>
        <TextInput
          label={t.t('ng-ui-sc-login-password')}
          type="password"
          value={pw}
          onChange={(v) => {
            setPw(v);
            setErrPw(null);
          }}
          error={errPw}
          autoComplete="current-password"
        />
      </div>
      <CheckboxField label={t.t('ng-ui-sc-login-remember')} description={t.t('ng-ui-sc-login-remember-hint')} isSelected={remember} onChange={setRemember} />
      <ProblemLine ctx={data.ctx} />
      <p className="step-footnote">{t.t('ng-ui-sc-login-provider')}</p>
      <div className="step-actions">
        <CancelButton onCancel={onCancel} />
        <Button variant="primary" type="submit" isDisabled={sent}>
          {t.t('ng-ui-sc-login-submit')}
        </Button>
      </div>
    </form>
  );
}

/**
 * Uhr je Vorgang bei A-Trust: der Vergleichswert. Nach der Rückkehr in die App lädt der Kern die Seite neu
 * (Long-Poll neu starten) – das ist eine neue Anfrage mit neuer Nummer, aber derselbe Vorgang mit demselben
 * Vergleichswert; die Uhr läuft weiter. Ohne Vergleichswert gilt die Nummer der Anfrage.
 */
export function countdownKey(request: number, reference: string | null): string {
  return reference ? `ref:${reference}` : `request:${request}`;
}

/** Restzeit mit Countdown-Ring (ohne `timeout_secs` kein Ring; neue Anfrage = neuer `key`, neue Uhr). */
function Countdown({ request, reference, total }: { request: number; reference: string | null; total: number }) {
  const remaining = useCountdown(total, 250, () => undefined, countdownKey(request, reference));
  return <CountdownRing remaining={remaining} total={total} />;
}

function LowTimeNotice({ request, reference, total }: { request: number; reference: string | null; total: number }) {
  const { t } = useApp();
  const remaining = useCountdown(total, 1000, () => undefined, countdownKey(request, reference));
  // Screenreader: einmalige Vorwarnung kurz vor Ablauf (der Ring selbst wird nicht laufend angesagt)
  const low = total > 60 && remaining > 0 && remaining <= 60;
  return (
    <span className="sr-only" role="status">
      {low ? t.t('ng-ui-sc-app-low-time') : ''}
    </span>
  );
}

function AppPane({ request, data, onCancel }: { request: number; data: Extract<NativeForm, { WaitForApp: unknown }>['WaitForApp']; onCancel: () => void }) {
  const { t, view } = useApp();
  const { sent, send } = useSubmit(request);
  const showData = useShowData(request, data.ctx);
  const mobile = isMobile(view);
  return (
    <div className="step-pane">
      <div className="app-status">
        <span className="app-phone" aria-hidden="true">
          <IconPhone size={26} />
          <span className="app-ping" />
        </span>
        <div>
          <StepTitle>{t.t('ng-ui-sc-app-sent')}</StepTitle>
          <p className="step-text">{t.t(mobile ? 'ng-ui-sc-app-instruction-mobile' : 'ng-ui-sc-app-instruction')}</p>
        </div>
      </div>
      {data.reference_value || data.timeout_secs ? (
        <div className="app-confirm">
          {data.reference_value ? <ReferenceValue value={data.reference_value} onShowData={showData} /> : <span />}
          {data.timeout_secs ? <Countdown key={request} request={request} reference={data.reference_value} total={data.timeout_secs} /> : null}
        </div>
      ) : null}
      {data.timeout_secs ? <LowTimeNotice key={`low-${request}`} request={request} reference={data.reference_value} total={data.timeout_secs} /> : null}
      <p className="step-hint">
        <IconClock size={15} />
        {t.t('ng-ui-sc-ref-hint')} <span className="waiting-dots">{t.t('ng-ui-sc-app-waiting')}</span>
      </p>
      <ProblemLine ctx={data.ctx} />
      <Alternatives ctx={data.ctx} send={send} sent={sent} />
      <div className="step-actions">
        <CancelButton onCancel={onCancel} />
        <Button variant="secondary" isDisabled={sent} onPress={() => send({ kind: 'continue' })}>
          {t.t('ng-ui-sc-app-confirmed')}
        </Button>
      </div>
    </div>
  );
}

function QrPane({ request, data, onCancel }: { request: number; data: Extract<NativeForm, { QrCode: unknown }>['QrCode']; onCancel: () => void }) {
  const { t } = useApp();
  const { sent, send } = useSubmit(request);
  const showData = useShowData(request, data.ctx);
  return (
    <div className="step-pane">
      <div className="app-status">
        <span className="app-phone" aria-hidden="true">
          <IconQr size={26} />
        </span>
        <div>
          <StepTitle>{t.t('ng-ui-sc-qr-title')}</StepTitle>
          <p className="step-text">{t.t('tanEnter-QR')}</p>
        </div>
      </div>
      <div className="qr-box">
        {data.image ? <img className="qr-image" src={qrDataUrl(data.image)} alt={t.t('ng-ui-qr-alt')} width={180} height={180} /> : null}
        <div className="qr-side">
          <ReferenceValue value={data.reference_value} size="sm" onShowData={showData} />
          {data.timeout_secs ? <Countdown key={request} request={request} reference={data.reference_value} total={data.timeout_secs} /> : null}
        </div>
      </div>
      {data.timeout_secs ? <LowTimeNotice key={`low-${request}`} request={request} reference={data.reference_value} total={data.timeout_secs} /> : null}
      <ProblemLine ctx={data.ctx} local={data.image ? null : t.t('error-FailedToLoadQRCode')} />
      <Alternatives ctx={data.ctx} send={send} sent={sent} />
      <div className="step-actions">
        <CancelButton onCancel={onCancel} />
        <Button variant="primary" isDisabled={sent} onPress={() => send({ kind: 'continue' })}>
          {t.t('ng-ui-sc-qr-continue')}
        </Button>
      </div>
    </div>
  );
}

function SmsPane({ request, data, onCancel }: { request: number; data: Extract<NativeForm, { SmsTan: unknown }>['SmsTan']; onCancel: () => void }) {
  const { t } = useApp();
  const { sent, send } = useSubmit(request);
  const [tan, setTan] = useState('');
  const [err, setErr] = useState<string | null>(null);
  const trimmed = tan.trim();
  // Live-Warnung wie PDF-Over ab mehr als 3 Zeichen
  const warn = trimmed.length > 3 && data.reference_value.startsWith(trimmed);
  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (sent) return;
    if (trimmed.length === 0) return setErr(t.t('error-NoTan'));
    if (data.reference_value.startsWith(trimmed)) return setErr(t.t('ng-ui-sc-sms-err-ref'));
    if (trimmed.length > 6) return setErr(t.t('error-TanTooLong'));
    setErr(null);
    send({ kind: 'tan', tan: trimmed });
    setTan('');
  };
  return (
    <form className="step-pane" onSubmit={submit} noValidate>
      <div className="app-status">
        <span className="app-phone" aria-hidden="true">
          <IconMessage size={26} />
        </span>
        <div>
          <StepTitle>{t.t('ng-ui-sc-step-sms')}</StepTitle>
          <p className="step-text">{t.t('ng-ui-sc-sms-text')}</p>
        </div>
      </div>
      <ReferenceValue value={data.reference_value} size="sm" />
      <div data-autofocus>
        <TextInput
          label={t.t('ng-ui-sc-sms-tan')}
          value={tan}
          onChange={(v) => {
            setTan(v);
            setErr(null);
          }}
          error={err}
          inputMode="numeric"
          autoComplete="one-time-code"
          inputClassName="input-tan"
          spellCheck={false}
          description={warn ? t.t('ng-ui-sc-sms-err-ref') : undefined}
        />
      </div>
      <ProblemLine ctx={data.ctx} />
      <Alternatives ctx={data.ctx} send={send} sent={sent} sms={false} />
      <div className="step-actions">
        <CancelButton onCancel={onCancel} />
        <Button variant="primary" type="submit" isDisabled={sent}>
          {t.t('ng-ui-sc-sms-submit')}
        </Button>
      </div>
    </form>
  );
}

function Fido2Pane({ request, data, onCancel }: { request: number; data: Extract<NativeForm, { Fido2: unknown }>['Fido2']; onCancel: () => void }) {
  const { t } = useApp();
  const { sent, send } = useSubmit(request);
  if (data.in_progress) {
    return (
      <div className="step-pane step-pane-center">
        <span className="fido-key" aria-hidden="true">
          <IconUsbKey size={40} />
          <span className="fido-pulse" />
        </span>
        <div role="status">
          <StepTitle>{t.t('ng-ui-sc-fido2-touch')}</StepTitle>
          <p className="step-text">{t.t('ng-ui-sc-fido2-touch-hint')}</p>
        </div>
        <ProblemLine ctx={data.ctx} />
        <div className="step-actions">
          <CancelButton onCancel={onCancel} />
        </div>
      </div>
    );
  }
  return (
    <div className="step-pane">
      <div className="app-status">
        <span className="app-phone" aria-hidden="true">
          <IconUsbKey size={26} />
        </span>
        <div>
          <StepTitle>{t.t('ng-ui-sc-fido2-title')}</StepTitle>
          <p className="step-text">{t.t('ng-ui-sc-fido2-text')}</p>
        </div>
      </div>
      <ProblemLine ctx={data.ctx} />
      <Alternatives ctx={data.ctx} send={send} sent={sent} fido2={false} />
      <div className="step-actions">
        <CancelButton onCancel={onCancel} />
        <Button variant="primary" isDisabled={sent} onPress={() => send({ kind: 'start_fido2' })}>
          {t.t('ng-ui-sc-fido2-start')}
        </Button>
      </div>
    </div>
  );
}

const SECRET_TEXT: Record<SecretPrompt['kind'], { title: string; label: string; retry: string; submit: string }> = {
  KeystorePassword: { title: 'ng-ui-sc-step-unlock', label: 'ng-ui-sc-ks-password', retry: 'error-KeyStoreStorePass', submit: 'ng-ui-sc-ks-submit' },
  KeyPassword: { title: 'ng-ui-sc-step-unlock', label: 'ng-ui-sc-ks-key-password', retry: 'error-KeyStoreKeyPass', submit: 'ng-ui-sc-ks-submit' },
  ProxyPassword: { title: 'ng-ui-sc-proxy-title', label: 'ng-ui-sc-proxy-password', retry: 'ng-ui-proxy-password-retry', submit: 'ng-ui-sc-continue' },
  Fido2Pin: { title: 'ng-ui-sc-fido2-title', label: 'ng-ui-sc-fido2-pin', retry: 'ng-ui-sc-fido2-pin-retry', submit: 'ng-ui-sc-continue' },
};

/**
 * Passwortabfrage. Erste Verwendung des Keystores (`ask_storage`): unter dem Feld „Passwort merken?“ (kein
 * eigener Dialog); die Wahl geht mit dem Absenden als `password_storage` an den Kern.
 */
function SecretPane({
  request,
  prompt,
  askStorage,
  keychainAvailable,
  onCancel,
}: {
  request: number;
  prompt: SecretPrompt;
  askStorage: boolean;
  keychainAvailable: boolean;
  onCancel: () => void;
}) {
  const { t, dispatch } = useApp();
  const [secret, setSecret] = useState('');
  const [storage, setStorage] = useState<PasswordStorage>(DEFAULT_STORAGE_CHOICE);
  const [sent, setSent] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const texts = SECRET_TEXT[prompt.kind];
  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (sent) return;
    if (!secret) return setErr(t.t('ng-ui-sc-secret-empty'));
    setSent(true);
    const event: UiEvent = askStorage ? { type: 'submit_secret', request, secret, password_storage: storage } : { type: 'submit_secret', request, secret };
    void dispatch(event).then((ok) => {
      if (!ok) setSent(false);
    });
    setSecret('');
  };
  const text =
    prompt.kind === 'Fido2Pin'
      ? t.t('ng-ui-sc-fido2-pin-text')
      : prompt.kind === 'ProxyPassword'
        ? t.t('ng-ui-sc-proxy-text', { host: prompt.subject })
        : t.t('ng-ui-sc-ks-text', { file: prompt.subject });
  return (
    <form className="step-pane" onSubmit={submit} noValidate>
      <StepTitle>{t.t(texts.title)}</StepTitle>
      <p className="step-text">{text}</p>
      {prompt.retry ? (
        <p className="message message-error" role="alert">
          {t.t(texts.retry)}
        </p>
      ) : null}
      <div data-autofocus>
        <TextInput
          label={t.t(texts.label)}
          type="password"
          value={secret}
          onChange={(v) => {
            setSecret(v);
            setErr(null);
          }}
          error={err}
          autoComplete="off"
          inputMode={prompt.kind === 'Fido2Pin' ? 'numeric' : undefined}
        />
      </div>
      {askStorage ? <PasswordStorageQuestion value={storage} onChange={setStorage} keychainAvailable={keychainAvailable} /> : null}
      <div className="step-actions">
        <CancelButton onCancel={onCancel} />
        <Button variant="primary" type="submit" isDisabled={sent}>
          {t.t(texts.submit)}
        </Button>
      </div>
    </form>
  );
}

function CardPane({ onCancel }: { onCancel: () => void }) {
  const { t } = useApp();
  return (
    <div className="step-pane">
      <StepTitle>{t.t('ng-ui-sc-step-card')}</StepTitle>
      <p className="step-text">{t.t('ng-ui-sc-card-text')}</p>
      <p className="step-hint" role="status">
        <Spinner size={16} />
        {t.t('ng-ui-sc-card-waiting')}
      </p>
      <div className="step-actions">
        <CancelButton onCancel={onCancel} />
      </div>
    </div>
  );
}

/** Meldung (A-Trust-Fehlerseite, Interstitial, HTTP) mit den Knöpfen der Meldung. */
function NoticePane({ request, notice }: { request: number; notice: Notice }) {
  const { t, dispatch } = useApp();
  const { title, text } = noticeTexts(t, notice);
  const answer = (a: NoticeAnswer) => void dispatch({ type: 'answer_notice', request, answer: a });
  const error = notice.kind === 'Error';
  return (
    <div className="step-pane">
      <span className={`step-hero ${error ? 'step-hero-danger' : 'step-hero-info'}`} aria-hidden="true">
        {error ? <IconAlertCircle size={28} /> : <IconInfo size={28} />}
      </span>
      <div role={error ? 'alert' : 'status'}>
        <StepTitle>{title}</StepTitle>
        <p className="step-text pre-line">{text}</p>
      </div>
      <div className="step-actions">
        {notice.buttons === 'Ok' ? (
          <Button variant="primary" onPress={() => answer('Ok')}>
            {t.t('common-Ok')}
          </Button>
        ) : notice.buttons === 'OkCancel' ? (
          <>
            <Button variant="ghost" onPress={() => answer('Cancel')}>
              {t.t('ng-ui-sc-cancel')}
            </Button>
            <Button variant="primary" onPress={() => answer('Ok')}>
              {t.t('common-Ok')}
            </Button>
          </>
        ) : (
          <>
            <Button variant="ghost" onPress={() => answer('Cancel')}>
              {t.t('ng-ui-sc-cancel')}
            </Button>
            <Button variant="primary" onPress={() => answer('Retry')}>
              {notice.code === 'AtrustError' ? t.t('ng-ui-sc-err-new-confirmation') : t.t('ng-ui-ws-retry')}
            </Button>
          </>
        )}
      </div>
    </div>
  );
}

/**
 * Rückfall „Signiermittel wählen“ (nur nach „Signatur fortsetzen“ mit nicht mehr verfügbarem Signiermittel). Die
 * Optionen kommen vom Kern (iOS/Android: nur ID Austria, ADR 0004 Nachtrag 11).
 */
function ChoosePane({ step, onCancel }: { step: Extract<StepView, { step: 'backend' }>; onCancel: () => void }) {
  const { t, dispatch } = useApp();
  const options = step.options.filter((o) => isShownBackend(o.id));
  const [choice, setChoice] = useState<BackendId | null>(options.find((o) => o.available)?.id ?? null);
  const selected = options.find((o) => o.id === choice);
  return (
    <div className="step-pane">
      <StepTitle>{t.t('ng-ui-sc-choose-title')}</StepTitle>
      <p className="step-text">{t.t('ng-ui-sc-choose-text')}</p>
      <RadioCards
        label={t.t('ng-ui-sc-choose-title')}
        hideLabel
        value={choice}
        onChange={setChoice}
        options={options.map((o) => ({
          value: o.id,
          label: signerName(t, o.id),
          description: o.available ? t.t(o.id === 'atrust_native' ? 'ng-ui-insp-signer-atrust-desc' : o.id === 'keystore' ? 'ng-ui-insp-signer-keystore-desc' : 'ng-ui-insp-signer-card-desc') : t.t('ng-ui-sc-choose-unavailable'),
          icon: (
            <span className={signerClass(o.id)}>
              <SignerIcon backend={o.id} size={18} />
            </span>
          ),
          isDisabled: !o.available,
        }))}
      />
      {selected?.capabilities?.document_leaves_device ? <PrivacyNote /> : null}
      <div className="step-actions">
        <CancelButton onCancel={onCancel} />
        <Button variant="primary" isDisabled={!choice} onPress={() => choice && void dispatch({ type: 'choose_backend', backend: choice })}>
          {t.t('ng-ui-sc-choose-sign')}
        </Button>
      </div>
    </div>
  );
}

/**
 * Rückfall ohne Signiermittel-Auswahl (nur ein Signiermittel – iOS/Android: ID Austria; Desktop per Richtlinie,
 * ADR 0004 Nachtrag 20): keine Liste zum Wählen. Das eine Signiermittel (vom Kern gewählt, `signers.selected`) mit
 * „Signieren“ (`choose_backend`, ausdrückliche Aktion) bzw. – nicht verfügbar – mit dem Grund des Kerns und nur
 * „Abbrechen“.
 */
function SinglePane({ step, backend, onCancel }: { step: Extract<StepView, { step: 'backend' }>; backend: BackendId | null; onCancel: () => void }) {
  const { t, view, dispatch } = useApp();
  const option = backend ? step.options.find((o) => o.id === backend) : undefined;
  const signer = backend ? view.signers.options.find((o) => o.backend === backend) : undefined;
  const available = option?.available === true;
  const name = signerName(t, backend);
  return (
    <div className="step-pane" data-testid="single-signer">
      <StepTitle>{backend ? t.t('ng-ui-sc-single-title', { signer: name }) : t.t('ng-ui-insp-sign')}</StepTitle>
      {available && backend ? (
        <p className="step-text">{t.t('ng-ui-sc-single-text', { signer: name })}</p>
      ) : (
        <div className="callout callout-warning" role="status">
          <IconAlert size={18} />
          <div>
            <p className="callout-title">{backend ? t.t('ng-ui-sc-single-unavailable', { signer: name }) : t.t('ng-ui-sc-choose-unavailable')}</p>
            {signer?.unavailable_reason ? <p>{t.code(signer.unavailable_reason)}</p> : null}
          </div>
        </div>
      )}
      {available && option?.capabilities?.document_leaves_device ? <PrivacyNote /> : null}
      <div className="step-actions">
        <CancelButton onCancel={onCancel} />
        {available && backend ? (
          <Button variant="primary" onPress={() => void dispatch({ type: 'choose_backend', backend })}>
            {t.t('ng-ui-sc-choose-sign')}
          </Button>
        ) : null}
      </div>
    </div>
  );
}

function formatTimestamp(iso: string | null, lang: 'de' | 'en'): string | null {
  if (!iso) return null;
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return new Intl.DateTimeFormat(lang === 'de' ? 'de-AT' : 'en-GB', { dateStyle: 'medium', timeStyle: 'medium' }).format(d);
}

/**
 * Fehlerschritt in der Karte: Alltagstext, eine Aktion, „Technische Details“ (Code, Zeitpunkt).
 * `beforeConfirm`: gescheitert vor der Bestätigung (Stepper auf „Anmelden“) – eine Zeitüberschreitung beim
 * Verbindungsaufbau bietet dann „Erneut versuchen“ statt „Neue Bestätigung anfordern“ an.
 */
function ErrorPane({ step, beforeConfirm }: { step: Extract<StepView, { step: 'error' }>; beforeConfirm: boolean }) {
  const { t, dispatch } = useApp();
  const friendly = friendlyError(t, step.code);
  const message = friendly.text ?? t.code(step.code, step.params);
  const at = formatTimestamp(step.context.occurred_at, t.lang);
  const declined = step.code === 'ng-signing-declined';
  const plainRetry = declined || (beforeConfirm && step.code === 'backend-timeout');
  return (
    <div className="step-pane">
      <span className="step-hero step-hero-danger" aria-hidden="true">
        <IconAlertCircle size={28} />
      </span>
      <div role="alert">
        <StepTitle>{friendly.title}</StepTitle>
        <p className="step-text pre-line">{message}</p>
        <HelpLink code={step.code} />
      </div>
      <Details label={t.t('ng-ui-sc-details')} level={4}>
        <p className="mono-small">
          {[step.code, step.context.backend ? signerName(t, step.context.backend) : null, at].filter(Boolean).join(' · ')}
        </p>
        {step.params.detail ? <p className="mono-small">{step.params.detail}</p> : null}
        <Badge tone="neutral">{step.code}</Badge>
      </Details>
      <div className="step-actions">
        <Button variant="ghost" onPress={() => void dispatch({ type: 'cancel' })}>
          {t.t('ng-ui-close')}
        </Button>
        {step.retry ? (
          <Button variant="primary" onPress={() => void dispatch({ type: 'retry' })}>
            {plainRetry ? t.t('ng-ui-ws-retry') : (friendly.action ?? t.t('ng-ui-ws-retry'))}
          </Button>
        ) : null}
      </div>
    </div>
  );
}

function DonePane() {
  const { t } = useApp();
  return (
    <div className="step-pane step-pane-center" role="status">
      <span className="done-check" aria-hidden="true">
        <IconCheck size={34} />
      </span>
      <p className="step-title" tabIndex={-1}>
        {t.t('ng-ui-sc-done')}
      </p>
      <p className="step-text">{t.t('ng-ui-sc-saving')}</p>
    </div>
  );
}
