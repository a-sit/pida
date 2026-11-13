### PIDA – own texts, English (EUPL-1.2)
###
### Maintained by hand. Every ID needs an entry in `de/` and `en/` with the same variables
### (test `crates/i18n/tests/code_catalog.rs`). Two kinds of IDs:
###
### - Own texts (package E and UI): prefix `ng-`, so they never collide with the IDs generated from
###   PDF-Over (`pdfover.ftl`).
### - Stable codes of the crates (ADR 0001 rule 11, plan §7.5): the ID is the code itself
###   (`cms-der`, `backend-network`, `fido2-timeout` …). Codes with a dot (`render.failed`,
###   `placement.notice.clamped`) are not valid in Fluent; their ID is derived like the PDF-Over keys
###   with `pida_i18n::fluent_id_for_java_key` (dot → hyphen: `render-failed`,
###   `placement-notice-clamped`).
###
### Variables are named after the fields of the error variant (`$detail` for free text). The test
### checks that every code produced by the sources (`fn code`, `fn message_id`, `NOTICE_*`, code
### literals) is present here in both languages.

## Configuration: errors (`pida_config::ConfigError::message_id`)

ng-config-error-io = Input/output error in the settings: { $detail }
    .text = The settings could not be read or saved.
ng-config-error-parse = The settings cannot be read: { $detail }
    .text = The saved settings cannot be read.
ng-config-error-serialize = The settings could not be saved: { $detail }
    .text = The settings could not be saved.
ng-config-error-unsupported-schema = The settings were written by a newer version (schema { $found }, supported up to { $supported }).
    .text = The settings come from a newer version of PIDA.
ng-config-error-legacy = The PDF-Over configuration cannot be read: { $detail }
    .text = The PDF-Over settings cannot be read.
ng-config-error-no-home = The configuration folder cannot be determined.
ng-config-error-secret = Error accessing the keychain: { $detail }
    .text = The system keychain cannot be accessed.
ng-config-file-missing = The configuration file “{ $path }” was not found.
ng-config-reset-done = The previous settings were moved to “{ $path }”.
ng-secrets-not-persistent = No keychain available: passwords are only kept until PIDA exits.

## Configuration: reading PDF-Over.config (`pida_config::LegacyWarning::code`)

ng-config-legacy-invalid-bku = BKU: the value “{ $value }” is invalid; no default is used.
ng-config-legacy-invalid-proxy-port = PROXY_PORT: “{ $value }” is not a valid number and is ignored.
ng-config-legacy-proxy-port-out-of-range = PROXY_PORT: { $value } is outside 1 to 65535 and is ignored.
ng-config-legacy-invalid-value = { $key }: the value “{ $value }” is invalid; the default applies.
ng-config-legacy-unknown-locale = { $key }: “{ $value }” is not a valid language; the system language applies.
ng-config-legacy-unsupported-locale = { $key }: the language “{ $value }” is not supported; { $lang } applies.
ng-config-legacy-unknown-profile = SIGNATURE_PROFILE: “{ $value }” is unknown; the standard signature block applies.
ng-config-legacy-unknown-pass-store-type = KEYSTORE_PASS_STORE_TYPE: “{ $value }” is unknown; keystore passwords are not saved.

## Configuration: import from ~/.pdf-over (`pida_config::migrate`)

ng-migrate-offer-title = Import settings from PDF-Over?
ng-migrate-offer-text = Settings of a previous PDF-Over installation were found (“{ $path }”). Do you want to import them? The old file remains unchanged.
ng-migrate-secrets-consent = The old configuration contains passwords in plain text. Do you want to move them to the operating system keychain?
ng-migrate-remove-plaintext = The passwords are still stored in plain text in “{ $path }”. You can have them removed there.
ng-migrate-done = Imported: { $imported } settings; not imported: { $skipped }.
ng-migrate-undo = Undo import
ng-migrate-key-ignored = { $key } is not imported (no function in PIDA).
ng-migrate-unknown-key = The unknown entry { $key } is ignored.
ng-migrate-path-missing = { $key }: the path “{ $path }” does not exist.
ng-migrate-path-dropped = { $key }: the path “{ $path }” does not exist and is not imported.
ng-migrate-emblem-unsupported = EMBLEM: “{ $path }” is not a supported image (PNG, JPEG, GIF) and is not imported.
ng-migrate-emblem-from-cache = EMBLEM: the original “{ $path }” is missing; the emblem cached by PDF-Over is imported.
ng-migrate-emblem-default = EMBLEM: “{ $path }” is the PDF-AS default emblem, not an own emblem; it is not imported (the PIDA seal remains).
ng-migrate-secret-ignored = { $key }: the password is not imported because the storage type is not “permanent”.
ng-migrate-secret-no-target = { $key }: the password cannot be assigned without the matching keystore file or proxy host and user.
ng-migrate-signature-field-discarded = The signature field permanently stored by PDF-Over is discarded (B-05); the selection now applies to the current document only.
ng-migrate-custom-profile = Custom PDF-AS profile found: “{ $file }”.
ng-migrate-note-default = The signature note equals the default text and will follow profile and language from now on.
ng-migrate-placeholder-search-off = Placeholder search was enabled, but neither QR code nor signature field search was selected; it stays disabled.
ng-migrate-proxy-without-host = PROXY_PORT or PROXY_USER without PROXY_HOST are not imported.
ng-migrate-keystore-jks = Keystore type { $value }: JKS or JCEKS keystores must be converted to PKCS#12 before use.

## CMS signatures (`pida_cades::CmsError::code`)

cms-der = The signature data (CMS) cannot be parsed.
    .text = The signature received is faulty and was discarded. Please try again.
cms-certificate = A certificate cannot be read.
    .text = The certificate cannot be read.
cms-unsupported-algorithm = The signature or hash algorithm is not supported.
    .text = The signature method of this certificate is not supported.
cms-not-signed-data = The data is not a CMS signature (SignedData).
    .text = The signature received is faulty and was discarded. Please try again.
cms-signer-count = The signature does not contain exactly one signer.
    .text = The signature received is faulty and was discarded. Please try again.
cms-missing-attribute = A required signature attribute is missing.
    .text = The signature received is faulty and was discarded. Please try again.
cms-digest-mismatch = The signature digest does not match the document.
    .text = The signature received is faulty and was discarded. Please try again.
cms-invalid-signature = The signature value is invalid.
    .text = The signature received is faulty and was discarded. Please try again.
cms-signer-certificate-missing = The signer certificate is missing from the signature.
    .text = The signature received is faulty and was discarded. Please try again.
cms-signing-certificate-mismatch = The signing-certificate attribute refers to a different certificate.
    .text = The signature received is faulty and was discarded. Please try again.
cms-unexpected-signer = The signature was created with a certificate other than the expected one.
cms-key = The key cannot be used.
    .text = The key cannot be used.
