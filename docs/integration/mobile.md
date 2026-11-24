# Mobile (iOS/Android) – Fundament M0 und Aufgaben für M-iOS/M-Android

Stand: 15.10.2026, Welle Mobile, Paket **M0** (plattformneutrales Fundament); ergänzt am 16.10.2026: **Mobile
nur mit ID Austria** – kein Keystore auf iOS/Android (Entscheidung des Auftraggebers, §12, ADR 0004 Nachtrag 11);
am 17.10.2026: **keine Signiermittel-Auswahl** – kein Konto-Chip (§13, ADR 0004 Nachtrag 20).
Grundlage: Plan
§5 (Plattform-Matrix, §5.2 Mobile 1.0, §5.3 Plattformspezifika), §3.5 (Lebenszyklus), §4.7 (FIDO2),
§7.7 (Sicherheit); ADR 0001; docs/analyse/02 (A-Trust auf Mobile: kein FIDO2, App-Wechsel).

Mobile 1.0 (Plan §5.2): ID Austria nativ (Szenario A: Anmeldung, SMS-TAN, Bestätigung in der App
„ID Austria“ mit App-Wechsel; **kein FIDO2** – ausgegraut mit Hinweis), **kein Keystore** (der ursprünglich
vorgesehene PKCS#12-Import in die App-Sandbox entfällt seit 16.10.2026, §12), **keine lokale BKU, keine CLI**. Dokumente über Share-Sheet/„Öffnen in“ (iOS) bzw.
Intents `VIEW`/`SEND` `application/pdf` (Android) und die System-Dateiauswahl; Ausgabe in die
App-Dokumente plus Teilen/„Sichern unter“; Positionieren per Touch, die Zahlenfelder bleiben.

M0 liefert alles, was **beide** Plattformen brauchen (Kern, Shell, Plugin-Rust-Seite, Oberfläche,
PDFium-Pakete); die nativen Seiten des Plugins sind **leere Hüllen**. M-iOS und M-Android arbeiten danach
parallel in diesem Worktree (Zuständigkeiten siehe §9).

## 1. Architektur

```
React-Oberfläche (apps/app/src)            ── ViewState / UiEvent (Tauri-IPC, unverändert) ──┐
  isMobile(view): Teilen/Exportieren, nur ID Austria, „Signatur fortsetzen“, FIDO2-Hinweis       │
                                                                                                ▼
Tauri-Shell (apps/app/src-tauri, Crate pida-app)                                pida-core-api (Kern)
  cfg(mobile): Sandbox-Pfade, Lebenszyklus, Dokument-Eingang,  ── CoreOptions ──▶  Workflow-Reducer (State::Resuming)
  mobile_pick_files, ShellHooks share/export, SecretStore                         resume.rs: Prepared::persist/restore
        │ Rust-API (keine WebView-Befehle)                                          Long-Poll nach Rückkehr, Backend-Liste
        ▼
Plugin tauri-plugin-pida-mobile (apps/app/plugins/pida-mobile)
  src/ (fertig): PidaMobile, Fehlercodes, Pfadprüfung, tls.rs (Android: rustls-platform-verifier)
  ios/Sources/PidaMobilePlugin.swift      ← M-iOS füllt (Keychain, Share, Document Picker, Inbox)
  android/src/main/java/…/PidaMobilePlugin.kt ← M-Android füllt (Keystore, ACTION_SEND, SAF, Intents)
```

Die Oberfläche hält keinen Fachzustand (Plan §3.1). Plattform kennt sie aus `ViewState.platform`
(`{ os, mobile }`, vom Kern aus `CoreConfig::platform`).

## 2. Plugin `pida-mobile`

Crate `tauri-plugin-pida-mobile`, Tauri-Name `pida-mobile`, registriert auf **allen** Plattformen
(Desktop: ohne native Seite, jede Methode liefert `Error::Unsupported`). Das Plugin hat **keine Befehle
für die WebView** (`COMMANDS = []`, keine Berechtigungen): der sichere Speicher darf aus der Oberfläche
nicht erreichbar sein. Nur die Rust-Shell ruft es auf – nie vom Hauptthread (iOS führt die Methoden dort
aus; die Shell nutzt Hintergrund-Threads, der sichere Speicher zusätzlich eine Frist von 5 s).

Rust-API (`PidaMobileExt::pida_mobile()` auf `AppHandle`):

| Methode (Rust) | nativ | Argumente (JSON, camelCase) | Antwort | Umsetzung iOS / Android |
|---|---|---|---|---|
| `secure_store_get(key)` | `secureStoreGet` | `{ key }` | `{ value: string \| null }` | Keychain `kSecClassGenericPassword`, `kSecAttrAccessibleWhenUnlockedThisDeviceOnly`, nicht synchronisiert / AES-256-GCM-Schlüssel im Android Keystore, verschlüsselte Werte in privaten SharedPreferences (ohne Sicherung) |
| `secure_store_set(key, value)` | `secureStoreSet` | `{ key, value }` | `{}` | ersetzt vorhandene Werte |
| `secure_store_delete(key)` | `secureStoreDelete` | `{ key }` | `{}` | fehlender Eintrag ist kein Fehler |
| `share_file(path, mime)` | `shareFile` | `{ path, mimeType }` | `{}` sobald gezeigt | `UIActivityViewController` / `ACTION_SEND` mit `FileProvider`-URI + `FLAG_GRANT_READ_URI_PERMISSION` |
| `open_file(path, mime)` | `openFile` | `{ path, mimeType }` | `{}` sobald die Anzeige startet | QuickLook (`QLPreviewController`, ohne Bearbeiten) / `ACTION_VIEW` mit `FileProvider`-URI an eine fremde PDF-App – Ergebnis „Öffnen“ (`ShellHooks::open_path`; nur `not-implemented` fällt auf `opener` zurück) |
| `export_file(path, name, mime)` → `bool` | `exportFile` | `{ path, fileName, mimeType }` | `{ saved }` | `UIDocumentPickerViewController(forExporting:asCopy:)` / SAF `ACTION_CREATE_DOCUMENT` + Kopie |
| `import_file(kind, multiple, dest)` → Dateien (`import_files` mit Zahl der nicht übernommenen) | `importFile` | `{ kind: "pdf"\|"pkcs12", multiple, destination }` | `{ files: [{ name, path }], skipped }` (Reihenfolge der Auswahl; `skipped` = gewählt, aber nicht kopiert, ADR 0004 Nachtrag 17; die Shell fragt seit §12 nur noch `pdf` an) | `UIDocumentPickerViewController(forOpeningContentTypes:asCopy:true)` / SAF `ACTION_OPEN_DOCUMENT`; **Bytes** nach `destination` kopieren |
| `take_pending_open_documents(urls, dest)` → Dateien | `takePendingOpenDocuments` | `{ urls: [string], destination }` | `{ files: [{ name, path }] }` | vorgemerkte Dokumente („Öffnen in“/Share bzw. Intents `VIEW`/`SEND`/`SEND_MULTIPLE`) **und** die übergebenen URLs (Security-Scoped `file://`, `content://`) nach `destination` kopieren, Warteschlange leeren |

Fehler: `invoke.reject(message, code)` – `not-implemented` (Hülle), `cancelled` (Nutzerin hat
abgebrochen; Export/Import liefern dann `false` bzw. `[]`), sonst ein kurzer eigener Code. Die Nachricht
wird verworfen (kann Pfade enthalten); Rust kennt `ng-mobile-unsupported`, `ng-mobile-not-implemented`,
`ng-mobile-cancelled`, `ng-mobile-timeout`, `ng-mobile-native` (übersetzt in `pida.ftl`).
Zurückgegebene Pfade müssen **innerhalb von `destination`** liegen und Dateien sein – andere verwirft die
Rust-Seite (`within`, Schutz gegen fehlerhafte native Seiten).

Tests der Rust-Seite: `cargo test -p tauri-plugin-pida-mobile` (Vertrag mit einer Attrappe der
nativen Seite: Argumente, Antworten, Abbruch, Pfadprüfung, Fehlercodes).

## 3. Lebenszyklus (Plan §3.5)

- **Ereignisse:** `WindowEvent::Suspended` (iOS `applicationWillResignActive`, Android `onPause`) →
  `UiEvent::Backgrounded`; `WindowEvent::Resumed` (iOS `applicationWillEnterForeground`, Android
  `onResume`) → `UiEvent::Foregrounded` (`apps/app/src-tauri/src/lib.rs`, `mobile::lifecycle_event`).
- **Kein Abbruch:** Im Reducer ändern `Backgrounded`/`Foregrounded` weder Zustand noch Effekte (Tabellentest
  in jedem Zustand, Property-Test `reducer_is_total_and_keeps_queue_consistent`). Auf Mobile gehen
  `CloseRequested`/`ExitRequested` **nicht** als `Quit` an den Kern (das hätte den Vorgang abgebrochen und den
  persistierten Zustand gelöscht).
- **Long-Poll nach Rückkehr:** Nach `Foregrounded` (nur wenn vorher `Backgrounded`) beantwortet der Kern eine
  offene Maske `WaitForApp`/`QrCode` mit „weiter“ (`CoreBroker::restart_long_poll`); das A-Trust-Backend lädt
  die Seite neu (R7) und startet einen frischen Long-Poll – die alte Verbindung hat das System im
  Hintergrund oft getrennt (sonst erst nach 10-s-Backoff bzw. 300-s-Frist). Ist die Bestätigung inzwischen
  angekommen, geht es sofort weiter. `backend-atrust` ist dafür unverändert.
- **App-Kill → „Signatur fortsetzen“:** Mit `CoreOptions::persist_prepared` (Standard auf iOS/Android)
  sichert der Kern **nach `prepare`** in `Paths::temp_dir/resume/` (App-Sandbox, nicht der Cache;
  Ordner 0700, Dateien 0600): `resume.json` (Name, Signiermittel, Position, wartende Dokumente,
  Zeitpunkt, SHA-256), `document/<Originalname>` (Kopie des Originals) und
  `pades::Prepared::persist` (Platzhalter-PDF + Begleitdatei). Beim nächsten Start bietet der Kern
  im Schritt `open` `ViewState.resume` an (höchstens 60 min alt, sonst Hinweis `ng-resume-expired`).
  `resume_signature` → `Event::ResumeRequested` → `State::Resuming`: Dokument laden, **ohne**
  Platzhalterfrage/Positionierung, gespeichertes Signiermittel („Signatur fortsetzen“ ist die
  Nutzeraktion; kein Zustimmungsschritt, das Angebot zeigt bei ID Austria den Kurzhinweis, ADR 0003). Sitzung, Anmeldung und Bestätigung laufen neu (das ID-Austria-Passwort wird nie
  persistiert); `Effect::Prepare` wird durch `Prepared::restore` ersetzt – Signaturzeitpunkt `/M`, Block
  und Position bleiben die des ersten Versuchs. Scheitert die Wiederherstellung (Prüfsumme, Original),
  wird mit Hinweis `ng-resume-restore-failed` neu vorbereitet. `discard_resume` löscht den Zustand.
- **Löschen:** `Effect::DeletePersisted` nach Erfolg, Abbruch und Fehler löscht nur den Zustand des
  **laufenden** Vorgangs (ein noch nicht angenommenes Angebot übersteht den Abbruch eines anderen Vorgangs
  vor `prepare`; ein neues `prepare` ersetzt es).

## 4. Dokumente, Ausgabe, Secrets, Signiermittel

- **Eingang:** `RunEvent::Opened` liefert URLs. `mobile::ingest_urls` liest `file:`-URLs als **Bytes** und
  legt Kopien in `<app_cache_dir>/inbox/<n>/<Name>` ab (Name bleibt → Ausgabename `<Name>_signed.pdf`);
  nicht lesbare URLs (Android `content://`, Security-Scoped) und Intents holt die Shell über
  `take_pending_open_documents` – beim Start und nach jeder Rückkehr in den Vordergrund. Die
  Dateiauswahl der Oberfläche läuft über den Befehl `mobile_pick_files` (`kind` `pdf`, Kopien in
  `<app_cache_dir>/import/<n>/<Name>`; Ergebnis `{ paths, skipped }` in der Reihenfolge der Auswahl, ADR 0004
  Nachtrag 17; `pkcs12` lehnt die Shell seit §12 mit `ng-ui-event-invalid` ab).
- **Ausgabe:** `CoreOptions::default_output_dir` = iOS `Documents` (Tauri `document_dir`, in der Dateien-App
  sichtbar), Android `filesDir/Documents` (`mobile::ANDROID_OUTPUT_DIR`; **nicht** `getExternalFilesDir`, den unter
  Android 9/10 Apps mit Speicher-Berechtigung mitlesen), dann ohne Speichern-Dialog; vorhandene Datei → Rückfrage
  „Überschreiben?“. Im Ergebnis „Teilen“ (`share_result`) und „Sichern unter …“ (`export_result`) –
  Effekte `Effect::ShareResult`/`ExportResult` → `ShellHooks::share_path`/`export_path` → Plugin.
- **Secrets:** `pida_config::ShellSecretStore` über `mobile::PluginSecureStorage` (Schlüssel
  `PIDA/<SecretId::account>`); Verfügbarkeit beim ersten Zugriff (Probe `secureStoreGet`). Solange
  die native Seite leer ist: nur RAM, `is_persistent() == false` (UI-Hinweis `ng-secrets-not-persistent`).
  Das ID-Austria-Passwort hat keinen `SecretId` und erreicht den Speicher nie.
- **Signiermittel:** nur ID Austria – keine lokale BKU und kein Keystore (Registry beide nur auf dem Desktop,
  §12); ID Austria mit `BackendOptionView.notes = ["ng-fido2-unavailable-mobile"]` – die Oberfläche zeigt FIDO2
  ausgegraut mit Hinweis (Auswahl, Masken mit `can_switch_to_fido2`, Einstellungen › FIDO2).
- **PKCS#12-Import (bis 16.10.2026; seit §12 auf Mobilgeräten ohne Wirkung):** `import_keystore { path, password, remember }` – der Kern prüft Format und Passwort
  (`backend-keystore::list_aliases`), kopiert die Datei nach `Paths::data_dir/keystores/` (0600), setzt
  Einstellungen › Keystore (aktiviert, `PKCS12`, Alias bei genau einem Schlüssel) und legt das Passwort
  mit `remember` im sicheren Speicher ab (Speicherart „Schlüsselbund“); Ergebnis in
  `ViewState.keystore_import` (`ng-keystore-imported`, `keystore-wrong-password`,
  `ng-keystore-import-format`, `ng-keystore-import-busy` während eines Signiervorgangs …).

## 5. PDFium

- **iOS:** `tools/fetch-pdfium.sh --xcframework` → `tools/pdfium/apple/PDFium.xcframework` mit den Slices
  `ios-arm64` und `ios-arm64_x86_64-simulator` (je `pdfium.framework/pdfium`, install name
  `@rpath/pdfium.framework/pdfium`, ad-hoc signiert). Verwendet `xcodebuild -create-xcframework`; scheitert
  `xcodebuild` (auf dem Entwicklungs-Mac: „failed to load a required plug-in“ ohne
  `xcodebuild -runFirstLaunch`), stellt das Skript dieselbe Struktur direkt zusammen. Im Xcode-Projekt
  „Embed & Sign“. Der Kern bindet `Frameworks/pdfium.framework/pdfium` im Bundle
  (`pida_render::library_paths_in`, `pdfium::mobile_dir`); keine Suche nach dem bloßen Dateinamen.
- **Mindest-iOS:** Die `pdfium-binaries` chromium/8100 für iOS sind mit `minos 17.0` gekennzeichnet; M-iOS setzt
  die Kennzeichnung mit `vtool` auf **16.0** (Plan §5.1, `PDFIUM_IOS_MIN`, docs/integration/mobile-ios.md §3) –
  auf iOS 16 selbst ungetestet.
- **Android:** `tools/fetch-pdfium.sh --jnilibs` → `tools/pdfium/android/jniLibs/{arm64-v8a,armeabi-v7a,x86_64}/libpdfium.so`
  (keine Abhängigkeit von `libc++_shared`). Der Kern lädt über `dlopen("libpdfium.so")` aus
  `nativeLibraryDir` (wie `System.loadLibrary`); `bundle.android.minSdkVersion = 28` (Plan: Android 9).

## 6. TLS unter Android

`rustls-platform-verifier` braucht JavaVM, Context und Class-Loader. Das Plugin initialisiert ihn beim
Start aus `ndk-context`, das tao beim Anlegen der Activity setzt (`tls::init_platform_verifier`, einziges
`unsafe` mit begründeten SAFETY-Kommentaren). Zusätzlich muss die Kotlin-Komponente ins APK
(Aufgabe M-Android): Maven-Repository
`https://github.com/rustls/rustls-platform-verifier/raw/maven-archive/android-release-support/maven/` und
`implementation("org.rustls:rustls-platform-verifier:<Version von rustls-platform-verifier-android aus Cargo.lock, derzeit 0.2.0>")` (Gruppe `org.rustls`, nicht `rustls` wie in der README des Crates; umgesetzt in M-Android);
die Proguard-Regel liefert `android/consumer-rules.pro`. Ohne Komponente scheitert die erste
A-Trust-Verbindung.

## 7. Oberfläche

- `ViewState.platform.mobile` (`isMobile`) schaltet: Durchsuchen über `mobile_pick_files`; Ergebnis mit
  „Teilen“/„Sichern unter …“ statt „Im Ordner anzeigen“; Einstellungen ohne Ausgabeordner, Proxy
  und Log-Ordner (die Update-Prüfung gibt es seit ADR 0004 Nachtrag 31 auch mobil: Hinweis auf eine neue Version
  im Store); Signiermittel nur ID Austria (bis 16.10.2026 Keystore-Tab mit Import-Assistent
  `features/settings/KeystoreImport.tsx` – entfernt, §12); „Signatur fortsetzen“-Dialog (`app/ResumeDialog.tsx`,
  Verwerfen nur ausdrücklich); FIDO2 ausgegraut mit Hinweis.
- Positionieren per Touch: außerhalb des Blocks angetippt steht der Block mittig **über** dem Finger
  (`TOUCH_OFFSET_PT` 12 pt), ein zweiter Finger verschiebt nichts, kurze Haptik (Android), größere
  Trefferfläche; Zahlenfelder gibt es seit 15.10.2026 nicht mehr (D1). Pinch-Zoom ist optional und offen.
- Safe Areas (`viewport-fit=cover`, `env(safe-area-inset-*)`), Tap-Ziele ≥ 44 px bei `pointer: coarse`,
  (bis E0: Hauptleiste auf schmalen Telefonen mit Namen nur des aktuellen Schritts; seit E1 ohne Hauptleiste).
- Capabilities: `capabilities/mobile.json` (`platforms: ["iOS", "android"]`) – Ereignisse, Kern-Bridge,
  `allow-mobile-pick-files`, `opener` nur `https:`/`mailto:`; **kein** Dateidialog, Log-Ordner, Dateimanager,
  keine DevTools. `capabilities/default.json` gilt nur noch für den Desktop.
- Befunde des Orchestrators behoben: (a) Links im Markup sind echte Inline-Links (`role="link"`), der
  Satzpunkt bleibt am Link (vorher: `<button>` als atomares Inline-Element, Punkt allein in der nächsten
  Zeile); (b) Live-Region und Felder nennen dieselben Kanten (`linke Kante … von links, Oberkante … von
  unten`; `announce` liefert `top` statt `bottom`).

- **Seit E1 (ADR 0004, „Arbeitsbereich“):** ein Bildschirm statt der Schritte; unter 760 px Inspektor als
  Bottom-Sheet (eingeklappt: Konto-Chip, „Signieren“ und die Überdeckungswarnung), Miniaturen eingeklappt;
  Sticker per Touch mit Fingerversatz (Block mittig **über** dem Finger), ein zweiter Finger verschiebt nicht,
  Doppeltippen setzt den Block, Haptik beim Einrasten; Zahlenfelder gibt es nicht mehr (D1). Signierkarte als
  Vollbild-Overlay über der Seitenansicht (App-Wechsel zur ID-Austria-App ist kein Abbruch), FIDO2 ausgegraut
  mit Hinweis; „Fertig“ mit „Öffnen“, „Teilen“, „Sichern unter …“ (kein Ordner), kleinere Vorschau, damit die
  Aktionen ohne Scrollen sichtbar bleiben; PKCS#12-Import aus dem Menü des Konto-Chips (entfällt seit §12: der
  Konto-Chip zeigt ID Austria ohne Wechsel); „Signatur fortsetzen“
  mit Kurzhinweis bei ID Austria. Nachweise: `src/app/Mobile.test.tsx` (`?platform=ios|android` im Mock),
  Screenshots `docs/integration/screenshots/stage1/mobil-*.png` (375 × 812 @2x, hell/dunkel). Offen [H]:
  Simulator/Emulator (Safe Areas, App-Wechsel bei der Bestätigung, Touch-Ziehen auf dem Gerät).
