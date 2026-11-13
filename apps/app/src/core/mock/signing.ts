/**
 * Mock-Kern, Bereich Signieren (UI-A): eine Signiersitzung mit den nativen ID-Austria-Masken (Anmeldung,
 * App-Bestätigung mit Vergleichswert und Countdown, QR-Code, SMS-TAN, FIDO2), Keystore-Passwort und
 * Chipkarte, dazu der Fortschritt für den Stepper (`SigningProgressView`, ADR 0004 §3.6).
 *
 * Wie im Arbeitsbereich-Modus des Kerns: Abbruch führt zurück zum Arbeitsbereich (Hinweis
 * `ng-signing-cancelled`), Fehler erscheinen als Fehlerschritt mit Herkunft `signing`, „Erneut versuchen“
 * nach einem Verbindungsfehler bzw. einer Ablehnung in der App nutzt **dieselbe** Sitzung (keine zweite
 * Anmeldung). Keine echten Zugangsdaten, kein Netz.
 */
import type {
  BackendId,
  CertificateSource,
  ConfirmMethod,
  FormAnswerView,
  FormContext,
  FormProblem,
  InteractionView,
  NativeForm,
  NoticeAnswer,
  Progress,
  SigningProgressView,
  SigningStage,
} from '../types';
import type { MockSigningFlow } from './shared';

/** Vergleichswert der Demo (wie im Prototyp). */
export const REFERENCE_VALUE = 'tpJXmaU/Aj';
/** Link „Signaturdaten anzeigen“ der Demo (öffnet nichts). */
export const SIGNATURE_DATA_URL = 'https://www.a-trust.at/mobile/ShowSigobj.aspx?demo';

