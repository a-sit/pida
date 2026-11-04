//! „Über PIDA“ und Update-Prüfung im Kern (ADR 0004 Nachtrag 21): Version und Versionsdatum, Zustände der Prüfung,
//! 24-h-Zwischenspeicher beim Start, Schalter und Richtlinie, Fehlerarten mit Grund, „Erneut prüfen“, keine Prüfung
//! ohne Erlaubnis (Mobile, CLI, Richtlinie, Tests ohne Transport) und keine Kennungen im Request.
//!
//! Alles offline mit einem Ersatz-Transport (`CoreOptions::update_transport`). Der einzige Test gegen
//! `api.github.com` läuft nur mit `PIDA_ONLINE_TESTS=1` (`github_releases_online`).
//!
//! UI-Proben des echten Kerns: `PIDA_BLESS=1 cargo test -p pida-core-api --test about real_core_about_views`
//! schreibt `apps/app/src/core/fixtures/real-core-about-views.json`.

mod common;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::*;
use pida_backend_api::Platform;
use pida_config::{Overlay, Policy};
use pida_core_api::update::{
    self, NetConfig, NetError, NetTransport, RELEASES_API, Request, Response, UpdateTransport,
};
use pida_core_api::{AboutView, Core, CoreConfig, CoreOptions, UiEvent, UpdateCheckState, UpdateCheckView, ViewState};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// Antwort des Ersatz-Transports.
#[derive(Clone)]
enum Reply {
    /// HTTP 200 mit diesem Körper
    Json(&'static str),
    /// anderer Status (leerer Körper)
    Status(u16),
    /// Rate-Limit der GitHub-API: 403 mit `x-ratelimit-remaining: 0`
    RateLimited,
    /// Netzfehler
    Error(fn() -> NetError),
}

const TODAY: &str = r#"[
  {"tag_name":"v0.3.0","draft":true,"prerelease":true,"name":"Entwurf"},
  {"tag_name":"v0.2.0","draft":false,"prerelease":true,"html_url":"https://github.com/a-sit/pida/releases/tag/v0.2.0"},
  {"tag_name":"v0.1.0","draft":false,"prerelease":true}
]"#;
const NEWER: &str = r#"[
  {"tag_name":"v0.3.0","draft":false,"prerelease":true},
  {"tag_name":"v0.2.0","draft":false,"prerelease":true}
]"#;

/// GitHub-Ersatz: Antworten der Reihe nach (danach die letzte), zählt Aufrufe, merkt sich Requests; mit `hold`
/// wartet jede Antwort auf eine Freigabe (Zustand „Suche nach Updates …“).
struct FakeGitHub {
    replies: Mutex<VecDeque<Reply>>,
    last: Mutex<Reply>,
    calls: AtomicUsize,
    requests: Mutex<Vec<(NetConfig, Request)>>,
    gate: Option<tokio::sync::Semaphore>,
}

impl FakeGitHub {
    fn new(replies: &[Reply]) -> Arc<FakeGitHub> {
        Self::build(replies, false)
    }

    fn held(replies: &[Reply]) -> Arc<FakeGitHub> {
        Self::build(replies, true)
    }

