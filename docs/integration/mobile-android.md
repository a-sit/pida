# Android – Paket M-Android (Projekt, native Plugin-Seite, Nachweise)

Stand: 15.10.2026, Welle Mobile, Paket **M-Android**; neue Oberfläche am Gerät: §8 (16.10.2026). Grundlage: docs/integration/mobile.md (Fundament M0,
Vertrag des Plugins §2, Lebenszyklus §3), Plan §5.2/§5.3/§7.7, ADR 0001.

Ergebnis: Die App läuft im Emulator `pida_api35` (Android 15, arm64). PDFium wird zur Laufzeit
geladen, Dokumente kommen über `VIEW`/`SEND` (content://) und die System-Dateiauswahl in die App, der
PKCS#12-Keystore wird importiert, das Passwort liegt verschlüsselt mit einem Android-Keystore-Schlüssel,
ein Test-PDF wird in der App mit dem Keystore signiert, über „Sichern unter …“ (SAF) exportiert und ist laut
pyHanko gültig (§6).

> **Seit 16.10.2026: Mobile nur mit ID Austria** (Entscheidung des Auftraggebers, ADR 0004 Nachtrag 11,
> mobile.md §12). Die Keystore-Abläufe dieses Protokolls – PKCS#12-Import über SAF, Passwort im Android-Keystore-
> verschlüsselten sicheren Speicher, Signatur und Stapel mit dem Keystore, Speicherart „Sicherer Speicher“ (§5
> Keystore-Import/Signatur, §8.2 Abläufe 3/4, §8.3 B6–B8, B10, B15, B16, §8.5) – **entfallen**; sie bleiben als
> Nachweis des damaligen Stands stehen. Auf Android gibt es seitdem nur ID Austria; der Kern ignoriert
> Keystore-Einstellungen früherer Builds und entfernt deren Keystore-Passwörter aus dem sicheren Speicher einmalig
> (§9).

## 1. Projekt `apps/app/src-tauri/gen/android`

Angelegt mit `npm run tauri android init --ci` (Tauri CLI 2.12.3) und versioniert (vorher in
`apps/app/src-tauri/.gitignore` ausgeschlossen; Build-Ausgaben ignorieren die `.gitignore` des Projekts).

| Punkt | Wert / Umsetzung |
|---|---|
| `applicationId`, `namespace` | `at.asit.pida.app` (Paketname bei Google Play seit 0.6.0; 0.5.3–0.5.5 `at.asit.pida`, bis 0.5.2 `at.pida`). Die Plugin-Klassen liegen in `at.asit.pida.mobile`. |
| SDK | `minSdk 28` (Android 9, Plan E-15), `targetSdk 36` (seit 0.5.3, Play-Vorgabe), `compileSdk 36` (die Tauri-API-Bibliothek `.tauri/tauri-api` verlangt 36; Plattform 36 und Build-Tools 36 lädt AGP beim ersten Build selbst). |
| Werkzeuge | AGP 9.3.1, Kotlin 2.2.10, Gradle 9.6.1 mit `distributionSha256Sum` (Prüfsumme von services.gradle.org) und `networkTimeout=60000` – der Wrapper brach den Download sonst nach 10 s ab. |
| Manifest | Intent-Filter `VIEW` (`content://`, `application/pdf`), `SEND` und `SEND_MULTIPLE` (`application/pdf`); Activity `singleTask` mit `configChanges` für Drehen, Dark Mode, Sprache, Schriftgröße, Tastatur; `windowSoftInputMode=adjustResize`. Berechtigungen nur `INTERNET` und `VIBRATE` (Haptik beim Positionieren), **keine** Speicher-Berechtigungen. |
| Klartext | `usesCleartextTraffic=false` im Release **und** Debug; nur `tauri android dev` (Dev-Server im LAN) braucht `PIDA_ANDROID_DEV_CLEARTEXT=1` beim Build. |
| Sicherung | `allowBackup=false`, `fullBackupContent` und `dataExtractionRules` schließen alle Bereiche aus (Cloud-Sicherung und Gerätewechsel): Einstellungen, importierter Keystore, verschlüsselte Secrets bleiben auf dem Gerät. |
| FileProvider | App: `${applicationId}.fileprovider` (wry, nur `Pictures/` für Kameraaufnahmen der WebView – die Vorlage gab den ganzen externen Speicher frei). Plugin: `${applicationId}.pida.files` (`PidaFileProvider`, nur `files-path Documents/` = Ausgabeordner `filesDir/Documents`, Review R-3) für „Teilen“ und „Öffnen“. Beide nicht exportiert, Lesezugriff nur per Einzelfreigabe. |
| Datei-Zuordnung | `tauri.android.conf.json` setzt `bundle.fileAssociations = null`: der von Tauri erzeugte Filter (`VIEW`/`SEND` mit `BROWSABLE`, implizit `file:`) entfällt, der Block zwischen den Markierungen im Manifest bleibt leer (Review R-6). |
| PDFium | `sourceSets.main.jniLibs.srcDir(tools/pdfium/android/jniLibs)` (aus `tools/fetch-pdfium.sh --jnilibs`, SHA-256 in `tools/pdfium.lock`); anderer Ort mit `-Ppida.pdfiumJniLibs=<pfad>`. Fehlt die Bibliothek, warnt Gradle; die App startet dann ohne Vorschau. Der Kern lädt `libpdfium.so` mit `dlopen` aus `nativeLibraryDir`. |
| TLS | Kotlin-Komponente `org.rustls:rustls-platform-verifier:0.2.0` (Maven-Archiv des Projekts auf GitHub, Repository mit `includeGroup("org.rustls")` im Wurzelprojekt). **Achtung:** Die README des Crates nennt die Gruppe `rustls`, im Archiv heißt sie `org.rustls`. Das Plugin bricht den Build ab, wenn `rustls-platform-verifier-android` in `Cargo.lock` eine andere Version hat. SHA-256 der AAR (Gradle-Cache): `aa021794230fbc2f0be355999e2cf67398dd563de066a2e17001ffbd0b69101b`. |
| Symbole, Farben | Icons aus `apps/app/src-tauri/icons/android` (adaptiv), Fenster- und Leistenfarben wie die Tokens der Oberfläche (`window_background` = `--c-bg` `#F4F5F7`/dunkel `#0F1114`, `system_bar_top` = `--c-surface` `#FFFFFF`/dunkel `#171A1F`; danach meldet die Oberfläche die Farben selbst, §8.3 B1); Standardvorlage (Tauri-Logo, Hello-World-Layout, Android-TV-Eintrag) entfernt. |

### `MainActivity.kt`

- **Eingehende Intents prüfen**, bevor tao (`RunEvent::Opened`) und das Plugin sie sehen: nur `VIEW`/`SEND`/
  `SEND_MULTIPLE`, deren URIs **alle** `content://` fremder Anbieter sind. `file://`, `data:` (Text-`SEND`)
  und URIs der eigenen Provider werden verworfen (ACTION_MAIN) – sonst könnte ein expliziter Intent einer
  anderen App eigene Dateien der Sandbox als „Dokument“ einschleusen (die eigene App darf ihre Provider
  ohne Freigabe lesen; die Shell liest `file:`-URLs selbst).
- **Keine doppelte Auslieferung nach Neuerstellung:** Mit `savedInstanceState` oder
  `FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY` wird der ursprüngliche Intent durch ACTION_MAIN ersetzt (sonst öffnete
  ein Neustart aus „Zuletzt verwendet“ das alte Dokument erneut).
- **Systemleisten:** Android 15 (targetSdk 35) zeichnet immer randlos; die WebView meldet Status- und
  Navigationsleiste nicht als `env(safe-area-inset-*)` – im Querformat lag die Statusleiste über der
  Kopfzeile. Die Activity legt Statusleiste, Navigationsleiste, Display-Ausschnitt und Tastatur als Abstand
  um den Inhalt (Screenshot 07). Die Flächen der Leisten zeichnet sie in den Farben, die die Oberfläche nach
  jedem Wechsel Hell/Dunkel über die Brücke `PidaSystemBars.update(top, bottom, dark)` meldet (nur `#rrggbb`,
  `apps/app/src/app/systemBars.ts`), samt heller bzw. dunkler Symbole.
- **Schriftgröße zur Laufzeit:** `fontScale` steht in `configChanges`; `onConfigurationChanged` setzt `textZoom`
  der WebView (sonst galt eine geänderte Systemschrift erst nach einem Neustart).
- **Zurück-Taste/-Geste:** schließt den obersten Dialog der Oberfläche (Esc an React Aria, danach eine
  Zeigerbewegung am `document`, damit der wieder fokussierte Auslöser keinen Tastatur-Fokusrahmen samt Tooltip
  zeigt), sonst `moveTaskToBack` – die Activity wird **nicht** beendet. Unter Tauri beendet das Zerstören der einzigen
  Activity auch den Prozess (§7.1) und damit einen laufenden Signiervorgang.
- **Startprüfung (nur Debug-Builds):** Intent-Extra `pida.smoke` setzt `PIDA_SMOKE=1` vor dem Start des
  Kerns (`apps/app/src-tauri/src/smoke.rs`).

## 2. Native Plugin-Seite (`apps/app/plugins/pida-mobile/android`)

Dateien: `PidaMobilePlugin.kt` (Befehle), `SecureStore.kt`, `Documents.kt` (reine Logik, JVM-Tests in
`src/test`), Manifest (FileProvider, `<queries>` für PDF-Anzeige-Apps), `res/xml/pida_share_paths.xml`.
Befehle laufen auf dem UI-Thread (tauri `run_mobile_plugin` → Looper); Dateizugriffe und Verschlüsselung
laufen auf einem eigenen Thread (`pida-mobile-io`), die Antwort kommt von dort. Fehler nur als kurze
Codes, nie mit Pfaden, URIs, Schlüsseln oder Werten (ADR 0001 Regel 9).

| Methode | Android-Umsetzung | Fehlercodes |
|---|---|---|
| `secureStoreGet/Set/Delete` | AES-256-GCM, Schlüssel `pida-secure-store-v1` im Android Keystore (StrongBox, sonst TEE; nicht exportierbar). Werte als `v1:` + Base64(IV ‖ Chiffrat) in den privaten SharedPreferences `pida_secure_store`; Eintragsname SHA-256 des Schlüssels, Schlüssel zusätzlich als AAD. Unlesbare Einträge (Schlüssel verloren) gelten als fehlend und werden entfernt. `EncryptedSharedPreferences` ist abgekündigt und wird nicht verwendet. | `keystore-failed`, `store-write-failed` |
| `shareFile` | `ACTION_SEND` mit FileProvider-URI, `ClipData` und `FLAG_GRANT_READ_URI_PERMISSION` im System-Auswahldialog; PIDA selbst ist ausgeschlossen (`EXTRA_EXCLUDE_COMPONENTS`). | `not-found`, `not-shareable`, `no-app` |
| `openFile` (**neu**, §3) | `ACTION_VIEW` mit FileProvider-URI an eine fremde PDF-App (Ergebnis „Öffnen“, F-87). | `not-found`, `not-shareable`, `no-app` |
| `exportFile` | SAF `ACTION_CREATE_DOCUMENT` (Vorschlag = Dateiname), danach Kopie der Bytes über den `ContentResolver` (`"wt"`); halb geschriebene Dokumente werden gelöscht. Abbruch → `cancelled` → Rust `Ok(false)`. | `cancelled`, `export-failed`, `too-large`, `no-picker` |
| `importFile` | SAF `ACTION_OPEN_DOCUMENT` (`pdf`: `application/pdf`; `pkcs12`: `application/x-pkcs12`, `application/pkcs12`, `application/octet-stream` – der Kern prüft das Format), Kopie nach `destination/<n>/<Anzeigename>`, höchstens 512 MiB. | `cancelled`, `bad-kind`, `destination`, `no-picker` |
| `takePendingOpenDocuments` | Intents aus dem Konstruktor (Kaltstart) und `onNewIntent` werden vorgemerkt; die URIs kommen zusätzlich über tao als Argument. `PendingDocuments`: jede Übergabe **genau einmal** – URIs aus selbst gesehenen Intents nur aus der Warteschlange, übergebene URIs nur, wenn sie hier unbekannt sind (die Reihenfolge von Start-, Rückkehr- und `Opened`-Aufruf ist nicht festgelegt). Nur `content://` fremder Anbieter; Kopie wie `importFile`. | (Einzeldateien werden übersprungen) |

Laufende SAF-Auswahlen sind je Aufruf (`invoke.id`) gemerkt – ein verlorenes Ergebnis blockiert keine
späteren Auswahlen. Lebenszyklus: `onPause`/`onResume` meldet tao der Rust-Seite
(`WindowEvent::Suspended`/`Resumed` → `UiEvent::Backgrounded`/`Foregrounded`, M0); im Log
`Lebenszyklus suspended=true/false` bei jeder Dateiauswahl, ohne dass der Vorgang abbricht.

## 3. Erweiterung des Vertrags (gemeinsame Dateien, minimal)

„Öffnen“ des Ergebnisses ging unter Android nicht: `opener` übergibt `file://`-Pfade, die Android anderen
Apps verweigert (Hinweis „Konnte signiertes Dokument nicht öffnen“). Deshalb:

- `apps/app/plugins/pida-mobile/src/lib.rs`: `PidaMobile::open_file(path, mime)` → native Methode
  `openFile` `{ path, mimeType }` → `{}` (Tabelle in der Moduldoku, Vertragstest `open_file_contract`).
- `apps/app/src-tauri/src/hooks.rs` `TauriHooks::open_path`: auf Mobile zuerst das Plugin; unter Android ist
  dessen Fehler endgültig, **unter iOS** fällt die Shell bei jedem Fehler (Methode fehlt in der Swift-Seite)
  auf `opener` zurück – M-iOS kann `openFile` (QuickLook) ergänzen, ohne die Rust-Seite zu ändern.
- `apps/app/plugins/pida-mobile/src/tls.rs`: nur Doku (Maven-Gruppe `org.rustls`).
- `apps/app/src-tauri/.gitignore`: Zeile `/gen/android` entfernt.

## 4. Bauen

Pfad mit Leerzeichen: Gradle, NDK und Tauri CLI kamen damit zurecht; gebaut wurde trotzdem über den Symlink
`/tmp/pida-android` → Worktree (die CLI löst ihn wieder in den echten Pfad auf).

```sh
source tools/env.sh
tools/fetch-pdfium.sh --jnilibs                       # einmalig: tools/pdfium/android/jniLibs/<abi>/libpdfium.so
cd apps/app
export CARGO_TARGET_DIR="$PWD/../../target-android"   # eigenes Ziel (parallel zu iOS)
export CARGO_PROFILE_DEV_DEBUG=line-tables-only       # Debug-.so 163 MB statt mehrerer hundert
npm run tauri android build -- --debug --target aarch64 --apk
#  → src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk (187 MB)

# JVM-Unit-Tests der Plugin-Logik (ohne Gerät)
cd src-tauri/gen/android && ./gradlew :tauri-plugin-pida-mobile:testDebugUnitTest
```

Emulator und Startprüfung:

```sh
emulator -avd pdfover_api35 -no-window -no-audio -no-boot-anim -gpu swiftshader_indirect &   # echter AVD-Name, rename-to-pida:keep
adb wait-for-device; until [ "$(adb shell getprop sys.boot_completed | tr -d '\r')" = 1 ]; do sleep 2; done
adb install -r app-universal-debug.apk
adb shell am start -W -n at.asit.pida.app/.MainActivity --ez pida.smoke true
adb logcat -d | grep pida-smoke                  # Tag RustStdoutStderr (tao leitet stdout/stderr um)
# Dokument per VIEW (die Freigabe klappt nur als Shell-Nutzer, nicht unter `adb root`)
adb push testdata/pdf/generated/simple-xref-table.pdf /sdcard/Download/Vertrag.pdf
adb shell content call --method scan_volume --uri content://media --arg external_primary
adb shell am start -a android.intent.action.VIEW -d content://media/external/file/<id> -t application/pdf \
  --grant-read-uri-permission -n at.asit.pida.app/.MainActivity
adb emu kill
```

Die WebView ist in Debug-Builds über `adb forward tcp:9222 localabstract:webview_devtools_remote_<pid>`
(Chrome DevTools Protocol) erreichbar; so lassen sich Bedienelemente finden und antippen (`adb shell input`).

### 4.1 Release-Build (signiert)

Der Release-Schlüssel liegt **nie im Repository**. `app/build.gradle.kts` liest ihn nur, wenn
`PIDA_ANDROID_KEYSTORE_PROPERTIES` den **absoluten** Pfad einer Properties-Datei außerhalb nennt:

```properties
# storeFile absolut oder relativ zu dieser Datei
storeFile=/Users/<name>/.pida-release/pida-release.jks
storePassword=…
keyAlias=pida
# bei PKCS#12 gleich storePassword
keyPassword=…
# optional:
# storeType=pkcs12
```

Kommentare nur in eigenen Zeilen (`#` am Zeilenanfang): Java-Properties kennen keinen Kommentar hinter einem
Wert, `storeFile=… # …` gehörte samt Kommentar zum Wert.

Ohne Variable bleibt alles wie bisher: Release unsigniert (`app-universal-release-unsigned.apk`), Debug mit dem
Debug-Schlüssel. Fehlt die Datei, ein Eintrag oder der Keystore, bricht Gradle ab; die Meldung nennt nur
Schlüsselnamen, nie Werte. Ordner `~/.pida-release` mit 0700, Dateien mit 0600; dort liegt auch eine
`LIESMICH.txt` mit dem Fingerabdruck. Künftige Updates lassen sich nur mit demselben Schlüssel installieren –
Keystore und Passwort getrennt sichern.

Release-Schlüssel seit v0.2.0 (17.10.2026): `CN=PIDA, O=PIDA, C=AT`, RSA 4096, selbstsigniert, gültig bis
04.03.2054; Zertifikat SHA-256
`E6:8B:35:B1:CF:4D:65:4C:14:9F:FD:C8:92:11:5B:FC:00:60:FC:8B:4B:AC:9A:3B:C4:48:A3:3E:29:83:D2:47`.
Ein früher installierter Debug-Build (Debug-Schlüssel) muss vor der Installation deinstalliert werden.

```sh
PIDA_RELEASE_REMAP=1 source tools/env.sh                # ohne Benutzerpfade (docs/entwicklung.md §10)
tools/fetch-pdfium.sh --jnilibs
tools/third-party-notices.sh --pdfium android-arm64 --out target/notices-android   # mit Android-Bibliotheken
cd apps/app && npm ci
export CARGO_TARGET_DIR="$PWD/../../target-android"
export PIDA_ANDROID_KEYSTORE_PROPERTIES="$HOME/.pida-release/keystore.properties"
export PIDA_NOTICES_DIR=target/notices-android PIDA_REQUIRE_NOTICES=1   # „Lizenzen anzeigen“ in der App
npm run tauri android build -- --apk --target aarch64
#  → src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release.apk (nur arm64-v8a)
apksigner verify --verbose --print-certs <apk>          # SHA-256 = LIESMICH.txt
aapt dump badging <apk> | head -1                       # package at.asit.pida.app, versionName
strings -a "$CARGO_TARGET_DIR/aarch64-linux-android/release/libpida_app_lib.so" | grep -c "$HOME"   # 0
```

`PIDA_RELEASE_REMAP=1` setzt `CARGO_ENCODED_RUSTFLAGS` (`--remap-path-prefix`); die Link-Argumente, die Tauri für
Android über `CARGO_TARGET_<triple>_RUSTFLAGS` setzt (`-Clink-arg=-landroid`, `-llog`, `-lOpenSLES`), entfallen
dann ganz – Cargo nimmt nur die erste vorhandene Quelle. Die Crates binden `libandroid`/`liblog` selbst ein,
OpenSLES braucht PIDA nicht; nach einem Tauri-Update die `NEEDED`-Einträge (`llvm-readelf -d`) und die
16-KB-Ausrichtung der `LOAD`-Segmente (`llvm-readelf -l`, `0x4000`) erneut mit einem Build ohne Remap
vergleichen. Nachweis 17.10.2026 (ohne
`PIDA_ANDROID_KEYSTORE_PROPERTIES`, also `app-universal-release-unsigned.apk` wie bisher): dieselben
benötigten Systembibliotheken wie 0.2.0 (`libandroid`, `libdl`, `liblog`, `libm`, `libc`), 0 statt 814
Fundstellen von `/Users/` in `libpida_app_lib.so`, `LOAD`-Ausrichtung `0x4000` wie 0.2.0. Gradle bricht mit
relativem Pfad bzw. unvollständiger Datei ab („keine Datei unter diesem absoluten Pfad“, „es fehlt storePassword,
keyPassword“). Mit einem Test-Keystore (Review 17.10.2026, `assembleArm64Release --info`): signiert, Passwörter
weder in der Gradle-Ausgabe noch im Klartext unter `build/`, `app/build/` oder `.gradle/` (der Configuration
Cache ist verschlüsselt; `signing-config-versions.json` nennt nur die Schemata); eine geänderte Properties-Datei
oder Variable verwirft den Configuration Cache, ohne Variable entsteht wieder `…-release-unsigned.apk`.

## 5. Nachweise (15.10.2026, Emulator `pida_api35`, Android 15, arm64, Sprache de-AT)

- **Start:** `pida-smoke: ok step=open pdfium=true webview_ms=1003 core_ms=1003` (erster Lauf), im
  letzten Build `… pdfium=true webview_ms=1993 core_ms=1995`; Kaltstart `TotalTime` 0,4–1,7 s.
  `pdfium=true` = `libpdfium.so` zur Laufzeit gebunden, die Vorschau rendert (Screenshot 02).
- **Dokument-Eingang:** `VIEW` (Kaltstart): `Dokumente vorgemerkt: 1` → `Dokumente eingegangen count=1` bei zwei
  `takePendingOpenDocuments`-Aufrufen (genau einmal). `SEND` aus der App „Dateien“ (Teilen → PIDA) in die
  laufende App: ebenso genau einmal. Dateiauswahl (`importFile`, `pdf`): Dokument geöffnet. Ohne URI-Freigabe
  (`am start` gibt `EXTRA_STREAM` keine Freigabe) wird das Dokument übersprungen (`unreadable`), ohne Absturz.
- **Abgewiesene Intents:** `VIEW file:///data/user/0/at.asit.pida.app/config/settings.toml`, `VIEW` auf den eigenen
  Provider (`content://at.asit.pida.app.pida.files/…`) und `SEND text/plain`: je „Intent ohne zulässiges
  Dokument verworfen“, kein Dokument geöffnet.
- **Positionieren per Touch:** Ziehen mit `adb shell input swipe` verschiebt den Block (Screenshot 02).
- **Signiermittel:** ID Austria mit „FIDO2 nicht verfügbar“ (durchgestrichen) und Hinweis, keine lokale BKU;
  nach dem Import zusätzlich „Keystore“ (Screenshot 03).
- **Keystore-Import:** `testdata/keystores/test-rsa2048.p12` über die SAF-Auswahl, Passwort `test`, „Passwort
  sicher auf diesem Gerät speichern“: `PKCS#12-Import ok=true code=ng-keystore-imported`, Hinweis „Das Passwort
  ist im sicheren Speicher des Geräts abgelegt“ (`keychain_available`). Kopie
  `data/keystores/test-rsa2048.p12` mit `-rw-------`; `shared_prefs/pida_secure_store.xml` enthält nur
  SHA-256-Namen und `v1:`-Chiffrate (kein Klartext `test`). Nach Neuinstallation (neuer Prozess) signiert die App
  ohne Passwortabfrage (`secureStoreGet` liefert das Passwort).
- **Signatur und Export:** Signatur mit dem Keystore in der App („Signatur war erfolgreich“, Screenshot 05),
  Ausgabe (Stand M-Android; seit dem Review `files/Documents` im internen Speicher) `Android/data/at.asit.pida.app/files/Documents/Vertrag_signed.pdf`; „Sichern unter …“ → SAF →
  `Download/Vertrag_signed.pdf` (bitgleich, SHA-256 `093b844f…d56e`). Auf dem Mac mit `adb pull`:

  ```text
  pyhanko sign validate --pretty-print --no-strict-syntax --trust testdata/keystores/test-rsa2048.crt.pem Vertrag_signed.pdf
  Certificate subject: "Common Name: PDF-Over NG Test RSA (NUR TEST), …"   The signer's certificate is trusted.
  The signature is cryptographically sound.   The signature covers the entire file.
  The signature is judged VALID.
  ```

  Wiederholt mit dem letzten Build (`Download/Vertrag_signed (1).pdf`, `--trust-replace`): VALID, deckt die
  ganze Datei ab; `qpdf --check` ohne Fehler.
- **Teilen:** Auswahldialog mit „Vertrag_signed.pdf“, PIDA selbst nicht mehr als Ziel (Screenshot 06).
- **Öffnen:** `openFile` startet die PDF-Anzeige (Drive-Viewer) mit dem signierten Dokument (Screenshot 08).
- **Lebenszyklus:** Dateiauswahl, Export, Teilen und Anzeige-App pausieren die App (`Lebenszyklus
  suspended=true` … `false`), der Vorgang läuft danach weiter (Export gelang nach der Rückkehr).
- **Drehen** während des Positionierens: gleicher Prozess, gleiche Activity, Schritt bleibt (Screenshot 07).
- **Zurück:** schließt den offenen Konfigurationsdialog; ohne Dialog geht die App in den Hintergrund, Prozess und
  Schritt bleiben (`LaunchState: HOT`).
- **Tests:** `cargo test -p tauri-plugin-pida-mobile -p pida-app` grün (Plugin 9, Shell 41 + 3 + 5);
  `cargo clippy -p pida-app -p tauri-plugin-pida-mobile` (Desktop und `aarch64-linux-android`) ohne
  Warnungen in diesen Crates (verbleibend nur `dead_code` in `pdf-cos`, `sigblock`, `pades`, wie in M0);
  Gradle `testDebugUnitTest` 5/5. Der APK enthält `org.rustls.platformverifier.*`; die Warnung
  „TLS-Prüfung der Plattform nicht initialisiert“ trat nicht auf.

Screenshots (`docs/integration/screenshots/android/`, 432 px breit, JPEG; `07-querformat.png` aus M-Android, übrige aus
dem Review): `01-start`, `02-positionieren`, `03-signiermittel`, `04-keystore-import`, `05-fertig`, `06-teilen`,
`07-querformat`, `08-oeffnen`, `09-sichern-unter`, `10-keystore-dateiauswahl`.

**Review (15.10.2026, mobile.md §10):** neu gebaut, im Flugmodus erneut durchgespielt – Startprüfung `pdfium=true`,
Keystore-Import über SAF (Kopie in `cache/import` danach entfernt), `VIEW`-Intent genau einmal, Ziehen des Blocks,
Hintergrund/Vordergrund ohne Zustandsverlust, Signatur ohne Passwortabfrage, Ausgabe `files/Documents/Vertrag_signed.pdf`
(0600), Teilen und „Öffnen“ über den FileProvider (`content://at.asit.pida.app.pida.files/…`), „Sichern unter“ bitgleich
nach `Download/`; pyHanko *VALID*. `cmd package query-activities`: `VIEW` mit `BROWSABLE` bzw. `file:` löst nicht mehr
auf PIDA auf.

## 6. Nicht nachgewiesen

- **ID Austria** (Anmeldung, SMS-TAN, App-Wechsel mit Long-Poll nach Rückkehr) und damit **„Signatur
  fortsetzen“ nach App-Kill**: brauchen A-Trust. Mit dem Keystore liegt zwischen `prepare` und Ende keine
  Wartezeit (der Passwortdialog kommt vor `prepare`), ein Kill lässt sich dort nicht platzieren. Abgedeckt nur
  durch die Kern-Tests aus M0 (`crates/core-api/tests/mobile.rs`, `crates/workflow/tests/lifecycle.rs`).
- TLS über den Android-Truststore (kein Netzverkehr in den Nachweisen).
- Release-Build (R8, Signatur), echte Geräte, Android 9–14, x86/armv7.

## 7. Befunde und offene Punkte

1. **Activity zerstört ⇒ Prozess endet.** Wird die Activity nicht wegen einer Konfigurationsänderung zerstört
   (Entwickleroption „Aktivitäten nicht behalten“, Speichermangel im Hintergrund, z. B. während der App „ID
   Austria“), entfernt tauri-runtime-wry das letzte Fenster und beendet die App (`ExitRequested`); wry legt die
   WebView nur nach Konfigurationsänderungen neu an. Nachgewiesen mit „Aktivitäten nicht behalten“: neuer
   Prozess, App startet bei „Dokument öffnen“. Folge: Ein Vorgang **vor** `prepare` ist verloren, danach greift
   „Signatur fortsetzen“. Ein `prevent_exit` hülfe nicht (ohne Fenster bliebe die neue Activity leer). Lösung
   braucht Tauri/wry (WebView für eine neue Activity neu anlegen) – Integration/Upstream.
2. **Abbruch beim Beenden:** Nach `app.exit` (Startprüfung) bzw. beim Ende nach Punkt 1 meldet libc
   `FORTIFY: pthread_mutex_lock called on a destroyed mutex` im HWUI-Thread, teils `SIGABRT` (Prozessende läuft
   über `exit()` mit statischen Destruktoren, während Android-Threads laufen). Die Startprüfung meldet vorher
   `ok`; für Nutzer kann ein Absturzhinweis erscheinen. Integration: Mobile-Ende über `finish()` ohne
   `exit()` oder `_exit` prüfen (M-iOS hat `smoke.rs` für iOS bereits auf `process::exit` umgestellt).
3. **Behoben im Review (mobile.md §10, R-3).** **Ausgabeordner:** `getExternalFilesDir(Documents)` ist unter Android 9/10 für Apps mit Speicher-Berechtigung
   lesbar und in „Dateien“ nicht sichtbar (Nutzer brauchen ohnehin „Teilen“/„Sichern unter“). Empfehlung:
   unter Android `filesDir/Documents` (Shell `mobile_options`, gemeinsame Datei) und die FileProvider-Pfade
   anpassen.
4. **Behoben (R-4).** **Kopien im Cache:** `cache/inbox/<n>/` und `cache/import/<n>/` wurden nie aufgeräumt (Dokumente bleiben,
   bis das System den Cache leert). Aufräumen beim Start oder nach Abschluss (Shell) empfohlen.
5. **Weitgehend behoben (R-8; offen: Warnung „keine Blockvorschau“ beim Laden).** **Oberfläche auf Android**: Tastatur-Kürzel-Unterstreichungen
   („A_b_lehnen“) erscheinen auch auf Touch-Geräten; die Überschreiben-Rückfrage nennt den vollen Pfad
   `/storage/emulated/0/Android/data/…`; der englische Text „Browse for PDF on your computer“ passt nicht zu
   Mobile; beim Laden erscheint einmal `WARN Rendern fehlgeschlagen error=keine Blockvorschau verfügbar`.
6. **Datenschutzhinweis (seit ADR 0003 ohne Dialog):** Ist ID Austria das einzige Signiermittel, startete
   „Zustimmen und fortfahren“ sofort die ID-Austria-Anmeldung (wie PDF-Over). Heute startet ID Austria ohne
   „Signieren“ nie von selbst – die Auswahl mit Kurzhinweis erscheint. Bei den Tests wurde so einmal unbeabsichtigt für etwa 2 s eine
   ID-Austria-Sitzung begonnen (Emulator online, keine Zugangsdaten eingegeben, sofort abgebrochen); danach lief
   der Emulator im Flugmodus.
7. **ABIs:** `tools/fetch-pdfium.sh --jnilibs` liefert arm64-v8a, armeabi-v7a, x86_64; ein Universal-Build ohne
   `--target` enthält auch x86 (i686) – dort ohne PDFium.
8. **Google Play (seit 0.5.3):** Paketname `at.asit.pida.app` seit 0.6.0 (unveränderlich; 0.5.3–0.5.5 `at.asit.pida`, in der Play Console gelöscht, bis 0.5.2 `at.pida` – je eine andere App),
   `targetSdk 36` (Play-Vorgabe für neue Apps/Updates ab Ende August 2026). Für die Play Console ein App Bundle bauen
   (`npm run tauri android build -- --aab`, signiert mit `PIDA_ANDROID_KEYSTORE_PROPERTIES`); dieser Schlüssel ist bei
   Play nur der **Upload-Schlüssel**, Google signiert die ausgelieferten APKs mit dem Schlüssel der Play-App-Signatur.
   Jeder Upload braucht einen höheren `versionCode` (aus der Version, 0.5.3 → 5003). Erste Verteilung: interner Test.
   **Beides in einem Schritt:** `tools/build-android-release.sh` (seit 0.9.1) baut die APK (arm64) und das App Bundle
   (alle ABIs) nach `dist/release-<Version>/`. Das Bundle trägt die **Symboltabelle** des nativen Codes
   (`BUNDLE-METADATA/com.android.tools.build.debugsymbols/<abi>/*.so.sym`, `ndk.debugSymbolLevel = "SYMBOL_TABLE"`):
   Die Play Console zeigt Absturzberichte damit mit Funktionsnamen, die Warnung „keine Symbole zum Debuggen“
   entfällt. Dafür baut das Skript den Kern ungestrippt mit Zeilentabellen (`CARGO_PROFILE_RELEASE_STRIP=none`,
   `CARGO_PROFILE_RELEASE_DEBUG=line-tables-only`); Gradle strippt die ausgelieferten Bibliotheken mit dem NDK aus
   `NDK_HOME` (`ndkPath`/`ndkVersion` in `app/build.gradle.kts`). `keepDebugSymbols` gilt seither nur noch für
   Debug-Varianten – vorher verhinderte es auch im Release das Strippen und die Symboltabelle. Das Skript prüft, dass das Bundle Symboltabellen enthält, die
   Bibliothek gestrippt ist und nirgends Benutzerpfade stehen.
   Ist die Version für die Tester im Play Store zu haben (nach Googles Prüfung), `tools/store-freigabe.sh android
   <Version>` ausführen: Erst dann melden die installierten Apps die neue Version (ADR 0004 Nachtrag 31).
   Hochladen in den internen Test: `tools/play-upload.py <aab> --notes-de "…" [--notes-en "…"]` (Play Developer API,
   Dienstkonto-Schlüssel `~/.pida-release/play-service-account.json` bzw. `PIDA_PLAY_SERVICE_ACCOUNT`, nur Python und
   `openssl`); `--check` prüft den Zugang, ohne etwas zu ändern. Das Dienstkonto hat in der Play Console nur das Recht,
   Test-Track-Releases für PIDA zu erstellen; geschlossene, offene und Produktions-Releases bleiben Handarbeit.
9. Gradle meldet Abkündigungen der Tauri-Vorlage (AGP 9 `android.newDsl=false`, Java 8 als Quelle/Ziel) –
   kommen mit einer neueren Tauri-Vorlage weg.

## 8. Neue Oberfläche (16.10.2026)

Die neue Oberfläche (ADR 0004, E1–E4) am Gerät durchgespielt – Branch `mobile/android`, zuletzt nach dem Merge von
`main` (3d00383: Keystore-Einstellungen mit Formaterkennung, Schlüsselauswahl und „Passwort merken?“, Papierkorb im
Stapel, schrumpfende Vergleichswert-Kästchen, gekürzter Konto-Chip).
Emulator `pdfover_api35` – so heißt das AVD auf diesem Rechner; `pida_api35` in §4/§5 ist der umbenannte Name, ein solches AVD gibt es nicht. <!-- rename-to-pida:keep -->
Android 15, API 35, `google_apis` arm64-v8a, 1080 × 2400 px, 420 dpi (411 CSS-px breit,
`devicePixelRatio` 2,625), Sprache de-AT. Mobile 1.0 (Plan §5.2): ID Austria ohne FIDO2, PKCS#12 nur per Import, keine
Chipkarte, keine CLI.

### 8.1 Bauweg, Gerät, Bedienung

```sh
source tools/env.sh                                   # im Worktree-Root
ln -sfn "$PWD" /tmp/pida-android                      # Pfad ohne Leerzeichen
cd /tmp/pida-android/apps/app
export CARGO_TARGET_DIR=/tmp/pida-android/target-android CARGO_PROFILE_DEV_DEBUG=line-tables-only
npm run tauri android build -- --debug --target aarch64 --apk   # app-universal-debug.apk, ≈ 358 MB (Debug)

AVD=pdfover_api35                                     # echter AVD-Name, rename-to-pida:keep
emulator -avd "$AVD" -no-window -no-audio -no-boot-anim -gpu swiftshader_indirect \
  -http-proxy http://127.0.0.1:9 -no-snapshot-save &
adb shell cmd connectivity airplane-mode enable       # zusätzlich zur Proxy-Sperre: „Active default network: none“
adb root; adb shell setprop persist.sys.locale de-AT; adb shell setprop ctl.restart zygote; adb unroot
adb install -r …/app-universal-debug.apk
adb shell am start -W -n at.asit.pida.app/.MainActivity --ez pida.smoke true
adb -s emulator-5554 emu kill                          # am Ende
```

- **Netz:** Proxy auf den gesperrten Port 9 **und** Flugmodus; A-Trust wurde nie erreicht, keine Zugangsdaten eingegeben.
  Der Schnellstart-Snapshot `default_boot` setzt Sprache und Installation bei jedem Emulatorstart zurück.
- **Bedienung über CDP:** `adb forward tcp:9333 localabstract:webview_devtools_remote_<pid>`; Bedienelemente per Name
  finden, Mittelpunkt × `devicePixelRatio` + Lage der WebView (`uiautomator dump`) → `adb shell input tap`; Texte und
  Zustände per `Runtime.evaluate`, Namen für den Bildschirmleser per `Accessibility.getFullAXTree`. Native Oberflächen
  (Dateiauswahl, Teilen, PDF-Anzeige) per `uiautomator dump`; Mehrfachauswahl in der Dateiauswahl per langem Druck
  (`input swipe x y x y 900`), dann „Auswählen“.
- **App-Ende während eines Laufs:** `adb shell run-as at.asit.pida.app kill -9 <pid>`; Ergebnisse vom Gerät mit
  `adb exec-out run-as at.asit.pida.app cat files/Documents/<Name>` (Ausgabe `files/Documents`, 0600).
- **Startprüfung** nach dem Merge: `pida-smoke: ok step=open pdfium=true webview_ms=914 core_ms=915` (frische
  Installation), Kaltstart `TotalTime` 458–574 ms.

Aufnahmen: `docs/integration/screenshots/android-ng/` (432 px breit bzw. hoch, JPEG), Nummern unten in Klammern.

### 8.2 Ergebnisse je Ablauf

| # | Ablauf | Ergebnis |
|---|---|---|
| 1 | Start, Ersteinrichtung | Frische Installation (auch nach `pm clear`), ohne PDF-Over-Altkonfiguration: Ersteinrichtung als Bottom-Sheet, Schritt 1 Signiermittel (Keystore ausgegraut „Später in den Einstellungen einrichten“), Schritt 2 Signaturbild; „Später einrichten“ → Startseite mit „Dokument auswählen“ und „Zuletzt signiert“ (01–03, dunkel 53). |
| 2 | PDF empfangen, Arbeitsbereich | `VIEW` (content://media) in die laufende App. Arbeitsbereich mit Bottom-Sheet, Block automatisch auf freier Fläche (Hinweis), per Touch ziehen – rastet an Rändern und unter dem Text ein, Seitenleiste mit Miniaturen, Seitenwechsel, Sheet ausgeklappt (Signaturbild, Optionen, Platzierung) (04–08). Feld-Platzhalter `Formular-mit-Platzhalter.pdf`: „Signaturfeld gefunden“, „Ins Feld setzen“ → „Im Signaturfeld“ (09, 10). QR-Platzhalter `testdata/golden/pdfas-4.4.2/inputs/qr.pdf` mit eingeschalteter Suche „QR-Code-Platzhalter“: „Positioniert im QR-Code-Platzhalter des Dokuments“, Block „Im QR-Platzhalter“, Seite 1 / 1 (67 aus der Gegenprüfung; 64 zeigt dagegen den Randfall aus §8.4 mit Seite 1 / 2 und noch den Fokusrahmen vor B18). Mehrere PDFs per Dateiauswahl → Stapel. |
| 3 | Keystore, Signieren, Fertig | Import aus dem Signiermittel-Menü („PKCS#12-Datei importieren …“, SAF, `test-rsa2048.p12`, Passwort `test`) im neuen Stil, die Tastatur verdeckt nichts; nach „Fertig“ ist der Keystore für dieses Dokument gewählt, der lange Name im Konto-Chip wird mit „…“ gekürzt (11–13, 45, 46). Ohne „Passwort sicher speichern“ fragt die erste Signatur direkt unter dem Passwortfeld „Passwort merken?“ (Nicht speichern / Bis zum Beenden / Im sicheren Speicher); die Wahl steht danach als `password_storage` mit `storage_chosen = true` in `settings.toml`, keine weitere Rückfrage (47, dunkel mit Tastatur 54). Signiermittel-Menü mit „Einstellungen …“ (14), Statuskarte „Entsperren → Signiert“ (15), Fertig mit Selbstprüfung, „Öffnen“ (PDF-Anzeige), „Teilen“ (Auswahl ohne PIDA), „Sichern unter …“ (SAF) (16–19, dunkel 55). Einstellungen › Signiermittel › Keystore: importierte Datei, Schlüssel „PDF-Over NG Test RSA (NUR TEST) · gültig bis 11.10.2036“ mit Alias darunter, Speicherart (48 vorher, 52 nachher). **pyHanko** (`sign validate --trust testdata/keystores/test-rsa2048.crt.pem`) für die vom Gerät geholten `Vertrag_signed.pdf`, `Angebot_signed.pdf`, `Rechnung_signed.pdf`, `Protokoll_signed.pdf` und `Gutachten-gross_signed.pdf` (120 MB, nach „Fortsetzen“): je `INTACT:TRUSTED,UNTOUCHED`, „The signature is judged VALID“, deckt die ganze Datei ab; `qpdf --check` ohne Fehler. |
| 4 | Stapel | 2–3 PDFs auf einmal → Übersicht mit Miniaturen und Papierkorb je Zeile (neu von `main`; auf Telefonen erst nach B17 sichtbar), Entfernen eines Dokuments, „Alle signieren“ → Keystore, Startdialog bei vorhandenen Zieldateien (Beide behalten / Überschreiben / Überspringen), Zusammenfassung „2 von 2 signiert“ (20–22, 49–51, 59). App während des Laufs in den Hintergrund (Home) und nach 6 s zurück: derselbe Prozess (`LaunchState: HOT`), `Lebenszyklus suspended=true/false`, der Lauf geht weiter bis „2 von 2 signiert“ (66). App-Ende (kill) im Schritt „Bestätigung 1 von 2“ → beim Neustart „Signatur fortsetzen?“ nur für `Gutachten-gross.pdf` („gerade eben“, beim Keystore ohne „Anmeldung und Bestätigung“, „Danach folgt ein weiteres Dokument“) → Passwort → signiert; das zweite Dokument bleibt „Bereit“ (23 vorher, 60, 24/61). Ein Ende vor dem Entsperren bzw. während „wird vorbereitet“ hinterlässt erwartungsgemäß kein Angebot (noch nichts vorbereitet). |
| 5 | Einstellungen | Vollbild, alle Bereiche: Signaturbild mit Live-Vorschau (auch Amtssignatur), Dokument (PDF/A, Platzhaltersuche QR/Felder), Signiermittel, Speichern (nur „Dokumente dieser App“, Dateinamenszusatz), Darstellung (Farbschema, Sprache), Datenschutz (Zertifikats-Cache, „Zuletzt signiert“ merken/leeren), Erweitert, Über PIDA; Suche mit Tastatur (25–33). Mobile-Einschränkungen: kein Proxy, kein Ausgabeordner, keine Updates, keine Logs, keine Chipkarte; der Datenschutztext nennt auf dem Gerät nur den Keystore (31 vorher, 63 nachher). |
| 6 | Prüfmodus (seit ADR 0004 Nachtrag 15 entfernt) | Signiertes Dokument (`Vertrag_signed (3).pdf`, zwei Revisionen): „unverändert und kryptografisch intakt“, Signaturen, „Auf der Seite zeigen“, „Dieses Dokument signieren“; das Ergebnis unter den Seiten scrollt (34–37 dunkel, 44). |
| 7 | ID Austria ohne Netz | Auswahl im Menü, Kurzhinweis „Das Dokument wird zum Signieren an A-Trust übermittelt. Details“ über dem Konto-Chip, auch eingeklappt (57); Signieren → „Der Signaturdienst ist gerade nicht erreichbar“ mit Alltagstext, die Ursache (DNS) nur unter „Technische Details“, „Erneut versuchen“/„Schließen“ (38 vorher, 58 nachher). FIDO2 in den Einstellungen nur als Hinweis „auf diesem Gerät nicht verfügbar“ (39). |
| 8 | Safe Areas, Tastatur, Schrift, Bildschirmleser | Statusleiste in der Farbe der Kopfzeile, Navigationsleiste in der Hintergrundfarbe, hell und dunkel (43, 53–55); Querformat mit Display-Ausschnitt als Abstand (40, 62); Tastatur über Passwort- und Suchfeldern ohne Verdecken (12, 33, 54); Schriftgröße 130 % (Systemeinstellung, wirkt zur Laufzeit) ohne abgeschnittene Texte (41, 42, 56). Bildschirmleser: AX-Baum per CDP (Fertig, Stapel mit Papierkorb) – alle Bedienelemente benannt (0 ohne Namen); Bedienelemente von Arbeitsbereich, Signierkarte und Einstellungen mit Namen (CDP-Liste), Fingerziele ≥ 44 px. Zurück-Geste: schließt den obersten Dialog (verschachtelt erst den Import, dann die Einstellungen), danach App in den Hintergrund, Prozess bleibt (65). |

### 8.3 Befunde und Behebungen

Plattformübergreifend in der Oberfläche, wo möglich (Desktop unverändert, solange Platz ist); Tests in Klammern.

1. **B1 Systemleisten** blieben im Dunkelmodus hell, die Statusleiste grau über der weißen Kopfzeile → die Oberfläche
   meldet nach jedem Wechsel die Farben (`app/systemBars.ts`, Brücke `PidaSystemBars` in `MainActivity.kt`,
   `values(-night)/colors.xml`) (`systemBars.test.ts`; 43).
2. **B2 Schriftgröße** des Systems wirkte erst nach einem Neustart → `onConfigurationChanged` setzt `textZoom`.
3. **B3 Prüfmodus:** das Ergebnis unter den Seiten ließ sich auf Telefonen nicht scrollen (`shell.css` überstimmte
   `verify.css`) → `.app.view-verify .app-main` (`mobile-css.test.ts`; 44). Mit dem Prüfmodus entfallen (ADR 0004
   Nachtrag 15).
4. **B4 Fingerziele < 44 px:** „Befehle“/Wortmarke in der schmalen Kopfzeile, „Hinweis schließen“, „Suche löschen“,
   kompakter Konto-Chip (`shell.css`, `components.css`; `mobile-css.test.ts`).
5. **B5 Datenschutz-Kurzhinweis** bei ID Austria stand schmal nur im ausgeklappten Sheet → direkt über dem Konto-Chip
   (`Inspector.tsx`; `Mobile.test.tsx`; 57).
6. **B6** Nach dem Import aus dem Konto-Chip blieb „Signiermittel wählen“ → der Keystore ist für dieses Dokument gewählt
   (`select_signer`, startet nichts; `SignerChip.tsx`, `KeystoreImport.tsx`; `Mobile.test.tsx`).
7. **B7 „Signatur fortsetzen“:** „vor 0 Minuten“ und „Anmeldung und Bestätigung sind erneut nötig“ auch beim Keystore →
   Fluent-Mehrzahl („gerade eben“, „vor einer Minute“), der Satz nur bei ID Austria, Symbol des Signiermittels
   (`ResumeDialog.tsx`, `pida.ftl`; `Mobile.test.tsx`; 23 → 60).
8. **B8** Keystore-Warteschritt „Verbindung wird aufgebaut …“ → „Keystore wird geöffnet …“ (`texts.ts`;
   `signing.test.tsx`; 15).
9. **B9** „Signaturdienst nicht erreichbar“ zeigte die Fehlerkette des HTTP-Clients als Text → Attribut `.hint`
   (`backend-network`), die Ursache nur unter „Technische Details“ (`SigningCard.tsx`, `StateCards.tsx`, `texts.ts`;
   `signing.test.tsx`; 38 → 58). Integration: dasselbe hatte iOS als Attribut `.text` umgesetzt – es gilt `.text`
   („Die Verbindung zum Signaturdienst ist fehlgeschlagen. Prüfen Sie die Internetverbindung …“), `.hint` entfällt.
10. **B10** Stapelstart mit Keystore: „Überspringen … ohne Bestätigung in der App“ → eigener Text (`StartBatchDialog.tsx`;
    `batch.test.tsx`; 59).
11. **B11** Der Datenschutztext nannte auf Mobilgeräten die Chipkarte → `ng-ui-privacy-local-mobile` (`privacy.tsx`;
    `settings.test.tsx`; 31 → 63).
12. **B12** Feldbanner „Klicken Sie dort …“ → „Wählen Sie dort …“ (Touch; `ui-workspace.ftl`).
13. **B13 Merge:** `batch.test.tsx` legte `KeystoreSettings` ohne das neue `storage_chosen` an – `tsc` brach den
    Android-Build (`beforeBuildCommand`) ab → ergänzt.
14. **B14 Segmente** („Passwort merken?“ in der Signierkarte, „Passwort“ in den Einstellungen) passten auf 411 px nicht:
    die Gruppe scrollte waagrecht, „Nicht speichern“ bzw. „Schlüsselbund“ waren angeschnitten → Segmente schrumpfen bis
    zum längsten Wort und brechen um (Mindest- statt fester Höhe, Fingerziel 44 px bleibt; `components.css`;
    `mobile-css.test.ts`; 47/48 → 52, 54, 56 mit 130 %). Integration: zusätzlich bricht die Gruppe selbst um
    (`.segmented { flex-wrap: wrap }`, iOS Befund 5) – passt eine Zeile nicht, rutschen Segmente in die nächste.
15. **B15** Einstellungen › Keystore auf Mobilgeräten: Speicherart „Schlüsselbund“ → „Sicherer Speicher“ (wie „Im
    sicheren Speicher“ beim ersten Signieren; `SignerRows.tsx`, `ui-tools.ftl`; `settings.test.tsx`).
16. **B16** Schlüsselzeile „… gültig bis 11.10.2036Alias: test-rsa2048“ ohne Trenner (für `.keystore-key` gab es keine
    Regel, auch am Desktop) → Name und Alias untereinander (`settings.css`; `mobile-css.test.ts`; 52).
17. **B17 Papierkorb im Stapel** (neu von `main`) war auf Telefonen unsichtbar: ≤ 520 px blendete
    `.batch-row-action.has-caret` samt Papierkorb aus → nur der Pfeil entfällt, der Papierkorb steht rechts neben dem
    Namen (`batch.css`; `mobile-css.test.ts`; 49, 50).
18. **B18 Zurück-Geste:** nach dem Schließen eines Dialogs zeigte der Auslöser Fokusrahmen und Tooltip (das synthetische
    Esc setzt in React Aria die Modalität „Tastatur“; 64 rechts oben) → `CLOSE_DIALOG_JS` sendet danach eine
    Zeigerbewegung an `document` (erreicht keine Bedienelemente); am Gerät: Fokus zurück auf „Einstellungen“ ohne
    `data-focus-visible` und ohne Tooltip, verschachtelte Dialoge wie vorher (65).

19. **B19 Fortsetzen im Stapel vergaß die Regel für vorhandene Zieldateien** (Gegenprüfung, am Gerät nachgestellt):
    Start mit „Beide behalten“, App-Ende während „Bestätigung 1 von 2“, „Fortsetzen“ → die Ausgabe fragte „Datei
    überschreiben?“ mit nur „Abbrechen“/„Überschreiben“ (68) – fertig in die App-Dokumente ging es nur durch Überschreiben
    eines früher signierten Dokuments. → Der Kern sichert die Regel des Laufs im persistierten Zustand
    (`ResumeRecord::on_existing`, optional, Format 1 bleibt lesbar) und wendet „Beide behalten“ bei der Ausgabe des
    fortgesetzten Dokuments an (erste freie Nummer, keine Rückfrage); „Überschreiben“ fragt weiterhin, weil nach dem
    App-Ende nicht mehr feststellbar ist, ob die Datei ein Ergebnis desselben Stapels ist (`core-api/src/resume.rs`,
    `runtime.rs`; `resume::tests::records_without_a_batch_policy_still_load`,
    `tests/mobile.rs::batch_resume_keeps_the_rule_for_existing_files`; am Gerät `Gutachten-gross_signed (2).pdf`, die
    vorhandene Datei unverändert, pyHanko `INTACT:TRUSTED,UNTOUCHED`; 69).

Prüfungen nach den Behebungen: `npm ci && npm run build && npm test` 606/606 (27 Dateien);
`cargo test -p pida-app -p pida-core-api --no-fail-fast` 253 bestanden / 0 fehlgeschlagen; `tools/rename-to-pida.py
--check` sauber; Android-Debug-Build neu, am Gerät erneut geprüft.

### 8.4 Nicht möglich bzw. offen

- **A-Trust** (Anmeldung, SMS-TAN, Bestätigung in der App, Vergleichswert ohne Leerzeichen und die schrumpfenden
  Vergleichswert-Kästchen der Signierkarte von `main`): nur mit Netz – am Gerät nicht geprüft; abgedeckt durch
  `signing.test.tsx` und die Kern-Tests (`backend-atrust`).
- **Platzhaltersuche nachträglich eingeschaltet:** `qr.pdf` bei ausgeschalteter Suche geöffnet → Block auf neuer Seite;
  Suche eingeschaltet und dieselbe Datei erneut per `VIEW` empfangen → beide Einträge „im QR-Platzhalter“, das gezeigte
  Dokument behält aber „Auf neuer Seite“ (Seite 1 / 2 mit leerer neuer Seite). Frisch geöffnet richtig (Seite 1 / 1).
  Kern (Neuauswertung der Platzierung, `new_page`) prüfen; dazu: dieselbe Datei zweimal empfangen ergibt zwei
  Stapel-Einträge. **Behoben** (ADR 0004 Nachtrag 12 Punkt 10): Ziel Feld/QR beendet „Auf neuer Seite“, dieselbe Datei
  kommt nicht doppelt dazu (Hinweis `ng-open-duplicate`).
- **Leiste der Einstellungsbereiche:** im Emulator zeitweise überlagerte Beschriftungen (z. B. „Doku“ plus Teile von
  „Signaturbild“; 25–31, 48, 52, 56) – nur in `adb screencap`; eine CDP-Aufnahme (`Page.captureScreenshot`) derselben
  Ansicht ist sauber, das DOM ohne Überlappung. Darstellungsfehler der Emulator-Grafik (`swiftshader_indirect`,
  Log „DisplaySurfaceGlContextHelper context was preempted“); auf einem echten Gerät gegenprüfen.
- **Text der Stapelübersicht:** „einmal anmelden, dann jedes Dokument einzeln bestätigen“ steht auch beim Keystore bzw.
  „Jedes Mal fragen“ – passt nur zu ID Austria; ebenso der Zähler „Bestätigungen k von n“ bzw. „Bestätigung 1 von 2“ in
  der Stapelkarte beim Keystore (Gegenprüfung). **Behoben** (Nachtrag 12 Punkt 1: Wortlaut je Signiermittel aus
  `SignerView::batch_flow`).
- TalkBack mit Sprachausgabe (geprüft wurde der AX-Baum), echte Geräte, Android 9–14, Release-Build: wie §6.

### 8.5 Gegenprüfung (16.10.2026, Stand `36ff4d1` plus B19)

Neu gebaut (Frontend aus dem Stand, Rust-Bibliothek identisch mit der geprüften APK; nach B19 erneut gebaut und
installiert), Emulator wie in 8.1 mit Proxy-Sperre und Flugmodus, A-Trust nie erreicht, keine Zugangsdaten. Die Abläufe
2–4 selbst am Gerät durchgespielt (Rohbilder nur im Arbeitsordner, für die Doku 67–69):

- **2:** frische Installation, `VIEW` mit Ersteinrichtung davor (das Dokument öffnet nach „Später einrichten“), Block
  automatisch platziert, Touch-Ziehen mit Ansage „5 mm von links, 4 mm von unten“, Seitenleiste und Seitenwechsel,
  Sheet ausgeklappt; Felder (`manySignFields-xref-stream.pdf`) „Ins Feld setzen“ → „Im Signaturfeld „Signature_0““;
  QR-Platzhalter frisch mit eingeschalteter Suche: Seite 1 / 1 im Platzhalter (67). Die Angabe „Seite 1 / 1 (64)“ in
  8.2 belegte das Bild nicht (64 zeigt Seite 1 / 2) – korrigiert.
- **3:** Import aus dem Konto-Chip ohne „Passwort sicher speichern“, „Fertig“ wählt den Keystore, erste Signatur fragt
  „Passwort merken?“ (Segmente umbrechend), `settings.toml`: `password_storage = "memory"`, `storage_chosen = true`;
  Fertig mit Selbstprüfung, „Öffnen“ (Auswahl), „Teilen“ (ohne PIDA), „Sichern unter …“ (SAF; Kopie bytegleich);
  Einstellungen › Keystore mit „Sicherer Speicher“ und Alias unter dem Schlüssel; Zurück-Geste ohne Fokusrahmen.
- **4:** drei PDFs per Dateiauswahl, Papierkorb je Zeile, „Alle signieren“ → „2 von 2 signiert“; Startdialog bei
  vorhandener Zieldatei („Überspringen“ ohne App-Text), Home während des Laufs → `HOT`, gleiche PID, „Beide behalten“
  ergibt `Angebot_signed (2).pdf`; App-Ende nach dem Sichern des Zustands → „Signatur fortsetzen?“ („gerade eben“, ohne
  Anmeldesatz) → B19 gefunden (68), behoben und am Gerät bestätigt (69).
- **pyHanko** (`--trust-replace --trust testdata/keystores/test-rsa2048.crt.pem`): `Vertrag_signed.pdf`,
  `Angebot_signed.pdf`, `Gutachten-gross_signed (2).pdf` (120 MB, nach „Fortsetzen“) je `INTACT:TRUSTED,UNTOUCHED`;
  `Protokoll_signed.pdf` (Eingabe mit hybrider Querverweistabelle) erst mit `--no-strict-syntax` – pyHanko lehnt
  hybride Dateien sonst grundsätzlich ab, kein Fehler der Signatur. `qpdf --check` ohne Fehler.
- **Tests nach B19:** `npm test` 606/606; `cargo test -p pida-app -p pida-core-api -p pida-workflow` 430 bestanden /
  0 fehlgeschlagen (darunter die zwei neuen); `cargo clippy -p pida-core-api --all-targets` ohne neue Warnung;
  `tools/rename-to-pida.py --check` sauber.
- **Weitere Beobachtungen (niedrig, offen):** In `manySignFields-xref-stream.pdf` meldet die Automatik „auf freier
  Fläche platziert“, der Block überdeckt aber ein Signaturfeld (Warnung „Überdeckt ein Feld“ mit „Freie Fläche
  suchen“; Kern, nicht Android-spezifisch – `main` hat die Automatik inzwischen überarbeitet). Die Ansage der Lage
  (`aria-live`) enthielt nach dem Öffnen eines anderen Stapeldokuments zunächst noch die Lage des vorigen
  („Seite 1 von 2 … Im Signaturfeld“ bei `qr.pdf`). „Sichern unter …“ meldet „„Vertrag_signed.pdf“ wurde gesichert“, obwohl die Dateiauswahl
  „Vertrag_signed (3).pdf“ anlegte. Nach „Fortsetzen“ fehlen in der Stapelzeile des fortgesetzten Dokuments Miniatur
  und Seiten/Größe. Die Leiste der Einstellungsbereiche zeigte das Überlagern auch hier nur in `adb screencap`
  (CDP-Aufnahme sauber). **Behoben** bis auf das Grafikartefakt (ADR 0004 Nachtrag 12 Punkte 4, 7, 8, 9: Automatik
  meidet Felder, Live-Region beim Dokumentwechsel, tatsächlicher Name aus `DISPLAY_NAME`, Voranalyse auch für die
  fortgesetzte Zeile).

## 9. Mobile nur mit ID Austria (16.10.2026)

Entscheidung des Auftraggebers: „Signieren mit Keystore brauchen wir auf mobilen Apps nicht, da reicht ID Austria.“
Umsetzung und Nachweise für beide Plattformen: mobile.md §12, ADR 0004 Nachtrag 11. Für Android bedeutet das:

- **Entfallen:** Abläufe mit Keystore (§5 Keystore-Import, Signatur mit dem Keystore, Screenshots `04-keystore-import`,
  `10-keystore-dateiauswahl`; §8.2 Abläufe 3 und 4 mit Keystore; §8.3 B6–B8, B10, B15, B16; §8.5 Punkt 3). Die
  Dateiauswahl der Shell fragt nur noch PDFs an (`mobile_pick_files` lehnt `pkcs12` ab); das Plugin behält
  `importFile` mit `pkcs12` unverändert (keine native Änderung).
- **Bleibt:** Signieren mit ID Austria (App-Bestätigung/SMS-TAN, FIDO2 ausgegraut), Stapel, „Signatur fortsetzen“
  (B19), Teilen, „Sichern unter“, „Öffnen“ über den FileProvider; `allowBackup=false` und die Verschlüsselung des
  sicheren Speichers unverändert.
- **Übergang früherer Builds:** gespeicherter Standard Keystore → ID Austria (beim Lesen, `[keystore]` bleibt in
  `settings.toml`), Keystore-Passwörter im sicheren Speicher (Schlüssel `PIDA/keystore-store:…`/`PIDA/keystore-key:…`
  zur Datei aus den Einstellungen und zu jeder Datei in `files/…/data/keystores`) werden beim ersten Start einmalig
  entfernt (Merker `data/keystore-secrets-removed`); ein gesicherter Keystore-Vorgang für „Signatur fortsetzen“ wird
  mit Hinweis verworfen. Die Datei `data/keystores/*.p12` selbst bleibt liegen (durch ihr Passwort geschützt).
- **Am Emulator:** nachgeholt am 17.10.2026 mit einem echten Update von einem früheren Build – siehe §10.

## 10. Gerätenachweis „Mobile nur mit ID Austria“ (17.10.2026)

Stand `mob2/android`: `main` 8606b0c mit dem Merge von `826fe82` (`mob2/int`: Mobile nur mit ID Austria, Randfälle
aus ADR 0004 Nachtrag 12, dazu von `main` „+ PDF“, „In Bearbeitung“ und die automatische Positionierung), danach die
Behebungen aus 10.2. Emulator `pdfover_api35` wie §8 (Android 15, 411 CSS-px breit, de-AT), Debug-APK nach §8.1. <!-- rename-to-pida:keep -->
Netz: Proxy `127.0.0.1:9` und Flugmodus („Active default network: none“) – A-Trust wurde nie erreicht (Technische
Details: DNS-Fehler „No address associated with hostname“), keine Zugangsdaten. Startprüfung nach der Installation:
`pida-smoke: ok step=open pdfium=true`. Der Prüfmodus wird parallel entfernt (`remove/verify`) und war nicht
Gegenstand. Aufnahmen: `docs/integration/screenshots/android-ng2/` (432 px breit, Nummern unten in Klammern;
„befund“ = vor der Behebung, „nachher“ = danach). Bedienung wie §8.1 über CDP und `adb shell input`; getippt wurde
für die Tastatur-Abläufe über die Bildschirmtastatur (Tippen auf die Tasten), nicht nur per `input text`.

**Update-Fall mit einem echten früheren Build:** `git archive 8606b0c` in einen Arbeitsordner, `tools/pdfium`
dazukopiert, eigenes Ziel als APFS-Klon (`cp -cR target-android …/target-old`, sonst ein vollständiger Neubau), die
von wry erzeugten Kotlin-Dateien (`gen/android/app/src/main/java/at/asit/pida/generated`, nicht versioniert – sonst
„Unresolved reference 'TauriActivity'“) aus dem Worktree kopiert, gebaut wie §8.1. Diesen Build installiert, in
Einstellungen › Signiermittel `testdata/keystores/test-rsa2048.p12` (Passwort aus der Testdaten-Notiz) mit „Passwort
sicher auf diesem Gerät speichern“ importiert und als Standard gewählt, danach den neuen Build per `adb install -r`
darüber installiert.

### 10.1 Ergebnisse je Ablauf

| # | Ablauf | Ergebnis |
|---|---|---|
| 1 | Frische Installation | Nach `pm clear`: Ersteinrichtung „Schritt 1 von 1“ nur mit dem Signaturbild, keine Wahl des Signiermittels (01); „Später einrichten“ → Hinweis „Signiermittel und Signaturbild können Sie jederzeit in den Einstellungen einrichten.“ (02). `settings.toml` ohne `backend.default`; beim ersten Start Merker `data/keystore-secrets-removed` (`removed=0`). |
| 2 | Update von einem Build mit Keystore | Vorher (06): `backend.default = "keystore"`, `[keystore]` mit `data/keystores/test-rsa2048.p12`, `password_storage = "keychain"`, zwei Einträge in `shared_prefs/pida_secure_store.xml`. Nach dem Update: Log „gespeichertes Standard-Signiermittel auf ID Austria abgebildet“ und „Keystore-Passwörter eines früheren Builds entfernt removed=2“; `pida_secure_store.xml` leer (`<map />`), Merker gesetzt, `settings.toml` beim Start unverändert, die `.p12`-Datei bleibt (wie §9). Start ohne Ersteinrichtung, ID Austria als Signiermittel (07, 08, 10). Sobald die App Einstellungen speichert (hier: Automatische Positionierung umgeschaltet), steht `default = "id_austria"` in der Datei; `[keystore]` bleibt unverändert (mobile.md §12.2). |
| 3 | Absuche nach Keystore | Alle acht Einstellungsbereiche (Text, `aria-label`, `title`, `placeholder`, aufgeklappte Details) nach `keystore`, `zertifikatsdatei`, `passwort merken`, `pkcs`, `p12`, `schlüsselbund`, `sicherer speicher`, `alias`, `bürgerkarte`, `gültig` – frisch und nach dem Update ohne Treffer (03, 08). Suche in den Einstellungen: „keystore“, „pkcs12“, „p12“, „zertifikatsdatei“, „passwort“, „alias“, „schlüsselbund“, „sicherer speicher“, „bürgerkarte“, „chipkarte“ ohne Treffer, „zertifikat“ nur „Signaturzertifikat zwischenspeichern“, „schlüssel“ nur FIDO2 (04, 09). Befehlspalette („keystore“, „zertifikat“, „signiermittel“) ohne Treffer, Hilfe (Tastenkürzel) ohne Keystore (05). Konto-Chip-Menü in Arbeitsbereich und Stapel nur „Einstellungen …“ (10). DOM-Suche ohne Treffer in Startseite, Arbeitsbereich samt Sheet, Stapel, Startdialog, Signierkarte und Fehleransicht. |
| 4 | PDF empfangen, Positionierung | `VIEW` in die laufende App, Block automatisch auf freier Fläche. Einstellungen › Dokument › „Automatische Positionierung“ wirkt sofort auch auf das offene Dokument: „Automatisch (laut Einstellung)“, Rolle „automatisch platzierter Signaturblock“, Ziehen ohne Wirkung (11, 12); „Diesmal selbst platzieren“ → „verschiebbarer Signaturblock“, Ziehen setzt die Lage (13). Stapel: mit Einstellung nur „Automatische Positionierung (laut Einstellung)“, ohne „Für diesen Stapel automatisch positionieren“; bei einem von Hand platzierten Eintrag Rückfrage, „Behalten“ lässt ihn „Positioniert · selbst platziert“ (17, 40); der Schalter bleibt nach Parken und Fortsetzen an. |
| 5 | „+ PDF“, Papierkorb | „Weitere PDFs hinzufügen …“ mit Mehrfachauswahl → Stapel (14, 15); „Anderes PDF öffnen …“ ersetzt das automatisch platzierte Dokument ohne Rückfrage, nach eigener Platzierung mit Rückfrage (23); Papierkorb je Zeile entfernt sofort (18). |
| 6 | „Zur Startseite“, „In Bearbeitung“ | Aus dem Stapel → Karte „In Bearbeitung · Stapel · 3 Dokumente“ (19); „Weiter bearbeiten“ → unverändert (Einträge, Pläne, Stapel-Option); „Schließen“ → Rückfrage wegen eigener Platzierung, danach Startseite ohne Karte (22). Teilen (`VIEW`) bei geparkter Arbeit → ohne Rückfrage dazu, Hinweis „Das Dokument wurde zur offenen Arbeit hinzugefügt.“ (20, ADR 0004 Nachtrag 10 Punkt 4); „Dokument auswählen“ bei geparkter Arbeit → „Zur offenen Arbeit hinzufügen?“ mit „Zum Stapel hinzufügen“ / „Offene schließen und neu beginnen“ / „Abbrechen“ (21). |
| 7 | Randfälle (Nachtrag 12) | Hinweis über ausgeklapptem Sheet oben (27); Live-Region nach dem Wechsel `qr.pdf` → `Formular.pdf` ohne Lage des vorigen (erst leer, dann „Seite 3 von 3 …“); `qr.pdf` bei ausgeschalteter Suche auf neuer Seite (24), Suche eingeschaltet → „Im QR-Code-Platzhalter“, Seite 1 / 1 (25), dieselbe Datei erneut empfangen → „„qr.pdf“ ist bereits geöffnet.“, kein zweiter Eintrag, auch im Stapel (26); Bereichsleiste bei 130 % rollt den aktiven Bereich ins Bild (34); Stapeltexte für ID Austria: „einmal anmelden, dann jedes Dokument einzeln bestätigen“, „1 Anmeldung · n Bestätigungen“, Stapelkarte „Bestätigungen 0 von 2“, „Überspringen … ohne Bestätigung in der App“ (28, 41). „Sichern unter“-Hinweis: siehe 10.3. |
| 8 | ID Austria ohne Netz, Tastatur, Darstellung | Einzeldokument und Stapel: „Verbindung wird aufgebaut …“ mit „Abbrechen“ (per `MutationObserver` aufgezeichnet, steht unter 150 ms) → „Der Signaturdienst ist gerade nicht erreichbar“ mit „Schließen“/„Erneut versuchen“ bzw. „Stapel abbrechen“/„Überspringen“/„Erneut versuchen“, Technische Details `backend-network` mit dem DNS-Fehler (28, 29); „Erneut versuchen“ verwendet die Sitzung wieder, „Stapel abbrechen“ ohne Anmeldung ohne Rückfrage. Feld „Telefonnummer oder Benutzername“ (Einstellungen): `type=text`, `autocapitalize=none`, `autocomplete=username` – Buchstaben-Tastatur (31); das Fenster schrumpft mit der Tastatur, Feld und Knöpfe bleiben sichtbar. Dunkel (32, 33), Querformat 863 × 360 mit Seitenleiste und Abstand zum Ausschnitt (36), Schrift 130 % ohne abgeschnittene Texte (34–36). AX-Baum (CDP): Einstellungen, Arbeitsbereich und Fehleransicht ohne unbenanntes Bedienelement. |

### 10.2 Befunde und Behebungen

1. **C1 (niedrig, Android) Fokusrahmen nach der Bildschirmtastatur.** In den Einstellungen getippt (Suche,
   Benutzername), dann mit der Zurück-Geste geschlossen: „Einstellungen“ in der Kopfzeile zeigte einen Fokusrahmen
   (37). Ursache: Die WebView hält nach Eingaben über die Bildschirmtastatur `:focus-visible` fest; B18 (§8.3) setzt nur
   die Modalität von React Aria zurück (`data-focus-visible` fehlte richtig). → Bei Fingerbedienung (`pointer: coarse`)
   zeigen React-Aria-Elemente außer Textfeldern den Rahmen nur noch mit `data-focus-visible` (`styles/base.css`;
   `mobile-css.test.ts`). Am Gerät: nach demselben Ablauf `:focus-visible` weiter wahr, aber `outline: none` (38);
   mit Tab (Hardware-Tastatur) erscheint der Rahmen wie bisher (`data-focus-visible`). Desktop unverändert.
2. **C2 (niedrig, alle Plattformen) Datenschutz-Kurzhinweis im Stapel in der Einzahl:** „Das Dokument wird zum
   Signieren an A-Trust übermittelt.“ in Übersicht, Startdialog und Stapelkarte bei mehreren Dokumenten (16, 30) →
   ab zwei Dokumenten „Die Dokumente werden zum Signieren an A-Trust übermittelt.“ (`ng-ui-backend-leaves-device-many`,
   `PrivacyNote count`; `batch.test.tsx`, `i18n.test.ts`; 39, 41, 42). Einzeldokument unverändert.
3. **C3 (niedrig, alle) „Überspringen“ in der Mehrzahl bei nur einer vorhandenen Zieldatei:** „Für ein Dokument gibt
   es die Zieldatei schon.“ und darunter „Diese Dokumente werden nicht signiert …“ (30) → Einzahl/Mehrzahl nach der
   Zahl der vorhandenen Zieldateien (`ng-ui-batch-start-skip-desc(-local)` mit `$n`; 41).
4. **C4 (niedrig, alle) Rückfrage zur Stapel-Option in der Mehrzahl bei einem Dokument:** „Eigene Platzierungen
   behalten?“ / „Auch diese automatisch platzieren“ zu „Ein Dokument haben Sie selbst platziert.“ (17) → „Eigene
   Platzierung behalten?“ / „Auch dieses automatisch platzieren“ (`ng-ui-batch-auto-confirm-title`,
   `ng-ui-batch-auto-replace` mit `$n`; `batch.test.tsx`, `i18n.test.ts`; 40).

Vertrag unverändert (nur Oberfläche und UI-Kataloge); am Gerät mit neu gebauter APK bestätigt (38–42).

### 10.3 Nicht möglich bzw. offen

- **Signatur mit Ergebnis, „Fertig“, „Sichern unter“-Hinweis, Teilen und Öffnen eines Ergebnisses:** ohne Keystore
  und ohne A-Trust auf Mobile nicht erreichbar (erwartet). Belegt durch die Kern-Tests (Signatur, tatsächlicher
  Exportname in `tests/mobile.rs`) und die Plugin-Tests (`./gradlew :tauri-plugin-pida-mobile:testDebugUnitTest`,
  grün).
- **Anmeldemaske von ID Austria** (Telefonnummer/Passwort in der Signierkarte): erscheint erst nach dem ersten Kontakt
  mit A-Trust – ohne Netz nicht erreichbar; die Benutzername-Tastatur ist am gleichen Feldtyp in den Einstellungen
  belegt (31).
- **Warteansicht** nur unter 150 ms sichtbar (DNS-Fehler sofort) – Text aufgezeichnet, keine Aufnahme.
- **„Signatur fortsetzen“ eines Keystore-Vorgangs** eines früheren Builds: nicht nachgestellt (der Zustand ist beim
  Keystore nur Millisekunden gesichert); belegt durch `keystore_resume_of_an_earlier_build_is_discarded`.
- **Bereichsleiste (§8.4) und doppelte Bildteile** in `adb screencap`: nur mit `-gpu swiftshader_indirect` (43); mit
  `-gpu host` (Kaltstart) sauber (44, ebenso 26 und 39) – Grafikfehler des Emulators, kein Fehler der App. Für
  künftige Aufnahmen `-gpu host` verwenden.
- **Kopfzeile schmal:** der Dokumentname bleibt neben sieben Bedienelementen bei 1–2 Zeichen („A…“, bei 130 % „A.“);
  mit der Entfernung des Prüfmodus wird „Signatur prüfen“ (52 px) frei – nach dem Merge von `remove/verify` erneut
  ansehen.
- **Datenschutztext** (Einstellungen › Datenschutz): der übernommene PDF-Over-Text nennt „Der Signaturvorgang mit der
  Chipkarte wird lokal durchgeführt“, darunter der Mobile-Satz zu ID Austria – Bereich des parallelen Laufs, nicht
  geändert.
- Ein Stapel mit nur noch einem Dokument bleibt Stapel („Alle Dokumente werden in einem Durchgang signiert …“) –
  niedrig, Wortlaut.
- `SEND_MULTIPLE` lässt sich mit `adb` nicht auslösen (mehrere PDFs wie in §8 über die Dateiauswahl).
- Emulator am Ende heruntergefahren (`adb emu kill`), Flugmodus, Schrift, Drehung und Dunkelmodus vorher
  zurückgesetzt.

### 10.4 Prüfungen

`cargo test --workspace --no-fail-fast` 1445 bestanden, 0 fehlgeschlagen, 5 ignoriert; `cargo clippy --workspace
--all-targets -- -D warnings` und `cargo fmt --all --check` ohne Befund; `npm ci && npm run build && npm test`
33 Dateien, 722 Tests; `ts_bindings` und `serde_contract` unverändert grün (Vertrag unberührt, nichts neu erzeugt);
`python3 tools/rename-to-pida.py --check` ohne Befund; `cargo check -p pida-app` für `aarch64-apple-ios-sim` und
`aarch64-linux-android` (NDK 29, `CC_/AR_/…_LINKER` gesetzt) grün; Plugin-Unit-Tests
(`:tauri-plugin-pida-mobile:testDebugUnitTest`) grün. Nebenbei: `settings.test.tsx` („Liste leeren“) scheiterte im vollen Lauf unter Last
gelegentlich, weil der Hinweistext zugleich in der Live-Region stand – die Prüfung sucht jetzt nur den Toast.

### 10.5 Zusammenführung (17.10.2026)

In `mob2/int` mit dem iOS-Nachweis (`mob2/ios`, mobile-ios.md §10) und damit mit `main` samt Entfernung des
Prüfmodus (ADR 0004 Nachträge 15 und 18) vereint; Einzelheiten in mobile.md §12.5/§12.6. C2 und C4 hatte der
iOS-Durchgang gleich behoben (N6, N7) – Code und Kataloge blieben gleich, die Tests beider Durchgänge sind vereint.
Die schmale Kopfzeile (10.3) hat nach dem Merge keinen Knopf „Signatur prüfen“ mehr; für Stapeleinträge zeigt die
Wortmarke bis 480 px zudem nur das Logo (iOS N5). Am Emulator ist das noch nicht wieder angesehen.
