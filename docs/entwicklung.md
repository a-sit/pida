# Entwicklung – PIDA

> Name: **PIDA** (PDF · ID · Austria), bis 15.10.2026 Arbeitstitel „PDF-Over NG“ – siehe
> [ADR 0002](adr/0002-name-pida.md). Die Umbenennung ist das Skript `tools/rename-to-pida.py`
> (`--dry-run`, `--check`, `--regenerate`); nach Zusammenführungen mit Ständen unter dem alten Namen
> erneut ausführen (ADR 0002 §5).

Praktische Anleitung für Setup, Tests, CI, Lizenzen und Releases. Verbindlich bleiben
[Plan](00-architektur-und-umsetzungsplan.md) und [ADR 0001](adr/0001-workspace-und-schnittstellen.md);
dieses Dokument beschreibt, *wie* man damit arbeitet. Stand: 15.10.2026 (Welle 3, Track T1).

## 1. Schnellstart

```sh
source tools/env.sh                          # rustup, .venv (pyHanko), PIDA_REFERENCE – auch in Worktrees
eval "$(tools/fetch-pdfium.sh --print-env)"  # PDFium chromium/8100 laden, PIDA_PDFIUM_DIR setzen
cargo check --workspace --all-targets --locked
cargo test --workspace --locked
cargo run -p pida-cli -- --help
```

Unter Windows (PowerShell): `Invoke-Expression (tools\fetch-pdfium.ps1 -PrintEnv)`, danach dieselben
`cargo`-Befehle.

## 2. Werkzeuge und Umgebung

| Werkzeug | Version | Zweck | Bezug |
|---|---|---|---|
| Rust | **1.99.0** stable, Edition 2024 (`rust-version` im Workspace) | Bauen | macOS: `brew install rustup` (keg-only, `/opt/homebrew/opt/rustup/bin`), dann `rustup toolchain install 1.99.0 -c clippy,rustfmt`; sonst rustup.rs |
| PDFium | chromium/8100 (pdfium-render 0.9.4 = API 7881) | Renderer, Tests | `tools/fetch-pdfium.sh` / `.ps1` (§3) |
| qpdf | ≥ 11.7 (lokal 12) | Orakel `qpdf --check`, Testdaten | `brew install qpdf`, `apt install qpdf` |
| OpenSSL | ≥ 3.2 (`openssl cms -cades`; lokal 4) | CMS-Orakel | macOS: `brew install openssl@3` (das System-`openssl` ist LibreSSL). Ubuntu 24.04 hat 3.0 ohne `-cades`: dort überspringt sich das CAdES-Orakel von `cades` (CI: Prüfung unter macOS) |
| pyHanko | 0.37.0 (+ cli 0.5.0, certvalidator 0.32.1), Pillow 12.3.0 | PAdES-/ICC-Orakel | `python3 -m venv .venv && .venv/bin/pip install -r tools/python-oracles.txt` |
| Java | 21 (nur Test-Orakel, nie im Produkt) | Differentialtests (Properties, Konfiguration, i18n, Keystore-Generator) | `brew install openjdk@21` bzw. Temurin 21 |
| xmllint | beliebig | SL-1.2-XML-Orakel | macOS vorhanden, `apt install libxml2-utils` |
| cargo-deny | **0.20.2** | Lizenz-/Lieferketten-Gate (§6) | `brew install cargo-deny` oder `cargo install --locked cargo-deny@0.20.2` |
| cargo-about | **0.9.2** | THIRD-PARTY-NOTICES (§6) | `brew install cargo-about` oder `cargo install --locked cargo-about@0.9.2` |
| Node | 22 (npm, Lockfile) | Tauri-Frontend `apps/app` | `brew install node@22` |
| Xcode | 27 | iOS-Ziele, ring für iOS | App Store |

