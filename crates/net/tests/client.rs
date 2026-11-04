//! Integrationstests des HTTP-Clients gegen einen lokalen Mock-Server (nur 127.0.0.1, kein Netz).

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use pida_net::testing::{MockResponse, MockServer};
use pida_net::{Body, FormPart, HttpClient, NetConfig, NetError, ProxyAuth, ProxyConfig, Purpose, Request, Url};
use zeroize::Zeroizing;

fn direct() -> NetConfig {
    NetConfig { proxy: ProxyConfig::None, ..NetConfig::default() }
}

#[tokio::test]
async fn redirects_are_not_followed_in_the_session_client() {
    let server = MockServer::start(|req| match req.path() {
        "/start" => MockResponse::redirect(302, "/ziel"),
        _ => MockResponse::html("<p>ziel</p>"),
    })
    .await;
    let client = HttpClient::new(&direct(), Purpose::AtrustSession).unwrap();
    let resp = client.get(&server.url("/start"), &[]).await.unwrap();
    assert_eq!(resp.status, 302);
    assert_eq!(resp.reason, "Found");
    assert_eq!(resp.header("location"), Some("/ziel"));
    assert_eq!(resp.url, server.url("/start"));
    assert_eq!(server.requests().len(), 1);
}

#[tokio::test]
async fn poll_and_image_clients_follow_redirects() {
    let server = MockServer::start(|req| match req.path() {
        "/start" => MockResponse::redirect(302, "/ziel"),
        _ => MockResponse::json("{\"Fin\":true}"),
    })
    .await;
    for purpose in [Purpose::AtrustPoll, Purpose::AtrustImage] {
        let client = HttpClient::new(&direct(), purpose).unwrap();
        let resp = client.get(&server.url("/start"), &[]).await.unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.url.path(), "/ziel");
        assert_eq!(resp.body, b"{\"Fin\":true}");
    }
}

#[tokio::test]
async fn cookie_jar_per_client_only_for_session_purpose() {
    let server = MockServer::start(|req| match req.path() {
        "/set" => MockResponse::html("ok").with_header("Set-Cookie", "ASP.NET_SessionId=abc123; path=/; HttpOnly"),
        _ => MockResponse::html("ok"),
    })
    .await;
    let session = HttpClient::new(&direct(), Purpose::AtrustSession).unwrap();
    session.get(&server.url("/set"), &[]).await.unwrap();
    session.get(&server.url("/check"), &[]).await.unwrap();
    // neuer Client = neuer, leerer Jar (je SL-Request wie PDF-Over)
    let second = HttpClient::new(&direct(), Purpose::AtrustSession).unwrap();
    second.get(&server.url("/check2"), &[]).await.unwrap();
    // Poll-Client speichert keine Cookies
    let poll = HttpClient::new(&direct(), Purpose::AtrustPoll).unwrap();
    poll.get(&server.url("/set"), &[]).await.unwrap();
    poll.get(&server.url("/check3"), &[]).await.unwrap();

    let reqs = server.requests();
    let cookie = |path: &str| reqs.iter().find(|r| r.path() == path).unwrap().header("cookie").map(str::to_owned);
    assert_eq!(cookie("/check").as_deref(), Some("ASP.NET_SessionId=abc123"));
    assert_eq!(cookie("/check2"), None);
    assert_eq!(cookie("/check3"), None);
}

#[tokio::test]
async fn user_agent_and_custom_headers() {
    let server = MockServer::start(|_| MockResponse::html("ok")).await;
    let client = HttpClient::new(&direct(), Purpose::AtrustPoll).unwrap();
    let req = Request::get(server.url("/poll")).with_header("Authorization", "Bearer TOKEN");
    client.execute(req).await.unwrap();
    let custom = NetConfig { user_agent: Some("Test-UA/1".into()), ..direct() };
    HttpClient::new(&custom, Purpose::LocalBku).unwrap().get(&server.url("/x"), &[]).await.unwrap();
    let reqs = server.requests();
    assert_eq!(reqs[0].header("authorization"), Some("Bearer TOKEN"));
    assert_eq!(reqs[0].header("user-agent"), Some(pida_net::DEFAULT_USER_AGENT));
    assert!(reqs[0].header("user-agent").unwrap().starts_with("PIDA v"));
    assert_eq!(reqs[1].header("user-agent"), Some("Test-UA/1"));
}