cms-key-mismatch = The private key does not match the certificate.
    .text = The key and the certificate do not match.
cms-too-large = The signature exceeds the reserved space.
cms-io = Read error while verifying the signature.
    .text = The signature received is faulty and was discarded. Please try again.

## Keystore (`pida_backend_keystore::KeystoreError::code`)

keystore-file-not-found = The keystore file was not found.
keystore-wrong-password = The keystore password is wrong.
keystore-alias-not-found = The alias was not found in the keystore.
keystore-not-a-key-entry = The alias is not a key entry.
keystore-conversion-needed = The keystore format (JKS/JCEKS) is not supported – please convert it to PKCS#12.
keystore-unsupported-pbe = The keystore encryption is not supported – please export it again.
keystore-different-key-password = The key is protected by a separate password – please export the PKCS#12 with a single password.
keystore-unsupported-key = The key type is not supported.
keystore-cms = The certificate or key in the keystore cannot be used.
keystore-io = The keystore file cannot be read.
keystore-malformed = The file is not a valid PKCS#12 keystore.
keystore-no-key-entry = The keystore contains no key with a certificate.
keystore-alias-required = The keystore contains several keys – please specify an alias.
keystore-key-mismatch = The key and certificate in the keystore do not match.

## Signature backends (`pida_backend_api::BackendError::code`); variables = fields of the variant (the core
## delivers them, `WorkflowError::params`). `.text` is the everyday text of the UI **without** variables – the cause
## is shown under “Technical details” (ADR 0004 addendum 19; test `error_codes_have_an_everyday_text_without_variables`).

backend-cancelled = The operation was cancelled.
    .title = Operation cancelled
    .action = Try again
backend-unavailable = The signing method is not available: { $detail }
    .text = The signing method is not available right now. The exact cause is shown under “Technical details”.
backend-network = The connection to the signature service failed: { $detail }
    .title = The signature service cannot be reached right now
    .text = The connection to the signature service failed. Check your internet connection and try again; the exact cause is shown under “Technical details”.
    .action = Try again
backend-proxy-auth = The proxy server requires authentication.
backend-document-too-large = The document exceeds the size limit of the signature service.
backend-http = The signature service rejected the request: HTTP { $status } { $reason }
    .text = The signature service rejected the request. Please try again later; the exact cause is shown under “Technical details”.
backend-security-layer = Operation aborted by the signature server ({ $code }): { $info }
    .text = The signature service aborted the operation. The exact cause is shown under “Technical details”.
backend-unknown-page = Unknown page of the signature service ({ $diagnostic }). The operation was cancelled.
    .text = A-Trust showed a page that PIDA does not know; the process was stopped. Please try again; the exact cause is under “Technical details”.
backend-loop = The signature service is responding in an endless loop; the operation was cancelled.
    .text = The signing service did not respond as expected; the process was stopped. Please try again.
backend-protocol = Unexpected response from the signature service: { $detail }
    .text = The signature service responded unexpectedly; the operation was cancelled. The exact cause is shown under “Technical details”.
backend-wrong-password = The password is wrong.
backend-keystore = Keystore error: { $detail }
    .text = The keystore could not be used. The exact cause is shown under “Technical details”.
backend-keystore-format = The keystore format { $format } is not supported – please convert it to PKCS#12.
    .text = The format of this keystore is not supported – please convert it to PKCS#12.
backend-certificate-mismatch = The signature was created with a certificate other than the expected one.
backend-fido2 = FIDO2 error: { $detail }
    .text = An error occurred with the security key. The exact cause is shown under “Technical details”.
backend-timeout = Timeout: the signature service did not respond in time.
    .title = Time has run out
    .action = Request a new confirmation
    .text = The signing service did not respond in time.
backend-internal = Internal error: { $detail }
    .text = An internal error occurred in PIDA. The exact cause is shown under “Technical details”.
backend-session-closed = The session at A-Trust has expired or was closed. Please try again – this starts a new session.
    .title = The session at A-Trust has expired
    .action = Try again

## Backend notices and forms without a PDF-Over key (`NoticeCode::AtrustInterstitial` as title,
## `FormProblem::LoginRejected`); mapping of all variants: crates/i18n/tests/code_catalog.rs

ng-notice-atrust-interstitial = Notice from A-Trust
ng-form-login-rejected = The login was not accepted. Please enter the password again.

## ID Austria via A-Trust, scenario A (`pida_backend_atrust`)

atrust-unknown-page = Unknown page of the signature service ({ $diagnostic }). The operation was cancelled.
    .text = A-Trust showed a page that PIDA does not know; the process was stopped. Please try again; the exact cause is under “Technical details”.
atrust-signature-data-link-rejected = The “show signature data” link does not lead to A-Trust and is not opened.

## FIDO2 (`pida_backend_atrust::Fido2Error::code` and detail codes of the providers)

fido2-unavailable = FIDO2 is not available on this device.
fido2-cancelled = The operation was cancelled at the security key.
fido2-invalid-options = The FIDO2 request from the signature service could not be read.
    .text = A-Trust's request for the security key cannot be read. Please use the app or SMS.
fido2-device = Security key error: { $detail }
    .text = The security key reported an error. Please try again or use the app or SMS.
fido2-pin-unsupported = This security key requires a PIN; PIN entry is not yet available on this system – please use the app or SMS.
fido2-no-credential = This security key does not hold an ID Austria registration.
fido2-timeout = Timeout: the security key was not touched.
fido2-locked = The security key is locked.
fido2-pin-auth-blocked = Too many wrong PIN entries in a row. Please unplug the security key, plug it back in and try again.
fido2-u2f-only = The security key does not support FIDO2 (U2F only).
    .text = This security key is too old for ID Austria (U2F only). Please use the app or SMS.
fido2-no-user-verification = The security key cannot verify the user (no PIN or biometrics set up).
    .text = Neither a PIN nor a fingerprint is set up on the security key. Please set one up or use the app or SMS.
fido2-hid-permission = No access to the security key – the udev rule (uaccess) for FIDO devices is missing.
    .text = No access to the security key. On Linux, PIDA needs a system rule for FIDO devices (udev) – please contact your IT or use the app or SMS.
fido2-no-device = No FIDO2 security key found. Please plug in your security key and try again.

## Signature block (`pida_sigblock::SigBlockError::code`, `NOTICE_CHARS_REPLACED`)

sigblock-unknown-profile = The signature profile “{ $profile }” does not exist.
    .text = The selected signature profile does not exist. Details are shown under “Technical details”.
sigblock-profile = The signature profile is invalid: { $detail }
    .text = The signature profile contains errors. Details are shown under “Technical details”.
sigblock-unsupported-expression = The expression { $expr } in the signature profile is not supported.
    .text = The signature profile contains an expression that is not supported. Details are shown under “Technical details”.