    fn build(replies: &[Reply], hold: bool) -> Arc<FakeGitHub> {
        Arc::new(FakeGitHub {
            replies: Mutex::new(replies.iter().cloned().collect()),
            last: Mutex::new(replies.last().cloned().unwrap_or(Reply::Json(TODAY))),
            calls: AtomicUsize::new(0),
            requests: Mutex::new(Vec::new()),
            gate: hold.then(|| tokio::sync::Semaphore::new(0)),
        })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn release(&self) {
        if let Some(g) = &self.gate {
            g.add_permits(1);
        }
    }
}

#[async_trait::async_trait]
impl UpdateTransport for FakeGitHub {
    async fn fetch(&self, config: &NetConfig, request: Request) -> Result<Response, NetError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.requests.lock().unwrap().push((config.clone(), request.clone()));
        if let Some(g) = &self.gate {
            g.acquire().await.expect("Semaphore").forget();
        }
        let reply = {
            let mut q = self.replies.lock().unwrap();
            q.pop_front().unwrap_or_else(|| self.last.lock().unwrap().clone())
        };
        let response = |status: u16, body: &[u8]| Response {
            status,
            reason: String::new(),
            url: request.url.clone(),
            headers: vec![("content-type".into(), "application/json; charset=utf-8".into())],
            body: body.to_vec(),
        };
        match reply {
            Reply::Json(body) => Ok(response(200, body.as_bytes())),
            Reply::Status(status) => Ok(response(status, b"")),
            Reply::RateLimited => {
                let mut r = response(403, br#"{"message":"API rate limit exceeded"}"#);
                r.headers.push(("x-ratelimit-limit".into(), "60".into()));
                r.headers.push(("x-ratelimit-remaining".into(), "0".into()));
                r.headers.push(("x-ratelimit-reset".into(), "1792224000".into()));
                Ok(r)
            }
            Reply::Error(e) => Err(e()),
        }
    }
}

fn config(fx: &Fixture, platform: Platform, headless: bool) -> CoreConfig {
    CoreConfig { platform, app_version: "0.2.0".into(), ..fx.config(Overlay::default(), headless) }
}

fn options(transport: &Arc<FakeGitHub>) -> CoreOptions {
    CoreOptions { update_transport: Some(transport.clone() as Arc<dyn UpdateTransport>), ..CoreOptions::isolated() }
}

fn start(fx: &Fixture, transport: &Arc<FakeGitHub>) -> Core {
    Core::new_with(config(fx, Platform::Linux, false), options(transport)).expect("Kern startet")
}

fn fixture(settings: &str) -> Fixture {
    let fx = Fixture::new();
    fx.write_settings(settings);
    fx
}

fn update(v: &ViewState) -> Option<UpdateCheckView> {
    v.about.as_ref().and_then(|a| a.update.clone())
}

fn state(v: &ViewState) -> Option<UpdateCheckState> {
    update(v).map(|u| u.state)
}

async fn wait_state(core: &Core, s: UpdateCheckState) -> ViewState {
    wait_for(core, &format!("Update-Zustand {s:?}"), |v| state(v) == Some(s)).await
}

/// Kurz warten, damit eine (unerwünschte) Prüfung hätte beginnen können.
async fn settle() {
    tokio::time::sleep(Duration::from_millis(300)).await;
}

fn rfc3339_ago(hours: i64) -> String {
    (OffsetDateTime::now_utc() - time::Duration::hours(hours)).format(&Rfc3339).unwrap()
}

fn write_cache(fx: &Fixture, checked_hours_ago: Option<i64>, attempted_hours_ago: i64) {
    std::fs::create_dir_all(&fx.paths.cache_dir).unwrap();
    let json = serde_json::json!({
        "checked_at": checked_hours_ago.map(rfc3339_ago),
        "attempted_at": rfc3339_ago(attempted_hours_ago),
        "releases": [{ "tag": "v0.2.0", "prerelease": true }],
    });
    std::fs::write(fx.paths.cache_dir.join(update::CACHE_FILE), json.to_string()).unwrap();
}

fn apply(core: &Core, revision: u64, on: bool) {
    let changes = std::collections::BTreeMap::from([("general.update_check".to_owned(), serde_json::json!(on))]);
    core.dispatch(UiEvent::ApplySettings { changes, revision });
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn about_view_names_version_release_date_and_links() {
    let fx = fixture("[general]\n");
    let gh = FakeGitHub::new(&[Reply::Json(TODAY)]);
    let core = start(&fx, &gh);
    let v = wait_state(&core, UpdateCheckState::UpToDate).await;
    let about: AboutView = v.about.clone().expect("about");
    assert_eq!(about.version, "0.2.0");
    assert_eq!(about.release_date, update::RELEASE_DATE);
    assert_eq!(about.release_date.len(), 10, "Versionsdatum JJJJ-MM-TT: {}", about.release_date);
    assert_eq!(about.release_notes_url, "https://github.com/a-sit/pida/releases/tag/v0.2.0");
    assert_eq!(about.source_url, "https://github.com/a-sit/pida");
    // gebaut aus Git: das Commit-Datum des gebauten Stands (ohne SOURCE_DATE_EPOCH)
    if std::env::var_os("SOURCE_DATE_EPOCH").is_none()
        && let Ok(out) = std::process::Command::new("git")
            .args(["-C", env!("CARGO_MANIFEST_DIR"), "log", "-1", "--format=%cs"])
            .output()
        && out.status.success()
    {
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), update::RELEASE_DATE, "Commit-Datum");
    }
}

