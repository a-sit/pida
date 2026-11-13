//! `pida-web-server` – Server der Web-Variante (Konzept Webdienst, Variante E).
//!
//! - **Oberfläche:** liefert `apps/web/dist` statisch aus (HTML, JavaScript, WebAssembly).
//! - **Rückkanal** ([`relay`]): DataURL für die SL-Befehlskette des A-Trust-Overlays (`/api/sl/…`).
//! - **`/api/config`:** SL-Endpunkt für das Overlay (A-Trust bzw. Testgegenstelle).
//! - **Testgegenstelle** ([`mock`], nur `--mock-atrust`): ersetzt A-Trust für lokale Tests.
//!
//! ```text
//! pida-web-server [--bind 127.0.0.1:1442] [--dir apps/web/dist] [--sl-endpoint URL]
//!                 [--mock-atrust testdata/keystores/test-rsa2048.p12 --mock-password test]
//! ```
//!
//! Im Betrieb steht davor ein TLS-Proxy, der unter der öffentlichen Adresse erreichbar ist: A-Trust muss die
//! DataURL (`https://<host>/api/sl/data/<id>`) erreichen können. Gespeichert wird nichts; SL-Inhalte liegen nur bis zur
//! Weitergabe im Speicher und werden nicht protokolliert.

mod form;
mod mock;
mod relay;

use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Incoming;
use hyper::header::{AUTHORIZATION, CACHE_CONTROL, CONTENT_TYPE, ETAG, IF_NONE_MATCH};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};

use crate::relay::{Relay, RelayError, Reply};

/// Produktiv-Endpunkt von A-Trust (SL-HTTPS-Binding).
const ATRUST_ENDPOINT: &str = "https://service.a-trust.at/mobile/https-security-layer-request/default.aspx";
/// Höchstgröße der SL-Antwort von A-Trust an die DataURL.
const MAX_DATA_URL_BODY: usize = 4 << 20;
/// Höchstgröße eines Befehls der Seite (Dokument als Base64): A-Trust nimmt höchstens 20 MB an (der Kern lehnt
/// größere PDFs schon beim Laden ab, `MAX_DOCUMENT_BYTES`), Base64 macht daraus knapp 27 MB.
const MAX_COMMAND_BODY: usize = 28 << 20;
/// So lange darf der Kopf einer Anfrage brauchen (sonst hält eine stumme Verbindung den Platz beliebig lange).
const HEADER_TIMEOUT: Duration = Duration::from_secs(20);
/// So lange darf der Rumpf einer Anfrage brauchen (Dokument als Base64 über eine langsame Leitung).
const BODY_TIMEOUT: Duration = Duration::from_secs(120);
/// Pause nach einem Fehler beim Annehmen einer Verbindung (z. B. keine freien Dateideskriptoren).
const ACCEPT_BACKOFF: Duration = Duration::from_millis(100);

struct App {
    dir: PathBuf,
    relay: Relay,
    sl_endpoint: String,
    /// `Content-Security-Policy` der Oberfläche ([`page_csp`])
    page_csp: String,
    mock: Option<mock::MockAtrust>,
}

type Resp = Response<Full<Bytes>>;

fn respond(status: StatusCode, content_type: &str, body: impl Into<Bytes>) -> Resp {
    let mut r = Response::new(Full::new(body.into()));
    *r.status_mut() = status;
    let h = r.headers_mut();
    if let Ok(v) = content_type.parse() {
        h.insert(CONTENT_TYPE, v);
    }
    h.insert("X-Content-Type-Options", "nosniff".parse().expect("konstant"));
    h.insert("Referrer-Policy", "no-referrer".parse().expect("konstant"));
    r
}

fn status(status: StatusCode) -> Resp {
    respond(status, "text/plain; charset=utf-8", status.canonical_reason().unwrap_or(""))
}

fn no_store(mut r: Resp) -> Resp {
    r.headers_mut().insert(CACHE_CONTROL, "no-store".parse().expect("konstant"));
    r
}

async fn read_body(req: Request<Incoming>, limit: usize) -> Result<(String, Vec<u8>), StatusCode> {
    let content_type = req.headers().get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_owned();
    match tokio::time::timeout(BODY_TIMEOUT, Limited::new(req.into_body(), limit).collect()).await {
        Ok(Ok(b)) => Ok((content_type, b.to_bytes().to_vec())),
        Ok(Err(_)) => Err(StatusCode::PAYLOAD_TOO_LARGE),
        Err(_) => Err(StatusCode::REQUEST_TIMEOUT),
    }
}