sigblock-unsupported-date-pattern = The date format “{ $pattern }” in the signature profile is not supported.
    .text = The date format in the signature profile is not supported. Details are shown under “Technical details”.
sigblock-font = The signature block font could not be loaded: { $detail }
    .text = The signature block font could not be loaded. Details are shown under “Technical details”.
sigblock-missing-glyph = The character “{ $char }” is not available in the signature block font.
    .text = A character is not available in the signature block font. Details are shown under “Technical details”.
sigblock-image = The signature logo could not be read (supported: PNG, JPEG, GIF).
sigblock-missing-resource = A resource of the signature profile is missing: { $detail }
    .text = A resource of the signature profile is missing. Details are shown under “Technical details”.
sigblock-pdf = The signature appearance could not be created.
sigblock-io = Error reading the signature block configuration.
sigblock-chars-replaced = Some characters are not available in the signature block font and were replaced by “?”: { $chars }

## Rendering (`pida_render::RenderError::code`, code `render.…` → ID `render-…`)

render-library_not_found = The PDF rendering library (PDFium) could not be found: { $details }
    .text = The PDF rendering library (PDFium) could not be found. Details are shown under “Technical details”.
render-load_failed = The document could not be loaded: { $details }
    .text = The document could not be loaded. Details are shown under “Technical details”.
render-password_required = The document is password protected.
render-page_out_of_range = Page { $page } does not exist (the document has { $count } pages).
    .text = This page does not exist in the document.
render-failed = The page could not be rendered: { $details }
    .text = The page could not be rendered. Details are shown under “Technical details”.

## Placement (`pida_placement::PlacementError::code`, `PlacementNotice::code`; `placement.…` → `placement-…`)

placement-invalid_position = Invalid position specification: { $position }
    .text = The position specification cannot be used. Details are shown under “Technical details”.
placement-no_such_page = Page { $page } does not exist.
    .text = This page does not exist in the document.
placement-placeholder_not_found = No matching signature placeholder was found.
placement-not_enough_space = There is not enough space for the signature block, and no new page may be added to an already signed document.
placement-qr = The QR code for the placeholder could not be created.
placement-notice-new_page_not_allowed = The document is already signed – the signature block was placed on the last page instead of a new page.
placement-notice-overlaps_content = The signature block covers page content.
placement-notice-overlaps_widget = The signature block covers an existing form or signature field.
placement-notice-clamped = The position was adjusted to fit the page.
placement-notice-scan_failed = The free space on the page could not be determined; the signature block was placed at the top of the page.

## Input analysis (`pida_pades::Finding::code`, plan §6.1)

pdf-xref-repaired = The document's cross-reference table is damaged and was repaired while reading. Signing is possible but does not fix the damage.
    .text = The document is slightly damaged; PIDA compensated for this when reading it. Signing is possible, but the damage remains.
pdf-xref-reconstructed = The document's cross-reference table is missing or unreadable. The document cannot be signed without repair.
    .text = The document is damaged and cannot be signed as it is. Save it again in a PDF program (e.g. “Print to PDF”) and try again.
pdf-tail-garbage = There is extraneous data after the end-of-file marker (%%EOF).
    .text = There is additional data at the end of the file that does not belong to the PDF.
pdf-prior-signature-beyond-eof = An existing signature extends beyond the end of the file – the document was truncated or damaged after signing and cannot be signed again.
    .text = An existing signature no longer matches the file – the document was changed or damaged after signing and cannot be signed again.
pdf-docmdp-p1 = The document is certified and does not allow any changes (DocMDP P=1); it cannot be signed.
    .text = The document is certified and must not be changed any more – not even by another signature.
pdf-user-password = The document is protected by an open password and cannot be signed.
pdf-xfa-dynamic = The document is a dynamic XFA form and cannot be signed.
    .text = The document is a special form (dynamic XFA) that cannot be signed. Please save it as a regular PDF and try again.
pdf-usage-rights = The document contains extended Adobe Reader rights. Signing invalidates them; Adobe Reader will show a warning afterwards.

## Oberfläche: General

ng-ui-yes = Yes
    .accesskey = Y
ng-ui-no = No
    .accesskey = N
ng-ui-close = Close
ng-ui-continue = Continue
    .accesskey = C
ng-ui-skip-to-content = Skip to content
ng-ui-link-external = (opens in browser)
ng-ui-password-show = Show password
ng-ui-password-hide = Hide password
ng-ui-page-n = Page { $page }
ng-ui-page-previous = Previous page
ng-ui-page-next = Next page
ng-ui-starting = PIDA is starting
ng-ui-finished = PIDA has exited.
ng-ui-finished-code = Exited with code { $code }.
ng-ui-mainbar-label = Steps
ng-ui-step-done = done
ng-ui-lang-system = System language
ng-ui-lang-de = German
ng-ui-lang-en = English
ng-ui-drop-not-here = Documents can be dropped in the “Open” step or on the result.

## Oberfläche: Errors of the user interface or the shell

ng-ui-core-unavailable = The signature core could not be started. Please try again; if the error persists, check the logs (Settings › About).
ng-ui-event-invalid = This action is not yet supported by this version of the signature core.
ng-ui-bridge-error = The connection to the signature core is disrupted.
ng-ui-error-unknown = Unexpected error ({ $code }).
ng-ui-error-unexpected = An unexpected error occurred. Please try again.
ng-ui-open-failed = Opening failed.
ng-ui-dialog-failed = The file dialog could not be opened.

## Oberfläche: Open

ng-ui-open-title = Open document
ng-ui-open-analyzing = Checking document …
ng-ui-dropzone-label = Drop area for PDF documents
ng-ui-dropzone-hint = Several documents are signed one after the other. Keyboard: Ctrl+O or ⌘O opens the file dialog.
ng-ui-dropzone-release = Release to open the documents
ng-ui-queue-title = Queue ({ $count })
ng-ui-queue-current = current

## Oberfläche: Placeholder

ng-ui-placeholder-title = Signature placeholder found
ng-ui-placeholder-qr = QR code placeholder on page { $page }
ng-ui-placeholder-field = Signature field “{ $name }”
ng-ui-placeholder-kind = Kind of placeholders

## Oberfläche: Positioning

ng-ui-position-title = Position the signature
ng-ui-position-help = Drag the signature block to the desired place or use the arrow keys (Shift: 1 pt, Ctrl/⌘: 50 pt), Page Up/Down for the page and Enter to sign.
ng-ui-position-block-role = movable signature block
ng-ui-position-announce = Page { $page } of { $pages }, left edge { $left } mm from the left, top edge { $top } mm from the bottom
ng-ui-position-pages = Pages
ng-ui-position-page-alt = Page { $page } of { $pages } – { $name }
ng-ui-position-new-page-alt = New blank page
ng-ui-position-preview-failed = The page preview is not available.
ng-ui-position-auto = Automatic
    .accesskey = A
