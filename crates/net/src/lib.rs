#![cfg_attr(all(target_arch = "wasm32", target_os = "unknown"), allow(dead_code, unused_imports))]
//! `pida-net` – HTTP-Client-Fassade für A-Trust (Szenario A), lokale BKU und Update-Check.
//!
//! Anforderungen (docs/analyse/02 §3.2–§3.4, §4.3, §6; Plan §2.1, §4.4):
//! - **je Sitzung ein Client mit eigenem, leerem Cookie-Jar** (je SL-Request ein neuer Client wie
//!   PDF-Over); Long-Poll- und QR-Bild-Client ohne Cookies ([`Purpose`]),
//! - **keine automatischen Redirects** im A-Trust-Hauptdialog und bei der lokalen BKU – der Aufrufer
//!   behandelt 301/302 selbst (immer als GET), 303/307/308 sind Fehler; Hilfen [`resolve`],
//!   [`parse_refresh`]. Long-Poll, QR-Bild und Update-Check folgen Redirects automatisch wie der
//!   Apache-HttpClient-Default in PDF-Over (höchstens 10),
//! - Proxy: System (Windows/macOS, `HTTP(S)_PROXY`/`NO_PROXY`), manuell mit Basic-Authentifizierung,
//!   oder keiner (lokale BKU immer ohne Proxy),
//! - TLS über rustls mit Provider `ring` und `rustls-platform-verifier` (OS-Truststore, kein
//!   Pinning – TLS-Inspektion in Behördennetzen),
//! - HTTP/1.1 wie Apache HttpClient 5 classic (kein HTTP/2, damit Reason-Phrase und Verhalten
//!   gleich bleiben), Timeouts, Höchstgröße des Antwortkörpers, User-Agent
//!   [`DEFAULT_USER_AGENT`] (`PIDA v<Version>`).
//!
//! Debug-Ausgaben und Fehlertexte enthalten nie Formularwerte, Tokens, Cookies oder URLs mit
//! Abfrageparametern (ADR 0001 §4 Regel 9).
//!
//! Ownership: Paket **D** (zusammen mit `sl12`, `backend-atrust`, `backend-localbku`).

mod httpdate;
mod multipart;
mod refresh;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

use std::net::SocketAddr;
use std::sync::Once;
use std::time::Duration;

pub use url::Url;
use zeroize::Zeroizing;

pub use refresh::parse_refresh;

/// User-Agent aller Requests, sofern [`NetConfig::user_agent`] nichts anderes vorgibt
/// (`PIDA v<Version>`, analog zu PDF-Over `Constants.APP_NAME_VERSION`).
pub const DEFAULT_USER_AGENT: &str = concat!("PIDA v", env!("CARGO_PKG_VERSION"));

/// Höchstzahl automatisch verfolgter Redirects für [`Purpose::AtrustPoll`], [`Purpose::AtrustImage`]
/// und [`Purpose::UpdateCheck`].
pub const MAX_AUTO_REDIRECTS: usize = 10;

/// Netzfehler.
#[derive(Debug, thiserror::Error)]
pub enum NetError {
    /// Verbindung fehlgeschlagen (DNS, TCP, Protokoll) – PDF-Over `error.FailedToConnect`.
    #[error("Verbindung fehlgeschlagen: {0}")]
    Connect(String),
    /// Zeitüberschreitung.
    #[error("Zeitüberschreitung")]
    Timeout,
    /// TLS-Fehler (Zertifikat, Handshake).
    #[error("TLS-Fehler: {0}")]
    Tls(String),
    /// Proxy-Fehler (inkl. 407 vom Proxy beim CONNECT, siehe [`NetError::is_proxy_auth_required`]).
    #[error("Proxy-Fehler: {0}")]
    Proxy(String),
    /// Ungültige URL.
    #[error("ungültige URL: {0}")]
    InvalidUrl(String),
    /// Antwortkörper nicht lesbar bzw. zu groß.
    #[error("Antwort nicht lesbar: {0}")]
    Body(String),
    /// Client nicht konfigurierbar.
    #[error("Konfigurationsfehler: {0}")]
    Config(String),
    /// Die Gegenstelle hat die Verbindung ohne Antwort geschlossen (Apache `NoHttpResponseException`;
    /// der Long-Poll pollt dann weiter, docs/analyse/02 §3.6).
    #[error("keine Antwort der Gegenstelle")]
    NoResponse,
    /// Eine bestehende Verbindung ist abgerissen (TLS ohne `close_notify`, Reset, Broken Pipe) –
    /// typisch für eine wiederverwendete Keep-Alive-Verbindung, die das Mobil-OS im Hintergrund
    /// gekappt hat. GET-Requests wiederholt [`HttpClient::execute`] einmal auf neuer Verbindung.
    #[error("Verbindung unterbrochen: {0}")]
    ConnectionLost(String),
}