- **Seit E2 (ADR 0004 §3.8, Stapel):** ab zwei geteilten bzw. gewählten PDFs die Stapelübersicht (unter 760 px
  einspaltig, „Alle signieren“ über die volle Breite); Ausgabe in die App-Dokumente ohne Ordnerwahl und ohne
  „Im Ordner zeigen“; Stapelkarte als Bottom-Sheet mit „Bestätigung k von n · Name“, Überspringen, Pausen und
  Abbruch-Rückfrage; App-Wechsel je Bestätigung ist kein Abbruch. App-Kill im Lauf: „Signatur fortsetzen“ setzt
  nur das unterbrochene Dokument fort, danach die Übersicht mit dem Rest „Bereit“ – der Lauf geht nicht von selbst
  weiter (Kern: `crates/core-api/tests/mobile.rs`). Nachweise: `features/batch/batch.test.tsx` (Mobile, schmal,
  axe), Screenshots `docs/integration/screenshots/stage2/mobil-*.png`; 360 px ohne waagrechtes Scrollen. Offen
  [H]: Simulator/Emulator (App-Wechsel je Bestätigung, App-Kill im Lauf).
- **Seit E3 (ADR 0004 §3.10/§3.11, Nachtrag 5):** Einstellungen als Vollbild mit waagrechter Bereichsleiste und
  Suche, sofort wirksam; ohne Proxy, Ausgabeordner, Protokolle, Chipkarte und Bildmarken-Zeile
  (der Kern lehnt `proxy.*`, `output.folder`, seit §12 `keystore.*` und Keystore bzw.
  Chipkarte als Standard auf Mobilgeräten mit `ng-settings-unavailable-mobile` ab); Keystore bis 16.10.2026 nur
  über den PKCS#12-Import-Assistenten, seit §12 gar nicht; Bereich
  „Dokument“ (PDF/A, Platzhaltersuche) wie am Desktop, Hinweise im ausgeklappten Bottom-Sheet; „Zuletzt
  signiert“ ohne Ordneranzeige. Nachweise: `features/settings/{settings,a11y}.test.tsx` (Mobile), Kern
  `crates/core-api/tests/settings.rs::mobile_restrictions`, Screenshots `docs/integration/screenshots/stage3/mobil-*.png`
  (375 × 812 @2x); 375 px ohne waagrechtes Scrollen der Seite. Offen [H]: Simulator/Emulator.