ng-ui-position-auto-done = The signature block was placed automatically.
ng-ui-position-invisible = Sign invisibly
    .accesskey = I
ng-ui-position-signed-hint = The document is already signed; a new page cannot be added.

## Oberfläche: Signing method

ng-ui-backend-title = Choose signing method
ng-ui-backend-hint = A default signing method can be set in the settings (Advanced).
ng-ui-backend-atrust-desc = Sign with ID Austria: confirm in the app, by SMS TAN or with a FIDO2 security key.
ng-ui-backend-local-desc = Sign with the smart card via locally installed smart card software.
ng-ui-backend-keystore-desc = Sign with a PKCS#12 file (keystore).
ng-ui-backend-unavailable = Currently not available.
ng-ui-backend-local-unavailable = No local smart card software found.
ng-ui-backend-keystore-unavailable = Keystore not configured (Settings › Keystore).
ng-ui-backend-leaves-device = The document is sent to A-Trust for signing.
ng-ui-backend-leaves-device-many = The documents are sent to A-Trust for signing.
ng-ui-backend-qes = Qualified signature
ng-ui-factor-app = App
ng-ui-factor-sms = SMS
ng-ui-factor-card = Card
ng-ui-privacy-title = Privacy
    .accesskey = y

## Oberfläche: Signing

ng-ui-progress-connecting = Connecting …
ng-ui-progress-certificate = Reading certificate …
ng-ui-progress-server = Waiting for the signature service …
ng-ui-progress-document = Signing document { $index } of { $total } …
ng-ui-progress-done = Signature created.
ng-ui-sms-title = Enter SMS TAN
ng-ui-qr-title = Scan QR code
ng-ui-qr-alt = QR code to scan with the “ID Austria” app
ng-ui-app-title = Confirm in the app
ng-ui-fido2-title = FIDO2 security key
ng-ui-fido2-hint = Connect your security key and follow the instructions of the system (enter PIN, touch the key).
ng-ui-fido2-failed = FIDO2 failed: { $detail }
ng-ui-countdown = Time remaining: { $time }
ng-ui-countdown-expired = The time has expired.
ng-ui-login-rejected = The login was rejected. Please enter the password again.
ng-ui-username-required = Please enter the username or phone number.
ng-ui-secret-title = Password
ng-ui-secret-prompt = Please enter the password:
ng-ui-secret-retry = The password is wrong.
ng-ui-secret-subject = For: { $subject }
ng-ui-proxy-password-prompt = Please enter the proxy password:
ng-ui-proxy-password-retry = The proxy password is wrong.
ng-ui-notice-keystore-conversion = JKS and JCEKS keystores are not supported. Please convert the keystore to PKCS#12 (e.g. with keytool -importkeystore).
ng-ui-notice-http = A-Trust rejected the request (code { $status }). Please try again later.
ng-ui-notice-atrust = Message from A-Trust: { $text }
ng-ui-notice-proxy-auth = Your network's proxy server requires a sign-in. Please enter the user name and password under Settings › Advanced › Proxy.
ng-ui-notice-too-large = The document is too large for ID Austria. Please reduce the size of the PDF and try again.
ng-ui-notice-connection = No connection to the signing service. Please check the internet connection and try again.
ng-ui-notice-localbku = The smart card software is not running. Please start it and try again.
ng-ui-notice-keystore = The keystore could not be opened. Please check the password.
ng-ui-notice-keystore-alias = The key “{ $alias }” is not in the keystore. Please choose another one under Settings › Signing methods.

## Oberfläche: Result

ng-ui-output-reveal = Show in folder
    .accesskey = F
ng-ui-output-saving = Saving the signed document …
ng-ui-output-next = Next document: { $name }
ng-ui-output-next-button = Sign next document
    .accesskey = N
ng-ui-output-more = Open more documents
    .accesskey = M
ng-ui-output-drop-hint = You can drop more documents here.
ng-ui-output-saved-next = { $file } was saved, continuing with the next document.

## Oberfläche: Settings

ng-ui-settings-interface = Interface
ng-ui-settings-reset = Reset settings …
    .accesskey = t
ng-ui-settings-reset-confirm = All settings are reset to their defaults; the previous ones are kept as a backup.
ng-ui-settings-reset-confirm-button = Reset
    .accesskey = R
ng-ui-settings-preview-alt = Preview of the signature block
ng-ui-settings-preview-note = The preview shows the saved settings.
ng-ui-emblem-none = No emblem selected
ng-ui-emblem-drop-hint = You can drag an image file (PNG, JPEG, GIF) onto the preview.
ng-ui-theme = Appearance
ng-ui-theme-system = Same as system
ng-ui-theme-light = Light
ng-ui-theme-dark = Dark
ng-ui-cms-profile = Signature format (keystore)
ng-ui-cms-profile-hint = Applies to signatures created locally.
ng-ui-cms-pdfas = Compatible with PDF-AS
ng-ui-cms-pades = PAdES-BASELINE-B
ng-ui-postfix-hint = Empty means “_signed”.
ng-ui-proxy-mode = Use proxy
ng-ui-proxy-system = System proxy
ng-ui-proxy-none = No proxy
ng-ui-proxy-manual = Manual
ng-ui-proxy-password-hint = The password is requested when needed and stored in the keychain.
ng-ui-certcache = Cache the signing certificate
ng-ui-certcache-hint = Stores only the public certificate so that the preview shows the real signature block.
ng-ui-keystore-keychain = Store in the operating system keychain
ng-ui-keystore-alias-hint = Leave empty if the keystore contains only one key.
ng-ui-problem-required = Please fill in this field.
ng-ui-problem-port = The port must be between 1 and 65535.
ng-ui-problem-invalid = The value is invalid.
ng-ui-problem-tan-missing = Please enter the TAN from the text message.
ng-ui-problem-tan-too-long = The TAN is too long – please check your entry.
ng-ui-problem-reference-value = That is the reference value. Please enter the TAN from the text message.
ng-ui-problem-password-too-short = The password is too short.
ng-ui-problem-password-too-long = The password is too long.
ng-ui-problem-qr-failed = The QR code could not be loaded. Please confirm in the app or by text message.
ng-ui-problem-atrust = Note from A-Trust: { $text }
ng-ui-about-version = Version { $version }
ng-ui-about-thirdparty = The license notices of the components used are included in the program package.

## Eingangsanalyse (`pida_pades::Finding::code`, Plan §6.1) – Welle 2

pdf-docmdp-p2 = The document is certified (form filling and signing allowed): only existing empty signature fields or invisible signatures.
    .text = The document is certified. It can only be signed in an existing empty signature field or invisibly.
pdf-docmdp-p3 = The document is certified (forms, signatures and comments allowed): only existing empty signature fields or invisible signatures.
    .text = The document is certified. It can only be signed in an existing empty signature field or invisibly.
