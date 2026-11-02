### PIDA – Texte der neuen Oberfläche, UI-A (Shell, Start, Arbeitsbereich, Signierkarte, Fertig, Stapel), deutsch (EUPL-1.2)
###
### Von Hand gepflegt (ADR 0004 §4.5). IDs: `ng-ui-<bereich>-<name>`, Bereiche dieser Datei:
### shell, start, ws, insp, sc, fin, batch. Alle Kataloge einer Sprache bilden ein Fluent-Bündel: keine ID aus `pida.ftl`,
### `pdfover.ftl` oder der anderen `ui-*.ftl` wiederverwenden; jede ID mit denselben Variablen in
### `de/` und `en/` (Test `crates/i18n/tests/catalogs.rs`). Einzahl/Mehrzahl über Selektoren.
###
### Texte aus dem Prototyp (`prototypes/pida-workspace/src/i18n/de.ts`), angepasst an den Kern: Wortlaut nach
### ADR 0004 §8 F3 – nie „gültig“ oder „rechtsgültig“, sondern „Signatur erstellt und geprüft (Selbstprüfung)“
### und „laut Zertifikat“; Schrittweiten der Tastatur wie im Kern (15/1/50 pt).

## Shell: Kopfzeile, Befehle, Hinweise

ng-ui-shell-skip = Zum Inhalt springen
ng-ui-shell-dl-region = Downloads der Apps
ng-ui-shell-dl-banner = PIDA gibt es auch als App für Windows, macOS, Linux, iOS und Android.
ng-ui-shell-dl-cta = Zu den Downloads
ng-ui-shell-dl-hide = Hinweis ausblenden
ng-ui-shell-dl-title = PIDA als App
ng-ui-shell-dl-subtitle = Mit zusätzlichen Funktionen, etwa gespeicherten Einstellungen und – am Computer – Keystore und Chipkarte.
ng-ui-shell-dl-desktop = Computer
ng-ui-shell-dl-mobile = Smartphone und Tablet
ng-ui-shell-dl-github = Download auf GitHub
ng-ui-shell-dl-loading = Neueste Version wird bei GitHub abgefragt …
ng-ui-shell-dl-version = Version { $version }
ng-ui-shell-dl-version-test = Version { $version } (Testversion)
ng-ui-shell-dl-unavailable = Die Downloads sind gerade nicht abrufbar – alle Versionen gibt es auf GitHub.
ng-ui-shell-dl-download = Herunterladen
ng-ui-shell-dl-from-github = Die Downloads kommen direkt von GitHub; dafür fragt diese Seite GitHub nach der neuesten Version.
ng-ui-shell-dl-qr = QR-Code zur App für { $platform } (folgt)
ng-ui-shell-dl-appstore = App Store – folgt
ng-ui-shell-dl-play = Google Play – folgt
ng-ui-shell-dl-source = Quellcode und alle Versionen:
ng-ui-shell-help = Hilfe
ng-ui-shell-help-title = Hilfe
ng-ui-shell-help-subtitle = Funktioniert ohne Internet.
ng-ui-shell-help-search = Hilfe durchsuchen
ng-ui-shell-help-todo = Was Sie tun können:
ng-ui-shell-help-exact = In den „Technischen Details“ steht die genaue Meldung, z. B.: { $message }
ng-ui-shell-help-code = Code
ng-ui-shell-help-results = Suchergebnisse
ng-ui-shell-help-empty = Nichts gefunden zu „{ $q }“. Versuchen Sie es mit einem anderen Wort.
ng-ui-shell-help-back = Zurück
ng-ui-shell-help-context = Passend zu dieser Ansicht
ng-ui-shell-help-topics = Themen
ng-ui-shell-help-related = Verwandte Artikel
ng-ui-shell-help-legal = Allgemeine Information, keine Rechtsberatung.
ng-ui-shell-help-fehler-hint = Jede Meldung von PIDA hat einen Code (unter „Technische Details“). Am schnellsten finden Sie den passenden Artikel über die Suche oder „Was bedeutet das?“ an der Meldung.
ng-ui-shell-help-topic-pida = PIDA verwenden
ng-ui-shell-help-topic-id-austria = ID Austria
ng-ui-shell-help-topic-pdf = PDF-Signaturen und Prüfung
ng-ui-shell-help-topic-recht = Recht und eIDAS
ng-ui-shell-help-topic-fehler = Meldungen und Lösungen
ng-ui-shell-help-what = Was bedeutet das?
ng-ui-shell-home = PIDA – Startseite
ng-ui-shell-starting = PIDA wird gestartet …
ng-ui-shell-title-workspace = Arbeitsbereich
ng-ui-shell-doc-info = { $pages } · { $size }
ng-ui-shell-pages =
    { $n ->
        [one] { $n } Seite
       *[other] { $n } Seiten
    }
ng-ui-shell-commands = Befehle
ng-ui-shell-commands-long = Befehle und Suche
ng-ui-shell-lang = Sprache der Oberfläche
ng-ui-shell-theme = Farbschema
ng-ui-shell-theme-system = Wie System
ng-ui-shell-theme-light = Hell
ng-ui-shell-theme-dark = Dunkel
ng-ui-shell-settings = Einstellungen
ng-ui-shell-toasts = Benachrichtigungen
ng-ui-shell-toast-close = Hinweis schließen
ng-ui-shell-cmd-open = Dokument öffnen …
    .keywords = öffnen datei pdf laden
ng-ui-shell-cmd-sign = Signieren
    .keywords = unterschreiben signatur
ng-ui-shell-cmd-settings = Einstellungen
    .keywords = optionen konfiguration
ng-ui-shell-cmd-signature = Signaturbild bearbeiten
    .keywords = logo profil bildmarke hinweis
ng-ui-shell-cmd-home = Zur Startseite
    .keywords = start startseite home
ng-ui-shell-cmd-theme-light = Farbschema: Hell
    .keywords = theme hell
ng-ui-shell-cmd-theme-dark = Farbschema: Dunkel
    .keywords = theme dunkel
ng-ui-shell-cmd-theme-system = Farbschema: Wie System
    .keywords = theme system
ng-ui-shell-cmd-lang-de = Sprache: Deutsch
    .keywords = language sprache
ng-ui-shell-cmd-lang-en = Language: English
    .keywords = language sprache englisch
ng-ui-shell-cmd-shortcuts = Tastenkürzel anzeigen
    .keywords = hilfe tastatur kürzel
ng-ui-shell-cmd-add = PDF hinzufügen …
    .keywords = weitere dokumente mehrere datei anhängen stapel