/// Text, mit dem [`NetError::Proxy`] eine 407-Antwort des Proxys beim `CONNECT` meldet.
const PROXY_AUTH_REQUIRED: &str = "407 Proxy Authentication Required";

impl NetError {
    /// `true`, wenn der Proxy eine Anmeldung verlangt (HTTP 407 beim `CONNECT`-Tunnel; bei einem
    /// Klartext-Proxy kommt die 407 dagegen als normale [`Response`]).
    pub fn is_proxy_auth_required(&self) -> bool {
        matches!(self, NetError::Proxy(m) if m == PROXY_AUTH_REQUIRED)
    }
}

/// Ergebnis-Alias.
pub type Result<T, E = NetError> = std::result::Result<T, E>;

/// Proxy-Konfiguration (Konfiguration `PROXY_*` bzw. CLI `-proxy*`, docs/analyse/05 §3.1).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ProxyConfig {
    /// System-Proxy (reqwest `system-proxy`, Umgebungsvariablen).
    #[default]
    System,
    /// Kein Proxy.
    None,
    /// Manuell.
    Manual {
        /// Host
        host: String,
        /// Port (1–65535)
        port: u16,
        /// Optionale Authentifizierung.
        auth: Option<ProxyAuth>,
    },
}

/// Proxy-Anmeldedaten (Passwort nur im RAM bzw. Schlüsselbund).
#[derive(Clone, PartialEq, Eq)]
pub struct ProxyAuth {
    /// Benutzer
    pub user: String,
    /// Passwort
    pub password: Zeroizing<String>,
}

impl std::fmt::Debug for ProxyAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxyAuth").field("user", &self.user).field("password", &"***").finish()
    }
}

/// Verwendungszweck eines Clients – bestimmt Cookies, Redirects und Proxy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Purpose {
    /// A-Trust-Hauptdialog: eigener Cookie-Jar, keine Redirects.
    AtrustSession,
    /// A-Trust-Long-Poll (`qrpoll`): ohne Cookies, Bearer-Token, lange Haltezeit.
    AtrustPoll,
    /// QR-Bild: ohne Cookies.
    AtrustImage,
    /// Lokale BKU auf 127.0.0.1: ohne Proxy.
    LocalBku,
    /// Update-Check.
    UpdateCheck,
}

impl Purpose {
    /// Eigener Cookie-Jar (nur der A-Trust-Hauptdialog, docs/analyse/02 §3.2).
    pub fn uses_cookies(self) -> bool {
        matches!(self, Purpose::AtrustSession)
    }

    /// Redirects folgt der Client selbst (Long-Poll, QR-Bild, Update-Check); sonst nie.
    pub fn follows_redirects(self) -> bool {
        matches!(self, Purpose::AtrustPoll | Purpose::AtrustImage | Purpose::UpdateCheck)
    }

    /// Proxy erlaubt (die lokale BKU wird immer direkt angesprochen, `BKUHelper.java:95-97`).
    pub fn allows_proxy(self) -> bool {
        !matches!(self, Purpose::LocalBku)
    }

    /// Nur `https` – auch für Redirects (Update-Check, ADR 0004 Nachtrag 21); eine `http`-Adresse bzw. ein
    /// Redirect dorthin ist ein Fehler ([`NetError::Config`]).
    pub fn requires_https(self) -> bool {
        matches!(self, Purpose::UpdateCheck)
    }
}

/// Client-Konfiguration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetConfig {
    /// Proxy.
    pub proxy: ProxyConfig,
    /// User-Agent; `None` = [`DEFAULT_USER_AGENT`].
    pub user_agent: Option<String>,
    /// Verbindungsaufbau-Timeout.
    pub connect_timeout: Duration,
    /// Gesamttimeout je Request (`None` = keiner, z. B. Long-Poll steuert selbst).
    pub request_timeout: Option<Duration>,
    /// Höchstgröße eines Antwortkörpers.
    pub max_body: usize,
}

impl Default for NetConfig {
    fn default() -> Self {
        Self {
            proxy: ProxyConfig::System,
            user_agent: None,
            connect_timeout: Duration::from_secs(30),
            request_timeout: Some(Duration::from_secs(120)),
            max_body: 64 * 1024 * 1024,
        }
    }
}

impl NetConfig {
    /// Wirksamer User-Agent.
    pub fn effective_user_agent(&self) -> &str {
        self.user_agent.as_deref().unwrap_or(DEFAULT_USER_AGENT)
    }
}

/// Teil eines `multipart/form-data`-Bodys. `Debug` zeigt nur Name, Typ und Länge – A-Trust-Formulare
/// tragen hier Passwort und TAN (ADR 0001 §4 Regel 9).
#[derive(Clone, PartialEq, Eq)]
pub struct FormPart {
    /// Feldname (`fileupload`, `XMLRequest` …).
    pub name: String,
    /// Dateiname (`sign.pdf`), nur für Dateiteile.
    pub filename: Option<String>,
    /// Content-Type (`application/pdf`, `text/plain; charset=UTF-8` …).
    pub content_type: Option<String>,
    /// Inhalt.
    pub data: Vec<u8>,
}

