# iOS – Paket M-iOS (Projekt, native Plugin-Seite, Nachweise)

Stand: 15.10.2026, Welle Mobile, Paket **M-iOS** auf dem Fundament M0 (docs/integration/mobile.md); neue Oberfläche
am Gerät durchgespielt am 16.10.2026 (§8).
Gemessen auf macOS 27 (arm64), Xcode 27.0 (27A266a), Swift 6.4, Rust 1.99, Tauri 2.12.4 / CLI 2.12.3,
iOS-Simulator **iOS 27.0** (iPhone 17), PDFium chromium/8100.

Ergebnis: Die App baut für den Simulator, startet (`pida-smoke: ok … pdfium=true`), nimmt Dokumente über
„Öffnen in“/Teilen (Kalt- und Warmstart) und die Dateiauswahl an, importiert PKCS#12 in die Sandbox, legt das
Keystore-Passwort in der Keychain ab und signiert damit gültig (pyHanko: *VALID*). Ergebnis teilen, „Sichern
unter …“ und „Öffnen“ (QuickLook) laufen nativ. FIDO2 ist ausgegraut mit Hinweis.

> **Seit 16.10.2026: Mobile nur mit ID Austria** (Entscheidung des Auftraggebers, ADR 0004 Nachtrag 11,
> mobile.md §12). Die Keystore-Abläufe dieses Protokolls – PKCS#12-Import in die Sandbox, Keystore-Passwort in der
> Keychain, Signatur und Stapel mit dem Keystore, Speicherart „Sicherer Speicher“, „Passwort merken?“, Umzug des
> Keystores nach einem Update (§5 I-06/I-08–I-10, §7 Nr. 1 und 4, §8.2 Abläufe 3/4, §8.3 Nr. 16/18/21, §8.5) –
> **entfallen**; sie bleiben als Nachweis des damaligen Stands stehen. Auf iOS gibt es seitdem nur ID Austria; der
> Kern ignoriert Keystore-Einstellungen früherer Builds und entfernt deren Keystore-Passwörter aus der Keychain
> einmalig (§9).

## 1. Projekt `apps/app/src-tauri/gen/apple`

Angelegt mit `npm run tauri ios init -- --ci --skip-targets-install` und **versioniert** (Build-Ausgaben
`build/`, `Externals/`, `.tauri/` ignoriert `gen/apple/.gitignore`). Quelle der Wahrheit ist
`project.yml` (XcodeGen); nach Änderungen `cd apps/app/src-tauri/gen/apple && xcodegen generate`.
**Nicht** erneut `tauri ios init` ausführen – das überschreibt `project.yml`.

| Punkt | Umsetzung |
|---|---|
| Bundle-ID | `at.asit.pida.app` (`tauri.conf.json` `identifier`, `PRODUCT_BUNDLE_IDENTIFIER`); Anzeigename „PIDA“ |
| Mindest-iOS | **16.0** (Plan §5.1): `tauri.conf.json` `bundle.iOS.minimumSystemVersion`, `project.yml` `deploymentTarget`, `Podfile`; App-Binary `LC_BUILD_VERSION minos 16.0` (geprüft mit `vtool -show-build`). Abweichung zu M0 (17.0) siehe §3 |
| Dokumenttypen | `CFBundleDocumentTypes`: `com.adobe.pdf`, Rolle `Viewer`, `LSHandlerRank` **`Alternate`** (nie Standard-App für PDF); Tauri ergänzt beim Bauen `CFBundleTypeExtensions: [pdf]` aus `bundle.fileAssociations` |
| Öffnen am Ort | `LSSupportsOpeningDocumentsInPlace = YES` – iOS übergibt das Original als Security-Scoped URL, die App kopiert die **Bytes** in die Sandbox (§2.5). Zusammen mit `UIFileSharingEnabled = YES` erscheint `Documents` (signierte Dokumente) in der Dateien-App unter „Auf meinem iPhone › PIDA“ (Screenshots 05, 08) |
| Sprachen | `CFBundleLocalizations = [de, en]` (§8.5 R4): Systemoberfläche im Prozess der App (Dokumentauswahl, Teilen, Kontextmenüs) in der Gerätesprache; ohne Angabe galt die App als nur englisch (`CFBundleDevelopmentRegion` en) |
| Exportkontrolle | `ITSAppUsesNonExemptEncryption = NO`: nur Standardverfahren (TLS, Signatur/Authentisierung) – **vom Auftraggeber zu bestätigen** (Exportkontroll-Einstufung) |
| ATS | keine Ausnahmen (`NSAppTransportSecurity` fehlt bewusst): nur HTTPS, keine lokale BKU. Die WebView lädt die gebündelte Oberfläche (`tauri://`, `pida-render://`), nicht `devUrl` |
| Face ID, Kamera, Fotos | keine Zweckangaben (`NSFaceIDUsageDescription` u. a.), weil nicht genutzt |
| Share-Extension | keine (Plan: „Öffnen in“ reicht; die App erscheint im Teilen-Menü über die Dokumenttypen, Screenshot 02) |
| Szenen | kein `UIApplicationSceneManifest`. iOS 27 führt die App trotzdem im Scene-Lebenszyklus aus (`KnownSceneSessions` im Container); tao 0.37 unterstützt beide Wege (App-Delegate bis iOS 26, Szenen ab iOS 27) |
| Orientierung | Vorlage: iPhone Hochformat + Querformat, iPad alle |
| Signierung | Simulator: ad hoc („Sign to Run Locally“). Für Geräte `bundle.iOS.developmentTeam` bzw. `APPLE_DEVELOPMENT_TEAM` setzen (nicht Teil von M-iOS) |

### PDFium

`project.yml` bettet `tools/pdfium/apple/PDFium.xcframework` (aus `tools/fetch-pdfium.sh --xcframework`) mit
**Embed & Sign** ein, **ohne zu linken** (`link: false`): `pida-render` lädt
`<App>.app/Frameworks/pdfium.framework/pdfium` zur Laufzeit (`pdfium::mobile_dir`). Xcode wählt den Slice
(`ios-arm64_x86_64-simulator` bzw. `ios-arm64`) und signiert ihn mit (Simulator: ad hoc). Bindung zur
Laufzeit belegt die Startprüfung (`pdfium=true`) und die Seitenvorschau (Screenshot 03). Fehlt das
xcframework, bricht der Xcode-Build ab – vorher `tools/fetch-pdfium.sh --xcframework` ausführen.

## 2. Native Plugin-Seite (`apps/app/plugins/pida-mobile/ios`)

Swift-Paket (Swift-Tools 5.3, Paket-Mindestversion iOS 14 für UTType/Dokument-Picker, App iOS 16), Dateien in
`Sources/`. Die Methoden laufen auf der seriellen IPC-Warteschlange von Tauri; UIKit nur über
`DispatchQueue.main`, Dateikopien auf einer eigenen Warteschlange (der sichere Speicher wartet nie hinter
einem iCloud-Download). Fehler nur als kurzer Code (`invoke.reject(_, code:)`), nie mit Pfaden oder Werten.

| Methode | Umsetzung | Codes |
|---|---|---|
| `secureStoreGet/Set/Delete` | `SecureStore.swift`: `kSecClassGenericPassword`, Dienst `<Bundle-ID>.secrets`, Konto = Schlüssel der Rust-Seite, **`kSecAttrAccessibleWhenUnlockedThisDeviceOnly`**, nicht synchronisiert. Setzen = `SecItemUpdate`, sonst `SecItemAdd`; Löschen eines fehlenden Eintrags ist kein Fehler | `keychain-locked`, `keychain-unavailable`, `keychain-error`, `keychain-bad-data` |
| (Start) | Keychain-Einträge überleben das Löschen der App: Fehlt die Markierung in `UserDefaults` (erste Ausführung nach einer Installation), löscht das Plugin alle Einträge seines Dienstes. Außerdem legt es `Library/Application Support/<Bundle-ID>/data/keystores` (0700) an und nimmt den Ordner von iCloud-/Geräte-Sicherungen aus (`isExcludedFromBackup`, Review R-5) | – |
| `shareFile` | `UIActivityViewController` mit der Datei-URL (nur Dateien in der Sandbox); `{}` sobald gezeigt; iPad: Popover mittig | `file-missing`, `no-view` |
| `openFile` | **QuickLook** (`QLPreviewController`, Bearbeiten gesperrt – jede Änderung bräche die Signatur); darin auch Teilen/Drucken. Vertrag von M-Android eingeführt (`PidaMobile::open_file`, `ShellHooks::open_path` auf Mobile) | `file-missing`, `no-preview`, `no-view` |
| `exportFile` | `UIDocumentPickerViewController(forExporting:asCopy: true)`; weicht `fileName` ab, zuerst eine benannte Kopie in `tmp/export-…`; Abbruch (auch Wegwischen) → `cancelled` | `file-missing`, `busy`, `no-view`, `io-error` |
| `importFile` | `UIDocumentPickerViewController(forOpeningContentTypes:asCopy: true)`, `pdf` → `UTType.pdf`, `pkcs12` → `com.rsa.pkcs-12`/`.p12`/`.pfx`; Kopie nach `<destination>/<n>/<Name>`, iOS-Kopie in `tmp` danach gelöscht | `bad-args`, `bad-destination`, `busy`, `cancelled` |
| `takePendingOpenDocuments` | `OpenedDocuments.swift` (§2.5) | `bad-destination` |

`destination` muss absolut sein, ohne `..`, innerhalb des App-Containers; Dateien höchstens 512 MiB, nur
reguläre Dateien, Namen ohne Pfadanteile/Steuerzeichen (wie `mobile::safe_name`).