ng-ui-shell-cmd-batch = Zum Stapel
    .keywords = stapel übersicht alle dokumente
ng-ui-shell-cmd-batch-back = Zurück zum Stapel
    .keywords = stapel übersicht alle dokumente
# „PDF hinzufügen“ in der Kopfzeile (ADR 0004 Nachtrag 9): Knopf „+ PDF“ mit Menü
ng-ui-shell-add = PDF
ng-ui-shell-add-long = PDF hinzufügen oder öffnen
ng-ui-shell-add-more = Weitere PDFs hinzufügen …
ng-ui-shell-add-more-desc = Zusammen mit „{ $name }“ als Stapel signieren
ng-ui-shell-replace = Anderes PDF öffnen …
ng-ui-shell-replace-desc = „{ $name }“ wird geschlossen
ng-ui-shell-replace-title = Anderes PDF öffnen?
ng-ui-shell-replace-text = Sie haben die Signatur in „{ $name }“ selbst platziert. Wenn Sie ein anderes PDF öffnen, wird dieses Dokument geschlossen und die Platzierung verworfen.
ng-ui-shell-replace-confirm = Anderes PDF öffnen
ng-ui-shell-key-close = Dialog schließen, Vorgang abbrechen
ng-ui-shell-key-accesskeys = Zugriffstasten (unterstrichene Buchstaben bei gedrückter Alt-Taste)
ng-ui-shell-key-move = Signaturblock verschieben (15 pt)
ng-ui-shell-key-move-fine = Fein verschieben (1 pt)
ng-ui-shell-key-move-coarse = Grob verschieben (50 pt)
ng-ui-shell-key-page = Block auf vorige / nächste Seite
ng-ui-shell-key-place = Block an diese Stelle setzen
ng-ui-shell-key-dblclick = Doppelklick
ng-ui-shell-key-zoom = Vergrößern / Verkleinern
ng-ui-shell-key-fit = An Breite anpassen

# Mehrfachauswahl (ADR 0004 Nachtrag 17): Titel bzw. Hinweiszeile der System-Dateiauswahl und Hinweis, wie dort
# mehrere PDFs markiert werden (macOS: ein Doppelklick übernimmt nur die angeklickte Datei; Android: Antippen übernimmt
# sofort genau eine Datei, mehrere erst nach langem Druck)
ng-ui-shell-pick-title =
    { $os ->
        [macos] PDF-Dokumente wählen – mehrere mit ⌘-Klick markieren, dann „Öffnen“
       *[other] PDF-Dokumente wählen – mehrere mit Strg-Klick markieren, dann „Öffnen“
    }
ng-ui-shell-pick-skipped =
    { $count ->
        [one] Eine gewählte Datei konnte nicht übernommen werden (nicht lesbar oder zu groß).
       *[other] { $count } gewählte Dateien konnten nicht übernommen werden (nicht lesbar oder zu groß).
    }

## Start

ng-ui-start-title = Was möchten Sie signieren?
ng-ui-start-lead = Öffnen Sie ein PDF – PIDA platziert die Signatur automatisch, Sie bestätigen mit Ihrem Signiermittel.
ng-ui-start-drop-label = Ablagefläche für PDF-Dokumente
ng-ui-start-drop-title = PDF hierher ziehen oder auswählen
ng-ui-start-drop-title-mobile = PDF auswählen
ng-ui-start-drop-release = Loslassen zum Öffnen
ng-ui-start-drop-hint = Mehrere Dateien werden als Stapel signiert – eine Anmeldung, jedes Dokument bestätigen Sie einzeln.
# je Ablauf des Stapels mit dem gewählten Signiermittel (Kern: `SignerView::batch_flow`, ADR 0004 Nachtrag 12)
ng-ui-start-drop-hint-unlock = Mehrere Dateien werden als Stapel signiert – der Keystore wird dafür einmal entsperrt.
ng-ui-start-drop-hint-each = Mehrere Dateien werden als Stapel signiert – jedes Dokument bestätigen Sie einzeln.
ng-ui-start-drop-hint-ask = Mehrere Dateien werden als Stapel signiert.
ng-ui-start-drop-hint-mobile = Oder ein PDF aus einer anderen App über „Teilen“ bzw. „Öffnen in“ an PIDA senden.
ng-ui-start-pick = PDF auswählen
    .accesskey = P
ng-ui-start-pick-mobile = Dokument auswählen
ng-ui-start-local = Ihre Dokumente bleiben auf diesem Gerät, bis Sie signieren.
ng-ui-start-shortcut = öffnet die Dateiauswahl
ng-ui-start-side = Zuletzt signiert
# Hinweis auf eine neue Version (Kern: `ViewState::update_hint`, Backlog Nr. 33)
ng-ui-start-update = PIDA { $version } ist verfügbar.
ng-ui-start-update-open = Zur Release-Seite
ng-ui-start-update-open-label = Release-Seite von PIDA { $version } auf GitHub öffnen
ng-ui-start-update-dismiss = Hinweis für diese Version ausblenden
# Geparkte Arbeit („Zur Startseite“ parkt das offene Dokument bzw. den Stapel, ADR 0004 Nachtrag 10)
ng-ui-start-parked-label = In Bearbeitung
ng-ui-start-parked-batch =
    { $n ->
        [one] Stapel · 1 Dokument
       *[other] Stapel · { $n } Dokumente
    }
ng-ui-start-parked-hint = Noch nicht signiert – die Platzierung bleibt erhalten.
ng-ui-start-parked-hint-placed = Noch nicht signiert – Ihre Platzierung bleibt erhalten.
ng-ui-start-parked-hint-batch = Pläne und Status der Dokumente bleiben erhalten.
ng-ui-start-parked-hint-unsaved = Signiert, aber noch nicht gespeichert.
ng-ui-start-parked-resume = Weiter bearbeiten
ng-ui-start-parked-close = Schließen
ng-ui-start-parked-close-title = Offene Arbeit schließen?
ng-ui-start-parked-close-text = „{ $name }“ wird geschlossen; Ihre Platzierung geht verloren. Bereits signierte Dateien bleiben erhalten.
ng-ui-start-parked-close-text-batch = Der Stapel wird geschlossen; Platzierungen und Status der Dokumente gehen verloren. Bereits signierte Dateien bleiben erhalten.
ng-ui-start-parked-close-text-unsaved = „{ $name }“ ist signiert, aber noch nicht gespeichert. Wenn Sie schließen, geht die Signatur verloren.
ng-ui-start-parked-close-confirm = Schließen
ng-ui-start-join-title = Zur offenen Arbeit hinzufügen?
ng-ui-start-join-text =
    „{ $name }“ ist noch in Bearbeitung. { $count ->
        [one] Soll das gewählte Dokument dazukommen? Zusammen bilden sie einen Stapel, Ihre Platzierung bleibt erhalten.
       *[other] Sollen die { $count } gewählten Dokumente dazukommen? Zusammen bilden sie einen Stapel, Ihre Platzierung bleibt erhalten.
    }