/// Start: eine Prüfung im Hintergrund – die laufende Version ist das neueste Release (Entwürfe zählen nicht); der
/// Request trägt keine Kennungen; das Ergebnis liegt im Zwischenspeicher (nur Zeitpunkte und Versionen).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_check_is_up_to_date_and_carries_no_identifiers() {
    let fx = fixture("[general]\n");
    let gh = FakeGitHub::new(&[Reply::Json(TODAY)]);
    let core = start(&fx, &gh);
    let v = wait_state(&core, UpdateCheckState::UpToDate).await;
    let u = update(&v).unwrap();
    assert_eq!(u.latest_version.as_deref(), Some("0.2.0"), "der Entwurf v0.3.0 zählt nicht");
    assert_eq!((u.release_url, u.reason), (None, None));
    let checked = OffsetDateTime::parse(u.checked_at.as_deref().expect("checked_at"), &Rfc3339).unwrap();
    assert!((OffsetDateTime::now_utc() - checked).whole_seconds().abs() < 60);
    assert_eq!(v.update_hint, None, "auf dem neuesten Stand: kein Hinweis");
    assert!(v.notices.is_empty(), "{:?}", v.notices);
    assert_eq!(gh.calls(), 1);
    // keine Kennungen: feste URL ohne Abfrage, nur zwei Header, Standard-User-Agent, keine Cookies
    let (net, request) = gh.requests.lock().unwrap()[0].clone();
    assert_eq!(request.url.as_str(), RELEASES_API);
    assert!(request.url.query().is_none());
    let headers: Vec<(String, String)> =
        request.headers.iter().map(|(k, v)| (k.to_ascii_lowercase(), v.clone())).collect();
    assert_eq!(
        headers,
        [
            ("accept".to_owned(), "application/vnd.github+json".to_owned()),
            ("x-github-api-version".to_owned(), "2022-11-28".to_owned())
        ]
    );
    assert_eq!(net.user_agent, None, "der übliche User-Agent `PIDA v<Version>`");
    assert_eq!(net.proxy, update::ProxyConfig::System, "Proxy der Einstellungen (System)");
    assert_eq!(net.max_body, update::MAX_RESPONSE_BYTES);
    assert_eq!(net.request_timeout, Some(update::REQUEST_TIMEOUT));
    // Zwischenspeicher: nur Zeitpunkte und Versionen
    let cache: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fx.paths.cache_dir.join(update::CACHE_FILE)).unwrap()).unwrap();
    let mut keys: Vec<&String> = cache.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(keys, ["attempted_at", "checked_at", "releases"]);
    assert_eq!(
        cache["releases"],
        serde_json::json!([{ "tag": "v0.2.0", "prerelease": true }, { "tag": "v0.1.0", "prerelease": true }])
    );
    // „Über PIDA“ gleich danach: Ergebnis jünger als eine Stunde – keine weitere Prüfung
    core.dispatch(UiEvent::CheckForUpdates { explicit: false });
    settle().await;
    assert_eq!(gh.calls(), 1);
}

/// Neue Version beim Start: Hinweis auf der Startseite (keine Meldung), Link auf die Release-Seite; nichts wird
/// geladen. „Ausblenden“ gilt dauerhaft für diese Version (Zwischenspeicher), eine neuere erscheint wieder.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_check_announces_a_new_version() {
    let fx = fixture("[general]\n");
    let gh = FakeGitHub::new(&[Reply::Json(NEWER)]);
    let core = start(&fx, &gh);
    let v = wait_state(&core, UpdateCheckState::Available).await;
    let u = update(&v).unwrap();
    assert_eq!(u.latest_version.as_deref(), Some("0.3.0"));
    assert_eq!(u.release_url.as_deref(), Some("https://github.com/a-sit/pida/releases/tag/v0.3.0"));
    let hint = v.update_hint.clone().expect("Hinweis auf der Startseite");
    assert_eq!(
        (hint.version.as_str(), hint.release_url.as_str()),
        ("0.3.0", "https://github.com/a-sit/pida/releases/tag/v0.3.0")
    );
    assert!(v.notices.is_empty(), "keine Meldung zusätzlich: {:?}", v.notices);
    // während „Jetzt prüfen“ bleibt der Hinweis stehen
    core.dispatch(UiEvent::CheckForUpdates { explicit: true });
    wait_for(&core, "zweite Prüfung", |_| gh.calls() == 2).await;
    wait_state(&core, UpdateCheckState::Available).await;
    assert!(core.view().update_hint.is_some());
    core.dispatch(UiEvent::DismissUpdateHint);
    wait_for(&core, "ausgeblendet", |v| v.update_hint.is_none()).await;
    let cache: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fx.paths.cache_dir.join(update::CACHE_FILE)).unwrap()).unwrap();
    assert_eq!(cache["dismissed_version"], "0.3.0");
    drop(core);
    // nächster Start (Ergebnis aus dem Zwischenspeicher): bleibt ausgeblendet; „Über PIDA“ zeigt die Version weiter
    let gh = FakeGitHub::new(&[]);
    let core = start(&fx, &gh);
    let v = wait_state(&core, UpdateCheckState::Available).await;
    settle().await;
    assert_eq!((core.view().update_hint, gh.calls()), (None, 0), "{:?}", v.notices);
}