### 2.5 Eingehende Dokumente

Mit „Öffnen am Ort“ liefert iOS Security-Scoped URLs; lesbar sind sie nur über **genau das `URL`-Objekt**.
tao wandelt die URL für `RunEvent::Opened` in einen String (die Sandbox-Erweiterung geht verloren) und meldet
beim Kaltstart im Scene-Lebenszyklus die URLs aus `UIScene.ConnectionOptions` gar nicht. Deshalb hängt sich
das Plugin beim Laden – vor `UIApplicationMain`, Tauri initialisiert die Plugins in `Builder::build` – vor
die Delegate-Methoden von tao (`TaoSceneDelegate scene:willConnectToSession:options:` und
`scene:openURLContexts:`, `AppDelegate application:openURL:options:`), merkt sich die URL-Objekte und ruft
danach die ursprüngliche Methode auf. tao selbst bleibt unverändert.

- **Kaltstart:** URLs aus den Verbindungsoptionen liefert der erste `takePendingOpenDocuments` von selbst.
- **Laufende App:** tao meldet die URL; liest die Rust-Seite sie selbst (Simulator, eigene Dateien), ist sie
  fertig; sonst reicht sie die URL weiter, und das Plugin öffnet sie über das gemerkte Objekt
  (`startAccessingSecurityScopedResource`, `NSFileCoordinator` für File Provider wie iCloud Drive).
  Nicht angefragte Einträge verfallen nach 10 min.
- **`Documents/Inbox`** (AirDrop, Mail): übernommene Kopien entfernt das Plugin; ältere als 10 min räumt es
  auf, damit sie nicht in der Dateien-App stehen bleiben.
- Fehlt eine Klasse/Methode (andere tao-Version), protokolliert das Plugin „hook … nicht installiert“ und es
  bleibt beim Weg ohne Hook (Kopien in der Sandbox funktionieren weiter, Security-Scoped URLs auf Geräten
  nicht). Bei jedem tao-Update prüfen.

### 2.6 Lebenszyklus

Ohne Zutun des Plugins: tao bildet `sceneWillResignActive`/`applicationWillResignActive` auf
`WindowEvent::Suspended` und `sceneWillEnterForeground`/`applicationWillEnterForeground` auf
`WindowEvent::Resumed` ab, die Shell macht daraus `UiEvent::Backgrounded`/`Foregrounded` (kein Abbruch,
Long-Poll-Neustart nach der Rückkehr, M0). Ein zusätzlicher Weg aus Swift würde die Ereignisse doppeln.
Bewusst **keine** Hintergrundzeit (`beginBackgroundTask`): Das System hielte den Vorgang dann bis zu ~30 s
nach dem Wechsel zur App „ID Austria“ am Leben und könnte ihn an beliebiger Stelle (auch nach der
Bestätigung, mitten in der Signaturabholung) einfrieren; ohne Hintergrundzeit endet die Verbindung fast immer
im Long-Poll, den der Kern nach der Rückkehr gezielt neu startet.

## 3. Gemeinsame Dateien (minimal)

- `tauri.conf.json`: nur `bundle.iOS.minimumSystemVersion` 17.0 → **16.0**.
- `tools/fetch-pdfium.sh --xcframework`: neue Variable `PDFIUM_IOS_MIN` (Standard **16.0**, leer = wie die
  Binaries). pdfium-binaries chromium/8100 baut mit `ios_deployment_target = 17.0`, importiert aber nur
  Funktionen aus libSystem, CoreFoundation und CoreGraphics, die es lange vor iOS 16 gibt (`nm -u`: 164
  Symbole, u. a. `CGBitmapContextCreate`, `CFStringCreateWithCStringNoCopy`, `aligned_alloc`), und linkt
  libc++ statisch. Das Skript setzt deshalb `LC_BUILD_VERSION` mit `vtool -set-build-version` auf 16.0
  (nur die Kennzeichnung, nicht der Code) und `MinimumOSVersion` im Framework entsprechend. **Nicht auf
  iOS 16 getestet** (auf diesem Mac gibt es nur die iOS-27-Laufzeit) – vor einer Freigabe auf einem
  iOS-16-Gerät prüfen oder PDFium selbst mit `ios_deployment_target = "16.0"` bauen.
- `apps/app/src-tauri/src/smoke.rs`: Die Startprüfung beendet die App unter iOS mit `process::exit`. tao kennt
  auf iOS kein `ControlFlow::Exit`; `app.exit` endete in einer Panik im Run-Loop-Beobachter und damit in
  `abort` (Absturzbericht nach jedem Smoke-Lauf).
- `apps/app/src-tauri/.gitignore`: `/gen/apple` nicht mehr ignoriert. Wurzel-`.gitignore`: `/target-*/`
  (eigene Target-Ordner der Mobil-Pakete).

## 4. Werkzeuge und Bauen

Einmalig auf dem Entwicklungs-Mac (für M-iOS so eingerichtet):

```sh
xcodebuild -downloadPlatform iOS        # iOS-Simulator-Laufzeit (iOS 27.0, 8 GB) – fehlte trotz Xcode
brew install xcodegen cocoapods          # tauri ios init verlangt beide (CocoaPods ungenutzt, Podfile leer)
rustup component add llvm-tools          # swift-rs 1.0.9: Xcode 27 macht @_cdecl-Symbole lokal, swift-rs
                                         # macht sie mit llvm-objcopy wieder global (sonst Linkfehler)
tools/fetch-pdfium.sh --xcframework      # tools/pdfium/apple/PDFium.xcframework (jetzt auch per xcodebuild)
```

`tauri ios init` installierte zusätzlich `libimobiledevice` (Homebrew). `xcodebuild -create-xcframework`
funktioniert inzwischen (die erste `simctl`-Ausführung hat die Xcode-Erstkomponenten nachinstalliert); der
Rückfall im Skript bleibt für andere Rechner.

Bauen über einen Pfad ohne Leerzeichen (geprüft; mit eigenem Target-Ordner, damit M-Android parallel bauen
kann):

```sh
ln -sfn "<worktree>" /tmp/pida-ios
cd /tmp/pida-ios/apps/app && source ../../tools/env.sh
export CARGO_TARGET_DIR=/tmp/pida-ios/target-ios LANG=en_US.UTF-8
npx tauri ios build --target aarch64-sim --debug
#  → src-tauri/gen/apple/build/arm64-sim/PIDA.app (Debug 224 MB: Binary 209 MB unoptimiert mit Debug-Infos, PDFium universal 14 MB)

DEV=<UDID>; xcrun simctl boot $DEV; xcrun simctl install $DEV "src-tauri/gen/apple/build/arm64-sim/PIDA.app"
SIMCTL_CHILD_PIDA_SMOKE=1 xcrun simctl launch $DEV at.asit.pida.app
xcrun simctl spawn $DEV log show --last 1m --predicate 'process == "PIDA"' | grep pida-smoke
```

stdout/stderr leitet Tauri unter iOS in das System-Log um (`log show`), die App-Logdatei liegt im Container
unter `Library/Application Support/at.asit.pida.app/logs/`. Prüfungen: `cargo clippy --no-deps -p pida-app
-p tauri-plugin-pida-mobile --target aarch64-apple-ios{,-sim} -- -D warnings` (grün; der Build des
Geräte-Ziels kompiliert dabei auch das Swift-Paket für `arm64-apple-ios`), `cargo test -p pida-app -p
tauri-plugin-pida-mobile` (58 Tests grün), Swift-Paket ohne Warnungen der eigenen Quellen
(`swift build --triple arm64-apple-ios16.0[-simulator] --sdk …`).

## 5. Nachweise (15.10.2026, iPhone 17, iOS 27.0, Sprache de)

| Nr. | Prüfung | Ergebnis |
|---|---|---|
| I-01 | Simulator-Build | `** BUILD SUCCEEDED **`; `Frameworks/pdfium.framework` (universal x86_64+arm64, ad hoc signiert), App und PDFium `minos 16.0` |
| I-02 | Startprüfung `PIDA_SMOKE=1` | `pida-smoke: ok step=open pdfium=true webview_ms=913 core_ms=914` (vorher 660 ms warm), Prozessende ohne Absturzbericht (vor dem Fix in `smoke.rs`: SIGABRT) |
| I-03 | Startbildschirm | Screenshot 01 |
| I-04 | „Öffnen in“ **Kaltstart** (App beendet, Vorschau › Teilen › PIDA) | Log `Dokumente eingegangen count=1`, Positionieren mit gerenderter Seite; Lieferung über den Hook `scene:willConnectToSession:options:` (tao meldet hier nichts) |
| I-05 | „Öffnen in“ **laufende App** | Screenshot 02; genau ein Dokument (`inbox/2/Testdokument.pdf`), kein Duplikat |
| I-06 | Dateiauswahl PDF/PKCS#12 | Dokument-Picker filtert (p12 bzw. PDF ausgegraut), Ordner „PIDA“ sichtbar (Screenshot 05) |
| I-07 | Positionieren per Touch | Block folgt dem Finger, steht darüber; Felder 38 mm / 249 mm (Screenshot 03) |
| I-08 | Signiermittel-Auswahl | ID Austria mit „FIDO2 nicht verfügbar“ (durchgestrichen) und Hinweis, Keystore wählbar (Screenshot 04) |
| I-09 | PKCS#12-Import `test-rsa2048.p12`, Passwort `test`, „sicher speichern“ | `ng-keystore-imported`, Datei 0600 in `data/keystores/`, Einstellungen `password_storage = "keychain"` (Screenshot 06) |
| I-10 | Keystore-Signatur in der App | ohne Passwortabfrage (Keychain), auch **nach Neustart des Prozesses**; drei Läufe, pyHanko `--trust test-rsa2048.crt.pem`: *trusted, cryptographically sound, covers the entire file, VALID*; `qpdf --check` ohne Fehler |
| I-11 | Ergebnis teilen | Share-Sheet mit `Testdokument_signed.pdf` (Screenshot 07) |
| I-12 | „Sichern unter …“ | Export-Picker mit Ziel „PIDA“ (Screenshot 08); Abbrechen durch Wegwischen → keine Fehlermeldung |
| I-13 | „Öffnen“ | QuickLook-Vorschau mit Signaturblock (Screenshot 09); vor `openFile`: Hinweis „Konnte signiertes Dokument nicht öffnen“ |
| I-14 | Lebenszyklus | Home-Taste → `Lebenszyklus suspended=true`, zurück → `suspended=false`, gleicher Prozess, Zustand erhalten |
| I-15 | Dateien-App | „Auf meinem iPhone › PIDA“ zeigt `Testdokument_signed.pdf` |

