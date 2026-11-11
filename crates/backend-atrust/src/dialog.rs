//! A-Trust-Dialog je SL-Request (docs/analyse/02 §3.3–§3.12, `MobileBKUConnector`).
//!
//! `handle_sl_request` sendet den Initial-Request (R1a urlencoded / R1b multipart) mit einem neuen
//! Client und leerem Cookie-Jar, folgt 301/302 (R2), `Refresh` (R3) und `<meta refresh>` (R4),
//! klassifiziert jede HTML-Seite und bestimmt mit `present` den nächsten Request (R5–R7), bis eine
//! Antwort mit MIME `…/xml` kommt (die SL-Antwort).
//!
//! **Erneuerung der Sitzung** (ADR 0004 Nachtrag 19): Statt eines Requests kann `present` eine neue
//! A-Trust-Sitzung verlangen ([`Next::Renew`]) – dann beginnt der SL-Request mit neuem Client und leerem
//! Cookie-Jar von vorn (Initial-Request an den Endpunkt), und die Login-Seite der neuen Sitzung sendet die
//! Zugangsdaten aus dem RAM automatisch wie im zweiten SL-Request. Das geschieht höchstens **einmal** je
//! SL-Request ([`Renewal`]) und nur nach einem **ausdrücklichen** Absenden der Anmeldedaten in der Maske:
//! `SessionClosed.aspx` als Antwort darauf, oder die Maske stand länger als
//! [`crate::selectors::SelectorSet::login_page_max_age_secs`] (dann vor dem Absenden). Nach SMS-TAN,
//! App-/QR-Bestätigung oder FIDO2 nie (dort wäre eine neue Bestätigung nötig): dann
//! [`BackendError::SessionClosed`].

use std::sync::{Arc, Mutex};
use std::time::Instant;

use pida_backend_api::{
    BackendError, BackendEvent, CertChain, Certificate, FormAnswer, FormContext, FormProblem, InteractionBroker,
    InteractionError, NativeForm, Notice, NoticeAnswer, NoticeButtons, NoticeCode, NoticeKind, Progress, QrImage,
    SecretKind, SecretPrompt, SessionContext, SignJob, SignResult,
};
use pida_net::{Body, FormPart, HttpClient, NetError, Purpose, Request, Response, Url};
use pida_sl12::{CreateCmsRequest, DocumentContent, HttpBody, PartOrder, SlRequest, SlResponse};
use zeroize::Zeroizing;

use crate::page::{self, PageBlock, ParsedPage, SubmitButton};
use crate::poll::{self, PollInfo, PollOutcome};
use crate::{AtrustConfig, Credentials, Fido2Error, Fido2Request, LoopGuard, WEBAUTHN_ORIGIN, fido2, validate};

/// Antwort von `send`: SL-Antwort oder klassifizierte Seite (mit der URL, von der sie geladen wurde –
/// anders als [`ParsedPage::base_url`] nicht durch `<base href>` beeinflussbar – und dem Zeitpunkt des
/// Empfangs nach [`crate::AtrustConfig::clock`]).
enum Reply {
    Xml(Response),
    Page(Box<ParsedPage>, Url, Instant),
}

/// Nächster Schritt nach einer Seite: Request in derselben A-Trust-Sitzung oder eine neue Sitzung
/// (Nachtrag 19, mit Grund fürs Log).
enum Next {
    Request(Request),
    Renew(&'static str),
}

/// Automatische Erneuerung der A-Trust-Sitzung (Nachtrag 19): höchstens **eine** je SL-Request.
#[derive(Debug, Default)]
struct Renewal {
    used: bool,
}

impl Renewal {
    /// Beansprucht die Erneuerung – `true` nur beim ersten Aufruf je SL-Request.
    fn take(&mut self) -> bool {
        !std::mem::replace(&mut self.used, true)
    }
}

/// Login-Zustand innerhalb eines SL-Requests (eine A-Trust-Sitzung).
#[derive(Debug, Default, Clone, Copy)]
struct LoginState {
    /// Zugangsdaten wurden in dieser A-Trust-Sitzung gesendet.
    submitted: bool,
    /// Danach kam eine Seite ohne Login- und Fehlerblock (Anmeldung erfolgreich).
    confirmed: bool,
    /// Automatisch übersprungene Seiten (AutoSkip) in dieser A-Trust-Sitzung.
    auto_skips: u32,
    /// Vom Long-Poll ausgelöste Neuladungen (R7) in dieser A-Trust-Sitzung.
    poll_reloads: u32,
    /// Der zuletzt gesendete Request war das **ausdrückliche** Absenden der Anmeldedaten aus der Maske
    /// (Voraussetzung der Erneuerung nach `SessionClosed.aspx`, Nachtrag 19) – nicht bei der automatischen
    /// Anmeldung. Gilt nur für die **unmittelbar** folgende Seite (samt Weiterleitungen): `present` setzt es
    /// bei jeder Seite zurück, damit z. B. „Wiederholen“ bzw. „Abbrechen“ auf einer Fehlerseite oder der
    /// Abbruch einer erneut gezeigten Maske nie eine neue Sitzung startet.
    entered: bool,
    /// In dieser A-Trust-Sitzung wurde der Abbruch an A-Trust gemeldet (`Button_Cancel` aus Maske oder
    /// Fehlerseite): eine danach beendete Sitzung ist der Abbruch der Nutzerin, keine Erneuerung (F-54).
    cancel_sent: bool,
}

/// Höchstzahl automatisch verfolgter Weiterleitungen (301/302, `Refresh`, `<meta refresh>`) für
/// eine Seite. Ergänzt den Loop-Guard aus PDF-Over (50 Requests in 2 s), der bei einer langsamen
/// Gegenstelle (> 40 ms je Antwort) nie auslöst; Browser begrenzen ähnlich (≈ 20).
const MAX_FOLLOWS_PER_PAGE: u32 = 100;

/// Höchstzahl AutoSkip-Seiten je SL-Request (regulär 1–2): verhindert, dass eine Seite, die sich
/// nach dem Absenden selbst wieder liefert, A-Trust ohne Nutzeraktion dauerhaft belastet.
const MAX_AUTO_SKIPS: u32 = 20;

/// Höchstzahl vom Long-Poll ausgelöster Neuladungen je SL-Request (regulär 1; jede weitere folgt
/// frühestens nach „fertig“, Fehler-Backoff oder der 300-s-Frist). Meldet der Poll dauerhaft „fertig“,
/// ohne dass sich die Seite ändert, entstünde sonst eine Schleife ohne Nutzeraktion.
const MAX_POLL_RELOADS: u32 = 30;

/// Zustand eines Signaturvorgangs über beide SL-Requests.
pub(crate) struct Dialog {
    config: AtrustConfig,
    remembered: Arc<Mutex<Credentials>>,
    ui: Arc<dyn InteractionBroker>,
    ctx: SessionContext,
    fido2_available: bool,
    remember: bool,
    fido2_default_pending: bool,
    announced_login: Option<String>,
    /// HTTP 407/413/sonstige ohne eigene Meldung als spezifischen Fehler melden (`sign_one`, ADR 0004
    /// §3.8): sonst käme nach der Meldung `Cancelled`, das der Kern als Ablehnung deutete
    quiet_fatal: bool,
}

impl std::fmt::Debug for Dialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dialog")
            .field("endpoint", &pida_net::redact_url(&self.config.endpoint))
            .field("fido2_available", &self.fido2_available)
            .field("remember", &self.remember)
            .field("fido2_default_pending", &self.fido2_default_pending)
            .finish_non_exhaustive()
    }
}

