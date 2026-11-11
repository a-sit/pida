/**
 * Wurzel der Oberfläche „Arbeitsbereich“ (ADR 0004 §4.2): abonniert den `ViewState` und rendert je Zustand
 * eine Ansicht – Start, Arbeitsbereich (Warnung, Positionieren, Signieren als Overlay), Fertig, Stapel
 * (Übersicht, Lauf mit der Stapelkarte als Overlay), Zustandskarten – dazu Kopfzeile, Sprunglink,
 * Signierkarte, Overlays der Pakete UI-B (Einstellungen, Ersteinrichtung, Palette), „Signatur fortsetzen“,
 * Hinweise und die Live-Region. Einen Prüfmodus gibt es nicht (ADR 0004 Nachtrag 15): wie PDF-Over verweist PIDA
 * für die Prüfung fremder Signaturen auf signaturpruefung.gv.at.
 *
 * Shell-Dienste: Dateiauswahl (Desktop-Dialog bzw. Mobile-Picker), Drag & Drop über die Fenster-Ereignisse
 * (Pfade; auf „+ PDF hinzufügen“ der Miniaturleiste wie „Weitere PDFs hinzufügen“), Tastenkürzel ⌘/Strg+O
 * (Öffnen), ⌘/Strg+↵ (Signieren – eine ausdrückliche Aktion, A1), ⌘/Strg+B (Zum Stapel bzw. Zurück zum Stapel),
 * ⌘/Strg+, (Einstellungen), ⌘/Strg+K und „?“ (sobald die Palette da ist), Zugriffstasten (Alt+Taste bzw. ⌃⌥+Taste),
 * Farbschema, Fokus beim Ansichtswechsel. Der Kern bleibt maßgeblich: Freigaben kommen aus
 * `ViewState::actions`. Bei geparkter Arbeit („Zur Startseite“, ADR 0004 Nachtrag 10) fragen gewählte bzw.
 * abgelegte PDFs und „Erneut signieren“ aus „Zuletzt signiert“ erst nach, ob sie zum Stapel kommen
 * (`ParkedJoinDialog`).
 *
 * Der Kern läuft in der App immer im Arbeitsbereich-Modus (`workspace_ui`, ADR 0004 §3.14): `StepView::Position`
 * kommt dort nur mit `document`, `StepView::Placeholder` gar nicht. Die bisherigen Bildschirme (`screens/`) sind
 * mit dem Merge von A1 entfernt (§3.15); ein klassischer Zustand ohne Dokument zeigt die Startansicht.
 */
