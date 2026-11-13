# PIDA Web – Browser-Variante: Aufbau, Betrieb, Entwicklung

Stand 0.5.0. PIDA Web ist dieselbe Oberfläche wie die App, im Browser ohne Installation. Der PIDA-Kern und PDFium
laufen als WebAssembly in der Seite; ein kleiner Server (`pida-web-server`) liefert sie aus und stellt den
Rückkanal für das Signieren mit ID Austria bereit. Konzept und Entscheidungen: Notion-Seite „Konzept: PIDA als
Webdienst“ (Variante E, A-Trust-Overlay).

## Was die Web-Variante kann – und was nicht

- **Signieren nur mit ID Austria.** Chipkarte gibt es im Browser nicht; einen Keystore nur als verstecktes
  Testmerkmal (`?test-keystore` in der Adresse, für interne Tests – die Oberfläche bietet ihn sonst nirgends an).
- **QR-Code-Platzhalter werden immer gesucht:** Jede Sitzung beginnt mit eingeschalteter Platzhaltersuche
  „QR-Code-Platzhalter“ (Einstellungen › Dokument, für die Sitzung änderbar).
- **Nichts wird gespeichert:** jede Sitzung beginnt mit denselben Einstellungen, ohne Ersteinrichtung und ohne
  „Zuletzt signiert“; Änderungen gelten bis zum Schließen der Seite. In den Einstellungen bleiben Signaturbild
  (samt eigener Bildmarke für die Sitzung), Dokument, Datenschutz und Über PIDA.
- **Ergebnis als Download**, ein Stapel als ZIP (`PIDA-signiert.zip`).
- **Download-Banner** oben mit den Paketen der Apps (neueste Release über die GitHub-API).

## Ablauf beim Signieren mit ID Austria

1. Die Seite öffnet ein Overlay mit der echten A-Trust-Seite (iframe, sonst „In eigenem Fenster öffnen“) und
   schickt den ersten Security-Layer-Befehl (`InfoboxReadRequest`) per Formular-POST an A-Trust, mit
   `DataURL = https://<Domain>/api/sl/data/<Sitzung>`.
2. Anmeldung, SMS-TAN, App-Bestätigung und FIDO2 laufen vollständig auf der A-Trust-Seite. Zugangsdaten
   erreichen weder die Seite noch den PIDA-Server.
3. A-Trust schickt die Antwort an die DataURL. Der Server reicht sie an die Seite weiter und **hält die Anfrage
   von A-Trust offen**, bis die Seite den nächsten Befehl schickt (`CreateCMSSignatureRequest` mit dem Dokument
   als Base64); A-Trust führt ihn in derselben Sitzung aus – eine Anmeldung für Zertifikat und alle Dokumente
   eines Stapels (Befehlskette wie bei PDF-AS-Web).
4. Zum Schluss bekommt A-Trust eine Abschlussseite, das Overlay schließt sich.

**Datenschutz:** Durch die Befehlskette läuft das Dokument über den PIDA-Server zu A-Trust – nur im
Arbeitsspeicher, sofort verworfen; protokolliert werden Ereignisse ohne Inhalt und ohne Kennung. Der Text unter
Einstellungen › Datenschutz sagt das so.

**Geprüft:** 26.10.2026 mit A-Trust in Chrome (Server lokal über einen Cloudflare-Tunnel): iframe bedienbar,
DataURL und Folgebefehl funktionieren, Signatur in Ordnung. Noch offen: Safari und Firefox (Drittanbieter-Cookies
im iframe; Popup als Rückfall), FIDO2 im iframe.

## Betrieb auf einem Linux-Server

Voraussetzung: eine (Sub-)Domain mit **HTTPS** – A-Trust muss die DataURL aus dem Internet erreichen. Der Server
lauscht nur lokal; davor steht ein Webserver (Vorlage für Apache liegt den Paketen bei).

### Pakete

Jede Release enthält dasselbe statisch gebaute Programm (musl, x86_64) in drei Formaten:

| Distribution | Paket | Installation, Update | Entfernen |
|---|---|---|---|
| Debian, Ubuntu | `pida-web_<v>_amd64.deb` | `sudo apt install ./pida-web_<v>_amd64.deb` | `sudo apt remove pida-web` (`purge`: auch `/etc/pida-web`) |
| RHEL, Alma, Rocky, Fedora, openSUSE | `pida-web-<v>-1.x86_64.rpm` | `sudo dnf install ./pida-web-<v>-1.x86_64.rpm` | `sudo dnf remove pida-web` |
| alle übrigen | `pida-web-<v>-linux-x86_64.tar.gz` | entpacken, `sudo ./install.sh` | `sudo /opt/pida-web/uninstall.sh` (`--purge`) |

- **Neuinstallation** richtet den Dienst `pida-web` ein und startet ihn; **Update** = neue Version
  installieren, der Dienst startet neu; `/etc/pida-web/pida-web.env` (Adresse/Port, Standard
  `127.0.0.1:1442`) bleibt erhalten.
- Ablage (deb/rpm): Programm `/usr/bin/pida-web-server`, Oberfläche `/usr/share/pida-web/` (nur diese Dateien
  liefert der Server aus), Dienst `pida-web.service`, Dokumentation und Apache-Vorlage unter
  `/usr/share/doc/pida-web/`; Archiv: `/opt/pida-web`.