fn to_net_body(body: HttpBody) -> Body {
    match body {
        HttpBody::UrlEncoded(fields) => Body::Form(fields),
        HttpBody::Multipart(parts) => Body::Multipart(
            parts
                .into_iter()
                .map(|p| FormPart { name: p.name, filename: p.filename, content_type: p.content_type, data: p.data })
                .collect(),
        ),
    }
}

/// R5: alle Formularfelder als Textteile plus genau ein „geklickter“ Button (immer multipart).
fn form_submit(page: &ParsedPage, button: Option<&SubmitButton>) -> Request {
    let mut parts: Vec<FormPart> =
        page.form_fields.iter().map(|(k, v)| FormPart::text(k.as_str(), v.as_str())).collect();
    if let Some(b) = button {
        parts.push(FormPart::text(b.name.as_str(), b.value.as_str()));
    }
    Request::post(page.form_action.clone(), Body::Multipart(parts))
}

fn net_to_backend(error: NetError) -> BackendError {
    match error {
        NetError::Connect(m) => BackendError::Network(m),
        NetError::Timeout => BackendError::Timeout,
        NetError::Tls(m) => BackendError::Network(format!("TLS: {m}")),
        NetError::Proxy(m) => BackendError::Network(format!("Proxy: {m}")),
        NetError::InvalidUrl(m) => BackendError::Protocol(format!("ungültige URL: {m}")),
        NetError::Body(m) => BackendError::Protocol(m),
        NetError::Config(m) => BackendError::Internal(m),
        NetError::NoResponse => BackendError::Network("keine Antwort des Signaturdienstes".into()),
        NetError::ConnectionLost(m) => BackendError::Network(m),
    }
}

fn resolve(base: &Url, href: &str) -> Result<Url, BackendError> {
    pida_net::resolve(base, href).map_err(net_to_backend)
}

/// Erster `<meta http-equiv="refresh">` (Attributwert ohne Groß-/Kleinschreibung) → `content`.
fn meta_refresh_content(html: &str) -> Option<String> {
    let doc = scraper::Html::parse_document(html);
    let sel = scraper::Selector::parse("meta[http-equiv]").ok()?;
    doc.select(&sel)
        .find(|m| m.attr("http-equiv").is_some_and(|v| v.eq_ignore_ascii_case("refresh")))
        .map(|m| m.attr("content").unwrap_or_default().to_owned())
}

/// `https://` zu `a-trust.at` bzw. einer Subdomain davon.
fn is_atrust_https(url: &Url) -> bool {
    url.scheme() == "https"
        && url
            .host_str()
            .is_some_and(|h| h.eq_ignore_ascii_case("a-trust.at") || h.to_ascii_lowercase().ends_with(".a-trust.at"))
}

/// Link „Signaturdaten anzeigen“ für den Systembrowser: **nur `https://`** (ADR 0001 §8.6) zu A-Trust
/// bzw. zum Host des konfigurierten Endpunkts.
pub(crate) fn allowed_external(url: &Url, endpoint: &Url) -> bool {
    is_atrust_https(url) || (url.scheme() == "https" && url.host_str() == endpoint.host_str())
}

/// Ziel für Zugangsdaten, TAN und FIDO2-Assertion: `https://` zu einer der vertrauten Domains der
/// Selektor-Datei (`a-trust.at`, `handy-signatur.at` samt Subdomains) oder exakt die Origin des
/// konfigurierten Endpunkts (Schema, Host, Port – erlaubt den lokalen Mock-Endpunkt der Tests).
pub(crate) fn trusted_target(url: &Url, endpoint: &Url, selectors: &crate::selectors::SelectorSet) -> bool {
    let same_origin = url.scheme() == endpoint.scheme()
        && url.host_str() == endpoint.host_str()
        && url.port_or_known_default() == endpoint.port_or_known_default();
    same_origin || (url.scheme() == "https" && url.host_str().is_some_and(|h| selectors.is_trusted_host(h)))
}

/// `scheme://host[:port]` ohne Pfad und Abfrage (Diagnose).
fn origin_of(url: &Url) -> String {
    match url.port() {
        Some(port) => format!("{}://{}:{port}", url.scheme(), url.host_str().unwrap_or("")),
        None => format!("{}://{}", url.scheme(), url.host_str().unwrap_or("")),
    }
}

/// Bricht einen laufenden FIDO2-Vorgang ab, wenn das Future gedroppt wird.
struct CancelFido2OnDrop {
    provider: Option<Arc<dyn fido2::Fido2Provider>>,
}

impl CancelFido2OnDrop {
    fn disarm(&mut self) {
        self.provider = None;
    }
}

impl Drop for CancelFido2OnDrop {
    fn drop(&mut self) {
        if let Some(p) = self.provider.take() {
            p.cancel();
        }
    }
}

impl Dialog {
    pub(crate) fn new(
        config: AtrustConfig,
        remembered: Arc<Mutex<Credentials>>,
        ui: Arc<dyn InteractionBroker>,
        ctx: SessionContext,
        fido2_available: bool,
    ) -> Self {
        let remember = config.remember;
        let fido2_default_pending = config.fido2_default && fido2_available;
        Self {
            config,
            remembered,
            ui,
            ctx,
            fido2_available,
            remember,
            fido2_default_pending,
            announced_login: None,
            quiet_fatal: false,
        }
    }

    /// HTTP-Fehler ohne eigene Meldung melden (siehe [`Dialog::fatal`]).
    pub(crate) fn set_quiet_fatal(&mut self, quiet: bool) {
        self.quiet_fatal = quiet;
    }

    /// Löscht das Passwort der Sitzung und des „Merken“-Speichers (Benutzername bleibt).
    pub(crate) fn clear_password(&self, credentials: &mut Credentials) {
        credentials.password = None;
        self.remembered.lock().expect("poisoned").password = None;
    }

    // --------------------------------------------------------------------------------------------
    // Öffentliche Abläufe

    pub(crate) async fn certificate(
        &mut self,
        credentials: &mut Credentials,
        guard: &mut LoopGuard,
    ) -> Result<CertChain, BackendError> {
        self.ui.progress(Progress::ReadingCertificate);
        match self.handle_sl_request(credentials, guard, &SlRequest::InfoboxReadCertificates, None).await? {
            SlResponse::InfoboxRead(response) => {
                let der = response
                    .signature_certificate()
                    .ok_or_else(|| BackendError::Protocol("InfoboxReadResponse ohne SecureSignatureKeypair".into()))?;
                let certificate = Certificate::from_der(der)
                    .map_err(|e| BackendError::Protocol(format!("Signaturzertifikat nicht lesbar: {e}")))?;
                Ok(CertChain::new(certificate))
            }
            SlResponse::Error(e) => Err(sl_error(e)),
            other => Err(BackendError::Protocol(format!("unerwartete SL-Antwort auf InfoboxRead: {}", kind(&other)))),
        }
    }