impl FormPart {
    /// Textteil mit `Content-Type: text/plain; charset=UTF-8` (Apache `addTextBody(…, TEXT_UTF8)`).
    pub fn text(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            filename: None,
            content_type: Some("text/plain; charset=UTF-8".into()),
            data: value.into().into_bytes(),
        }
    }

    /// Dateiteil.
    pub fn file(
        name: impl Into<String>,
        filename: impl Into<String>,
        content_type: impl Into<String>,
        data: Vec<u8>,
    ) -> Self {
        Self { name: name.into(), filename: Some(filename.into()), content_type: Some(content_type.into()), data }
    }
}

/// Request-Körper. `Debug` ohne Werte (Regel 9).
#[derive(Clone, PartialEq, Eq, Default)]
pub enum Body {
    /// leer
    #[default]
    Empty,
    /// `application/x-www-form-urlencoded; charset=UTF-8`
    Form(Vec<(String, String)>),
    /// `multipart/form-data` in genau dieser Teil-Reihenfolge.
    Multipart(Vec<FormPart>),
    /// Rohdaten.
    Bytes {
        /// Content-Type
        content_type: String,
        /// Daten
        data: Vec<u8>,
    },
}

/// HTTP-Methode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Method {
    /// GET
    Get,
    /// POST
    Post,
}

impl Method {
    /// `GET`/`POST`.
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
        }
    }
}

/// Request. `Debug` schwärzt `Authorization`/`Cookie`-Header und Körperwerte (Regel 9).
#[derive(Clone, PartialEq, Eq)]
pub struct Request {
    /// Methode
    pub method: Method,
    /// Ziel
    pub url: Url,
    /// zusätzliche Header (z. B. `Authorization: Bearer …` beim Long-Poll)
    pub headers: Vec<(String, String)>,
    /// Körper
    pub body: Body,
}

impl Request {
    /// GET ohne Header.
    pub fn get(url: Url) -> Self {
        Self { method: Method::Get, url, headers: Vec::new(), body: Body::Empty }
    }

    /// POST mit Körper.
    pub fn post(url: Url, body: Body) -> Self {
        Self { method: Method::Post, url, headers: Vec::new(), body }
    }

    /// Fügt einen Header hinzu (Builder-Stil).
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

/// Antwort (Körper vollständig gelesen). `Debug` zeigt nur die Körperlänge und schwärzt
/// `Set-Cookie` (Regel 9).
#[derive(Clone, PartialEq, Eq)]
pub struct Response {
    /// Statuscode.
    pub status: u16,
    /// Reason-Phrase (für `atrusterror.http_generic`).
    pub reason: String,
    /// URL des Requests (Basis für relative `Location`/Formular-URLs); bei Clients, die Redirects
    /// selbst folgen, die URL der letzten Antwort.
    pub url: Url,
    /// Header in Empfangsreihenfolge, Namen kleingeschrieben.
    pub headers: Vec<(String, String)>,
    /// Körper.
    pub body: Vec<u8>,
}

/// Header-Werte, die nie in Logs erscheinen dürfen.
fn is_sensitive_header(name: &str) -> bool {
    ["authorization", "proxy-authorization", "cookie", "set-cookie"].iter().any(|h| name.eq_ignore_ascii_case(h))
}

struct RedactedHeaders<'a>(&'a [(String, String)]);

impl std::fmt::Debug for RedactedHeaders<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.0.iter().map(|(k, v)| (k.as_str(), if is_sensitive_header(k) { "***" } else { v.as_str() })))
            .finish()
    }
}

/// URL ohne Abfrage und Fragment (Abfrageparameter können Sitzungskennungen tragen, Regel 9).
pub fn redact_url(url: &Url) -> String {
    let mut s = format!("{}://{}", url.scheme(), url.host_str().unwrap_or(""));
    if let Some(port) = url.port() {
        s.push_str(&format!(":{port}"));
    }
    s.push_str(url.path());
    if url.query().is_some() {
        s.push_str("?…");
    }
    s
}

impl std::fmt::Debug for FormPart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FormPart")
            .field("name", &self.name)
            .field("filename", &self.filename)
            .field("content_type", &self.content_type)
            .field("len", &self.data.len())
            .finish()
    }
}

impl std::fmt::Debug for Body {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Body::Empty => f.write_str("Empty"),
            Body::Form(fields) => {
                f.debug_tuple("Form").field(&fields.iter().map(|(k, _)| k).collect::<Vec<_>>()).finish()
            }
            Body::Multipart(parts) => f.debug_tuple("Multipart").field(parts).finish(),
            Body::Bytes { content_type, data } => {
                f.debug_struct("Bytes").field("content_type", content_type).field("len", &data.len()).finish()
            }
        }
    }
}

impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Request")
            .field("method", &self.method)
            .field("url", &redact_url(&self.url))
            .field("headers", &RedactedHeaders(&self.headers))
            .field("body", &self.body)
            .finish()
    }
}

impl std::fmt::Debug for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Response")
            .field("status", &self.status)
            .field("reason", &self.reason)
            .field("url", &redact_url(&self.url))
            .field("headers", &RedactedHeaders(&self.headers))
            .field("body_len", &self.body.len())
            .finish()
    }
}

impl Response {
    /// Erster Header mit diesem Namen (ohne Groß-/Kleinschreibung).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    /// MIME-Typ ohne Parameter, kleingeschrieben (`text/html`, `text/xml` …).
    pub fn mime(&self) -> Option<String> {
        self.header("content-type").map(|ct| ct.split(';').next().unwrap_or("").trim().to_ascii_lowercase())
    }

    /// Charset aus `Content-Type` (Parameter `charset`, Anführungszeichen entfernt).
    pub fn charset(&self) -> Option<String> {
        let ct = self.header("content-type")?;
        ct.split(';').skip(1).find_map(|param| {
            let (name, value) = param.split_once('=')?;
            if !name.trim().eq_ignore_ascii_case("charset") {
                return None;
            }
            let value = value.trim().trim_matches('"').trim();
            (!value.is_empty()).then(|| value.to_owned())
        })
    }

    /// Körper als Text mit Charset aus `Content-Type`, Default UTF-8 (docs/analyse/02 §3.4 Schritt 6).
    /// Unbekannte Charsets und ungültige Bytes werden ersetzt (U+FFFD), nie abgebrochen.
    pub fn text(&self) -> String {
        let encoding = self
            .charset()
            .and_then(|label| encoding_rs::Encoding::for_label(label.as_bytes()))
            .unwrap_or(encoding_rs::UTF_8);
        let (text, _) = encoding.decode_without_bom_handling(&self.body);
        text.into_owned()
    }

    /// HTTP-`Date` (Zeitplausibilisierung `/M`, Plan §6.10 „Zeit“). IMF-fixdate, RFC 850 und
    /// asctime (RFC 9110 §5.6.7); sonst `None`.
    pub fn date(&self) -> Option<time::OffsetDateTime> {
        httpdate::parse_http_date(self.header("date")?)
    }
}

/// HTTP-Client einer Sitzung.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
#[derive(Debug)]
pub struct HttpClient {
    inner: reqwest::Client,
    purpose: Purpose,
    max_body: usize,
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
impl HttpClient {
    /// Neuer Client mit eigenem Cookie-Jar (sofern der Zweck Cookies vorsieht).
    pub fn new(config: &NetConfig, purpose: Purpose) -> Result<Self> {
        install_crypto_provider();
        let redirect = if purpose.follows_redirects() {
            reqwest::redirect::Policy::limited(MAX_AUTO_REDIRECTS)
        } else {
            reqwest::redirect::Policy::none()
        };
        let mut builder = reqwest::Client::builder()
            .user_agent(config.effective_user_agent())
            .connect_timeout(config.connect_timeout)
            .http1_only()
            .redirect(redirect)
            .https_only(purpose.requires_https())
            .cookie_store(purpose.uses_cookies());
        if let Some(timeout) = config.request_timeout {
            builder = builder.timeout(timeout);
        }
        let proxy = if purpose.allows_proxy() { &config.proxy } else { &ProxyConfig::None };
        builder = match proxy {
            ProxyConfig::System => builder,
            ProxyConfig::None => builder.no_proxy(),
            ProxyConfig::Manual { host, port, auth } => builder.proxy(manual_proxy(host, *port, auth.as_ref())?),
        };
        let inner = builder.build().map_err(|e| NetError::Config(error_chain(&e)))?;
        Ok(Self { inner, purpose, max_body: config.max_body })
    }

    /// Zweck.
    pub fn purpose(&self) -> Purpose {
        self.purpose
    }

    /// Führt einen Request aus. Redirects folgt nur ein Client, dessen [`Purpose::follows_redirects`]
    /// gilt; sonst kommt jede 3xx-Antwort unverändert zurück.
    ///
    /// Ein GET, dessen Verbindung abreißt ([`NetError::ConnectionLost`]), wird einmal wiederholt: Nach
    /// einem App-Wechsel auf iOS/Android ist die Keep-Alive-Verbindung im Pool oft tot, ohne dass der
    /// Client es weiß. POST wird nie wiederholt (der Server könnte ihn schon verarbeitet haben).
    pub async fn execute(&self, request: Request) -> Result<Response> {
        // nur ein GET wird für die Wiederholung kopiert – ein POST (Dokument, Zugangsdaten) bleibt ein einziges Mal im
        // Speicher
        if request.method != Method::Get {
            return self.execute_once(request).await;
        }
        match self.execute_once(request.clone()).await {
            Err(NetError::ConnectionLost(reason)) => {
                tracing::info!(%reason, "Verbindung abgerissen – GET wird einmal auf neuer Verbindung wiederholt");
                self.execute_once(request).await
            }
            result => result,
        }
    }