/// Höchstens einmal je 24 h beim Start: ein Versuch vor 23 h → keine Prüfung, Ergebnis aus dem Zwischenspeicher;
/// vor 25 h → Prüfung. „Über PIDA“ prüft, wenn das Ergebnis älter als eine Stunde ist.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn startup_check_at_most_once_per_day() {
    let fx = fixture("[general]\n");
    write_cache(&fx, Some(23), 23);
    let gh = FakeGitHub::new(&[Reply::Json(NEWER)]);
    let core = start(&fx, &gh);
    let v = wait_state(&core, UpdateCheckState::UpToDate).await;
    settle().await;
    assert_eq!(gh.calls(), 0, "Versuch vor 23 h: beim Start keine Prüfung");
    assert!(update(&v).unwrap().checked_at.is_some(), "Zeitpunkt aus dem Zwischenspeicher");
    // „Über PIDA“ öffnen: das Ergebnis ist 23 h alt → erneut prüfen (findet 0.3.0, auch die Startseite zeigt es)
    core.dispatch(UiEvent::CheckForUpdates { explicit: false });
    let v = wait_state(&core, UpdateCheckState::Available).await;
    assert_eq!(gh.calls(), 1);
    assert_eq!(v.update_hint.map(|h| h.version).as_deref(), Some("0.3.0"));
    drop(core);

    let fx = fixture("[general]\n");
    write_cache(&fx, Some(25), 25);
    let gh = FakeGitHub::new(&[Reply::Json(TODAY)]);
    let core = start(&fx, &gh);
    wait_for(&core, "Prüfung beim Start", |_| gh.calls() == 1).await;
    wait_state(&core, UpdateCheckState::UpToDate).await;
    drop(core);

    // ein erfolgloser Versuch vor einer Stunde zählt ebenso (kein Ergebnis → „noch nicht geprüft“)
    let fx = fixture("[general]\n");
    write_cache(&fx, None, 1);
    let gh = FakeGitHub::new(&[Reply::Json(TODAY)]);
    let core = start(&fx, &gh);
    let v = wait_state(&core, UpdateCheckState::Unchecked).await;
    settle().await;
    assert_eq!(gh.calls(), 0);
    assert_eq!(update(&v).unwrap().checked_at, None);
    core.dispatch(UiEvent::CheckForUpdates { explicit: false });
    wait_state(&core, UpdateCheckState::UpToDate).await;
    assert_eq!(gh.calls(), 1);
}

/// Schalter aus: keine automatische Prüfung (weder beim Start noch beim Öffnen), „Jetzt prüfen“ prüft einmalig und
/// zeigt das Ergebnis; ein Wechsel des Schalters gilt sofort.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn switch_off_checks_only_on_explicit_click() {
    let fx = fixture("[general]\nupdate_check = false\n");
    write_cache(&fx, Some(30), 30);
    let gh = FakeGitHub::new(&[Reply::Json(TODAY)]);
    let core = start(&fx, &gh);
    let v = wait_state(&core, UpdateCheckState::Disabled).await;
    assert_eq!(update(&v).unwrap().latest_version, None, "kein altes Ergebnis bei ausgeschalteter Prüfung");
    core.dispatch(UiEvent::CheckForUpdates { explicit: false });
    settle().await;
    assert_eq!(gh.calls(), 0, "ausgeschaltet: weder beim Start noch beim Öffnen von „Über PIDA“");
    core.dispatch(UiEvent::CheckForUpdates { explicit: true });
    wait_state(&core, UpdateCheckState::UpToDate).await;
    assert_eq!(gh.calls(), 1);
    // Schalter an (Ergebnis bleibt) und wieder aus („Automatische Prüfung ist ausgeschaltet“)
    core.dispatch(UiEvent::OpenSettings);
    wait_for(&core, "Einstellungen", |v| v.settings.is_some()).await;
    apply(&core, 1, true);
    wait_for(&core, "an", |v| v.settings.as_ref().is_some_and(|s| s.settings.general.update_check)).await;
    assert_eq!(state(&core.view()), Some(UpdateCheckState::UpToDate));
    apply(&core, 2, false);
    wait_state(&core, UpdateCheckState::Disabled).await;
    settle().await;
    assert_eq!(gh.calls(), 1, "Umschalten prüft nicht");
}