import { Suspense, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { I18nProvider } from 'react-aria-components';
import type { Bridge, FileDragEvent } from '../core/bridge';
import { mainBar, workspaceDefaults } from '../core/defaults';
import { isMobile, type OpenFilesMode, type UiEvent, type ViewState } from '../core/types';
import { webProfile } from '../core/web-profile';
import { accessibilityOf } from '../features/accessibility';
import { BatchView } from '../features/batch';
import { DoneView, OutputDialogs } from '../features/done';
import { Onboarding } from '../features/onboarding';
import { HELP_AVAILABLE, HelpPanel } from '../features/help';
import { CommandPalette, PALETTE_AVAILABLE, ShortcutsDialog } from '../features/palette';
import { QrPlaceholderDialog } from '../features/qr/QrPlaceholderDialog';
import { SettingsPanel } from '../features/settings';
import { cardMode, errorInCard, SigningCard } from '../features/signing';
import { ParkedJoinDialog, StartView, type JoinRequest } from '../features/start';
import { ErrorCard, FinishedCard, LoadingCard, StartingCard, WarningCard, Workspace } from '../features/workspace';
import { translator } from '../i18n/catalog';
import { accessKeyFromEvent, trackAltKey, triggerAccessKey } from '../ui/AccessKeys';
import { Toasts } from '../ui/Toasts';
import { batchNav, buildCommands, buildShortcuts, demoKeys } from './commands';
import { AppContext, type AppContextValue, type SettingsSectionId, type ShellOverlay, type Toast } from './context';
import { DownloadBanner } from './DownloadBanner';
import { EMBLEM_DROP_EVENT } from './events';
import { Header } from './Header';
import { keepFocusedFieldVisible } from './keyboard';
import { ResumeDialog } from './ResumeDialog';
import { syncSystemBars, watchSystemBars } from './systemBars';
import { applyTheme, readTheme, writeTheme, type Theme } from './theme';
import './shell.css';

/** Zustand bis zum ersten `ViewState` des Kerns. */
const STARTING: ViewState = {
  step: { step: 'starting' },
  main_bar: mainBar(['', '', '', ''], false),
  settings: null,
  language: 'de',
  notices: [],
  migration: null,
  done: [],
  // bis zum ersten ViewState: Desktop-Bedienelemente (der Startschritt hat keine plattformabhängigen)
  platform: { os: 'linux', mobile: false },
  resume: null,
  keystore_import: null,
  privacy_text: null,
  ...workspaceDefaults(),
};

const IMAGE_EXT = /\.(png|jpe?g|gif)$/i;

/** Ansicht des Hauptbereichs. */
export type MainView = 'starting' | 'start' | 'loading' | 'workspace' | 'warning' | 'done' | 'error' | 'finished' | 'batch';

/**
 * Ansicht je Zustand (ADR 0004 §3.1: `document` gesetzt in Warnung, Positionieren, Signieren, Fertig; im
 * Stapellauf liegt die Karte über der Stapelübersicht, §3.8).
 */
export function mainView(view: ViewState): MainView {
  const s = view.step;
  const doc = view.document;
  switch (s.step) {
    case 'starting':
      return 'starting';
    case 'finished':
      return 'finished';
    case 'open':
      return 'start';
    case 'batch':
      return view.batch ? 'batch' : 'start';
    case 'loading':
      return 'loading';
    case 'warning':
      return doc ? 'workspace' : 'warning';
    case 'signing':
      return doc ? 'workspace' : view.batch ? 'batch' : 'start';
    case 'placeholder':
    case 'position':
    case 'backend':
      return doc ? 'workspace' : 'start';
    case 'output':
      return s.result.path || s.result.save_pending ? 'done' : doc ? 'workspace' : 'done';
    case 'error':
      return errorInCard(view) ? (doc ? 'workspace' : 'start') : 'error';
  }
}

/** Titel des Fensters je Ansicht. */
const TITLE: Record<MainView, string> = {
  starting: 'ng-ui-shell-starting',
  start: 'ng-ui-start-title',
  loading: 'ng-ui-ws-loading-title',
  workspace: 'ng-ui-shell-title-workspace',
  warning: 'ng-ui-warning-title',
  done: 'ng-ui-fin-title',
  error: 'ng-ui-sc-error-title',
  finished: 'ng-ui-finished',
  batch: 'ng-ui-batch-title',
};

export { accessKeyFromEvent, triggerAccessKey };

function isTyping(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el || !el.tagName) return false;
  const tag = el.tagName;
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || el.isContentEditable;
}