- **Seit E4 (ADR 0004 §3.9/§3.12, Nachtrag 6):** Prüfmodus mit den Seiten über dem Ergebnis (Kopfzeile
  „Prüfmodus“), Datei über die Dokumentauswahl (`pickMobileFiles`) bzw. „Zuletzt signiert“ – **seit Nachtrag 15
  (17.10.2026) entfernt**: „Zuletzt signiert“ bietet auf Mobilgeräten „Öffnen“ (Vorschau, QuickLook bzw.
  `ACTION_VIEW`), „Teilen“ (Share-Sheet) und „Erneut signieren“ wie „Fertig“; Ersteinrichtung als
  Sheet ohne Chipkarte (seit §12 ohne Signiermittel-Schritt – nur ID Austria), ohne „neben dem Original“ (Ausgabe bleibt in
  den App-Dokumenten) und ohne „PDF-Over-Einstellungen übernehmen“ in Einstellungen › Erweitert (auf Mobilgeräten
  gibt es kein PDF-Over); Palette ohne Tastenkappen. Nachweise: `features/recent/recent.test.tsx` (Mobile, seit
  Nachtrag 15), `features/onboarding/onboarding.test.tsx`, `features/palette/palette.test.tsx` (Mobile, schmal, axe), Kern
  `crates/core-api/tests/first_run.rs` (Mobile), Screenshots `docs/integration/screenshots/stage4/mobil-*.png`
  (375 × 812 @2x); 360 px ohne waagrechtes Scrollen. Offen [H]: Simulator/Emulator, App-Kill während der
  Ersteinrichtung.

## 8. Bauen und Prüfen

Umgebung wie `tools/env.sh` (Rust 1.99, JDK 21, Android-SDK, NDK 29.0.14206865). Pfad mit Leerzeichen:
`cargo check` für iOS/Android funktioniert direkt; scheitern Gradle/Xcode-Skripte daran, über einen
Symlink ohne Leerzeichen bauen (`ln -s "<worktree>" /tmp/pida-mobile` und dort `npm run tauri …`).

```sh
# PDFium für Mobile (einmalig)
tools/fetch-pdfium.sh --xcframework --jnilibs

# Kern, Shell, Plugin (Desktop-Tests)
cargo test -p pida-workflow -p pida-core-api -p pida-app -p tauri-plugin-pida-mobile -p pida-config
cd apps/app && npm test

# Typprüfung Mobile (iOS kompiliert dabei das Swift-Paket des Plugins)
cargo check -p pida-app --target aarch64-apple-ios-sim
cargo check -p pida-app --target aarch64-apple-ios
NDK=$NDK_HOME/toolchains/llvm/prebuilt/darwin-x86_64/bin
CC_aarch64_linux_android=$NDK/aarch64-linux-android24-clang AR_aarch64_linux_android=$NDK/llvm-ar \
CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=$NDK/aarch64-linux-android24-clang \
  cargo check -p pida-app --target aarch64-linux-android
```

Projekte anlegen (M-iOS/M-Android, je in `apps/app`): `npm run tauri ios init` (braucht `xcodegen`:
`brew install xcodegen`; auf dem Entwicklungs-Mac zusätzlich einmal `sudo xcodebuild -runFirstLaunch`),
`npm run tauri android init`. Danach `apps/app/src-tauri/.gitignore` anpassen (`/gen/apple`, `/gen/android`
werden versioniert, nur Build-Ausgaben ignorieren). Kennung `at.pida`: unter Android ist `-` im
Paketnamen unzulässig – Tauri leitet `at.pida` ab; prüfen und in `tauri.conf.json` ggf. per
`bundle.android`/Plattform-Konfiguration festlegen.

Nachweise M0 (15.10.2026): Workspace-Tests grün (`cargo test --workspace`), `npm test` 264 Tests,
`cargo clippy -D warnings` (Desktop, `aarch64-apple-ios`, `aarch64-apple-ios-sim`,
`aarch64-linux-android` mit NDK) für Shell und Plugin; `cargo check -p pida-app` für iOS-Simulator und
Android grün (verbleibende Warnungen nur `dead_code` in `pdf-cos`, `sigblock`, `pades` – Felder, die auf
den Mobil-Zielen ungelesen bleiben; außerhalb von M0, `pdf-cos`/`sigblock` gehören zu T2). Nicht geprüft (Aufgabe M-iOS/M-Android): Start im Simulator/Emulator, native Seiten,
Gradle-Build der Kotlin-Hülle.

## 9. Aufgaben

Gemeinsame Regeln: Kern, Shell-Rust, Plugin-Rust und Oberfläche sind von M0 fertig; Änderungen daran nur
mit Absprache (Integration), Vertrag in §2 nicht einseitig ändern. Keine echten Zugangsdaten, nur
Testdaten (`testdata/pdf`, `testdata/keystores`, Passwort `test`), kein Netzverkehr in Tests.

**M-iOS** (Dateien: `apps/app/plugins/pida-mobile/ios/**`, `apps/app/src-tauri/gen/apple/**`):

1. `tauri ios init`; Projekt versionieren; `PDFium.xcframework` einbinden („Embed & Sign“), Deployment
   Target 17.0.
2. `Info.plist`: `CFBundleDocumentTypes` (`com.adobe.pdf`, Rolle Viewer/Editor), `LSSupportsOpeningDocumentsInPlace`
   (Entscheidung: Kopie in `Inbox` vs. in place mit Security Scope), `UIFileSharingEnabled` (Ausgabe in
   „Dateien“ sichtbar), `ITSAppUsesNonExemptEncryption` (Exportkontrolle), ATS ohne Ausnahmen.
3. Swift: Keychain (`secureStore*`), `UIActivityViewController` (`shareFile`), Dokument-Picker
   (`exportFile`, `importFile` mit Kopie nach `destination`), „Öffnen in“/Share-Eingang inkl.
   Security-Scoped URLs (`takePendingOpenDocuments`).
4. Im Simulator nachweisen: Start, PDFium gebunden (Vorschau), Dokument über „Teilen“ öffnen, Keystore-
   Import + Signatur mit `test-rsa2048.p12` (historisch – entfällt seit §12), Ausgabe teilen/sichern, App-Wechsel während einer
   (simulierten) Wartemaske, App beenden → „Signatur fortsetzen“; Screenshots.
5. QuickLook für „Öffnen“ des Ergebnisses (`opener` öffnet auf iOS keine Pfade) – ggf. `ShellHooks::open_path`
   über das Plugin (Vertrag mit M0/Integration abstimmen).

**M-Android** (Dateien: `apps/app/plugins/pida-mobile/android/**`, `apps/app/src-tauri/gen/android/**`) –
**umgesetzt** (15.10.2026), Stand, Nachweise und Befunde: docs/integration/mobile-android.md:

1. `tauri android init`; Projekt versionieren; `jniLibs` aus `tools/pdfium/android/jniLibs` einbinden
   (`sourceSets.main.jniLibs.srcDirs`), `minSdk 28`.
2. Manifest: Intent-Filter `VIEW`, `SEND`, `SEND_MULTIPLE` für `application/pdf` (Activity
   `singleTask`, `configChanges` für Drehen/Dark Mode), `FileProvider` (Authority
   `${applicationId}.pida.files`) für „Teilen“.
3. Kotlin: Android Keystore (`secureStore*`), `ACTION_SEND` (`shareFile`), SAF `ACTION_CREATE_DOCUMENT`
   (`exportFile`), `ACTION_OPEN_DOCUMENT` (`importFile`), Intents in `load()`/`onNewIntent` vormerken und
   über `ContentResolver` nach `destination` kopieren (`takePendingOpenDocuments`).
4. `rustls-platform-verifier`-Komponente (Maven, Version aus `Cargo.lock`, §6) einbinden.
5. Im Emulator `pida_api35` nachweisen wie M-iOS Punkt 4; zusätzlich Drehen während der Wartemaske,
   „Nicht behalten“-Entwickleroption (Activity neu) und Zurück-Taste.
6. „Öffnen“ des Ergebnisses über `ACTION_VIEW` (FileProvider) statt `opener`-Pfad.

**Offen für die Integration:** Pinch-Zoom der Vorschau (optional), Bildmarke aus der Fotomediathek
(HEIC, Plan §5.2), Managed App Configuration/Restrictions (Richtlinien), CI-Jobs mit Simulator/Emulator,
Mindest-iOS (§5).

## 10. Adversarialer Review der Mobile-Welle (15.10.2026)

Geprüft: `git diff main...HEAD` (M0, M-iOS, M-Android) auf Sicherheit (Plan §7.7), Funktion (§5.2/§3.5) und
Belege; beide Plattformen neu gebaut und im Simulator bzw. Emulator durchgespielt. Behobene Befunde:

| # | Schwere | Befund | Behebung |
|---|---|---|---|
| R-1 | hoch | Einstellungen speichern absolute Pfade; iOS vergibt bei jedem Update einen neuen Container (`…/Application/<UUID>/`). Danach war der importierte Keystore „nicht auf diesem Gerät“, und mit nur ID Austria als verfügbarem Signiermittel startete der Kern **ohne Rückfrage** eine ID-Austria-Sitzung (anonymer InfoboxRead bei A-Trust – Ursache der beiden Vorfälle aus M-iOS/M-Android) | (a) `workflow::auto_choice` F-54 wie PDF-Over (`KEYSTORE_ENABLED`): ein **eingeschalteter** Keystore verhindert die automatische Wahl auch dann, wenn die Datei fehlt – es erscheint die Auswahl. (b) `core-api` `relocate_sandbox_files` (nur Mobile): fehlt `[keystore] file` bzw. `signature.emblem`, liegt aber unter gleichem Namen in `data_dir/keystores` bzw. `data_dir/emblem`, wird der Pfad umgestellt und die Passwörter im sicheren Speicher (Konto = Pfad) übertragen (`/var` ↔ `/private/var` berücksichtigt). Im Simulator belegt: Update-Installation → neue Container-UUID → Log `importierter Keystore nach Umzug der Sandbox neu zugeordnet secrets=2`, Signatur ohne Passwortabfrage, pyHanko *VALID* |
| R-2 | hoch | `app.exit` auf Mobile: `Finished` des Kerns (z. B. „Abbrechen“ im Fehlerschritt beim Start, `SKIP_FINISH`) und „Beenden“ ohne Kern riefen `app.exit` – unter iOS Panik in tao + `abort`, unter Android `exit()` gegen laufende Systemthreads | Shell `finish_on_mobile`: Mobile beendet sich nie selbst, sondern startet einen neuen Kern (wieder „Dokument öffnen“); `DispatchOutcome::Quit` ebenso |
| R-3 | mittel | Android-Ausgabe in `getExternalFilesDir(Documents)` – unter Android 9/10 für Apps mit Speicher-Berechtigung lesbar | Ausgabe in `filesDir/Documents` (Datei 0600), FileProvider des Plugins `files-path Documents/`; Teilen, „Sichern unter“, „Öffnen“ im Emulator belegt |
| R-4 | mittel | Kopien im Cache blieben liegen, darunter ein **zweites Exemplar des importierten PKCS#12** (`cache/import/<n>/*.p12`) | Kern entfernt nach erfolgreichem Import die Kopie der Dateiauswahl (nur innerhalb von `Paths::cache_dir`, nach falschem Passwort bleibt sie für den nächsten Versuch); Shell räumt beim Start Kopien in `inbox/` und `import/` älter als 1 h (`STALE_COPY_AGE`, = Höchstalter „Signatur fortsetzen“) weg |
| R-5 | mittel | iOS: importierter Keystore unter `Library/Application Support` landete in iCloud-/Geräte-Sicherungen (Android: `allowBackup=false`, Keychain: `…ThisDeviceOnly`) | Plugin legt `…/data/keystores` beim Laden an (0700) und setzt `isExcludedFromBackup` (geprüft: xattr `com_apple_backup_excludeItem`) |
| R-6 | mittel | Android: von Tauri erzeugter Intent-Filter (`VIEW`/`SEND` mit `BROWSABLE`, implizit `file:`) breiter als nötig | `tauri.android.conf.json` setzt `bundle.fileAssociations = null`, der erzeugte Block im Manifest ist leer; `cmd package query-activities` löst `BROWSABLE`- und `file:`-`VIEW` nicht mehr auf, `content:`-`VIEW`/`SEND` weiter |
| R-7 | mittel | `tauri-plugin-dialog` auch im Mobile-Binary (iOS: Photos/PhotosUI-Bezüge, App-Store-Zweckangaben; Android: zusätzliche Bibliothek), obwohl Mobile nur den Dokument-Picker nutzt | Abhängigkeit und Registrierung nur Desktop (`otool -L`: kein Photos-Framework mehr) |
| R-8 | niedrig | Oberfläche: Überschreiben-Rückfrage mit vollem Sandbox-Pfad; Zugriffstasten-Unterstreichungen auf Touch-Geräten (auch in Dialogen, die als Portal außerhalb von `.app` liegen); „Browse for PDF on your computer“ | nur Dateiname; `data-mobile` auch an `<html>`, CSS blendet `.mnemonic` aus; eigener Text `ng-ui-browse-mobile` („Dokument wählen …“) |
| R-9 | niedrig | Android-Secure-Store verwarf leere Werte (Längenprüfung `>` statt `>=`); `hooks::open_path` fiel unter iOS bei jedem Fehler auf `opener` zurück | korrigiert; Rückfall nur noch bei `not-implemented` |
| R-10 | niedrig | Doku: Keychain-Zugriffsklasse, `openFile` unter iOS, Mindest-iOS veraltet | korrigiert (§2, §5, Plugin-Moduldoku) |

**Nachtrag 16.10.2026 (§12):** Seit „Mobile nur mit ID Austria“ betreffen R-1 (b) für den Keystore und R-4 nur
noch frühere Builds – `relocate_sandbox_files` ordnet nur noch die Bildmarke neu zu, `[keystore] file` bleibt
unberührt; R-5 (Sicherungsausschluss von `data/keystores`) schützt weiter liegengebliebene Dateien früherer Builds.
R-1 (a) gilt unverändert am Desktop; auf Mobilgeräten verhindert jetzt der Autostart-Befund F-54 (ohne „Signieren“
keine Sitzung bei A-Trust) dasselbe.

Geprüft und **in Ordnung**: WebView erreicht das Plugin nicht (Tauri prüft die ACL vor `run_mobile_plugin`;
`mobile.json` ohne `pida-mobile:*`), CSP/Capabilities minimal, ID-Austria-Passwort ohne `SecretId` (nur RAM der
Sitzung), Fehler/Logs ohne Pfade, URIs oder Werte, Pfadprüfungen bei Import/Eingang (Rust `within`, Swift
`isInside`, Kotlin `Documents.within`), `resume.json` nur mit relativen Pfaden und Prüfsummen, Keychain
`WhenUnlockedThisDeviceOnly` ohne Synchronisierung, Android-Keystore-AES-GCM mit Schlüssel als AAD, Backup-Regeln,
Lebenszyklus (Hintergrund ≠ Abbruch, Long-Poll-Neustart), FIDO2 ausgegraut, keine lokale BKU.

**Nachweise (diese Prüfung):** iOS-Simulator iPhone 17/iOS 27 – Startprüfung `pida-smoke: ok step=open
pdfium=true`, Keystore-Import über den Dokument-Picker (Kopie im Cache danach entfernt, Datei 0600,
Sicherungsausschluss), Positionieren per Touch, Signiermittel-Auswahl (FIDO2 ausgegraut, Keystore wählbar), Signatur
ohne Passwortabfrage, Teilen, „Sichern unter“ (bitgleich), QuickLook, Überschreiben-Rückfrage nur mit Dateinamen,
Lebenszyklus `suspended=true/false`, Update-Installation mit Umzug des Keystores; drei signierte Dokumente laut
pyHanko *VALID* (`--trust testdata/keystores/test-rsa2048.crt.pem`), `qpdf --check` ohne Fehler. Netzschutz: Start
mit `HTTPS_PROXY`/`HTTP_PROXY`/`ALL_PROXY=http://127.0.0.1:9` (System-Proxy des Kerns) – keine Verbindung zu A-Trust.
Android-Emulator `pida_api35` im Flugmodus – Startprüfung `pdfium=true`, Keystore-Import über SAF (Cache-Kopie
entfernt, Secrets nur als `v1:`-Chiffrat), Dokument per `VIEW`-Intent (genau einmal), Ziehen des Blocks, Hintergrund
und zurück ohne Zustandsverlust, Signatur ohne Passwortabfrage, Ausgabe in `files/Documents` (0600), Teilen,
„Sichern unter“ (bitgleich nach `Download/`), „Öffnen“ über den FileProvider; pyHanko *VALID*. Screenshots in
`docs/integration/screenshots/{ios,android}/`.

Desktop ohne Regression: `cargo test --workspace --no-fail-fast` grün (1129 Tests, danach 2 weitere in
`security_config`), `npm test` 266 Tests, `cargo clippy -D warnings` für Shell, Plugin, Kern und Workflow (Desktop,
`aarch64-apple-ios-sim`, `aarch64-linux-android`), Gradle `testDebugUnitTest` 5/5; macOS-Debug-Bundle-Startprüfung
dreimal `pida-smoke: ok step=open pdfium=true` (464/400/384 ms). Hinweis: Läuft bereits eine andere Instanz von
PIDA (Single-Instance-Socket `/tmp/at_pida_si.sock`), übergibt die Startprüfung an sie und gibt nichts
aus – für die Prüfung deshalb mit `--config '{"identifier":"at.pida.smoketest"}'` gebaut.

Offen (nicht behoben, siehe auch mobile-ios.md §7 und mobile-android.md §7): Android beendet den Prozess, wenn das
System die Activity ohne Konfigurationsänderung zerstört (Tauri/wry, upstream); Startprüfung unter Android endet in
`exit()` (FORTIFY-Meldung möglich, nur Debug-Startprüfung); App-Wechsel zur App „ID Austria“ und „Signatur fortsetzen“
nach App-Kill nur durch Kern-Tests belegt (brauchen A-Trust); `CFBundleShortVersionString`/`versionName` 1.0.0 statt
0.1.0 (behoben 16.10.2026, §11); iOS bietet im Share-Sheet die App selbst und „Markup“ an.

## 11. Neue Oberfläche auf Mobile (16.10.2026)

Die neue Oberfläche (ADR 0004, E1–E4: Arbeitsbereich mit Bottom-Sheet, Signier-Statuskarte, Fertig mit Teilen und
„Sichern unter“, Stapel, Einstellungen im Vollbild mit Bereich „Dokument“, Prüfmodus – seit ADR 0004 Nachtrag 15
entfernt –, Ersteinrichtung) wurde auf
beiden Plattformen am Gerät durchgespielt, die Befunde behoben und beide Stände in `mobile/int` mit `main`
zusammengeführt. Einzelheiten, Aufnahmen und Befundlisten: mobile-ios.md §8 (Simulator iPhone 17 und zur
Gegenprüfung iPhone 17e, iOS 27; `docs/integration/screenshots/ios-ng/`) und mobile-android.md §8 (Emulator
API 35, 411 CSS-px; `docs/integration/screenshots/android-ng/`). Umfang Mobile 1.0 (Plan §5.2): ID Austria
(App-Bestätigung/SMS, kein FIDO2), PKCS#12 nur per Import (seit §12: kein Keystore mehr), keine lokale Chipkarte,
keine CLI. Netz immer gesperrt
(iOS: Proxy `127.0.0.1:9`; Android: Proxy und Flugmodus), A-Trust nie erreicht, keine Zugangsdaten eingegeben.

### 11.1 Ergebnisse