Screenshots: `docs/integration/screenshots/ios/01-start.jpg` … `11-ueberschreiben-nur-dateiname.jpg` (verkleinert; 01, 03–11
neu aus dem Review, mobile.md §10; 02 aus M-iOS).

**Review (15.10.2026, mobile.md §10):** neu gebaut und erneut durchgespielt – Startprüfung `pdfium=true`,
Keystore-Import (Cache-Kopie entfernt, `data/keystores` mit Sicherungsausschluss), Signatur ohne Passwortabfrage,
Teilen, „Sichern unter“, QuickLook, Überschreiben-Rückfrage nur mit Dateinamen (Screenshot 11), Update-Installation
mit neuer Container-UUID: Keystore und Keychain-Passwörter neu zugeordnet, Signatur weiter ohne Abfrage. Alle
signierten Dokumente laut pyHanko *VALID*. Gestartet mit `SIMCTL_CHILD_HTTPS_PROXY=http://127.0.0.1:9` (u. a.), damit
kein Request A-Trust erreichen kann.

## 6. Nicht nachgewiesen

- **App-Wechsel während der Wartemaske** und **„Signatur fortsetzen“ nach App-Kill**: Beides braucht einen
  Vorgang, der nach `prepare` wartet – das gibt es nur mit ID Austria (A-Trust), Keystore signiert sofort.
  Die Logik ist durch die Kern-Tests von M0 abgedeckt (`core-api` `tests/mobile.rs`, `workflow`
  `tests/lifecycle.rs`); der Simulator belegt nur die Lebenszyklus-Ereignisse (I-14).
- Security-Scoped URLs auf einem **Gerät** (der Simulator erzwingt die Sandbox beim Lesen nicht; dort liest
  schon die Rust-Seite). Ebenso iCloud-Drive-Dateien (Download über `NSFileCoordinator`).
- iOS 16 bis 26 (nur iOS-27-Laufzeit vorhanden): App-Delegate-Weg von tao, PDFium mit umgesetztem
  `minos`.
- Geräte-Build mit Team-Signierung, App-Store-Validierung.

## 7. Befunde und offene Punkte

1. **Behoben im Review (mobile.md §10, R-1).** **Kern, hoch: absolute Pfade in den Einstellungen.** iOS vergibt bei jeder Installation/Aktualisierung
   einen neuen Container-Pfad (`…/Data/Application/<UUID>/`). `[keystore] file` (und
   `general.last_source_directory`) speichern den absoluten Pfad – nach einem App-Update ist der Keystore
   „nicht auf diesem Gerät“, obwohl die Datei mitgewandert ist. Folge im Test: Mit nur einem verbleibenden
   Signiermittel startete die App ohne Auswahl direkt ID Austria (anonymer InfoboxRead-Request an A-Trust,
   sofort abgebrochen). Vorschlag: Keystore-Pfad relativ zu `Paths::data_dir` speichern bzw. beim Laden
   unter `data_dir/keystores/<Name>` neu auflösen (core-api/config, nicht M-iOS).
2. **Behoben (R-2).** **Shell:** `app.exit` panikt unter iOS (tao); betroffen sind außer der Startprüfung `Finished` →
   `app.exit(code)` und `DispatchOutcome::Quit` in `lib.rs` (z. B. „Beenden“ im Fehlerschritt ohne Kern).
   Auf Mobile nicht beenden bzw. `process::exit` nur nach Rückfrage.
3. **Behoben (R-8).** **Oberfläche:** Rückfrage „Datei … existiert bereits“ zeigt den vollen Sandbox-Pfad (unlesbar, läuft aus
   dem Dialog) – auf Mobile nur den Dateinamen. Unterstrichene Tastenkürzel (Mnemonics) sind auf dem
   Telefon ohne Nutzen.
4. **Behoben (R-4).** **Kern/Shell:** Kopien im Cache bleiben liegen: `Caches/…/import/<n>/test-rsa2048.p12` (der importierte
   Keystore liegt danach ein zweites Mal im Cache) und `inbox/<n>/…`. Nach dem Import bzw. nach Abschluss
   löschen.
5. **Behoben (16.10.2026, §8).** Logs im System-Log enthielten ANSI-Farbcodes (stderr-Ausgabe im Debug-Build) –
   `core-api/src/logging.rs` schaltet sie auf iOS/Android, ohne Terminal und mit `NO_COLOR` ab.
6. **Behoben (R-7).** `tauri-plugin-dialog` ist auch auf Mobile registriert und zieht `Photos`/`PhotosUI` in das Binary – vor
   der App-Store-Einreichung prüfen, ob Zweckangaben verlangt werden, sonst das Plugin nur auf dem Desktop
   registrieren.
7. **Behoben (16.10.2026, Integration `mobile/int`).** `CFBundleShortVersionString`/`CFBundleVersion` setzte die CLI
   auf 1.0.0, die App meldete 0.1.0 – jetzt `version` 0.1.0 in `tauri.conf.json`, `Info.plist`/`project.yml`
   ebenso (Test `apps/app/src/app/version.test.ts`).
8. **Behoben (R-10).** Doku-Tabellen in `apps/app/plugins/pida-mobile/src/lib.rs` und docs/integration/mobile.md §2 nennen
   für die Keychain noch `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`; umgesetzt ist (Vorgabe M-iOS)
   `…WhenUnlockedThisDeviceOnly` – der Kern liest Secrets nur im Vordergrund. Ebenso ist dort `openFile`
   für iOS noch als fehlend beschrieben.
9. Hooks vor den Delegates von tao (§2.5) hängen an Klassen- und Selektornamen von tao 0.37.1 – bei
   Tauri-Updates prüfen (Log „hook … nicht installiert“).
10. Optional: Pinch-Zoom in der Vorschau, Bildmarke aus der Fotomediathek, Managed App Configuration.

## 8. Neue Oberfläche (16.10.2026)

Branch `mobile/ios` mit dem Stand von `main` vom 16.10. (neue Oberfläche E1–E4 nach ADR 0004, dazu
Keystore-Formaterkennung mit Schlüsselauswahl und „Passwort merken?“, Papierkorb im Stapel, Signierkarte mit
schrumpfenden Vergleichswert-Kästchen). Gerät: Simulator iPhone 17, iOS 27.0, Sprache de, Debug-Build.
Aufnahmen: `docs/integration/screenshots/ios-ng/01-…` bis `79-…` (540 px breit bzw. hoch; `…-befund-…` zeigt den
Zustand vor der jeweiligen Behebung). 01–50 stammen aus dem ersten, wegen des Nutzungslimits unterbrochenen Lauf
(WIP-Commit), 51–79 aus der Fortsetzung mit dem gemergten `main`.

### 8.1 Bauweg und Bedienung

- Bauen wie in §4 über den Symlink ohne Leerzeichen (`/tmp/pida-ios`, `CARGO_TARGET_DIR=/tmp/pida-ios/target-ios`):
  `npx tauri ios build --target aarch64-sim --debug` (inkrementell 1–3 min), danach `xcrun simctl install`.
  Ein Update-Install vergibt einen neuen Container – genau der Fall aus Befund 1 unten.
- Start **immer ohne Weg zu A-Trust**: `SIMCTL_CHILD_HTTPS_PROXY`, `…_HTTP_PROXY`, `…_ALL_PROXY` (auch klein
  geschrieben) = `http://127.0.0.1:9` (Verbindung abgelehnt). Für die Warteansicht von ID Austria stattdessen ein
  lokales „schwarzes Loch“ auf `127.0.0.1:9999`, das Verbindungen annimmt und nie antwortet (leitet nichts weiter).
  Keine Zugangsdaten eingegeben.
- Startprüfung: `SIMCTL_CHILD_PIDA_SMOKE=1` → im System-Log
  `[stdout] pida-smoke: ok step=open pdfium=true webview_ms=827 core_ms=827` (erst nach Befund 2 sichtbar).
- Oberfläche bedient über den **Web-Inspektor der WKWebView** (Debug-Build; WIRP-Protokoll über den Socket von
  `webinspectord_sim`, `Runtime.evaluate`): Knöpfe per `click()`, Feldwerte per Wert-Setter + `input`-Ereignis,
  Messungen (Überlauf, Safe Areas, Zoom, Barrierefreiheit). Native Teile (Dokument-Picker, Share-Sheet,
  Home-Taste, Schalter im Import) per Tipp im Simulator; Aufnahmen mit `xcrun simctl io <udid> screenshot`.