pdf-permissions = The document is protected; its permissions do not allow signing.
pdf-encrypted = The document is encrypted; the signature is added using the same encryption.
pdf-xfa-hybrid = The document contains an XFA form; XFA viewers may not show the signature block.
    .text = The document contains a special form (XFA). Some PDF programs may not show the signature block.
pdf-field-locked = Some signature fields are locked by existing signatures and cannot be selected.
pdf-seed-value = The signature field contains requirements for the signature; they are met.
    .text = The signature field specifies requirements for the signature; PIDA meets them.
pdf-seed-value-incompatible = The signature field contains requirements this signature cannot meet.
    .text = The signature field specifies requirements that this signature cannot meet.
pdf-pdfa-need-appearances = The PDF/A document requests regenerated form appearances and is already non-conforming.
    .text = The document is marked as PDF/A but already does not fully meet this standard.
pdf-prior-signature-invalid = An existing signature is invalid.
pdf-prior-signature-unverifiable = An existing signature cannot be verified.
pdf-public-key = The document is encrypted with certificates and cannot be signed.
    .text = The document is encrypted for specific recipients and cannot be signed.
pdf-unsupported-security = The document's encryption is not supported.
    .text = The document is protected in a way that PIDA does not support.
pdf-no-pages = The document has no pages.
pdf-not-a-pdf = The file is not a PDF document.
pdf-unreadable = The document is damaged and cannot be read.
pdf-limit-exceeded = The document exceeds a processing limit.
    .text = The document is too large or too complex to process.
pdf-decryption = The document could not be decrypted.
pdf-io = The document could not be read (read error).
    .text = The document could not be read.
pdf-signature-subfilter = The signature method of an existing signature is not verified.
    .text = An existing signature uses a method that PIDA does not check.
pdf-signature-byterange = The signed range of an existing signature is invalid.
    .text = An existing signature does not cover the document correctly and is probably damaged.
pdf-signature-dictionary = An existing signature has no readable signature dictionary.
    .text = An existing signature is damaged and cannot be read.

## PAdES-Engine (`pida_pades::PadesError::code`, Invarianten I1–I6, Hinweise) – Welle 2

pades-pdf-unreadable = The document could not be read.
pades-write = The signature could not be written into the document.
pades-field-already-signed = The selected signature field is already signed.
pades-field-locked = The selected signature field is locked.
pades-certificate-not-valid = The certificate is not valid at the time of signing.
pades-io = Read or write error.
    .text = The document could not be read or written.
pades-field-not-found = The selected signature field was not found.
pades-new-page-not-allowed = No new page may be added to this document.
pades-invalid-plan = The signature cannot be placed this way.
pades-invalid-state = The saved signing process is invalid and will be discarded.
    .text = An interrupted signing process could not be resumed and was discarded.
pades-invariant-i1 = The signature self-check failed (original not preserved); nothing was saved.
    .text = The signature did not pass the final self-check. Nothing was saved – please try again.
pades-invariant-i2 = The signature self-check failed (signed range); nothing was saved.
    .text = The signature did not pass the final self-check. Nothing was saved – please try again.
pades-invariant-i3 = The signature self-check failed (signature data or certificate); nothing was saved.
    .text = The signature did not pass the final self-check. Nothing was saved – please try again.
pades-invariant-i4 = The signature self-check failed (existing signatures); nothing was saved.
    .text = The signature did not pass the final self-check. Nothing was saved – please try again.
pades-invariant-i5 = The signature self-check failed (result not readable); nothing was saved.
    .text = The signature did not pass the final self-check. Nothing was saved – please try again.
pades-invariant-i6 = The signature self-check failed (disallowed change); nothing was saved.
    .text = The signature did not pass the final self-check. Nothing was saved – please try again.
pades-pdfua-skipped = The accessibility structure (PDF/UA) was not extended.
    .text = The accessibility information (PDF/UA) was not added.
pades-output-intent-skipped = The PDF/A colour profile (output intent) was not added because the document is already signed.
    .text = A PDF/A setting was not added because the document is already signed.
pades-i5-without-pdfium = The check with PDFium was skipped.
    .text = An additional check of the result was skipped.
pades-new-visible-field-not-allowed = The document is certified: a new visible signature field would violate the certification. Please choose an existing empty signature field or an invisible signature.
pades-not-self-checked = The signature self-check has not been passed; nothing is saved.
    .text = The signature did not pass the final self-check. Nothing was saved – please try again.

## Signatur-Backends (`BackendError::code`, `FormProblem::code`) – Welle 2

backend-login-rejected = The login was rejected. Please check your credentials.

## FIDO2 (`Fido2Error::code`) – Welle 2

fido2-pin-required = The security key requires a PIN.
fido2-pin-invalid = The security key PIN is incorrect.
fido2-internal = Internal error during sign-in with the security key.

## Kommandozeile (`apps/cli`) – Welle 2

