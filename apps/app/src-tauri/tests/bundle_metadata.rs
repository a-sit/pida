//! Herausgeber und Urheberrecht der Installer und Pakete (ADR 0004 Nachtrag 21, Entscheidung des Auftraggebers:
//! A-SIT). Tauri übernimmt die Werte aus `tauri.conf.json`:
//!
//! - `bundle.publisher` → Windows-Versionsressource `CompanyName` (tauri-build), NSIS `Publisher` in „Apps &
//!   Features“, MSI `Manufacturer`,
//! - `bundle.copyright` → Windows `LegalCopyright` (Versionsressource und NSIS-Installer), macOS
//!   `NSHumanReadableCopyright`,
//! - `bundle.homepage` → deb `Homepage`, rpm `URL`, NSIS/MSI „Info-Link“,
//! - Cargo `authors` → deb `Maintainer`.
//!
//! iOS: das versionierte Xcode-Projekt (`gen/apple`) trägt `NSHumanReadableCopyright` selbst (Info.plist und
//! `project.yml` für xcodegen). Android kennt kein Copyright-Feld im Manifest; dort steht der Vermerk in „Über
//! PIDA“.
//!
//! Windows/NSIS: Der Herausgeber steckt auch im Registrierungsschlüssel `Software\<Herausgeber>\<Produkt>`, unter
//! dem sich der Installer den Ordner merkt. Bis 0.2.0 war das „pida“ (Tauri-Vorgabe aus der damaligen Kennung `at.pida`);
//! `windows/installer-hooks.nsh` übernimmt den alten Ordner, damit „Vor der Installation deinstallieren“ die alte
//! Installation findet.

fn json(text: &str) -> serde_json::Value {
    serde_json::from_str(text).expect("JSON")
}

const TAURI_CONF: &str = include_str!("../tauri.conf.json");
const IOS_INFO_PLIST: &str = include_str!("../gen/apple/pida-app_iOS/Info.plist");
const IOS_PROJECT: &str = include_str!("../gen/apple/project.yml");
const WORKSPACE: &str = include_str!("../../../../Cargo.toml");
const NSIS_HOOKS: &str = include_str!("../windows/installer-hooks.nsh");

#[test]
fn installers_name_a_sit_as_publisher_and_copyright_holder() {
    let conf = json(TAURI_CONF);
    let bundle = &conf["bundle"];
    assert_eq!(bundle["publisher"], "A-SIT", "Herausgeber (CompanyName, Publisher, Manufacturer)");
    let copyright = bundle["copyright"].as_str().expect("bundle.copyright");
    assert!(
        copyright.starts_with("© ") && copyright.contains(" A-SIT") && copyright.ends_with("EUPL-1.2"),
        "{copyright}"
    );
    let year: u16 = copyright[3..7].parse().expect("Jahr im Urheberrechtsvermerk");
    assert!(year >= 2026, "{copyright}");
    assert_eq!(bundle["license"], "EUPL-1.2");
    assert_eq!(bundle["homepage"], "https://github.com/a-sit/pida");
    assert!(!TAURI_CONF.contains("PIDA contributors") && !WORKSPACE.contains("PIDA contributors"));

    // iOS: dieselbe Angabe im Projekt (Info.plist und xcodegen-Vorlage)
    assert!(
        IOS_INFO_PLIST.contains(&format!("<key>NSHumanReadableCopyright</key>\n\t<string>{copyright}</string>")),
        "Info.plist ohne NSHumanReadableCopyright „{copyright}“"
    );
    assert!(IOS_PROJECT.contains(&format!("NSHumanReadableCopyright: \"{copyright}\"")), "project.yml");
}

/// Cargo-Metadaten (deb `Maintainer`, `cargo metadata`): Urheber A-SIT, Repository auf GitHub.
#[test]
fn cargo_metadata_names_a_sit() {
    // `authors.workspace = true` in der Shell (wie in allen Crates)
    assert_eq!(env!("CARGO_PKG_AUTHORS"), "A-SIT");
    assert_eq!(env!("CARGO_PKG_LICENSE"), "EUPL-1.2");
    assert!(WORKSPACE.contains("\nauthors = [\"A-SIT\"]\n"), "[workspace.package] authors");
    assert!(WORKSPACE.contains("\nrepository = \"https://github.com/a-sit/pida\"\n"), "[workspace.package] repository");
}