#[tokio::test]
async fn urlencoded_and_multipart_bodies() {
    let server = MockServer::start(|_| MockResponse::xml("<ok/>")).await;
    let client = HttpClient::new(&direct(), Purpose::AtrustSession).unwrap();
    let xml = "<?xml version=\"1.0\"?><sl:X xmlns:sl=\"urn:x\">ä &amp; +</sl:X>";
    client.post_form(&server.url("/form"), &[("XMLRequest".into(), xml.into())]).await.unwrap();
    let parts = vec![
        FormPart::file("fileupload", "sign.pdf", "application/pdf", vec![0, 1, 2, 0xff, b'\r', b'\n']),
        FormPart::text("XMLRequest", xml),
    ];
    client.post_multipart(&server.url("/multi"), parts).await.unwrap();
    client
        .execute(Request::post(
            server.url("/raw"),
            Body::Bytes { content_type: "application/x-test".into(), data: b"r".to_vec() },
        ))
        .await
        .unwrap();

    let reqs = server.requests();
    assert_eq!(reqs[0].method, "POST");
    assert_eq!(reqs[0].header("content-type"), Some("application/x-www-form-urlencoded; charset=UTF-8"));
    assert_eq!(reqs[0].form_field("XMLRequest").as_deref(), Some(xml));

    let multi = reqs[1].multipart();
    assert!(reqs[1].header("content-type").unwrap().starts_with("multipart/form-data; boundary="));
    assert_eq!(reqs[1].header("content-length").unwrap().parse::<usize>().unwrap(), reqs[1].body.len());
    assert_eq!(multi.len(), 2);
    assert_eq!((multi[0].name.as_str(), multi[0].filename.as_deref()), ("fileupload", Some("sign.pdf")));
    assert_eq!(multi[0].content_type.as_deref(), Some("application/pdf"));
    assert_eq!(multi[0].data, vec![0, 1, 2, 0xff, b'\r', b'\n']);
    assert_eq!(multi[1].name, "XMLRequest");
    assert_eq!(multi[1].content_type.as_deref(), Some("text/plain; charset=UTF-8"));
    assert_eq!(multi[1].text(), xml);

    assert_eq!(reqs[2].header("content-type"), Some("application/x-test"));
    assert_eq!(reqs[2].body, b"r");
}

#[tokio::test]
async fn status_reason_headers_and_text() {
    let server = MockServer::start(|req| match req.path() {
        "/teapot" => MockResponse::status(599).with_reason("Eigener Grund").with_body("x"),
        _ => MockResponse::body("text/html; charset=ISO-8859-1", b"Gr\xfc\xdfe".to_vec())
            .with_header("Refresh", "0; URL=/weiter")
            .with_header("Date", "Wed, 14 Oct 2026 10:00:00 GMT"),
    })
    .await;
    let client = HttpClient::new(&direct(), Purpose::AtrustSession).unwrap();
    let r = client.get(&server.url("/teapot"), &[]).await.unwrap();
    assert_eq!((r.status, r.reason.as_str()), (599, "Eigener Grund"));
    let r = client.get(&server.url("/html"), &[]).await.unwrap();
    assert_eq!(r.text(), "Grüße");
    assert_eq!(r.mime().as_deref(), Some("text/html"));
    assert_eq!(r.header("REFRESH"), Some("0; URL=/weiter"));
    assert_eq!(pida_net::parse_refresh(r.header("refresh").unwrap()).as_deref(), Some("/weiter"));
    assert_eq!(r.date().unwrap().unix_timestamp(), 1_791_972_000);
    assert!(r.headers.iter().all(|(k, _)| k.chars().all(|c| !c.is_ascii_uppercase())));
}

#[tokio::test]
async fn request_timeout() {
    let server = MockServer::start(|_| MockResponse::html("spät").with_delay(Duration::from_secs(3))).await;
    let cfg = NetConfig { request_timeout: Some(Duration::from_millis(200)), ..direct() };
    let client = HttpClient::new(&cfg, Purpose::AtrustPoll).unwrap();
    let err = client.get(&server.url("/"), &[]).await.unwrap_err();
    assert!(matches!(err, NetError::Timeout), "{err:?}");
}