ng-cli-help-title = PIDA – PDF signing with ID Austria
ng-cli-help-usage = Usage: pida-cli [options] [files …] | pida-cli <command> [options]
ng-cli-help-options = Options (compatible with PDF-Over):
ng-cli-help-new-options = New options:
ng-cli-help-commands = Commands:
ng-cli-help-headless = Sign without prompts (keystore with -kspass/--kspass-stdin/--kspass-env, local BKU); automatic without a terminal
ng-cli-help-json = Print the result as JSON on stdout
ng-cli-help-kspass-stdin = Read the keystore password from stdin (first line)
ng-cli-help-kspass-env = Read the keystore password from the environment variable
ng-cli-help-log-level = Log level (error, warn, info, debug, trace)
ng-cli-help-version = Show the version
ng-cli-help-cmd-sign = Sign documents (files, folders, patterns such as *.pdf)
ng-cli-help-cmd-verify = Check the signatures of a PDF: unchanged and cryptographically intact (developer tool; it does not check the chain of trust or revocation status)
ng-cli-help-cmd-profiles = List signature profiles
ng-cli-help-cmd-migrate = Import settings from ~/.pdf-over (--from <folder>, --dry-run)
ng-cli-help-cmd-render-block = Write the signature block as PDF/PNG (--profile <ID> <file>)
ng-cli-help-cmd-licenses = Print the EUPL-1.2 license text and the third-party notices (-o <folder>: write them as files)
ng-cli-help-exit-codes = Exit codes: 0 success, 1 general error, 2 usage, 3 document rejected, 4 cancelled, 5 signing device/network, 6 self-check (nothing saved), 7 write error
ng-cli-no-input = No document given (help: pida-cli -h).
ng-cli-keypass-mismatch = -kskeypass differs from -kspass: PKCS#12 has a single password. Please export the keystore with one password.
ng-cli-kspass-env-missing = The environment variable { $name } is not set.
ng-cli-stdin-failed = Could not read the password from stdin: { $detail }
ng-cli-signed = signed: { $input } → { $output }
ng-cli-cancelled = cancelled: { $input }
ng-cli-failed = failed: { $input }: { $message }
ng-cli-overwrite = File { $name } already exists. Overwrite? [y/N]{ " " }
ng-cli-retry = Retry? [y/N]{ " " }
ng-cli-continue = Continue? [y/N]{ " " }
ng-cli-username = Username/phone number:{ " " }
ng-cli-password = Password:{ " " }
ng-cli-tan = TAN (reference value { $reference }):{ " " }
ng-cli-qr = Reference value { $reference }: please scan the QR code with the ID Austria app ({ $path }).
ng-cli-app = Reference value { $reference }: please confirm in the ID Austria app.
ng-cli-countdown = { $seconds } s left …
ng-cli-fido2 = FIDO2: please touch your security key …
ng-cli-keystore-password = Keystore password for { $name }:{ " " }
ng-cli-key-password = Key password for { $name }:{ " " }
ng-cli-proxy-password = Proxy password for { $name }:{ " " }
ng-cli-wrong-password = Wrong password.
ng-cli-progress = … { $step }
ng-cli-verify-intact = Intact (unchanged): { $field } ({ $signer })
ng-cli-verify-not-intact = Not intact: { $field } ({ $error })
ng-cli-verify-scope = Chain of trust and revocation status not checked – full check: https://www.signaturpruefung.gv.at
ng-cli-no-signatures = No signatures found.
ng-cli-written = written: { $path }
ng-cli-licenses-missing = This build contains no third-party notices (development build). They are included in the release packages; tools/third-party-notices.sh generates them.
ng-cli-licenses-rust-std = For the complete notices of the Rust standard library, “pida-cli licenses -o <folder>” writes RUST-STD-COPYRIGHT.html.
ng-cli-no-legacy = No PDF-Over configuration found ({ $path }).
ng-cli-migrate-dry-run = Dry run – nothing was written.
ng-cli-unknown-profile = Profile { $id } does not exist (list: pida-cli profiles).

## Fortschritt (`pida_core_api::messages::progress_message`) – Welle 2

ng-progress-connecting = Connecting
ng-progress-certificate = Reading the certificate
ng-progress-waiting = Waiting for the signing service
ng-progress-document = Document { $index } of { $total }
ng-progress-done = Done

## Einstellungsprüfung (`pida-core-api`, Feldprobleme) – Welle 2

ng-settings-invalid-postfix = The file name suffix must not contain path separators (/ or \).
ng-settings-proxy-host-missing = A host is required for a manual proxy.
ng-settings-invalid-logo-size = The logo size must be greater than 0 and at most 120 mm.

## Prüfung vorhandener Signaturen (`pida verify`, Entwickler-Werkzeug; die App hat keinen Prüfmodus, ADR 0004 Nachtrag 15) – Welle 2

ng-verify-failed = The file could not be checked: { $detail }
    .text = The file could not be checked.
ng-verify-no-signature-dictionary = Signature field without signature data.
ng-verify-no-byte-range = Signature without /ByteRange or /Contents.
ng-verify-byte-range = The signed range (/ByteRange) of the signature is malformed.

## Workflow und Kern (`pida-workflow`, `pida-core-api`) – Welle 2

ng-invariant-violated = The signature self-check failed ({ $invariant }: { $detail }). Nothing was saved.
    .text = The signature self-check failed. Nothing was saved. Details are shown under “Technical details”.
ng-cms-too-large = The signature is larger than the reserved space ({ $needed } instead of { $reserve } bytes). “Retry” prepares the document again with more space; this needs another confirmation.
    .title = The signature needs more space
    .text = The signature is larger than the reserved space. “Try again” prepares the document again with more space; this needs another confirmation.
    .action = Try again
ng-backend-choice-required = Several signing devices are available ({ $detail }) – please choose one with -b.
    .text = Several signing devices are available – please choose one.
ng-no-backend-available = No signing device is available.
ng-default-backend-unavailable = The chosen signing device ({ $detail }) is not available.
    .text = The chosen signing device is not available.
ng-document-rejected = The document cannot be signed.
    .title = This document cannot be signed
    .action = Open another document
ng-document-too-large = The document is larger than 20 MB and cannot be accepted for processing (signature size limit).
    .title = Document too large
    .text = This document is larger than 20 MB and therefore cannot be accepted for processing. Documents up to 20 MB can be signed.
    .action = Open another document
ng-pdf-no-sign-rights = The document permissions do not allow signing.
ng-field-already-signed = The signature field is already signed.
ng-field-locked = The signature field is locked.
# Several signature blocks in one document, one signature per block
ng-blocks-limit = More than 10 signature blocks per document are not possible.
ng-block-target-taken = Another signature block is already placed here.
ng-blocks-new-page = “On a new page” is only possible with a single signature block.
ng-certificate-not-valid = The certificate is not valid at the time of signing.
ng-clock-skew = This device's clock differs from the signing service by about { $minutes } minutes.
ng-atrust-login-rejected = The login was rejected. Please enter the password again.
ng-unknown-document = The document is not open (any more).
ng-core-stopped = The application core has stopped.
ng-no-preview = No preview of the signature block is available.

## Welle 3 (Integration: Bridge, Shell, CLI-Argumente der GUI, Zertifikats-Cache)

ng-ui-warning-title = Note on the document
ng-ui-warning-lead = Checking the document found the following:
ng-ui-warning-question = Do you want to sign the document anyway?
ng-ui-warning-proceed = Sign anyway
ng-ui-warning-discard = Discard document
ng-ui-done-title = Done ({ $count })
ng-ui-done-signed = signed: { $file }
ng-ui-done-cancelled = cancelled
ng-ui-done-failed = failed: { $message }
ng-ui-settings-read-only = These settings are not saved (configuration given with -c or unreadable settings file). Changes apply until PIDA exits.
ng-cli-fido2-pin = Security key PIN:{ " " }
ng-certcache-mismatch = The certificate has changed (for example renewed, or a different account). The signature was discarded. "Retry" reads the certificate again, rebuilds the signature block with the new details and asks for confirmation again.
ng-placeholder-profile-unknown = The signature placeholder names the unknown profile "{ $profile }"; the configured profile is used for signing.
ng-ui-args-cli-only = The option { $option } is only available on the command line (pida-cli).
ng-ui-about-notices = Show licenses
# Licenses in the app (backlog no. 2): PIDA's EUPL, third-party licenses, Rust standard library
ng-ui-licenses-title = Licenses
ng-ui-licenses-subtitle = PIDA is licensed under the EUPL-1.2 and contains third-party components under their own licenses.
ng-ui-licenses-tab-eupl = PIDA (EUPL-1.2)
ng-ui-licenses-tab-notices = Third-party components
ng-ui-licenses-tab-rust-std = Rust standard library
ng-ui-licenses-loading = Loading …
ng-ui-licenses-missing = The third-party license notices are missing in this development build (tools/third-party-notices.sh).

