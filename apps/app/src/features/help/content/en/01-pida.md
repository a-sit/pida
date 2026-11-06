# PIDA – the app

Translation of content/de/01-pida.md. Headings carry the German article id (`{#…}`) so that both languages share ids.

## What is PIDA? {#was-ist-pida}

PIDA signs PDF documents electronically – with ID Austria and, on a computer (desktop), also with a smart card or a
keystore (PKCS#12 file). PIDA is a functionally equivalent re-implementation of PDF-Over, the PDF signing tool by
A-SIT, but without Java. PIDA is available for Windows, macOS, Linux and Android, and as a browser version (web
service). As with PDF-Over, signatures are created according to the PDF-AS method (signature block and profiles from
PDF-AS). PIDA is free software under the EUPL-1.2 and an open-source project by A-SIT, not an official product of the
Republic of Austria or of A-Trust GmbH.

## Signing methods {#signiermittel}

- **ID Austria** (desktop, Android, browser): signature via A-Trust. Log in with your phone number or user name and
  your signature password; confirm in the “ID Austria” app (check the reference value), by SMS TAN or, on the
  desktop, with a FIDO2 security key. Some identities (e.g. test identities) have no second factor; A-Trust then signs
  right after login. In a batch, one login is enough for all documents.
- **Smart card** (desktop only): insert the card into the reader; PIDA talks to the locally installed smart card
  software (local BKU, e.g. on port 3495), where you enter the PIN.
- **Keystore** (desktop only): select a PKCS#12 file (.p12/.pfx) under Settings › Signing methods. JKS and JCEKS
  files must first be converted to PKCS#12 (e.g. with `keytool -importkeystore`). If you like, the password can be
  remembered in the system keychain. A keystore creates an advanced signature, not a qualified one (unless the key is
  stored on a qualified signature creation device).
- On Android and in the browser, only ID Austria is available.

## Using PIDA {#bedienung}

- **Workspace:** drag a PDF in or select one – PIDA places the signature block automatically on free space. Move it
  with the mouse, your finger or the arrow keys, or double-click (double-tap) to put it right where you want it; the
  block snaps to margins, below the text and into empty signature fields. You can also sign on a new page or invisibly (without a signature block).
- **Signature fields and QR placeholders** in the document are detected and offered as targets.
- **Demo:** “Start demo” opens a bundled demo document with a QR placeholder – to try PIDA without risk. Shortcut ⌘D
  or Ctrl+D, in the browser ⌥⇧D or Alt+Shift+D; also in the command palette and, on mobile devices, via “Try it with
  the demo document” on the start page. The signature is genuine, but the document has no legally relevant content.
- **Batch:** several PDFs at once – log in to ID Austria once and confirm each document individually. Protected or
  damaged files are skipped.
- **Signature block:** profiles “Standard (small)”, “Official signature” and more from PDF-AS, language of the
  signature block (German/English), your own emblem (default: the PIDA seal), note text – with a preview under
  Settings › Signature block.
- **PDF/A:** PDF/A documents are signed in a PDF/A-compliant way.
- **Done:** after signing, PIDA shows the result; “Open”, “Show in folder” (desktop), “Share” (Android) or “Download”
  (browser). “Recently signed” lists the latest documents (can be switched off).
- German and English, light and dark theme, command palette (⌘K or Ctrl+K), fully usable with the keyboard.
- **Switching from PDF-Over:** on first start, PIDA offers to take over the settings from PDF-Over. The PDF-Over
  start arguments keep working; in addition there is the command line `pida-cli`.

## Signature level and verification {#signaturniveau-und-prufung}

- PIDA creates signatures according to the PDF-AS method at the level PAdES-B-B: without a timestamp, without
  long-term validation data (DSS/LTV) and without certification (DocMDP).
- With ID Austria, the result is a qualified electronic signature (A-Trust is a qualified trust service provider).
- PIDA checks every new signature itself before saving (document unchanged, signature cryptographically intact) and
  only saves if this self-check succeeds. PIDA does not check the certificate's chain of trust or revocation status –
  use the signature verification at www.signaturpruefung.gv.at for that.

## Privacy {#datenschutz}

- **ID Austria:** for signing, the PDF document is sent to A-Trust (see A-Trust's privacy policy). A connection is only
  made once you explicitly start signing.
- **Smart card and keystore:** signing happens on your own device; the document is not uploaded.
- Settings, “Recently signed” and logs stay on the device. Logs contain no passwords, TANs or document contents.
- **Update check (desktop):** at most once a day, PIDA asks GitHub for the latest version; this can be switched off
  under Settings › Advanced.
- **Browser version:** documents and settings are not stored and are gone once you close the page. For ID Austria,
  PIDA shows the real A-Trust page in a window above the app; you enter your credentials directly at A-Trust. The
  document passes through the PIDA server – in memory only.

## Installation (short version) {#installation-kurzfassung}

- **Windows:** installer `…-setup.exe` (or `.msi` for managed installations, `…-portable.zip` without installation).
  SmartScreen warns because the installer is not code-signed: “More info” › “Run anyway”.
- **macOS:** open the `.dmg`, drag PIDA into “Applications”. The app is not notarised: up to macOS 14, right-click ›
  “Open”; from macOS 15, System Settings › Privacy & Security › “Open Anyway”.
- **Linux:** `.deb` (Debian, Ubuntu), `.rpm` (Fedora, RHEL) or AppImage.
- **Android:** download the `.apk` and allow installation from unknown sources.
- An iOS version is not part of the releases. The packages do not update themselves.

## Command line pida-cli {#kommandozeile-pida-cli}

`pida-cli` understands the PDF-Over start arguments (`-i -o -b -n -p -ks -kspass -ksalias -a -s -c -v` …) and adds
commands such as `verify` (self-check of a file), `profiles`, `migrate --dry-run`, `licenses`, as well as
`--headless` (no prompts) and `--json`. Examples:

- `pida-cli contract.pdf -b MOBILE` – sign with ID Austria, prompts in the terminal
- `pida-cli sign ./invoices -b KS -ks certificate.p12 --kspass-env PIDA_KSPASS -a -o ./signed --headless` – sign a
  whole folder with a keystore, password from an environment variable, automatic position, output to `./signed`,
  no prompts
- `pida-cli verify contract_signed.pdf` – self-check (without chain of trust and revocation status)
- `pida-cli --help` – all options and exit codes
