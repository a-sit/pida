//! Einstellungen „sofort wirksam“ (ADR 0004 §3.11, Paket K3): Übernahme einzelner Felder aus
//! [`UiEvent::ApplySettings`](crate::UiEvent::ApplySettings), Richtliniensperren, Mobile-Einschränkungen und
//! die vom Kern berechneten Beispiele.
//!
//! Regeln der Übernahme (rein, ohne E/A außer den Dateiprüfungen von [`validate`]):
//!
//! - Schlüssel = TOML-Punktpfad `abschnitt.feld` eines **bekannten** Felds der [`Settings`] (`signature.profile`,
//!   `proxy.port` …); Wert = serde-JSON des Felds wie in `Settings`. Alles andere (eine Ebene, drei Ebenen,
//!   `schema_version`, unbekannte Abschnitte bzw. Felder, [`Settings::unknown`]) → Problem
//!   [`MSG_FIELD_UNKNOWN`]; `Settings::unknown` wird nie berührt.
//! - Die Änderungen werden auf die **aktuellen** Einstellungen des Kerns gesetzt (nie ein ganzes `Settings`
//!   aus der UI) – Kernänderungen seit dem letzten Projizieren (`last_source_directory`, gemerkter
//!   Benutzername, `SelectSigner { make_default }`, `SetUiLanguage`) gehen nicht verloren.
//! - Je Feld: von einer Richtlinie festgelegt → Grund als Problem ([`locked_paths`]); auf Mobile nicht
//!   verfügbar (Proxy, Ausgabeordner, Updates, Keystore; als Standard-Signiermittel nur ID Austria bzw. „Jedes
//!   Mal fragen“, ADR 0004 Nachtrag 11) → [`MSG_UNAVAILABLE_MOBILE`]; Wert nicht lesbar → [`MSG_INVALID_VALUE`] (Proxy-Port:
//!   [`MSG_INVALID_PORT`]); Prüfung ([`validate`]) meldet ein Problem **für dieses Feld** → abgelehnt. Ein
//!   abgelehntes Feld behält seinen bisherigen Wert; alle anderen werden übernommen.
//! - Hinweise für **andere** Felder (z. B. manueller Proxy ohne Host nach `proxy.mode = manual`) lehnen
//!   nichts ab; sie stehen nach der Übernahme in `problems` ([`validate`] der neuen Einstellungen).

use pida_fs::PathExt as _;
use std::collections::BTreeMap;
use std::path::Path;

use pida_config::{Policy, ProxyMode, Settings};

/// Meldungs-ID: unbekannte Einstellung (Pfad nicht im Schema).
pub(crate) const MSG_FIELD_UNKNOWN: &str = "ng-settings-field-unknown";
/// Meldungs-ID: Wert hat nicht den erwarteten Typ bzw. Wertebereich.
pub(crate) const MSG_INVALID_VALUE: &str = "ng-settings-invalid-value";
/// Meldungs-ID: Proxy-Port außerhalb von 1 bis 65535.
pub(crate) const MSG_INVALID_PORT: &str = "ng-settings-invalid-port";
/// Meldungs-ID: von der Organisation (Richtlinie) festgelegt.
pub(crate) const MSG_POLICY: &str = "ng-settings-policy";
/// Meldungs-ID: „Zuletzt signiert“ per Richtlinie festgelegt.
pub(crate) const MSG_RECENT_POLICY: &str = "ng-recent-policy";
/// Meldungs-ID: auf Mobilgeräten nicht verfügbar (Proxy, Ausgabeordner, Updates, Keystore, Standard-Signiermittel
/// außer ID Austria).
pub(crate) const MSG_UNAVAILABLE_MOBILE: &str = "ng-settings-unavailable-mobile";

/// Bekannte Abschnitte der Einstellungen (alles andere ist [`Settings::unknown`]).
const SECTIONS: &[&str] = &[
    "general",
    "signature",
    "placeholder",
    "backend",
    "id_austria",
    "keystore",
    "output",
    "proxy",
    "certificate_cache",
    "accessibility",
];