| Ablauf | iOS | Android |
|---|---|---|
| Start, Ersteinrichtung, „Später einrichten“ | ok | ok |
| PDF empfangen, Arbeitsbereich, Ziehen per Touch, Signaturfeld/QR-Platzhalter | ok nach R1 (Block nach dem Ziehen unter dem Sheet); mehrere PDFs per „Teilen“ nur das erste (offen) | ok |
| Keystore-Import (entfällt seit §12), Signieren, Fertig, Teilen, „Sichern unter“, Öffnen | ok nach R2 (Tastatur verdeckte Dialoge), R4 (Systemtexte englisch); pyHanko *VALID* | ok; pyHanko *VALID* (5 Dateien, auch 120 MB) |
| Stapel, Hintergrund/Vordergrund, App-Ende im Lauf | ok (3 von 3 nach Hintergrund); Keystore hat nach Kill nichts fortzusetzen | ok nach B19 („Fortsetzen“ behält „Beide behalten“) |
| Einstellungen im Vollbild, Suche, Mobile-Einschränkungen | ok nach Befunden 5, 6, 11, 16 | ok nach B11, B14–B16 |
| Prüfmodus (seit ADR 0004 Nachtrag 15 entfernt) | ok | ok nach B3 (Ergebnis scrollt) |
| ID Austria ohne Netz | ok nach 13, 14 und R3 (Benutzername eingebbar) | ok nach B5, B9 |
| Safe Areas, Tastatur, Schriftgröße, Bildschirmleser (Inspektion) | ok nach 4, 7–10, 15 (Dynamic Type bis 200 %) | ok (130 %, Zurück-Geste nach B18) |

### 11.2 Wichtigste Behebungen

- **Beide Plattformen (unabhängig gefunden):** Segmente brechen um; Keystore-Schlüssel mit Alias darunter;
  Papierkorb im schmalen Stapel sichtbar; Fingerziele ≥ 44 px (Kopfzeile, Hinweis schließen, Suche löschen,
  kompakter Konto-Chip); „Signaturdienst nicht erreichbar“ ohne technische Fehlerkette; Speicherart „Sicherer
  Speicher“ statt „Schlüsselbund“ (entfällt mit dem Keystore seit §12).
- **iOS:** Dynamic Type über das Plugin (`TextSize.swift`); Software-Tastatur verdeckt keine Bottom-Sheet-Dialoge
  mehr (`components.css`, `app/keyboard.ts`); kein WebKit-Zoom nach Fokus (Felder 16 px); Safe Areas im
  Querformat; `CFBundleLocalizations` de/en; „Zuletzt signiert“ folgt nach einem Update dem neuen Container;
  Zeile der Startprüfung im System-Log; keine ANSI-Farbcodes im Log.
- **Android:** Farben der Systemleisten, Schriftgröße zur Laufzeit, Zurück-Geste ohne Fokusrahmen; Kern:
  `ResumeRecord::on_existing` – ein fortgesetzter Stapel behält die Regel für vorhandene Dateien.
- **Plattformübergreifend aus den Gerätetests:** Bildlauf bei Ansichtswechsel zurückgesetzt; Bildlauf-Ausgleich,
  wenn die Leiste „Signaturfeld gefunden“ erscheint oder verschwindet; „Erneut versuchen“ bei Zeitüberschreitung
  vor der Bestätigung; „Telefonnummer oder Benutzername“ mit Standardtastatur.

### 11.3 Zusammenführung (`mobile/int`)

Beide Branches (`mobile/ios` 750b71c, `mobile/android` d2bf925) und danach `main` (6656e62: automatische
Positionierung, „+ PDF“, „Zur Startseite“/„In Bearbeitung“) gemergt. Wo beide Plattformen denselben Befund
verschieden behoben hatten, gilt eine Lösung:

| Thema | gilt | entfällt |
|---|---|---|
| Fehlertext ohne Fehlerkette | Attribut `.text` (iOS) in `friendlyError()`, Karte, Stapelpause, Fehlerkarte | `.hint` (Android) samt Texten in `pida.ftl` |
| Kopfzeile ≥ 44 px | `min-width`/`min-height` auf allen Finger-Geräten (iOS), dazu „+ PDF“ aus `main` | feste 44 px nur ≤ 760 px (Android) |
| Papierkorb im schmalen Stapel | `.batch-row-action.has-caret` in der ersten Zeile (Android, mit Test) | absolut positionierter Papierkorb (iOS) |
| Keystore-Schlüssel | Name und Alias untereinander (Android, mit Test) | Flex-Zeile mit Umbruch (iOS) |
| Segmente | beides: Gruppe bricht um (iOS) **und** Beschriftung bricht um (Android) | – |
| Konto-Chip ≥ 44 px | eine Touch-Regel | doppelte Regel |

Nachträge der Integration: Android-Test `batch_resume_keeps_the_rule_for_existing_files` an `OpenFiles { mode }`
aus `main` angepasst; Hinweisleiste „Signaturfeld gefunden“ nur bei verschiebbarem Block (`movable` aus `main`)
zusammen mit dem Bildlauf-Ausgleich (iOS R1); `pida-cli` kompilierte seit „Passwort merken?“ (458f6d5) nicht mehr
(`SubmitSecret` ohne `password_storage`, Muster `InteractionView::Secret`) – die Kommandozeile ändert die
Speicherart nicht (`password_storage: None`). **App-Version vereinheitlicht:** 0.1.0 in `Cargo.toml`,
`package.json`, `tauri.conf.json` (`version`, vorher fehlend – die CLI setzte iOS auf 1.0.0), `Info.plist` und
`project.yml` (`CFBundleShortVersionString`/`CFBundleVersion`), Android über `tauri.properties` aus `version`
(`versionName` 0.1.0, `versionCode` 1000) mit denselben Rückfallwerten in `build.gradle.kts`; Test
`apps/app/src/app/version.test.ts`.

**Prüfungen (Stand `mobile/int`):** `cargo test --workspace --no-fail-fast` 1424 bestanden, 0 fehlgeschlagen,
5 ignoriert (darin `ts_bindings` und `serde_contract` ohne Neuerzeugung); `npm ci && npm run build && npm test`
32 Dateien, 687 Tests; `python3 tools/rename-to-pida.py --check` ohne Befund; `cargo check -p pida-app` für
`aarch64-apple-ios-sim` und `aarch64-linux-android` (NDK) grün, je eigenes `CARGO_TARGET_DIR`. macOS-Debug-Bundle:
Startprüfung `PIDA_SMOKE=1` mit leerem `PIDA_HOME`/`HOME` dreimal `pida-smoke: ok step=open pdfium=true`
(507/430/414 ms), Bundle-Version 0.1.0. Wie in §10 mit `--config '{"identifier":"at.pida.smoketest"}'` gebaut, damit
die Prüfung nicht an eine laufende PIDA (Socket `/tmp/at_pida_si.sock`) übergibt.

### 11.4 Offen

- **Gemergter Stand nicht erneut am Gerät:** Die Kombinationen aus 11.3 (z. B. iOS-Tastaturabstand auf Android,
  Android-Segmente und -Papierkorb auf iOS) sind per Test und Durchsicht geprüft, nicht erneut im Simulator bzw.
  Emulator; ebenso die neuen Funktionen aus `main` („+ PDF“, „In Bearbeitung“, automatische Positionierung) auf
  dem Telefon.
- **Entschieden (16.10.2026, „Hinweis reicht“):** Teilen mehrerer PDFs aus der iOS-Dateien-App liefert nur das
  erste (ohne Share-Extension) – nach dem Empfang über „Teilen“ zeigt der Arbeitsbereich auf iOS einen ausblendbaren
  Hinweis mit „Weitere PDFs hinzufügen …“ (ADR 0004 Nachtrag 12 Punkt 13, mobile-ios.md §8.6).
- **Nur durch Tests belegt (braucht A-Trust):** Anmeldung, SMS-TAN, App-Bestätigung, Vergleichswert-Kästchen,
  „Signatur fortsetzen“ nach App-Kill mit ID Austria.
- **Texte und Randfälle** (Stapelwortlaut beim Keystore – seit §12 nur am Desktop – bzw. „Jedes Mal fragen“,
  „Einrichtung abgeschlossen“ nach „Später einrichten“, „Bis Programmende“; Automatik im seitenfüllenden
  Signaturfeld, Toast über dem Bottom-Sheet, Fokus nach Miniatur, nachträgliche QR-Suche und doppelter Empfang,
  aria-live nach Dokumentwechsel, Name nach „Sichern unter“, fortgesetzte Stapelzeile): **behoben** in ADR 0004
  Nachtrag 12 (Desktop und Mobile, je mit Test).
- **Offen (Meldung Auftraggeber, iOS):** Zieht man den Block auf eine Seite mit einem seitenfüllenden leeren
  Signaturfeld (z. B. letzte Seite von `Testdokument.pdf`, `manySignFields-xref-stream.pdf`), übernimmt der
  Feld-Magnet das Feldrechteck und der Block füllt die ganze Seite. Seit Nachtrag 12 legt die Automatik solche
  Dokumente auf die neue Seite – das Zurückziehen auf die letzte Seite löst es dann sofort aus. Nicht Teil dieses
  Stands; die Behebung (nur geeignete Felder als Ziel) entsteht getrennt (ADR 0004 Nachtrag 13).
- **Barrierefreiheit:** VoiceOver und TalkBack nicht akustisch geprüft (nur Namen, Rollen, Struktur und axe);
  Dynamic Type bis 200 %, `px`-Maße wachsen nicht mit; `.link` 24 px (WCAG 2.5.8 erfüllt, bewusst so). Die
  waagrechte Bereichsnavigation der Einstellungen rollt den aktiven Bereich jetzt ins Bild (Nachtrag 12).
- **Nicht abgedeckt:** echte Geräte, Android 9–14, Release-Builds; Überlappungen in der Bereichsleiste nur in
  `adb screencap` (vermutlich Grafikartefakt des Emulators); `general.last_source_directory` bleibt nach einem
  iOS-Update ein Pfad im alten Container (auf Mobile ungenutzt).

## 12. Mobile nur mit ID Austria (16.10.2026)

Entscheidung des Auftraggebers: „Signieren mit Keystore brauchen wir auf mobilen Apps nicht, da reicht ID Austria.“
Plan §5.2 sah den PKCS#12-Import in die App-Sandbox vor – er entfällt. Auf iOS und Android ist ID Austria das einzige
Signiermittel (App-Bestätigung bzw. SMS-TAN, FIDO2 ausgegraut wie bisher); der Desktop behält den Keystore unverändert.
Regeln und Vertrag: ADR 0004 Nachtrag 11 (kein neues Feld, keine neue Variante – der Kern liefert auf Mobilgeräten keine
Keystore-Option und keine Keystore-Angaben); Folgen für die Geräteprotokolle: mobile-ios.md §9, mobile-android.md §9.

### 12.1 Was sich auf iOS/Android ändert