export function App({ bridge, isMac: isMacProp }: { bridge: Bridge; isMac?: boolean }) {
  const [view, setView] = useState<ViewState | null>(null);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [liveText, setLiveText] = useState('');
  const [dragActive, setDragActive] = useState(false);
  const [section, setSection] = useState<SettingsSectionId>('signature');
  const [overlay, setOverlay] = useState<ShellOverlay>(null);
  // Offline-Hilfe: offen mit optionalem Ziel-Artikel
  const [help, setHelp] = useState<{ target: string | null } | null>(null);
  const openHelp = useCallback((target: string | null = null) => {
    if (HELP_AVAILABLE) setHelp({ target });
  }, []);
  const [theme, setThemeState] = useState<Theme>(() => readTheme());
  const toastId = useRef(0);
  const [initialFailed, setInitialFailed] = useState(false);
  const signHandler = useRef<(() => void) | null>(null);
  const isMac = isMacProp ?? (typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent));

  const current = view ?? STARTING;
  const t = translator(current.language);
  const mobile = isMobile(current);
  const main = mainView(current);

  // --- Zustand vom Kern ------------------------------------------------------------------------
  useEffect(() => {
    let alive = true;
    const off = bridge.onState((v) => alive && setView(v));
    // Erster Stand: scheitert die Abfrage (Kern noch nicht bereit), einige Male wiederholen – sonst bliebe die
    // Oberfläche ohne jede Meldung im Startzustand stehen; danach den Fehler zeigen
    let timer: ReturnType<typeof setTimeout> | null = null;
    const load = (attempt: number) => {
      bridge
        .view()
        .then((v) => alive && setView((cur) => cur ?? v))
        .catch(() => {
          if (!alive) return;
          if (attempt < 3) timer = setTimeout(() => load(attempt + 1), 400 * (attempt + 1));
          else setInitialFailed(true);
        });
    };
    load(0);
    return () => {
      alive = false;
      if (timer) clearTimeout(timer);
      off();
    };
  }, [bridge]);

  useEffect(() => {
    applyTheme(theme);
    // Android: Systemleisten in den Farben des Schemas (nach dem Setzen von `data-theme`)
    syncSystemBars();
  }, [theme]);
  useEffect(() => watchSystemBars(), []);
  useEffect(() => trackAltKey(), []);
  useEffect(() => {
    document.documentElement.lang = current.language;
  }, [current.language]);
  // Auch an der Dokumentwurzel: Dialoge (React Aria `Modal`) liegen als Portal außerhalb von `.app`
  useEffect(() => {
    document.documentElement.toggleAttribute('data-mobile', mobile);
  }, [mobile]);
  // Bildschirmtastatur: fokussiertes Feld im verkleinerten Dialog im Blick halten (app/keyboard.ts)
  useEffect(() => (mobile ? keepFocusedFieldVisible() : undefined), [mobile]);

  const setTheme = useCallback((next: Theme) => {
    writeTheme(next);
    setThemeState(next);
  }, []);

  const toast = useCallback((text: string, kind: Toast['kind'] = 'info', detail?: string) => {
    toastId.current += 1;
    const id = toastId.current;
    setToasts((list) => [...list.slice(-3), { id, kind, text, detail }]);
  }, []);
  const dismiss = useCallback((id: number) => setToasts((list) => list.filter((x) => x.id !== id)), []);
  // Kern beim Start nicht erreichbar (und auch kein Stand über `onState` gekommen): sagen statt schweigen
  const hasView = view !== null;
  useEffect(() => {
    if (initialFailed && !hasView) toast(translator(STARTING.language).t('ng-ui-bridge-error'), 'error');
  }, [initialFailed, hasView, toast]);

  // Leeren und kurz danach setzen, damit derselbe Text erneut angesagt wird; eine noch ausstehende Ansage verfällt
  // (sonst landete nach `announce('')` – z. B. beim Dokumentwechsel – noch der Text des vorigen Dokuments)
  const liveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const announce = useCallback((text: string) => {
    if (liveTimer.current) clearTimeout(liveTimer.current);
    liveTimer.current = null;
    setLiveText('');
    if (text) liveTimer.current = setTimeout(() => setLiveText(text), 30);
  }, []);

  const language = current.language;
  const dispatch = useCallback(
    (event: UiEvent): Promise<boolean> =>
      bridge.dispatch(event).then(
        () => true,
        (e: unknown) => {
          const code = e instanceof Error ? e.message : 'ng-ui-bridge-error';
          toast(translator(language).code(code, {}, 'ng-ui-bridge-error'), 'error');
          return false;
        },
      ),
    [bridge, toast, language],
  );

  const openSettings = useCallback(
    (next: SettingsSectionId = 'signature') => {
      setSection(next);
      void dispatch({ type: 'open_settings' });
    },
    [dispatch],
  );

  // --- Dateien öffnen --------------------------------------------------------------------------
  // Geparkte Arbeit („Zur Startseite“, ADR 0004 Nachtrag 10): neue PDFs aus der Dateiauswahl bzw. abgelegte und
  // „Erneut signieren“ aus „Zuletzt signiert“ fragen vorher, ob sie zum Stapel kommen (`ParkedJoinDialog`); „Öffnen
  // mit“ und zweite Instanz entscheidet der Kern
  const [join, setJoin] = useState<JoinRequest | null>(null);
  const parkedRef = useRef(current.parked ?? null);
  parkedRef.current = current.parked ?? null;
  useEffect(() => {
    if (!current.parked) setJoin(null);
  }, [current.parked]);

  // `mode` aus „+ PDF“ (ADR 0004 Nachtrag 9): „Anderes PDF öffnen“ wählt eine Datei, sonst Mehrfachauswahl;
  // was daraus folgt (Stapel, Schließen), entscheidet der Kern. Alle gewählten Pfade gehen in der Reihenfolge der
  // Auswahl an den Kern; Duplikate und fehlende Dateien meldet der Kern (ADR 0004 Nachtrag 17)
  const os = current.platform.os;
  const browse = useCallback(
    async (mode?: OpenFilesMode) => {
      const multiple = mode !== 'replace';
      const open = (paths: string[]) => {
        if (!mode && parkedRef.current) setJoin({ paths });
        else void dispatch(mode ? { type: 'open_files', paths, mode } : { type: 'open_files', paths });
      };
      if (mobile) {
        // Mobile: Dokument-Picker bzw. SAF über das Plugin; Kopien in der Sandbox (keine Pfade der Quelle). Nicht
        // kopierbare Dateien (nicht lesbar, zu groß) zählt die native Seite – kurzer Hinweis statt stillem Verwerfen
        try {
          const { paths, skipped } = await bridge.pickMobileDocuments({ multiple });
          if (skipped > 0) toast(t.t('ng-ui-shell-pick-skipped', { count: skipped }), 'error');
          if (paths.length > 0) open(paths);
        } catch (e) {
          const code = e instanceof Error ? e.message : 'ng-ui-dialog-failed';
          toast(t.code(code, {}, 'ng-ui-dialog-failed'), 'error');
        }
        return;
      }
      try {
        const paths = await bridge.pickFiles({
          // Mehrfachauswahl: die Hinweiszeile des Dialogs sagt, wie mehrere markiert werden (macOS: ein Doppelklick
          // übernimmt nur die angeklickte Datei)
          title: multiple ? t.t('ng-ui-shell-pick-title', { os }) : t.t('dataSourceSelection-browse'),
          multiple,
          filters: [
            { name: t.t('common-PDFExtension_Description'), extensions: ['pdf'] },
            { name: t.t('common-AllExtension_Description'), extensions: ['*'] },
          ],
        });
        if (paths && paths.length > 0) open(paths);
      } catch {
        toast(t.t('ng-ui-dialog-failed'), 'error');
      }
    },
    [bridge, dispatch, t, toast, mobile, os],
  );

  // „Erneut signieren“ aus „Zuletzt signiert“ (ADR 0004 Nachtrag 15): wie „Öffnen“ (`sign_recent`, der Kern kennt
  // den Pfad), bei geparkter Arbeit erst die Rückfrage
  const signRecent = useCallback(
    (id: number) => {
      if (parkedRef.current) setJoin({ recentId: id });
      else void dispatch({ type: 'sign_recent', id });
    },
    [dispatch],
  );

  // „Demo starten“: frische Kopie des Demodokuments (Plattform) und weiter wie „Öffnen“ – bei geparkter Arbeit erst die
  // Rückfrage, was der Kern daraus macht, entscheidet er
  const startDemo = useCallback(async () => {
    try {
      const path = await bridge.demoDocument();
      if (parkedRef.current) setJoin({ paths: [path] });
      else void dispatch({ type: 'open_files', paths: [path] });
    } catch (e) {
      toast(t.code(e instanceof Error ? e.message : 'ng-ui-open-failed', {}, 'ng-ui-open-failed'), 'error');
    }
  }, [bridge, dispatch, t, toast]);

  const requestSign = useCallback(() => signHandler.current?.(), []);
  const registerSign = useCallback((handler: (() => void) | null) => {
    signHandler.current = handler;
  }, []);

  // --- Drag & Drop -----------------------------------------------------------------------------
  const dropState = useRef({ current, dispatch, toast });
  dropState.current = { current, dispatch, toast };
  useEffect(() => {
    return bridge.onFileDrag((e: FileDragEvent) => {
      if (e.type === 'enter' || e.type === 'over') {
        setDragActive(true);
        return;
      }
      setDragActive(false);
      if (e.type !== 'drop' || e.paths.length === 0) return;
      const { current: v, dispatch: d, toast: tst } = dropState.current;
      if (v.settings) {
        const hit = typeof document.elementFromPoint === 'function' ? document.elementFromPoint(e.x, e.y) : null;
        const image = e.paths.find((p) => IMAGE_EXT.test(p));
        if (image && hit?.closest('[data-emblem-drop]')) window.dispatchEvent(new CustomEvent(EMBLEM_DROP_EVENT, { detail: image }));
        return;
      }
      // auf „+ PDF hinzufügen“ der Miniaturleiste: wie „Weitere PDFs hinzufügen“ (Nachtrag 9)
      const hit = typeof document.elementFromPoint === 'function' ? document.elementFromPoint(e.x, e.y) : null;
      const add = !!hit?.closest('[data-add-drop]');
      if (!v.actions.open_files) tst(translator(v.language).t('ng-ui-drop-not-here'));
      // auf die Startseite mit geparkter Arbeit: erst fragen (Nachtrag 10)
      else if (!add && v.parked) setJoin({ paths: e.paths });
      else void d(add ? { type: 'open_files', paths: e.paths, mode: 'add' } : { type: 'open_files', paths: e.paths });
    });
  }, [bridge]);

  // --- Tastatur --------------------------------------------------------------------------------
  const keyState = useRef({ current, browse, startDemo, openSettings, requestSign, overlay, dispatch });
  keyState.current = { current, browse, startDemo, openSettings, requestSign, overlay, dispatch };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const { current: v, browse: b, startDemo: demo, openSettings: settings, requestSign: sign, overlay: o, dispatch: d } = keyState.current;
      const mod = isMac ? e.metaKey : e.ctrlKey;
      const k = (e.key ?? '').toLowerCase(); // Autofill löst `keydown` ohne `key` aus
      // F1 = Hilfe, auch über dem Signierdialog (z. B. „Was ist der Vergleichswert?“)
      if (e.key === 'F1' && HELP_AVAILABLE) {
        e.preventDefault();
        setHelp((h) => h ?? { target: null });
        return;
      }
      const blocked = o !== null || v.settings !== null || document.querySelector('[data-modal-layer]') !== null;
      if (mod && !e.altKey && !e.shiftKey && k === 'k' && PALETTE_AVAILABLE) {
        e.preventDefault();
        if (o === 'palette') {
          setOverlay(null);
          return;
        }
        // nicht beim Signieren und nicht über anderen Dialogen (Ersteinrichtung, Rückfragen, „Signatur
        // fortsetzen“ …); über den Einstellungen und der Kürzelübersicht wie im Prototyp
        const otherDialog = Array.from(document.querySelectorAll('[data-modal-layer]')).some((el) => !el.closest('.settings-modal, .shortcuts-modal, .palette-modal'));
        if (v.step.step !== 'signing' && !otherDialog) setOverlay('palette');
        return;
      }
      if (mod && !e.altKey && !e.shiftKey && e.key === ',') {
        if (v.actions.settings && !blocked) {
          e.preventDefault();
          settings();
        }
        return;
      }
      if (mod && !e.altKey && !e.shiftKey && k === 'o') {
        if (v.actions.open_files && !blocked) {
          e.preventDefault();
          void b();
        }
        return;
      }
      // „Demo starten“ (`demoKeys`): Apps ⌘/Strg+D, Web ⌥⇧D bzw. Alt+Umschalt+D über die physische Taste (unter macOS
      // tippt ⌥⇧D ein Sonderzeichen); nicht beim Tippen
      const dk = demoKeys();
      if (e.code === 'KeyD' && mod === dk.mod && e.altKey === !!dk.alt && e.shiftKey === !!dk.shift && !(isMac ? e.ctrlKey : e.metaKey) && !isTyping(e.target)) {
        if (v.actions.open_files && !blocked) {
          e.preventDefault();
          void demo();
        }
        return;
      }
      if (mod && !e.altKey && !e.shiftKey && k === 'b') {
        // ⌘/Strg+B = „Zum Stapel“ bzw. „Zurück zum Stapel“ (Freigabe `actions.show_batch` bzw. aus dem Stapel geöffnet)
        const nav = batchNav(v);
        if (nav && !blocked) {
          e.preventDefault();
          void d({ type: nav === 'back' ? 'back_to_batch' : 'show_batch' });
        }
        return;
      }
      if (mod && !e.altKey && !e.shiftKey && e.key === 'Enter') {
        // ⌘↵ = „Signieren“ bzw. im Stapel „Alle signieren“ (ausdrückliche Aktion, A1)
        if ((v.actions.sign || v.actions.start_batch) && !blocked) {
          e.preventDefault();
          sign();
        }
        return;
      }
      // Einzeltaste: im barrierefreien Modus aus (WCAG 2.1.4, Sprachsteuerung); dort über ⌘K bzw. die Hilfe
      if (e.key === '?' && !mod && !isTyping(e.target) && PALETTE_AVAILABLE && !blocked && !accessibilityOf(v).mode) {
        e.preventDefault();
        setOverlay('shortcuts');
        return;
      }
      const key = accessKeyFromEvent(e, isMac);
      if (key && triggerAccessKey(key)) e.preventDefault();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [isMac]);

  // --- Fokus und Titel beim Ansichtswechsel ------------------------------------------------------
  useEffect(() => {
    document.title = `PIDA – ${t.t(TITLE[main])}`;
  }, [main, t]);
  // Jede Ansicht beginnt oben: `.app-main` ist für alle Ansichten derselbe Container. Ohne Zurücksetzen
  // übernähme z. B. der Arbeitsbereich (`overflow: hidden`) den Bildlauf der Startseite – im Querformat des
  // Telefons stünden Werkzeugleiste und Sheet dann verschoben und ließen sich nicht zurückrollen.
  useLayoutEffect(() => {
    const el = document.getElementById('main');
    if (el) el.scrollTop = 0;
  }, [main]);
  const firstView = useRef(true);
  useEffect(() => {
    if (firstView.current) {
      firstView.current = false;
      return undefined;
    }
    const id = setTimeout(() => {
      const active = document.activeElement;
      if (active && active !== document.body && document.body.contains(active)) return;
      (document.querySelector<HTMLElement>('[data-main-heading]') ?? document.getElementById('screen-heading') ?? document.getElementById('main'))?.focus({ preventScroll: true });
    }, 0);
    return () => clearTimeout(id);
  }, [main]);
  // Fokus-Rückfall: Schließt ein Overlay, dessen Auslöser nicht mehr existiert, stünde der Fokus auf <body>
  const overlayOpen = current.settings !== null || cardMode(current) !== null || overlay !== null;
  const wasOpen = useRef(overlayOpen);
  useEffect(() => {
    const was = wasOpen.current;
    wasOpen.current = overlayOpen;
    if (!was || overlayOpen) return undefined;
    const timer = setTimeout(() => {
      const active = document.activeElement;
      if (!active || active === document.body || !active.isConnected) document.getElementById('main')?.focus({ preventScroll: true });
    }, 60);
    return () => clearTimeout(timer);
  }, [overlayOpen]);

  const ctx = useMemo<AppContextValue>(
    () => ({ bridge, view: current, dispatch, t, announce, toast, dragActive, isMac, theme, setTheme, openSettings, openHelp, settingsSection: section, browse, signRecent, startDemo, overlay, setOverlay, requestSign, registerSign }),
    [bridge, current, dispatch, t, announce, toast, dragActive, isMac, theme, setTheme, openSettings, openHelp, section, browse, signRecent, startDemo, overlay, requestSign, registerSign],
  );

  const commands = useMemo(
    () =>
      buildCommands({
        view: current,
        t,
        isMac,
        dispatch,
        browse,
        startDemo,
        requestSign,
        openSettings,
        setTheme,
        setOverlay,
      }),
    [current, t, isMac, dispatch, browse, startDemo, requestSign, openSettings, setTheme],
  );

  const step = current.step;
  // Web-Variante: Banner mit den Downloads der Apps (DownloadBanner.tsx)
  const web = webProfile() !== null;
  return (
    <I18nProvider locale={current.language === 'de' ? 'de-AT' : 'en-GB'}>
      <AppContext.Provider value={ctx}>
        <div
          className={`app view-${main === 'workspace' ? 'workspace' : main === 'done' ? 'done' : main === 'batch' ? 'batch' : 'start'}`}
          data-step={step.step}
          data-view={main}
          data-drag={dragActive || undefined}
          data-mobile={mobile || undefined}
          data-os={current.platform.os}
          data-web={web || undefined}
        >
          <a className="skip-link" href="#main">
            {t.t('ng-ui-shell-skip')}
          </a>
          {web ? <DownloadBanner /> : null}
          <Header theme={theme} onTheme={setTheme} />
          <main
            id="main"
            className="app-main"
            tabIndex={-1}
            // Arbeitsbereich (`overflow: hidden`): der Inhaltsbereich rollt nie – rollt ihn etwas trotzdem (Fokus,
            // `scrollIntoView` in WKWebView), sofort zurück, sonst stünde alles verschoben (macOS-Testbuild 30.10.2026)
            onScroll={(e) => {
              if (main === 'workspace' && e.currentTarget.scrollTop !== 0) e.currentTarget.scrollTop = 0;
            }}
          >
            {main === 'starting' ? <StartingCard /> : null}
            {main === 'start' ? <StartView /> : null}
            {main === 'loading' && step.step === 'loading' ? <LoadingCard step={step} /> : null}
            {main === 'workspace' && current.document ? <Workspace doc={current.document} /> : null}
            {main === 'batch' && current.batch ? <BatchView batch={current.batch} /> : null}
            {main === 'warning' && step.step === 'warning' ? <WarningCard step={step} /> : null}
            {main === 'done' && step.step === 'output' ? <DoneView step={step} /> : null}
            {main === 'error' && step.step === 'error' ? <ErrorCard step={step} /> : null}
            {main === 'finished' && step.step === 'finished' ? <FinishedCard exitCode={step.exit_code} /> : null}
          </main>
          <SigningCard />
          {step.step === 'output' ? <OutputDialogs result={step.result} /> : null}
          {current.settings ? <SettingsPanel settingsView={current.settings} section={section} /> : null}
          {(current.migration || current.first_run) && !current.settings ? <Onboarding migration={current.migration} firstRun={current.first_run} /> : null}
          {current.resume && !current.settings && !current.migration ? <ResumeDialog offer={current.resume} /> : null}
          {join && current.parked ? <ParkedJoinDialog parked={current.parked} request={join} onClose={() => setJoin(null)} /> : null}
          <CommandPalette isOpen={overlay === 'palette'} onClose={() => setOverlay(null)} commands={commands} />
          {HELP_AVAILABLE && help ? (
            <Suspense fallback={null}>
              <HelpPanel target={help.target} onClose={() => setHelp(null)} />
            </Suspense>
          ) : null}
          {overlay === 'qr' ? <QrPlaceholderDialog onClose={() => setOverlay(null)} /> : null}
          <ShortcutsDialog isOpen={overlay === 'shortcuts'} onClose={() => setOverlay(null)} commands={commands} shortcuts={buildShortcuts(t, isMac)} />
          <Toasts toasts={toasts} onDismiss={dismiss} notices={current.notices} />
          {/* `data-live-announcer`: React Aria lässt die Region bei offenen Dialogen sichtbar (sonst inert, Ansagen gingen verloren) */}
          <div className="sr-only" aria-live="polite" aria-atomic="true" data-live-announcer="true" data-testid="live-region">
            {liveText}
          </div>
        </div>
      </AppContext.Provider>
    </I18nProvider>
  );
}