- Der Dienst läuft mit eigenem, rechtelosem Benutzer (`DynamicUser`) und strengen systemd-Schutzoptionen; er
  braucht keinen Schreibzugriff.

### Apache davor

```sh
sudo a2enmod proxy proxy_http headers ssl deflate brotli
sudo a2ensite pida-web                 # Debian: Vorlage liegt in /etc/apache2/sites-available/pida-web.conf
sudo certbot --apache -d pida.example.at
sudo systemctl reload apache2
```

In der Vorlage die Domain eintragen. RHEL & Co.: die Vorlage aus `/usr/share/doc/pida-web/` nach
`/etc/httpd/conf.d/pida-web.conf` kopieren und den Proxy unter SELinux erlauben
(`sudo setsebool -P httpd_can_network_connect 1`). Wichtig in der Vorlage: `LimitRequestBody` ≥ 28 MB (Befehle
mit dem Dokument als Base64, Grenze des Servers 28 MB; PDFs höchstens 20 MB wie bei A-Trust), `ProxyTimeout 630`,
Komprimierung für `application/wasm`.

**`ProxyTimeout`:** Der Server hält die DataURL-Anfrage von A-Trust, bis die Seite den nächsten Befehl schickt –
höchstens 10 Minuten. Wartet Apache kürzer (ältere Vorlage: 120 s), bricht er die Kette vorher mit einem 504 an
A-Trust ab, etwa wenn jemand zwischen Anmeldung und Signatur länger braucht. Bestehende Installationen: in
`pida-web.conf` die Zeile auf `ProxyTimeout 630` ändern und Apache neu laden (`apachectl configtest`, dann
`systemctl reload apache2` bzw. `httpd`); ein Paket-Update überschreibt eine angepasste Konfiguration nicht.

Apache hängt die Adresse des Clients selbst an `X-Forwarded-For` an (`mod_proxy_http`); der Server nimmt daraus den
letzten Eintrag für die Grenzen je Client. Dafür muss der Proxy auf derselben Maschine laufen (Verbindung über
`127.0.0.1`) – sonst zählt die Adresse des Proxys.

### Was der Server tut

| Pfad | Zweck |
|---|---|
| `/` | Oberfläche (`/usr/share/pida-web`) |
| `GET /api/config` | SL-Endpunkt von A-Trust für das Overlay |
| `POST /api/sl/sessions` | neue Sitzung (zufällige Kennung, Token nur für die Seite) |
| `POST /api/sl/data/<id>` | DataURL für A-Trust (Antwort halten, nächsten Befehl zurückgeben) |
| `GET /api/sl/sessions/<id>/response` | Long-Poll der Seite (25 s) |
| `POST /api/sl/sessions/<id>/command`, `…/finish`, `…/abort` | nächster Befehl bzw. Ende der Kette |

Sitzungen leben höchstens 30 Minuten, höchstens 2000 gleichzeitig; je Client (IP-Adresse) höchstens 200 offene
und 60 neue pro Minute (sonst `429`). Weitergegeben werden nur
`InfoboxReadRequest` und `CreateCMSSignatureRequest` im SL-Namespace; alle `/api/`-Antworten tragen
`Content-Security-Policy: sandbox`. Die Oberfläche selbst kommt mit einer Inhaltsrichtlinie: Skripte und
WebAssembly nur von der eigenen Adresse, Rahmen und Formularziele nur A-Trust (`*.a-trust.at`,
`*.handy-signatur.at`, der eingestellte SL-Endpunkt), Verbindungen nur zum eigenen Server und zur Release-Liste
auf GitHub, keine Einbettung in fremde Seiten. Sitzungen liegen im Speicher: **eine Instanz** je Domain (kein Load-Balancing
ohne Sticky Sessions).

## Entwicklung

```sh
apps/web/build.sh                                    # Oberfläche (vite --mode web) + Kern als WebAssembly → apps/web/dist
cargo run -p pida-web-server -- --mock-atrust testdata/keystores/test-rsa2048.p12 --mock-password test
# → http://127.0.0.1:1442/  (Testgegenstelle statt A-Trust: postet wie A-Trust an die DataURL)
```

- `apps/web/build.sh` braucht das Rust-Ziel `wasm32-unknown-unknown`, `wasm-bindgen-cli` 0.2.129 (unter
  `target/wasm-tools`) und lädt PDFium als WebAssembly über `tools/fetch-pdfium.sh wasm`. Mit
  `PIDA_WEB_NOTICES=<Ordner>` kommen die Drittlizenzen nach `dist/notices` („Über PIDA › Lizenzen“).
- Bausteine: `crates/fs` (Dateisystem im Speicher), `crates/core-api/src/rt.rs` (einfädige Laufzeit im Browser),
  `crates/backend-atrust-web` (ID Austria über SL-Befehlskette), `apps/web` (Web-Shell, wasm-bindgen),
  `apps/app/src/core/web.ts` und `ida-overlay.ts` (Brücke und Overlay), `apps/web-server` (Server, Rückkanal,
  Testgegenstelle, `packaging/`).
- Pakete lokal (Linux mit `dpkg-deb`, `rpmbuild`): `apps/web-server/packaging/package.sh`; in der CI baut und
  testet sie der Job „Web-Variante (Linux-Pakete)“ in `.github/workflows/release-bundles.yml` (Installation,
  Update, Entfernen in Debian- und Rocky-Containern, `packaging/test-install.sh`).