- **Querformat:** `simctl` kann nicht drehen und UI-Scripting des Simulators ist auf diesem Mac nicht freigegeben –
  gedreht mit einem kleinen XCUITest-Hilfsprojekt (`XCUIDevice.shared.orientation`, `xcodebuild
  test-without-building`), dessen Runner danach wieder deinstalliert wurde. Dunkel: `xcrun simctl ui <udid>
  appearance dark`; Schriftgröße: `xcrun simctl ui <udid> content_size accessibility-extra-large`.
- Eigenheiten der Werkzeuge: Der Web-Inspektor antwortet nicht, solange ein Menü (React-Aria-Popover) offen ist
  bzw. kurz nach großen Ansichtswechseln – dann Tipp statt Skript oder App neu starten. Eingefügter Text
  (Simulator-Zwischenablage) kommt in WKWebView-Feldern nicht an. Die Software-Tastatur erscheint nicht, weil die
  Hardware-Tastatur verbunden ist (nur die Zubehörleiste); Aufnahmen 16 und 35 aus dem ersten Lauf zeigen sie.

### 8.2 Ergebnisse je Ablauf

| Nr. | Ablauf | Aufnahmen | Ergebnis |
|---|---|---|---|
| 1 | Start, Ersteinrichtung (frisch, ohne PDF-Over-Altkonfiguration), „Später einrichten“ | 01–03, 73 | ok; nach „Später einrichten“ `first_run_completed`, Keystore-Speicherart noch nicht gewählt (`storage_chosen = false`) |
| 2 | PDF empfangen (Teilen/„Öffnen in“ aus Dateien, Dokumentauswahl), Arbeitsbereich mit Bottom-Sheet, Block automatisch platziert, Touch-Ziehen mit Einrasten, Seitenwechsel, Signaturfeld/QR-Platzhalter | 04–12, 24, 25, 48, 56 | ok. Befund 17 (Teilen mehrerer PDFs, offen) |
| 3 | Keystore per Import im neuen Stil, Signiermittel-Menü, Signieren → Statuskarte → Fertig, Teilen, „Sichern unter“, Öffnen (QuickLook) | 13–23, 49, 52, 74–78 | ok; „Passwort merken?“ bei der ersten Verwendung nach Neuinstallation (76) → `password_storage = "keychain"`, danach liest die Einstellungsansicht den Schlüssel selbst (78). pyHanko `--trust test-rsa2048.crt.pem`: *trusted, cryptographically sound, covers the entire file, VALID* (Einzeldokument erster Lauf, `Testdokument_signed.pdf` nach Neuinstallation) |
| 4 | Stapel: mehrere PDFs (Mehrfachauswahl), Übersicht mit Miniaturen, Papierkorb je Zeile, „Alle signieren“ mit Keystore, Zusammenfassung; App in den Hintergrund und zurück; App beenden während des Laufs | 25–29, 50, 63–67 | ok: Hintergrund/Vordergrund mitten im Lauf (Passwortabfrage) → `Lebenszyklus suspended=true/false`, Karte und Lauf bleiben, danach 3 von 3 signiert (67); pyHanko *VALID* für beide Stapel (je 3 Dokumente). App-Kill: siehe 8.4 |
| 5 | Einstellungen im Vollbild: alle Bereiche, Suche, Bereich „Dokument“ (PDF/A, Platzhaltersuche), Live-Vorschau des Signaturbilds, Datenschutz/„Zuletzt signiert“, Mobile-Einschränkungen | 30–43, 51, 54, 68, 71, 72, 78 | ok; ohne Proxy, Ausgabeordner, Update-Prüfung und Log-Ordner (38, 40), Suche „Proxy“ ohne Treffer (68). Befunde 1, 5, 6, 11, 16 |
| 6 | Prüfmodus mit signiertem Dokument (Status, „Auf der Seite zeigen“), „Dieses Dokument signieren“ (seit ADR 0004 Nachtrag 15 entfernt) | 44–47, 70 | ok (auch aus „Zuletzt signiert“ und im Querformat) |
| 7 | ID Austria ohne Netz: Auswahl, Datenschutz-Kurzhinweis, Signieren → Fehler-/Warteansicht, FIDO2 | 57–62, 78 | ok: Kurzhinweis „Das Dokument wird zum Signieren an A-Trust übermittelt. Details“ in der Karte (58), App-Wechsel während des Verbindungsaufbaus ist kein Abbruch (61), Abbruch nach Zeitüberschreitung (62); FIDO2 in den Einstellungen mit Hinweis „auf diesem Gerät nicht verfügbar“ (78). Befunde 13, 14 |
| 8 | Safe Areas (Dynamic Island, Statusleiste, Home-Indikator), Tastatur, Schriftgröße, Bildschirmleser | 53–55, 68–72, 79 | nach den Befunden 4, 7–10, 15 ok: Hochformat und Querformat ohne Inhalt in der Unsicher-Zone (Messung gegen `env(safe-area-inset-*)`: 62 px links/rechts, 20 px unten), kein Zoom beim Fokus (`visualViewport.scale = 1`), Dynamic Type bis 200 % ohne waagrechten Überlauf. Barrierefreiheit per Inspektion auf Start, Arbeitsbereich, Einstellungen, Stapel, Prüfmodus: `lang="de"`, keine Bedienelemente ohne Namen, keine Bilder ohne `alt`, keine fokussierbaren Elemente unter `aria-hidden`, keine doppelten IDs, Überschriften je Ansicht; Knöpfe ≥ 44 × 44 px (außer dem nur bei Tastaturfokus sichtbaren Sprunglink) |

### 8.3 Befunde und Behebungen

1. **Kern: „Zuletzt signiert“ nach einem Update leer** („Datei nicht mehr vorhanden“): `recent.json` speichert
   absolute Pfade, iOS vergibt bei jedem Update einen neuen Container. Wie Keystore und Bildmarke (§7 Nr. 1)
   stellt der Kern auf Mobile fehlende Einträge auf die App-Dokumente um (`pida_config::RecentStore::relocate`,
   `core-api/src/recent.rs` `relocate_recent`); Log „„Zuletzt signiert“ nach Umzug der Sandbox neu zugeordnet
   entries=4“, Aufnahme 51. Tests: `recent::tests::relocate_…`, `tests/mobile.rs`
   `recent_documents_follow_a_moved_sandbox` (iOS umgestellt, Desktop unverändert).
2. **Shell: Zeile der Startprüfung fehlte im System-Log.** Tauri leitet stdout über eine Pipe und eine
   Dispatch-Quelle (QoS „utility“) weiter, `process::exit` kam ihr zuvor. `smoke.rs` wartet unter iOS 500 ms.
3. **ANSI-Farbcodes im System-Log** (§7 Nr. 5): `core-api/src/logging.rs` (`stderr_ansi`, Test) – Log jetzt
   `[stderr]  INFO PIDA startet version=0.1.0`, keine `ESC[`-Folgen.
4. **Oberfläche: Seite blieb nach dem Fokus eines Felds vergrößert** (`visualViewport.scale = 1.067`,
   Kopfzeile unter der Statusleiste, Ränder abgeschnitten, Aufnahme 52): WebKit zoomt bei Schrift unter 16 px.
   Eingabefelder auf Touch-Geräten mit 16 px (`components.css`).
5. **Segmentierte Auswahl abgeschnitten** („Schlüsselbund“ bzw. „Im sicheren Speicher“ hinter einer
   Scrollleiste, betrifft auch das neue „Passwort merken?“): Segmente brechen um (`.segmented`, 54, 76, 78).
   Nach der Integration mit Android (B14) zusätzlich: die Beschriftung eines Segments darf umbrechen.
6. **Schlüsselanzeige** „…gültig bis 11.10.2036Alias: test-rsa2048“ ohne Abstand (alle Plattformen):
   `.keystore-key` (`settings.css`), Aufnahme 78. Integration: Android-Regel B16 (Name und Alias untereinander).
7. **Plugin: Dynamic Type wurde ignoriert.** Neues `ios/Sources/TextSize.swift`: Grundschrift
   `html { font-size }` = 16 px × `UIFontMetrics(.body)` (Standardgröße = 16 px, begrenzt auf 0,85–2,0), folgt
   Änderungen zur Laufzeit (`UIContentSizeCategory.didChangeNotification`), Aufnahmen 53, 55.
8. **Große Schrift: Startfläche und Karten ragten über den Rand** (54): Startspalte `minmax(0, 1fr)`, Knöpfe
   brechen auf Touch-Geräten um (`.btn`), Aufnahme 79.
9. **Querformat: Safe Area links/rechts ignoriert** – Wortmarke, Inhalt und Einstellungen lagen unter der
   Dynamic Island: Kopfzeile, Hauptbereich, Einstellungen im Vollbild, Hinweise (Toasts) und Fuß des Inspektors
   berücksichtigen `env(safe-area-inset-*)` (69–72).
10. **Ansichtswechsel übernahm den Bildlauf der Startseite:** `.app-main` ist für alle Ansichten derselbe
    Container; im Querformat (Startseite rollt) stand der Arbeitsbereich danach um 82 px verschoben und ließ sich
    nicht zurückrollen. `App.tsx` setzt den Bildlauf bei jedem Ansichtswechsel zurück (Test in `Mobile.test.tsx`).
11. **Einstellungen im Querformat:** die mitlaufende Vorschau des Signaturbilds (273 px) war höher als der
    Inhaltsbereich (193 px) – alles darunter rollte unsichtbar vorbei (71). Bei Fensterhöhe ≤ 520 px rollt sie
    mit (72).
