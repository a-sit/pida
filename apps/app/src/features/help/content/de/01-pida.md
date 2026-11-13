# PIDA – die App

Quelle: README von PIDA (Stand 0.2.5), Quellcode. Geprüft gegen den Code; Stand Oktober 2026.

## Was ist PIDA?

PIDA signiert PDF-Dokumente elektronisch – mit ID Austria, am Computer (Desktop) auch mit einer Chipkarte oder einem
Keystore (PKCS#12-Datei). PIDA ist ein funktional gleichwertiger Nachbau von PDF-Over, dem PDF-Signaturwerkzeug von
A-SIT, aber ohne Java. Es gibt PIDA für Windows, macOS, Linux und Android sowie als Browser-Variante (Webdienst).
Die Signaturen entstehen wie bei PDF-Over nach dem PDF-AS-Verfahren (Signaturblock und Profile aus PDF-AS). PIDA ist
freie Software unter der EUPL-1.2 und ein Open-Source-Projekt von A-SIT, kein offizielles Produkt der Republik
Österreich oder der A-Trust GmbH.

## Signiermittel

- **ID Austria** (Desktop, Android, Browser): Signatur über A-Trust. Anmeldung mit Telefonnummer oder Benutzername
  und Signatur-Passwort; bestätigt wird in der App „ID Austria“ (Vergleichswert prüfen), per SMS-TAN oder am Desktop
  mit einem FIDO2-Sicherheitsschlüssel. Manche Identitäten (z. B. Test-Identitäten) haben keinen zweiten Faktor; dann
  signiert A-Trust direkt nach der Anmeldung. Im Stapel genügt eine Anmeldung für alle Dokumente.
- **Chipkarte** (nur Desktop): Karte ins Lesegerät stecken; PIDA spricht die lokal installierte Chipkarten-Software
  an (lokale BKU, z. B. auf Port 3495), die PIN wird dort eingegeben.
- **Keystore** (nur Desktop): PKCS#12-Datei (.p12/.pfx) unter Einstellungen › Signiermittel wählen. JKS- und
  JCEKS-Dateien müssen vorher nach PKCS#12 umgewandelt werden (z. B. mit `keytool -importkeystore`). Das Passwort kann
  auf Wunsch im Schlüsselbund des Systems gemerkt werden. Ein Keystore erzeugt eine fortgeschrittene Signatur, keine
  qualifizierte (außer der Schlüssel liegt auf einer qualifizierten Signaturerstellungseinheit).
- Auf Android und im Browser gibt es nur ID Austria.

## Bedienung

- **Arbeitsbereich:** PDF hineinziehen oder auswählen – PIDA platziert den Signaturblock automatisch auf freier
  Fläche. Verschieben mit Maus, Finger oder Pfeiltasten, oder per Doppelklick bzw. Doppeltippen direkt an eine Stelle
  setzen; der Block rastet an Rändern, unter dem Text und in leere Signaturfelder ein. Signatur auch auf einer neuen Seite oder unsichtbar (ohne Signaturbild).
- **Signaturfelder und QR-Platzhalter** im Dokument werden erkannt und als Ziel angeboten.
- **Mehrere Signaturblöcke:** Für Dokumente mit mehreren Unterschriftsstellen fügt „Weiteren Block hinzufügen“
  (unter „Signaturblöcke“) einen weiteren Block ein, höchstens zehn je Dokument. Die Blöcke sind nummeriert; ein Klick
  wählt einen Block aus, verschoben und platziert wird immer der ausgewählte. „Alle Platzhalter belegen“ setzt in
  jedes leere Signaturfeld einen Block. Jeder Block erhält eine eigene Signatur, in der Reihenfolge der Nummern –
  der Knopf heißt dann zum Beispiel „3-mal signieren“. Mit ID Austria kann für jede Signatur eine eigene Bestätigung
  nötig sein. Bricht der Vorgang ab, wird nichts gespeichert. Mit „Auf neuer Seite“ dürfen auch mehrere Blöcke auf der
  neuen Seite liegen; sie werden zuerst signiert und bekommen deshalb automatisch die ersten Nummern.
- **Demo:** „Demo starten“ öffnet ein mitgeliefertes Demodokument mit QR-Platzhalter – zum gefahrlosen Ausprobieren.
  Kürzel ⌘D bzw. Strg+D, im Browser ⌥⇧D bzw. Alt+Umschalt+D; auch in der Befehlspalette und auf Mobilgeräten über
  „Mit Demodokument ausprobieren“ auf der Startseite. Signiert wird echt, das Dokument hat aber keinen rechtlich
  bedeutsamen Inhalt.
- **Stapel:** mehrere PDFs auf einmal – einmal bei ID Austria anmelden, jedes Dokument einzeln bestätigen.
  Geschützte oder fehlerhafte Dateien werden übersprungen.
- **Signaturbild:** Profile „Standard (klein)“, „Amtssignatur“ und weitere aus PDF-AS, Sprache des Signaturblocks
  (Deutsch/Englisch), eigene Bildmarke (Standard: PIDA-Siegel), Hinweistext – mit Vorschau unter Einstellungen ›
  Signaturbild.
- **PDF/A:** PDF/A-Dokumente werden PDF/A-konform signiert.
- **Fertig:** Nach dem Signieren zeigt PIDA das Ergebnis; „Öffnen“, „Im Ordner zeigen“ (Desktop), „Teilen“ (Android)
  bzw. „Herunterladen“ (Browser). „Zuletzt signiert“ listet die letzten Dokumente (abschaltbar).
- Deutsch und Englisch, helles und dunkles Design, Befehlspalette (⌘K bzw. Strg+K), vollständig per Tastatur
  bedienbar.
- **Umstieg von PDF-Over:** Beim ersten Start bietet PIDA an, die Einstellungen aus PDF-Over zu übernehmen. Die
  Startargumente von PDF-Over funktionieren weiter; dazu gibt es die Kommandozeile `pida-cli`.

## Signaturniveau und Prüfung

- PIDA erstellt Signaturen nach dem PDF-AS-Verfahren auf dem Niveau PAdES-B-B: ohne Zeitstempel, ohne
  Langzeitprüfdaten (DSS/LTV) und ohne Zertifizierung (DocMDP).
- Mit ID Austria entsteht eine qualifizierte elektronische Signatur (A-Trust ist qualifizierter
  Vertrauensdiensteanbieter).
- PIDA prüft jede neue Signatur vor dem Speichern selbst (Dokument unverändert, Signatur kryptografisch intakt) und
  speichert nur, wenn diese Selbstprüfung gelingt. Vertrauenskette und Sperrstatus des Zertifikats prüft PIDA nicht –
  dafür gibt es die Signaturprüfung unter www.signaturpruefung.gv.at.

## Datenschutz

- **ID Austria:** Zum Signieren wird das PDF-Dokument an A-Trust übermittelt (Datenschutzerklärung von A-Trust). Eine
  Verbindung entsteht erst, wenn das Signieren ausdrücklich gestartet wird.
- **Chipkarte und Keystore:** Signiert wird auf dem eigenen Gerät; das Dokument wird nicht hochgeladen.
- Einstellungen, „Zuletzt signiert“ und Protokolle bleiben auf dem Gerät. Protokolle enthalten keine Passwörter,
  TANs oder Dokumentinhalte.
- **Update-Prüfung (Desktop):** höchstens einmal am Tag fragt PIDA bei GitHub nach der neuesten Version; abschaltbar
  unter Einstellungen › Erweitert.
- **Browser-Variante:** Dokumente und Einstellungen werden nicht gespeichert und sind nach dem Schließen der Seite weg.
  Für ID Austria zeigt PIDA die echte A-Trust-Seite in einem Fenster über der App; Zugangsdaten gibt man direkt bei
  A-Trust ein. Das Dokument läuft dabei – nur im Speicher – über den PIDA-Server.

## Installation (Kurzfassung)

- **Windows:** Installer `…-setup.exe` (oder `.msi` für verteilte Installationen, `…-portable.zip` ohne
  Installation). SmartScreen warnt, weil der Installer nicht code-signiert ist: „Weitere Informationen“ ›
  „Trotzdem ausführen“.
- **macOS:** `.dmg` öffnen, PIDA in „Programme“ ziehen. Die App ist nicht notarisiert: bis macOS 14 Rechtsklick ›
  „Öffnen“; ab macOS 15 Systemeinstellungen › Datenschutz & Sicherheit › „Dennoch öffnen“.
- **Linux:** `.deb` (Debian, Ubuntu), `.rpm` (Fedora, RHEL) oder AppImage.
- **Android:** `.apk` herunterladen, Installation aus unbekannten Quellen erlauben.
- Eine iOS-Version ist nicht Teil der Releases. Die Pakete aktualisieren sich nicht selbst.

## Kommandozeile pida-cli

`pida-cli` versteht die Startargumente von PDF-Over (`-i -o -b -n -p -ks -kspass -ksalias -a -s -c -v` …) und ergänzt
Befehle wie `verify` (Selbstprüfung einer Datei), `profiles`, `migrate --dry-run`, `licenses`, sowie `--headless`
(ohne Rückfragen) und `--json`. Beispiele:

- `pida-cli vertrag.pdf -b MOBILE` – mit ID Austria signieren, Rückfragen im Terminal
- `pida-cli sign ./rechnungen -b KS -ks zertifikat.p12 --kspass-env PIDA_KSPASS -a -o ./signiert --headless` – einen
  ganzen Ordner mit einem Keystore signieren, Passwort aus einer Umgebungsvariable, automatische Position, Ausgabe in
  `./signiert`, ohne Rückfragen
- `pida-cli verify vertrag_signed.pdf` – Selbstprüfung (ohne Vertrauenskette und Sperrstatus)
- `pida-cli --help` – alle Optionen und Exit-Codes