fn bearer(req: &Request<Incoming>) -> String {
    req.headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("")
        .to_owned()
}

fn relay_status(e: RelayError) -> Resp {
    match e {
        RelayError::NotFound => status(StatusCode::GONE),
        RelayError::Busy => status(StatusCode::SERVICE_UNAVAILABLE),
        RelayError::TooMany => status(StatusCode::TOO_MANY_REQUESTS),
        RelayError::NotWaiting => status(StatusCode::CONFLICT),
        RelayError::Invalid => status(StatusCode::BAD_REQUEST),
    }
}

/// Abschlussseite für A-Trust (wird im Overlay angezeigt; die Seite schließt das Overlay selbst).
fn final_page(done: bool) -> Resp {
    let (title, text) = if done {
        ("Signatur übernommen", "PIDA hat die Signatur übernommen. Dieses Fenster schließt sich gleich.")
    } else {
        ("Abgebrochen", "Der Vorgang wurde abgebrochen. Sie können dieses Fenster schließen.")
    };
    respond(
        StatusCode::OK,
        "text/html; charset=utf-8",
        format!(
            r#"<!doctype html><html lang="de"><head><meta charset="utf-8"><title>{title}</title></head><body style="font:15px/1.5 system-ui,sans-serif;padding:24px"><h1 style="font-size:20px">{title}</h1><p>{text}</p></body></html>"#
        ),
    )
}

fn content_type_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "wasm" => "application/wasm",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

/// Kennung des Dateistands (Größe und Änderungszeit) für bedingte Anfragen.
fn etag_for(meta: &std::fs::Metadata) -> Option<String> {
    let modified = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(format!("\"{:x}-{:x}\"", meta.len(), modified.as_nanos()))
}

/// Herkunft (`scheme://host[:port]`) einer absoluten `http(s)`-Adresse; relative Adressen (Testgegenstelle) → `None`.
fn origin_of(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let host = rest.split(['/', '?', '#']).next().filter(|h| !h.is_empty() && !h.contains(['@', ' ', ';', ',']))?;
    Some(&url[..url.len() - rest.len() + host.len()])
}

/// `Content-Security-Policy` der Oberfläche: Skripte und WebAssembly nur von der eigenen Herkunft, Rahmen und
/// Formularziele nur A-Trust (bzw. der eingestellte SL-Endpunkt und die eigene Testgegenstelle), keine Einbettung
/// in fremde Seiten.
///
/// - `'wasm-unsafe-eval'`: Kern und PDFium sind WebAssembly; `'unsafe-inline'` nur für Stile (Positionen und Größen
///   setzt die Oberfläche als `style`-Attribut).
/// - `img-src … data: blob:`: Seitenbilder und Bildmarke entstehen im Browser.
/// - `connect-src`: eigener Rückkanal und die Release-Liste auf GitHub (Download-Dialog).
/// - `frame-src`/`form-action`: Die SL-Befehle gehen als Formular in das A-Trust-Overlay; dort leitet A-Trust
///   innerhalb seiner Domains weiter (`a-trust.at`, `handy-signatur.at` – dieselben wie in der App).
fn page_csp(sl_endpoint: &str) -> String {
    let mut sl = String::from("'self' https://*.a-trust.at https://*.handy-signatur.at");
    if let Some(origin) = origin_of(sl_endpoint) {
        sl.push(' ');
        sl.push_str(origin);
    }
    format!(
        "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; \
         img-src 'self' data: blob:; font-src 'self' data:; connect-src 'self' https://api.github.com; \
         frame-src {sl}; form-action {sl}; frame-ancestors 'none'; base-uri 'self'; object-src 'none'"
    )
}

/// Kopfzeilen der Oberfläche: Inhaltsrichtlinie ([`page_csp`]) und keine Einbettung in fremde Seiten (Clickjacking).
fn page_headers(mut r: Resp, csp: &str) -> Resp {
    let h = r.headers_mut();
    h.insert("X-Frame-Options", "DENY".parse().expect("konstant"));
    h.insert("X-Content-Type-Options", "nosniff".parse().expect("konstant"));
    h.insert("Referrer-Policy", "no-referrer".parse().expect("konstant"));
    if let Ok(v) = csp.parse() {
        h.insert("Content-Security-Policy", v);
    }
    r
}