#[tokio::test]
async fn connection_closed_without_response() {
    let server = MockServer::start(|_| MockResponse::no_response()).await;
    let client = HttpClient::new(&direct(), Purpose::AtrustPoll).unwrap();
    let err = client.get(&server.url("/"), &[]).await.unwrap_err();
    assert!(matches!(err, NetError::NoResponse), "{err:?}");
}

#[tokio::test]
async fn connection_refused() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let client = HttpClient::new(&direct(), Purpose::AtrustSession).unwrap();
    let url = Url::parse(&format!("http://{addr}/x?sid=GEHEIM")).unwrap();
    let err = client.get(&url, &[]).await.unwrap_err();
    assert!(matches!(err, NetError::Connect(_)), "{err:?}");
    assert!(!err.to_string().contains("GEHEIM"), "keine URL-Abfrage in Fehlertexten: {err}");
}

#[tokio::test]
async fn body_limit() {
    let server = MockServer::start(|req| match req.path() {
        "/gross" => MockResponse::html("x".repeat(2048)),
        _ => MockResponse::html("x".repeat(1024)),
    })
    .await;
    let cfg = NetConfig { max_body: 1024, ..direct() };
    let client = HttpClient::new(&cfg, Purpose::AtrustSession).unwrap();
    assert_eq!(client.get(&server.url("/genau"), &[]).await.unwrap().body.len(), 1024);
    let err = client.get(&server.url("/gross"), &[]).await.unwrap_err();
    assert!(matches!(err, NetError::Body(_)), "{err:?}");
}

#[tokio::test]
async fn manual_proxy_with_basic_auth() {
    let proxy = MockServer::start(|_| MockResponse::html("über proxy")).await;
    let cfg = NetConfig {
        proxy: ProxyConfig::Manual {
            host: "127.0.0.1".into(),
            port: proxy.addr().port(),
            auth: Some(ProxyAuth { user: "user".into(), password: Zeroizing::new("pass".into()) }),
        },
        ..NetConfig::default()
    };
    let client = HttpClient::new(&cfg, Purpose::AtrustSession).unwrap();
    let resp = client.get(&Url::parse("http://ziel.example.invalid/pfad?q=1").unwrap(), &[]).await.unwrap();
    assert_eq!(resp.text(), "über proxy");
    let reqs = proxy.requests();
    assert_eq!(reqs[0].target, "http://ziel.example.invalid/pfad?q=1");
    assert_eq!(reqs[0].header("proxy-authorization"), Some("Basic dXNlcjpwYXNz"));
}

#[tokio::test]
async fn local_bku_never_uses_the_proxy() {
    let proxy = MockServer::start(|_| MockResponse::html("proxy")).await;
    let bku = MockServer::start(|_| MockResponse::html("bku")).await;
    let cfg = NetConfig {
        proxy: ProxyConfig::Manual { host: "127.0.0.1".into(), port: proxy.addr().port(), auth: None },
        ..NetConfig::default()
    };
    let client = HttpClient::new(&cfg, Purpose::LocalBku).unwrap();
    assert_eq!(client.get(&bku.url("/http-security-layer-request"), &[]).await.unwrap().text(), "bku");
    assert!(proxy.requests().is_empty());
}

#[tokio::test]
async fn connect_tunnel_407_is_reported() {
    let proxy = MockServer::start(|req| {
        assert_eq!(req.method, "CONNECT");
        MockResponse::status(407).with_header("Proxy-Authenticate", "Basic realm=\"x\"")
    })
    .await;
    let cfg = NetConfig {
        proxy: ProxyConfig::Manual { host: "127.0.0.1".into(), port: proxy.addr().port(), auth: None },
        ..NetConfig::default()
    };
    let client = HttpClient::new(&cfg, Purpose::AtrustSession).unwrap();
    let err = client.get(&Url::parse("https://service.example.invalid/").unwrap(), &[]).await.unwrap_err();
    assert!(err.is_proxy_auth_required(), "{err:?}");
    assert_eq!(proxy.requests()[0].target, "service.example.invalid:443");
}