/// Richtlinie `update_check = false`: nie prüfen, auch nicht auf Klick; `= true` erzwingt die automatische Prüfung.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn policy_decides_before_the_switch() {
    let fx = fixture("[general]\n");
    let gh = FakeGitHub::new(&[Reply::Json(TODAY)]);
    let off = Policy { update_check: Some(false), ..Policy::default() };
    let core = Core::new_with(config(&fx, Platform::Windows, false), CoreOptions { policy: Some(off), ..options(&gh) })
        .unwrap();
    wait_state(&core, UpdateCheckState::PolicyOff).await;
    core.dispatch(UiEvent::CheckForUpdates { explicit: false });
    core.dispatch(UiEvent::CheckForUpdates { explicit: true });
    settle().await;
    assert_eq!(gh.calls(), 0, "Richtlinie „aus“: keine Prüfung, auch nicht auf Klick");
    assert_eq!(state(&core.view()), Some(UpdateCheckState::PolicyOff));
    assert!(!fx.paths.cache_dir.join(update::CACHE_FILE).exists());
    drop(core);

    let fx = fixture("[general]\nupdate_check = false\n");
    let gh = FakeGitHub::new(&[Reply::Json(TODAY)]);
    let on = Policy { update_check: Some(true), ..Policy::default() };
    let core =
        Core::new_with(config(&fx, Platform::MacOs, false), CoreOptions { policy: Some(on), ..options(&gh) }).unwrap();
    wait_state(&core, UpdateCheckState::UpToDate).await;
    assert_eq!(gh.calls(), 1, "Richtlinie „an“ vor der Einstellung");
}

/// Fehlerarten → `failed` mit Grund (nie „auf dem neuesten Stand“), „Erneut prüfen“ prüft erneut; kein Hinweis.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failures_name_a_reason_and_retry_works() {
    let fx = fixture("[general]\n");
    let gh = FakeGitHub::held(&[
        Reply::Error(|| NetError::Timeout),
        Reply::RateLimited,
        Reply::Status(429),
        Reply::Status(403),
        Reply::Status(502),
        Reply::Json("<html>Wartung</html>"),
        Reply::Json(r#"{"message":"Not Found"}"#),
        Reply::Error(|| NetError::Connect("dns error".into())),
        Reply::Error(|| NetError::Tls("invalid peer certificate".into())),
        Reply::Error(|| NetError::Proxy("407 Proxy Authentication Required".into())),
        Reply::Json(TODAY),
    ]);
    let core = start(&fx, &gh);
    let expected = [
        update::MSG_FAILED_TIMEOUT,
        update::MSG_FAILED_RATE_LIMITED,
        update::MSG_FAILED_RATE_LIMITED,
        update::MSG_FAILED_FORBIDDEN,
        update::MSG_FAILED_SERVER,
        update::MSG_FAILED_INVALID,
        update::MSG_FAILED_INVALID,
        update::MSG_FAILED_OFFLINE,
        update::MSG_FAILED_TLS,
        update::MSG_FAILED_PROXY,
    ];
    for (i, reason) in expected.iter().enumerate() {
        // jede Antwort wartet auf die Freigabe: „prüft“ ist sicher zu sehen, danach das neue Ergebnis
        wait_state(&core, UpdateCheckState::Checking).await;
        gh.release();
        let v = wait_state(&core, UpdateCheckState::Failed).await;
        let u = update(&v).unwrap();
        assert_eq!(u.reason.as_deref(), Some(*reason), "Versuch {}", i + 1);
        assert_eq!((u.latest_version, u.checked_at), (None, None), "nie geprüft = nie „aktuell“");
        assert!(v.notices.is_empty(), "Fehler ohne Hinweis: {:?}", v.notices);
        assert_eq!(gh.calls(), i + 1);
        core.dispatch(UiEvent::CheckForUpdates { explicit: true });
    }
    wait_state(&core, UpdateCheckState::Checking).await;
    gh.release();
    let v = wait_state(&core, UpdateCheckState::UpToDate).await;
    assert_eq!(gh.calls(), expected.len() + 1);
    assert_eq!(update(&v).unwrap().reason, None);
    // ein Fehler nach einem Erfolg zeigt den Zeitpunkt der letzten erfolgreichen Prüfung weiter
    let cache: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fx.paths.cache_dir.join(update::CACHE_FILE)).unwrap()).unwrap();
    assert!(cache["checked_at"].is_string() && cache["attempted_at"].is_string());
}

