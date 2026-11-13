//! SL-Rückkanal (DataURL) für das A-Trust-Overlay der Web-Variante.
//!
//! Ablauf je Sitzung (Kennung in der DataURL, Token nur für die Seite):
//! 1. Die Seite legt eine Sitzung an ([`Relay::create`]) und schickt den ersten Befehl per Formular an A-Trust,
//!    mit `DataURL = …/api/sl/data/<id>`.
//! 2. A-Trust schickt die SL-Antwort an die DataURL ([`Relay::deliver`]). Der Rückkanal legt sie für die Seite ab
//!    und **hält die Anfrage von A-Trust offen**, bis die Seite antwortet.
//! 3. Die Seite holt die Antwort ([`Relay::take_response`], Long-Poll) und schickt entweder den nächsten Befehl
//!    ([`Relay::command`] → Antwort an A-Trust `text/xml`, A-Trust führt ihn in derselben Sitzung aus) oder beendet
//!    die Kette ([`Relay::finish`], erfolgreich oder abgebrochen → Abschlussseite an A-Trust, Sitzung gelöscht).
//!
//! **Datenschutz:** SL-Antworten (Zertifikat, Signatur) und Befehle (mit dem Dokument) liegen nur im Speicher, jeweils
//! bis zur Weitergabe; protokolliert werden Ereignisse ohne Inhalt und ohne Kennung. Sitzungen verfallen nach
//! [`SESSION_TTL`]; die Zahl offener Sitzungen ist begrenzt ([`MAX_SESSIONS`]).

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rand_core::{OsRng, RngCore};
use tokio::sync::{Notify, oneshot};

/// Höchstzahl gleichzeitig offener Sitzungen.
pub const MAX_SESSIONS: usize = 2000;
/// Höchstzahl offener Sitzungen je Client (IP-Adresse): Ein einzelner Client kann den Rückkanal nicht für alle
/// füllen. Großzügig, weil hinter einer Adresse viele Menschen sitzen können (Behörde, Firma hinter NAT).
pub const MAX_SESSIONS_PER_CLIENT: usize = 200;
/// Höchstzahl neuer Sitzungen je Client in [`RATE_WINDOW`] (eine Signatur = eine Sitzung).
pub const MAX_NEW_PER_WINDOW: u32 = 60;
/// Zeitfenster für [`MAX_NEW_PER_WINDOW`].
pub const RATE_WINDOW: Duration = Duration::from_secs(60);
/// Lebensdauer einer Sitzung (Anmeldung, Bestätigung, Stapel).
pub const SESSION_TTL: Duration = Duration::from_secs(30 * 60);
/// So lange wartet die Anfrage von A-Trust auf den nächsten Befehl der Seite.
pub const HOLD_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Long-Poll der Seite.
pub const POLL_WAIT: Duration = Duration::from_secs(25);

/// Antwort an A-Trust auf die DataURL-Anfrage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// nächster SL-Befehl (`text/xml`)
    Command(String),
    /// Kette fertig (Abschlussseite)
    Finish,
    /// abgebrochen, abgelaufen bzw. Sitzung unbekannt (Abschlussseite „abgebrochen“)
    Abort,
}

/// Fehler der Seiten-Endpunkte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayError {
    /// Sitzung unbekannt, abgelaufen oder Token falsch
    NotFound,
    /// zu viele offene Sitzungen
    Busy,
    /// dieser Client hat zu viele offene Sitzungen bzw. legt zu schnell neue an
    TooMany,
    /// A-Trust wartet gerade auf keinen Befehl
    NotWaiting,
    /// kein zulässiger SL-Befehl
    Invalid,
}

/// SL-1.2-Namespace.
const SL_NAMESPACE: &str = "http://www.buergerkarte.at/namespaces/securitylayer/1.2#";
/// Befehle, die die Seite an A-Trust weitergeben darf.
const ALLOWED_COMMANDS: [&str; 2] = ["InfoboxReadRequest", "CreateCMSSignatureRequest"];