12. **Stapel schmal: Papierkorb unsichtbar** (neu aus `main`, 63): Die Regel „Pfeil ausblenden“ blendete die
    ganze Aktionsspalte aus. Jetzt steht der Papierkorb oben rechts in der Zeile, nur der Pfeil entfällt (64).
    Integration: Android-Regel B17 (`.batch-row-action.has-caret` in der ersten Zeile rechts neben dem Namen).
13. **ID Austria ohne Netz: Fehlerkette im Alltagstext** („Proxy: error sending request: client error
    (Connect): tunnel error …“, 59): `backend-network` hat ein Attribut `.text` ohne `{ $detail }`; Karte,
    Stapelpause und Fehlerkarte zeigen es, die Ursache steht nur unter „Technische Details“ (60). CLI und Log
    behalten die vollständige Meldung (Test in `signing.test.tsx`).
14. **Zeitüberschreitung beim Verbindungsaufbau** bot „Neue Bestätigung anfordern“ an, obwohl noch keine
    Bestätigung lief (62): vor der Bestätigung (Stepper auf „Anmelden“) „Erneut versuchen“ (Test).
15. **Kopfzeile:** Befehlsknopf (Suche) 36 px breit → mindestens 44 × 44 px auf Touch-Geräten (`shell.css`);
    bei der Integration auch „+ PDF“ (neu aus `main`).
16. **Bezeichnung** der Speicherart in Einstellungen › Keystore auf Mobile „Sicherer Speicher“ statt
    „Schlüsselbund“ (wie „Im sicheren Speicher“ bei „Passwort merken?“; Android kennt keinen Schlüsselbund).

Prüfungen nach den Behebungen: `npm ci && npm run build && npm test` (25 Dateien, 595 Tests grün),
`cargo test -p pida-app -p pida-core-api -p pida-config -p pida-i18n --no-fail-fast` (404 Tests grün),
`cargo fmt --check`, `python3 tools/rename-to-pida.py --check` grün; Oberflächenänderungen zusätzlich am Desktop
mit dem Mock-Kern geprüft (Fehlerkarte „Signaturdienst nicht erreichbar“).

### 8.4 Nicht am Gerät nachweisbar, offene Punkte

17. **Teilen mehrerer PDFs** aus der Dateien-App an PIDA liefert nur das erste (24): Ohne Share-Extension übergibt
    iOS beim „Öffnen in“ ein Dokument (Plan: keine Share-Extension). Stapel auf dem iPhone über „Dokument
    auswählen“ bzw. „PDF hinzufügen“ mit Mehrfachauswahl (25, 63). Hinweis in der Hilfe oder eine Share-Extension
    wären die Optionen – Entscheidung des Auftraggebers. **Entschieden (16.10.2026): „Hinweis reicht“**, keine
    Share-Extension – umgesetzt in §8.6.
18. **„Signatur fortsetzen“ nach App-Kill** nur mit ID Austria (Wartezustand nach `prepare`); der Keystore wird
    vor `prepare` entsperrt und signiert sofort, nach einem Kill gibt es nichts fortzusetzen (50). Mit ID Austria
    braucht es A-Trust – abgedeckt durch `core-api/tests/mobile.rs`
    (`batch_on_mobile_resumes_only_the_interrupted_document`, `app_kill_offers_resume_…`) und `Mobile.test.tsx`.
19. **Vergleichswert-Kästchen der Signierkarte** (neu aus `main`) erscheinen erst nach der Anmeldung bei A-Trust –
    am Gerät nicht prüfbar; UI-Tests in `signing.test.tsx`.
20. **Links** sind 24 px hoch (WCAG 2.5.8 erfüllt); die Regel „44 px auf Touch-Geräten“ in `base.css` für `.link`
    wirkt nicht, weil `components.css` sie überschreibt. Bewusst nicht geändert: Links stehen oft im Fließtext
    (z. B. „Details“ im Datenschutz-Hinweis), 44 px würden die Zeilen auseinanderziehen.
21. **Texte:** „Überspringen – … ohne Bestätigung in der App“ erscheint auch beim Keystore (65); Speicherart
    heißt in den Einstellungen „Bis Programmende“, bei „Passwort merken?“ „Bis zum Beenden“. Behoben (§8.6).
22. Dynamic Type ist bei 200 % begrenzt; Wortmarke und Bildschirmmaße in `px` wachsen nicht mit. Lange Namen
    (Dokument, Signiermittel-Chip) werden bei großer Schrift mit „…“ gekürzt (55).
23. VoiceOver selbst wurde nicht gehört (Simulator ohne VoiceOver-Steuerung); geprüft sind Namen, Rollen und
    Struktur per Inspektion (§8.2 Nr. 8) sowie axe in den UI-Tests.
24. `general.last_source_directory` bleibt nach einem Update ein Pfad im alten Container (auf Mobile ungenutzt).
25. Bei großer Schrift rollt die waagrechte Bereichsnavigation der Einstellungen den aktiven Bereich nicht in
    den sichtbaren Teil (79: „Signiermittel“ aktiv, sichtbar sind „Signaturbild“ und „Dokument“). Behoben (§8.6).

### 8.5 Gegenprüfung (Review, 16.10.2026)

Unabhängig nachgespielt auf einem zweiten Gerät: Simulator **iPhone 17e** (390 × 844 pt, Notch statt Dynamic
Island), iOS 27.0, frisch installiert, Debug-Build des Stands `daf4679` (danach mit den Behebungen unten neu
gebaut und als **Update** installiert – neuer Container). Netz wie in §8.1 nur über Proxy `127.0.0.1:9`, keine
Zugangsdaten. Bedienung diesmal zusätzlich mit einem XCUITest-Hilfsprojekt (Tippen, Ziehen, Text tippen, Home,
Elementbaum; nur im Scratchpad) – `typeText` blendet die **Software-Tastatur** ein, damit war die
Tastatur-Überdeckung prüfbar (§8.1: „erscheint nicht“). Aufnahmen `80-…` bis `94-…` (`…-befund-review-…` vor der
Behebung).

| Ablauf | Ergebnis der Gegenprüfung |
|---|---|
| 2 – PDF empfangen, Arbeitsbereich | Teilen aus Dateien bei laufender App (80), Dokumentauswahl, Bottom-Sheet, Miniaturen, Seitenwechsel, „Ins Feld setzen“, Ziehen per Touch: ok. **Befund R1** (Block nach dem Ziehen unter dem Sheet, 81 → 82). Teilen von drei PDFs: iOS liefert nur ein `UIOpenURLAction` (System-Log) – Befund 17 bestätigt, keine Lücke in `OpenedDocuments.swift` |
| 3 – Keystore, Signieren, Fertig | Import über das Signiermittel-Menü, „Passwort merken?“ bei der ersten Verwendung, Signieren in `Signature_0`, Fertig, Teilen, „Sichern unter“ (Datei in „Auf meinem iPhone“ bytegleich), Öffnen (QuickLook): ok (91). **Befunde R2** (Tastatur verdeckt Dialog, 83 → 84/85), **R4** (Systemtexte englisch, 89 → 90), **R5** (VoiceOver liest Sandbox-Pfad). pyHanko `--trust test-rsa2048.crt.pem`: *trusted, sound, covers the entire file, VALID* (`Testdokument_signed.pdf`, auch das nach dem Update überschriebene) |
| 4 – Stapel | Mehrfachauswahl (4 PDFs), Papierkorb im schmalen Layout, „Alle signieren“ mit Keystore, Passwort, Home-Taste während der Passwortabfrage und unmittelbar nach „Signieren“ → 3 von 3 (92); nach den Behebungen neuer Stapel mit „Beide behalten“ → 2 von 2, Passwortfeld über der Tastatur (86). Kein `resume.json` übrig. pyHanko: alle 5 Dateien *VALID* |
| Update-Install | „Zuletzt signiert“ nach Umzug neu zugeordnet (`entries=4`, 93) – Befund 1 bestätigt |

Befunde der Gegenprüfung und Behebungen:

- **R1 (mittel) Arbeitsbereich: Block verschwindet nach dem Ziehen unter dem Bottom-Sheet.** Zieht man den Block
  aus einem Signaturfeld, erscheint die Leiste „Signaturfeld gefunden“ (auf dem Telefon 132 px) und schiebt die
  Seiten um ihre Höhe nach unten – der eben abgelegte Block lag dann unter dem Sheet (81; umgekehrt beim Setzen
  ins Feld). `Workspace.tsx` gleicht den Bildlauf um die Höhenänderung der Leiste aus (82, alle Plattformen;
  Test in `workspace.test.tsx`).
- **R2 (hoch) Software-Tastatur verdeckt Dialoge.** Die iOS-WebView verkleinert nur den sichtbaren Bereich; Sheets
  am unteren Rand lagen samt Feld und Knöpfen unter der Tastatur: Keystore-Passwort der Signierkarte (83, schon
  Aufnahme 16 des ersten Laufs zeigte es beim Import), Keystore-Import, Stapel-Passwort, Anmeldung bei ID Austria.
  `components.css`: Overlays auf Mobile mit Abstand `100dvh − --visual-viewport-height` (von React Aria am
  Overlay gesetzt), Dialoge höchstens so hoch wie der sichtbare Rest, oben Platz für die Statusleiste;
  `app/keyboard.ts` holt das fokussierte Feld danach im Dialog in den Blick (84–86; Test `keyboard.test.ts`).
  Ohne Tastatur, am Desktop und auf Android (Fenster schrumpft) ist der Abstand 0.