/// „Suche nach Updates …“ während der Prüfung; der Kern bleibt bedienbar und startet keine zweite Prüfung.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn checking_is_visible_and_never_blocks() {
    let fx = fixture("[general]\n");
    let gh = FakeGitHub::held(&[Reply::Json(NEWER)]);
    let core = start(&fx, &gh);
    let v = wait_state(&core, UpdateCheckState::Checking).await;
    assert!(!matches!(v.step, pida_core_api::StepView::Starting), "Start wartet nicht auf die Prüfung");
    core.dispatch(UiEvent::OpenSettings);
    let v = wait_for(&core, "Einstellungen trotz Prüfung", |v| v.settings.is_some()).await;
    assert_eq!(state(&v), Some(UpdateCheckState::Checking));
    core.dispatch(UiEvent::CheckForUpdates { explicit: true });
    core.dispatch(UiEvent::CheckForUpdates { explicit: false });
    settle().await;
    assert_eq!(gh.calls(), 1, "keine zweite Prüfung während der ersten");
    gh.release();
    wait_state(&core, UpdateCheckState::Available).await;
}

/// Nach einem Fehler prüft nicht jedes Öffnen von „Über PIDA“ erneut (Rate-Limit: weitere Anfragen verlängern die
/// Sperre nur; offline: nichts zu gewinnen) – der Zustand „nicht möglich“ bleibt mit „Erneut prüfen“ stehen, das
/// jederzeit prüft. Ein altes Ergebnis aus dem Zwischenspeicher wird dagegen beim Öffnen erneuert, auch wenn der
/// letzte Versuch (in einer früheren Sitzung) erst kurz zurückliegt.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_checks_are_not_repeated_on_every_open() {
    for (first, reason) in [
        (Reply::RateLimited, update::MSG_FAILED_RATE_LIMITED),
        (Reply::Error(|| NetError::Connect("dns error".into())), update::MSG_FAILED_OFFLINE),
    ] {
        let fx = fixture("[general]\n");
        let gh = FakeGitHub::new(&[first, Reply::Json(TODAY)]);
        let core = start(&fx, &gh);
        let v = wait_state(&core, UpdateCheckState::Failed).await;
        assert_eq!(update(&v).unwrap().reason.as_deref(), Some(reason));
        for _ in 0..3 {
            core.dispatch(UiEvent::CheckForUpdates { explicit: false });
        }
        settle().await;
        assert_eq!(gh.calls(), 1, "{reason}: Öffnen nach einem Fehler prüft nicht sofort erneut");
        assert_eq!(state(&core.view()), Some(UpdateCheckState::Failed));
        core.dispatch(UiEvent::CheckForUpdates { explicit: true });
        wait_state(&core, UpdateCheckState::UpToDate).await;
        assert_eq!(gh.calls(), 2, "{reason}: „Erneut prüfen“ prüft");
    }

    // Erfolg vor 10 Tagen, erfolgloser Versuch vor 30 Minuten (frühere Sitzung): beim Start keine Prüfung, aber das
    // Öffnen erneuert das alte „auf dem neuesten Stand“
    let fx = fixture("[general]\n");
    std::fs::create_dir_all(&fx.paths.cache_dir).unwrap();
    let ago = |minutes: i64| (OffsetDateTime::now_utc() - time::Duration::minutes(minutes)).format(&Rfc3339).unwrap();
    let cache = serde_json::json!({
        "checked_at": ago(10 * 24 * 60),
        "attempted_at": ago(30),
        "releases": [{ "tag": "v0.2.0", "prerelease": true }],
    });
    std::fs::write(fx.paths.cache_dir.join(update::CACHE_FILE), cache.to_string()).unwrap();
    let gh = FakeGitHub::new(&[Reply::Json(NEWER)]);
    let core = start(&fx, &gh);
    wait_state(&core, UpdateCheckState::UpToDate).await;
    settle().await;
    assert_eq!(gh.calls(), 0, "Versuch vor 30 Minuten: beim Start keine Prüfung");
    core.dispatch(UiEvent::CheckForUpdates { explicit: false });
    wait_state(&core, UpdateCheckState::Available).await;
    assert_eq!(gh.calls(), 1);
}

/// Schalter während der Prüfung beim Start ausgeschaltet: kein Hinweis auf die neue Version.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_startup_notice_after_the_switch_was_turned_off() {
    let fx = fixture("[general]\n");
    let gh = FakeGitHub::held(&[Reply::Json(NEWER)]);
    let core = start(&fx, &gh);
    wait_state(&core, UpdateCheckState::Checking).await;
    core.dispatch(UiEvent::OpenSettings);
    wait_for(&core, "Einstellungen", |v| v.settings.is_some()).await;
    apply(&core, 1, false);
    wait_for(&core, "aus", |v| v.settings.as_ref().is_some_and(|s| !s.settings.general.update_check)).await;
    gh.release();
    wait_state(&core, UpdateCheckState::Disabled).await;
    settle().await;
    assert_eq!(gh.calls(), 1);
    assert_eq!(core.view().update_hint, None, "ausgeschaltet: kein Hinweis");
}