    pub(crate) async fn sign(
        &mut self,
        credentials: &mut Credentials,
        guard: &mut LoopGuard,
        jobs: Vec<SignJob>,
    ) -> Result<Vec<SignResult>, BackendError> {
        let total = jobs.len();
        let mut results = Vec::with_capacity(total);
        for (index, job) in jobs.into_iter().enumerate() {
            if total > 1 {
                self.ui.progress(Progress::Document { index: index + 1, total });
            }
            let SignJob::Document(job) = job else {
                return Err(BackendError::Internal("ID Austria (Szenario A) signiert nur Dokumente (Modell B)".into()));
            };
            let data = job
                .source
                .blacked_out_bytes()
                .map_err(|e| BackendError::Internal(format!("vorbereitetes Dokument nicht lesbar: {e}")))?;
            let range = job.source.excluded_range();
            drop(job); // `DocumentJob::source` nicht über den Request hinaus halten (ADR 0001 §8.5)
            let request = SlRequest::CreateCmsSignature(CreateCmsRequest::new(
                DocumentContent::Upload,
                pida_sl12::ExcludedByteRange { from: range.from, to: range.to },
            ));
            self.ui.progress(Progress::Signing);
            match self.handle_sl_request(credentials, guard, &request, Some(data)).await? {
                SlResponse::CreateCmsSignature(r) => results.push(SignResult::Cms(r.cms)),
                SlResponse::Error(e) => return Err(sl_error(e)),
                other => {
                    return Err(BackendError::Protocol(format!(
                        "unerwartete SL-Antwort auf CreateCMSSignature: {}",
                        kind(&other)
                    )));
                }
            }
        }
        self.ui.progress(Progress::Done);
        Ok(results)
    }

    // --------------------------------------------------------------------------------------------
    // Transport