/// Auf Mobilgeräten nicht einstellbar (ADR 0004 E3 Punkt 7: ohne Proxy/Ordner; Nachtrag 11: ohne
/// Keystore – signiert wird nur mit ID Austria). Ein Eintrag ohne Punkt sperrt den ganzen Abschnitt. Die
/// Update-Prüfung ist seit Nachtrag 31 auch dort einstellbar.
const MOBILE_UNAVAILABLE: &[&str] = &["proxy", "output.folder", "keystore"];

/// Beispieldatei für [`examples`].
pub(crate) const EXAMPLE_FILE: &str = "Vertrag.pdf";

/// Ergebnis von [`apply_changes`].
#[derive(Debug)]
pub(crate) struct Applied {
    /// neue Einstellungen (abgelehnte Felder mit bisherigem Wert)
    pub settings: Settings,
    /// abgelehnte Felder → i18n-ID
    pub rejected: BTreeMap<String, String>,
}

/// Grund, falls die Richtlinie den Pfad (bzw. seinen Abschnitt) festlegt.
pub(crate) fn locked_reason<'a>(locked: &'a BTreeMap<String, String>, path: &str) -> Option<&'a str> {
    let section = path.split('.').next().unwrap_or(path);
    locked.get(path).or_else(|| locked.get(section)).map(String::as_str)
}

/// Von der Richtlinie festgelegte Pfade (bzw. Abschnitte) → i18n-ID des Grunds (`SettingsView::locked`).
pub(crate) fn locked_paths(policy: Option<&Policy>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Some(p) = policy else { return out };
    match p.recent_documents {
        Some(false) => {
            out.insert("general.recent_documents".into(), MSG_RECENT_POLICY.into());
        }
        Some(true) => {
            out.insert("general.recent_documents".into(), MSG_POLICY.into());
        }
        None => {}
    }
    if p.update_check.is_some() {
        out.insert("general.update_check".into(), MSG_POLICY.into());
    }
    if p.proxy.is_some() {
        out.insert("proxy".into(), MSG_POLICY.into());
    }
    if p.profile.is_some() {
        out.insert("signature.profile".into(), MSG_POLICY.into());
    }
    if p.forced_backend.is_some() {
        out.insert("backend.default".into(), MSG_POLICY.into());
    }
    out
}

/// Auf Mobilgeräten nicht verfügbar?
fn mobile_unavailable(path: &str) -> bool {
    let section = path.split('.').next().unwrap_or(path);
    MOBILE_UNAVAILABLE.iter().any(|p| *p == path || *p == section)
}

/// Abschnitt als JSON-Objekt.
fn section_json(s: &Settings, section: &str) -> Option<serde_json::Value> {
    let v = match section {
        "general" => serde_json::to_value(&s.general),
        "signature" => serde_json::to_value(&s.signature),
        "placeholder" => serde_json::to_value(s.placeholder),
        "backend" => serde_json::to_value(&s.backend),
        "id_austria" => serde_json::to_value(&s.id_austria),
        "keystore" => serde_json::to_value(&s.keystore),
        "output" => serde_json::to_value(&s.output),
        "proxy" => serde_json::to_value(&s.proxy),
        "certificate_cache" => serde_json::to_value(s.certificate_cache),
        "accessibility" => serde_json::to_value(s.accessibility),
        _ => return None,
    };
    v.ok()
}