/// Herausgeber-Wechsel im NSIS-Installer: Der Installer übernimmt beim Start der Oberfläche den Ordner der bis 0.2.0
/// unter „pida“ registrierten Installation (sonst bräche „Vor der Installation deinstallieren“ ab, siehe
/// `windows/installer-hooks.nsh`).
#[test]
fn nsis_installer_adopts_the_registry_key_of_the_former_publisher() {
    let conf = json(TAURI_CONF);
    let nsis = &conf["bundle"]["windows"]["nsis"];
    assert_eq!(nsis["installerHooks"], "windows/installer-hooks.nsh", "bundle.windows.nsis.installerHooks");
    // Installation für den aktuellen Benutzer (Tauri-Vorgabe): Standardordner %LOCALAPPDATA%\<Produkt>
    assert!(nsis.get("installMode").is_none_or(|m| m == "currentUser"), "{nsis}");
    let product = conf["productName"].as_str().expect("productName");
    let publisher = conf["bundle"]["publisher"].as_str().expect("bundle.publisher");
    // bisheriger Herausgeber: Tauri-Vorgabe = zweiter Teil der damaligen Kennung (at.pida → pida; seit 0.5.3 at.asit.pida)
    let legacy = "pida";
    assert_eq!(publisher, "A-SIT");
    let new_key = format!("\"Software\\{publisher}\\{product}\"");
    let old_key = format!("\"Software\\{legacy}\\{product}\"");
    assert!(NSIS_HOOKS.contains("!define MUI_CUSTOMFUNCTION_GUIINIT PidaAdoptLegacyPublisherKey"), "vor den Seiten");
    assert!(NSIS_HOOKS.contains("Function PidaAdoptLegacyPublisherKey") && NSIS_HOOKS.contains("FunctionEnd"));
    assert!(NSIS_HOOKS.contains(&format!("ReadRegStr $0 SHCTX {new_key} \"\"")), "neuer Schlüssel {new_key}");
    assert!(NSIS_HOOKS.contains(&format!("ReadRegStr $0 SHCTX {old_key} \"\"")), "alter Schlüssel {old_key}");
    assert!(NSIS_HOOKS.contains(&format!("WriteRegStr SHCTX {new_key} \"\" $0")), "Übernahme");
    assert!(
        NSIS_HOOKS.contains(&format!("StrCmp $INSTDIR \"$LOCALAPPDATA\\{product}\"")),
        "nur statt des Standardordners"
    );
    // Code nur in ASCII (Kommentare dürfen Umlaute haben)
    for line in NSIS_HOOKS.lines().filter(|l| !l.trim_start().starts_with(';')) {
        assert!(line.is_ascii(), "{line}");
    }
}

/// Laufende App beim Setup bzw. Deinstallieren (tauri-apps/tauri#16277): Die Ergänzung ersetzt die Prüfung der
/// Vorlage (Restart Manager, bricht bei jedem Fehler von RmShutdown ab – auch wenn PIDA schon beendet ist). Sie fragt
/// nur bei laufendem PIDA, beendet notfalls wie bis Tauri 2.11, wartet auf die Datei und bricht nur ab, wenn PIDA
/// danach noch läuft.
#[test]
fn nsis_running_app_check_tolerates_a_closed_pida() {
    let hooks = NSIS_HOOKS;
    assert!(hooks.contains("!macroundef CheckIfAppIsRunning"), "ersetzt die Prüfung der Vorlage");
    assert!(hooks.contains("!macro CheckIfAppIsRunning executablePath productName"), "gleiche Parameter");
    assert!(hooks.contains("nsis_tauri_utils::FindProcess \"${MAINBINARYNAME}.exe\""), "fragt nur bei laufendem PIDA");
    assert!(hooks.contains("nsis_tauri_utils::KillProcessCurrentUser \"${MAINBINARYNAME}.exe\""), "Rückfall wie 2.11");
    assert!(hooks.contains("RmShutdown(p R0, i ${RmForceShutdown}, p 0)"), "zuerst über den Restart Manager");
    assert!(hooks.contains("${If} $R4 < 40") && hooks.contains("Sleep 250"), "bis zu 10 s warten");
    // abbrechen nur, wenn PIDA nach dem Warten noch läuft
    let tail = &hooks[hooks.rfind("nach 10 s noch belegt").expect("Abbruchzweig")..];
    assert!(tail.contains("${If} $R6 != 0\n      Goto pida_done_${UniqueID}"), "kein PIDA mehr: weiter");
    assert!(tail.contains("Abort $R3"), "sonst wie die Vorlage");
}