    async fn cancellable<F: std::future::Future>(&self, future: F) -> Result<F::Output, BackendError> {
        let cancel = self.ctx.cancel.clone();
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(BackendError::Cancelled),
            out = future => Ok(out),
        }
    }

    /// Ein SL-Request. Je A-Trust-Sitzung ein neuer Client mit leerem Cookie-Jar; verlangt `present` eine
    /// neue Sitzung ([`Next::Renew`], höchstens einmal, [`Renewal`]), beginnt der Request mit dem
    /// Initial-Request von vorn. Das Dokument bleibt dafür bis zum Ende des SL-Requests im RAM.
    async fn handle_sl_request(
        &mut self,
        credentials: &mut Credentials,
        guard: &mut LoopGuard,
        request: &SlRequest,
        upload: Option<Vec<u8>>,
    ) -> Result<SlResponse, BackendError> {
        let mut renewal = Renewal::default();
        loop {
            self.ui.progress(Progress::Connecting);
            let client = HttpClient::new(&self.config.net, Purpose::AtrustSession).map_err(net_to_backend)?;
            let body = pida_sl12::http_body(request, upload.as_deref(), PartOrder::FileFirst);
            let mut next = Request::post(self.config.endpoint.clone(), to_net_body(body));
            let mut login = LoginState::default();
            let reason = loop {
                match self.send(&client, guard, next).await? {
                    Reply::Xml(response) => {
                        if let Some(date) = response.date() {
                            self.ui.event(BackendEvent::ServerTime(date));
                        }
                        let text = response.text();
                        let parsed = pida_sl12::parse_response(text.as_bytes())
                            .map_err(|e| BackendError::Protocol(format!("SL-Antwort nicht lesbar: {e}")))?;
                        // Identität ohne zweiten Faktor (z. B. Test-Identitäten von A-Trust): auf die Anmeldung folgt
                        // gleich die SL-Antwort, keine Seite dazwischen – die Anmeldung war erfolgreich („Merken“,
                        // Ereignis an den Kern wie nach einer Bestätigungsseite)
                        if login.submitted
                            && !login.confirmed
                            && !matches!(parsed, SlResponse::Error(_))
                            && let (Some(user), Some(password)) = (&credentials.username, &credentials.password)
                        {
                            self.login_succeeded(user.clone(), password.clone());
                        }
                        return Ok(parsed);
                    }
                    Reply::Page(page, page_url, received) => {
                        match self.present(credentials, *page, &page_url, received, &mut login, &mut renewal).await? {
                            Next::Request(request) => next = request,
                            Next::Renew(reason) => break reason,
                        }
                        self.ui.progress(Progress::WaitingForServer);
                    }
                }
            };
            tracing::info!(reason, "A-Trust-Sitzung wird einmalig erneuert (neuer Initial-Request)");
        }
    }

    /// `sendHTTPRequest`: Loop-Guard, 301/302, Status, `Refresh`, MIME, `<meta refresh>`, Parser.
    async fn send(
        &mut self,
        client: &HttpClient,
        guard: &mut LoopGuard,
        mut request: Request,
    ) -> Result<Reply, BackendError> {
        let mut follows = 0u32;
        loop {
            guard.check(Instant::now())?;
            follows += 1;
            if follows > MAX_FOLLOWS_PER_PAGE + 1 {
                tracing::warn!("zu viele Weiterleitungen – Abbruch");
                return Err(BackendError::LoopDetected);
            }
            // Jede Anfrage der Sitzung (Erstanfrage, Weiterleitung, `Refresh`, Formularziel) geht nur an A-Trust
            // bzw. die Origin des konfigurierten Endpunkts: die Sitzung trägt Cookies, Formulare tragen
            // Zugangsdaten, und die Antwort (Seite oder Security-Layer-XML) wird als die von A-Trust angenommen.
            self.ensure_trusted_url(&request.url, "Anfrage")?;
            tracing::debug!(method = request.method.as_str(), url = %pida_net::redact_url(&request.url), "A-Trust-Request");
            // Basis für relative Ziele ist die URI des auslösenden Requests (Körper nicht kopieren)
            let url = request.url.clone();
            let response = match self.cancellable(client.execute(request)).await? {
                Ok(r) => r,
                Err(e) if e.is_proxy_auth_required() => {
                    return Err(self.fatal(NoticeCode::ProxyAuthRequired, None, BackendError::ProxyAuthRequired).await);
                }
                Err(e) => return Err(net_to_backend(e)),
            };
            match response.status {
                301 | 302 => {
                    let location = response
                        .header("location")
                        .ok_or_else(|| BackendError::Protocol("HTTP-Weiterleitung ohne Location-Header".into()))?;
                    request = Request::get(resolve(&url, location)?);
                    continue;
                }
                200 => {}
                407 => {
                    return Err(self.fatal(NoticeCode::ProxyAuthRequired, None, BackendError::ProxyAuthRequired).await);
                }
                413 => {
                    return Err(self.fatal(NoticeCode::DocumentTooLarge, None, BackendError::DocumentTooLarge).await);
                }
                status => {
                    // `atrusterror.http_generic` = „… HTTP %d %s“: der Status steht im Code, der Text ist
                    // nur die Reason-Phrase (`Notice::text`), fehlend wie Java „(null)“.
                    let reason = if response.reason.is_empty() { "(null)".to_owned() } else { response.reason.clone() };
                    let error = BackendError::Http { status, reason: reason.clone() };
                    return Err(self.fatal(NoticeCode::Http { status }, Some(reason), error).await);
                }
            }
            if let Some(refresh) = response.header("refresh") {
                let target = pida_net::parse_refresh(refresh)
                    .ok_or_else(|| BackendError::Protocol("ungültiger Refresh-Header".into()))?;
                request = Request::get(resolve(&url, &target)?);
                continue;
            }
            let Some(mime) = response.mime() else {
                return Err(BackendError::UnknownPage {
                    diagnostic: format!("path={} content-type fehlt", url.path()),
                });
            };
            if mime == "text/html" {
                let html = response.text();
                if let Some(content) = meta_refresh_content(&html)
                    && !content.is_empty()
                {
                    let target = pida_net::parse_refresh(&content)
                        .ok_or_else(|| BackendError::Protocol("ungültiges meta refresh".into()))?;
                    request = Request::get(resolve(&url, &target)?);
                    continue;
                }
                return match page::classify(&url, &html, &self.config.selectors) {
                    Ok(page) => {
                        tracing::debug!(block = page.block.kind(), path = page.base_url.path(), "A-Trust-Seite");
                        if matches!(page.block, PageBlock::WaitingForApp2Fa { .. })
                            && tracing::enabled!(tracing::Level::DEBUG)
                        {
                            // Diagnose (Nachtrag 19 Punkt 4): liefert A-Trust einen Link zum Öffnen der App mit?
                            // Nur Schema, Host und Pfad – ohne Abfrage und Fragment, ohne mailto/tel/sms.
                            tracing::debug!(
                                path = page.base_url.path(),
                                targets = ?page::link_targets(&url, &html),
                                "App-Warteseite: Link-Ziele"
                            );
                        }
                        Ok(Reply::Page(Box::new(page), url, self.config.clock.now()))
                    }
                    Err(e) => {
                        Err(BackendError::UnknownPage { diagnostic: format!("{e}; {}", page::diagnostic(&url, &html)) })
                    }
                };
            }
            if mime.ends_with("/xml") {
                return Ok(Reply::Xml(response));
            }
            return Err(BackendError::UnknownPage { diagnostic: format!("path={} content-type={mime}", url.path()) });
        }
    }

    // --------------------------------------------------------------------------------------------
    // Meldungen

    /// Fehlermeldung mit OK; danach wie PDF-Over Abbruch (siehe Crate-Doku).
    async fn fatal(&self, code: NoticeCode, text: Option<String>, error: BackendError) -> BackendError {
        if self.ctx.headless || self.quiet_fatal {
            return error;
        }
        let notice = Notice { kind: NoticeKind::Error, code, text, buttons: NoticeButtons::Ok };
        match self.cancellable(self.ui.notice(notice)).await {
            Ok(Ok(_)) | Ok(Err(InteractionError::Cancelled)) | Err(_) => BackendError::Cancelled,
            Ok(Err(InteractionError::Unsupported)) => error,
        }
    }

    /// Meldung mit Antwort; `Cancelled`/`Unsupported` zählen als Abbrechen.
    async fn ask(&self, notice: Notice) -> Result<NoticeAnswer, BackendError> {
        match self.cancellable(self.ui.notice(notice)).await? {
            Ok(answer) => Ok(answer),
            Err(InteractionError::Cancelled) => Ok(NoticeAnswer::Cancel),
            Err(InteractionError::Unsupported) => {
                tracing::warn!("Broker kann keine Meldung zeigen – wird als Abbrechen gewertet");
                Ok(NoticeAnswer::Cancel)
            }
        }
    }

    async fn form(&self, form: NativeForm) -> Result<FormAnswer, BackendError> {
        match self.cancellable(self.ui.native_form(form)).await? {
            Ok(answer) => Ok(answer),
            Err(InteractionError::Cancelled) => Err(BackendError::Cancelled),
            Err(InteractionError::Unsupported) => {
                Err(BackendError::Unavailable("Eingabemaske in dieser Oberfläche nicht verfügbar".into()))
            }
        }
    }

    /// Zugangsdaten, TAN und FIDO2-Assertion gehen nur an die vertrauten A-Trust-Domains der
    /// Selektor-Datei (nur `https://`) bzw. an die Origin des konfigurierten Endpunkts ([`trusted_target`])
    /// – geprüft werden die Seite selbst (die URL, von der sie geladen wurde) und das Formularziel.
    /// Eine Weiterleitung oder ein Formular zu einem fremden Host (PDF-Over prüft das nicht) bricht
    /// ab, bevor eine Maske erscheint oder Daten gesendet werden. Für FIDO2 ist das zwingend: der
    /// Klon setzt die Origin `https://service.a-trust.at` selbst.
    fn ensure_trusted(&self, page_url: &Url, page: &ParsedPage, what: &str) -> Result<(), BackendError> {
        for url in [page_url, &page.form_action] {
            self.ensure_trusted_url(url, what)?;
        }
        Ok(())
    }

    /// Ein einzelnes Ziel gegen [`trusted_target`] prüfen – gilt für jede Anfrage der Sitzung ([`Self::send`]),
    /// die Poll-Adresse (Bearer-Token) und das QR-Bild.
    fn ensure_trusted_url(&self, url: &Url, what: &str) -> Result<(), BackendError> {
        if trusted_target(url, &self.config.endpoint, &self.config.selectors) {
            return Ok(());
        }
        tracing::warn!(target_origin = %origin_of(url), what, "Ziel gehört nicht zu A-Trust");
        Err(BackendError::Protocol(format!("{what}: {} gehört nicht zu A-Trust – Vorgang abgebrochen", origin_of(url))))
    }

    fn context(&self, page: &ParsedPage, problem: Option<FormProblem>) -> FormContext {
        FormContext {
            signature_data_url: page
                .signature_data_link
                .as_ref()
                .filter(|u| allowed_external(u, &self.config.endpoint))
                .map(|u| u.as_str().to_owned()),
            can_switch_to_sms: page.sms_link.is_some(),
            can_switch_to_fido2: page.fido2_link.is_some(),
            // Plattform kann FIDO2 (Provider vorhanden); ob gerade ein Schlüssel steckt, prüft der Start
            fido2_visible: self.config.fido2.is_some(),
            problem,
        }
    }

    // --------------------------------------------------------------------------------------------
    // presentResponseToUserAndReturnNextRequest

    async fn present(
        &mut self,
        credentials: &mut Credentials,
        mut page: ParsedPage,
        page_url: &Url,
        received: Instant,
        login: &mut LoginState,
        renewal: &mut Renewal,
    ) -> Result<Next, BackendError> {
        // Antwort auf das ausdrückliche Absenden der Anmeldedaten? Nur diese eine Seite (Nachtrag 19)
        let reply_to_entered = std::mem::take(&mut login.entered);
        // 1. weder Fehler- noch Login-Block (noch beendete Sitzung) bei gesetzten Credentials = Anmeldung
        //    erfolgreich
        if !matches!(
            page.block,
            PageBlock::Error { .. } | PageBlock::UsernamePassword { .. } | PageBlock::SessionClosed { .. }
        ) {
            if login.submitted {
                login.confirmed = true;
            }
            if let (Some(user), Some(password)) = (&credentials.username, &credentials.password) {
                self.login_succeeded(user.clone(), password.clone());
            }
        }
        // 2. FIDO2 standardmäßig (einmalig je Sitzung)
        if self.fido2_default_pending
            && let Some(link) = &page.fido2_link
        {
            self.fido2_default_pending = false;
            return Ok(Next::Request(Request::get(link.clone())));
        }
        // 3. Blöcke
        let request = match page.block.clone() {
            PageBlock::UsernamePassword { username_field, password_field, submit, cancel, error_message } => {
                self.ensure_trusted(page_url, &page, "Anmeldung")?;
                return self
                    .login(
                        credentials,
                        &mut page,
                        (login, renewal, received),
                        (&username_field, &password_field),
                        (submit, cancel),
                        error_message,
                    )
                    .await;
            }
            PageBlock::SessionClosed { closed_page, diagnostic, .. } => {
                return self.session_closed(credentials, login, renewal, closed_page, reply_to_entered, diagnostic);
            }
            PageBlock::AutoSkip { submit } => {
                login.auto_skips += 1;
                if login.auto_skips > MAX_AUTO_SKIPS {
                    tracing::warn!(path = page.base_url.path(), "AutoSkip-Seite wiederholt sich – Abbruch");
                    return Err(BackendError::LoopDetected);
                }
                form_submit(&page, submit.as_ref())
            }
            PageBlock::Interstitial { message, next } => {
                let notice = Notice {
                    kind: NoticeKind::Info,
                    code: NoticeCode::AtrustInterstitial,
                    text: Some(message),
                    buttons: NoticeButtons::Ok,
                };
                // Schließen des Hinweises (SWT.CANCEL) = Nutzerabbruch, wie `MobileBKUState.showInformationMessage`
                match self.cancellable(self.ui.notice(notice)).await? {
                    Ok(NoticeAnswer::Cancel) | Err(InteractionError::Cancelled) => return Err(BackendError::Cancelled),
                    Ok(_) | Err(InteractionError::Unsupported) => form_submit(&page, next.as_ref()),
                }
            }
            PageBlock::Error { message, back, cancel, recoverable, requires_response } => {
                // Gescheiterte Anmeldung (Fehlerseite nach dem Absenden, vor der bestätigten Anmeldung): Passwort
                // verwerfen, nie automatisch erneut senden. Andere Fehlerseiten lassen das gemerkte Passwort stehen.
                if login.submitted && !login.confirmed {
                    self.clear_password(credentials);
                }
                let buttons = if recoverable { NoticeButtons::RetryCancel } else { NoticeButtons::Ok };
                let notice =
                    Notice { kind: NoticeKind::Error, code: NoticeCode::AtrustError, text: Some(message), buttons };
                let answer = self.ask(notice).await?;
                if recoverable && matches!(answer, NoticeAnswer::Retry | NoticeAnswer::Ok) {
                    form_submit(&page, back.as_ref())
                } else {
                    // Abbruch an A-Trust melden (A-Trust antwortet mit SL-`ErrorResponse`). Ist
                    // `#Button_Cancel` kein absendbarer Button, liefe ein Senden ohne Button nur zur selben
                    // Seite zurück – dann direkt abbrechen.
                    match cancel {
                        Some(cancel) if requires_response => {
                            login.cancel_sent = true;
                            form_submit(&page, Some(&cancel))
                        }
                        _ => return Err(BackendError::Cancelled),
                    }
                }
            }
            PageBlock::SmsTan { tan_field, reference_value, submit } => {
                self.ensure_trusted(page_url, &page, "SMS-TAN")?;
                self.sms_tan(&mut page, &tan_field, &reference_value, submit.as_ref()).await?
            }
            PageBlock::QrCode { reference_value, image_url, poll } => {
                let (image, problem) = match self.load_qr_image(&image_url).await? {
                    Some(img) => (Some(img), None),
                    None => (None, Some(FormProblem::QrImageFailed)),
                };
                let form = NativeForm::QrCode {
                    reference_value,
                    image,
                    timeout_secs: Some(self.config.poll.total_timeout.as_secs().min(u32::MAX as u64) as u32),
                    ctx: self.context(&page, problem),
                };
                self.wait_with_poll(&page, form, poll, login).await?
            }
            PageBlock::WaitingForApp2Fa { reference_value, poll } => {
                let form = NativeForm::WaitForApp {
                    reference_value,
                    timeout_secs: Some(self.config.poll.total_timeout.as_secs().min(u32::MAX as u64) as u32),
                    ctx: self.context(&page, None),
                };
                self.wait_with_poll(&page, form, poll, login).await?
            }
            PageBlock::Fido2 { options_json, result_field, submit } => {
                self.ensure_trusted(page_url, &page, "FIDO2")?;
                self.fido2(&mut page, &options_json, &result_field, submit.as_ref()).await?
            }
        };
        Ok(Next::Request(request))
    }

    /// Beendete bzw. abgelaufene A-Trust-Sitzung (Nachtrag 19). Kam `SessionClosed.aspx` als **unmittelbare**
    /// Antwort auf ein **ausdrückliches** Absenden der Anmeldedaten (`reply_to_entered`, vor bestätigter
    /// Anmeldung), wird die Sitzung einmal je SL-Request erneuert (die Zugangsdaten bleiben dafür im RAM). Nach
    /// einem an A-Trust gemeldeten Abbruch ist es der Abbruch der Nutzerin ([`BackendError::Cancelled`]). Sonst
    /// – zweites Mal, Fehlerseite ohne Meldung, „Wiederholen“ auf einer Fehlerseite, automatische Anmeldung,
    /// nach SMS-TAN oder App-Bestätigung – [`BackendError::SessionClosed`] ohne Meldung („Erneut versuchen“ =
    /// neue Sitzung). Nicht bestätigte Zugangsdaten werden dann wie nach einer Fehlerseite gelöscht; nach
    /// bestätigter Anmeldung bleiben sie (das Passwort war richtig).
    fn session_closed(
        &mut self,
        credentials: &mut Credentials,
        login: &LoginState,
        renewal: &mut Renewal,
        closed_page: bool,
        reply_to_entered: bool,
        diagnostic: String,
    ) -> Result<Next, BackendError> {
        let after_login = login.submitted && !login.confirmed;
        if login.cancel_sent {
            tracing::info!(closed_page, %diagnostic, "A-Trust-Sitzung nach dem gemeldeten Abbruch beendet");
            return Err(BackendError::Cancelled);
        }
        if closed_page && after_login && reply_to_entered && renewal.take() {
            return Ok(Next::Renew("Sitzung nach dem Absenden der Anmeldedaten beendet"));
        }
        tracing::warn!(closed_page, after_login, reply_to_entered, %diagnostic, "A-Trust-Sitzung beendet bzw. abgelaufen");
        if after_login {
            self.clear_password(credentials);
        }
        Err(BackendError::SessionClosed { diagnostic })
    }

    fn login_succeeded(&mut self, username: String, password: Zeroizing<String>) {
        if self.remember {
            *self.remembered.lock().expect("poisoned") =
                Credentials { username: Some(username.clone()), password: Some(password) };
        }
        if self.announced_login.as_deref() != Some(username.as_str()) {
            self.ui.event(BackendEvent::LoginSucceeded { username: username.clone(), remember: self.remember });
            self.announced_login = Some(username);
        }
    }

    fn update_remember(&mut self, remember: bool) {
        if remember == self.remember {
            return;
        }
        self.remember = remember;
        if !remember {
            *self.remembered.lock().expect("poisoned") = Credentials::default();
            self.ui.event(BackendEvent::ForgetCredentials);
        }
    }

    /// Anmeldung. Fehlen Zugangsdaten, fragt die Maske (ausdrückliches Absenden); sonst wird automatisch
    /// angemeldet. Stand die Maske länger als die übliche A-Trust-Sitzung
    /// ([`crate::selectors::SelectorSet::login_page_max_age`], Zeit aus [`crate::AtrustConfig::clock`]), wird vor
    /// dem Absenden einmal eine neue Sitzung geholt ([`Next::Renew`]); deren Login-Seite sendet dieselben
    /// Zugangsdaten automatisch.
    async fn login(
        &mut self,
        credentials: &mut Credentials,
        page: &mut ParsedPage,
        (login, renewal, received): (&mut LoginState, &mut Renewal, Instant),
        (username_field, password_field): (&str, &str),
        (submit, cancel): (Option<SubmitButton>, Option<SubmitButton>),
        error_message: Option<String>,
    ) -> Result<Next, BackendError> {
        // Erneute Login-Seite nach dem Senden (ohne zwischenzeitlich erfolgreiche Anmeldung) oder
        // Fehlermeldung auf der Login-Seite: nie automatisch erneut senden (Kontosperre).
        let rejected = login.submitted && !login.confirmed;
        if rejected || error_message.is_some() {
            self.clear_password(credentials);
        }
        let entered = credentials.username.is_none() || credentials.password.is_none();
        if entered {
            let mut problem = error_message.map(FormProblem::Atrust).or(rejected.then_some(FormProblem::LoginRejected));
            loop {
                let form = NativeForm::Login {
                    prefill_username: credentials.username.clone(),
                    remember: self.remember,
                    ctx: FormContext { problem: problem.clone(), ..self.context(page, None) },
                };
                match self.form(form).await {
                    Ok(FormAnswer::Login { username, password, remember }) => {
                        if let Err(p) = validate::validate_password(&password) {
                            problem = Some(p);
                            continue;
                        }
                        self.update_remember(remember);
                        credentials.username = Some(username);
                        credentials.password = Some(password);
                        break;
                    }
                    Ok(other) => {
                        tracing::warn!(answer = answer_kind(&other), "unerwartete Antwort auf die Anmeldemaske");
                        problem = None;
                    }
                    // Abbruch: A-Trust bekommt `Button_Cancel` (die Schleife läuft weiter, A-Trust
                    // antwortet dann mit einer SL-`ErrorResponse`). Ohne absendbaren Abbruch-Button
                    // käme nur wieder die Login-Seite (PDF-Over: Endlosschleife aus Abbruch und Maske)
                    // – dann direkt abbrechen.
                    Err(BackendError::Cancelled) if !self.ctx.cancel.is_cancelled() => {
                        return match cancel.as_ref() {
                            Some(button) => {
                                login.cancel_sent = true;
                                Ok(Next::Request(form_submit(page, Some(button))))
                            }
                            None => Err(BackendError::Cancelled),
                        };
                    }
                    Err(e) => return Err(e),
                }
            }
        }
        let (Some(user), Some(password)) = (&credentials.username, &credentials.password) else {
            return Err(BackendError::Internal("Zugangsdaten fehlen".into()));
        };
        if entered
            && let Some(max_age) = self.config.selectors.login_page_max_age()
            && self.config.clock.now().saturating_duration_since(received) > max_age
            && renewal.take()
        {
            return Ok(Next::Renew("Anmeldeseite älter als die übliche A-Trust-Sitzung"));
        }
        page.set_field(username_field, user.as_str());
        page.set_field(password_field, password.as_str());
        login.submitted = true;
        login.confirmed = false;
        login.entered = entered;
        Ok(Next::Request(form_submit(page, submit.as_ref())))
    }

    async fn sms_tan(
        &mut self,
        page: &mut ParsedPage,
        tan_field: &str,
        reference_value: &str,
        submit: Option<&SubmitButton>,
    ) -> Result<Request, BackendError> {
        let mut problem = None;
        loop {
            let mut ctx = self.context(page, problem.take());
            ctx.can_switch_to_sms = false; // bereits SMS
            let form = NativeForm::SmsTan { reference_value: reference_value.to_owned(), ctx };
            match self.form(form).await? {
                FormAnswer::Tan(tan) => match validate::validate_tan(&tan, reference_value) {
                    Ok(()) => {
                        page.set_field(tan_field, page::java_trim(&tan));
                        return Ok(form_submit(page, submit));
                    }
                    Err(p) => problem = Some(p),
                },
                // Ohne FIDO2-Link sendete PDF-Over per switch-Fallthrough eine leere TAN; hier wird
                // die Maske erneut gezeigt.
                FormAnswer::SwitchToFido2 => match &page.fido2_link {
                    Some(link) => return Ok(Request::get(link.clone())),
                    None => problem = None,
                },
                other => tracing::warn!(answer = answer_kind(&other), "unerwartete Antwort auf die TAN-Maske"),
            }
        }
    }

    /// R8: QR-Bild mit eigenem Client ohne Cookies; Fehler → `None` (Maske zeigt `QrImageFailed`).
    async fn load_qr_image(&self, url: &Url) -> Result<Option<QrImage>, BackendError> {
        // ein Bild von einem fremden Host wird nicht geholt (die Maske zeigt dann `QrImageFailed`)
        if self.ensure_trusted_url(url, "QR-Bild").is_err() {
            return Ok(None);
        }
        let Ok(client) = HttpClient::new(&self.config.net, Purpose::AtrustImage) else { return Ok(None) };
        match self.cancellable(client.get(url, &[])).await? {
            Ok(r) if (200..300).contains(&r.status) && !r.body.is_empty() => {
                Ok(Some(QrImage { mime: r.mime(), bytes: r.body }))
            }
            Ok(r) => {
                tracing::warn!(status = r.status, "QR-Bild nicht geladen");
                Ok(None)
            }
            Err(e) => {
                tracing::warn!(error = %e, "QR-Bild nicht geladen");
                Ok(None)
            }
        }
    }

    /// QR/App: Maske und Long-Poll nebenläufig. Poll „fertig“ → Maske droppen (schließt sie) und
    /// Seite neu laden (R7); Wechsel per Link (R6). Ein Abbruch lässt das gemerkte Passwort stehen.
    async fn wait_with_poll(
        &mut self,
        page: &ParsedPage,
        form: NativeForm,
        poll_info: Option<PollInfo>,
        login: &mut LoginState,
    ) -> Result<Request, BackendError> {
        let reload = Request::get(page.base_url.clone());
        // das Bearer-Token der Abfrage geht nur an A-Trust
        if let Some(info) = &poll_info {
            self.ensure_trusted_url(&info.url, "Statusabfrage")?;
        }
        let poll_client = match &poll_info {
            Some(_) => Some(HttpClient::new(&self.config.net, Purpose::AtrustPoll).map_err(net_to_backend)?),
            None => None,
        };
        let poll_config = self.config.poll;
        let cancel = self.ctx.cancel.clone();
        let ui = Arc::clone(&self.ui);
        let answer = {
            let form_future = ui.native_form(form);
            let poll_future = async {
                match (&poll_client, &poll_info) {
                    (Some(client), Some(info)) => poll::run(client, info, poll_config, &cancel).await,
                    _ => std::future::pending().await,
                }
            };
            tokio::pin!(form_future, poll_future);
            let mut poll_active = poll_info.is_some();
            loop {
                tokio::select! {
                    outcome = &mut poll_future, if poll_active => match outcome {
                        PollOutcome::Reload => {
                            login.poll_reloads += 1;
                            if login.poll_reloads > MAX_POLL_RELOADS {
                                tracing::warn!("Long-Poll meldet wiederholt „fertig“ ohne Seitenwechsel – Abbruch");
                                return Err(BackendError::LoopDetected);
                            }
                            return Ok(reload);
                        }
                        PollOutcome::Stopped => poll_active = false,
                        PollOutcome::Cancelled => return Err(BackendError::Cancelled),
                    },
                    answer = &mut form_future => break answer,
                    _ = cancel.cancelled() => return Err(BackendError::Cancelled),
                }
            }
        };
        match answer {
            Ok(FormAnswer::SwitchToSms) => Ok(page.sms_link.clone().map(Request::get).unwrap_or(reload)),
            Ok(FormAnswer::SwitchToFido2) => Ok(page.fido2_link.clone().map(Request::get).unwrap_or(reload)),
            Ok(_) => Ok(reload),
            // Abbruch in der QR-/App-Maske: das gemerkte Passwort bleibt (nur „Merken“ abwählen oder eine
            // gescheiterte Anmeldung verwerfen es)
            Err(InteractionError::Cancelled) => Err(BackendError::Cancelled),
            Err(InteractionError::Unsupported) => {
                Err(BackendError::Unavailable("Eingabemaske in dieser Oberfläche nicht verfügbar".into()))
            }
        }
    }

    /// FIDO2: Assertion startet automatisch; Fehler → Maske mit Hinweis, „erneut“/SMS/Abbruch.
    async fn fido2(
        &mut self,
        page: &mut ParsedPage,
        options_json: &str,
        result_field: &str,
        submit: Option<&SubmitButton>,
    ) -> Result<Request, BackendError> {
        // Verfügbarkeit bei jedem Start live prüfen: ein Sicherheitsschlüssel (CTAP-HID) kann nach dem
        // Öffnen der Sitzung eingesteckt werden; „erneut versuchen“ prüft wieder.
        let configured = self.config.fido2.clone();
        let mut problem = None;
        let mut start = true;
        // PIN (CTAP2 `clientPIN`) nur im RAM, nur für diese Maske (Welle 2: `SecretKind::Fido2Pin`)
        let mut pin: Option<Zeroizing<String>> = None;
        loop {
            // nach `PinRequired`/`PinInvalid`: PIN abfragen (die FIDO2-Maske ist dann geschlossen)
            let mut ask_pin: Option<Fido2Error> = None;
            let available = if start { configured.clone().filter(|p| p.is_available()) } else { None };
            if start && available.is_none() {
                start = false;
                problem = Some(FormProblem::Fido2Error {
                    code: Fido2Error::Unavailable.code().into(),
                    detail: "FIDO2 ist auf diesem Gerät nicht verfügbar".into(),
                });
            }
            if start && let Some(provider) = &available {
                start = false;
                let form = NativeForm::Fido2 { in_progress: true, ctx: self.context(page, None) };
                let request =
                    Fido2Request { options_json: options_json.to_owned(), origin: WEBAUTHN_ORIGIN.to_owned() };
                let parent = self.ui.parent_window();
                let worker = Arc::clone(provider);
                let mut guard = CancelFido2OnDrop { provider: Some(Arc::clone(provider)) };
                let task_pin = pin.clone();
                let task = tokio::task::spawn_blocking(move || {
                    worker.get_assertion_with_pin(&request, parent, task_pin.as_deref().map(String::as_str))
                });
                let ui = Arc::clone(&self.ui);
                let cancel = self.ctx.cancel.clone();
                let form_future = ui.native_form(form);
                tokio::pin!(task, form_future);
                tokio::select! {
                    result = &mut task => {
                        guard.disarm();
                        match result {
                            Ok(Ok(assertion)) => {
                                page.set_field(result_field, fido2::result_json(&assertion));
                                return Ok(form_submit(page, submit));
                            }
                            Ok(Err(Fido2Error::Cancelled)) => problem = None,
                            Ok(Err(e @ (Fido2Error::PinRequired | Fido2Error::PinInvalid))) if provider.supports_pin() => {
                                ask_pin = Some(e);
                            }
                            Ok(Err(e)) => problem = Some(fido2_problem(&e)),
                            Err(e) => {
                                problem = Some(FormProblem::Fido2Error {
                                    code: "fido2-internal".into(),
                                    detail: format!("interner Fehler: {e}"),
                                })
                            }
                        }
                    }
                    answer = &mut form_future => {
                        // laufende Anfrage abbrechen – eine bereits beendete nicht (kein verspäteter
                        // Abbruch, der beim Provider hängen bliebe)
                        if task.is_finished() {
                            guard.disarm();
                        }
                        drop(guard);
                        match answer {
                            Ok(FormAnswer::SwitchToSms) if page.sms_link.is_some() => {
                                return Ok(Request::get(page.sms_link.clone().expect("geprüft")));
                            }
                            Ok(FormAnswer::StartFido2) => {
                                // abgebrochene Anfrage erst auslaufen lassen (kein paralleler Aufruf)
                                let _ = tokio::time::timeout(std::time::Duration::from_secs(2), &mut task).await;
                                start = true;
                            }
                            Ok(_) => {}
                            Err(InteractionError::Cancelled) => return Err(BackendError::Cancelled),
                            Err(InteractionError::Unsupported) => {
                                return Err(BackendError::Unavailable("FIDO2-Maske nicht verfügbar".into()));
                            }
                        }
                    }
                    _ = cancel.cancelled() => return Err(BackendError::Cancelled),
                }
                if ask_pin.is_none() {
                    continue;
                }
            }
            if let Some(e) = ask_pin {
                let retry = matches!(e, Fido2Error::PinInvalid) || pin.is_some();
                let prompt = SecretPrompt { kind: SecretKind::Fido2Pin, subject: "FIDO2".into(), retry };
                let answer = tokio::select! {
                    a = self.ui.secret(prompt) => a,
                    _ = self.ctx.cancel.cancelled() => return Err(BackendError::Cancelled),
                };
                match answer {
                    Ok(p) => {
                        pin = Some(p);
                        problem = None;
                        start = true;
                    }
                    // abgebrochen: Maske mit aktivem „erneut“-Knopf, PIN vergessen
                    Err(InteractionError::Cancelled) => {
                        pin = None;
                        problem = None;
                    }
                    Err(InteractionError::Unsupported) => {
                        pin = None;
                        problem = Some(fido2_problem(&e));
                    }
                }
                continue;
            }
            let form = NativeForm::Fido2 { in_progress: false, ctx: self.context(page, problem.clone()) };
            match self.form(form).await? {
                FormAnswer::StartFido2 if configured.is_some() => {
                    problem = None;
                    start = true;
                }
                FormAnswer::SwitchToSms => match &page.sms_link {
                    Some(link) => return Ok(Request::get(link.clone())),
                    None => tracing::warn!("SMS gewählt, aber kein SMS-Link auf der FIDO2-Seite"),
                },
                other => tracing::debug!(answer = answer_kind(&other), "Antwort auf die FIDO2-Maske ohne Wirkung"),
            }
        }
    }
}