| Bereich | vorher (Mobile 1.0 bis 16.10.2026) | jetzt |
|---|---|---|
| Signiermittel (Auswahl, Konto-Chip, Stapel) | ID Austria und Keystore (nach Import), „PKCS#12-Datei importieren …“ im Menü | nur ID Austria; Konto-Chip ohne „Signiermittel wechseln“, Menü nur „Einstellungen …“ (seit 17.10.2026 gar kein Konto-Chip, §13) |
| Standard-Signiermittel | Einstellung (`backend.default`), Ersteinrichtung mit Auswahl | immer ID Austria; Ersteinrichtung ohne Signiermittel-Schritt |
| Einstellungen › Signiermittel | Standard, ID Austria, FIDO2-Hinweis, Keystore (Import-Assistent, Schlüssel, Speicherart „Sicherer Speicher“) | ID Austria (Telefonnummer/Benutzername, „merken“) und FIDO2-Hinweis; Suche findet keine Keystore-Einträge |
| „Passwort merken?“ (Keystore) | beim ersten Keystore-Passwort | entfällt |
| Dateiauswahl der Shell | `pdf` und `pkcs12` | nur `pdf` (`pkcs12` → `ng-ui-event-invalid`); Plugin unverändert |
| Übernahme aus PDF-Over | Keystore samt Passwörtern | ohne Keystore, Standard ID Austria, Übernahmeliste erklärt es |
| Datenschutz (Einstellungen) | „Mit einem Keystore wird auf diesem Gerät signiert …“ | „Auf diesem Gerät wird mit ID Austria signiert; dazu wird das Dokument an A-Trust übermittelt.“ |

### 12.2 Übergang von früheren Mobile-Builds

- **Einstellungen:** `backend.default = keystore` (bzw. `local_bku`) gilt beim Lesen als `id_austria`; `[keystore]`
  bleibt in `settings.toml` unverändert stehen und wird ignoriert (nicht gelöscht); `keystore.*` lässt sich auf dem Gerät
  nicht mehr ändern (`ng-settings-unavailable-mobile`).
- **Sicherer Speicher:** Keystore-Passwörter (Dienst „PIDA“, Konten `keystore-store:`/`keystore-key:` zur Datei aus den
  Einstellungen und zu jeder Datei in `data/keystores`; Schlüsselpasswörter zum eingestellten, zum leeren und – ist das
  Passwort der Datei noch gespeichert – zu jedem Alias der Datei) entfernt der Kern beim ersten Start einmalig; Merker
  `data/keystore-secrets-removed`. Andere Einträge (Proxy, fremde Apps) bleiben. Ohne dauerhaften Speicher bzw. nach
  einem Fehler beim Löschen versucht er es beim nächsten Start erneut.
- **Dateien:** `data/keystores/*.p12` bleiben liegen (durch ihr Passwort geschützt, weiter von der Sicherung
  ausgenommen, R-5); `relocate_sandbox_files` ordnet nur noch die Bildmarke neu zu.
- **„Signatur fortsetzen“:** ein gesicherter Keystore-Vorgang wird beim Start mit Hinweis `ng-resume-signer-not-offered`
  verworfen statt angeboten; ein ID-Austria-Vorgang wird wie bisher angeboten.
- **Ereignisse:** `import_keystore`, `inspect_keystore`, „Passwort merken?“ und jede Wahl des Keystores sind ohne Wirkung
  (Log-Notiz, keine Sitzung, kein Fehler), ebenso `select_signer` mit der Chipkarte (sonst gewählt bzw. mit
  `make_default` als Standard gespeichert); ein Passwort im verworfenen Ereignis wird sofort überschrieben.

### 12.3 Nachweise

Kern (`cargo test -p pida-core-api`):

- `mobile_signers.rs`: nur der Desktop bietet den Keystore an; Abbildung des Standards ohne Änderung an `[keystore]`;
  Übernahme ohne Datei, Alias und Keystore-Passwörter (Proxy-Passwort bleibt, keine Werte im `Debug`); Erkennung der
  Keystore-Ereignisse; Kennungen der zu entfernenden Passwörter (`/var` ↔ `/private/var`, importierte Dateien, Alias).
- `backends.rs::registry_offers_only_id_austria_on_mobile` (auch mit Overlay und fest vorgegebenen Backends; Desktop
  unverändert), `config.rs::mobile_default_signer_is_id_austria` (Richtlinie, „Jedes Mal fragen“),
  `settings_apply.rs` (Mobile-Sperren für `keystore.*` und den Standard; Desktop unverändert).
- `tests/mobile.rs`: `keystore_of_an_earlier_mobile_build_is_ignored_and_its_passwords_removed_once` (iOS: nur ID
  Austria als Standard, Passwörter einmalig entfernt, `[keystore]` bleibt, Import/Prüfung/Wahl/Stapel mit Keystore ohne
  Wirkung), `desktop_keeps_the_keystore_of_the_same_settings` (Gegenprobe Linux), `keystore_settings_of_an_earlier_build_do_not_start_id_austria_automatically`
  (F-54, iOS und Linux), `keystore_resume_of_an_earlier_build_is_discarded` (Android), `migration_on_mobile_takes_over_no_keystore`
  (Android und Linux), `pkcs12_import_is_without_effect_on_mobile` (iOS und Android), `mobile_backend_list_has_only_id_austria_and_marks_fido2`,
  `output_goes_to_app_documents_and_can_be_shared_and_exported` (jetzt mit ID Austria); die bisherigen Import-Tests laufen
  als Desktop-Tests weiter.

Oberfläche (`npm test`, Mock-Kern mit `platform: ios|android`):

- `app/MobileIdAustriaOnly.test.tsx` je Plattform: Arbeitsbereich mit Konto-Chip und Menü, Signierkarte, jeder
  Einstellungsbereich und die Suche (`keystore`, `pkcs12`, `p12`, `zertifikatsdatei`, `alias`, `schlüsselbund`),
  Ersteinrichtung ohne und mit Altkonfiguration, Stapel, „Fertig“ – nirgends „Keystore“, „PKCS“, „Datei mit Zertifikat“,
  „Zertifikatsdatei“, „Passwort merken“, „Sicherer Speicher“, „Schlüssel anzeigen“; jeweils axe ohne Verstöße;
  Desktop-Gegenprobe (Keystore im Menü und in den Einstellungen).
- `app/Mobile.test.tsx` (Konto-Chip ohne Wechsel), `features/settings/{settings,keystore,a11y,units}.test.tsx`,
  `features/onboarding/onboarding.test.tsx` („Schritt 1 von 1“, Übernahme mit Hinweis), `features/signing/signing.test.tsx`
  (Rückfall ohne Import), `core/contract.test.ts` (keine Mobile-Probe mit Keystore).

Prüfungen: siehe 12.4.

### 12.4 Prüfungen und Offenes

**Prüfungen (Stand `mob2/core`, ab `main` 8606b0c):** `cargo test --workspace --no-fail-fast` 1435 bestanden,
0 fehlgeschlagen, 5 ignoriert; `cargo clippy` wie die CI (`--workspace --exclude` je Crate aus `CLIPPY_LENIENT`
`--no-deps --all-targets --locked -- -D warnings`) ohne Befund – ohne die Ausnahmen bricht der Lauf weiter nur an den
bekannten `dead_code`-Stellen in `pida-pades` (`signed.rs`, Feld `written`) und `pida-backend-keystore`
(`tests/backend.rs`, `copy_to`), unverändert seit Welle 3 (docs/entwicklung.md §5); `cargo fmt --all --check`;
`npm ci && npm run build && npm test` 33 Dateien, 700 Tests; TypeScript (`ts_bindings`) und Proben (`serde_contract`)
neu erzeugt – nur Doku-Kommentare, Probe `keystore_imported` (vorher `mobile_keystore_imported`) und `ws_batch_mobile`
mit nur ID Austria; `python3 tools/rename-to-pida.py --check` ohne Befund; `cargo check -p pida-app` für
`aarch64-apple-ios-sim` und `aarch64-linux-android` (NDK 29) grün, je eigenes `CARGO_TARGET_DIR`.

**Adversariales Review (Stand 4080126, 16.10.2026):** Behauptungen des Umsetzers am Code und mit eigenen Proben
nachgeprüft (Kern: Ereignisse, alte Einstellungen, `SaveSettings`, Übernahme, „Fortsetzen“, Richtlinie, sicherer
Speicher; Oberfläche: alle Ansichten mit Einstellungen eines früheren Builds). Behoben:

- `select_signer` mit der Chipkarte wählte sie auf Mobilgeräten im Konto-Chip aus und speicherte mit `make_default` den
  Standard `local_bku` in die Datei (niedrig) – jetzt ohne Wirkung wie beim Keystore (`mobile_signers::ignored_on_mobile`,
  Test in `tests/mobile.rs`; Mock gleichgezogen).
- Schlüsselpasswörter zu einem früher gewählten, nicht mehr eingestellten Alias blieben im sicheren Speicher (niedrig) –
  der Kern liest die Aliase der Datei mit ihrem noch gespeicherten Passwort (`stored_key_aliases`, Tests).
- Passwörter verworfener Keystore-Ereignisse wurden ohne Überschreiben freigegeben (niedrig) – `discard_event`.
- Doppeltes `.keywords` an `ng-ui-set-updates` (DE/EN, seit B3): die Desktop-Suche fand „Updates“ bei „Keystore“, aber
  nicht bei „Aktualisierung“ (niedrig, nur Desktop) – entfernt, Tests in `units.test.ts` und `i18n.test.ts` (kein
  Attribut doppelt).
- Die wörtliche Prüfung `cargo clippy --workspace --all-targets -- -D warnings` brach an drei alten `dead_code`-Stellen
  (`pades/src/signed.rs` Feld `written`, `pades/tests/e2e_corpus.rs` Feld `key_stem`, `backend-keystore/tests/backend.rs`
  `copy_to`) – begründet (`#[allow(dead_code, reason = …)]`) bzw. entfernt; der Lauf ist jetzt ohne Befund
  (`CLIPPY_LENIENT` in der CI unverändert).
- Veraltete Kommentare (`core/types.ts`, `styles/components.css`).

Prüfungen nach dem Review: `cargo test --workspace --no-fail-fast` 1436 bestanden, 0 fehlgeschlagen, 5 ignoriert;
`cargo clippy --workspace --all-targets -- -D warnings` und `cargo fmt --all --check` ohne Befund; `npm ci && npm run
build && npm test` 33 Dateien, 704 Tests; `ts_bindings` und `serde_contract` unverändert grün (Vertrag unberührt);
`python3 tools/rename-to-pida.py --check`; `cargo check -p pida-app` für `aarch64-apple-ios-sim` und
`aarch64-linux-android` grün. Kein Simulator bzw. Emulator gestartet.

Ohne Befund: Desktop-Pfade (Registrierung, Standard, Einstellungen, Ersteinrichtung, Konto-Chip, Mock) verhalten sich wie
vorher; `SaveSettings`/`ApplySettings` lassen `[keystore]` auf dem Gerät unverändert; keine Mobile-Probe des Vertrags
nennt einen Keystore; das Plugin bietet `pkcs12` weiter an, die Shell lehnt es ab.

**Offen:**