/// Adresse des Clients für die Grenzen je Client. Hinter dem TLS-Proxy (Verbindung von der eigenen Maschine) steht
/// sie im letzten Eintrag von `X-Forwarded-For` – den hängt der Proxy selbst an, frühere Einträge kann der Client
/// frei erfinden. Direkte Verbindungen zählen mit ihrer eigenen Adresse; der Kopf wird dann nicht beachtet.
fn client_ip(peer: IpAddr, forwarded_for: Option<&str>) -> IpAddr {
    if !peer.is_loopback() {
        return peer;
    }
    forwarded_for.and_then(|v| v.rsplit(',').next()).and_then(|last| last.trim().parse().ok()).unwrap_or(peer)
}

async fn static_file(dir: &Path, uri_path: &str, if_none_match: Option<&str>, csp: &str) -> Resp {
    let rel = uri_path.trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    // Dateien unter assets/ tragen einen Hash im Namen (Vite) und ändern sich nie; alles andere wird beim Server
    // nachgefragt und kommt bei unverändertem Stand ohne Inhalt zurück (WebAssembly-Kern, PDFium: mehrere MB).
    let cache = if rel.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
    let rel = Path::new(rel);
    if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        return status(StatusCode::NOT_FOUND);
    }
    let path = dir.join(rel);
    let etag = match tokio::fs::metadata(&path).await {
        Ok(meta) if meta.is_file() => etag_for(&meta),
        _ => return status(StatusCode::NOT_FOUND),
    };
    let unchanged = match (&etag, if_none_match) {
        (Some(tag), Some(sent)) => sent.split(',').any(|s| s.trim().trim_start_matches("W/") == tag),
        _ => false,
    };
    let mut r = if unchanged {
        let mut r = Response::new(Full::new(Bytes::new()));
        *r.status_mut() = StatusCode::NOT_MODIFIED;
        r
    } else {
        match tokio::fs::read(&path).await {
            Ok(data) => respond(StatusCode::OK, content_type_for(&path), data),
            Err(_) => return status(StatusCode::NOT_FOUND),
        }
    };
    r.headers_mut().insert(CACHE_CONTROL, cache.parse().expect("konstant"));
    if let Some(tag) = etag.and_then(|t| t.parse().ok()) {
        r.headers_mut().insert(ETAG, tag);
    }
    page_headers(r, csp)
}