/// Abschnitt aus JSON setzen.
fn set_section(s: &mut Settings, section: &str, v: serde_json::Value) -> Result<(), serde_json::Error> {
    match section {
        "general" => s.general = serde_json::from_value(v)?,
        "signature" => s.signature = serde_json::from_value(v)?,
        "placeholder" => s.placeholder = serde_json::from_value(v)?,
        "backend" => s.backend = serde_json::from_value(v)?,
        "id_austria" => s.id_austria = serde_json::from_value(v)?,
        "keystore" => s.keystore = serde_json::from_value(v)?,
        "output" => s.output = serde_json::from_value(v)?,
        "proxy" => s.proxy = serde_json::from_value(v)?,
        "certificate_cache" => s.certificate_cache = serde_json::from_value(v)?,
        "accessibility" => s.accessibility = serde_json::from_value(v)?,
        _ => {}
    }
    Ok(())
}

/// Einen Pfad setzen; `Err` = i18n-ID des Problems.
fn set_path(current: &Settings, path: &str, value: &serde_json::Value) -> Result<Settings, &'static str> {
    let Some((section, field)) = path.split_once('.') else { return Err(MSG_FIELD_UNKNOWN) };
    if field.contains('.') || !SECTIONS.contains(&section) {
        return Err(MSG_FIELD_UNKNOWN);
    }
    let mut json = section_json(current, section).ok_or(MSG_FIELD_UNKNOWN)?;
    let obj = json.as_object_mut().ok_or(MSG_FIELD_UNKNOWN)?;
    if !obj.contains_key(field) {
        return Err(MSG_FIELD_UNKNOWN);
    }
    obj.insert(field.to_owned(), value.clone());
    let mut next = current.clone();
    let invalid = if path == "proxy.port" { MSG_INVALID_PORT } else { MSG_INVALID_VALUE };
    set_section(&mut next, section, json).map_err(|_| invalid)?;
    // Wert unverändert angekommen? (sonst hätte serde ihn still umgedeutet, z. B. ein unbekannter
    // Schlüssel in einem Objekt-Wert)
    let roundtrip = section_json(&next, section).and_then(|j| j.get(field).cloned());
    if roundtrip.as_ref() != Some(value) && !numbers_equal(roundtrip.as_ref(), value) {
        return Err(invalid);
    }
    Ok(next)
}