/** Beispiel-QR (SVG-Muster, kein echter Code). */
export function demoQrBytes(): number[] {
  const cells: string[] = [];
  let seed = 7;
  for (let y = 0; y < 21; y++) {
    for (let x = 0; x < 21; x++) {
      seed = (seed * 1103515245 + 12345) & 0x7fffffff;
      const finder = (x < 7 && y < 7) || (x > 13 && y < 7) || (x < 7 && y > 13);
      const on = finder ? x % 6 === 0 || y % 6 === 0 || (x % 7 > 1 && x % 7 < 5 && y % 7 > 1 && y % 7 < 5) : seed % 3 === 0;
      if (on) cells.push(`<rect x="${x}" y="${y}" width="1" height="1"/>`);
    }
  }
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="-2 -2 25 25" width="250" height="250"><rect x="-2" y="-2" width="25" height="25" fill="#fff"/><g fill="#000">${cells.join('')}</g></svg>`;
  return Array.from(new TextEncoder().encode(svg));
}

/** Was die Sitzung vom umgebenden Ablauf braucht. */
export interface SigningHost {
  later(fn: () => void, factor?: number): void;
  /** Signatur erstellt → Ausgabe */
  finished(): void;
  /** Fehler → Fehlerschritt in der Karte (`retry`: Sitzung bleibt für „Erneut versuchen“ offen) */
  failed(code: string, params: Record<string, string>, retry: boolean): void;
  /** Abbruch in einer Maske bzw. Meldung → zurück zum Arbeitsbereich */
  cancelled(): void;
  /** Login erfolgreich (Benutzername merken übernimmt der Einstellungsbereich) */
  loggedIn(remember: boolean): void;
  /** A-Trust hat das Passwort verworfen (Fehlerseite) */
  passwordCleared(): void;
  /** Keystore-Passwort angenommen (Stapel: gilt für die ganze Sitzung) */
  unlocked?(): void;
}

export interface SigningOptions {
  backend: BackendId;
  doc: { id: number; name: string };
  flow: MockSigningFlow;
  /** sichtbarer Block: das Zertifikat wird gelesen */
  visible: boolean;
  fido2Available: boolean;
  fido2Default: boolean;
  mobile: boolean;
  autoComplete: boolean;
  /** Anmeldedaten aus dem „Merken“-Speicher (keine Login-Maske) */
  remembered: boolean;
  prefillUsername: string | null;
  rememberChecked: boolean;
  keystoreSubject: string;
  /** erster Signierversuch scheitert mit `backend-network` (Szenario `error-atrust`) */
  networkErrorOnce: boolean;
  /** Vergleichswert dieser Bestätigung (Stapel: je Dokument ein eigener; Standard `REFERENCE_VALUE`) */
  referenceValue?: string;
  /** Zertifikat in dieser Sitzung schon gelesen (Stapel ab dem zweiten Dokument mit sichtbarem Block) */
  certificateRead?: boolean;
  /** Mehrfachsignatur: Anzahl der Signaturen dieses Dokuments (je Block eine; Standard 1) */
  signatures?: number;
  /** Keystore in dieser Sitzung schon entsperrt (Stapel: Passwort nur einmal) */
  sessionUnlocked?: boolean;
  /** erster Signierversuch scheitert mit diesem Fehler (Stapel: Störungen je Dokument) */
  failOnce?: { code: string; params: Record<string, string>; retry: boolean } | null;
  /** weiteres Dokument einer laufenden Sitzung (Stapel): „Dokument k von n“ statt Verbindungsaufbau */
  continuing?: { index: number; total: number } | null;
}

/** Eine Signiersitzung (Einzeldokument). */
export class SigningSession {
  interaction: InteractionView = { kind: 'waiting', progress: 'Connecting' };
  private stage: SigningStage = 'connecting';
  private method: ConfirmMethod | null = null;
  private loginDone = false;
  private certificate: CertificateSource;
  private request = 0;
  private networkErrorPending: boolean;
  private failPending: SigningOptions['failOnce'];
  private confirmations = 0;
  /** schon eine Bestätigung angefordert (Ablehnung/Zeitablauf nur beim ersten Mal) */
  private attempted = false;
  /** Sitzung durch Abbruch beendet (spätere Zeitgeber wirken nicht mehr) */
  private closed = false;

  constructor(
    private readonly host: SigningHost,
    readonly opts: SigningOptions,
  ) {
    this.certificate = opts.backend === 'atrust_native' && opts.visible ? (opts.certificateRead ? 'session' : 'read') : 'not_needed';
    this.networkErrorPending = opts.networkErrorOnce;
    this.failPending = opts.failOnce ?? null;
    this.loginDone = opts.remembered;
  }

  get backend(): BackendId {
    return this.opts.backend;
  }

  /** Fortschritt für Stepper und Karte. */
  progress(): SigningProgressView {
    return {
      backend: this.opts.backend,
      stage: this.stage,
      method: this.method,
      doc: { id: this.opts.doc.id, name: this.opts.doc.name },
      index: 1,
      total: 1,
      login_done: this.loginDone,
      // Mehrfachsignatur: der Mock bestätigt alle Signaturen des Dokuments in einem Schritt
      confirmations_done: this.confirmations * (this.opts.signatures ?? 1),
      confirmations_total: this.opts.signatures ?? 1,
      certificate: this.certificate,
      batch: false,
      pause: null,
    };
  }

  private get ref(): string {
    return this.opts.referenceValue ?? REFERENCE_VALUE;
  }

  /** Sitzung öffnen bzw. nach „Erneut versuchen“ fortsetzen. */
  start(): void {
    this.closed = false;
    const next = this.opts.continuing;
    this.stage = next ? 'preparing' : 'connecting';
    this.interaction = next ? { kind: 'waiting', progress: { Document: { index: next.index, total: next.total } } } : { kind: 'waiting', progress: 'Connecting' };
    this.later(() => {
      switch (this.opts.backend) {
        case 'keystore':
          if (this.opts.sessionUnlocked) {
            // Stapel: Passwort der Sitzung bekannt – signieren ohne neue Abfrage
            this.stage = 'confirming';
            this.method = 'password';
            this.interaction = { kind: 'waiting', progress: 'Signing' };
            this.later(() => this.confirmOrFail(() => this.finish()));
            return;
          }
          this.stage = 'confirming';
          this.method = 'password';
          this.interaction = {
            kind: 'secret',
            request: ++this.request,
            prompt: { kind: 'KeystorePassword', subject: this.opts.keystoreSubject, retry: false },
            // „Passwort merken?“ ergänzt die Projektion (`withStorageQuestion`)
            ask_storage: false,
            keychain_available: false,
          };
          return;
        case 'local_bku':
          this.stage = 'confirming';
          this.method = 'card';
          this.interaction = { kind: 'waiting', progress: 'Signing' };
          if (this.opts.autoComplete) this.later(() => this.finish(), 8);
          return;
        default:
          if (this.loginDone) this.afterLogin();
          else this.showLogin(null);
      }
    });
  }

  private later(fn: () => void, factor = 1): void {
    this.host.later(() => {
      if (!this.closed) fn();
    }, factor);
  }

  private ctx(extra: Partial<FormContext> = {}): FormContext {
    return {
      signature_data_url: SIGNATURE_DATA_URL,
      can_switch_to_sms: true,
      can_switch_to_fido2: true,
      fido2_visible: this.opts.fido2Available && !this.opts.mobile,
      problem: null,
      ...extra,
    };
  }

  private form(form: NativeForm): void {
    this.interaction = { kind: 'form', request: ++this.request, form };
  }

  private showLogin(problem: FormProblem | null): void {
    this.stage = 'login';
    this.method = null;
    this.loginDone = false;
    this.form({
      Login: {
        prefill_username: this.opts.prefillUsername,
        remember: this.opts.rememberChecked,
        ctx: this.ctx({ problem, can_switch_to_sms: false, can_switch_to_fido2: false, fido2_visible: false }),
      },
    });
  }

  private afterLogin(): void {
    if (this.certificate === 'read') {
      this.stage = 'certificate';
      this.interaction = { kind: 'waiting', progress: 'ReadingCertificate' };
      this.later(() => {
        this.certificate = 'session';
        this.confirm();
      });
      return;
    }
    this.confirm();
  }

  /** Vorgemerkte Störung auslösen (`true`) oder `next` ausführen. */
  private confirmOrFail(next: () => void): void {
    const fail = this.failPending;
    if (fail) {
      this.failPending = null;
      this.host.failed(fail.code, { ...fail.params }, fail.retry);
      return;
    }
    next();
  }

  /** Bestätigungsmaske je Ablauf. */
  private confirm(): void {
    if (this.networkErrorPending) {
      this.networkErrorPending = false;
      this.host.failed('backend-network', { detail: 'service.a-trust.at:443 – Zeitüberschreitung nach 30 s' }, true);
      return;
    }
    if (this.failPending) {
      this.confirmOrFail(() => undefined);
      return;
    }
    this.stage = 'confirming';
    // Ablehnung bzw. Zeitablauf nur bei der ersten Bestätigung; „Erneut“ fordert eine neue an, die gelingt
    const first = !this.attempted;
    this.attempted = true;
    const scripted = this.opts.flow === 'decline' || this.opts.flow === 'timeout' ? (first ? this.opts.flow : 'app') : this.opts.flow;
    const flow = this.opts.fido2Default && this.opts.fido2Available && !this.opts.mobile ? 'fido2' : scripted;
    switch (flow) {
      case 'none':
        // ohne zweiten Faktor: A-Trust liefert nach der Anmeldung gleich die Signatur (keine Bestätigungsmaske)
        this.method = null;
        this.finish();
        return;
      case 'sms':
        this.method = 'sms';
        return this.form({ SmsTan: { reference_value: this.ref, ctx: this.ctx() } });
      case 'qr':
        this.method = 'qr';
        return this.form({
          QrCode: { reference_value: this.ref, image: { bytes: demoQrBytes(), mime: 'image/svg+xml' }, timeout_secs: 300, ctx: this.ctx() },
        });
      case 'fido2':
        this.method = 'fido2';
        return this.form({ Fido2: { in_progress: false, ctx: this.ctx({ can_switch_to_fido2: false }) } });
      case 'decline': {
        this.method = 'app';
        this.form({ WaitForApp: { reference_value: this.ref, timeout_secs: 300, ctx: this.ctx() } });
        const req = this.request;
        this.later(() => {
          if (this.interaction.kind === 'form' && this.interaction.request === req) this.host.failed('ng-signing-declined', {}, true);
        }, 6);
        return;
      }
      case 'timeout': {
        this.method = 'app';
        this.form({ WaitForApp: { reference_value: this.ref, timeout_secs: 12, ctx: this.ctx() } });
        const req = this.request;
        // Long-Poll abgelaufen → A-Trust zeigt eine Fehlerseite (das Backend löscht das Passwort)
        this.host.later(() => {
          if (this.closed || this.interaction.kind !== 'form' || this.interaction.request !== req) return;
          this.host.passwordCleared();
          this.interaction = {
            kind: 'notice',
            request: ++this.request,
            notice: { kind: 'Error', code: 'AtrustError', text: 'Die Zeit für die Bestätigung ist abgelaufen.', buttons: 'RetryCancel' },
          };
        }, 34);
        return;
      }
      default: {
        this.method = 'app';
        this.form({ WaitForApp: { reference_value: this.ref, timeout_secs: 300, ctx: this.ctx() } });
        if (this.opts.autoComplete) {
          const req = this.request;
          this.later(() => {
            if (this.interaction.kind === 'form' && this.interaction.request === req) this.finish();
          }, 12);
        }
      }
    }
  }

  /** Long-Poll meldet „bestätigt“ bzw. Chipkarte fertig (Tests, Browser-Vorschau). */
  completeConfirmation(): boolean {
    if (this.stage !== 'confirming') return false;
    if (this.interaction.kind === 'form' || (this.interaction.kind === 'waiting' && this.opts.backend === 'local_bku')) {
      this.finish();
      return true;
    }
    return false;
  }

  private finish(): void {
    this.stage = 'finishing';
    this.confirmations = 1;
    this.interaction = { kind: 'waiting', progress: 'Signing' };
    this.later(() => {
      this.stage = 'saving';
      this.host.finished();
    });
  }

  submitForm(request: number, answer: FormAnswerView): void {
    if (this.interaction.kind !== 'form' || this.interaction.request !== request) return;
    const form = this.interaction.form;
    switch (answer.kind) {
      case 'login': {
        if (answer.password.length < 5) return this.showLogin('PasswordTooShort');
        if (answer.password.length > 200) return this.showLogin('PasswordTooLong');
        if (answer.password === 'falsch') return this.showLogin('LoginRejected');
        this.loginDone = true;
        this.host.loggedIn(answer.remember);
        this.interaction = { kind: 'waiting', progress: 'WaitingForServer' };
        this.later(() => this.afterLogin());
        return;
      }
      case 'tan':
        if (answer.tan === '000000') {
          return this.form({ SmsTan: { reference_value: this.ref, ctx: this.ctx({ problem: { Atrust: 'TAN falsch' } }) } });
        }
        this.interaction = { kind: 'waiting', progress: 'WaitingForServer' };
        this.later(() => this.finish());
        return;
      case 'switch_to_sms':
        this.method = 'sms';
        return this.form({ SmsTan: { reference_value: this.ref, ctx: this.ctx() } });
      case 'switch_to_fido2':
        this.method = 'fido2';
        return this.form({ Fido2: { in_progress: false, ctx: this.ctx({ can_switch_to_fido2: false }) } });
      case 'start_fido2':
        this.form({ Fido2: { in_progress: true, ctx: this.ctx({ can_switch_to_fido2: false }) } });
        if (this.opts.autoComplete) this.later(() => this.finish(), 6);
        return;
      case 'continue':
        if ('QrCode' in form || 'WaitForApp' in form) {
          this.interaction = { kind: 'waiting', progress: 'WaitingForServer' };
          this.later(() => this.finish());
        }
        return;
    }
  }

  submitSecret(request: number, secret: string): void {
    if (this.interaction.kind !== 'secret' || this.interaction.request !== request) return;
    if (secret === 'falsch') {
      this.interaction = { ...this.interaction, request: ++this.request, prompt: { ...this.interaction.prompt, retry: true } };
      return;
    }
    if (this.interaction.prompt.kind === 'KeystorePassword' || this.interaction.prompt.kind === 'KeyPassword') this.host.unlocked?.();
    this.interaction = { kind: 'waiting', progress: 'Signing' };
    this.later(() => this.finish());
  }

  answerNotice(request: number, answer: NoticeAnswer): void {
    if (this.interaction.kind !== 'notice' || this.interaction.request !== request) return;
    if (answer === 'Cancel') {
      this.closed = true;
      this.host.cancelled();
      return;
    }
    // „Erneut“ nach der Fehlerseite: A-Trust zeigt wieder die Anmeldung (Passwort gelöscht)
    this.showLogin(null);
  }

  /** „Signaturdaten anzeigen“: URL nur, wenn die offene Maske den Link liefert. */
  signatureDataUrl(request: number): string | null {
    if (this.interaction.kind === 'form' && this.interaction.request === request) return SIGNATURE_DATA_URL;
    return null;
  }

  /** Abbruch durch PIDA (Esc, ×, „Abbrechen“) bzw. Sitzung verwerfen. */
  close(): void {
    this.closed = true;
  }

  /** „Erneut versuchen“ im Fehlerschritt: dieselbe Sitzung, ohne neue Anmeldung. */
  retry(): void {
    this.closed = false;
    if (this.opts.backend === 'atrust_native' && this.loginDone) {
      this.stage = 'confirming';
      this.interaction = { kind: 'waiting', progress: 'WaitingForServer' };
      this.later(() => this.confirm());
    } else {
      this.start();
    }
  }
}

/** Waiting-Fortschritt als Text-Schlüssel (für Tests). */
export function progressKey(p: Progress): string {
  return typeof p === 'string' ? p : 'Document';
}