async fn handle(app: Arc<App>, peer: IpAddr, req: Request<Incoming>) -> Result<Resp, Infallible> {
    let path = req.uri().path().to_owned();
    let method = req.method().clone();
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let resp = match (&method, parts.as_slice()) {
        (&Method::GET, ["api", "config"]) => no_store(respond(
            StatusCode::OK,
            "application/json",
            serde_json::json!({ "sl_endpoint": app.sl_endpoint }).to_string(),
        )),
        (&Method::POST, ["api", "sl", "sessions"]) => {
            let forwarded = req.headers().get("x-forwarded-for").and_then(|v| v.to_str().ok());
            match app.relay.create(client_ip(peer, forwarded)) {
                Ok((id, token)) => no_store(respond(
                    StatusCode::OK,
                    "application/json",
                    serde_json::json!({ "id": id, "token": token }).to_string(),
                )),
                Err(e) => relay_status(e),
            }
        }
        // DataURL: A-Trust schickt die SL-Antwort und wartet auf den nächsten Befehl
        (&Method::POST, ["api", "sl", "data", id]) => {
            let id = (*id).to_owned();
            // unbekannte Sitzung: gleich die Abschlussseite, ohne den Rumpf anzunehmen
            match if app.relay.exists(&id) { read_body(req, MAX_DATA_URL_BODY).await.map(Some) } else { Ok(None) } {
                Err(code) => status(code),
                Ok(None) => no_store(final_page(false)),
                Ok(Some((ct, body))) => {
                    let fields = form::fields(&ct, &body);
                    match form::field(&fields, "XMLResponse") {
                        None => status(StatusCode::BAD_REQUEST),
                        Some(xml) => match app.relay.deliver(&id, xml.to_vec()).await {
                            Reply::Command(cmd) => no_store(respond(StatusCode::OK, "text/xml; charset=UTF-8", cmd)),
                            Reply::Finish => no_store(final_page(true)),
                            Reply::Abort => no_store(final_page(false)),
                        },
                    }
                }
            }
        }
        (&Method::GET, ["api", "sl", "sessions", id, "response"]) => {
            let token = bearer(&req);
            match app.relay.take_response(id, &token).await {
                Ok(Some(xml)) => no_store(respond(StatusCode::OK, "text/xml; charset=UTF-8", xml)),
                Ok(None) => no_store(status(StatusCode::NO_CONTENT)),
                Err(e) => relay_status(e),
            }
        }
        (&Method::POST, ["api", "sl", "sessions", id, "command"]) => {
            let id = (*id).to_owned();
            let token = bearer(&req);
            // erst prüfen, dann lesen: ohne gültige Sitzung nimmt der Server keine 28 MB an
            if !app.relay.is_authorized(&id, &token) {
                return Ok(api_response(relay_status(RelayError::NotFound)));
            }
            match read_body(req, MAX_COMMAND_BODY).await {
                Err(code) => status(code),
                Ok((_, body)) => match String::from_utf8(body) {
                    Err(_) => status(StatusCode::BAD_REQUEST),
                    Ok(xml) => match app.relay.command(&id, &token, xml) {
                        Ok(()) => status(StatusCode::NO_CONTENT),
                        Err(e) => relay_status(e),
                    },
                },
            }
        }
        (&Method::POST, ["api", "sl", "sessions", id, outcome @ ("finish" | "abort")]) => {
            match app.relay.finish(id, &bearer(&req), *outcome == "finish") {
                Ok(()) => status(StatusCode::NO_CONTENT),
                Err(e) => relay_status(e),
            }
        }
        (&Method::POST, ["mock-atrust", "sl"]) if app.mock.is_some() => match read_body(req, MAX_COMMAND_BODY).await {
            Err(code) => status(code),
            Ok((ct, body)) => {
                let fields = form::fields(&ct, &body);
                let xml = form::field(&fields, "XMLRequest").map(|v| String::from_utf8_lossy(v).into_owned());
                let data_url = form::field(&fields, "DataURL").map(|v| String::from_utf8_lossy(v).into_owned());
                match (xml, data_url, &app.mock) {
                    (Some(xml), Some(data_url), Some(m)) => {
                        no_store(respond(StatusCode::OK, "text/html; charset=utf-8", m.start(&xml, &data_url).0))
                    }
                    _ => status(StatusCode::BAD_REQUEST),
                }
            }
        },
        (&Method::POST, ["mock-atrust", action @ ("confirm" | "cancel"), id]) if app.mock.is_some() => {
            let m = app.mock.as_ref().expect("geprüft");
            no_store(respond(StatusCode::OK, "text/html; charset=utf-8", m.answer(id, *action == "confirm").await.0))
        }
        (&Method::GET | &Method::HEAD, _) if !path.starts_with("/api/") => {
            let if_none_match = req.headers().get(IF_NONE_MATCH).and_then(|v| v.to_str().ok());
            static_file(&app.dir, &path, if_none_match, &app.page_csp).await
        }
        _ => status(StatusCode::NOT_FOUND),
    };
    Ok(if path.starts_with("/api/") { api_response(resp) } else { resp })
}

/// API-Antworten (SL-XML, Abschlussseiten) dürfen im Browser nie als Dokument dieser Herkunft laufen.
fn api_response(mut resp: Resp) -> Resp {
    resp.headers_mut().insert("Content-Security-Policy", "sandbox; default-src 'none'".parse().expect("konstant"));
    resp
}