/// Ist `xml` ein zulässiger SL-Befehl (Wurzel `InfoboxReadRequest`/`CreateCMSSignatureRequest` im SL-Namespace)?
/// Der Rückkanal gibt sonst nichts als Antwort weiter – auch kein XHTML, das ein Browser darstellen würde.
pub fn is_sl_command(xml: &str) -> bool {
    let mut rest = xml.trim_start_matches('\u{feff}').trim_start();
    // XML-Deklaration und Kommentare vor der Wurzel überspringen
    loop {
        if let Some(r) = rest.strip_prefix("<?") {
            let Some(end) = r.find("?>") else { return false };
            rest = r[end + 2..].trim_start();
        } else if let Some(r) = rest.strip_prefix("<!--") {
            let Some(end) = r.find("-->") else { return false };
            rest = r[end + 3..].trim_start();
        } else {
            break;
        }
    }
    let Some(root) = rest.strip_prefix('<') else { return false };
    let Some(tag_end) = root.find('>') else { return false };
    let tag = &root[..tag_end];
    let name = tag.split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("");
    let (prefix, local) = match name.split_once(':') {
        Some((p, l)) => (Some(p), l),
        None => (None, name),
    };
    let ns_attr = match prefix {
        Some(p) => format!("xmlns:{p}=\"{SL_NAMESPACE}\""),
        None => format!("xmlns=\"{SL_NAMESPACE}\""),
    };
    ALLOWED_COMMANDS.contains(&local) && tag.contains(&ns_attr) && !xml.contains("http://www.w3.org/1999/xhtml")
}

#[derive(Default)]
struct State {
    /// SL-Antwort von A-Trust, von der Seite noch nicht abgeholt
    response: Option<Vec<u8>>,
    /// gehaltene DataURL-Anfrage von A-Trust
    held: Option<oneshot::Sender<Reply>>,
}

struct Session {
    token: String,
    client: IpAddr,
    created: Instant,
    state: Mutex<State>,
    arrived: Notify,
}

/// Rückkanal (alle Sitzungen im Speicher).
#[derive(Clone, Default)]
pub struct Relay {
    sessions: Arc<Mutex<HashMap<String, Arc<Session>>>>,
    /// neue Sitzungen je Client im laufenden Zeitfenster: `(Beginn, Anzahl)`
    recent: Arc<Mutex<HashMap<IpAddr, (Instant, u32)>>>,
}

fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    OsRng.fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