/// FIDO2-Fehler als Maskenhinweis mit stabilem Code (`FormProblem::Fido2Error`, Welle 2). Verlangt
/// der Schlüssel eine PIN, die der Provider nicht übernehmen kann, gilt `fido2-pin-unsupported`.
fn fido2_problem(e: &Fido2Error) -> FormProblem {
    match e {
        Fido2Error::PinRequired => FormProblem::Fido2Error {
            code: "fido2-pin-unsupported".into(),
            detail: "Dieser Sicherheitsschlüssel verlangt eine PIN; die PIN-Eingabe ist auf diesem System nicht verfügbar – bitte App oder SMS verwenden".into(),
        },
        other => FormProblem::Fido2Error { code: other.code().into(), detail: other.to_string() },
    }
}

/// Art einer Maskenantwort für Logs (ohne Benutzername, Passwort oder TAN).
fn answer_kind(answer: &FormAnswer) -> &'static str {
    match answer {
        FormAnswer::Login { .. } => "Login",
        FormAnswer::Tan(_) => "Tan",
        FormAnswer::SwitchToSms => "SwitchToSms",
        FormAnswer::SwitchToFido2 => "SwitchToFido2",
        FormAnswer::StartFido2 => "StartFido2",
        FormAnswer::Continue => "Continue",
    }
}