**`tools/env.sh`** setzt `PATH` (rustup keg-only, `~/.cargo/bin`, `.venv/bin`), `PIDA_ROOT`,
`PIDA_MAIN` (Hauptcheckout, auch aus einem `git worktree`) und `PIDA_REFERENCE` (Referenzquellen,
nur lesen; Tests lesen nie daraus). Lehnt eine Sandbox `source tools/env.sh` ab, genügen die
gleichwertigen Exporte:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:<hauptcheckout>/.venv/bin:$PATH"
export PIDA_REFERENCE="<hauptcheckout>/reference"
```

**Umgebungsvariablen der Tests** (alle optional; ohne Werkzeug/Variable überspringen sich Tests mit
einer Meldung auf stderr):

| Variable | Wirkung |
|---|---|
| `PIDA_PDFIUM_DIR` | Verzeichnis mit `libpdfium.dylib`/`.so` bzw. `pdfium.dll` (sonst PDFium-Tests übersprungen) |
| `PIDA_PYTHON` | Python mit pyHanko (sonst `python3` neben `pyhanko`, dann `python3` im PATH) |
| `PIDA_I18N_BLESS=1` | erzeugt `crates/i18n/locales/*/pdfover.ftl` neu (`--test catalogs`) |
| `PIDA_UPDATE_RESULTS=1`, `PIDA_E2E_OUT=<dir>` | `testdata/pdf-incremental/RESULTS.md` aktualisieren bzw. Ausgaben behalten |
| `PIDA_FUZZ_ITERS`, `PIDA_FUZZ_SEED`, `PIDA_FUZZ_FILE`, `PIDA_FUZZ_TRACE` | längere/gezielte Robustheitsläufe (pdf-cos, pdf-incremental) |
| `PIDA_TEST_KEYCHAIN=1` | Test gegen den echten Schlüsselbund (config) |

Grundregeln für Tests (ADR 0001 §4 Regel 6): kein Netzverkehr, keine echten Zugangsdaten, Testdaten
nur aus `testdata/` bzw. Crate-Assets.

## 3. PDFium

- `tools/pdfium.lock` hält Release (`chromium/8100`) und SHA-256 der 12 Archive von
  [bblanchon/pdfium-binaries](https://github.com/bblanchon/pdfium-binaries): `mac-arm64`, `mac-x64`,
  `linux-x64`, `linux-arm64`, `win-x64`, `win-arm64`, `ios-device-arm64`, `ios-simulator-arm64`,
  `ios-simulator-x64`, `android-arm64`, `android-arm`, `android-x64`. Alle Hashes wurden am 15.10.2026
  gegen die Digests der GitHub-Release-API geprüft; `mac-arm64` ist bitgleich mit der bisher genutzten Kopie.
- `tools/fetch-pdfium.sh [plattform…|--all]` (macOS, Linux, Git Bash) bzw. `tools/fetch-pdfium.ps1`
  (Windows PowerShell 5.1/7) laden nach `tools/pdfium/<plattform>/` (nicht versioniert), prüfen den
  Hash, verwerfen abweichende Archive, lehnen Archive mit absoluten oder `..`-Pfaden ab und laden
  nichts neu, solange die entpackte Version zum Lock passt. Die Bibliothek liegt in `lib/`, unter
  Windows in `bin/` (`pdfium.dll`). `--print-dir`, `--print-env`, `--github-env` liefern
  `PIDA_PDFIUM_DIR`.
- Neue Release: `PDFIUM_RELEASE=chromium/<n> tools/fetch-pdfium.sh --update-lock --all`, Hashes gegen
  die Release-Seite bzw. `gh attestation verify <archiv> --repo bblanchon/pdfium-binaries` abgleichen,
  dann `pdfium-render`/`pdfium_latest` (Root-`Cargo.toml`) und ADR 0060 nachziehen.
- Suchreihenfolge zur Laufzeit und Sicherheitsregeln (kein Arbeitsverzeichnis): ADR 0060 §1. Unter iOS
  wird die dylib später in ein xcframework verpackt (Plan §2.1).

## 4. Bauen und Testen

```sh
cargo check --workspace --all-targets --locked
cargo check -p pida-render -p pida-placement -p pida-pades --no-default-features --all-targets --locked
cargo test --workspace --locked                     # mit PIDA_PDFIUM_DIR und Orakeln im PATH
cargo test -p pida-backend-atrust --features fido2-hid      # macOS/Linux (fido2-windows unter Windows)
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
tools/cross-clippy.sh x86_64-pc-windows-msvc        # CI-Clippy für Windows/Linux lokal nachstellen (§9)
```

- `--locked` wie in der CI: `Cargo.lock` wird mit committet; kein `cargo update` ohne Absprache.
- **Netz in Tests:** alle Tests laufen offline (Ersatz-Transporte, lokale Mock-Server). Einziger Online-Test ist
  `cargo test -p pida-core-api --test about github_releases_online` gegen `api.github.com` (Update-Prüfung, ADR 0004
  Nachtrag 21) – nur mit `PIDA_ONLINE_TESTS=1`, sonst übersprungen.
- **Versionsdatum** („Über PIDA“): `crates/core-api/build.rs` bettet `SOURCE_DATE_EPOCH` (UTC-Datum) bzw. das
  Commit-Datum des gebauten Stands ein (`git log -1 --format=%cs`, nur aus dem Repository, das das Crate verwaltet);
  ohne Git die Dateizeit von `Cargo.toml` (GitHub-Quellarchive: Zeit des Commits), zuletzt das Datum des Bauens. Angaben
  vor 2026 zählen nicht; nur das Datum (`build_support/release_date.rs`, `tests/release_date.rs`).
- FIDO2 unter Linux braucht Zugriff auf `/dev/hidraw*` (udev-Regel mit `uaccess`, Flatpak `--device=all`).
- Die Tauri-Shell (`apps/app`, Paket I3) braucht unter Linux `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`,
  `libsoup-3.0-dev`, `libjavascriptcoregtk-4.1-dev`, `librsvg2-dev`, `libayatana-appindicator3-dev`,
  `libxdo-dev` und vor dem Rust-Build ein gebautes Frontend (`npm ci && npm run build` in `apps/app`).

**App (Tauri-Shell, `apps/app`)** – Befehle im Ordner `apps/app`:

```sh
npm ci && npm run build && npm test           # Oberfläche (tsc strict, Vite, Vitest)
cargo test -p pida-app                     # Shell: Startargumente, Kernstart, Protokoll, Sicherheitskonfiguration
npm run notices                               # THIRD-PARTY-NOTICES nach target/notices (für das Bundle)
PIDA_PDFIUM_DIR=… npm run bundle:mac:debug     # .app (Debug) mit libpdfium.dylib und Hinweisen
PIDA_PDFIUM_DIR=… npm run bundle:mac:release   # .app + .dmg (Release); verlangt PDFium und Hinweise
PIDA_PDFIUM_DIR=… npm run bundle:debug         # Debug-Bundles der Plattform (alle Formate)
PIDA_SMOKE=1 HOME="$(mktemp -d)" "../../target/debug/bundle/macos/PIDA.app/Contents/MacOS/pida"
```

- `scripts/bundle-config.mjs` erzeugt die Tauri-Ergänzung `src-tauri/bundle.conf.json` (nicht
  versioniert): PDFium aus `PIDA_PDFIUM_DIR` (macOS `Contents/Frameworks`, sonst Ressourcen `pdfium/`)
  und die Hinweise aus `PIDA_NOTICES_DIR` (Standard `target/notices`) als Ressourcen; mit
  `--require-pdfium`/`--require-notices` bricht es ab, wenn etwas fehlt (Release, CI).
- **Startprüfung** `PIDA_SMOKE=1`: Die App beendet sich, sobald die Oberfläche den ersten ViewState
  abgefragt hat und der Kern bereit ist, und schreibt `pida-smoke: ok step=… pdfium=… webview_ms=…
  core_ms=…` auf stdout (Exit 0; ohne Antwort binnen 60 s Exit 3). `HOME` (Windows: `APPDATA`,
  `LOCALAPPDATA`) auf einen leeren Ordner setzen – die App legt sonst Log und Fensterzustand im echten
  Benutzerprofil an.
- **Bridge-Typen**: `crates/core-api/src/view.rs` ist die einzige Quelle; `apps/app/src/core/generated.ts`
  entsteht mit ts-rs 12.0.1 (`PIDA_BLESS=1 cargo test -p pida-core-api --test ts_bindings`), der Test
  schlägt bei jeder Abweichung fehl. Fremde Typen (Settings, Placement, pades) haben Spiegel mit
  vollständiger `From`-Abbildung und Serialisierungstest (`core-api/src/ts.rs`, `workflow/src/ts_mirror.rs`,
  `backend-api/src/ts_mirror.rs`, Feature `ts`). Beispiel-JSON beider Richtungen:
  `PIDA_BLESS=1 cargo test -p pida-app --test serde_contract` bzw.
  `PIDA_BLESS=1 npx vitest run src/core/contract.test.ts`.

### Fuzzing

Der PDF-Parser, die Signaturvorbereitung und der Security-Layer-Parser verarbeiten fremde Eingaben. Sie werden
auf zwei Arten mit kaputten Eingaben geprüft:

- **Robustheitstests** (stabile Toolchain, Teil von `cargo test`): `crates/pdf-cos/tests/robustness.rs` und
  `crates/pades/tests/fuzz_prepare.rs` mutieren den Korpus deterministisch (`PIDA_FUZZ_ITERS` bzw.
  `PIDA_FUZZ_PREPARE_ITERS`, `PIDA_FUZZ_SEED`), dazu gezielte Laufzeit- und Speicherfallen.
- **Coverage-geführtes Fuzzing** (cargo-fuzz/libFuzzer mit AddressSanitizer, Nightly): `fuzz/` ist ein eigener
  Workspace mit vier Zielen.

| Ziel | prüft | Startwerte |
|---|---|---|
| `pdf_open` | `SourcePdf::open` und alle lesenden Auswertungen (`crates/pdf-cos/tests/common/exercise.rs`) | PDFs aus `testdata/`, `fuzz/pdf.dict` |
| `pdf_xmp` | PDF/A-Erkennung im XMP-Paket | – |
| `pades_prepare` | Öffnen, Analyse, Planung, Vorbereitung (unsichtbar und sichtbar), Hash (`crates/pades/tests/common/fuzz_case.rs`) | PDFs aus `testdata/`, `fuzz/pdf.dict` |
| `sl12_response` | `is_sl_response`, `parse_response` | `testdata/sl12/responses`, `fuzz/sl12.dict` |

`pdf_open` und `pades_prepare` rufen dieselbe Routine auf wie der jeweilige Robustheitstest; neue lesende Methoden
bzw. Schritte dort ergänzen, dann decken beide Wege sie ab.

```sh
rustup toolchain install nightly --profile minimal && cargo install cargo-fuzz --locked   # einmalig
tools/fuzz.sh                    # 60 s je Ziel
tools/fuzz.sh 3600 pdf_open      # eine Stunde nur „pdf_open“
```

Der Korpus wächst in `fuzz/corpus/<Ziel>`, Funde liegen in `fuzz/artifacts/<Ziel>` (beides nicht eingecheckt).
Als Fund zählen Panic, Speicherfehler, ein Durchlauf über 20 s und mehr als 2 GB Speicher. Nachstellen:
`cargo fuzz run <Ziel> <Fund>` (in `fuzz/`, Nightly) oder für `pdf_open` auf der stabilen Toolchain
`PIDA_FUZZ_FILE=<Fund> cargo test -p pida-pdf-cos --test robustness replay_single_file -- --nocapture`.
Nach der Korrektur kommt die (möglichst verkleinerte, `cargo fuzz tmin`) Eingabe als Regressionsfall nach
`testdata/`. Die CI pinnt Nightly und cargo-fuzz (`FUZZ_TOOLCHAIN`, `CARGO_FUZZ_VERSION` in `ci.yml` und
`security.yml`); lokal genügt die aktuelle Nightly (`PIDA_FUZZ_TOOLCHAIN` wählt eine andere).

### Isolierte bzw. portable Profile (`PIDA_HOME`)

Ist `PIDA_HOME` gesetzt, legen App und CLI Einstellungen, Daten, Cache, Logs und Temp-Dateien unter
`$PIDA_HOME/{config,data,cache,logs,tmp}` ab und suchen die PDF-Over-Altkonfiguration in
`$PIDA_HOME/.pdf-over`. Das isoliert Tests auf allen Plattformen – unter Windows lassen sich die
Profilordner über `HOME`/`APPDATA` nicht umlenken (Known-Folder-API) – und erlaubt eine portable
Installation (z. B. auf einem USB-Stick).

```bash
PIDA_HOME="$(mktemp -d)" target/release/pida-cli profiles
```

**Portables Windows-Paket** (`PIDA_<version>_x64-portable.zip`, Release-Workflow, Backlog Nr. 31): Liegt neben
`pida.exe` ein Ordner `PIDA-Daten` (`pida_config::PORTABLE_DIR`) und ist `PIDA_HOME` nicht gesetzt, gilt er als
`PIDA_HOME`. Die App legt dann auch die Daten der WebView2 dort ab (`PIDA-Daten/webview`; das Hauptfenster entsteht
in `setup` mit `data_directory`), merkt keinen Fensterzustand (das Plugin schriebe nach `%APPDATA%`) und nutzt keinen
Schlüsselbund: Keystore- und Proxy-Passwörter gelten nur bis zum Beenden (Entscheidung vom 20.10.2026). Die
WebView2-Laufzeit bleibt Voraussetzung; Single-Instance gilt weiter je Konto (eine laufende installierte PIDA nimmt
Dokumente der portablen an). Der Release-Workflow prüft das ZIP mit einem Start, der nichts ins Benutzerprofil
schreiben darf.

## 5. Lints und Formatierung

- `cargo fmt --all` (rustfmt.toml: Breite 120). Pakete formatieren nur ihre eigenen Crates (`cargo fmt -p …`).
- Workspace-Lints (Root-`Cargo.toml`, `[workspace.lints]`): `unsafe_code = deny` (FFI nur in eigenen
  Modulen mit lokalem `allow` und `SAFETY`-Kommentar), `missing_debug_implementations`, `dead_code`,
  `unused_variables`, `unused_lifetimes`, `redundant_lifetimes`, `unused_import_braces`; Clippy
  zusätzlich `todo`, `unimplemented`, `dbg_macro`, `mem_forget`, `rc_buffer`, `lossy_float_literal`,
  `verbose_file_reads`, `unnecessary_safety_comment`, `infinite_loop`, `rest_pat_in_fully_bound_structs`.
  Aufgenommen wurde nur, was im Bestand ohne Befund ist; `trivial_numeric_casts` bewusst nicht
  (FFI-Typen wie `c_ulong` sind je Plattform verschieden).
- Die CI prüft `cargo clippy --no-deps --all-targets -- -D warnings` für alle Crates außer der Liste
  `CLIPPY_LENIENT` in `.github/workflows/ci.yml`; für diese laufen dieselben Lints ohne Build-Bruch.
  `--no-deps` ist nötig, weil `-D warnings` sonst auch für mitübersetzte Workspace-Abhängigkeiten gilt.
  Lokal gleichwertig:
  `cargo clippy --workspace --exclude <je Eintrag aus CLIPPY_LENIENT> --no-deps --all-targets --locked -- -D warnings`.
- `CLIPPY_LENIENT` (Stand Welle 3): `pida-pades`, `pida-pdf-cos`, `pida-sigblock`,
  `pida-placement`, `pida-backend-keystore` wegen je einer `dead_code`-Warnung in fremder Ownership:
  `crates/pades/src/signed.rs:149` (Feld `written`), `crates/pdf-cos/src/lexer.rs:459` (Feld `end`),
  `crates/sigblock/src/fonts.rs:169` (Feld `scaling`), `crates/placement/tests/golden_pdfas442.rs:78`
  (Feld `profile`), `crates/backend-keystore/tests/backend.rs:30` (Funktion `copy_to`). `pida-workflow`, `pida-core-api`, `pida-cli` und `pida-app` sind seit
  Welle 3 streng geprüft. Behebung: entfernen oder mit `#[allow(dead_code, reason = "…")]` begründen,
  dann das Crate aus der Liste streichen. Ziel ist eine leere Liste. Stand 16.10.2026 (`mob2/core`): die Stellen in
  `pida-pades` (dazu `tests/e2e_corpus.rs`, Feld `key_stem`) und `pida-backend-keystore` sind begründet bzw. entfernt;
  `cargo clippy --workspace --all-targets -- -D warnings` läuft unter macOS ohne Befund – die Liste lässt sich kürzen,
  sobald die CI das auch unter Linux und Windows bestätigt.

## 6. Abhängigkeiten, Lizenzen, Lieferkette

**Neue Abhängigkeiten** (ADR 0001 §4 Regel 2): Steht sie in `[workspace.dependencies]`, mit
`workspace = true` einbinden (eigene `features` erlaubt). Sonst zunächst mit exakter Version im
Crate-`Cargo.toml` und Kommentar; die Integration hebt sie in den Workspace (Welle 2: `encoding_rs`,
`hyper`, `tokio-rustls`, `pkcs5`, `rand_core`). Versionen nicht nebenbei ändern, kein `cargo update`.

**cargo-deny** (`deny.toml`, CI-Job `deny`, blockierend):

```sh
cargo deny --locked check          # advisories bans licenses sources
```

- *Lizenzen*: Allowlist nach Plan §2.5/§10.2 (MIT, Apache-2.0, BSD-2/3, ISC, Zlib, MPL-2.0,
  Unicode-3.0, BSL-1.0, OFL-1.1/Bitstream-Vera), `confidence-threshold = 0.93`. Einzige Ausnahme (seit
  Welle 3, Tauri): `target-lexicon` mit `Apache-2.0 WITH LLVM-exception` (Build-Abhängigkeit von
  `cfg-expr`/`system-deps` unter Linux), gleichlautend in `tools/licenses/about.toml`
  (`[target-lexicon] accepted`). `webpki-root-certs`/CDLA zieht rustls-platform-verifier nur unter
  wasm32. Ausgewertet werden genau die 12 Auslieferungsziele und alle Features.
- *Advisories*: Lücken, `yanked`, `unmaintained` und `unsound` brechen den Build (auch transitiv);
  begründete Ausnahmen: RUSTSEC-2023-0071 (rsa/Marvin, nur Entschlüsseln betroffen, Plan §2.1),
  RUSTSEC-2024-0370 (proc-macro-error „unmaintained“, nur Build-Zeit über glib-macros 0.18/Tauri, Linux)
  RUSTSEC-2024-0429 (glib 0.18 `VariantStrIter` unsound, gtk-rs 0.18 über Tauri unter Linux, ungenutzt)
  und seit 17.10.2026 RUSTSEC-2026-0370/-0371/-0372 (`zvariant`, `zbus_names`, `zvariant_derive`
  „unmaintained“, in zbus 6 aufgegangen; zbus 5 nur unter Linux über keyring/secret-service und die
  Tauri-Plugins opener/single-instance, deren stabile Versionen noch an zbus 5 hängen) sowie seit 24.11.2026
  RUSTSEC-2026-0543 (`chrono` „unmaintained“, keine Lücke; nur über `pdfium-render` für Datumswerte in
  Anmerkungen und Formularfeldern, auch in 0.9.5 fest verdrahtet).
- *Bans*: doppelte Versionen = Fehler; die bekannten Duplikate stehen mit Begründung in `skip`
  (die zweite RustCrypto-Generation über p12-keystore/lopdf/secret-service, ADR 0001 §5.2, und seit
  Welle 3 die Tauri-Plattformschichten: gtk-rs 0.18, jni 0.21, ico/schemars, windows-sys 0.45/0.60).
  Verboten: GPL/AGPL-PDF-Crates (`pdfium`, `mupdf`, `pdf_signer`, `pdfluent`), `openssl`/`native-tls`,
  `aws-lc-*`; rustls ohne `aws_lc_rs`/`fips`, reqwest ohne `default-tls`/`native-tls`/`rustls`.
- *Quellen*: nur crates.io.
- Bringt eine Änderung ein neues Duplikat oder eine neue Lizenz, wird sie bewertet und begründet in
  `deny.toml` eingetragen – nie pauschal (`skip-tree`, globale Lizenz) freigeschaltet.
- Ein Tauri-Update bringt in der Regel neue bzw. wegfallende Duplikate: `cargo deny --locked check bans`
  nennt die genauen Versionen; veraltete `skip`-Einträge meldet cargo-deny als Warnung.

**THIRD-PARTY-NOTICES** (Plan §10.2; CI-Job `notices` lädt das Ergebnis als Artefakt hoch):

```sh
tools/third-party-notices.sh            # → target/notices/THIRD-PARTY-NOTICES.md
```

Enthält zuerst die übernommenen EUPL-Bestandteile (PDF-AS-4-Konfiguration und -Bildmarken, © EGIZ, EUPL-1.1
oder später; Herkunft der PDF-Over-Textkataloge, © A-SIT), dann PDFium samt mitgebauten Bibliotheken (aus dem PDFium-Paket), die Font-/AFM-Lizenzen aus
`crates/sigblock/assets/licenses`, alle ausgelieferten Rust-Crates über cargo-about
(`tools/licenses/about.toml`, `about.hbs`; Dev-Abhängigkeiten ausgenommen, Build-Abhängigkeiten
enthalten; bei „MIT OR Apache-2.0“ gilt Apache-2.0, weil viele Crates nur eine MIT-Vorlage ohne Urheber
mitliefern – verbliebene Vorlagen „`<year> <copyright holders>`“ brechen ab, außer für die noch zu klärenden
Crates in `tools/licenses/template-exceptions.txt`) mit den NOTICE-Dateien von Apache-2.0-Crates, die Rust-Standardbibliothek (steht nicht in
`Cargo.lock`, ist aber in jeder Binary: MIT-/Apache-Text aus der Toolchain, vollständige Hinweise als
`RUST-STD-COPYRIGHT.html` daneben) und die npm-Laufzeitabhängigkeiten von `apps/app` (ohne Lizenzdatei
im Paket, z. B. `@fluent/bundle`: Urheber und Verweis auf den SPDX-Standardtext) samt dem Hilfscode, den
vite und rolldown ins Bundle schreiben. Beide Dateien kommen
über `scripts/bundle-config.mjs` als Ressourcen in jedes App-Bundle (mit dem EUPL-Text als `LICENSE.txt`),
`apps/cli/build.rs` bettet sie samt EUPL-Text ein (Feature `notices`) – für `pida-cli licenses` und für
„Lizenzen anzeigen“ im Über-Tab: ein eigener Dialog der App (Command `license_text`) mit EUPL-1.2, Drittkomponenten
und Rust-Standardbibliothek, auf allen Plattformen, auch Android (das APK hat keine Ressourcendateien).
`PIDA_REQUIRE_NOTICES=1` bricht den Build ohne Hinweise ab (Release-Workflow, lokale Release-Builds). PDFium-Lizenzen stammen aus dem Paket der Host-Plattform;
für ein anderes Paket `--pdfium <plattform>` angeben. Die abweichend lizenzierten HarfBuzz-Daten in PDFium
(`src/ms-use`, Microsoft, MIT) ergänzt das Skript aus `tools/licenses/harfbuzz-ms-use-COPYING.txt`. Mit
`--pdfium win-…` kommt der Windows-Teil dazu: WebView2-SDK (BSD-3, statisch in `pida.exe`), NSIS-Laufzeit des
Setups (zlib, LZMA-Modul CPL-1.0), `nsis_tauri_utils` und die WiX-Teile des MSI (MS-RL); Texte, Versionen und
Quellen stehen in `tools/licenses/windows`. Das Skript läuft nur unter Linux/macOS – die Windows-Pakete
bekommen ihre Hinweise im Release-Workflow aus dem Linux-Job `notices-windows` (Artefakt, `--require-notices`
auch für Windows); der CI-Job `notices` erzeugt beide Varianten. Mit `--pdfium android-…` kommt der
Android-Teil dazu (`tools/android-licenses.py` mit dem Init-Skript `tools/licenses/android-artifacts.gradle`): die
Java-/Kotlin-Bibliotheken des APK-Laufzeit-Klassenpfads (AndroidX, Kotlin, Material, Jackson …) mit Lizenz aus dem
POM und ihren LICENSE-/NOTICE-Dateien – das Android-Gradle-Plugin verwirft diese beim Packen. Für das APK daher
vor dem Bau `tools/third-party-notices.sh --pdfium android-arm64 --out target/notices-android` und
`PIDA_NOTICES_DIR=target/notices-android PIDA_REQUIRE_NOTICES=1` setzen (braucht JDK und das Android-Projekt).

**npm** (Plan §2.5: dieselbe Allowlist für die UI): `node tools/npm-licenses.mjs apps/app/package-lock.json`
prüft das Lockfile – Laufzeitpakete gegen die Allowlist, reine Build-Werkzeuge (`dev: true`) zusätzlich
gegen übliche permissive Daten-/Werkzeuglizenzen (CC0, CC-BY, 0BSD, BlueOak, Python-2.0 …); Copyleft und
fehlende Angaben brechen ab. Ausnahmen je Paket stehen mit Begründung in `RUNTIME_EXCEPTIONS` (derzeit
`tslib`, 0BSD, Laufzeit-Helfer von react-aria-components). `devOptional`-Pakete zählen zur Laufzeit (npm installiert sie auch ohne
Dev-Abhängigkeiten), eigene Workspace-Pakete (Einträge außerhalb `node_modules/`) werden übersprungen.
Alles, was der Bundler in die App übernimmt – auch Framework-Laufzeit –, gehört in `dependencies`, sonst
fehlt es in den Hinweisen. Ausnahme ist Code, den ein Build-Werkzeug selbst erzeugt (`BUNDLED_TOOL_CODE`:
vite – modulepreload-polyfill, preload-helper –, rolldown – runtime): Diese Werkzeuge brauchen eine
Laufzeit-Lizenz, fehlen sie im Lockfile, bricht die Prüfung ab; ihre Texte stehen in einem eigenen Abschnitt. Mit `--notices <datei>` entstehen die Hinweise; `--self-test` prüft Parser und
Einordnung (CI-Job `deny`).

## 7. i18n: Codes und Kataloge

- Kataloge: `crates/i18n/locales/<de|en>/pdfover.ftl` (erzeugt, nie von Hand) und `pida.ftl`
  (von Hand; ADR 0050).
- Stabile Codes der Crates (`BackendError::code`, `CmsError::code`, `KeystoreError::code`,
  `Fido2Error::code`, `SigBlockError::code`, `RenderError::code`, `PlacementError::code`,
  `PlacementNotice::code`, `pades::Finding::code`, `ConfigError::message_id`, `LegacyWarning::code` …)
  stehen in `pida.ftl` unter **ihrem Code als ID**; Codes mit Punkt (`render.failed`) werden mit
  `pida_i18n::fluent_id_for_java_key` abgebildet (`render-failed`). Variablen heißen wie die Felder
  der Variante (`$detail` für Freitext, `$status`, `$code` …).
- `NoticeCode::code()`/`FormProblem::code()` (backend-api) liefern die Katalog-ID jeder Variante; der
  Test `notice_and_form_variants_have_codes` verlangt eine Abbildung je Variante ohne `_ =>`.
- **Eine Quelle für alle Oberflächen**: Kern, CLI (`apps/cli/src/text.rs`, keine eigenen Texte mehr) und
  UI (`apps/app/src/i18n`, Texte `ng-ui-*`) lesen dieselben Dateien. Die Welle-2-Liste
  (`docs/integration/wave2-message-ids.json`) prüft `wave2_message_ids_are_in_catalog`.
- Neuer Code: im Crate einführen, DE- und EN-Text mit denselben Variablen in beide `pida.ftl`
  eintragen, `cargo test -p pida-i18n` ausführen. Der Test `code_catalog` findet jeden Code im
  Quelltext (`fn code`/`fn message_id`, `NOTICE_*`, Literale der Familien `ng-`, `cms-`, `keystore-`,
  `backend-`, `fido2-`, `atrust-`, `sigblock-`, `pdf-`, `pades-`, `workflow-`, `core-`, `render.`,
  `placement.` …) und prüft DE/EN, gleiche Variablen, die Welle-1-Liste
  (`testdata/i18n/wave1-message-ids.json`, wortgleich) und IDs in `t!`/`translate`-Aufrufen.

## 8. CI (`.github/workflows/ci.yml`)

| Job | Inhalt | blockierend |
|---|---|---|
| `fmt` | `cargo fmt --all --check`, `shellcheck` der Skripte in `tools/` | ja |
| `build` (ubuntu, macOS, Windows) | PDFium laden (Cache über `tools/pdfium.lock`), Orakel (pyHanko aus `tools/python-oracles.txt`, Java 21, qpdf/xmllint unter Linux, qpdf/OpenSSL 3 unter macOS), ggf. Frontend und Tauri-Systembibliotheken, `cargo check`, Check ohne PDFium, Clippy `--no-deps -D warnings` (außer `CLIPPY_LENIENT`), FIDO2 (`fido2-windows` unter Windows, sonst `fido2-hid`), `cargo test --no-fail-fast` (alle Testbinaries, auch nach einem Fehler), unter Linux Doc-Build mit `RUSTDOCFLAGS=-D warnings` | ja |
| `fuzz` | Fuzz-Ziele (`pdf-cos`, `pades`, `sl12`) mit gepinnter Nightly bauen und je 30 s laufen lassen (`tools/fuzz.sh`, siehe „Fuzzing“ in §4) | ja |
| `deny` | `cargo deny --locked check` (cargo-deny 0.20.2), Selbsttest `tools/npm-licenses.mjs --self-test` | ja |
| `notices` | `tools/third-party-notices.sh`, Artefakt `THIRD-PARTY-NOTICES` (`.md` + `RUST-STD-COPYRIGHT.html`) | ja |
| `mobile` | `cargo check` für `aarch64-apple-ios` und `aarch64-linux-android` (NDK des Runners) | verifizierte Crates ja, ganzer Workspace informativ |
| `app` | nur wenn `apps/app/package.json` existiert: Lockfile-Pflicht, npm-Lizenzprüfung, `npm ci`, `lint`/`build`/`test` (falls vorhanden) | ja |
| `bundle` (ubuntu, macOS, Windows) | Tauri-Debug-Bundle mit PDFium (`deb`/`app`/`nsis`), Hinweise auf Linux/macOS, macOS: Codesignatur (`tools/verify-macos-bundle.sh`, §10), Größe in der Zusammenfassung, Startprüfung `PIDA_SMOKE=1` mit `pdfium=true` (Linux mit `xvfb-run`), Bundle als Artefakt | ja; Startprüfung nur unter macOS blockierend |

Caching: `Swatinem/rust-cache` (Speichern nur auf `main`), PDFium, pip, npm. Actions sind auf
Commit-SHAs gepinnt (Kommentar = Release-Tag); `actionlint` prüft die Datei lokal
(`brew install actionlint shellcheck && actionlint`).

**Täglicher Security-Check** (`.github/workflows/security.yml`, 05:00 UTC und manuell): `cargo deny --locked
check advisories`, `npm audit --omit=dev` für `apps/app` (devDependencies nur als Hinweis in der
Zusammenfassung), die Prüfung, dass alle Actions auf Commit-SHAs gepinnt sind, und das Fuzzing (`tools/fuzz.sh`,
10 Minuten je Ziel; der gewachsene Korpus wandert über den Actions-Cache in den nächsten Lauf, Funde hängen als
Artefakt `fuzz-funde` am Lauf). Ein Fund lässt den Lauf scheitern (E-Mail von GitHub); der Workflow ändert
nichts selbst.

## 9. Cross-Checks (lokal, 15.10.2026, macOS arm64, Rust 1.99.0)

`cargo check --locked --target <ziel> -p <crate>` je Crate (Skript-Ergebnis):

| Ziel | Ergebnis |
|---|---|
| `x86_64-pc-windows-msvc` | ok: java-properties, i18n, pdf-cos, pdf-incremental, cades, sigblock, placement, render, sl12, backend-api, config, backend-keystore, pades, workflow. Fehlschlag nur am C-Build von `ring` (kein MSVC/Windows-SDK auf macOS): net, backend-localbku, backend-atrust (`fido2-windows`), core-api, cli → CI `windows-latest` |
| `x86_64-unknown-linux-gnu` | wie Windows: alle reinen Rust-Crates ok, `ring`-abhängige nur nativ (CI `ubuntu-latest`) |
| `aarch64-apple-ios` | ok: alle Crates außer cli (ring baut mit Xcode). Seit Welle 3 bindet `pida-config` `keyring` nur auf Desktop-Zielen ein (unter iOS verlangte `apple-native-keyring-store` 1.0.2 das Feature `protected`); auf Mobile liefert `platform_secret_store` den Platzhalter `MobileSecretStorePlaceholder`, bis die Mobile-Shell ihren Speicher setzt. Behoben in Welle 2: `time` ohne `local-offset` |
| `aarch64-linux-android` | ok: alle reinen Rust-Crates inkl. config/workflow; `ring`-abhängige brauchen das NDK (`CC_aarch64_linux_android`), CI-Job `mobile` setzt es |
| Mobile-Welle M0 (Shell) | ok: `cargo check`/`clippy -D warnings` für `pida-app` und `tauri-plugin-pida-mobile` auf `aarch64-apple-ios`, `aarch64-apple-ios-sim` (kompiliert das Swift-Paket des Plugins) und `aarch64-linux-android` (NDK 29.0.14206865, `CC_/AR_/CARGO_TARGET_…_LINKER` auf `aarch64-linux-android24-clang`); Bauen, Plugin-Vertrag und Aufgaben: docs/integration/mobile.md |

**Clippy für Fremdziele** (`tools/cross-clippy.sh <ziel>`, Review Welle 2): stellt die blockierenden
Clippy-Schritte des Jobs `build` (strikt ohne `CLIPPY_LENIENT` aus `ci.yml`, dazu FIDO2 mit
`fido2-windows` bzw. `fido2-hid`) für `x86_64-pc-windows-msvc` und `x86_64-unknown-linux-gnu` nach. ring
übersetzt dafür mit clang und Platzhalter-Headern (nur Typprüfung, nie gelinkt), damit auch net, localbku,
atrust, core-api und cli geprüft werden. Ergebnis 15.10.2026: beide Ziele grün, nachdem
`clippy::unnecessary_cast` in `crates/render/src/ffi.rs` (`c_ulong` ist unter Windows `u32`) mit
begründetem `allow` versehen wurde – ohne das wäre `clippy -D warnings` auf `windows-latest` rot gewesen.
Tests und Linken bleiben der nativen CI vorbehalten.

Seit Welle 3 gehört die Tauri-Shell (`pida-app`) zum Workspace: Für `x86_64-pc-windows-msvc` braucht
ihr Build-Skript den Ressourcen-Compiler `llvm-rc` (`brew install llvm` bzw. für die reine Typprüfung ein
Platzhalter im `PATH`, der auf `/?` „Resource Converter“ ausgibt und für `/fo <datei>` eine leere Datei schreibt; die
Versionsressource steht dann trotzdem lesbar in `target/cross-clippy/…/build/pida-app-*/out/resource.rc`) – damit ist der Windows-Lauf am 15.10.2026 grün, einschließlich
des `cfg(windows)`-Codes der Shell (Elternfenster für Windows Hello). Für `x86_64-unknown-linux-gnu`
verlangen die gtk-rs-Sys-Crates eine Linux-Sysroot mit `pkg-config`; ohne sie bricht der Lauf an
`glib-sys` ab – Linux prüft die CI nativ.

Gefunden und behoben: `trivial_numeric_casts` schlug nur für Windows an (`c_ulong` in render/ffi) und
ist deshalb nicht Teil der Workspace-Lints.

## 10. Release-Grundgerüst

Noch kein Release-Workflow; die Bausteine stehen fest:

1. **Version** in `[workspace.package] version` (SemVer), Tag `vX.Y.Z` auf `main`; Changelog-Eintrag.
2. **Build** je Plattform mit `--release --locked` (Profil: `lto = "thin"`, `codegen-units = 1`,
   `strip = "symbols"`): CLI `pida-cli` und Tauri-Bundles (`apps/app`, Tauri-CLI 2.12.x: NSIS/MSI,
   DMG Universal, deb/rpm/Flatpak, iOS/Android) – Plan §9.1.
3. **PDFium beilegen**: `tools/fetch-pdfium.sh <plattform>` (Hash aus `tools/pdfium.lock`); `lib/`
   bzw. `bin/` neben die Binary, macOS `Frameworks/`, iOS als xcframework (ADR 0060 §1).
4. **Hinweise**: `tools/third-party-notices.sh` → `THIRD-PARTY-NOTICES.md` in jedes Paket und ins
   Über-Fenster; dazu SBOM (CycloneDX) und `cargo deny check` als Gate (Plan §2.5).
5. **Signieren nur in der CI** (Plan §9.3): Windows Artifact Signing/HSM, Apple `notarytool`/`stapler`
   (PDFium-dylib mitsigniert), Linux-Repo-Schlüssel offline; Mobile über die Stores.
6. **Updates** (Plan §9.4): Tauri-Updater mit Ed25519-Signatur und eigenem Manifest; Linux über
   Paketquellen/Flathub.
7. **Abnahme**: Größen-Gate ≤ 25 MB je Desktop-Installer (Plan §9.1), Smoke-Test je Paket.
8. **Herausgeber und Urheberrecht** (ADR 0004 Nachtrag 21): `bundle.publisher = "A-SIT"`, `bundle.copyright =
   "© 2026 A-SIT · EUPL-1.2"` und `bundle.homepage` in `apps/app/src-tauri/tauri.conf.json` (Windows `CompanyName`/
   `LegalCopyright`, NSIS `Publisher`, MSI `Manufacturer`, macOS `NSHumanReadableCopyright`, deb/rpm/NSIS/MSI
   Homepage), Cargo `authors = ["A-SIT"]` (deb `Maintainer`), iOS `NSHumanReadableCopyright` in `gen/apple`
   (Info.plist und `project.yml`); NSIS übernimmt beim Aktualisieren den Ordner aus dem Schlüssel des bisherigen
   Herausgebers „pida“ (`apps/app/src-tauri/windows/installer-hooks.nsh`); geprüft von
   `apps/app/src-tauri/tests/bundle_metadata.rs`. Nachweis macOS:
   `npm run bundle:mac:debug`, dann `plutil -p target/debug/bundle/macos/PIDA.app/Contents/Info.plist`; Windows:
   die von tauri-build erzeugte `target/…/build/pida-app-*/out/resource.rc` (`CompanyName`, `LegalCopyright`).

Die Tauri-Shell bündelt seit Welle 3 (`npm run bundle:mac:release` u. a., Abnahme:
`docs/integration/app-abnahme.md`); ein `.github/workflows/release.yml` mit Signieren folgt. Signier-Secrets
gehören dem Auftraggeber (Plan §9.3, E-13).

### Release bauen

**Desktop-Bundles** baut `.github/workflows/release-bundles.yml` (Tag `v*` oder manuell); lokal
`npm run bundle:mac:release` (§4, in Terminal-Sitzungen mit `CI=true`).

**PIDA Web** (Browser-Variante, ab 0.5.0): derselbe Workflow baut im Job „Web-Variante (Linux-Pakete)“ Oberfläche
und Kern als WebAssembly (`apps/web/build.sh`), den Server statisch für x86_64-Linux (musl) und daraus
`pida-web_<v>_amd64.deb`, `pida-web-<v>-1.x86_64.rpm` und `pida-web-<v>-linux-x86_64.tar.gz`
(`apps/web-server/packaging/package.sh`); `packaging/test-install.sh` prüft Installation, Update und Entfernen in
Debian- und Rocky-Containern. Lokal: `apps/web/build.sh`, dann
`cargo run -p pida-web-server -- --mock-atrust testdata/keystores/test-rsa2048.p12 --mock-password test`
(Testgegenstelle statt A-Trust). Aufbau und Betrieb: [integration/web.md](integration/web.md).

**macOS-Signatur (ad hoc):** `bundle.macOS.signingIdentity` ist `"-"` (`apps/app/src-tauri/tauri.conf.json`):
Tauri signiert `Contents/Frameworks/libpdfium.dylib`, die Binary und `PIDA.app` vor dem DMG-Bau ad hoc und
versiegelt die Ressourcen. Bis 0.2.0 war die App nur vom Linker signiert (`Sealed Resources=none`,
`codesign --verify --strict`: „code has no resources but signature indicates they must be present“) – aus dem
Internet geladen meldete macOS sie als **„beschädigt“**. Jetzt ist die Signatur gültig; Gatekeeper lehnt die
nicht notarisierte App weiter ab (`spctl -a -t exec -vv` → `rejected`, `syspolicy_check distribution` nur noch
„Adhoc Signed App“ und „Notary Ticket Missing“), bietet aber „Dennoch öffnen“ (README, Installation).
`hardenedRuntime` ist dabei **aus**: Die Library Validation der Hardened Runtime lehnt die ad hoc signierte
`libpdfium.dylib` ab („mapping process and mapped file (non-platform) have different Team IDs“), PIDA liefe
ohne Vorschau. Mit einer echten Identität (`APPLE_SIGNING_IDENTITY`, ersetzt bei Tauri das `"-"`; Notarisierung)
schaltet `scripts/bundle-config.mjs` sie wieder ein. `APPLE_CERTIFICATE` (Zertifikat-Import in CI) braucht
zusätzlich `APPLE_SIGNING_IDENTITY`, sonst bricht `bundle-config.mjs` ab: Tauri verglich das Zertifikat sonst mit
dem `"-"` und signierte – bei einem Namen mit „-“ – mit Developer ID, aber ohne Hardened Runtime. Prüfen:

```sh
tools/verify-macos-bundle.sh target/release/bundle   # App + App in der DMG: codesign --verify --deep --strict,
                                                    # versiegelte Ressourcen, jede Mach-O-Datei signiert mit der
                                                    # Team-ID der App; ad hoc ohne, Identität mit Hardened Runtime