ng-ui-start-join-text-batch =
    Ein Stapel ist noch in Bearbeitung. { $count ->
        [one] Soll das gewählte Dokument dazukommen?
       *[other] Sollen die { $count } gewählten Dokumente dazukommen?
    }
ng-ui-start-join-add = Zum Stapel hinzufügen
ng-ui-start-join-replace = Offene schließen und neu beginnen
# Mehrfachauswahl (ADR 0004 Nachtrag 17): wie in der Dateiauswahl mehrere PDFs markiert werden
ng-ui-start-multi-hint =
    { $os ->
        [macos] Mehrere PDFs auf einmal: im Dialog mit ⌘-Klick markieren, dann „Öffnen“.
        [ios] Mehrere PDFs auf einmal: in der Dateiauswahl die PDFs antippen (Häkchen), dann „Öffnen“.
        [android] Mehrere PDFs auf einmal: in der Dateiauswahl ein PDF lange drücken, weitere antippen, dann „Auswählen“.
       *[other] Mehrere PDFs auf einmal: im Dialog mit Strg-Klick markieren, dann „Öffnen“.
    }

## Arbeitsbereich: Seiten, Sticker, Live-Region, Zustandskarten

ng-ui-ws-canvas = Dokumentvorschau: { $name }
ng-ui-ws-toolbar = Ansicht
ng-ui-ws-pages = Seiten
ng-ui-ws-thumb = Seite { $n }
ng-ui-ws-thumb-sticker = Seite { $n }, enthält den Signaturblock
ng-ui-ws-thumbs-add = PDF hinzufügen
ng-ui-ws-thumbs-add-hint = oder hierher ziehen
ng-ui-ws-toggle-pages = Seitenleiste ein-/ausblenden
ng-ui-ws-page-indicator = Seite { $n } / { $total }
ng-ui-ws-zoom-in = Vergrößern
ng-ui-ws-zoom-out = Verkleinern
ng-ui-ws-zoom-value = Zoom { $value } %
ng-ui-ws-fit = An Breite anpassen
ng-ui-ws-page-alt = Seite { $n } von { $total } – { $name }
ng-ui-ws-page-failed = Vorschau nicht verfügbar
ng-ui-ws-new-page = Neue Seite für die Signatur
ng-ui-ws-sticker-role = verschiebbarer Signaturblock
ng-ui-ws-sticker-label = Signaturblock
ng-ui-ws-sticker-help = Ziehen oder mit den Pfeiltasten verschieben, Umschalt+Pfeil verschiebt fein, Alt+Pfeil grob, Bild auf/ab wechselt die Seite, { $sign } signiert.
# Einstellung „Automatische Positionierung“ (ADR 0004 Nachtrag 8): Block gesperrt
ng-ui-ws-sticker-role-locked = automatisch platzierter Signaturblock
ng-ui-ws-sticker-locked-help = Automatisch platziert laut Einstellung „Automatische Positionierung“. { $sign } signiert; „Diesmal selbst platzieren“ unter „Platzierung“ gibt den Block zum Verschieben frei.
ng-ui-ws-sticker-overlap = Überdeckt Inhalt
ng-ui-ws-sticker-widget = Überdeckt ein Feld
ng-ui-ws-sticker-in-field = Im Signaturfeld
ng-ui-ws-sticker-in-qr = Im QR-Platzhalter
ng-ui-ws-placed = Signaturblock automatisch auf freier Fläche platziert
ng-ui-ws-placed-qr = Signaturblock automatisch im QR-Platzhalter platziert
ng-ui-ws-placed-field = Signaturblock automatisch im Signaturfeld platziert
ng-ui-ws-marker = Hier signieren
ng-ui-ws-marker-field = Hier signieren: Signaturfeld „{ $name }“
ng-ui-ws-marker-qr = Hier signieren: QR-Code-Platzhalter
ng-ui-ws-field-found = Signaturfeld gefunden
ng-ui-ws-field-banner = „{ $name }“ auf Seite { $page }. Wählen Sie dort „Hier signieren“ oder setzen Sie den Block direkt ins Feld.
ng-ui-ws-field-use = Ins Feld setzen
ng-ui-ws-qr-banner = Positioniert im QR-Code-Platzhalter des Dokuments.
ng-ui-ws-invisible = Unsichtbare Signatur – auf den Seiten erscheint kein Signaturbild.
ng-ui-ws-announce = Seite { $page } von { $pages }, linke obere Ecke { $left } mm von links, { $top } mm von unten.
ng-ui-ws-announce-snap = Rastet ein: { $where }.
ng-ui-ws-announce-overlap = Achtung: überdeckt Inhalt.
ng-ui-ws-announce-widget = Achtung: überdeckt ein Formularfeld.
ng-ui-ws-announce-free = Freie Fläche.
ng-ui-ws-announce-field = Im Signaturfeld „{ $name }“.
ng-ui-ws-announce-qr = Im QR-Code-Platzhalter.
ng-ui-ws-announce-qr-uncovered = Der QR-Platzhalter wird nicht ganz bedeckt.
ng-ui-ws-announce-invisible = Unsichtbare Signatur, kein Signaturbild.
ng-ui-ws-snap-left = linker Rand
ng-ui-ws-snap-right = rechter Rand
ng-ui-ws-snap-hcenter = Mitte
ng-ui-ws-snap-top = oberer Rand
ng-ui-ws-snap-bottom = unterer Rand
ng-ui-ws-snap-vcenter = Seitenmitte
ng-ui-ws-snap-below = unter dem Text
ng-ui-ws-snap-corner = rechts unten
ng-ui-ws-zone-top-left = links oben
ng-ui-ws-zone-top-center = oben mittig
ng-ui-ws-zone-top-right = rechts oben
ng-ui-ws-zone-middle-left = links mittig
ng-ui-ws-zone-middle-center = mittig
ng-ui-ws-zone-middle-right = rechts mittig
ng-ui-ws-zone-bottom-left = links unten
ng-ui-ws-zone-bottom-center = unten mittig
ng-ui-ws-zone-bottom-right = rechts unten
ng-ui-ws-loading-title = Dokument wird geöffnet
ng-ui-ws-loading = Das Dokument wird gelesen und geprüft …
ng-ui-ws-locked-title = Dieses PDF ist mit einem Kennwort geschützt
ng-ui-ws-locked-text = Dokumente mit einem Kennwort zum Öffnen kann PIDA nicht signieren. Öffnen Sie bitte eine Fassung ohne Kennwortschutz.
ng-ui-ws-locked-other = Anderes Dokument öffnen
ng-ui-ws-notpdf-title = Diese Datei ist kein PDF
ng-ui-ws-retry = Erneut versuchen