/// Vergleich ohne frühes Ende (Token).
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Relay {
    /// Neue Sitzung für `client`: `(id, token)`.
    pub fn create(&self, client: IpAddr) -> Result<(String, String), RelayError> {
        let mut sessions = lock(&self.sessions);
        sessions.retain(|_, s| s.created.elapsed() < SESSION_TTL);
        if sessions.len() >= MAX_SESSIONS {
            return Err(RelayError::Busy);
        }
        if sessions.values().filter(|s| s.client == client).count() >= MAX_SESSIONS_PER_CLIENT {
            tracing::warn!("zu viele offene SL-Sitzungen eines Clients");
            return Err(RelayError::TooMany);
        }
        {
            let mut recent = lock(&self.recent);
            let now = Instant::now();
            let entry = recent.entry(client).or_insert((now, 0));
            if now.duration_since(entry.0) >= RATE_WINDOW {
                *entry = (now, 0);
            }
            if entry.1 >= MAX_NEW_PER_WINDOW {
                tracing::warn!("ein Client legt zu schnell SL-Sitzungen an");
                return Err(RelayError::TooMany);
            }
            entry.1 += 1;
        }
        let id = random_hex(16);
        let token = random_hex(32);
        sessions.insert(
            id.clone(),
            Arc::new(Session {
                token: token.clone(),
                client,
                created: Instant::now(),
                state: Mutex::default(),
                arrived: Notify::new(),
            }),
        );
        tracing::info!("SL-Sitzung angelegt");
        Ok((id, token))
    }

    fn session(&self, id: &str) -> Option<Arc<Session>> {
        let sessions = lock(&self.sessions);
        sessions.get(id).filter(|s| s.created.elapsed() < SESSION_TTL).cloned()
    }

    fn authorized(&self, id: &str, token: &str) -> Result<Arc<Session>, RelayError> {
        self.session(id).filter(|s| same(&s.token, token)).ok_or(RelayError::NotFound)
    }

    /// Gibt es die Sitzung (noch)? Für die DataURL, bevor der Server den Rumpf der Anfrage liest.
    pub fn exists(&self, id: &str) -> bool {
        self.session(id).is_some()
    }

    /// Passen Kennung und Token? Für die Seiten-Endpunkte, bevor der Server den Rumpf der Anfrage liest.
    pub fn is_authorized(&self, id: &str, token: &str) -> bool {
        self.authorized(id, token).is_ok()
    }

    /// DataURL: SL-Antwort von A-Trust ablegen und auf den nächsten Befehl der Seite warten.
    pub async fn deliver(&self, id: &str, response: Vec<u8>) -> Reply {
        let Some(session) = self.session(id) else {
            tracing::info!("DataURL für unbekannte Sitzung");
            return Reply::Abort;
        };
        let (tx, rx) = oneshot::channel();
        {
            let mut state = lock(&session.state);
            // eine neue Antwort ersetzt eine gehaltene Anfrage (z. B. neuer Anlauf im Popup)
            if let Some(old) = state.held.replace(tx) {
                let _ = old.send(Reply::Abort);
            }
            state.response = Some(response);
        }
        session.arrived.notify_waiters();
        tracing::info!("SL-Antwort von A-Trust angekommen");
        match tokio::time::timeout(HOLD_TIMEOUT, rx).await {
            Ok(Ok(reply)) => reply,
            _ => {
                tracing::info!("kein Befehl der Seite – Kette beendet");
                self.remove(id);
                Reply::Abort
            }
        }
    }

    /// Seite: SL-Antwort abholen (Long-Poll); `None` = noch keine.
    pub async fn take_response(&self, id: &str, token: &str) -> Result<Option<Vec<u8>>, RelayError> {
        let session = self.authorized(id, token)?;
        let deadline = tokio::time::Instant::now() + POLL_WAIT;
        loop {
            let notified = session.arrived.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if let Some(r) = lock(&session.state).response.take() {
                return Ok(Some(r));
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return Ok(None);
            }
        }
    }

    /// Seite: nächster Befehl an A-Trust.
    pub fn command(&self, id: &str, token: &str, xml: String) -> Result<(), RelayError> {
        let session = self.authorized(id, token)?;
        if !is_sl_command(&xml) {
            return Err(RelayError::Invalid);
        }
        let held = lock(&session.state).held.take().ok_or(RelayError::NotWaiting)?;
        held.send(Reply::Command(xml)).map_err(|_| RelayError::NotWaiting)
    }

    /// Seite: Kette fertig bzw. abgebrochen; die Sitzung wird gelöscht.
    pub fn finish(&self, id: &str, token: &str, done: bool) -> Result<(), RelayError> {
        let session = self.authorized(id, token)?;
        if let Some(held) = lock(&session.state).held.take() {
            let _ = held.send(if done { Reply::Finish } else { Reply::Abort });
        }
        self.remove(id);
        tracing::info!(done, "SL-Sitzung beendet");
        Ok(())
    }

    fn remove(&self, id: &str) {
        lock(&self.sessions).remove(id);
    }

    /// Abgelaufene Sitzungen entfernen (gehaltene Anfragen bekommen „abgebrochen“).
    pub fn sweep(&self) {
        let mut sessions = lock(&self.sessions);
        sessions.retain(|_, s| {
            let keep = s.created.elapsed() < SESSION_TTL;
            if !keep && let Some(held) = lock(&s.state).held.take() {
                let _ = held.send(Reply::Abort);
            }
            keep
        });
        drop(sessions);
        lock(&self.recent).retain(|_, (start, _)| start.elapsed() < RATE_WINDOW);
    }

    /// Offene Sitzungen (Tests).
    #[cfg(test)]
    pub fn len(&self) -> usize {
        lock(&self.sessions).len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLIENT: IpAddr = IpAddr::V4(std::net::Ipv4Addr::new(192, 0, 2, 1));
    const OTHER: IpAddr = IpAddr::V4(std::net::Ipv4Addr::new(192, 0, 2, 2));

    #[test]
    fn one_client_cannot_create_sessions_without_limit() {
        // Tempo: nach MAX_NEW_PER_WINDOW ist für diesen Client Schluss, für andere nicht
        let relay = Relay::default();
        for _ in 0..MAX_NEW_PER_WINDOW {
            let (id, token) = relay.create(CLIENT).unwrap();
            relay.finish(&id, &token, false).unwrap();
        }
        assert_eq!(relay.create(CLIENT), Err(RelayError::TooMany));
        assert!(relay.create(OTHER).is_ok());
        // ein neues Zeitfenster gibt den Client wieder frei
        lock(&relay.recent).get_mut(&CLIENT).unwrap().0 -= RATE_WINDOW;
        assert!(relay.create(CLIENT).is_ok());

        // offene Sitzungen: die Grenze je Client greift auch ohne Tempo
        let relay = Relay::default();
        for _ in 0..MAX_SESSIONS_PER_CLIENT {
            relay.create(CLIENT).unwrap();
            lock(&relay.recent).clear();
        }
        assert_eq!(relay.create(CLIENT), Err(RelayError::TooMany));
        assert_eq!(relay.len(), MAX_SESSIONS_PER_CLIENT);
        assert!(relay.create(OTHER).is_ok());
        relay.sweep();
        assert!(lock(&relay.recent).contains_key(&OTHER), "laufendes Zeitfenster bleibt");
    }

    #[tokio::test]
    async fn chain_with_two_commands_then_finish() {
        let relay = Relay::default();
        let (id, token) = relay.create(CLIENT).unwrap();
        assert_eq!(id.len(), 32);
        assert_eq!(token.len(), 64);
        assert_eq!(relay.take_response(&id, "falsch").await, Err(RelayError::NotFound));
        assert!(relay.exists(&id) && !relay.exists("unbekannt"));
        assert!(relay.is_authorized(&id, &token) && !relay.is_authorized(&id, "falsch"));

        // A-Trust liefert die erste Antwort und wartet
        let r1 = relay.clone();
        let id1 = id.clone();
        let atrust = tokio::spawn(async move { r1.deliver(&id1, b"<InfoboxReadResponse/>".to_vec()).await });
        assert_eq!(relay.take_response(&id, &token).await.unwrap().unwrap(), b"<InfoboxReadResponse/>");
        relay.command(&id, &token, cms()).unwrap();
        assert_eq!(atrust.await.unwrap(), Reply::Command(cms()));

        // zweite Antwort, dann Ende
        let r2 = relay.clone();
        let id2 = id.clone();
        let atrust = tokio::spawn(async move { r2.deliver(&id2, b"<CreateCMSSignatureResponse/>".to_vec()).await });
        assert!(relay.take_response(&id, &token).await.unwrap().is_some());
        relay.finish(&id, &token, true).unwrap();
        assert_eq!(atrust.await.unwrap(), Reply::Finish);
        assert_eq!(relay.len(), 0);
        assert_eq!(relay.command(&id, &token, cms()), Err(RelayError::NotFound));
    }

    fn cms() -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<CreateCMSSignatureRequest Structure="detached" xmlns="{SL_NAMESPACE}" xmlns:ns2="http://www.w3.org/2000/09/xmldsig#"><KeyboxIdentifier>x</KeyboxIdentifier></CreateCMSSignatureRequest>"#
        )
    }

    #[test]
    fn only_sl_commands_are_forwarded() {
        assert!(is_sl_command(&cms()));
        assert!(is_sl_command(&format!(
            r#"<sl:InfoboxReadRequest xmlns:sl="{SL_NAMESPACE}"><sl:InfoboxIdentifier/></sl:InfoboxReadRequest>"#
        )));
        assert!(!is_sl_command("<CreateCMSSignatureRequest/>"), "ohne SL-Namespace");
        assert!(!is_sl_command(&format!(r#"<NullOperationRequest xmlns="{SL_NAMESPACE}"/>"#)), "nicht zugelassen");
        assert!(!is_sl_command(r#"<html xmlns="http://www.w3.org/1999/xhtml"><script>alert(1)</script></html>"#));
        assert!(!is_sl_command(&format!(
            r#"<CreateCMSSignatureRequest xmlns="{SL_NAMESPACE}"><x:script xmlns:x="http://www.w3.org/1999/xhtml">alert(1)</x:script></CreateCMSSignatureRequest>"#
        )));
        assert!(!is_sl_command("kein XML"));
    }

    #[tokio::test]
    async fn command_without_waiting_atrust_and_unknown_dataurl() {
        let relay = Relay::default();
        let (id, token) = relay.create(CLIENT).unwrap();
        assert_eq!(relay.command(&id, &token, "x".into()), Err(RelayError::Invalid));
        assert_eq!(relay.command(&id, &token, cms()), Err(RelayError::NotWaiting));
        assert_eq!(relay.deliver("unbekannt", b"x".to_vec()).await, Reply::Abort);
    }

    #[tokio::test]
    async fn abort_answers_atrust() {
        let relay = Relay::default();
        let (id, token) = relay.create(CLIENT).unwrap();
        let r = relay.clone();
        let i = id.clone();
        let atrust = tokio::spawn(async move { r.deliver(&i, b"x".to_vec()).await });
        assert!(relay.take_response(&id, &token).await.unwrap().is_some());
        relay.finish(&id, &token, false).unwrap();
        assert_eq!(atrust.await.unwrap(), Reply::Abort);
    }

    #[tokio::test(start_paused = true)]
    async fn poll_returns_none_without_answer() {
        let relay = Relay::default();
        let (id, token) = relay.create(CLIENT).unwrap();
        assert_eq!(relay.take_response(&id, &token).await.unwrap(), None);
    }

    #[test]
    fn token_compare() {
        assert!(same("abc", "abc"));
        assert!(!same("abc", "abd"));
        assert!(!same("abc", "ab"));
    }
}