fn kind(response: &SlResponse) -> &'static str {
    match response {
        SlResponse::InfoboxRead(_) => "InfoboxReadResponse",
        SlResponse::CreateCmsSignature(_) => "CreateCMSSignatureResponse",
        SlResponse::NullOperation => "NullOperationResponse",
        SlResponse::Error(_) => "ErrorResponse",
    }
}

/// SL-`ErrorResponse` → Fehler (6000/6001 = Nutzerabbruch, docs/analyse/02 §2.6).
pub(crate) fn sl_error(e: pida_sl12::ErrorResponse) -> BackendError {
    if e.is_user_cancel() {
        BackendError::Cancelled
    } else {
        BackendError::SecurityLayer { code: e.code, info: e.info }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_refresh_detection() {
        assert_eq!(
            meta_refresh_content("<html><head><meta http-equiv=\"REFRESH\" content=\"0; url=/x\"></head></html>")
                .as_deref(),
            Some("0; url=/x")
        );
        assert_eq!(meta_refresh_content("<meta http-equiv=refresh>").as_deref(), Some(""));
        assert_eq!(meta_refresh_content("<meta http-equiv=\"content-type\" content=\"text/html\">"), None);
        // nur der erste Treffer zählt
        assert_eq!(
            meta_refresh_content(
                "<meta http-equiv=refresh content=\"\"><meta http-equiv=refresh content=\"0;url=/y\">"
            )
            .as_deref(),
            Some("")
        );
    }

    #[test]
    fn external_link_policy() {
        let endpoint = Url::parse(crate::PROD_ENDPOINT).unwrap();
        let ok = |s: &str| allowed_external(&Url::parse(s).unwrap(), &endpoint);
        assert!(ok("https://service.a-trust.at/mobile/ShowSigobj.aspx?x=1"));
        assert!(ok("https://www.a-trust.at/x"));
        assert!(!ok("http://service.a-trust.at/x"));
        assert!(!ok("https://evil-a-trust.at/x"));
        assert!(!ok("https://a-trust.at.evil.example/x"));
        // Systembrowser nur mit https, auch beim lokalen Endpunkt
        let local = Url::parse("http://127.0.0.1:8080/mobile/default.aspx").unwrap();
        assert!(!allowed_external(&Url::parse("http://127.0.0.1:8080/ShowSigobj.aspx").unwrap(), &local));
        assert!(allowed_external(&Url::parse("https://127.0.0.1/ShowSigobj.aspx").unwrap(), &local));
        assert!(!allowed_external(&Url::parse("https://127.0.0.2/ShowSigobj.aspx").unwrap(), &local));
    }

    #[test]
    fn trusted_targets_for_credentials() {
        let endpoint = Url::parse(crate::PROD_ENDPOINT).unwrap();
        let sel = crate::selectors::SelectorSet::builtin();
        let ok = |s: &str| trusted_target(&Url::parse(s).unwrap(), &endpoint, &sel);
        assert!(ok("https://service.a-trust.at/mobile/Login.aspx"));
        assert!(ok("https://www.a-trust.at/x"));
        assert!(!ok("http://service.a-trust.at/mobile/Login.aspx"), "nie Klartext");
        assert!(!ok("https://a-trust.at.evil.example/x"));
        assert!(!ok("https://evil-a-trust.at/x"));
        assert!(ok("https://www.handy-signatur.at/mobile/x.aspx"));
        assert!(!ok("http://www.handy-signatur.at/mobile/x.aspx"));
        let local = Url::parse("http://127.0.0.1:8080/mobile/default.aspx").unwrap();
        let t = |s: &str| trusted_target(&Url::parse(s).unwrap(), &local, &sel);
        assert!(t("http://127.0.0.1:8080/Login.aspx"));
        assert!(!t("http://127.0.0.1:9090/Login.aspx"));
        assert!(!t("https://127.0.0.1:8080/Login.aspx"));
    }

    #[test]
    fn sl_errors() {
        assert_eq!(sl_error(pida_sl12::ErrorResponse { code: 6001, info: "x".into() }), BackendError::Cancelled);
        assert_eq!(sl_error(pida_sl12::ErrorResponse { code: 6000, info: "x".into() }), BackendError::Cancelled);
        assert_eq!(
            sl_error(pida_sl12::ErrorResponse { code: 1501, info: "y".into() }),
            BackendError::SecurityLayer { code: 1501, info: "y".into() }
        );
    }
}