- **Neuinstallation unter iOS:** Die Keychain überdauert das Löschen der App, der Datenordner nicht. Wer einen früheren
  Build mit gespeichertem Keystore-Passwort löscht und neu installiert, behält diese Einträge (Dienst „PIDA“) – ohne
  Einstellungen kennt der Kern die Konten nicht, und der sichere Speicher des Plugins kann nicht aufzählen. Die Einträge
  werden nie gelesen; entfernen ließen sie sich nur mit einer neuen Plugin-Methode (z. B. „alle `keystore-`-Konten
  löschen“).
- **Am Gerät (17.10.2026 nachgeholt):** iOS (iPhone 17e, mobile-ios.md §10) und Android (Emulator,
  mobile-android.md §10) – Ersteinrichtung, alle Ansichten samt Einstellungssuche ohne Keystore-Begriffe und je ein
  Update von einem Build mit Keystore als Standard; Ergebnis je Plattform und die behobenen Befunde in 12.5.
- **Liegengebliebene Dateien:** `data/keystores/*.p12` früherer Builds bleiben auf dem Gerät (bewusst nur ignoriert);
  ob sie später gelöscht werden sollen, entscheidet der Auftraggeber.
- **Aufräumen im Vertrag:** `UiEvent::ImportKeystore`/`DismissKeystoreImport`, `KeystoreImportView` und der
  Import im Kern werden von keiner Oberfläche mehr genutzt (der Desktop wählt die Datei direkt); sie bleiben, bis der
  Vertrag einmal bereinigt wird (§3.0: nur additiv).

### 12.5 Am Gerät (17.10.2026): Ergebnis je Plattform

Zwei Gerätedurchgänge auf dem Integrationsstand `826fe82` (Mobile nur ID Austria, Randfälle aus ADR 0004 Nachtrag 12,
„+ PDF“, „In Bearbeitung“, automatische Positionierung): iOS auf dem Simulator **iPhone 17e** (iOS 27, Branch
`mob2/ios`, mobile-ios.md §10, Aufnahmen `screenshots/ios-ng2/`) und Android auf dem Emulator `pdfover_api35` <!-- rename-to-pida:keep -->
(Android 15, Branch `mob2/android`, mobile-android.md §10, Aufnahmen `screenshots/android-ng2/`).
Beide ohne Verbindung zu A-Trust (Proxy `127.0.0.1:9`, Flugmodus bzw. lokales „schwarzes Loch“) und ohne
Zugangsdaten; Simulator und Emulator danach heruntergefahren. Der Prüfmodus war nicht Gegenstand (Entscheidung vom
17.10.2026, ADR 0004 Nachtrag 15).

| Nr. | Ablauf | iOS (iPhone 17e) | Android (Emulator) |
|---|---|---|---|
| 1 | Frische Installation, Ersteinrichtung | ok: „Schritt 1 von 1“ nur mit dem Signaturbild, kein Signiermittel-Schritt; „Später einrichten“ mit Hinweis auf die Einstellungen | ok: ebenso nach `pm clear`; Merker `keystore-secrets-removed` gesetzt (`removed=0`) |
| 2 | Update von einem Build mit Keystore als Standard | ok: Review-Build 1.0.0 mit importiertem `.p12` und `default = "keystore"` → Start mit ID Austria, „Zuletzt signiert“ übernommen, `removed=0` (die Keychain hatte keine PIDA-Einträge), Merker gesetzt, `settings.toml` byte-gleich, `.p12` bleibt liegen | ok: echtes Update von Build 8606b0c mit importiertem `.p12`, Passwort im sicheren Speicher und Keystore als Standard → `removed=2`, `pida_secure_store.xml` leer, Merker gesetzt, `settings.toml` beim Start unverändert (erst beim nächsten Speichern `default = "id_austria"`, `[keystore]` bleibt), `.p12` bleibt liegen |
| 3 | Absuche nach Keystore, Zertifikatsdatei, „Passwort merken?“ | ok: Text und Attribute aller Ansichten (Start, acht Einstellungsbereiche, Suche, Befehle, Hilfe, Arbeitsbereich, Konto-Chip, Signier-, Fehler- und Stapelkarte, Startdialog) ohne Treffer; die Dokumentauswahl zeigt das `.p12` ausgegraut | ok: ebenso frisch und nach dem Update; Einstellungssuche und Befehlspalette ohne Treffer, Konto-Chip-Menü nur „Einstellungen …“ |
| 4 | PDF empfangen, automatische Positionierung, „Diesmal selbst platzieren“, Stapel-Option | behoben (N7): Teilen aus „Dateien“ mit Hinweis zum Teilen erstmals am Gerät; Automatik, Freigabe und Ziehen per Touch, Stapel-Option mit Rückfrage | behoben (C4): `VIEW`-Intent, die Einstellung wirkt sofort auf das offene Dokument, „Diesmal selbst platzieren“, Stapel-Option mit Rückfrage |
| 5 | „+ PDF“ (weitere hinzufügen, anderes öffnen), Papierkorb | ok | ok |
| 6 | „Zur Startseite“ → „In Bearbeitung“, neues PDF bei offener Arbeit | ok: Parken ohne Rückfrage, „Weiter bearbeiten“ unverändert, Teilen fügt ohne Rückfrage hinzu (Nachtrag 10 Pkt. 4), „Dokument auswählen“ fragt „Zur offenen Arbeit hinzufügen?“ | ok: ebenso, „Schließen“ fragt bei eigener Platzierung nach |
| 7 | Randfälle (Nachtrag 12) | behoben (N1, N6, N8): Hinweis über dem Sheet oben, Live-Region ohne alte Lage, `qr.pdf` im QR-Platzhalter und kein zweiter Eintrag, Bereichsleiste bei großer Schrift | behoben (C2, C3): ebenso bei 130 % Schrift |
| 8 | ID Austria ohne Netz, Tastatur, dunkel, Querformat, große Schrift | behoben (N1–N5): „Verbindung wird aufgebaut …“ und „Die Zeit ist abgelaufen“ mit „Erneut versuchen“, Buchstaben-Tastatur im Benutzernamen-Feld, dunkel, Querformat, 200 % Schrift | behoben (C1): „Der Signaturdienst ist gerade nicht erreichbar“ (DNS-Fehler, A-Trust nie erreicht), Buchstaben-Tastatur, dunkel, Querformat 863 × 360, 130 % Schrift, AX-Baum ohne unbenanntes Bedienelement |

**Behobene Befunde** (nur Oberfläche und UI-Kataloge, Vertrag unberührt; wirken auf allen Plattformen, wo nicht anders
genannt):

- **Gleich auf beiden Geräten gefunden und gleich behoben:** Datenschutz-Kurzhinweis im Stapel in der Einzahl (iOS N6
  = Android C2) – Übersicht, Startdialog und Stapelkarte ab zwei Dokumenten „Die Dokumente werden zum Signieren an
  A-Trust übermittelt.“ (`ng-ui-backend-leaves-device-many`, `PrivacyNote count`); Rückfrage zur Stapel-Option bei
  einem Dokument in der Mehrzahl (N7 = C4) – „Eigene Platzierung behalten?“ / „Auch dieses automatisch platzieren“
  (`ng-ui-batch-auto-confirm-title`/`-replace` mit `$n`). Beim Zusammenführen blieben Code und Kataloge gleich; die
  Tests beider Durchgänge sind vereint (`batch.test.tsx`, `i18n.test.ts`).
- **iOS:** N1 Knöpfe der Signierkarte bleiben auf Mobile am unteren Rand stehen (`signing.css`); N2 Stapelkarte bei
  großer Schrift nicht breiter als das Telefon (`components.css`); N3 Zoomwert ohne Umbruch (`workspace.css`); N4
  `data-keyboard` bei eingeblendeter Bildschirmtastatur, die Signaturbild-Vorschau rollt mit (`app/keyboard.ts`,
  `settings.css`); N5 bis 480 px Breite nur das Logo, wenn die Kopfzeile „Zurück zum Stapel“ trägt (`shell.css`); N8
  Stapelabbruch ohne Signatur: „Stapel abgebrochen – es wurde noch kein Dokument signiert.“ (`ng-batch-aborted` `[0]`).
- **Android:** C1 bei Fingerbedienung zeigen React-Aria-Elemente außer Textfeldern den Fokusrahmen nur mit
  `data-focus-visible` – nach Bildschirmtastatur und Zurück-Geste kein Rahmen mehr, mit Hardware-Tastatur weiter
  (`base.css`); C3 „Überspringen“ im Startdialog in Einzahl/Mehrzahl nach der Zahl der vorhandenen Zieldateien
  (`ng-ui-batch-start-skip-desc(-local)` mit `$n`); `settings.test.tsx` („Liste leeren“) sucht nur den Toast (unter
  Last stand derselbe Text in der Live-Region).

**Nicht am Gerät belegbar** (ohne Keystore und ohne A-Trust kommt auf Mobile keine Signatur zustande): „Fertig“, der
„Sichern unter“-Hinweis mit dem tatsächlichen Namen, Teilen und Öffnen eines Ergebnisses, Anmelde- und TAN-Maske der
Signierkarte mit Tastatur, Vergleichswert, „Signatur fortsetzen“ eines Keystore-Vorgangs; die Keychain-Bereinigung lief
unter iOS nur ohne vorhandene Einträge (`removed=0`, unter Android mit `removed=2`). Belegt durch Kern- und
Oberflächentests (12.3, `core-api/tests/mobile.rs`, `signing.test.tsx`, `done.test.tsx`) und die Plugin-Unit-Tests.

### 12.6 Zusammenführung (`mob2/int`, 17.10.2026)

`mob2/ios` (a820da7, enthält schon `main` a04de88 mit der Entfernung des Prüfmodus, aufgelöst nach ADR 0004
Nachtrag 18; mobile-ios.md §10.5) per Fast-Forward, danach `mob2/android` (f6f57f4, ab `826fe82`): vier
Konfliktdateien (`batch.test.tsx`, `privacy.tsx`, `mobile-css.test.ts`, dieses Dokument) – nur Kommentare und eine
doppelte Prüfung, der Code beider Behebungen war gleich; `git merge main` danach ohne Änderung. Vertrag unberührt:
`ts_bindings`, `serde_contract` und die Real-Core-Proben (`workspace`, `batch`, `first_run`) mit `PIDA_BLESS=1` ohne
Änderung.

Nebenbei: `core-api/tests/workspace.rs::declined_in_the_app_offers_retry_in_the_same_session` scheiterte unter Last
gelegentlich (1 von 5 Einzelläufen; Test und Kern wie auf `main`): „Schließen“ verwirft die Sitzung asynchron, die
Prüfung erwartete den Abbruch schon, sobald der Arbeitsbereich stand. Sie wartet jetzt wie der Test darüber bis zu 1 s
auf den Abbruch (danach 15 von 15 Läufen grün).