    async fn execute_once(&self, request: Request) -> Result<Response> {
        let method = match request.method {
            Method::Get => reqwest::Method::GET,
            Method::Post => reqwest::Method::POST,
        };
        let mut builder = self.inner.request(method, request.url.clone());
        for (name, value) in &request.headers {
            builder = builder.header(name.as_str(), value.as_str());
        }
        builder = match request.body {
            Body::Empty => builder,
            Body::Form(fields) => builder
                .header(reqwest::header::CONTENT_TYPE, "application/x-www-form-urlencoded; charset=UTF-8")
                .body(encode_form(&fields)),
            Body::Multipart(parts) => {
                let (content_type, data) = multipart::encode(&parts);
                builder.header(reqwest::header::CONTENT_TYPE, content_type).body(data)
            }
            Body::Bytes { content_type, data } => {
                builder.header(reqwest::header::CONTENT_TYPE, content_type).body(data)
            }
        };
        tracing::debug!(method = request.method.as_str(), url = %redact_url(&request.url), "HTTP-Request");
        let mut response = builder.send().await.map_err(map_reqwest_error)?;
        let status = response.status();
        let reason = response
            .extensions()
            .get::<hyper::ext::ReasonPhrase>()
            .map(|r| String::from_utf8_lossy(r.as_bytes()).into_owned())
            .or_else(|| status.canonical_reason().map(str::to_owned))
            .unwrap_or_default();
        let url = response.url().clone();
        let headers = response
            .headers()
            .iter()
            .map(|(name, value)| (name.as_str().to_ascii_lowercase(), header_value_to_string(value.as_bytes())))
            .collect();
        if let Some(len) = response.content_length()
            && len > self.max_body as u64
        {
            return Err(body_too_large(self.max_body));
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(map_reqwest_error)? {
            if body.len() + chunk.len() > self.max_body {
                return Err(body_too_large(self.max_body));
            }
            body.extend_from_slice(&chunk);
        }
        tracing::debug!(status = status.as_u16(), body_len = body.len(), "HTTP-Antwort");
        Ok(Response { status: status.as_u16(), reason, url, headers, body })
    }

    /// GET.
    pub async fn get(&self, url: &Url, headers: &[(String, String)]) -> Result<Response> {
        let mut request = Request::get(url.clone());
        request.headers.extend_from_slice(headers);
        self.execute(request).await
    }

    /// POST urlencoded.
    pub async fn post_form(&self, url: &Url, fields: &[(String, String)]) -> Result<Response> {
        self.execute(Request::post(url.clone(), Body::Form(fields.to_vec()))).await
    }

    /// POST multipart in der gegebenen Teil-Reihenfolge.
    pub async fn post_multipart(&self, url: &Url, parts: Vec<FormPart>) -> Result<Response> {
        self.execute(Request::post(url.clone(), Body::Multipart(parts))).await
    }
}

fn body_too_large(max: usize) -> NetError {
    NetError::Body(format!("Antwort größer als {max} Bytes"))
}

/// Proxy für die manuelle Konfiguration (`http://host:port`, Basic-Authentifizierung).
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn manual_proxy(host: &str, port: u16, auth: Option<&ProxyAuth>) -> Result<reqwest::Proxy> {
    let host = host.trim();
    if host.is_empty() {
        return Err(NetError::Config("Proxy-Host fehlt".into()));
    }
    if port == 0 {
        return Err(NetError::Config("Proxy-Port 0 ist ungültig".into()));
    }
    let host_in_url = if host.contains(':') && !host.starts_with('[') { format!("[{host}]") } else { host.to_owned() };
    let url = format!("http://{host_in_url}:{port}");
    Url::parse(&url).map_err(|e| NetError::Config(format!("Proxy-Adresse ungültig: {e}")))?;
    let mut proxy = reqwest::Proxy::all(url.as_str()).map_err(|e| NetError::Config(error_chain(&e)))?;
    if let Some(auth) = auth {
        proxy = proxy.basic_auth(&auth.user, auth.password.as_str());
    }
    Ok(proxy)
}

/// `application/x-www-form-urlencoded` (UTF-8, Leerzeichen als `+`, wie Apache `URLEncodedUtils`).
fn encode_form(fields: &[(String, String)]) -> String {
    url::form_urlencoded::Serializer::new(String::new()).extend_pairs(fields.iter().map(|(k, v)| (k, v))).finish()
}

/// Header-Werte sind Bytes; gültiges UTF-8 wird übernommen, sonst ISO-8859-1.
fn header_value_to_string(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_owned(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

/// Fehlerkette ohne URL (`reqwest::Error::without_url`), für Diagnose.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn error_chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut parts = vec![error.to_string()];
    let mut source = error.source();
    while let Some(s) = source {
        let text = s.to_string();
        if !parts.iter().any(|p| p.contains(&text)) {
            parts.push(text);
        }
        source = s.source();
    }
    parts.join(": ")
}

/// Sucht `T` in der Fehlerkette. `std::io::Error::source()` überspringt den eingebetteten Fehler
/// (rustls/hyper-rustls verpacken TLS-Fehler so); deshalb wird zusätzlich `get_ref()` durchsucht.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn chain_contains<T: std::error::Error + 'static>(
    error: &(dyn std::error::Error + 'static),
    pred: &impl Fn(&T) -> bool,
) -> bool {
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(error);
    let mut depth = 0;
    while let Some(e) = current {
        if let Some(t) = e.downcast_ref::<T>()
            && pred(t)
        {
            return true;
        }
        if let Some(io) = e.downcast_ref::<std::io::Error>()
            && let Some(inner) = io.get_ref()
            && chain_contains(inner, pred)
        {
            return true;
        }
        depth += 1;
        if depth > 32 {
            break;
        }
        current = e.source();
    }
    false
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn map_reqwest_error(error: reqwest::Error) -> NetError {
    let error = error.without_url();
    if error.is_timeout() {
        return NetError::Timeout;
    }
    if error.is_builder() {
        return NetError::Config(error_chain(&error));
    }
    let text = error_chain(&error);
    if text.contains("proxy authorization required") {
        return NetError::Proxy(PROXY_AUTH_REQUIRED.into());
    }
    if chain_contains::<rustls::Error>(&error, &|_| true) {
        return NetError::Tls(text);
    }
    if chain_contains::<hyper::Error>(&error, &|e| e.is_incomplete_message()) {
        return NetError::NoResponse;
    }
    if chain_contains::<std::io::Error>(&error, &|e| {
        use std::io::ErrorKind::*;
        matches!(e.kind(), UnexpectedEof | ConnectionReset | ConnectionAborted | BrokenPipe | NotConnected)
    }) || chain_contains::<hyper::Error>(&error, &|e| e.is_canceled() || e.is_closed())
    {
        return NetError::ConnectionLost(text);
    }
    if chain_contains::<std::io::Error>(&error, &|e| e.kind() == std::io::ErrorKind::TimedOut) {
        return NetError::Timeout;
    }
    if error.is_body() || error.is_decode() {
        return NetError::Body(text);
    }
    if text.contains("tunnel") || text.contains("proxy") {
        return NetError::Proxy(text);
    }
    NetError::Connect(text)
}

/// Installiert den rustls-Crypto-Provider `ring` prozessweit (idempotent). Muss vor dem ersten
/// Client aufgerufen werden; [`HttpClient::new`] tut das selbst. Ist bereits ein anderer Provider
/// installiert, bleibt dieser aktiv.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub fn install_crypto_provider() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            // Fehler = ein anderer Thread war schneller; dessen Provider bleibt.
            let _ = rustls::crypto::ring::default_provider().install_default();
        }
    });
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
/// HTTP-Client im Browser: Der Kern spricht dort nicht selbst mit dem Netz (CORS; A-Trust läuft über das Overlay der
/// Seite, Konzept Webdienst Variante E). Gleiche Schnittstelle, jeder Request scheitert mit [`NetError::Config`].
#[derive(Debug)]
pub struct HttpClient {
    purpose: Purpose,
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
impl HttpClient {
    /// Neuer Client (ohne Verbindung).
    pub fn new(_config: &NetConfig, purpose: Purpose) -> Result<Self> {
        Ok(Self { purpose })
    }