## Inspektor

ng-ui-insp-label = Signatur
ng-ui-insp-sign-with = Signieren mit
ng-ui-insp-switch = Signiermittel wechseln
ng-ui-insp-signer-menu = Signiermittel
ng-ui-insp-chip-label = Signiermittel: { $value }. Wechseln
ng-ui-insp-chip-label-single = Signiermittel: { $value }
ng-ui-insp-make-default = Als Standard verwenden
ng-ui-insp-manage = Einstellungen …
ng-ui-insp-signer-atrust = ID Austria
ng-ui-insp-signer-atrust-desc = Bestätigung in der App, mit FIDO2-Schlüssel oder per SMS-TAN
ng-ui-insp-signer-atrust-desc-mobile = Bestätigung in der App „ID Austria“ oder per SMS-TAN
ng-ui-insp-signer-keystore = Keystore
ng-ui-insp-signer-keystore-desc = Zertifikatsdatei (PKCS#12) auf diesem Gerät
ng-ui-insp-signer-card = Chipkarte
ng-ui-insp-signer-card-desc = Karte im Lesegerät mit lokaler Chipkarten-Software
ng-ui-insp-signer-card-reader = Chipkarte · Kartenleser
ng-ui-insp-signer-none = Signiermittel wählen
ng-ui-insp-block = Signaturbild
ng-ui-insp-block-edit = Signaturbild bearbeiten
ng-ui-insp-block-alt = Vorschau des Signaturbilds
ng-ui-insp-edit = Bearbeiten
ng-ui-insp-options = Optionen
ng-ui-insp-invisible = Unsichtbar signieren
ng-ui-insp-invisible-hint = Ohne Signaturbild; die Signatur steckt nur in der Datei.
ng-ui-insp-new-page = Auf neuer Seite
ng-ui-insp-new-page-hint = Hängt eine leere Seite für das Signaturbild an.
ng-ui-insp-block-lang = Sprache des Signaturblocks
ng-ui-insp-block-lang-hint = Gilt auch für weitere Dokumente (Einstellungen › Signaturbild).
ng-ui-insp-new-page-locked = Für dieses Dokument nicht möglich.
# Einstellungen › Dokument (E3): was beim Öffnen erkannt bzw. automatisch entschieden wurde
ng-ui-insp-detect-pdfa = PDF/A-Dokument erkannt – wird PDF/A-konform signiert
ng-ui-insp-detect-pdfa-forced = PDF/A-kompatible Signatur (erzwungen)
ng-ui-insp-detect-qr = QR-Platzhalter gefunden – Signatur dort platziert
ng-ui-insp-detect-field = Signaturfeld gefunden – Signatur im Feld platziert
ng-ui-insp-placement = Platzierung
ng-ui-insp-placement-text = Seite { $page } · { $zone }
ng-ui-insp-placement-field = Im Signaturfeld „{ $name }“
ng-ui-insp-placement-qr = Im QR-Code-Platzhalter
ng-ui-insp-placement-free = Frei platzieren
ng-ui-insp-placement-hint = Ziehen Sie den Block auf der Seite – er rastet an Rändern und unter dem Text ein.
ng-ui-insp-auto-place = Neu platzieren
# Einstellung „Automatische Positionierung“ (ADR 0004 Nachtrag 8)
ng-ui-insp-placement-auto = Automatisch (laut Einstellung)
ng-ui-insp-placement-locked-hint = Bereit zum Signieren. Die Lage bestimmt die Einstellung „Automatische Positionierung“.
ng-ui-insp-placement-unlock = Diesmal selbst platzieren
ng-ui-insp-existing = Vorhandene Signaturen
ng-ui-insp-existing-meta = Angabe beim Signieren: { $time }
ng-ui-insp-existing-unknown = Unterzeichner nicht lesbar
ng-ui-insp-status-valid = intakt
ng-ui-insp-status-invalid = verändert
ng-ui-insp-status-unknown = nicht prüfbar
ng-ui-insp-overlap = Der Signaturblock überdeckt Inhalt der Seite.
ng-ui-insp-overlap-widget = Der Signaturblock überdeckt ein Formular- oder Signaturfeld.
ng-ui-insp-overlap-fix = Freie Fläche suchen
ng-ui-insp-qr-uncovered = Der Signaturblock bedeckt den QR-Platzhalter nicht ganz – ein Teil des Platzhalterbilds bleibt sichtbar.
ng-ui-insp-emblem-placeholder = Für die Amtssignatur fehlt die Bildmarke Ihrer Behörde – der Signaturblock zeigt einen Platzhalter.
ng-ui-insp-emblem-placeholder-choose = Bildmarke wählen
ng-ui-insp-emblem-placeholder-mobile = Für die Amtssignatur fehlt die Bildmarke Ihrer Behörde – der Signaturblock zeigt einen Platzhalter. In der App lässt sich keine eigene Bildmarke festlegen.
ng-ui-insp-emblem-placeholder-qr = Der QR-Platzhalter gibt die Amtssignatur vor; sie zeigt einen Platzhalter statt der Bildmarke Ihrer Behörde.
# Hinweis zum Teilen auf iOS (Kern: `ViewState::share_hint`, ADR 0004 Nachtrag 12)
ng-ui-ws-share-hint = Mehrere PDFs geteilt? iOS übergibt beim Teilen nur das erste. Für einen Stapel wählen Sie die übrigen über „Weitere PDFs hinzufügen …“ – dort ist Mehrfachauswahl möglich.
ng-ui-ws-share-hint-dismiss = Hinweis ausblenden
ng-ui-insp-sign = Signieren
    .accesskey = S
ng-ui-insp-sheet-expand = Optionen einblenden
ng-ui-insp-sheet-collapse = Optionen ausblenden
ng-ui-insp-warning = Hinweis zum Dokument
ng-ui-insp-warning-proceed = Trotzdem signieren
ng-ui-insp-warning-discard = Dokument verwerfen

## Signierkarte

ng-ui-sc-title = Signieren mit { $signer }
ng-ui-sc-steps = Fortschritt
ng-ui-sc-step-login = Anmelden
ng-ui-sc-step-app = In der App bestätigen
ng-ui-sc-step-fido2 = Mit FIDO2 bestätigen
ng-ui-sc-step-sms = SMS-TAN eingeben
ng-ui-sc-step-unlock = Entsperren
ng-ui-sc-step-card = PIN eingeben
ng-ui-sc-step-done = Signiert
ng-ui-sc-state-done = erledigt
ng-ui-sc-state-current = aktueller Schritt
ng-ui-sc-state-todo = ausstehend
ng-ui-sc-state-error = fehlgeschlagen
ng-ui-sc-cancel = Abbrechen
ng-ui-sc-continue = Weiter
ng-ui-sc-details = Technische Details
ng-ui-sc-connecting = Verbindung wird aufgebaut …
ng-ui-sc-keystore-opening = Keystore wird geöffnet …
ng-ui-sc-certificate = Zertifikat wird gelesen …
ng-ui-sc-waiting-server = Warten auf den Signaturdienst …
ng-ui-sc-signing = Signatur wird erstellt …
ng-ui-sc-done = Signiert
ng-ui-sc-saving = Das signierte Dokument wird gespeichert …
ng-ui-sc-document = Dokument { $index } von { $total }
ng-ui-sc-login-user = Telefonnummer oder Benutzername
ng-ui-sc-login-password = Signatur-Passwort
ng-ui-sc-login-remember = Daten merken
ng-ui-sc-login-remember-hint = PIDA fragt bis zum Beenden nicht erneut nach dem Passwort; der Benutzername bleibt gespeichert.
ng-ui-sc-login-submit = Anmelden
ng-ui-sc-login-provider = Dieser Dienst wird von A-Trust bereitgestellt.
ng-ui-sc-app-sent = Benachrichtigung an die ID-Austria-App gesendet
ng-ui-sc-app-instruction = Öffnen Sie die App, vergleichen Sie den Wert und bestätigen Sie die Signatur.
ng-ui-sc-app-instruction-mobile = Bestätigen Sie in der App „ID Austria“ und kehren Sie dann zu PIDA zurück.
ng-ui-sc-app-remaining = Verbleibende Zeit: { $time }
ng-ui-sc-app-low-time = Noch eine Minute für die Bestätigung in der App.
ng-ui-sc-app-waiting = Warten auf Bestätigung …
ng-ui-sc-app-confirmed = In der App bestätigt
ng-ui-sc-ref = Vergleichswert
ng-ui-sc-ref-hint = Muss mit der Anzeige in der App übereinstimmen.
ng-ui-sc-data = Signaturdaten anzeigen
ng-ui-sc-alternatives = Stattdessen:
ng-ui-sc-alt-app = In der App bestätigen
ng-ui-sc-alt-fido2 = FIDO2-Schlüssel
ng-ui-sc-alt-sms = SMS-TAN
ng-ui-sc-qr-title = QR-Code mit der App „ID Austria“ scannen
ng-ui-sc-qr-continue = Weiter
ng-ui-sc-sms-text = Sie erhalten eine SMS mit der TAN.
ng-ui-sc-sms-tan = TAN aus der SMS
ng-ui-sc-sms-submit = Bestätigen
ng-ui-sc-sms-err-ref = Das ist der Vergleichswert – bitte geben Sie die TAN aus der SMS ein.
ng-ui-sc-fido2-title = Sicherheitsschlüssel verwenden
ng-ui-sc-fido2-text = Stecken Sie Ihren FIDO2-Sicherheitsschlüssel an und starten Sie die Bestätigung.
ng-ui-sc-fido2-start = Mit Schlüssel bestätigen
ng-ui-sc-fido2-touch = Berühren Sie jetzt Ihren Sicherheitsschlüssel
ng-ui-sc-fido2-touch-hint = Der Schlüssel blinkt, sobald er bereit ist.
ng-ui-sc-fido2-pin = PIN des Sicherheitsschlüssels
ng-ui-sc-fido2-pin-text = Geben Sie die PIN Ihres FIDO2-Sicherheitsschlüssels ein.
ng-ui-sc-fido2-pin-retry = Die PIN wurde nicht angenommen. Bitte versuchen Sie es noch einmal.
ng-ui-sc-ks-text = Geben Sie das Passwort für „{ $file }“ ein.
ng-ui-sc-ks-password = Keystore-Passwort
ng-ui-sc-ks-key-password = Passwort des Schlüssels
ng-ui-sc-ks-submit = Signieren
ng-ui-sc-proxy-title = Anmeldung am Proxy
ng-ui-sc-proxy-text = Der Proxy „{ $host }“ verlangt ein Passwort.
ng-ui-sc-proxy-password = Proxy-Passwort
ng-ui-sc-secret-empty = Bitte geben Sie das Passwort ein.
ng-ui-sc-card-text = Stecken Sie Ihre Chipkarte in das Lesegerät und geben Sie die Signatur-PIN in der Chipkarten-Software ein.
ng-ui-sc-card-waiting = Warten auf die Chipkarten-Software …
ng-ui-sc-err-new-confirmation = Neue Bestätigung anfordern
ng-ui-sc-error-title = Das hat nicht geklappt
ng-ui-sc-choose-title = Signiermittel wählen
ng-ui-sc-choose-text = Das ursprünglich gewählte Signiermittel ist nicht verfügbar. Womit möchten Sie signieren?
ng-ui-sc-choose-unavailable = Derzeit nicht verfügbar
ng-ui-sc-choose-sign = Signieren
# ohne Signiermittel-Auswahl (nur ein Signiermittel, ADR 0004 Nachtrag 20): kein „Signiermittel wählen“
ng-ui-sc-single-title = Weiter mit { $signer }
ng-ui-sc-single-text = Das ursprünglich gewählte Signiermittel ist nicht verfügbar. Signiert wird mit { $signer }.
ng-ui-sc-single-unavailable = { $signer } ist derzeit nicht verfügbar.

## Fertig

ng-ui-fin-title = Signiert
ng-ui-fin-seal = Signatur erstellt und geprüft (Selbstprüfung)
ng-ui-fin-text = PIDA hat die neue Signatur vor dem Speichern selbst geprüft. Vertrauenskette und Sperrstatus prüft signaturpruefung.gv.at.
ng-ui-fin-qes = Qualifiziert laut Zertifikat
ng-ui-fin-ades = Fortgeschritten laut Zertifikat
ng-ui-fin-saving = Das signierte Dokument wird gespeichert …
ng-ui-fin-not-saved = Das signierte Dokument ist noch nicht gespeichert.
ng-ui-fin-save = Speichern unter …
ng-ui-fin-save-failed-title = Das Dokument konnte nicht gespeichert werden
ng-ui-fin-save-failed-text = Bitte einen anderen Speicherort wählen oder erneut versuchen; die genaue Ursache steht unter „Technische Details“.
ng-ui-fin-saved-in = Gespeichert in { $folder }
ng-ui-fin-saved-mobile = Gespeichert in den Dokumenten dieser App
ng-ui-fin-signed-by = Signiert von { $name } · { $time }
ng-ui-fin-signed-at = Signiert am { $time }
ng-ui-fin-preview = Vorschau der signierten Seite
ng-ui-fin-open = Öffnen
ng-ui-fin-reveal = Im Ordner zeigen
ng-ui-fin-download = Herunterladen
ng-ui-fin-ready-web = Bereit zum Herunterladen – nach dem Schließen der Seite ist das Ergebnis weg.
ng-ui-fin-share = Teilen
ng-ui-fin-export = Sichern unter …
ng-ui-fin-check = Vollständige Prüfung bei signaturpruefung.gv.at
ng-ui-fin-next = Nächstes Dokument
ng-ui-fin-next-hint = Weiter mit „{ $name }“
ng-ui-fin-waiting =
    { $n ->
        [one] { $n } Dokument wartet
       *[other] { $n } Dokumente warten
    }
ng-ui-fin-more = Weiteres Dokument signieren
ng-ui-fin-overwrite-title = Datei überschreiben?
ng-ui-fin-overwrite = Überschreiben

## Stapel (ADR 0004 §3.8): eine Anmeldung, je Dokument eine eigene Bestätigung

ng-ui-batch-title = Stapel
ng-ui-batch-docs =
    { $n ->
        [one] { $n } Dokument
       *[other] { $n } Dokumente
    }
ng-ui-batch-lead = Alle Dokumente werden in einem Durchgang signiert: einmal anmelden, dann jedes Dokument einzeln bestätigen. Dokumente mit Fehler werden übersprungen.
ng-ui-batch-lead-unlock = Alle Dokumente werden in einem Durchgang signiert – der Keystore wird dafür einmal entsperrt. Dokumente mit Fehler werden übersprungen.
ng-ui-batch-lead-each = Alle Dokumente werden in einem Durchgang signiert, jedes bestätigen Sie einzeln. Dokumente mit Fehler werden übersprungen.
ng-ui-batch-lead-ask = Alle Dokumente werden in einem Durchgang signiert. Dokumente mit Fehler werden übersprungen.
ng-ui-batch-add = PDF hinzufügen
    .accesskey = H
ng-ui-batch-sign-all = Alle signieren
    .accesskey = A
ng-ui-batch-sign-all-long = Alle signieren ({ $n })
ng-ui-batch-estimate = Voraussichtlich
ng-ui-batch-estimate-logins =
    { $n ->
        [one] { $n } Anmeldung
       *[other] { $n } Anmeldungen
    }
ng-ui-batch-estimate-confirmations =
    { $n ->
        [one] { $n } Bestätigung
       *[other] { $n } Bestätigungen
    }
ng-ui-batch-estimate-cert = (ggf. +1 für das Zertifikat)
ng-ui-batch-estimate-keystore = Keystore-Passwort einmal
ng-ui-batch-estimate-none = Wählen Sie beim Signieren das Signiermittel.
ng-ui-batch-output-next = Speichern: jeweils neben dem Original
ng-ui-batch-output-folder = Speichern in { $folder }
ng-ui-batch-output-ask = Den Ordner für die signierten Dokumente wählen Sie beim Start.
ng-ui-batch-output-missing = Den Ordner „{ $folder }“ gibt es nicht – Sie wählen beim Start einen anderen.
ng-ui-batch-output-app = Speichern in den Dokumenten dieser App
ng-ui-batch-existing =
    { $n ->
        [one] Eine Zieldatei gibt es schon – Sie entscheiden beim Start.
       *[other] { $n } Zieldateien gibt es schon – Sie entscheiden beim Start.
    }
# Automatische Positionierung im Stapel (ADR 0004 Nachtrag 8): nur für diesen Stapel bzw. laut Einstellung
ng-ui-batch-auto = Für diesen Stapel automatisch positionieren
ng-ui-batch-auto-hint = Alle Dokumente werden automatisch platziert und sind bereit zum Signieren – nur für diesen Stapel, die Einstellung bleibt unverändert.
ng-ui-batch-auto-setting = Automatische Positionierung (laut Einstellung)
ng-ui-batch-auto-confirm-title =
    { $n ->
        [one] Eigene Platzierung behalten?
       *[other] Eigene Platzierungen behalten?
    }
ng-ui-batch-auto-confirm-text =
    { $n ->
        [one] Ein Dokument haben Sie selbst platziert. Soll es ebenfalls automatisch platziert werden?
       *[other] { $n } Dokumente haben Sie selbst platziert. Sollen sie ebenfalls automatisch platziert werden?
    }
ng-ui-batch-auto-keep = Behalten
ng-ui-batch-auto-replace =
    { $n ->
        [one] Auch dieses automatisch platzieren
       *[other] Auch diese automatisch platzieren
    }
ng-ui-batch-list = Dokumente im Stapel
ng-ui-batch-open = „{ $name }“ öffnen und positionieren
ng-ui-batch-review = Prüfen
ng-ui-batch-remove = „{ $name }“ aus dem Stapel entfernen
ng-ui-batch-remove-short = Entfernen
ng-ui-batch-status-analyzing = Wird geprüft
ng-ui-batch-status-ready = Bereit
ng-ui-batch-status-positioned = Positioniert
ng-ui-batch-status-rejected = Abgelehnt
ng-ui-batch-status-review = Bitte prüfen
ng-ui-batch-status-queued = Wartet
ng-ui-batch-status-active = Wird signiert
ng-ui-batch-status-signed = Signiert
ng-ui-batch-status-failed = Fehler
ng-ui-batch-status-skipped = Übersprungen
ng-ui-batch-status-cancelled = Abgebrochen
ng-ui-batch-detail-auto = automatisch platziert
ng-ui-batch-detail-checked = Position geprüft
ng-ui-batch-detail-qr = im QR-Platzhalter
ng-ui-batch-detail-field = im Signaturfeld „{ $name }“
ng-ui-batch-detail-manual = Seite { $page }, selbst platziert
ng-ui-batch-detail-invisible = unsichtbar
ng-ui-batch-detail-locked = Mit Kennwort geschützt
ng-ui-batch-detail-notpdf = Keine PDF-Datei
ng-ui-batch-detail-review = Hinweis zum Dokument – einzeln öffnen und bestätigen
ng-ui-batch-detail-signed = als { $file }
ng-ui-batch-detail-skipped = von Ihnen übersprungen
ng-ui-batch-detail-skipped-existing = Zieldatei gab es schon
ng-ui-batch-detail-cancelled = nicht signiert
ng-ui-batch-detail-exists = „{ $file }“ gibt es schon
ng-ui-batch-detail-waiting = wartet auf die Bestätigung
ng-ui-batch-stage-connecting = Verbindung wird aufgebaut
ng-ui-batch-stage-login = Anmeldung
ng-ui-batch-stage-certificate = Zertifikat wird gelesen
ng-ui-batch-stage-preparing = wird vorbereitet
ng-ui-batch-stage-confirming = Bestätigung läuft
ng-ui-batch-stage-signing = wird signiert
ng-ui-batch-stage-finishing = Signatur wird erstellt
ng-ui-batch-stage-saving = wird gespeichert
ng-ui-batch-stage-paused = angehalten
ng-ui-batch-progress = { $done } von { $total } signiert
ng-ui-batch-sum-title = Stapel abgeschlossen
ng-ui-batch-sum-title-aborted = Stapel abgebrochen
ng-ui-batch-sum-signed = { $signed } von { $total } signiert
ng-ui-batch-sum-skipped = { $n } übersprungen
ng-ui-batch-sum-rejected = { $n } abgelehnt
ng-ui-batch-sum-failed = { $n } fehlgeschlagen
ng-ui-batch-sum-cancelled = { $n } abgebrochen
ng-ui-batch-sum-not-started = { $n } nicht begonnen
ng-ui-batch-sum-folder = Gespeichert in { $folder }.
ng-ui-batch-sum-next = Jeweils neben dem Original gespeichert.
ng-ui-batch-sum-app = Gespeichert in den Dokumenten dieser App.
ng-ui-batch-reveal = Im Ordner zeigen
ng-ui-batch-download-zip = Alle als ZIP herunterladen
ng-ui-batch-new = Neuer Stapel
ng-ui-batch-back = Zurück zum Stapel
ng-ui-batch-show = Zum Stapel
ng-ui-batch-start-title =
    { $n ->
        [one] Ein Dokument signieren
       *[other] { $n } Dokumente signieren
    }
ng-ui-batch-start-folder = Speichern in
ng-ui-batch-start-folder-none = Noch kein Ordner gewählt
ng-ui-batch-start-folder-pick = Ordner wählen …
ng-ui-batch-start-folder-change = Anderen Ordner wählen …
ng-ui-batch-start-folder-dialog = Ordner für die signierten Dokumente
ng-ui-batch-start-existing = Wenn es die Zieldatei schon gibt
ng-ui-batch-start-existing-count =
    { $n ->
        [one] Für ein Dokument gibt es die Zieldatei schon.
       *[other] Für { $n } Dokumente gibt es die Zieldatei schon.
    }
ng-ui-batch-start-keep = Beide behalten
ng-ui-batch-start-keep-desc = Die neue Datei bekommt eine Nummer, z. B. „{ $example }“.
ng-ui-batch-start-overwrite = Überschreiben
ng-ui-batch-start-overwrite-desc = Die vorhandene Datei wird durch die signierte ersetzt.
ng-ui-batch-start-skip = Überspringen
ng-ui-batch-start-skip-desc =
    { $n ->
        [one] Dieses Dokument wird nicht signiert – ohne Bestätigung in der App.
       *[other] Diese Dokumente werden nicht signiert – ohne Bestätigung in der App.
    }
ng-ui-batch-start-skip-desc-local =
    { $n ->
        [one] Dieses Dokument wird nicht signiert.
       *[other] Diese Dokumente werden nicht signiert.
    }
ng-ui-batch-start-go = Signieren starten
ng-ui-sc-batch-doc = Dokument { $index } von { $total } · { $name }
ng-ui-sc-batch-confirm = Bestätigung { $index } von { $total } · { $name }
ng-ui-sc-batch-count = Bestätigungen
ng-ui-sc-batch-count-signed = Signiert
ng-ui-sc-batch-count-value = { $done } von { $total }
ng-ui-sc-batch-list = Alle Dokumente des Stapels
ng-ui-sc-batch-skip = Dokument überspringen
ng-ui-sc-batch-abort = Stapel abbrechen
ng-ui-sc-batch-abort-title = Stapel abbrechen?
ng-ui-sc-batch-abort-text = Bereits signierte Dokumente bleiben gespeichert, die übrigen werden nicht signiert. Das Passwort wird vergessen.
ng-ui-sc-batch-abort-text-local = Bereits signierte Dokumente bleiben gespeichert, die übrigen werden nicht signiert.
ng-ui-sc-batch-abort-keep = Weiter signieren
ng-ui-sc-batch-pause-title = Der Stapel ist angehalten
ng-ui-sc-batch-pause-decide = Bitte entscheiden Sie, wie es weitergeht.
ng-ui-sc-batch-pause-new-session = Erneut versuchen öffnet eine neue Sitzung – eine neue Anmeldung ist möglich.
ng-ui-sc-batch-pause-new-session-unlock = Erneut versuchen öffnet den Keystore neu – das Passwort wird gegebenenfalls erneut abgefragt.
ng-ui-sc-batch-pause-new-session-each = Erneut versuchen öffnet eine neue Sitzung.
ng-ui-sc-batch-pause-skip = Überspringen

## Barrierefreie Bedienung (docs/analyse/06): Schalter in der Kopfzeile, Platzierung ohne Vorschau, Prüfung vor dem Signieren
ng-ui-shell-a11y = Barrierefreie Bedienung
ng-ui-shell-a11y-desc = Platzierung ohne Vorschau, ausführliche Ansagen und Prüfung vor dem Signieren.
ng-ui-shell-a11y-on = Barrierefreie Bedienung ein. Rechts unter „Platzierung der Signatur“ wählen Sie, wie die Signatur platziert wird.
ng-ui-shell-a11y-off = Barrierefreie Bedienung aus.
ng-ui-ws-announce-zone = Lage: { $zone }.
ng-ui-insp-pos-title = Platzierung der Signatur
ng-ui-insp-pos-open = Ohne Vorschau platzieren …
ng-ui-insp-pos-method = Wie soll die Signatur platziert werden?
ng-ui-insp-pos-auto = Automatisch
ng-ui-insp-pos-auto-desc = Unter den letzten Text der letzten Seite
ng-ui-insp-pos-auto-do = Automatisch platzieren
ng-ui-insp-pos-placeholder = Platzhalter im Dokument
ng-ui-insp-pos-placeholder-desc = { $n ->
        [0] QR-Code-Platzhalter oder Signaturfeld – keiner gefunden
        [one] QR-Code-Platzhalter oder Signaturfeld – einer gefunden
       *[other] QR-Code-Platzhalter oder Signaturfeld – { $n } gefunden
    }
ng-ui-insp-pos-coords = Position in Millimetern
ng-ui-insp-pos-coords-desc = Seite und Abstände eingeben, z. B. aus einer Anleitung
ng-ui-insp-pos-free = Freie Fläche oder selbst verschieben
ng-ui-insp-pos-free-desc = Freie Flächen der Seite, Pfeiltasten oder Ziehen
ng-ui-insp-pos-found = Im Dokument gefunden
ng-ui-insp-pos-field-item = Signaturfeld „{ $name }“
ng-ui-insp-pos-qr-item = QR-Code-Platzhalter { $id }
ng-ui-insp-pos-qr-item-noid = QR-Code-Platzhalter
ng-ui-insp-pos-item-where = Seite { $page }, { $zone }
ng-ui-insp-pos-none = Kein QR-Code-Platzhalter und kein leeres Signaturfeld gefunden.
ng-ui-insp-pos-qr-search-hint = QR-Code-Platzhalter sucht PIDA nur mit „Signaturplatzhalter automatisch suchen“ (Einstellungen › Dokument).
ng-ui-insp-pos-settings-doc = Einstellungen › Dokument
ng-ui-insp-pos-page = Seite
ng-ui-insp-pos-x = Von links (mm)
ng-ui-insp-pos-y = Von unten (mm)
ng-ui-insp-pos-coords-hint = Gemessen wird die linke obere Ecke des Signaturblocks ab der linken unteren Seitenecke – wie bei PDF-AS. Seite { $page }: { $pw } × { $ph } mm, Signaturblock { $bw } × { $bh } mm.
ng-ui-insp-pos-coords-apply = Position übernehmen
ng-ui-insp-pos-err-page = Seite 1 bis { $max }.
ng-ui-insp-pos-err-x = 0 bis { $max } mm, damit der Block auf die Seite passt.
ng-ui-insp-pos-err-y = { $min } bis { $max } mm (Oberkante des Blocks).
ng-ui-insp-pos-free-list = Freie Flächen, in die der Block passt
ng-ui-insp-pos-free-item = Freie Fläche { $n }: Seite { $page }, { $zone }, { $w } × { $h } mm
ng-ui-insp-pos-free-none = Für die geladenen Seiten ist noch keine freie Fläche bekannt.
ng-ui-insp-pos-free-hint = Oder den Signaturblock in der Vorschau ziehen bzw. mit den Pfeiltasten verschieben.
ng-ui-insp-pos-to-block = Zum Signaturblock
ng-ui-insp-pos-current = Aktuelle Position
ng-ui-insp-pos-step-standard = 15 pt (Standard)
ng-ui-insp-pos-quick = Ansagen und Bedienung anpassen
ng-ui-insp-pos-all-settings = Alle Einstellungen zur Barrierefreiheit
ng-ui-insp-check-title = Vor dem Signieren prüfen
ng-ui-insp-check-placement = Platzierung
ng-ui-insp-check-block = Signaturbild
ng-ui-insp-check-block-size = { $w } × { $h } mm
ng-ui-insp-check-invisible = Unsichtbar – ohne Signaturbild
ng-ui-insp-check-signer = Signieren mit
ng-ui-insp-check-overlap = Achtung: Der Signaturblock überdeckt Inhalt der Seite. Wer das Dokument bekommt, kann diesen Inhalt dann nicht mehr lesen.
ng-ui-insp-check-back = Zurück zur Platzierung
ng-ui-insp-check-sign = Signieren

## QR-Platzhalter erstellen (ADR 0004 Nachtrag 24)
ng-ui-shell-qr-title = QR-Platzhalter erstellen
ng-ui-shell-qr-subtitle = Für Dokumente, die andere unterschreiben: Der Platzhalter legt fest, wo die Signatur hinkommt.
ng-ui-shell-qr-id = Nummer
ng-ui-shell-qr-id-hint = Für mehrere Unterschriften je eine eigene Nummer, z. B. 0001 und 0002 – und den Unterzeichnenden ihre Nummer mitteilen.
ng-ui-shell-qr-profile = Signaturbild
ng-ui-shell-qr-profile-standard = Standard
ng-ui-shell-qr-profile-standard-desc = Signaturblock für Unterschriften von Personen
ng-ui-shell-qr-profile-amtssignatur = Amtssignatur
ng-ui-shell-qr-profile-amtssignatur-desc = Für Behörden
ng-ui-shell-qr-profile-signer = Wie bei der unterzeichnenden Person eingestellt
ng-ui-shell-qr-profile-signer-desc = Deren eigenes Signaturbild, z. B. mit Bildmarke
ng-ui-shell-qr-lang = Sprache des Signaturblocks
ng-ui-shell-qr-lang-caption = Sprache der Beschriftung
ng-ui-shell-qr-alt = QR-Code-Platzhalter Nummer { $id }, Signaturbild { $profile }, { $lang }
ng-ui-shell-qr-howto = Fügen Sie das Bild in Ihr Dokument ein, so breit, wie der Signaturblock werden soll (gespeichert mit 8,5 cm). Beim Signieren ersetzt der Signaturblock den Platzhalter – in PIDA, PDF-Over und PDF-AS.
ng-ui-shell-qr-save-png = Als PNG speichern
ng-ui-shell-qr-save-svg = Als SVG speichern
ng-ui-shell-qr-share-png = PNG teilen
ng-ui-shell-qr-share-svg = SVG teilen
ng-ui-shell-cmd-qr = QR-Platzhalter erstellen …
    .keywords = qr platzhalter signaturplatzhalter vorlage vorbereiten unterzeichnende mehrere dokument
ng-ui-shell-cmd-demo = Demo starten
    .keywords = demo demodokument beispiel ausprobieren testen probieren
ng-ui-start-demo = Mit Demodokument ausprobieren
ng-ui-start-qr-title = Dokument für andere vorbereiten
ng-ui-start-qr-text = Mit einem QR-Platzhalter legen Sie fest, wo die Signatur hinkommt – auch für mehrere Unterzeichnende.
ng-ui-start-qr-action = QR-Platzhalter erstellen