/// `23` und `23.0` sind derselbe Wert (`logo_only_size_mm` als `f64`).
fn numbers_equal(a: Option<&serde_json::Value>, b: &serde_json::Value) -> bool {
    match (a.and_then(serde_json::Value::as_f64), b.as_f64()) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// Richtlinie lässt den **Wert** nicht zu (ausgeblendetes Amtssignatur-Profil, nicht erlaubtes
/// Standard-Signiermittel)?
fn policy_forbids(policy: Option<&Policy>, path: &str, next: &Settings) -> bool {
    let Some(p) = policy else { return false };
    match path {
        "signature.profile" => !p.allows_profile(next.signature.profile),
        "backend.default" => next.backend.default.is_some_and(|b| !p.allows(b)),
        _ => false,
    }
}

/// Änderungen übernehmen (Regeln im Modulkopf). `locked` aus [`locked_paths`].
pub(crate) fn apply_changes(
    current: &Settings,
    changes: &BTreeMap<String, serde_json::Value>,
    locked: &BTreeMap<String, String>,
    policy: Option<&Policy>,
    mobile: bool,
) -> Applied {
    let mut settings = current.clone();
    let mut rejected = BTreeMap::new();
    for (path, value) in changes {
        if let Some(reason) = locked_reason(locked, path) {
            rejected.insert(path.clone(), reason.to_owned());
            continue;
        }
        if mobile && mobile_unavailable(path) {
            rejected.insert(path.clone(), MSG_UNAVAILABLE_MOBILE.to_owned());
            continue;
        }
        let mut next = match set_path(&settings, path, value) {
            Ok(n) => n,
            Err(code) => {
                rejected.insert(path.clone(), code.to_owned());
                continue;
            }
        };
        if policy_forbids(policy, path, &next) {
            rejected.insert(path.clone(), MSG_POLICY.to_owned());
            continue;
        }
        // Mobile: nur ID Austria (bzw. „Jedes Mal fragen“) als Standard-Signiermittel (ADR 0004 Nachtrag 11)
        if mobile
            && path == "backend.default"
            && matches!(
                next.backend.default,
                Some(pida_config::BackendChoice::Keystore | pida_config::BackendChoice::LocalBku)
            )
        {
            rejected.insert(path.clone(), MSG_UNAVAILABLE_MOBILE.to_owned());
            continue;
        }
        // vor `sanitize` prüfen: eine ungültige Logo-Größe wird abgelehnt, nicht still auf 23 mm gesetzt
        if let Some((_, code, _)) = validate(&next).into_iter().find(|(p, _, _)| p == path) {
            rejected.insert(path.clone(), code.to_owned());
            continue;
        }
        normalize(&mut next, path);
        next.sanitize();
        settings = next;
    }
    Applied { settings, rejected }
}

/// Normalisierung eines übernommenen Felds (ADR 0004 §3.11 „Benutzername speichert der Kern normalisiert“):
/// der ID-Austria-Benutzername wie nach der Anmeldung (`+43…` statt `0…`, ohne Leerraum); leer = keiner.
fn normalize(next: &mut Settings, path: &str) {
    // eine Wahl der Speicherart in den Einstellungen gilt als ausdrückliche Wahl (keine Rückfrage mehr bei
    // der ersten Passworteingabe)
    if path == "keystore.password_storage" {
        next.keystore.storage_chosen = true;
    }
    if path == "id_austria.username" {
        let user = next.id_austria.username.take();
        next.id_austria.username =
            user.as_deref().map(str::trim).filter(|u| !u.is_empty()).map(pida_config::normalize_mobile_number);
    }
}

/// Problem eines Felds: (Pfad, i18n-ID, blockiert das klassische Speichern).
pub(crate) type FieldProblem = (String, &'static str, bool);

/// Prüfung der Einstellungen je Feld (TOML-Pfad → i18n-ID). `blocking` = das klassische „Speichern“
/// (`SaveSettings`) lehnt ab; sonst nur Hinweis (wie „Ignorieren“ in PDF-Over, F-102). `ApplySettings`
/// lehnt jedes Feld mit Problem ab, das es gerade setzen sollte.
pub(crate) fn validate(settings: &Settings) -> Vec<FieldProblem> {
    let mut problems = Vec::new();
    let postfix = &settings.output.postfix;
    if postfix.contains(['/', '\\']) || postfix.contains('\0') {
        problems.push(("output.postfix".into(), "ng-settings-invalid-postfix", true));
    }
    if settings.proxy.mode == ProxyMode::Manual && settings.proxy.host.as_deref().is_none_or(|h| h.trim().is_empty()) {
        problems.push(("proxy.host".into(), "ng-settings-proxy-host-missing", true));
    }
    if settings.proxy.port == Some(0) {
        problems.push(("proxy.port".into(), MSG_INVALID_PORT, true));
    }
    if let Some(e) = &settings.signature.emblem
        && !e.fs_is_file()
    {
        problems.push(("signature.emblem".into(), "exception-PathNotExist", false));
    }
    // Noch keine Datei gewählt ist kein Fehler (die Zeile zeigt „Keine Datei gewählt“, der Keystore ist
    // nur noch nicht als Standard wählbar: `SettingsView::keystore_ready`). Fehler nur, wenn die gewählte
    // Datei verschwunden ist.
    if settings.keystore.enabled && settings.keystore.file.as_deref().is_some_and(|f| !f.fs_is_file()) {
        problems.push(("keystore.file".into(), "ng-keystore-file-not-found", false));
    }
    let size = settings.signature.logo_only_size_mm;
    if !(size.is_finite() && size > 0.0 && size <= pida_sigblock::profile::MAX_LOGO_SIZE_MM) {
        problems.push(("signature.logo_only_size_mm".into(), "ng-settings-invalid-logo-size", false));
    }
    if let Some(dir) = &settings.output.folder
        && dir.is_absolute()
        && !dir.fs_exists()
        && dir.parent().is_some_and(|p| !p.fs_exists())
    {
        problems.push(("output.folder".into(), "exception-PathNotExist", false));
    }
    problems
}

/// Ändert sich das Signaturbild (Vorschau neu bauen)? Profil, Blocksprache, Hinweis, Bildmarke, Logo-Größe,
/// PDF/A (eigene Profilvariante).
pub(crate) fn block_affected(before: &Settings, after: &Settings) -> bool {
    let (a, b) = (&before.signature, &after.signature);
    a.profile != b.profile
        || a.block_language != b.block_language
        || a.note != b.note
        || a.emblem != b.emblem
        || a.logo_only_size_mm.to_bits() != b.logo_only_size_mm.to_bits()
        || a.pdfa_force != b.pdfa_force
}

/// Ändern sich Signiermittel (Backends neu bauen, Konto-Chip)?
pub(crate) fn backends_affected(before: &Settings, after: &Settings) -> bool {
    before.id_austria != after.id_austria
        || before.keystore != after.keystore
        || before.proxy != after.proxy
        || before.backend != after.backend
}

/// Vom Kern berechnete Beispiele (`SettingsView::examples`): `output.postfix` → Ausgabename der
/// Beispieldatei `Vertrag.pdf` nach F-81 (die UI baut die Regel nicht nach).
pub(crate) fn examples(settings: &Settings) -> BTreeMap<String, String> {
    let name = pida_workflow::output_file_name(Path::new(EXAMPLE_FILE), &settings.output.postfix);
    BTreeMap::from([("output.postfix".to_owned(), name)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use pida_config::{BackendChoice, NoteSetting, SignatureProfile};
    use serde_json::json;

    fn changes(list: &[(&str, serde_json::Value)]) -> BTreeMap<String, serde_json::Value> {
        list.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()
    }

    fn apply(s: &Settings, list: &[(&str, serde_json::Value)]) -> Applied {
        apply_changes(s, &changes(list), &BTreeMap::new(), None, false)
    }

    #[test]
    fn partial_apply_keeps_rejected_fields_and_takes_the_rest() {
        let s = Settings::default();
        let a = apply(&s, &[("proxy.port", json!(70000)), ("signature.profile", json!("AMTSSIGNATURBLOCK"))]);
        assert_eq!(a.rejected.get("proxy.port").map(String::as_str), Some(MSG_INVALID_PORT));
        assert_eq!(a.settings.proxy.port, None, "bisheriger Wert");
        assert_eq!(a.settings.signature.profile, SignatureProfile::Amtssignaturblock);
        // Port 0, Text statt Zahl
        let a = apply(&s, &[("proxy.port", json!(0))]);
        assert_eq!(a.rejected["proxy.port"], MSG_INVALID_PORT);
        let a = apply(&s, &[("proxy.port", json!("80a"))]);
        assert_eq!(a.rejected["proxy.port"], MSG_INVALID_PORT);
        let a = apply(&s, &[("proxy.port", json!(8080))]);
        assert!(a.rejected.is_empty());
        assert_eq!(a.settings.proxy.port, Some(8080));
    }

    #[test]
    fn unknown_paths_and_values_are_problems() {
        let mut s = Settings::default();
        s.unknown.insert("future".into(), toml::Value::Integer(1));
        for path in [
            "signature",
            "signature.note.text",
            "schema_version",
            "future",
            "future.x",
            "signature.nope",
            "unknown.future",
        ] {
            let a = apply(&s, &[(path, json!(1))]);
            assert_eq!(a.rejected[path], MSG_FIELD_UNKNOWN, "{path}");
            assert_eq!(a.settings, s, "{path}: nichts geändert");
        }
        let a = apply(&s, &[("signature.profile", json!("GIBTS_NICHT")), ("general.update_check", json!("ja"))]);
        assert_eq!(a.rejected["signature.profile"], MSG_INVALID_VALUE);
        assert_eq!(a.rejected["general.update_check"], MSG_INVALID_VALUE);
        assert_eq!(a.settings.unknown, s.unknown, "unbekannte Einträge bleiben unberührt");
    }

    #[test]
    fn values_take_the_serde_form_of_the_field() {
        let s = Settings::default();
        let a = apply(
            &s,
            &[
                ("signature.note", json!({ "mode": "custom", "text": "Eigener Hinweis" })),
                ("signature.block_language", json!("en")),
                ("signature.logo_only_size_mm", json!(30)),
                ("placeholder.search", json!(true)),
                ("placeholder.kind", json!("fields")),
                ("signature.pdfa_force", json!(true)),
                ("output.folder", json!(".")),
                ("backend.default", json!("keystore")),
                ("general.recent_documents", json!(false)),
            ],
        );
        assert!(a.rejected.is_empty(), "{:?}", a.rejected);
        assert_eq!(a.settings.signature.note, NoteSetting::Custom("Eigener Hinweis".into()));
        assert_eq!(a.settings.signature.logo_only_size_mm, 30.0);
        assert!(a.settings.placeholder.search && a.settings.signature.pdfa_force);
        assert_eq!(a.settings.placeholder.kind, pida_config::PlaceholderKind::Fields);
        assert_eq!(a.settings.backend.default, Some(BackendChoice::Keystore));
        assert!(!a.settings.general.recent_documents);
        // Benutzername normalisiert wie nach der Anmeldung; leer = keiner; ein Name bleibt
        let a = apply(&s, &[("id_austria.username", json!(" 0664 123 4567 "))]);
        assert_eq!(a.settings.id_austria.username.as_deref(), Some("+436641234567"));
        let a = apply(&s, &[("id_austria.username", json!("  "))]);
        assert_eq!(a.settings.id_austria.username, None);
        let a = apply(&s, &[("id_austria.username", json!("max.muster"))]);
        assert_eq!(a.settings.id_austria.username.as_deref(), Some("max.muster"));
        // Hinweis für ein anderes Feld lehnt nichts ab
        let a = apply(&s, &[("proxy.mode", json!("manual"))]);
        assert!(a.rejected.is_empty());
        assert!(
            validate(&a.settings).iter().any(|(p, c, _)| p == "proxy.host" && *c == "ng-settings-proxy-host-missing")
        );
        // Prüfung des gesendeten Felds lehnt ab
        let a = apply(&s, &[("output.postfix", json!("a/b")), ("signature.logo_only_size_mm", json!(500))]);
        assert_eq!(a.rejected["output.postfix"], "ng-settings-invalid-postfix");
        assert_eq!(a.rejected["signature.logo_only_size_mm"], "ng-settings-invalid-logo-size");
        assert_eq!(a.settings, s);
        let a = apply(&s, &[("signature.logo_only_size_mm", json!(-5))]);
        assert_eq!(a.rejected["signature.logo_only_size_mm"], "ng-settings-invalid-logo-size", "nicht still 23 mm");
    }

    #[test]
    fn policy_and_mobile_restrictions() {
        let policy = Policy {
            recent_documents: Some(false),
            proxy: Some(pida_config::ProxySettings::default()),
            hide_amtssignatur: true,
            allowed_backends: Some(vec![BackendChoice::IdAustria]),
            ..Policy::default()
        };
        let locked = locked_paths(Some(&policy));
        assert_eq!(locked["general.recent_documents"], MSG_RECENT_POLICY);
        assert_eq!(locked["proxy"], MSG_POLICY);
        let s = Settings::default();
        let a = apply_changes(
            &s,
            &changes(&[
                ("general.recent_documents", json!(true)),
                ("proxy.host", json!("proxy.example")),
                ("signature.profile", json!("AMTSSIGNATURBLOCK")),
                ("backend.default", json!("keystore")),
                ("signature.pdfa_force", json!(true)),
            ]),
            &locked,
            Some(&policy),
            false,
        );
        assert_eq!(a.rejected["general.recent_documents"], MSG_RECENT_POLICY);
        assert_eq!(a.rejected["proxy.host"], MSG_POLICY, "ganzer Abschnitt gesperrt");
        assert_eq!(a.rejected["signature.profile"], MSG_POLICY);
        assert_eq!(a.rejected["backend.default"], MSG_POLICY);
        assert!(a.settings.signature.pdfa_force, "Rest übernommen");
        assert!(locked_paths(None).is_empty());
        // Mobile: Proxy, Ausgabeordner, Keystore (ganzer Abschnitt, ADR 0004 Nachtrag 11) nicht einstellbar; als
        // Standard-Signiermittel nur ID Austria bzw. „Jedes Mal fragen“. Die Update-Prüfung schon (Nachtrag 31).
        let a = apply_changes(
            &s,
            &changes(&[
                ("proxy.mode", json!("none")),
                ("output.folder", json!("/tmp")),
                ("general.update_check", json!(false)),
                ("keystore.file", json!("/x.p12")),
                ("keystore.enabled", json!(true)),
                ("keystore.password_storage", json!("keychain")),
                ("keystore.alias", json!("max")),
                ("backend.default", json!("keystore")),
                ("signature.profile", json!("AMTSSIGNATURBLOCK")),
            ]),
            &BTreeMap::new(),
            None,
            true,
        );
        for p in [
            "proxy.mode",
            "output.folder",
            "keystore.file",
            "keystore.enabled",
            "keystore.password_storage",
            "keystore.alias",
            "backend.default",
        ] {
            assert_eq!(a.rejected[p], MSG_UNAVAILABLE_MOBILE, "{p}");
        }
        assert!(!a.rejected.contains_key("general.update_check") && !a.settings.general.update_check);
        assert_eq!(a.settings.keystore, s.keystore, "Keystore unverändert");
        assert_eq!(a.settings.backend.default, s.backend.default);
        assert_eq!(a.settings.signature.profile, SignatureProfile::Amtssignaturblock);
        for (value, expected) in [(json!("local_bku"), None), (json!("id_austria"), Some(BackendChoice::IdAustria))] {
            let a = apply_changes(&s, &changes(&[("backend.default", value)]), &BTreeMap::new(), None, true);
            assert_eq!(a.settings.backend.default, expected.or(s.backend.default));
            assert_eq!(a.rejected.contains_key("backend.default"), expected.is_none());
        }
        let a = apply_changes(&s, &changes(&[("backend.default", json!(null))]), &BTreeMap::new(), None, true);
        assert!(a.rejected.is_empty(), "„Jedes Mal fragen“ bleibt möglich");
        // Desktop unverändert: Keystore einstellbar und als Standard wählbar
        let a = apply_changes(
            &s,
            &changes(&[("keystore.enabled", json!(true)), ("backend.default", json!("keystore"))]),
            &BTreeMap::new(),
            None,
            false,
        );
        assert!(a.rejected.is_empty(), "{:?}", a.rejected);
        assert_eq!(a.settings.backend.default, Some(BackendChoice::Keystore));
    }

    #[test]
    fn block_change_and_examples() {
        let s = Settings::default();
        let mut t = s.clone();
        t.proxy.port = Some(1);
        assert!(!block_affected(&s, &t));
        assert!(backends_affected(&s, &t));
        t.signature.pdfa_force = true;
        assert!(block_affected(&s, &t));
        assert_eq!(examples(&s)["output.postfix"], "Vertrag_signed.pdf");
        t.output.postfix = "_signiert".into();
        assert_eq!(examples(&t)["output.postfix"], "Vertrag_signiert.pdf");
    }
}
