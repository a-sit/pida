//! Long-Poll für QR- und App-Bestätigung (docs/analyse/02 §3.6; `MobileBKUConnector.LongPollThread`).
//!
//! | Antwort | Verhalten |
//! |---|---|
//! | `Fin == true` | Seite neu laden (R7) |
//! | `Wait == true` | sofort erneut pollen |
//! | `Error == true` | ebenfalls neu laden (A-Trust zeigt dann den Fehler) |
//! | weder noch | Schleife beenden, Maske wartet weiter auf die Nutzerin |
//! | kein JSON / Schlüssel fehlt | 10 s warten, dann neu laden |
//! | keine Antwort / Timeout / Verbindung abgerissen | weiter pollen; nach 300 s seit Beginn neu laden |
//! | sonstiger Netzfehler | 10 s warten, dann neu laden |
//!
//! Der Poll-Client hat **keine Cookies** und sendet `Authorization: Bearer <Token>`. Abweichung zum
//! Schutz des Servers: Folgt die nächste Anfrage schneller als [`MIN_POLL_INTERVAL`], wird bis dahin
//! gewartet (PDF-Over pollt ohne Pause).

use std::time::Duration;

use pida_backend_api::CancelToken;
use pida_net::{HttpClient, NetError, Request, Url};

/// Mindestabstand zweier Polls (nur wirksam, wenn der Server nicht hält).
pub const MIN_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Poll-Info aus `qrpoll("URL", "TOKEN");` im ersten `<script>` in `#jsLongPoll`.
/// `Debug` schwärzt das Token (ADR 0001 §4 Regel 9).
#[derive(Clone, PartialEq, Eq)]
pub struct PollInfo {
    /// Poll-URL (relativ zur Dokument-Basis aufgelöst)
    pub url: Url,
    /// Bearer-Token
    pub token: String,
}

impl std::fmt::Debug for PollInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PollInfo").field("url", &pida_net::redact_url(&self.url)).field("token", &"***").finish()
    }
}

/// Extrahiert die Poll-Info aus dem Skripttext (`qrpoll("…", "…");`, `ATrustParser.java:52-77`).
pub fn parse_poll_script(script: &str, base: &Url) -> Option<PollInfo> {
    parse_poll_script_named("qrpoll", script, base)
}

/// Wie [`parse_poll_script`] mit dem Funktionsnamen aus der Selektor-Datei.
pub(crate) fn parse_poll_script_named(function: &str, script: &str, base: &Url) -> Option<PollInfo> {
    let marker = format!("{function}(\"");
    let start = script.find(&marker)? + marker.len();
    let mid = start + script[start..].find("\", \"")?;
    let url = &script[start..mid];
    let token_start = mid + 4;
    let end = token_start + script[token_start..].find("\");")?;
    let token = &script[token_start..end];
    let url = pida_net::resolve(base, url).ok()?;
    Some(PollInfo { url, token: token.to_owned() })
}

/// Antwort eines Polls (`getBoolean` in der Reihenfolge `Fin`, `Wait`, `Error`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PollStatus {
    /// `Fin == true` → Seite neu laden (R7), dann 500 ms Pause
    Finished,
    /// `Wait == true` → sofort weiter
    Wait,
    /// `Error == true` → ebenfalls neu laden (A-Trust zeigt den Fehler)
    Error,
    /// weder noch → Schleife beenden, UI wartet weiter auf die Nutzerin
    Unknown,
    /// JSON-Fehler/fehlender Schlüssel → 10 s warten, dann neu laden
    Malformed,
}

/// org.json `getBoolean`: `true`/`false` bzw. die Zeichenketten `"true"`/`"false"` (ohne
/// Groß-/Kleinschreibung); alles andere wirft.
fn get_boolean(object: &serde_json::Map<String, serde_json::Value>, key: &str) -> Option<bool> {
    match object.get(key)? {
        serde_json::Value::Bool(b) => Some(*b),
        serde_json::Value::String(s) if s.eq_ignore_ascii_case("true") => Some(true),
        serde_json::Value::String(s) if s.eq_ignore_ascii_case("false") => Some(false),
        _ => None,
    }
}