- **R3 (mittel) ID Austria: Benutzername nicht eingebbar.** „Telefonnummer oder Benutzername“ (Signierkarte und
  Einstellungen) hatte `inputMode="tel"` – die Telefon-Tastatur von iOS hat keine Buchstaben (87). Jetzt
  Standardtastatur mit `autocapitalize="none"` (88; Tests in `signing.test.tsx`, `settings.test.tsx`).
- **R4 (mittel) Systemoberfläche in der App englisch** (Dokumentauswahl „Cancel“, Teilen „Copy/Markup/Print“, 89):
  `Info.plist` ohne `CFBundleLocalizations`, die App galt als nur englisch. Jetzt `[de, en]` in `Info.plist` und
  `project.yml` → „Abbrechen“, „Kopieren/Markierung/Drucken“ (90). `Info.plist` von Hand ergänzt: `xcodegen
  generate` schriebe `project.pbxproj` und das Schema vollständig um.
- **R5 (niedrig) VoiceOver las im Fertig-Schritt den Sandbox-Pfad** (`title` mit dem vollständigen Pfad als
  Bezeichnung statt „Gespeichert in den Dokumenten dieser App“): auf Mobile ohne `title` (Test in `done.test.tsx`).
- **R6 (niedrig) Konto-Chip im Fuß des Arbeitsbereichs 38 px hoch** (`.signer-chip.is-compact` setzte 36 px nach
  der Touch-Regel in `base.css`) – entgegen §8.2 Nr. 8 „Knöpfe ≥ 44 px“. Touch-Regel in `components.css`.

Nicht behoben (niedrig, plattformübergreifend bzw. Textfragen):

- R7: Liegt auf der letzten Seite ein seitenfüllendes Signaturfeld (`Testdokument.pdf`, `Signature_7`), meldet
  die Sprechblase „automatisch auf freier Fläche platziert“, zugleich warnt das Sheet „überdeckt ein Formular-
  oder Signaturfeld“; „Freie Fläche suchen“ wechselt nur zwischen den Warnungen (80, 81).
- R8: Auf dem Telefon liegt ein Hinweis (Toast) über dem unteren Rand eines Dialogs, z. B. „überdeckt Inhalt“
  über der Überschreiben-Frage (94).
- R9: Stapel mit Keystore: „Bestätigung 1 von 3“/„Bestätigungen“ und „Alle … einmal anmelden, dann jedes
  Dokument einzeln bestätigen“ sind ID-Austria-Wortlaut; „Einrichtung abgeschlossen“ nach „Später einrichten“.
- R10: Nach der Wahl einer Miniatur springt der Fokus sichtbar auf den Umschalter, sein Tooltip verdeckt die
  Wortmarke.
- `main` ist seit dem Merge um fünf Commits weiter (u. a. Kopfzeile mit „+ PDF“ und Haus-Symbol, Schlüsselliste
  ohne Schlüsselbund-Zugriff) – vor der Übernahme mergen und Abläufe 2–4 kurz wiederholen.

Prüfungen danach: `npm run build`, `npm test` (26 Dateien, 598 Tests grün; die neuen Tests scheitern ohne die
Behebungen), `python3 tools/rename-to-pida.py --check`; `cargo test -p pida-config -p pida-core-api -p pida-i18n
-p pida-app` (404 Tests grün, Stand `daf4679`; die Behebungen ändern keinen Rust-Code).

### 8.6 Nachtrag: Randfälle und Hinweis zum Teilen (ADR 0004 Nachtrag 12)

Behoben auf allen Plattformen (Kern bzw. Oberfläche, Tests siehe ADR 0004 Nachtrag 12): R7 (die Automatik zählt
die Widgets vorhandener Felder als belegt – `Testdokument.pdf` landet auf einer neuen Seite statt im seitenfüllenden
`Signature_7`), R8 (Hinweise stehen oben, solange ein Bottom-Sheet unten steht), R9 (Stapelwortlaut je Signiermittel;
nach „Später einrichten“ „Signiermittel und Signaturbild können Sie jederzeit in den Einstellungen einrichten.“),
R10 (nach Tippen auf eine Miniatur kein Fokusrahmen und kein Tooltip), Nr. 21 („Bis zum Beenden“ überall) und Nr. 25
(die Bereichsleiste rollt den aktiven Bereich ins Bild). Dazu aus der Android-Prüfung: Toast nach „Sichern unter“
mit dem tatsächlichen Namen (iOS: `lastPathComponent` der exportierten Kopie), Live-Region nach Dokumentwechsel,
fortgesetzte Stapelzeile mit Miniatur und Seitenzahl, `qr.pdf` nach nachträglich eingeschalteter Suche im
Platzhalter (Seite 1 / 1) und dieselbe Datei erneut empfangen → kein zweiter Eintrag, Hinweis „„qr.pdf“ ist bereits
geöffnet.“

**Teilen mehrerer PDFs (Nr. 17, Entscheidung „Hinweis reicht“).** Die Shell reicht über „Teilen“ bzw. „Öffnen in“
empfangene Dateien als `files_received` an den Kern (`Shell::open_files`; wie `open_files` ohne `mode`). Auf iOS zeigt
der Arbeitsbereich danach über „Signieren“ – auch im eingeklappten Bottom-Sheet – einen kurzen, nicht blockierenden
Hinweis:

> Mehrere PDFs geteilt? iOS übergibt beim Teilen nur das erste. Für einen Stapel wählen Sie die übrigen über „Weitere
> PDFs hinzufügen …“ – dort ist Mehrfachauswahl möglich.

mit dem Knopf „Weitere PDFs hinzufügen …“ (System-Dokumentauswahl mit Mehrfachauswahl, `open_files { mode: add }`) und
„Hinweis ausblenden“ (×). Er erscheint nur nach dem Empfang über „Teilen“ (nicht beim Start, nicht nach der
Dokumentauswahl), nur solange das empfangene Dokument allein offen ist (mit weiteren PDFs ist er erledigt) und nach
dem Ausblenden bis zum Beenden der App nicht mehr. Android empfängt mehrere PDFs (`SEND_MULTIPLE`) und zeigt ihn nicht.
Englisch: „Shared several PDFs? iOS only passes on the first one. For a batch, choose the others with “Add more
PDFs …” – you can select several there.“ Tests: `core-api/tests/mobile.rs`
(`ios_shows_the_share_hint_only_after_receiving_a_document`), `Mobile.test.tsx` („Hinweis zum Teilen auf iOS“). Am
Gerät noch nicht nachgespielt (nur `cargo check` für `aarch64-apple-ios-sim`).

## 9. Mobile nur mit ID Austria (16.10.2026)

Entscheidung des Auftraggebers: „Signieren mit Keystore brauchen wir auf mobilen Apps nicht, da reicht ID Austria.“
Umsetzung und Nachweise für beide Plattformen: mobile.md §12, ADR 0004 Nachtrag 11. Für iOS bedeutet das:

- **Entfallen:** Abläufe mit Keystore (§5 I-06 „PKCS#12“-Filter, I-08 „Keystore wählbar“, I-09, I-10; §8.2 Abläufe 3
  und 4 mit Keystore; §8.3 Nr. 16 „Sicherer Speicher“, Nr. 18 Keystore nach App-Kill, Nr. 21 Texte beim Keystore).
  Die Dateiauswahl der Shell fragt nur noch PDFs an (`mobile_pick_files` lehnt `pkcs12` ab); das Plugin behält
  `importFile` mit `pkcs12` unverändert (keine native Änderung).
- **Bleibt:** Signieren mit ID Austria (App-Bestätigung/SMS-TAN, FIDO2 ausgegraut), Stapel, „Signatur fortsetzen“,
  Teilen, „Sichern unter“, QuickLook. Der Sicherungsausschluss von `Library/Application Support/…/data/keystores`
  (R-5) bleibt für Dateien früherer Builds.
- **Übergang früherer Builds:** gespeicherter Standard Keystore → ID Austria (beim Lesen, `[keystore]` bleibt in
  `settings.toml`), Keystore-Passwörter in der Keychain (Dienst „PIDA“, Konten `keystore-store:`/`keystore-key:` zur
  Datei aus den Einstellungen und zu jeder Datei in `data/keystores`) werden beim ersten Start einmalig entfernt
  (Merker `data/keystore-secrets-removed`); ein gesicherter Keystore-Vorgang für „Signatur fortsetzen“ wird mit Hinweis
  verworfen. Die Datei `data/keystores/*.p12` selbst bleibt liegen (durch ihr Passwort geschützt).
- **Am Simulator:** am 17.10.2026 auf dem iPhone 17e nachgeholt, samt Update eines Builds mit Keystore als Standard (§10).

## 10. Gerätenachweis „Mobile nur ID Austria“ und Randfälle (17.10.2026)

Branch `mob2/ios` (ab `main` 8606b0c mit dem Integrationsstand `826fe82`: Mobile nur ID Austria, Randfälle aus ADR 0004
Nachtrag 12, „+ PDF“, „In Bearbeitung“/Parken, automatische Positionierung). Gerät: Simulator **iPhone 17e** (390 × 844
pt, Notch), iOS 27.0, Sprache de, Debug-Build nach §4/§8.1. Netz nur über Proxy `127.0.0.1:9` (abgelehnt) bzw. für die
Warteansicht ein lokales „schwarzes Loch“ auf `127.0.0.1:9999` (nimmt an, antwortet nie, leitet nichts weiter) – keine
Verbindung zu A-Trust, keine Zugangsdaten (ein Platzhalter-Benutzername für die Tastaturprobe wurde wieder gelöscht und
nie gespeichert). Bedienung wie §8.5: Web-Inspektor (der Socket wird jetzt über die PID des `launchd_sim` genau dieses
Simulators gewählt, nicht „der erste“ – zwei weitere Simulatoren liefen) und das XCUITest-Hilfsprojekt (Tippen, Ziehen,
Tastatur, Drehen). Prüfmodus nicht geprüft (laut Entscheidung vom 17.10.2026 entfernt; am Ende mit `main` gemergt, §10.5).
Aufnahmen `docs/integration/screenshots/ios-ng2/00-…` bis `81-…` (540 px), `…-befund-…` vor, `…-nachher-…` nach der
Behebung, `…-merge-…` mit dem gemergten `main`.