/// iOS/Android (keine Verteilung über GitHub) und die CLI prüfen nie; ohne Transport (Tests) ebenso.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mobile_cli_and_isolated_cores_never_check() {
    for platform in [Platform::Ios, Platform::Android] {
        let fx = fixture("[general]\n");
        let gh = FakeGitHub::new(&[Reply::Json(NEWER)]);
        let core = Core::new_with(config(&fx, platform, false), options(&gh)).unwrap();
        let v = wait_for(&core, "Projektion", |v| v.about.is_some()).await;
        let about = v.about.unwrap();
        assert_eq!((about.version.as_str(), about.update), ("0.2.0", None), "{platform:?}: keine Update-Zeile");
        assert_eq!(v.update_hint, None, "{platform:?}: kein Hinweis auf der Startseite");
        core.dispatch(UiEvent::CheckForUpdates { explicit: true });
        settle().await;
        assert_eq!(gh.calls(), 0, "{platform:?}");
    }
    let fx = fixture("[general]\n");
    let gh = FakeGitHub::new(&[Reply::Json(NEWER)]);
    let core = Core::new_with(config(&fx, Platform::Linux, true), options(&gh)).unwrap();
    core.dispatch(UiEvent::CheckForUpdates { explicit: true });
    settle().await;
    assert_eq!(gh.calls(), 0, "CLI (ohne Oberfläche)");
    assert_eq!(core.view().about.and_then(|a| a.update), None);
    drop(core);
    let fx = fixture("[general]\n");
    let core = Core::new_with(config(&fx, Platform::Linux, false), CoreOptions::isolated()).unwrap();
    let v = wait_for(&core, "Projektion", |v| v.about.is_some()).await;
    assert_eq!(v.about.unwrap().update, None, "CoreOptions::isolated: kein Netz");
}

// ------------------------------------------------------------------------------------------------
// Zustände des echten Kerns für die Oberfläche
// ------------------------------------------------------------------------------------------------

const UI_FIXTURE: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../apps/app/src/core/fixtures/real-core-about-views.json");

/// Probe für die Datei: Zeitpunkte und Versionsdatum fest, Sprache Deutsch.
fn fixture_state(v: &ViewState) -> serde_json::Value {
    let mut value = serde_json::to_value(v).expect("ViewState serialisierbar");
    value["language"] = serde_json::Value::String("de".into());
    if let Some(about) = value.get_mut("about").filter(|a| a.is_object()) {
        about["release_date"] = serde_json::Value::String("2026-10-17".into());
        if about["update"]["checked_at"].is_string() {
            about["update"]["checked_at"] = serde_json::Value::String("2026-10-17T10:42:17+02:00".into());
        }
    }
    value
}