```

CI (`bundle`, macOS) und Release-Workflow führen denselben Schritt aus; die Startprüfung verlangt
`pdfium=true`.

**Lokale Release-Builds ohne Benutzerpfade** (`pida-cli`, Android-Bibliothek): Panik-Orte und Debug-Infos
enthalten sonst absolute Pfade wie `/Users/<name>/.cargo/registry/src/…`. In einer **eigenen Shell** nur für
Release-Builds:

```sh
PIDA_RELEASE_REMAP=1 source tools/env.sh   # CARGO_ENCODED_RUSTFLAGS mit --remap-path-prefix für
                                           # $CARGO_HOME/registry, $CARGO_HOME/git, $RUSTUP_HOME, Repository
tools/third-party-notices.sh                # target/notices: Drittlizenz-Hinweise für `pida-cli licenses`
PIDA_REQUIRE_NOTICES=1 cargo build --release --locked -p pida-cli
strings -a target/release/pida-cli | grep -c "$HOME"   # 0
```

`PIDA_REQUIRE_NOTICES=1` lässt den Build abbrechen, wenn die Hinweise fehlen (`apps/cli/build.rs` bettet
`THIRD-PARTY-NOTICES.md` und `RUST-STD-COPYRIGHT.html` aus `$PIDA_NOTICES_DIR` bzw. `target/notices` komprimiert
ein; ein relativer `PIDA_NOTICES_DIR` gilt ab der Wurzel des Repositorys). Ohne die Variable werden vorhandene
Hinweise ebenfalls eingebettet; fehlen sie, entsteht mit einer Cargo-Warnung ein Entwicklungs-Build, dessen
`pida-cli licenses` nur den EUPL-Text samt Hinweis ausgibt. Später erzeugte Hinweise bettet der nächste Build ein
(`build.rs` beobachtet `target/notices`).

Nachweis 17.10.2026 (macOS arm64): `pida-cli` 0.2.0 enthielt 708 Fundstellen von `/Users/`, mit
`PIDA_RELEASE_REMAP=1` 0 (die Orte heißen `/cargo/registry/src/index.crates.io-…/tokio-1.53.3/src/…`); die
Android-Bibliothek 814 → 0 (docs/integration/mobile-android.md §4.1). `CARGO_ENCODED_RUSTFLAGS` (Trenner 0x1f)
statt `RUSTFLAGS`, weil der Repo-Pfad Leerzeichen enthalten kann; vorhandene `RUSTFLAGS` werden übernommen.
Ohne `PIDA_RELEASE_REMAP` bleibt alles wie bisher; Debug-Builds und Tests in einer Shell mit der Variable würden
mit den anderen Flags neu übersetzt.