/// Wertet die JSON-Antwort aus.
pub fn parse_poll_response(body: &[u8]) -> PollStatus {
    let text = String::from_utf8_lossy(body);
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text.trim_start_matches('\u{feff}')) else {
        return PollStatus::Malformed;
    };
    let Some(object) = value.as_object() else { return PollStatus::Malformed };
    match get_boolean(object, "Fin") {
        None => PollStatus::Malformed,
        Some(true) => PollStatus::Finished,
        Some(false) => match get_boolean(object, "Wait") {
            None => PollStatus::Malformed,
            Some(true) => PollStatus::Wait,
            Some(false) => match get_boolean(object, "Error") {
                None => PollStatus::Malformed,
                Some(true) => PollStatus::Error,
                Some(false) => PollStatus::Unknown,
            },
        },
    }
}

/// Zeitgrenzen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PollConfig {
    /// Gesamtzeit („a-trust timeout is 5 minutes“)
    pub total_timeout: Duration,
    /// Pause nach Fehler/Exception
    pub error_backoff: Duration,
    /// Pause nach „fertig“ (PDF-Over pausiert den Poll-Thread nach dem Signal; hier endet die
    /// Schleife mit dem Signal, die Pause entfällt deshalb)
    pub after_finish: Duration,
}

impl Default for PollConfig {
    fn default() -> Self {
        Self {
            total_timeout: Duration::from_secs(300),
            error_backoff: Duration::from_secs(10),
            after_finish: Duration::from_millis(500),
        }
    }
}

/// Ergebnis der Poll-Schleife.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PollOutcome {
    /// Seite neu laden (fertig, Fehler, Timeout)
    Reload,
    /// Poll beendet ohne Signal – weiter auf die Nutzerin warten
    Stopped,
    /// abgebrochen
    Cancelled,
}

/// Schläft, bis `duration` vorbei ist oder `cancel` auslöst (`false` = abgebrochen).
async fn sleep_or_cancel(duration: Duration, cancel: &CancelToken) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(duration) => true,
        _ = cancel.cancelled() => false,
    }
}