**Prüfungen (Stand `mob2/int` nach beiden Merges):** `cargo test --workspace --no-fail-fast` 1483 bestanden,
0 fehlgeschlagen, 5 ignoriert; `cargo clippy --workspace --all-targets -- -D warnings` und `cargo fmt --all --check`
ohne Befund; `npm ci && npm run build && npm test` 34 Dateien, 710 Tests; `ts_bindings`, `serde_contract` und die
Real-Core-Proben mit `PIDA_BLESS=1` ohne Änderung; `python3 tools/rename-to-pida.py --check` ohne Befund; `cargo check
-p pida-app` für `aarch64-apple-ios-sim` und `aarch64-linux-android` (NDK, je eigenes `CARGO_TARGET_DIR`) grün.
macOS-Debug-Bundle mit `--config '{"identifier":"at.pida.smoketest"}'` (übergibt nicht an eine laufende PIDA),
Startprüfung `PIDA_SMOKE=1` mit leerem `PIDA_HOME`/`HOME` dreimal `pida-smoke: ok step=open pdfium=true`
(445/448/452 ms). Für die Zusammenführung kein Simulator bzw. Emulator gestartet.

**Offen bzw. zur Entscheidung des Auftraggebers** (Einzelheiten in mobile-ios.md §10.3 und mobile-android.md §10.3):

- **Datenschutzinformation auf Mobile:** Der übernommene PDF-Over-Text in Einstellungen › Datenschutz nennt auch auf
  iOS/Android „Der Signaturvorgang mit der Chipkarte wird lokal durchgeführt.“ – Bereich des parallelen Laufs
  (Online-Prüfung/Datenschutz), nicht geändert; Vorschlag: den Satz auf Mobilgeräten weglassen.
- **Duplikate nach Nachtrag 17 (Merge-Entscheidung, Nachtrag 18 Pkt. 3):** Am Desktop sind inhaltsgleiche Dateien in
  verschiedenen Ordnern zwei Dokumente (der Inhaltsvergleich aus Nachtrag 12 entfiel); eine eben signierte Datei mit
  demselben Pfad bleibt am Desktop in „Fertig“ und wird als Duplikat gemeldet, nur Kopien auf Mobilgeräten öffnen als
  nächstes Dokument. Zur Bestätigung.
- **Stapel-Option ohne Einstellung:** Einträge ohne eigenen Plan stehen schon vor dem Einschalten auf „Bereit ·
  automatisch platziert“; der Schalter ändert sichtbar nur Einträge mit Vorschau-Plan – ob der Hinweistext das klarer
  sagen soll.
- **Stapel mit nur noch einem Dokument** bleibt Stapel („Alle Dokumente werden in einem Durchgang …“) – Wortlaut,
  niedrig.
- **Liegengebliebene Dateien:** `data/keystores/*.p12` früherer Builds (siehe 12.4); eine frische Installation legt
  zudem einen leeren `data/keystores` an und schreibt `[keystore]` mit Standardwerten – unsichtbar, ohne Wirkung.
- Klein, nicht geändert: Die Beschreibung des Signaturblocks für Bildschirmleser nennt auch auf dem Telefon
  Tastenkürzel; der Hinweis „Signaturblock automatisch … platziert“ ist bei großer Schrift oben angeschnitten
  (kurzlebig); in der schmalen Android-Kopfzeile war der Dokumentname neben „Signatur prüfen“ auf 1–2 Zeichen gekürzt –
  mit der Entfernung des Prüfmodus frei geworden, am Gerät noch nicht wieder angesehen.
- Aufnahmen mit dem Android-Emulator künftig mit `-gpu host` (mit `swiftshader_indirect` doppelte Bildteile in
  `adb screencap`, kein Fehler der App).

## 13. Keine Signiermittel-Auswahl (17.10.2026)

Meldung des Auftraggebers (iPhone, Fuß des Arbeitsbereichs mit dem Auswahl-Chip „ID Austria · a···er ⌄“ neben
„Signieren“): „In der mobilen App brauchen wir die Signiermittelauswahl nicht, es gibt nur ID Austria als einziges
Mittel.“ Regeln und Vertrag: ADR 0004 Nachtrag 20 (additives Feld `SignersView::choice_offered`, der Kern entscheidet).

### 13.1 Was sich ändert

| Bereich | vorher (Nachtrag 11) | jetzt |
|---|---|---|
| Arbeitsbereich, Bottom-Sheet (Telefon) | Konto-Chip „ID Austria · Konto ⌄“ neben „Signieren“, Menü nur „Einstellungen …“ | kein Konto-Chip; „Signieren“ über die volle Breite, Kurzhinweis „wird an A-Trust übermittelt“ darüber |
| Arbeitsbereich breit (Tablet, Querformat) | Abschnitt „Signieren mit“ mit Konto-Chip | Abschnitt entfällt; Kurzhinweis an seiner Stelle oben im rollbaren Teil (nicht im festen Fuß – im Querformat mit großer Schrift bliebe sonst kaum Platz für Signaturbild und Optionen) |
| Stapelübersicht | „PDF hinzufügen“, Konto-Chip, „Alle signieren“ | „PDF hinzufügen“ und „Alle signieren“ (schmal je volle Breite) |
| Ersteinrichtung, Signierkarte | ohne Signiermittel-Schritt; Rückfall „Signiermittel wählen“ mit einer Option | unverändert ohne Schritt; Rückfall „Weiter mit ID Austria“ ohne Liste |
| Konto (Benutzername/Telefonnummer) | im Konto-Chip | in der Signierkarte beim Anmelden; geändert unter Einstellungen › Signiermittel (unverändert) |

Am Desktop gilt dieselbe Regel, wenn eine Richtlinie nur ein Signiermittel freigibt bzw. vorschreibt; mit mehreren
Signiermitteln bleibt dort alles wie bisher (Konto-Chip, „Signiermittel wechseln“, „Jedes Mal fragen“). Gibt es gar
kein Signiermittel (Richtlinie schließt ID Austria aus), sperrt der Kern „Signieren“ und „Alle signieren“
(`actions`), auch in der Befehlspalette und für ⌘↵.

### 13.2 Nachweise

- Kern: `tests/mobile.rs::only_id_austria_on_mobile_offers_no_signer_choice` (iOS, Android, Gegenprobe Linux),
  `tests/workspace.rs::{single_signer_by_policy_offers_no_choice, single_unavailable_signer_names_the_reason,
  single_unavailable_signer_in_the_batch_names_the_reason, no_signer_at_all_locks_sign_and_start_batch}`,
  `workspace.rs::{eligible_signers_follow_policy_not_availability, choice_offered_defaults}`.
- Oberfläche (Mock-Kern mit `platform: ios|android` bzw. `allowedBackends`): `app/MobileIdAustriaOnly.test.tsx`
  (alle Ansichten, Befehlspalette samt Suche, Kürzelübersicht, Ersteinrichtung, Stapel – nirgends Konto-Chip, Menü,
  „Signiermittel wählen/wechseln“; axe), `app/Mobile.test.tsx`, `app/SingleSigner.test.tsx` (Desktop per Richtlinie
  und Gegenprobe mit mehreren), `features/signing/signing.test.tsx`, `app/RealCore.test.tsx`, `core/contract.test.ts`,
  `styles/mobile-css.test.ts`.
- Sichtprüfung im Mock-Modus (Vite auf Port 1437, Telefon-Viewport 375 × 812 mit Touch-Emulation, hell und dunkel,
  Desktop; axe-core 4.14 im Browser ohne neue Befunde – `label-content-name-mismatch` nur an den bekannten Stellen
  aus N-44 und am Zähler von „Alle signieren“, unverändert): `screenshots/mobile-signer/` –
  `ios-01-arbeitsbereich-eingeklappt.jpg`, `ios-02-arbeitsbereich-ausgeklappt.jpg`, `ios-03-signierkarte-konto.jpg`,
  `ios-04-stapel-ohne-konto-chip.jpg`, `android-01-arbeitsbereich-eingeklappt.jpg`, `android-02-stapel-dunkel.jpg`,
  `desktop-01-richtlinie-nur-id-austria.jpg`, `desktop-02-mehrere-signiermittel-gegenprobe.jpg`. Kein Simulator bzw.
  Emulator gestartet, kein Gerät benutzt, keine Anfrage an A-Trust.
- Nachprüfung (Review, 17.10.2026; Vite auf Port 1447): Querformat 812 × 375 mit 200 % Schrift (`html { font-size:
  32px }` wie `TextSize.swift`) – mit dem Kurzhinweis im festen Fuß blieben für Signaturbild, Optionen und Platzierung
  rund 40 px, jetzt wieder so viel wie mit Auswahl: `ios-05-querformat-200-prozent.jpg`; `desktop-01-…` neu
  aufgenommen (Kurzhinweis oben). Hochformat 375 × 812 mit 200 %: Bottom-Sheet ein- und ausgeklappt, Stapel (beide
  Knöpfe je volle Breite, nicht abgeschnitten) ohne Befund. Desktop mit mehreren Signiermitteln: Konto-Chip, Menü
  „Signiermittel wechseln“ mit „Einstellungen …“, nach Wahl der Chipkarte „Als Standard verwenden“ – unverändert.

### 13.3 Prüfungen

`cargo test --workspace --no-fail-fast` 1509 bestanden, 0 fehlgeschlagen, 5 ignoriert (ein erster Lauf parallel zu
`npm test` und `vite build` scheiterte einmal an der Zeitprüfung `workspace.rs::layout_and_overlap_check_read_only_the_grid`
– Zähler des PDFium-Scans unter Last, unabhängig von dieser Änderung; einzeln dreimal und im zweiten Gesamtlauf grün);
`cargo clippy` wie die CI (`--workspace` ohne `CLIPPY_LENIENT`, `--no-deps --all-targets --locked -- -D warnings`) und
`cargo fmt --all --check` ohne Befund; `npm ci && npm run build && npm test` 36 Dateien, 732 Tests (die Warnung zur
Chunk-Größe über 900 kB gab es schon vorher: 911,7 kB auf `main`, jetzt 914,0 kB); `ts_bindings` und `serde_contract`
mit `PIDA_BLESS=1` neu erzeugt und ohne `PIDA_BLESS` grün, ebenso die Real-Core-Proben (`workspace`, `batch`,
`first_run`); `python3 tools/rename-to-pida.py --check` ohne Befund; `cargo check -p pida-app` für
`aarch64-apple-ios-sim` und `aarch64-linux-android` (NDK 29, je eigenes `CARGO_TARGET_DIR`) grün.

Nach dem Review (Kurzhinweis breit im rollbaren Teil, Freigaben ohne Signiermittel im Kern, Grund auch bei „Alle
signieren“): `cargo test --workspace --no-fail-fast` 1511 bestanden, 0 fehlgeschlagen, 5 ignoriert; Clippy wie die CI
und `cargo fmt --all --check` ohne Befund; `npm ci && npm run build && npm test` 36 Dateien, 734 Tests (mehrfach
hintereinander grün); `ts_bindings` neu erzeugt (nur die Doku von `ActionsView::sign`/`start_batch`), Rename-Prüfung
und Mobile-`cargo check` (iOS-Simulator, Android) grün.