#[tokio::test]
async fn tcp_probe_open_and_closed() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    assert!(pida_net::tcp_probe(addr, Duration::from_secs(1)).await);
    drop(listener);
    assert!(!pida_net::tcp_probe(addr, Duration::from_secs(1)).await);
}

#[tokio::test]
async fn many_sequential_requests_on_one_client() {
    let counter = Arc::new(AtomicUsize::new(0));
    let c = Arc::clone(&counter);
    let server = MockServer::start(move |_| {
        c.fetch_add(1, Ordering::SeqCst);
        MockResponse::html("ok")
    })
    .await;
    let client = HttpClient::new(&direct(), Purpose::AtrustSession).unwrap();
    for i in 0..20 {
        client.get(&server.url(&format!("/{i}")), &[]).await.unwrap();
    }
    assert_eq!(counter.load(Ordering::SeqCst), 20);
}

/// Update-Check (ADR 0004 Nachtrag 21): nur `https` – eine `http`-Adresse erreicht den Server nicht, und ein Redirect
/// von `https` nach `http` würde ebenso abgelehnt (reqwest `https_only` gilt auch für Redirects). Die anderen Zwecke
/// sprechen weiter `http` (lokale BKU, Testserver).
#[tokio::test]
async fn update_check_refuses_plain_http() {
    let server = MockServer::start(|_| MockResponse::json("[]")).await;
    let client = HttpClient::new(&direct(), Purpose::UpdateCheck).unwrap();
    let err = client.get(&server.url("/repos/a-sit/pida/releases"), &[]).await.unwrap_err();
    assert!(matches!(err, NetError::Config(_)), "{err:?}");
    assert!(server.requests().is_empty(), "kein Request über http");
    let poll = HttpClient::new(&direct(), Purpose::AtrustPoll).unwrap();
    assert_eq!(poll.get(&server.url("/x"), &[]).await.unwrap().status, 200);
}

/// Server, der die ersten `resets` Verbindungen nach dem Lesen des Requests hart zurücksetzt (RST,
/// wie eine vom Mobil-OS gekappte Keep-Alive-Verbindung) und danach normal antwortet.
async fn resetting_server(resets: usize) -> (SocketAddr, Arc<AtomicUsize>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let connections = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&connections);
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else { return };
            let n = counter.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let mut read = 0;
                while !buf[..read].windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut buf[read..]).await {
                        Ok(0) | Err(_) => return,
                        Ok(k) => read += k,
                    }
                }
                if n < resets {
                    #[allow(deprecated)]
                    let _ = stream.set_linger(Some(Duration::ZERO));
                    return;
                }
                let _ = stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                    .await;
            });
        }
    });
    (addr, connections)
}

#[tokio::test]
async fn get_is_retried_once_after_connection_reset() {
    let (addr, connections) = resetting_server(1).await;
    let client = HttpClient::new(&direct(), Purpose::AtrustSession).unwrap();
    let url = Url::parse(&format!("http://{addr}/seite")).unwrap();
    let r = client.get(&url, &[]).await.expect("zweiter Versuch auf neuer Verbindung");
    assert_eq!((r.status, r.body.as_slice()), (200, b"ok".as_slice()));
    assert_eq!(connections.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn get_retry_only_once() {
    let (addr, connections) = resetting_server(2).await;
    let client = HttpClient::new(&direct(), Purpose::AtrustSession).unwrap();
    let url = Url::parse(&format!("http://{addr}/seite")).unwrap();
    let err = client.get(&url, &[]).await.unwrap_err();
    assert!(matches!(err, NetError::ConnectionLost(_)), "{err:?}");
    assert_eq!(connections.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn post_is_not_retried_after_connection_reset() {
    let (addr, connections) = resetting_server(1).await;
    let client = HttpClient::new(&direct(), Purpose::AtrustSession).unwrap();
    let url = Url::parse(&format!("http://{addr}/senden")).unwrap();
    let err = client.post_form(&url, &[("a".into(), "b".into())]).await.unwrap_err();
    assert!(matches!(err, NetError::ConnectionLost(_)), "{err:?}");
    assert_eq!(connections.load(Ordering::SeqCst), 1);
}