    /// Zweck.
    pub fn purpose(&self) -> Purpose {
        self.purpose
    }

    /// Im Browser nicht möglich.
    pub async fn execute(&self, _request: Request) -> Result<Response> {
        Err(NetError::Config("im Browser spricht der Kern nicht selbst mit dem Netz".into()))
    }

    /// Im Browser nicht möglich (wie [`HttpClient::execute`]).
    pub async fn get(&self, _url: &Url, _headers: &[(String, String)]) -> Result<Response> {
        self.execute(Request::get(Url::parse("about:blank").map_err(|e| NetError::InvalidUrl(e.to_string()))?)).await
    }

    /// Im Browser nicht möglich (wie [`HttpClient::execute`]).
    pub async fn post_form(&self, url: &Url, _fields: &[(String, String)]) -> Result<Response> {
        self.get(url, &[]).await
    }

    /// Im Browser nicht möglich (wie [`HttpClient::execute`]).
    pub async fn post_multipart(&self, url: &Url, _parts: Vec<FormPart>) -> Result<Response> {
        self.get(url, &[]).await
    }
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
/// Im Browser ohne Wirkung (kein rustls).
pub fn install_crypto_provider() {}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
/// Im Browser gibt es keine TCP-Verbindungen – immer `false`.
pub async fn tcp_probe(_addr: SocketAddr, _timeout: Duration) -> bool {
    false
}

/// Löst `href` relativ zur Basis auf (wie jsoup `absUrl`: ASCII-Steuerzeichen werden entfernt;
/// ungültig → Fehler).
pub fn resolve(base: &Url, href: &str) -> Result<Url> {
    let cleaned: String = href.chars().filter(|c| !c.is_ascii_control()).collect();
    base.join(cleaned.trim()).map_err(|e| NetError::InvalidUrl(e.to_string()))
}

/// TCP-Probe (lokale BKU, `127.0.0.1:3495`, docs/analyse/02 §4.1).
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub async fn tcp_probe(addr: SocketAddr, timeout: Duration) -> bool {
    matches!(tokio::time::timeout(timeout, tokio::net::TcpStream::connect(addr)).await, Ok(Ok(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_shows_form_values_or_tokens() {
        let part = FormPart {
            name: "signaturpasswort".into(),
            filename: None,
            content_type: Some("text/plain; charset=UTF-8".into()),
            data: b"geheim-123".to_vec(),
        };
        let mut req = Request::post(Url::parse("https://service.a-trust.at/x").unwrap(), Body::Multipart(vec![part]));
        req.headers.push(("Authorization".into(), "Bearer TOKEN-XYZ".into()));
        let dbg = format!("{req:?}");
        assert!(dbg.contains("signaturpasswort"));
        assert!(!dbg.contains("geheim-123"), "{dbg}");
        assert!(!dbg.contains("TOKEN-XYZ"), "{dbg}");

        let form = Body::Form(vec![("XMLRequest".into(), "<sl:Base64Content>JVBERi0=".into())]);
        assert!(!format!("{form:?}").contains("JVBERi0"));
    }

    #[test]
    fn debug_hides_query_and_cookies() {
        let req = Request::get(Url::parse("https://service.a-trust.at/mobile/a.aspx?sid=GEHEIM").unwrap());
        assert!(!format!("{req:?}").contains("GEHEIM"));
        let resp = Response {
            status: 200,
            reason: "OK".into(),
            url: Url::parse("https://h/p?sid=GEHEIM").unwrap(),
            headers: vec![("set-cookie".into(), "ASP.NET_SessionId=KEKS".into())],
            body: b"<html>Inhalt</html>".to_vec(),
        };
        let dbg = format!("{resp:?}");
        assert!(!dbg.contains("GEHEIM") && !dbg.contains("KEKS") && !dbg.contains("Inhalt"), "{dbg}");
        assert_eq!(redact_url(&Url::parse("http://127.0.0.1:3495/x?a=b#f").unwrap()), "http://127.0.0.1:3495/x?…");
    }

    fn response_with(headers: &[(&str, &str)], body: &[u8]) -> Response {
        Response {
            status: 200,
            reason: "OK".into(),
            url: Url::parse("https://example.invalid/").unwrap(),
            headers: headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            body: body.to_vec(),
        }
    }

    #[test]
    fn charset_and_text() {
        let r = response_with(&[("content-type", "text/html; charset=\"ISO-8859-1\"")], b"Gr\xfc\xdfe");
        assert_eq!(r.charset().as_deref(), Some("ISO-8859-1"));
        assert_eq!(r.mime().as_deref(), Some("text/html"));
        assert_eq!(r.text(), "Grüße");

        let r = response_with(&[("Content-Type", "TEXT/XML;Charset=utf-8")], "Grüße".as_bytes());
        assert_eq!(r.charset().as_deref(), Some("utf-8"));
        assert_eq!(r.mime().as_deref(), Some("text/xml"));
        assert_eq!(r.text(), "Grüße");

        // ohne Charset → UTF-8; unbekanntes Charset → UTF-8; ungültige Bytes → Ersatzzeichen
        let r = response_with(&[("content-type", "text/html")], "ä".as_bytes());
        assert_eq!(r.charset(), None);
        assert_eq!(r.text(), "ä");
        let r = response_with(&[("content-type", "text/html; charset=x-unbekannt")], b"a\xffb");
        assert_eq!(r.text(), "a\u{fffd}b");
        let r = response_with(&[], b"x");
        assert_eq!(r.mime(), None);
        assert_eq!(r.charset(), None);
    }

    #[test]
    fn windows_1252_label() {
        let r = response_with(&[("content-type", "text/html; charset=windows-1252")], b"\x80");
        assert_eq!(r.text(), "€");
    }

    #[test]
    fn date_header() {
        let r = response_with(&[("date", "Tue, 14 Oct 2026 15:04:05 GMT")], b"");
        let d = r.date().unwrap();
        assert_eq!((d.year(), d.month() as u8, d.day(), d.hour(), d.minute(), d.second()), (2026, 10, 14, 15, 4, 5));
        assert!(response_with(&[("date", "gestern")], b"").date().is_none());
        assert!(response_with(&[], b"").date().is_none());
    }

    #[test]
    fn resolve_like_jsoup_absurl() {
        let base = Url::parse("https://service.a-trust.at/mobile/https-security-layer-request/default.aspx").unwrap();
        assert_eq!(
            resolve(&base, "Login.aspx?sid=1").unwrap().as_str(),
            "https://service.a-trust.at/mobile/https-security-layer-request/Login.aspx?sid=1"
        );
        assert_eq!(resolve(&base, "/a/b").unwrap().as_str(), "https://service.a-trust.at/a/b");
        assert_eq!(resolve(&base, "  ./x\n.aspx ").unwrap().path(), "/mobile/https-security-layer-request/x.aspx");
        assert_eq!(resolve(&base, "https://other.example/").unwrap().as_str(), "https://other.example/");
        assert_eq!(resolve(&base, "").unwrap(), base);
        assert!(resolve(&base, "http://[::1").is_err());
    }

    #[test]
    fn purposes() {
        assert!(Purpose::AtrustSession.uses_cookies());
        assert!(!Purpose::AtrustPoll.uses_cookies());
        assert!(!Purpose::AtrustImage.uses_cookies());
        assert!(!Purpose::LocalBku.uses_cookies());
        assert!(!Purpose::AtrustSession.follows_redirects());
        assert!(!Purpose::LocalBku.follows_redirects());
        assert!(Purpose::AtrustPoll.follows_redirects());
        assert!(!Purpose::LocalBku.allows_proxy());
        assert!(Purpose::AtrustSession.allows_proxy());
        // nur der Update-Check verlangt `https` (A-Trust-Testserver und lokale BKU bleiben, wie sie sind)
        assert!(Purpose::UpdateCheck.requires_https() && Purpose::UpdateCheck.follows_redirects());
        assert!(Purpose::UpdateCheck.allows_proxy() && !Purpose::UpdateCheck.uses_cookies());
        for p in [Purpose::AtrustSession, Purpose::AtrustPoll, Purpose::AtrustImage, Purpose::LocalBku] {
            assert!(!p.requires_https(), "{p:?}");
        }
    }

    #[test]
    fn form_encoding_matches_apache() {
        let s = encode_form(&[("XMLRequest".into(), "<a b=\"c\">ä &+/*-._~</a>".into())]);
        assert_eq!(s, "XMLRequest=%3Ca+b%3D%22c%22%3E%C3%A4+%26%2B%2F*-._%7E%3C%2Fa%3E");
    }

    #[test]
    fn manual_proxy_validation() {
        assert!(manual_proxy("", 8080, None).is_err());
        assert!(manual_proxy("proxy.example", 0, None).is_err());
        assert!(manual_proxy("proxy.example", 3128, None).is_ok());
        assert!(manual_proxy("::1", 3128, None).is_ok());
        let auth = ProxyAuth { user: "u".into(), password: Zeroizing::new("p".into()) };
        assert!(manual_proxy("10.0.0.1", 3128, Some(&auth)).is_ok());
        assert!(!format!("{auth:?}").contains('p') || format!("{auth:?}").contains("***"));
    }

    #[test]
    fn client_construction_all_purposes() {
        let cfg = NetConfig::default();
        for p in
            [Purpose::AtrustSession, Purpose::AtrustPoll, Purpose::AtrustImage, Purpose::LocalBku, Purpose::UpdateCheck]
        {
            let c = HttpClient::new(&cfg, p).unwrap();
            assert_eq!(c.purpose(), p);
        }
        let manual = NetConfig {
            proxy: ProxyConfig::Manual { host: "proxy.example".into(), port: 8080, auth: None },
            request_timeout: None,
            ..NetConfig::default()
        };
        HttpClient::new(&manual, Purpose::AtrustSession).unwrap();
        assert!(DEFAULT_USER_AGENT.starts_with("PIDA v"));
        assert_eq!(cfg.effective_user_agent(), DEFAULT_USER_AGENT);
    }

    #[test]
    fn proxy_auth_detection() {
        assert!(NetError::Proxy(PROXY_AUTH_REQUIRED.into()).is_proxy_auth_required());
        assert!(!NetError::Proxy("anderes".into()).is_proxy_auth_required());
        assert!(!NetError::Timeout.is_proxy_auth_required());
    }
}