### 10.1 Ergebnisse je Ablauf

| Nr. | Ablauf | Aufnahmen | Ergebnis |
|---|---|---|---|
| 1 | Frische Installation (vorher deinstalliert), Ersteinrichtung | 20, 21 | ok: „Schritt 1 von 1“, nur Signaturbild (Profil, Sprache), kein Signiermittel-Schritt; „Später einrichten“ → Hinweis „Signiermittel und Signaturbild können Sie jederzeit in den Einstellungen einrichten.“, `first_run_completed = true` |
| 2 | Update-Fall: der Review-Build vom 16.10. (Version 1.0.0, importierter `test-rsa2048.p12`) mit von Hand ergänztem `[backend] default = "keystore"` – vorher Keystore als Standard (00, 01) –, dann der neue Build als Update (neuer Container) | 00–09 | ok: Start ohne Rückfrage, Log „gespeichertes Standard-Signiermittel auf ID Austria abgebildet“, „Zuletzt signiert“ umgezogen (`entries=6`), „Keystore-Passwörter eines früheren Builds entfernt removed=0“ und Merker `data/keystore-secrets-removed`; `settings.toml` byte-gleich (Standard und `[keystore]` bleiben stehen), `data/keystores/test-rsa2048.p12` bleibt. Einstellungen › Signiermittel nur ID Austria und FIDO2-Hinweis (03), Konto-Chip „ID Austria“, Menü nur „Einstellungen …“ (08), Signierkarte „Signieren mit ID Austria“ (09). Die Keychain des 17e enthielt keine PIDA-Einträge (der Review-Build hatte „Nicht speichern“), die Bereinigung selbst belegen die Kerntests |
| 3 | Suche nach „Keystore“, „Zertifikatsdatei“, „Passwort merken“ & Co. | 04, 06, 12–18 | ok: Textinhalt (auch verborgener) und Attribute (`aria-label`, `title`, `placeholder`, `alt`) jeder Ansicht ohne Treffer für Keystore, Zertifikatsdatei, PKCS, `.p12`, Passwort merken, Schlüsselbund, Sicherer Speicher, Datei mit Zertifikat, Schlüssel anzeigen, Alias und die beiden Wörter, die die Oberfläche nie verwendet (Begriffsregeln) – Start, alle acht Einstellungsbereiche, Einstellungssuche (Keystore, Zertifikatsdatei, PKCS, p12, Passwort merken, Passwort, Alias, Schlüsselbund, Sicherer Speicher: „Keine Treffer“; „Schlüssel“ findet nur den FIDO2-Hinweis), Befehle (Keystore: „Keine Treffer“), Hilfe (Tastenkürzel), Arbeitsbereich, Konto-Chip-Menü, Signierkarte, Fehlerkarte, Stapel, Startdialog, Stapelkarte. Die Dokumentauswahl bietet `test-rsa2048.p12` ausgegraut an (06) |
| 4 | PDF empfangen („Teilen“ aus Dateien), Arbeitsbereich, automatische Positionierung, „Diesmal selbst platzieren“, Stapel-Option | 22–32, 34, 38–40 | ok: Hinweis zum Teilen (§8.6) erstmals am Gerät – über „Signieren“, mit „Weitere PDFs hinzufügen …“, nach × bis zum Beenden weg (23, 30). Einstellungen › Dokument „Automatische Positionierung“ an → „Automatisch (laut Einstellung) · Bereit zum Signieren“, „Diesmal selbst platzieren“ gibt den Block frei (Seite 3 · oben mittig), Ziehen per Touch (31, 32). Stapel: mit Einstellung „Automatische Positionierung (laut Einstellung)“ (34), ohne Einstellung der Schalter „Für diesen Stapel automatisch positionieren“ mit Rückfrage für den selbst platzierten Eintrag, „Behalten“ (39, 40); bleibt nach Parken/Fortsetzen an. Befund N7 |
| 5 | „+ PDF“ und Papierkorb | 10, 11, 33–37, 51, 52 | ok: „Weitere PDFs hinzufügen …“ (Mehrfachauswahl, „Zusammen mit … als Stapel signieren“) und „Anderes PDF öffnen …“ („… wird geschlossen“); „PDF hinzufügen“ im Stapel; dieselbe Datei erneut → „„Stapel-1.pdf“ ist bereits geöffnet.“ (35; nach dem Merge Text aus Nachtrag 17, 81); Papierkorb entfernt den Eintrag ohne Rückfrage (36, 37) |
| 6 | „Zur Startseite“ → „In Bearbeitung“ | 41–47 | ok: ohne Rückfrage geparkt, Karte „Stapel · 3 Dokumente“, „Weiter bearbeiten“ unverändert (auch die Stapel-Option); per „Teilen“ empfangenes PDF kommt ohne Rückfrage dazu (Nachtrag 10 Punkt 4, 43); „Dokument auswählen“ bei geparkter Arbeit → „Zur offenen Arbeit hinzufügen?“ mit „Zum Stapel hinzufügen“ (44, 45); „Schließen“ der Karte fragt bei mehreren Dokumenten nach (46, 47) |
| 7 | Randfälle (Nachtrag 12) | 35, 48–52, 54, 13/14 | ok: Hinweis oben, solange das Sheet ausgeklappt ist (50); Live-Region nach „Anderes PDF öffnen …“ ohne die Lage des vorigen Dokuments (52); `qr.pdf` (PDF-AS-Testdatei) erst „Seite 1 / 2“ (neue Seite), nach nachträglich eingeschalteter Platzhaltersuche „Seite 1 / 1“ im QR-Platzhalter (48, 49), erneut empfangen → „„qr.pdf“ ist bereits geöffnet.“, kein zweiter Eintrag (50); Bereichsleiste bei großer Schrift mit dem aktiven Bereich im Bild (54); Stapeltexte für ID Austria („einmal anmelden, dann jedes Dokument einzeln bestätigen“, „1 Anmeldung · 4 Bestätigungen“, „Überspringen – ohne Bestätigung in der App“, 12, 13). „Sichern unter“-Hinweis siehe 10.3. Befunde N1, N6, N8 |
| 8 | ID Austria ohne Netz, Tastatur, dunkel, Querformat, große Schrift | 09, 14, 53–64, 74, 75 | Fehlerkarte „Der Signaturdienst ist gerade nicht erreichbar“ (09), über das schwarze Loch „Verbindung wird aufgebaut …“ (62) und nach der Zeitüberschreitung „Die Zeit ist abgelaufen“ mit „Erneut versuchen“ (63). Benutzername-Feld mit Buchstaben-Tastatur, kleingeschrieben, `autocomplete=username` (53); Befehle-Suche über der Tastatur (16). Dunkel (59–61), Querformat ohne Inhalt unter der Notch (61, 74), Dynamic Type 200 % ohne waagrechten Überlauf der Seite. Befunde N1–N5 |

### 10.2 Befunde und Behebungen (alle Plattformen, nur Oberfläche; Vertrag unberührt)

- **N1 (mittel) Knöpfe der Signierkarte unter dem Rand.** Ist der Inhalt höher als das Sheet, rollt die Karte in sich –
  „Erneut versuchen“ der Stapelpause ohne Netz lag halb unter dem Rand (14), bei großer Schrift und im Querformat jede
  Aktion (56). Auf Mobile bleibt der Fuß (`.step-actions`, nicht bei den zentrierten Kurzansichten) jetzt mit Trennlinie
  am unteren Rand stehen (`signing.css`; 66, 69, 74, 75); mit eingeblendeter Bildschirmtastatur rollt er wie bisher mit,
  damit das fokussierte Feld nicht darunter liegt.
- **N2 (mittel) Große Schrift: Stapelkarte breiter als das Telefon.** `.sc-batch-foot .details { min-width: 12rem }` =
  384 px bei 32 px Grundschrift – die ganze Stapelkarte ragte 28 px hinaus, „0 von 2“ abgeschnitten (58). Jetzt
  `min(12rem, 100%)`; dazu brechen Auslöser wie „Alle Dokumente des Stapels“ linksbündig statt zentriert um (69).
- **N3 (mittel) Große Schrift: Zoomwert „41 %“ brach um** und ragte aus der 48 px hohen Werkzeugleiste (55) – `nowrap` (70).
- **N4 (hoch) Bildschirmtastatur verdeckte den „Hinweistext“.** In Einstellungen › Signaturbild blieb die mitlaufende
  Vorschau stehen; im kleinen sichtbaren Rest über der Tastatur lag das fokussierte Feld darunter (64, `elementFromPoint`
  = Vorschaubild). Das Fenster bleibt unter iOS gleich hoch (die Regel `max-height: 520px` aus §8.3 Nr. 11 greift nicht).
  `app/keyboard.ts` setzt jetzt `data-keyboard` am Wurzelelement, solange der sichtbare Bereich mindestens 120 pt
  kleiner als das Fenster ist (nicht beim Zoomen, nicht für die Zubehörleiste allein); die Vorschau rollt dann mit (72).