/// Führt die Poll-Schleife aus (eigener Client **ohne** Cookies, `Authorization: Bearer`).
/// Drop-sicher: Wird das Future gedroppt, endet der laufende Request.
pub async fn run(client: &HttpClient, info: &PollInfo, config: PollConfig, cancel: &CancelToken) -> PollOutcome {
    let start = tokio::time::Instant::now();
    let deadline = start + config.total_timeout;
    let mut last_request: Option<tokio::time::Instant> = None;
    loop {
        if cancel.is_cancelled() {
            return PollOutcome::Cancelled;
        }
        if let Some(last) = last_request {
            let since = last.elapsed();
            if since < MIN_POLL_INTERVAL && !sleep_or_cancel(MIN_POLL_INTERVAL - since, cancel).await {
                return PollOutcome::Cancelled;
            }
        }
        last_request = Some(tokio::time::Instant::now());
        let request = Request::get(info.url.clone()).with_header("Authorization", format!("Bearer {}", info.token));
        let result = tokio::select! {
            r = client.execute(request) => r,
            _ = cancel.cancelled() => return PollOutcome::Cancelled,
        };
        match result {
            Ok(response) => match parse_poll_response(&response.body) {
                PollStatus::Finished | PollStatus::Error => return PollOutcome::Reload,
                // „weiter warten“ – aber nicht über die A-Trust-Frist hinaus (dann zeigt die neu geladene Seite den Stand)
                PollStatus::Wait if tokio::time::Instant::now() >= deadline => return PollOutcome::Reload,
                PollStatus::Wait => continue,
                PollStatus::Unknown => {
                    tracing::warn!("unbekannte Long-Poll-Antwort – Poll beendet, Maske wartet weiter");
                    return PollOutcome::Stopped;
                }
                PollStatus::Malformed => {
                    tracing::warn!(status = response.status, "Long-Poll-Antwort nicht auswertbar");
                    return if sleep_or_cancel(config.error_backoff, cancel).await {
                        PollOutcome::Reload
                    } else {
                        PollOutcome::Cancelled
                    };
                }
            },
            // abgerissene Verbindung (z. B. vom Mobil-OS im Hintergrund gekappt): einfach weiter pollen
            Err(NetError::NoResponse | NetError::Timeout | NetError::ConnectionLost(_)) => {
                if tokio::time::Instant::now() >= deadline {
                    tracing::debug!("Long-Poll ohne Antwort nach Ablauf der A-Trust-Frist – Seite neu laden");
                    return PollOutcome::Reload;
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "Long-Poll fehlgeschlagen");
                return if sleep_or_cancel(config.error_backoff, cancel).await {
                    PollOutcome::Reload
                } else {
                    PollOutcome::Cancelled
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("https://service.a-trust.at/mobile/https-security-layer-request/qr.aspx?sid=1").unwrap()
    }

    #[test]
    fn script_parsing() {
        let s = "\n var x = 1;\n qrpoll(\"../poll/LongPoll.ashx?id=7\", \"TOKEN-abc\");\n";
        let info = parse_poll_script(s, &base()).unwrap();
        assert_eq!(info.url.as_str(), "https://service.a-trust.at/mobile/poll/LongPoll.ashx?id=7");
        assert_eq!(info.token, "TOKEN-abc");
        let abs = parse_poll_script("qrpoll(\"https://poll.example/x\", \"T\");", &base()).unwrap();
        assert_eq!(abs.url.as_str(), "https://poll.example/x");
        // exakte Trenner: `", "` und `");`
        assert!(parse_poll_script("qrpoll('a', 'b');", &base()).is_none());
        assert!(parse_poll_script("qrpoll(\"a\",\"b\");", &base()).is_none());
        assert!(parse_poll_script("qrpoll(\"a\", \"b\")", &base()).is_none());
        assert!(parse_poll_script("", &base()).is_none());
        assert!(parse_poll_script("qrpoll(\"http://[::1\", \"b\");", &base()).is_none());
        assert_eq!(parse_poll_script_named("poll2", "poll2(\"/p\", \"t\");", &base()).unwrap().url.path(), "/p");
        assert!(!format!("{info:?}").contains("TOKEN-abc"));
    }

    #[test]
    fn response_parsing_like_org_json() {
        use PollStatus::*;
        assert_eq!(parse_poll_response(br#"{"Fin":true,"Wait":false,"Error":false}"#), Finished);
        assert_eq!(parse_poll_response(br#"{"Fin":false,"Wait":true,"Error":false}"#), Wait);
        assert_eq!(parse_poll_response(br#"{"Fin":false,"Wait":false,"Error":true}"#), Error);
        assert_eq!(parse_poll_response(br#"{"Fin":false,"Wait":false,"Error":false}"#), Unknown);
        // Reihenfolge: Fin zuerst, fehlende Schlüssel werfen
        assert_eq!(parse_poll_response(br#"{"Fin":true}"#), Finished);
        assert_eq!(parse_poll_response(br#"{"Wait":true}"#), Malformed);
        assert_eq!(parse_poll_response(br#"{"Fin":false}"#), Malformed);
        assert_eq!(parse_poll_response(br#"{"Fin":false,"Wait":false}"#), Malformed);
        // Zeichenketten "true"/"false" wie getBoolean
        assert_eq!(parse_poll_response(br#"{"Fin":"TRUE"}"#), Finished);
        assert_eq!(parse_poll_response(br#"{"Fin":"false","Wait":"true"}"#), Wait);
        assert_eq!(parse_poll_response(br#"{"Fin":1}"#), Malformed);
        assert_eq!(parse_poll_response(b"[]"), Malformed);
        assert_eq!(parse_poll_response(b"<html>"), Malformed);
        assert_eq!(parse_poll_response(b""), Malformed);
    }
}