/// `real-core-about-views.json` für `apps/app/src/features/settings/about.test.tsx`: jeder Zustand der Prüfung am
/// Desktop und „Über PIDA“ auf iOS. Neu schreiben: `PIDA_BLESS=1 cargo test -p pida-core-api --test about
/// real_core_about_views`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_core_about_views_for_the_ui() {
    let mut states: Vec<(&str, ViewState)> = Vec::new();
    let open = |core: &Core| core.dispatch(UiEvent::OpenSettings);

    let fx = fixture("[general]\n");
    let gh = FakeGitHub::held(&[Reply::Json(TODAY)]);
    let core = start(&fx, &gh);
    open(&core);
    states.push((
        "about_checking",
        wait_for(&core, "prüft", |v| v.settings.is_some() && state(v) == Some(UpdateCheckState::Checking)).await,
    ));
    gh.release();
    states.push(("about_up_to_date", wait_state(&core, UpdateCheckState::UpToDate).await));
    drop(core);

    let fx = fixture("[general]\n");
    let gh = FakeGitHub::new(&[Reply::Json(NEWER)]);
    let core = start(&fx, &gh);
    open(&core);
    states.push((
        "about_available",
        wait_for(&core, "neu", |v| v.settings.is_some() && state(v) == Some(UpdateCheckState::Available)).await,
    ));
    drop(core);

    let fx = fixture("[general]\n");
    let gh = FakeGitHub::new(&[Reply::RateLimited]);
    let core = start(&fx, &gh);
    open(&core);
    states.push((
        "about_failed",
        wait_for(&core, "Fehler", |v| v.settings.is_some() && state(v) == Some(UpdateCheckState::Failed)).await,
    ));
    drop(core);

    let fx = fixture("[general]\nupdate_check = false\n");
    let gh = FakeGitHub::new(&[Reply::Json(TODAY)]);
    let core = start(&fx, &gh);
    open(&core);
    states.push((
        "about_disabled",
        wait_for(&core, "aus", |v| v.settings.is_some() && state(v) == Some(UpdateCheckState::Disabled)).await,
    ));
    drop(core);

    let fx = fixture("[general]\n");
    write_cache(&fx, None, 1);
    let gh = FakeGitHub::new(&[Reply::Json(TODAY)]);
    let core = start(&fx, &gh);
    open(&core);
    states.push((
        "about_unchecked",
        wait_for(&core, "ungeprüft", |v| v.settings.is_some() && state(v) == Some(UpdateCheckState::Unchecked)).await,
    ));
    drop(core);

    let fx = fixture("[general]\n");
    let gh = FakeGitHub::new(&[Reply::Json(TODAY)]);
    let off = Policy { update_check: Some(false), ..Policy::default() };
    let core =
        Core::new_with(config(&fx, Platform::Linux, false), CoreOptions { policy: Some(off), ..options(&gh) }).unwrap();
    open(&core);
    states.push((
        "about_policy_off",
        wait_for(&core, "Richtlinie", |v| v.settings.is_some() && state(v) == Some(UpdateCheckState::PolicyOff)).await,
    ));
    drop(core);

    let fx = fixture("[general]\n");
    let gh = FakeGitHub::new(&[Reply::Json(TODAY)]);
    let core = Core::new_with(config(&fx, Platform::Ios, false), options(&gh)).unwrap();
    open(&core);
    states.push(("about_ios", wait_for(&core, "iOS", |v| v.settings.is_some() && v.about.is_some()).await));
    drop(core);

    for (name, v) in &states {
        assert!(v.first_run.is_none(), "{name}: ohne Ersteinrichtung");
        assert_eq!(v.about.as_ref().map(|a| a.version.as_str()), Some("0.2.0"), "{name}");
    }
    let fixture: serde_json::Map<String, serde_json::Value> =
        states.iter().map(|(name, v)| ((*name).to_owned(), fixture_state(v))).collect();
    if std::env::var_os("PIDA_BLESS").is_some() {
        let text = serde_json::to_string_pretty(&fixture).unwrap() + "\n";
        std::fs::write(UI_FIXTURE, text).expect("Fixture schreiben");
        return;
    }
    let text = std::fs::read_to_string(UI_FIXTURE).expect("Fixture lesen (PIDA_BLESS=1 erzeugt sie)");
    let stored: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&text).unwrap();
    for (name, value) in &fixture {
        let s = stored
            .get(name)
            .unwrap_or_else(|| panic!("real-core-about-views.json ohne {name}: PIDA_BLESS=1 neu erzeugen"));
        assert_eq!(s["about"], value["about"], "{name}: real-core-about-views.json veraltet (PIDA_BLESS=1)");
        let _: ViewState =
            serde_json::from_value(s.clone()).unwrap_or_else(|e| panic!("{name}: passt nicht zum Vertrag ({e})"));
    }
}

// ------------------------------------------------------------------------------------------------
// Online (nur mit PIDA_ONLINE_TESTS=1)
// ------------------------------------------------------------------------------------------------

/// Einziger Test gegen das echte `api.github.com` (öffentlich, ohne Anmeldung): Antwort lesbar, das Release v0.2.0
/// ist dabei, die Bewertung für 0.2.0 gelingt. Ohne `PIDA_ONLINE_TESTS=1` übersprungen.
#[tokio::test]
async fn github_releases_online() {
    if std::env::var_os("PIDA_ONLINE_TESTS").is_none_or(|v| v != "1") {
        eprintln!("PIDA_ONLINE_TESTS=1 nicht gesetzt – Online-Test übersprungen");
        return;
    }
    let releases =
        update::fetch_releases(&NetTransport, update::ProxyConfig::System).await.expect("GitHub-Releases lesbar");
    let tags: Vec<String> = releases.iter().map(|r| r.tag()).collect();
    assert!(tags.iter().any(|t| t == "v0.2.0"), "{tags:?}");
    let current = update::Version::parse("0.2.0").unwrap();
    let evaluation = update::evaluate(&current, &releases);
    eprintln!("GitHub: {} Releases, Bewertung für 0.2.0: {evaluation:?}", releases.len());
}