## Mobile (mobile wave, package M0: lifecycle, "resume signature", sharing; plan §5.2). Since ADR 0004 addendum 11 the core
## returns the `ng-keystore-import*` codes on the desktop only (iOS/Android: ID Austria only).

ng-fido2-unavailable-mobile = FIDO2 security keys are not available on this device. Please confirm in the "ID Austria" app or with an SMS TAN.
ng-resume-expired = An interrupted signature was older than one hour and has been discarded.
ng-resume-restore-failed = The interrupted signature could not be restored; the document is prepared again.
ng-resume-backend-unavailable = The signing method of the interrupted signature is no longer available. Please sign again.
ng-keystore-imported = The keystore "{ $name }" has been imported.
ng-keystore-import-busy = A keystore cannot be imported while a signature is in progress.
ng-keystore-import-unreadable = The file could not be read or is too large.
ng-keystore-import-format = The file is not a PKCS#12 file (.p12, .pfx).
ng-keystore-import-failed = The keystore could not be copied into the app.
ng-qr-caption-1 = Placeholder for the
ng-qr-caption-2 = electronic signature
ng-qr-caption-number = No.
ng-qr-saved = QR placeholder saved: { $name }
ng-qr-failed = The QR placeholder could not be created.
ng-qr-invalid-id = The number may only contain digits (1 to 10 digits).
ng-share-failed = The document could not be shared.
ng-export-done = "{ $name }" has been saved.
ng-export-failed = The document could not be saved.
ng-mobile-unsupported = Not available on this device.
ng-mobile-not-implemented = This function is not yet available in this version.
ng-mobile-cancelled = Cancelled.
ng-mobile-timeout = The system did not respond in time.
ng-mobile-native = The operating system function failed.
ng-ui-output-share = Share
    .accesskey = h
ng-ui-output-export = Save as …
    .accesskey = v
ng-ui-output-mobile-hint = The signed document is stored in this app's documents.
ng-ui-resume-title = Resume signature?
ng-ui-resume-text = The signature of "{ $name }" was interrupted { $minutes ->
        [0] just now
        [one] a minute ago
       *[other] { $minutes } minutes ago
    } (the system closed the app). You can resume it with the same position.
ng-ui-resume-again = You will need to log in and confirm again.
ng-ui-resume-pending-one = One more document follows.
ng-ui-resume-pending = { $count } more documents follow.
ng-ui-resume-continue = Resume
    .accesskey = R
ng-ui-resume-discard = Discard
    .accesskey = D
ng-ui-backend-atrust-desc-mobile = Sign with ID Austria: confirm in the "ID Austria" app or with an SMS TAN.
ng-ui-fido2-unavailable-badge = FIDO2 not available
ng-ui-browse-mobile = Choose document …
    .accesskey = C
ng-ui-dropzone-hint-mobile = You can also send PDF documents from other apps to PIDA with "Share" or "Open in".
ng-ui-position-help-mobile = Tap the desired place or drag the signature block with your finger; it stays visible above your finger.

## Datenschutz ohne Zustimmungsdialog (ADR 0003): Kurzhinweis beim Signiermittel, Einstellungen › Datenschutz, CLI