- **N5 (mittel) Kopfzeile eines aus dem Stapel geöffneten Dokuments:** Wortmarke, Haus, „Zurück zum Stapel“, Dokument,
  „+ PDF“ und die Aktionen brauchen mehr als 390 px – der Kontext schrumpfte, „+ PDF“ lag 20 px über „Prüfen“
  (Fingerziele überlappten, bei jeder Schriftgröße, 71). Bis 480 px Breite zeigt die Wortmarke dann nur das Logo (Name
  bleibt „PIDA – Startseite“, 73). Mit dem gemergten `main` fehlt „Prüfen“; ohne die Regel blieben auf 360-pt-Telefonen
  nur 2 px Luft – sie bleibt deshalb (78).
- **N6 (niedrig) Datenschutz-Kurzhinweis im Stapel in der Einzahl** („Das Dokument wird …“ bei 4 Dokumenten, 13) – in
  Übersicht, Startdialog und Stapelkarte jetzt „Die Dokumente werden zum Signieren an A-Trust übermittelt.“
  (`ng-ui-backend-leaves-device-many`, 65).
- **N7 (niedrig) Rückfrage „Eigene Platzierungen behalten?“ bei einem Dokument** mit Knopf „Auch diese automatisch
  platzieren“ zum Text „Ein Dokument … Soll es …“ (39) – Titel und Knopf folgen der Zahl („Eigene Platzierung
  behalten?“, „Auch dieses automatisch platzieren“, 68).
- **N8 (niedrig) Stapelabbruch ohne signiertes Dokument:** „Stapel abgebrochen – 0 Dokumente wurden bereits signiert und
  gespeichert.“ (60) → „Stapel abgebrochen – es wurde noch kein Dokument signiert.“ (`ng-batch-aborted` `[0]`, 67).

Tests (scheitern ohne die Behebungen): `styles/mobile-css.test.ts` (N1–N5: Regeln und Reihenfolge), `app/keyboard.test.ts`
(`data-keyboard` an/aus, Zoom, Zubehörleiste), `features/batch/batch.test.tsx` (N6 in Übersicht, Startdialog und Karte;
N7 Einzahl; N8 Abbruch ohne Signatur).

### 10.3 Nicht behoben bzw. nicht am Gerät nachweisbar

- **Datenschutzinformation** (Einstellungen › Datenschutz, Text aus PDF-Over) nennt auch auf Mobile „Der Signaturvorgang
  mit der Chipkarte wird lokal durchgeführt.“ (05) – Bereich des parallelen Laufs „Online-Prüfung/Datenschutz“, nicht
  geändert; Vorschlag: den Satz auf Mobilgeräten weglassen.
- Die Beschreibung des Signaturblocks für Bildschirmleser nennt auch auf dem Telefon Tastenkürzel („⌘+↵ signiert“,
  Pfeiltasten) – mit Hardware-Tastatur (iPad) zutreffend, nicht geändert.
- Stapel-Option ohne Einstellung: Einträge ohne eigenen Plan stehen schon vor dem Einschalten auf „Bereit · automatisch
  platziert“ (12, 38; Kern: ohne Plan nach PDF-AS-Regel beim Signieren) – der Schalter ändert sichtbar nur Einträge mit
  Vorschau-Plan. Wie Nachtrag 8 vorgesehen; ob der Hinweistext das klarer sagen soll, entscheidet der Auftraggeber.
- Der Hinweis „Signaturblock automatisch … platziert“ ist bei großer Schrift oben unter der Werkzeugleiste angeschnitten
  (kurzlebig, 70).
- Eine frische Installation legt den leeren Ordner `data/keystores` an (Sicherungsausschluss des Plugins, R-5) und
  schreibt `[keystore]` mit Standardwerten in `settings.toml` – unsichtbar und ohne Wirkung.
- Ohne A-Trust bzw. ohne Signatur nicht am Gerät: „Fertig“, „Sichern unter“-Hinweis mit tatsächlichem Namen, Anmelde-
  und TAN-Maske der Signierkarte mit Tastatur, Vergleichswert; belegt durch Kern- und Oberflächentests
  (`signing.test.tsx`, `done.test.tsx`, `core-api/tests/mobile.rs`).

### 10.4 Prüfungen (vor dem Merge mit `main`)

`cargo test --workspace --no-fail-fast` 1445 bestanden, 0 fehlgeschlagen, 5 ignoriert; `cargo clippy --workspace
--all-targets -- -D warnings` und `cargo fmt --all --check` ohne Befund; `npm ci && npm run build && npm test` 33 Dateien,
726 Tests; `ts_bindings` und `serde_contract` sowie die Real-Core-Proben (`workspace`, `batch`, `verify`) mit
`PIDA_BLESS=1` unverändert (Vertrag unberührt); `python3 tools/rename-to-pida.py --check` ohne Befund; `cargo check -p
pida-app` für `aarch64-apple-ios-sim` und `aarch64-linux-android` (NDK, eigenes `CARGO_TARGET_DIR`) grün. Simulator
iPhone 17e danach heruntergefahren (`xcrun simctl shutdown`), kein anderer Simulator angefasst.

### 10.5 Merge mit `main` (Prüfmodus entfernt, ADR 0004 Nachträge 13–17)

Während des Laufs landete auf `main` die Entfernung des Prüfmodus (Nachtrag 15) zusammen mit Nachtrag 13 („Weiter
bearbeiten“ eines Stapels), 16 (Einrasten in Signaturfelder) und 17 (Mehrfachauswahl). `git merge main` mit 22
Konfliktdateien, aufgelöst nach ADR 0004 Nachtrag 18: Prüfmodus vollständig entfernt (auch die Prüfmodus-Sperren aus
dem Mobile-Stand); Vertrag additiv vereint (TypeScript, `view-states.json`, `ui-events.json` und die Real-Core-Proben
neu erzeugt); Duplikate nach Nachtrag 17 (`ng-open-duplicates`), die eigene Erkennung aus Nachtrag 12 samt
`ng-open-duplicate` entfällt, die Ausnahme für eben signierte Dokumente bleibt für Kopien auf Mobilgeräten (neuer Test
`mobile.rs::shared_copy_opens_again_after_signing_on_mobile`, scheitert ohne die Regel); `files_received` (Hinweis zum
Teilen) über dieselbe Auswahl; `mobile_pick_files` nur `pdf`, Ergebnis `{ paths, skipped }`.

Kurzprobe am iPhone 17e mit dem gemergten Stand (Update-Install, ohne Netz zu A-Trust): Start ohne Prüfmodus und ohne
Keystore-Begriffe (76), Stapel mit Datenschutzhinweis in der Mehrzahl (77), Kopfzeile eines Stapeleintrags ohne
Überlappung (78), Stapelpause mit stehenden Knöpfen (79), Abbruch ohne Signatur (80), per „Teilen“ empfangene Kopie
eines offenen Dokuments → „„Stapel-2.pdf“ ist bereits geöffnet und wurde nicht noch einmal hinzugefügt.“ (81). Danach
heruntergefahren.


Prüfungen nach dem Merge: `cargo test --workspace --no-fail-fast` 1483 bestanden, 0 fehlgeschlagen, 5 ignoriert; `cargo
clippy --workspace --all-targets -- -D warnings` und `cargo fmt --all --check` ohne Befund; `npm ci && npm run build &&
npm test` 34 Dateien, 708 Tests; `ts_bindings`, `serde_contract` und die Real-Core-Proben (`workspace`, `batch`,
`first_run`) mit `PIDA_BLESS=1` ohne weitere Änderung; `python3 tools/rename-to-pida.py --check` ohne Befund; `cargo
check -p pida-app` für `aarch64-apple-ios-sim` und `aarch64-linux-android` grün.

## 11. TestFlight (seit 0.6.0, 28.10.2026)

- **Team und App:** A-SIT-Team `9CYHJNG644`, App „PIDA“ (`at.asit.pida.app`) in App Store Connect; nur interne Tests
  (Gruppe „PIDA interne Tests“, automatische Verteilung). Keine Einreichung zur App-Store-Prüfung ohne ausdrückliche
  Freigabe.
- **Bauen und signieren:** `npx tauri ios build --target aarch64 --export-method app-store-connect` mit
  `APPLE_DEVELOPMENT_TEAM=9CYHJNG644` erzeugt das Archiv; der Export scheitert dort an der Cloud-Signatur (der
  API-Schlüssel mit Rolle App Manager darf die in der Cloud verwalteten Vertriebszertifikate nicht nutzen). Exportiert
  wird daher über das A-SIT-Konto in Xcode: `xcodebuild -exportArchive -allowProvisioningUpdates` mit
  `method = app-store-connect`, `teamID = 9CYHJNG644`, `signingStyle = automatic`.
- **Hochladen:** `tools/testflight-upload.py <ipa>` – Upload mit `xcrun altool` und dem Teamschlüssel aus
  `~/.pida-release/appstore.env`; nach der Verarbeitung schaltet das Skript „Tester automatisch benachrichtigen“ aus
  und gibt den Build der internen Gruppe (keine E-Mail/Push je Testbuild; `--notify` lässt sie an). Dafür ist in der
  Gruppe „PIDA interne Tests“ die **automatische Verteilung aus** (nur in App Store Connect umstellbar, nicht per
  API) – sonst verteilt Apple schon während der Verarbeitung und benachrichtigt. `--check` zeigt Builds und Gruppen.
- **Exportkontrolle:** `ITSAppUsesNonExemptEncryption = false` (Info.plist) – keine Frage je Build.