struct Args {
    bind: SocketAddr,
    dir: PathBuf,
    sl_endpoint: Option<String>,
    mock_keystore: Option<PathBuf>,
    mock_password: String,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        bind: "127.0.0.1:1442".parse().expect("konstant"),
        dir: PathBuf::from("apps/web/dist"),
        sl_endpoint: None,
        mock_keystore: None,
        mock_password: String::new(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{arg}: Wert fehlt"));
        match arg.as_str() {
            "--bind" => a.bind = value()?.parse().map_err(|e| format!("--bind: {e}"))?,
            "--dir" => a.dir = value()?.into(),
            "--sl-endpoint" => a.sl_endpoint = Some(value()?),
            "--mock-atrust" => a.mock_keystore = Some(value()?.into()),
            "--mock-password" => a.mock_password = value()?,
            "-V" | "--version" => {
                println!("pida-web-server {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "-h" | "--help" => {
                return Err("pida-web-server [--bind ADR] [--dir DIST] [--sl-endpoint URL] [--mock-atrust P12 --mock-password PW]".into());
            }
            other => return Err(format!("unbekanntes Argument {other}")),
        }
    }
    Ok(a)
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::new("info")).init();
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let mock = match &args.mock_keystore {
        None => None,
        Some(p12) => {
            let bytes = std::fs::read(p12).unwrap_or_else(|e| {
                eprintln!("Test-Keystore {}: {e}", p12.display());
                std::process::exit(2);
            });
            let key =
                pida_backend_keystore::LoadedKey::open_pkcs12(&bytes, &args.mock_password, None).unwrap_or_else(|e| {
                    eprintln!("Test-Keystore nicht lesbar: {e}");
                    std::process::exit(2);
                });
            tracing::warn!("Testgegenstelle statt A-Trust aktiv – nur für lokale Tests");
            Some(mock::MockAtrust::new(key, args.bind.port()))
        }
    };
    let sl_endpoint = match (&args.sl_endpoint, &mock) {
        (Some(e), _) => e.clone(),
        (None, Some(_)) => "/mock-atrust/sl".to_owned(),
        (None, None) => ATRUST_ENDPOINT.to_owned(),
    };
    let page_csp = page_csp(&sl_endpoint);
    let app = Arc::new(App { dir: args.dir, relay: Relay::default(), sl_endpoint, page_csp, mock });
    let sweeper = app.relay.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            sweeper.sweep();
        }
    });
    let listener = tokio::net::TcpListener::bind(args.bind).await.unwrap_or_else(|e| {
        eprintln!("{}: {e}", args.bind);
        std::process::exit(1);
    });
    println!("PIDA (Web) auf http://{}/", args.bind);
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok((stream, peer)) => (stream, peer.ip()),
            Err(e) => {
                // z. B. keine freien Dateideskriptoren: nicht in einer engen Schleife drehen
                tracing::warn!("Verbindung nicht angenommen: {e}");
                tokio::time::sleep(ACCEPT_BACKOFF).await;
                continue;
            }
        };
        let app = Arc::clone(&app);
        tokio::spawn(async move {
            let service = service_fn(move |req| handle(Arc::clone(&app), peer, req));
            let _ = http1::Builder::new()
                .timer(TokioTimer::new())
                .header_read_timeout(HEADER_TIMEOUT)
                .serve_connection(TokioIo::new(stream), service)
                .await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_address_comes_from_the_proxy_only() {
        let local: IpAddr = "127.0.0.1".parse().unwrap();
        let direct: IpAddr = "203.0.113.7".parse().unwrap();
        let client: IpAddr = "198.51.100.4".parse().unwrap();
        // hinter dem Proxy: letzter Eintrag (vom Proxy angehängt), erfundene Einträge davor zählen nicht
        assert_eq!(client_ip(local, Some("198.51.100.4")), client);
        assert_eq!(client_ip(local, Some("10.0.0.1, 198.51.100.4")), client);
        assert_eq!(client_ip("::1".parse().unwrap(), Some("2001:db8::1")), "2001:db8::1".parse::<IpAddr>().unwrap());
        // ohne bzw. mit unlesbarem Kopf: die Verbindung selbst
        assert_eq!(client_ip(local, None), local);
        assert_eq!(client_ip(local, Some("unbekannt")), local);
        // direkte Verbindung: der Kopf kommt vom Client und wird nicht beachtet
        assert_eq!(client_ip(direct, Some("198.51.100.4")), direct);
    }

    #[test]
    fn page_policy_allows_only_own_code_and_atrust_frames() {
        let csp = page_csp(ATRUST_ENDPOINT);
        assert!(csp.contains("script-src 'self' 'wasm-unsafe-eval';"));
        assert!(!csp.contains("script-src 'self' 'unsafe-inline'") && !csp.contains("'unsafe-eval'"));
        assert!(
            csp.contains(
                "frame-src 'self' https://*.a-trust.at https://*.handy-signatur.at https://service.a-trust.at;"
            )
        );
        assert!(csp.contains("form-action 'self' https://*.a-trust.at"));
        assert!(csp.contains("frame-ancestors 'none'"));
        // Testgegenstelle (relative Adresse): nur die eigene Herkunft kommt dazu
        assert!(
            page_csp("/mock-atrust/sl").contains("frame-src 'self' https://*.a-trust.at https://*.handy-signatur.at;")
        );
        // eigener Endpunkt: genau dessen Herkunft, nichts aus Pfad oder Abfrage
        assert_eq!(origin_of("https://sl.example.at:8443/pfad?x=1"), Some("https://sl.example.at:8443"));
        assert_eq!(origin_of("https://sl.example.at"), Some("https://sl.example.at"));
        assert_eq!(origin_of("https://a; script-src *"), None);
        assert_eq!(origin_of("/mock-atrust/sl"), None);
    }
}
