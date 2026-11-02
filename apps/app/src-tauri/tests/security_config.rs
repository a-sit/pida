//! Regressionstests der Sicherheitskonfiguration (Plan §7.7): minimale Capabilities, strenge CSP,
//! keine Remote-Inhalte, eingefrorener Prototyp, kein globales `__TAURI__`.

use std::collections::BTreeSet;

const CAPABILITIES: &str = include_str!("../capabilities/default.json");
const MOBILE_CAPABILITIES: &str = include_str!("../capabilities/mobile.json");
const TAURI_CONF: &str = include_str!("../tauri.conf.json");

fn json(text: &str) -> serde_json::Value {
    serde_json::from_str(text).expect("gültiges JSON")
}

fn permission_ids(caps: &serde_json::Value) -> BTreeSet<String> {
    caps["permissions"]
        .as_array()
        .expect("permissions")
        .iter()
        .map(|p| match p {
            serde_json::Value::String(s) => s.clone(),
            other => other["identifier"].as_str().expect("identifier").to_owned(),
        })
        .collect()
}

#[test]
fn capabilities_are_minimal() {
    let caps = json(CAPABILITIES);
    assert_eq!(caps["windows"], serde_json::json!(["main"]), "nur das Hauptfenster");
    assert_eq!(caps["platforms"], serde_json::json!(["linux", "macOS", "windows"]), "nur Desktop");
    assert!(caps.get("remote").is_none(), "keine Remote-URLs mit IPC-Zugriff");
    let ids = permission_ids(&caps);
    let expected: BTreeSet<String> = [
        "core:event:allow-listen",
        "core:event:allow-unlisten",
        "core:webview:allow-internal-toggle-devtools",
        "allow-core-view",
        "allow-core-dispatch",
        "allow-app-info",
        "allow-open-log-dir",
        "allow-reveal-output",
        "allow-license-text",
        // „Demo starten“: ohne Parameter, schreibt nur das eingebettete Demodokument in den eigenen Cache-Ordner
        "allow-demo-document",
        "dialog:allow-open",
        "dialog:allow-save",
        "opener:allow-open-url",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert_eq!(ids, expected, "Berechtigungen nur erweitern, wenn die Oberfläche sie braucht");
    for id in &ids {
        assert!(!id.ends_with(":default"), "keine Sammelberechtigung {id}");
    }
    // Links nur https/mailto (kein file:, kein http:, keine Programme)
    let opener = caps["permissions"]
        .as_array()
        .and_then(|list| list.iter().find(|p| p["identifier"] == "opener:allow-open-url"))
        .expect("opener-Eintrag");
    assert_eq!(opener["allow"], serde_json::json!([{ "url": "https://*" }, { "url": "mailto:*" }]));
}

/// Mobile (iOS/Android): noch weniger als der Desktop – kein Dateidialog, kein Log-Ordner, kein
/// Dateimanager, keine DevTools; Dateiauswahl nur über den eigenen Befehl (Kopie in die Sandbox). Das
/// Plugin `pida-mobile` hat keine Befehle für die Oberfläche (sicherer Speicher nie aus der WebView).
#[test]
fn mobile_capabilities_are_minimal() {
    let caps = json(MOBILE_CAPABILITIES);
    assert_eq!(caps["windows"], serde_json::json!(["main"]));
    assert_eq!(caps["platforms"], serde_json::json!(["iOS", "android"]));
    assert!(caps.get("remote").is_none());
    let ids = permission_ids(&caps);
    let expected: BTreeSet<String> = [
        "core:event:allow-listen",
        "core:event:allow-unlisten",
        "allow-core-view",
        "allow-core-dispatch",
        "allow-app-info",
        "allow-license-text",
        "allow-demo-document",
        "allow-mobile-pick-files",
        "opener:allow-open-url",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert_eq!(ids, expected);
    assert!(ids.iter().all(|id| !id.starts_with("pida-mobile:")), "Plugin ohne WebView-Befehle");
    let opener = caps["permissions"]
        .as_array()
        .and_then(|list| list.iter().find(|p| p["identifier"] == "opener:allow-open-url"))
        .expect("opener-Eintrag");
    assert_eq!(opener["allow"], serde_json::json!([{ "url": "https://*" }, { "url": "mailto:*" }]));
}

#[test]
fn csp_has_no_remote_sources() {
    let conf = json(TAURI_CONF);
    let security = &conf["app"]["security"];
    assert_eq!(security["freezePrototype"], true);
    assert_eq!(conf["app"]["withGlobalTauri"], false);
    let csp = security["csp"].as_str().expect("CSP gesetzt");
    let directives: Vec<(&str, Vec<&str>)> = csp
        .split(';')
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(|d| {
            let mut parts = d.split_whitespace();
            (parts.next().unwrap_or_default(), parts.collect())
        })
        .collect();
    let get = |name: &str| directives.iter().find(|(n, _)| *n == name).map(|(_, v)| v.clone());
    assert_eq!(get("default-src"), Some(vec!["'self'"]));
    assert_eq!(get("script-src"), Some(vec!["'self'"]), "keine Inline-/Remote-Skripte");
    assert_eq!(get("object-src"), Some(vec!["'none'"]));
    assert_eq!(get("frame-src"), Some(vec!["'none'"]));
    assert_eq!(get("base-uri"), Some(vec!["'none'"]));
    assert_eq!(get("form-action"), Some(vec!["'none'"]));
    for (name, sources) in &directives {
        for source in sources {
            let remote = source.starts_with("http://") || source.starts_with("https://");
            if remote {
                assert!(
                    source.ends_with(".localhost"),
                    "{name}: nur lokale Tauri-Ursprünge (*.localhost), gefunden {source}"
                );
            }
            assert_ne!(*source, "*", "{name}: keine Wildcard");
            assert_ne!(*source, "'unsafe-eval'", "{name}: kein eval");
            if *name != "style-src" {
                assert_ne!(*source, "'unsafe-inline'", "{name}: kein unsafe-inline");
            }
        }
    }
}

const ANDROID_MANIFEST: &str = include_str!("../gen/android/app/src/main/AndroidManifest.xml");
const ANDROID_CONF: &str = include_str!("../tauri.android.conf.json");
const ANDROID_SHARE_PATHS: &str =
    include_str!("../../plugins/pida-mobile/android/src/main/res/xml/pida_share_paths.xml");
const IOS_INFO_PLIST: &str = include_str!("../gen/apple/pida-app_iOS/Info.plist");

/// Teil des XML ohne Kommentare (die Dateien erklären ihre Entscheidungen in Kommentaren).
fn without_xml_comments(xml: &str) -> String {
    let mut out = String::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        match rest[start..].find("-->") {
            Some(end) => rest = &rest[start + end + 3..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// Android (Plan §7.7, Review der Mobile-Welle R-3/R-6): Dokumente nur über `content:`-Intents ohne
/// `BROWSABLE` (Webseiten), kein Klartext, keine Sicherung, keine Speicher-Berechtigungen; der
/// FileProvider des Plugins gibt nur den internen Ausgabeordner frei.
#[test]
fn android_surface_is_minimal() {
    let manifest = without_xml_comments(ANDROID_MANIFEST);
    assert!(!manifest.contains("BROWSABLE"), "keine Intent-Filter für Webseiten");
    assert!(!manifest.contains("pathPattern"), "kein von Tauri erzeugter Datei-Filter");
    assert!(manifest.contains(r#"android:scheme="content""#), "VIEW nur für content://");
    assert!(!manifest.contains(r#"android:scheme="file""#));
    assert!(manifest.contains(r#"android:allowBackup="false""#));
    assert!(manifest.contains(r#"android:usesCleartextTraffic="${usesCleartextTraffic}""#));
    for permission in ["READ_EXTERNAL_STORAGE", "WRITE_EXTERNAL_STORAGE", "MANAGE_EXTERNAL_STORAGE", "READ_MEDIA"] {
        assert!(!manifest.contains(permission), "keine Speicher-Berechtigung {permission}");
    }
    assert_eq!(manifest.matches(r#"android:exported="true""#).count(), 1, "nur die Haupt-Activity exportiert");
    let conf = json(ANDROID_CONF);
    assert_eq!(conf["bundle"]["fileAssociations"], serde_json::Value::Null, "Datei-Zuordnung unter Android aus");
    let paths = without_xml_comments(ANDROID_SHARE_PATHS);
    assert!(paths.contains(r#"<files-path name="documents" path="Documents/" />"#), "{paths}");
    assert!(!paths.contains("external"), "nicht der externe App-Speicher (Android 9/10 für andere Apps lesbar)");
    assert!(!paths.contains("root-path") && !paths.contains(r#"path=".""#) && !paths.contains(r#"path="/""#));
}

/// iOS: keine ATS-Ausnahmen (nur HTTPS), keine Zweckangaben für ungenutzte Dienste.
#[test]
fn ios_info_plist_has_no_transport_exceptions() {
    assert!(!IOS_INFO_PLIST.contains("NSAppTransportSecurity"));
    assert!(!IOS_INFO_PLIST.contains("NSAllowsArbitraryLoads"));
    for key in ["NSPhotoLibraryUsageDescription", "NSCameraUsageDescription", "NSFaceIDUsageDescription"] {
        assert!(!IOS_INFO_PLIST.contains(key), "{key}");
    }
    assert!(IOS_INFO_PLIST.contains("<key>LSHandlerRank</key>\n\t\t\t<string>Alternate</string>"));
}