ng-document-leaves-device = Note: the document is sent to A-Trust for signing (A-Trust privacy policy: https://www.a-trust.at/downloads/documents/datenschutzerklaerung.pdf).
ng-ui-privacy-statement = The software PIDA processes the data required for the signature process locally on the installed device. The signature process with the smart card is carried out locally. During the signature process using ID Austria, the PDF document to be signed is sent to the company A-Trust. The storage period and further data protection information for signatures using ID Austria can be found in the data protection declaration of A-Trust at <a>https://www.a-trust.at/downloads/documents/datenschutzerklaerung.pdf</a>.
ng-ui-privacy-statement-mobile = The software PIDA processes the data required for the signature process locally on the installed device. During the signature process using ID Austria, the PDF document to be signed is sent to the company A-Trust. The storage period and further data protection information for signatures using ID Austria can be found in the data protection declaration of A-Trust at <a>https://www.a-trust.at/downloads/documents/datenschutzerklaerung.pdf</a>.
ng-ui-privacy-details = Details
ng-ui-privacy-details-label = Privacy details
ng-ui-privacy-local = With a keystore or the smart card (local smart card software) the document is signed on this device and is not uploaded.
ng-ui-privacy-local-mobile = On this device, documents are signed with ID Austria; for this, the document is sent to A-Trust.
ng-ui-privacy-statement-web = PIDA in the browser processes the data needed for signing in your browser; documents and settings are not stored and are gone once you close the page. When signing with ID Austria, the PDF document to be signed is sent to A-Trust via the server of this page. For the retention period and further data protection information on ID Austria signatures, please refer to the A-Trust privacy policy: <a>https://www.a-trust.at/downloads/documents/datenschutzerklaerung.pdf</a>.
ng-ui-privacy-local-web = When signing, the server of this page only passes the document on to A-Trust: it keeps it briefly in memory, does not store it and discards it at once. You enter your ID Austria credentials directly on the A-Trust page – the server never sees them.
ng-ui-privacy-atrust-link = A-Trust privacy policy: <a>https://www.a-trust.at/downloads/documents/datenschutzerklaerung.pdf</a>
ng-ui-privacy-pida-link = PIDA privacy policy (A-SIT): <a>https://pdf.a-sit.at/datenschutz/</a>
ng-ui-privacy-about = Where your documents go when signing is described in the <a>Privacy</a> section.
ng-ui-certcache-title = Certificate cache

## New “workspace” interface (ADR 0004 §3.16): messages of the core. Wording per ADR 0004 §8 F3 – never
## “valid” or “legally valid”. UI texts of the packages live in `ui-workspace.ftl` and `ui-tools.ftl`.

ng-signer-unavailable = The selected signing method is currently not available. Please choose another one.
ng-signer-localbku-not-running = The smart card software (local BKU) is not running. Start it and try again.
ng-signer-keystore-missing = No keystore is set up, or the file is missing (Settings › Signing methods).
ng-signer-policy = This signing method is not permitted by your organisation.
ng-signing-cancelled = Signing cancelled – nothing was signed.
ng-signing-declined = Declined or expired in the app – nothing was signed.
    .title = Declined or expired in the app
    .action = Try again
ng-constraint-signed = The document is already signed; a new page would alter the existing signatures.
ng-constraint-certified = The document is certified; it may only be signed in existing signature fields or invisibly.
ng-constraint-invisible-profile = The selected profile signs invisibly (Settings › Signature block).
ng-batch-added =
    { $count ->
        [one] One more document was added.
       *[other] { $count } more documents were added.
    }
ng-batch-item-signed = “{ $file }” was signed.
ng-parked-files-added =
    { $count ->
        [one] The document was added to the open work.
       *[other] { $count } documents were added to the open work.
    }
ng-open-duplicates =
    { $count ->
        [one] “{ $file }” is already open and was not added again.
       *[other] { $count } of the chosen documents are already open and were not added again.
    }
ng-open-missing =
    { $count ->
        [one] One chosen item was not added – it no longer exists or is not a file.
       *[other] { $count } chosen items were not added – they no longer exist or are not files.
    }
ng-batch-done = { $signed } of { $total } documents signed.
ng-batch-nothing-to-sign = No document in the batch is ready to be signed.
ng-batch-output-folder-required = Please choose a folder for the signed documents before starting.
ng-batch-aborted =
    { $signed ->
        [0] Batch cancelled – no document had been signed yet.
        [one] Batch cancelled – one document had already been signed and saved.
       *[other] Batch cancelled – { $signed } documents had already been signed and saved.
    }
ng-batch-paused = The batch is paused. Please decide how to continue.
ng-batch-large = { $count } documents: each document is confirmed separately in the app.
ng-batch-pending-app-request = Please decline any request still open in the app; always compare the reference value.
ng-mobile-keystore-copies-removed = Keystore copies that are no longer needed were removed from PIDA. On this device, documents are signed with ID Austria only.
ng-recent-missing = The file is no longer at the saved location.
ng-recent-policy = “Recently signed” has been switched off by your organisation.
ng-migration-item-block = Signature block (profile, language, note)
ng-migration-item-emblem = Emblem
ng-migration-item-username = ID Austria user name
ng-migration-item-output = Save location and file name
ng-migration-item-keystore = Keystore
ng-migration-item-proxy = Proxy
ng-migration-item-placeholder = Placeholder search
ng-migration-item-position = Automatic positioning
ng-migration-item-language = Language
ng-migration-not-found = No PDF-Over settings were found on this device.
ng-firstrun-done = Setup complete. You can change everything in the settings at any time.
ng-firstrun-later = You can set up the signing method and signature block in the settings at any time.
ng-settings-field-unknown = Unknown setting.
ng-settings-invalid-value = This value is not valid for the setting.
ng-settings-invalid-port = The port must be between 1 and 65535.
ng-settings-policy = This setting is set by your organisation.
ng-settings-unavailable-mobile = This setting is not available on mobile devices.
ng-window-title = PIDA – PDF signature with ID Austria
ng-ui-brand-tagline = PDF signature with ID Austria
ng-keystore-file-not-found = The selected keystore file could not be found. Please choose it again.
ng-keystore-password-needed = Please enter the keystore password.
ng-keystore-file-not-found-at = The keystore file “{ $name }” could not be found. Please choose it again under Settings › Signing methods.

## Mobile with ID Austria only (decision of the client, 16.10.2026, ADR 0004 addendum 11)

ng-migration-item-signer-mobile = Signing method: ID Austria – other signing methods from PDF-Over are not available on this device
ng-migrate-keystore-mobile = { $key } is not imported (on mobile devices, signing uses ID Austria only).
ng-resume-signer-not-offered = An interrupted signature was discarded because its signing method is no longer available in this app. Please open the document again and sign with ID Austria.

## About PIDA and update check (ADR 0004 addendum 21). The core delivers `ng-update-failed-*` (`update.rs`): the
## reason after "Check not possible: " (lower case, no full stop).

ng-update-failed-offline = no connection to GitHub
ng-update-failed-timeout = GitHub did not respond in time
ng-update-failed-proxy = proxy error (Settings › Advanced › Proxy)
ng-update-failed-tls = no secure connection to GitHub possible
ng-update-failed-rate-limited = too many requests to GitHub – please check again later
ng-update-failed-forbidden = access to GitHub denied (network or proxy)
ng-update-failed-server = GitHub reported an error
ng-update-failed-invalid = unexpected response from GitHub
ng-ui-about-logo = PIDA symbol
ng-ui-about-version-date = Version { $version } · { $date }
ng-ui-about-update-region = Updates
ng-ui-about-update-unchecked = Not checked for updates yet
ng-ui-about-update-disabled = Automatic check is turned off
ng-ui-about-update-policy = Disabled by your organisation
ng-ui-about-update-checking = Checking for updates …
ng-ui-about-update-current = PIDA is up to date
ng-ui-about-update-available = Version { $version } is available
ng-ui-about-update-failed = Check not possible: { $reason }
ng-ui-about-update-check-now = Check now
ng-ui-about-update-retry = Check again
ng-ui-about-update-download = Download
ng-ui-about-update-download-label = Download version { $version } (release page on GitHub)
ng-ui-about-links = Links
ng-ui-about-website = Website (pdf.a-sit.at)
ng-ui-about-imprint = Imprint
## Web version: footer of the start page (imprint easy to find, § 5 ECG)
ng-ui-web-legal = Legal
ng-ui-web-imprint = Imprint
ng-ui-web-privacy = Privacy
ng-ui-about-release-notes = Release notes
ng-ui-about-source = Source code
ng-ui-about-privacy = Privacy
ng-ui-about-license =
    This software is freely provided by A-SIT under the conditions of the EUPL-1.2; the license text is included with the program. Terms and details at <a>https://technology.a-sit.at/en/terms-of-a-license/</a>. For technical support, contact <a href="mailto:software@egiz.gv.at">software@egiz.gv.at</a>.

    Included third-party components are provided under their own licenses, which are compatible with the EUPL – see “Show third-party licenses”.
ng-ui-about-not-official = PIDA is an open-source project by A-SIT and not an official product of the Republic of Austria or A-Trust GmbH. “ID Austria” is a trademark of the Republic of Austria; the name only identifies the supported signing method.
ng-ui-about-based-on = Based on PDF-Over by A-SIT.
ng-ui-about-copyright = Copyright © { $year } A-SIT · Licence EUPL-1.2
ng-ui-about-publisher = A-SIT – Secure Information Technology Center – Austria
ng-ui-privacy-update-check = The update check asks GitHub (api.github.com) for the latest version. GitHub sees this device's IP address; no identifiers are sent. You can turn it off under Advanced › “Check for updates automatically”.


## Web-Variante: A-Trust-Overlay (apps/app/src/core/ida-overlay.ts)
ng-web-ida-title = Sign with ID Austria
ng-web-ida-origin = Page from { $host } – your input goes directly to A-Trust
ng-web-atrust-popup = Open in separate window
ng-web-atrust-close = Cancel
ng-web-atrust-popup-hint = Sign-in with A-Trust continues in the separate window.
ng-web-atrust-popup-blocked = The browser blocked the window. Allow pop-ups for this page.
