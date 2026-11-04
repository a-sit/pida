# Sicherheitsrichtlinie

*English summary below.*

PIDA erzeugt qualifizierte elektronische Signaturen. Wir nehmen Hinweise auf Sicherheitslücken ernst und
bitten um **vertrauliche Meldung**.

## Lücke melden

- **Bitte keine öffentlichen Issues, Diskussionen oder Pull-Requests** zu Sicherheitslücken.
- Melden Sie vertraulich über GitHub: **[Sicherheitslücke melden](https://github.com/a-sit/pida/security/advisories/new)**
  (Reiter *Security* → *Report a vulnerability*).
- Hilfreich sind: betroffene Version bzw. Commit, Plattform, Schritte zur Reproduktion, erwartetes und
  tatsächliches Verhalten, mögliche Auswirkungen sowie – falls vorhanden – ein Vorschlag zur Behebung.
- Bitte senden Sie **keine echten Zugangsdaten, TANs, Signaturzertifikate oder vertraulichen Dokumente** mit.
  Für Reproduktionen genügen die Testdaten unter `testdata/` (z. B. `testdata/keystores/`, nur Testschlüssel).

Wir bestätigen den Eingang so bald wie möglich, stimmen das weitere Vorgehen mit Ihnen ab und nennen Sie
auf Wunsch in der Veröffentlichung (Coordinated Disclosure).

## Unterstützte Versionen

PIDA befindet sich in der Vorab-Phase (`0.x`, Pre-Releases). Sicherheitskorrekturen fließen in `main` und in
das jeweils nächste Pre-Release; ältere Pre-Releases werden nicht nachgepflegt.

| Version | Unterstützt |
|---|---|
| `main` / neuestes Pre-Release | ✅ |
| ältere Pre-Releases | ❌ |

## Abgrenzung

- **ID Austria / A-Trust:** Schwachstellen in den Diensten von A-Trust oder der ID Austria bitte direkt an
  A-Trust bzw. die zuständige Stelle melden. Betrifft es das Zusammenspiel mit PIDA (z. B. die Verarbeitung
  der A-Trust-Seiten), melden Sie es hier.
- **PDF-Over (A-SIT):** Lücken im Original-PDF-Over bitte an dessen Projekt melden
  ([a-sit/PDF-Over](https://github.com/a-sit/PDF-Over)).
- **Abhängigkeiten:** Bekannte Lücken in Bibliotheken erkennen Dependabot und `cargo deny` (Advisories); ist
  PIDA dadurch konkret ausnutzbar, melden Sie es bitte hier.

## Grundsätze im Code

- Keine Telemetrie; Logs enthalten keine Passwörter, TANs, Tokens, Dokumentinhalte oder personenbezogenen
  Zertifikatsdaten (ADR 0001, Regel 9).
- Das ID-Austria-Passwort wird nie dauerhaft gespeichert; Keystore-Passwörter nur im Schlüsselbund des
  Betriebssystems (iOS Keychain, Android Keystore, macOS/Windows/Linux-Schlüsselbund).
- Jede Signatur wird vor dem Speichern selbst geprüft (Invarianten I1–I6); bestehende Signaturen bleiben
  unverändert (Append-only).

---

## English summary

Please **do not report security issues publicly**. Use GitHub's private
[vulnerability reporting](https://github.com/a-sit/pida/security/advisories/new) instead. Include the affected
version/commit, platform, reproduction steps and impact, and never send real credentials, TANs, certificates or
confidential documents (use the test data in `testdata/`). Only `main` and the latest pre-release receive fixes.
Issues in A-Trust/ID Austria services or in the original PDF-Over belong to those projects.
