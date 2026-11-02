### PIDA – texts of the new user interface, UI-A (shell, start, workspace, signing card, done, batch), English (EUPL-1.2)
###
### Maintained by hand (ADR 0004 §4.5). IDs: `ng-ui-<area>-<name>`, areas of this file:
### shell, start, ws, insp, sc, fin, batch. All catalogs of a language form one Fluent bundle: do not reuse an ID from
### `pida.ftl`, `pdfover.ftl` or the other `ui-*.ftl`; every ID with the same variables in
### `de/` and `en/` (test `crates/i18n/tests/catalogs.rs`). Singular/plural via selectors.
###
### Texts from the prototype (`prototypes/pida-workspace/src/i18n/en.ts`), adapted to the core: wording per
### ADR 0004 §8 F3 – never “valid” or “legally valid”, but “Signature created and checked (self-check)” and
### “according to the certificate”; keyboard steps as in the core (15/1/50 pt).

## Shell: header, commands, notifications

ng-ui-shell-skip = Skip to content
ng-ui-shell-dl-region = App downloads
ng-ui-shell-dl-banner = PIDA is also available as an app for Windows, macOS, Linux, iOS and Android.
ng-ui-shell-dl-cta = Go to downloads
ng-ui-shell-dl-hide = Hide notice
ng-ui-shell-dl-title = PIDA as an app
ng-ui-shell-dl-subtitle = With additional features, such as saved settings and – on a computer – keystore and smart card.
ng-ui-shell-dl-desktop = Computer
ng-ui-shell-dl-mobile = Phone and tablet
ng-ui-shell-dl-github = Download on GitHub
ng-ui-shell-dl-loading = Fetching the latest version from GitHub …
ng-ui-shell-dl-version = Version { $version }
ng-ui-shell-dl-version-test = Version { $version } (test version)
ng-ui-shell-dl-unavailable = Downloads are currently unavailable – all versions are on GitHub.
ng-ui-shell-dl-download = Download
ng-ui-shell-dl-from-github = Downloads come directly from GitHub; for this, this page asks GitHub for the latest version.
ng-ui-shell-dl-qr = QR code for the { $platform } app (coming)
ng-ui-shell-dl-appstore = App Store – coming
ng-ui-shell-dl-play = Google Play – coming
ng-ui-shell-dl-source = Source code and all versions:
ng-ui-shell-help = Help
ng-ui-shell-help-title = Help
ng-ui-shell-help-subtitle = Works without internet.
ng-ui-shell-help-search = Search help
ng-ui-shell-help-todo = What you can do:
ng-ui-shell-help-exact = The exact message is shown under “Technical details”, e.g.: { $message }
ng-ui-shell-help-code = Code
ng-ui-shell-help-results = Search results
ng-ui-shell-help-empty = Nothing found for “{ $q }”. Try another word.
ng-ui-shell-help-back = Back
ng-ui-shell-help-context = For this view
ng-ui-shell-help-topics = Topics
ng-ui-shell-help-related = Related articles
ng-ui-shell-help-legal = General information, not legal advice.
ng-ui-shell-help-fehler-hint = Every PIDA message has a code (under “Technical details”). The quickest way to the right article is the search or “What does this mean?” on the message.
ng-ui-shell-help-topic-pida = Using PIDA
ng-ui-shell-help-topic-id-austria = ID Austria
ng-ui-shell-help-topic-pdf = PDF signatures and verification
ng-ui-shell-help-topic-recht = Law and eIDAS
ng-ui-shell-help-topic-fehler = Messages and solutions
ng-ui-shell-help-what = What does this mean?
ng-ui-shell-home = PIDA – home
ng-ui-shell-starting = Starting PIDA …
ng-ui-shell-title-workspace = Workspace
ng-ui-shell-doc-info = { $pages } · { $size }
ng-ui-shell-pages =
    { $n ->
        [one] { $n } page
       *[other] { $n } pages
    }
ng-ui-shell-commands = Commands
ng-ui-shell-commands-long = Commands and search
ng-ui-shell-lang = Interface language
ng-ui-shell-theme = Colour scheme
ng-ui-shell-theme-system = System
ng-ui-shell-theme-light = Light
ng-ui-shell-theme-dark = Dark
ng-ui-shell-settings = Settings
ng-ui-shell-toasts = Notifications
ng-ui-shell-toast-close = Dismiss notification
ng-ui-shell-cmd-open = Open document …
    .keywords = open file pdf load
ng-ui-shell-cmd-sign = Sign
    .keywords = signature
ng-ui-shell-cmd-settings = Settings
    .keywords = options preferences configuration
ng-ui-shell-cmd-signature = Edit signature block
    .keywords = logo profile emblem note
ng-ui-shell-cmd-home = Go to start
    .keywords = home start
ng-ui-shell-cmd-theme-light = Colour scheme: light
    .keywords = theme light
ng-ui-shell-cmd-theme-dark = Colour scheme: dark
    .keywords = theme dark
ng-ui-shell-cmd-theme-system = Colour scheme: system
    .keywords = theme system
ng-ui-shell-cmd-lang-de = Sprache: Deutsch
    .keywords = language german sprache
ng-ui-shell-cmd-lang-en = Language: English
    .keywords = language english
ng-ui-shell-cmd-shortcuts = Show keyboard shortcuts
    .keywords = help keyboard shortcuts
ng-ui-shell-cmd-add = Add PDFs …
    .keywords = more documents multiple file append batch
ng-ui-shell-cmd-batch = To the batch
    .keywords = batch overview all documents
ng-ui-shell-cmd-batch-back = Back to batch
    .keywords = batch overview all documents
# “Add PDF” in the header (ADR 0004 addendum 9): “+ PDF” button with a menu
ng-ui-shell-add = PDF
ng-ui-shell-add-long = Add or open a PDF
ng-ui-shell-add-more = Add more PDFs …
ng-ui-shell-add-more-desc = Sign together with “{ $name }” as a batch
ng-ui-shell-replace = Open a different PDF …
ng-ui-shell-replace-desc = “{ $name }” will be closed
ng-ui-shell-replace-title = Open a different PDF?
ng-ui-shell-replace-text = You placed the signature in “{ $name }” yourself. Opening a different PDF closes this document and discards the placement.
ng-ui-shell-replace-confirm = Open a different PDF
ng-ui-shell-key-close = Close dialog, cancel
ng-ui-shell-key-accesskeys = Access keys (underlined letters while Alt is held)
ng-ui-shell-key-move = Move signature block (15 pt)
ng-ui-shell-key-move-fine = Move finely (1 pt)
ng-ui-shell-key-move-coarse = Move in large steps (50 pt)
ng-ui-shell-key-page = Block to previous / next page
ng-ui-shell-key-place = Put the block here
ng-ui-shell-key-dblclick = Double-click
ng-ui-shell-key-zoom = Zoom in / out
ng-ui-shell-key-fit = Fit to width

# Multiple selection (ADR 0004 addendum 17): title or message line of the system file chooser and a hint on how
# to mark several PDFs there (macOS: a double-click takes only the clicked file; Android: a tap takes exactly one file
# right away, several only after a long press)
ng-ui-shell-pick-title =
    { $os ->
        [macos] Choose PDF documents – ⌘-click to mark several, then “Open”
       *[other] Choose PDF documents – Ctrl-click to mark several, then “Open”
    }
ng-ui-shell-pick-skipped =
    { $count ->
        [one] One chosen file could not be added (not readable or too large).
       *[other] { $count } chosen files could not be added (not readable or too large).
    }

## Start

ng-ui-start-title = What would you like to sign?
ng-ui-start-lead = Open a PDF – PIDA places the signature automatically, you confirm with your signing method.
ng-ui-start-drop-label = Drop area for PDF documents
ng-ui-start-drop-title = Drop a PDF here or choose one
ng-ui-start-drop-title-mobile = Choose a PDF
ng-ui-start-drop-release = Release to open
ng-ui-start-drop-hint = Several files are signed as a batch – one login, you confirm each document individually.
# per batch flow of the chosen signing method (core: `SignerView::batch_flow`, ADR 0004 addendum 12)
ng-ui-start-drop-hint-unlock = Several files are signed as a batch – the keystore is unlocked once.
ng-ui-start-drop-hint-each = Several files are signed as a batch – you confirm each document individually.
ng-ui-start-drop-hint-ask = Several files are signed as a batch.
ng-ui-start-drop-hint-mobile = Or send a PDF from another app to PIDA with “Share” or “Open in”.
ng-ui-start-pick = Choose PDF
    .accesskey = C
ng-ui-start-pick-mobile = Choose document
ng-ui-start-local = Your documents stay on this device until you sign.
ng-ui-start-shortcut = opens the file chooser
ng-ui-start-side = Recently signed
# Notice about a new version (core: `ViewState::update_hint`, backlog no. 33)
ng-ui-start-update = PIDA { $version } is available.
ng-ui-start-update-open = Go to release page
ng-ui-start-update-open-label = Open the release page of PIDA { $version } on GitHub
ng-ui-start-update-dismiss = Hide notice for this version
# Parked work (“Go to start” parks the open document or batch, ADR 0004 addendum 10)
ng-ui-start-parked-label = In progress
ng-ui-start-parked-batch =
    { $n ->
        [one] Batch · 1 document
       *[other] Batch · { $n } documents
    }
ng-ui-start-parked-hint = Not signed yet – the placement is kept.
ng-ui-start-parked-hint-placed = Not signed yet – your placement is kept.
ng-ui-start-parked-hint-batch = Plans and status of the documents are kept.
ng-ui-start-parked-hint-unsaved = Signed, but not saved yet.
ng-ui-start-parked-resume = Continue editing
ng-ui-start-parked-close = Close
ng-ui-start-parked-close-title = Close the open work?
ng-ui-start-parked-close-text = “{ $name }” will be closed and your placement discarded. Files already signed are kept.
ng-ui-start-parked-close-text-batch = The batch will be closed; placements and status of the documents are discarded. Files already signed are kept.
ng-ui-start-parked-close-text-unsaved = “{ $name }” is signed but not saved yet. If you close it, the signature is lost.
ng-ui-start-parked-close-confirm = Close
ng-ui-start-join-title = Add to the open work?
ng-ui-start-join-text =
    “{ $name }” is still in progress. { $count ->
        [one] Add the chosen document? Together they form a batch; your placement is kept.
       *[other] Add the { $count } chosen documents? Together they form a batch; your placement is kept.
    }
ng-ui-start-join-text-batch =
    A batch is still in progress. { $count ->
        [one] Add the chosen document?
       *[other] Add the { $count } chosen documents?
    }
ng-ui-start-join-add = Add to batch
ng-ui-start-join-replace = Close open work and start over
# Multiple selection (ADR 0004 addendum 17): how to mark several PDFs in the file chooser
ng-ui-start-multi-hint =
    { $os ->
        [macos] Several PDFs at once: ⌘-click them in the dialog, then “Open”.
        [ios] Several PDFs at once: tap the PDFs in the file chooser (check mark), then “Open”.
        [android] Several PDFs at once: long-press a PDF in the file chooser, tap more, then “Select”.
       *[other] Several PDFs at once: Ctrl-click them in the dialog, then “Open”.
    }

## Workspace: pages, sticker, live region, state cards

ng-ui-ws-canvas = Document preview: { $name }
ng-ui-ws-toolbar = View
ng-ui-ws-pages = Pages
ng-ui-ws-thumb = Page { $n }
ng-ui-ws-thumb-sticker = Page { $n }, contains the signature block
ng-ui-ws-thumbs-add = Add PDF
ng-ui-ws-thumbs-add-hint = or drop it here
ng-ui-ws-toggle-pages = Show/hide page sidebar
ng-ui-ws-page-indicator = Page { $n } / { $total }
ng-ui-ws-zoom-in = Zoom in
ng-ui-ws-zoom-out = Zoom out
ng-ui-ws-zoom-value = Zoom { $value } %
ng-ui-ws-fit = Fit to width
ng-ui-ws-page-alt = Page { $n } of { $total } – { $name }
ng-ui-ws-page-failed = Preview not available
ng-ui-ws-new-page = New page for the signature
ng-ui-ws-sticker-role = movable signature block
ng-ui-ws-sticker-label = Signature block
ng-ui-ws-sticker-help = Drag or move with the arrow keys, Shift+arrow moves finely, Alt+arrow in large steps, Page Up/Down changes the page, { $sign } signs.
# Setting “Automatic positioning” (ADR 0004 addendum 8): block locked
ng-ui-ws-sticker-role-locked = automatically placed signature block
ng-ui-ws-sticker-locked-help = Placed automatically according to the setting “Automatic positioning”. { $sign } signs; “Place it myself this time” under “Placement” releases the block for moving.
ng-ui-ws-sticker-overlap = Covers content
ng-ui-ws-sticker-widget = Covers a field
ng-ui-ws-sticker-in-field = In signature field
ng-ui-ws-sticker-in-qr = In QR placeholder
ng-ui-ws-placed = Signature block placed automatically on free space
ng-ui-ws-placed-qr = Signature block placed automatically in the QR placeholder
ng-ui-ws-placed-field = Signature block placed automatically in the signature field
ng-ui-ws-marker = Sign here
ng-ui-ws-marker-field = Sign here: signature field “{ $name }”
ng-ui-ws-marker-qr = Sign here: QR code placeholder
ng-ui-ws-field-found = Signature field found
ng-ui-ws-field-banner = “{ $name }” on page { $page }. Choose “Sign here” there or put the block straight into the field.
ng-ui-ws-field-use = Put in field
ng-ui-ws-qr-banner = Placed in the document’s QR code placeholder.
ng-ui-ws-invisible = Invisible signature – no signature block appears on the pages.
ng-ui-ws-announce = Page { $page } of { $pages }, top left corner { $left } mm from the left, { $top } mm from the bottom.
ng-ui-ws-announce-snap = Snaps to: { $where }.
ng-ui-ws-announce-overlap = Warning: covers content.
ng-ui-ws-announce-widget = Warning: covers a form field.
ng-ui-ws-announce-free = Free area.
ng-ui-ws-announce-field = In signature field “{ $name }”.
ng-ui-ws-announce-qr = In the QR code placeholder.
ng-ui-ws-announce-qr-uncovered = The QR placeholder is not fully covered.
ng-ui-ws-announce-invisible = Invisible signature, no signature block.
ng-ui-ws-snap-left = left margin
ng-ui-ws-snap-right = right margin
ng-ui-ws-snap-hcenter = centre
ng-ui-ws-snap-top = top margin
ng-ui-ws-snap-bottom = bottom margin
ng-ui-ws-snap-vcenter = page centre
ng-ui-ws-snap-below = below the text
ng-ui-ws-snap-corner = bottom right
ng-ui-ws-zone-top-left = top left
ng-ui-ws-zone-top-center = top centre
ng-ui-ws-zone-top-right = top right
ng-ui-ws-zone-middle-left = middle left
ng-ui-ws-zone-middle-center = centre
ng-ui-ws-zone-middle-right = middle right
ng-ui-ws-zone-bottom-left = bottom left
ng-ui-ws-zone-bottom-center = bottom centre
ng-ui-ws-zone-bottom-right = bottom right
ng-ui-ws-loading-title = Opening document
ng-ui-ws-loading = Reading and checking the document …
ng-ui-ws-locked-title = This PDF is password-protected
ng-ui-ws-locked-text = PIDA cannot sign documents that need a password to open. Please open a version without password protection.
ng-ui-ws-locked-other = Open another document
ng-ui-ws-notpdf-title = This file is not a PDF
ng-ui-ws-retry = Try again

## Inspector

ng-ui-insp-label = Signature
ng-ui-insp-sign-with = Sign with
ng-ui-insp-switch = Switch signing method
ng-ui-insp-signer-menu = Signing method
ng-ui-insp-chip-label = Signing method: { $value }. Switch
ng-ui-insp-chip-label-single = Signing method: { $value }
ng-ui-insp-make-default = Use as default
ng-ui-insp-manage = Settings …
ng-ui-insp-signer-atrust = ID Austria
ng-ui-insp-signer-atrust-desc = Confirm in the app, with a FIDO2 key or by SMS TAN
ng-ui-insp-signer-atrust-desc-mobile = Confirm in the “ID Austria” app or by SMS TAN
ng-ui-insp-signer-keystore = Keystore
ng-ui-insp-signer-keystore-desc = Certificate file (PKCS#12) on this device
ng-ui-insp-signer-card = Smart card
ng-ui-insp-signer-card-desc = Card in a reader with local smart card software
ng-ui-insp-signer-card-reader = Smart card · card reader
ng-ui-insp-signer-none = Choose signing method
ng-ui-insp-block = Signature block
ng-ui-insp-block-edit = Edit signature block
ng-ui-insp-block-alt = Preview of the signature block
ng-ui-insp-edit = Edit
ng-ui-insp-options = Options
ng-ui-insp-invisible = Sign invisibly
ng-ui-insp-invisible-hint = No signature block; the signature is only in the file.
ng-ui-insp-new-page = On a new page
ng-ui-insp-new-page-hint = Appends an empty page for the signature block.
ng-ui-insp-block-lang = Signature block language
ng-ui-insp-block-lang-hint = Also applies to further documents (Settings › Signature block).
ng-ui-insp-new-page-locked = Not possible for this document.
# Settings › Document (E3): what was detected or decided automatically on opening
ng-ui-insp-detect-pdfa = PDF/A document detected – will be signed PDF/A-compliant
ng-ui-insp-detect-pdfa-forced = PDF/A-compatible signature (enforced)
ng-ui-insp-detect-qr = QR placeholder found – signature placed there
ng-ui-insp-detect-field = Signature field found – signature placed in the field
ng-ui-insp-placement = Placement
ng-ui-insp-placement-text = Page { $page } · { $zone }
ng-ui-insp-placement-field = In signature field “{ $name }”
ng-ui-insp-placement-qr = In the QR code placeholder
ng-ui-insp-placement-free = Place freely
ng-ui-insp-placement-hint = Drag the block on the page – it snaps to margins and below the text.
ng-ui-insp-auto-place = Re-place
# Setting “Automatic positioning” (ADR 0004 addendum 8)
ng-ui-insp-placement-auto = Automatic (per setting)
ng-ui-insp-placement-locked-hint = Ready to sign. The position is set by the setting “Automatic positioning”.
ng-ui-insp-placement-unlock = Place it myself this time
ng-ui-insp-existing = Existing signatures
ng-ui-insp-existing-meta = Stated at signing: { $time }
ng-ui-insp-existing-unknown = Signatory not readable
ng-ui-insp-status-valid = intact
ng-ui-insp-status-invalid = modified
ng-ui-insp-status-unknown = cannot be checked
ng-ui-insp-overlap = The signature block covers page content.
ng-ui-insp-overlap-widget = The signature block covers a form or signature field.
ng-ui-insp-overlap-fix = Find free space
ng-ui-insp-qr-uncovered = The signature block does not fully cover the QR placeholder – part of the placeholder image remains visible.
ng-ui-insp-emblem-placeholder = The official signature has no emblem of your authority yet – the signature block shows a placeholder.
ng-ui-insp-emblem-placeholder-choose = Choose emblem
ng-ui-insp-emblem-placeholder-mobile = The official signature has no emblem of your authority – the signature block shows a placeholder. An own emblem cannot be set in the app.
ng-ui-insp-emblem-placeholder-qr = The QR placeholder sets the official signature; it shows a placeholder instead of your authority's emblem.
# Sharing hint on iOS (core: `ViewState::share_hint`, ADR 0004 addendum 12)
ng-ui-ws-share-hint = Shared several PDFs? iOS only passes on the first one. For a batch, choose the others with “Add more PDFs …” – you can select several there.
ng-ui-ws-share-hint-dismiss = Hide hint
ng-ui-insp-sign = Sign
    .accesskey = S
ng-ui-insp-sheet-expand = Show options
ng-ui-insp-sheet-collapse = Hide options
ng-ui-insp-warning = Note on the document
ng-ui-insp-warning-proceed = Sign anyway
ng-ui-insp-warning-discard = Discard document

## Signing card

ng-ui-sc-title = Sign with { $signer }
ng-ui-sc-steps = Progress
ng-ui-sc-step-login = Log in
ng-ui-sc-step-app = Confirm in the app
ng-ui-sc-step-fido2 = Confirm with FIDO2
ng-ui-sc-step-sms = Enter SMS TAN
ng-ui-sc-step-unlock = Unlock
ng-ui-sc-step-card = Enter PIN
ng-ui-sc-step-done = Signed
ng-ui-sc-state-done = done
ng-ui-sc-state-current = current step
ng-ui-sc-state-todo = pending
ng-ui-sc-state-error = failed
ng-ui-sc-cancel = Cancel
ng-ui-sc-continue = Continue
ng-ui-sc-details = Technical details
ng-ui-sc-connecting = Connecting …
ng-ui-sc-keystore-opening = Opening the keystore …
ng-ui-sc-certificate = Reading certificate …
ng-ui-sc-waiting-server = Waiting for the signing service …
ng-ui-sc-signing = Creating signature …
ng-ui-sc-done = Signed
ng-ui-sc-saving = Saving the signed document …
ng-ui-sc-document = Document { $index } of { $total }
ng-ui-sc-login-user = Phone number or user name
ng-ui-sc-login-password = Signature password
ng-ui-sc-login-remember = Remember data
ng-ui-sc-login-remember-hint = PIDA does not ask for the password again until you quit; the user name is saved.
ng-ui-sc-login-submit = Log in
ng-ui-sc-login-provider = This service is provided by A-Trust.
ng-ui-sc-app-sent = Notification sent to the ID Austria app
ng-ui-sc-app-instruction = Open the app, compare the value and confirm the signature.
ng-ui-sc-app-instruction-mobile = Confirm in the “ID Austria” app, then return to PIDA.
ng-ui-sc-app-remaining = Time remaining: { $time }
ng-ui-sc-app-low-time = One minute left to confirm in the app.
ng-ui-sc-app-waiting = Waiting for confirmation …
ng-ui-sc-app-confirmed = Confirmed in the app
ng-ui-sc-ref = Reference value
ng-ui-sc-ref-hint = Must match the value shown in the app.
ng-ui-sc-data = Show signature data
ng-ui-sc-alternatives = Instead:
ng-ui-sc-alt-app = Confirm in the app
ng-ui-sc-alt-fido2 = FIDO2 key
ng-ui-sc-alt-sms = SMS TAN
ng-ui-sc-qr-title = Scan the QR code with the “ID Austria” app
ng-ui-sc-qr-continue = Continue
ng-ui-sc-sms-text = You will receive a text message with the TAN.
ng-ui-sc-sms-tan = TAN from the text message
ng-ui-sc-sms-submit = Confirm
ng-ui-sc-sms-err-ref = That is the reference value – please enter the TAN from the text message.
ng-ui-sc-fido2-title = Use a security key
ng-ui-sc-fido2-text = Connect your FIDO2 security key and start the confirmation.
ng-ui-sc-fido2-start = Confirm with key
ng-ui-sc-fido2-touch = Now touch your security key
ng-ui-sc-fido2-touch-hint = The key blinks as soon as it is ready.
ng-ui-sc-fido2-pin = Security key PIN
ng-ui-sc-fido2-pin-text = Enter the PIN of your FIDO2 security key.
ng-ui-sc-fido2-pin-retry = The PIN was not accepted. Please try again.
ng-ui-sc-ks-text = Enter the password for “{ $file }”.
ng-ui-sc-ks-password = Keystore password
ng-ui-sc-ks-key-password = Key password
ng-ui-sc-ks-submit = Sign
ng-ui-sc-proxy-title = Proxy login
ng-ui-sc-proxy-text = The proxy “{ $host }” requires a password.
ng-ui-sc-proxy-password = Proxy password
ng-ui-sc-secret-empty = Please enter the password.
ng-ui-sc-card-text = Insert your smart card into the reader and enter the signature PIN in the smart card software.
ng-ui-sc-card-waiting = Waiting for the smart card software …
ng-ui-sc-err-new-confirmation = Request new confirmation
ng-ui-sc-error-title = That did not work
ng-ui-sc-choose-title = Choose signing method
ng-ui-sc-choose-text = The signing method chosen originally is not available. What would you like to sign with?
ng-ui-sc-choose-unavailable = Currently not available
ng-ui-sc-choose-sign = Sign
# without a choice of signing method (only one, ADR 0004 addendum 20): no “Choose signing method”
ng-ui-sc-single-title = Continue with { $signer }
ng-ui-sc-single-text = The signing method chosen originally is not available. The document will be signed with { $signer }.
ng-ui-sc-single-unavailable = { $signer } is currently not available.

## Done

ng-ui-fin-title = Signed
ng-ui-fin-seal = Signature created and checked (self-check)
ng-ui-fin-text = PIDA checked the new signature itself before saving. signaturpruefung.gv.at also checks the chain of trust and revocation status.
ng-ui-fin-qes = Qualified according to the certificate
ng-ui-fin-ades = Advanced according to the certificate
ng-ui-fin-saving = Saving the signed document …
ng-ui-fin-not-saved = The signed document has not been saved yet.
ng-ui-fin-save = Save as …
ng-ui-fin-save-failed-title = The document could not be saved
ng-ui-fin-save-failed-text = Please choose another location or try again; the exact cause is shown under “Technical details”.
ng-ui-fin-saved-in = Saved in { $folder }
ng-ui-fin-saved-mobile = Saved in this app’s documents
ng-ui-fin-signed-by = Signed by { $name } · { $time }
ng-ui-fin-signed-at = Signed on { $time }
ng-ui-fin-preview = Preview of the signed page
ng-ui-fin-open = Open
ng-ui-fin-reveal = Show in folder
ng-ui-fin-download = Download
ng-ui-fin-ready-web = Ready to download – the result is gone once you close the page.
ng-ui-fin-share = Share
ng-ui-fin-export = Save as …
ng-ui-fin-check = Full verification at signaturpruefung.gv.at
ng-ui-fin-next = Next document
ng-ui-fin-next-hint = Continue with “{ $name }”
ng-ui-fin-waiting =
    { $n ->
        [one] { $n } document waiting
       *[other] { $n } documents waiting
    }
ng-ui-fin-more = Sign another document
ng-ui-fin-overwrite-title = Overwrite file?
ng-ui-fin-overwrite = Overwrite

## Batch (ADR 0004 §3.8): one login, one confirmation per document

ng-ui-batch-title = Batch
ng-ui-batch-docs =
    { $n ->
        [one] { $n } document
       *[other] { $n } documents
    }
ng-ui-batch-lead = All documents are signed in one go: log in once, then confirm each document individually. Documents with errors are skipped.
ng-ui-batch-lead-unlock = All documents are signed in one go – the keystore is unlocked once. Documents with errors are skipped.
ng-ui-batch-lead-each = All documents are signed in one go; you confirm each one individually. Documents with errors are skipped.
ng-ui-batch-lead-ask = All documents are signed in one go. Documents with errors are skipped.
ng-ui-batch-add = Add PDF
    .accesskey = D
ng-ui-batch-sign-all = Sign all
    .accesskey = A
ng-ui-batch-sign-all-long = Sign all ({ $n })
ng-ui-batch-estimate = Expected
ng-ui-batch-estimate-logins =
    { $n ->
        [one] { $n } login
       *[other] { $n } logins
    }
ng-ui-batch-estimate-confirmations =
    { $n ->
        [one] { $n } confirmation
       *[other] { $n } confirmations
    }
ng-ui-batch-estimate-cert = (possibly +1 for the certificate)
ng-ui-batch-estimate-keystore = keystore password once
ng-ui-batch-estimate-none = You choose the signing method when you sign.
ng-ui-batch-output-next = Save: next to each original
ng-ui-batch-output-folder = Save in { $folder }
ng-ui-batch-output-ask = You choose the folder for the signed documents when you start.
ng-ui-batch-output-missing = The folder “{ $folder }” does not exist – you choose another one when you start.
ng-ui-batch-output-app = Save in this app’s documents
ng-ui-batch-existing =
    { $n ->
        [one] One target file already exists – you decide when you start.
       *[other] { $n } target files already exist – you decide when you start.
    }
# Automatic positioning in the batch (ADR 0004 addendum 8): for this batch only or per setting
ng-ui-batch-auto = Position automatically for this batch
ng-ui-batch-auto-hint = All documents are placed automatically and ready to sign – for this batch only, the setting stays unchanged.
ng-ui-batch-auto-setting = Automatic positioning (per setting)
ng-ui-batch-auto-confirm-title =
    { $n ->
        [one] Keep your own placement?
       *[other] Keep your own placements?
    }
ng-ui-batch-auto-confirm-text =
    { $n ->
        [one] You placed one document yourself. Should it be placed automatically as well?
       *[other] You placed { $n } documents yourself. Should they be placed automatically as well?
    }
ng-ui-batch-auto-keep = Keep
ng-ui-batch-auto-replace =
    { $n ->
        [one] Place this one automatically too
       *[other] Place these automatically too
    }
ng-ui-batch-list = Documents in the batch
ng-ui-batch-open = Open and position “{ $name }”
ng-ui-batch-review = Review
ng-ui-batch-remove = Remove “{ $name }” from the batch
ng-ui-batch-remove-short = Remove
ng-ui-batch-status-analyzing = Checking
ng-ui-batch-status-ready = Ready
ng-ui-batch-status-positioned = Positioned
ng-ui-batch-status-rejected = Rejected
ng-ui-batch-status-review = Please review
ng-ui-batch-status-queued = Waiting
ng-ui-batch-status-active = Signing
ng-ui-batch-status-signed = Signed
ng-ui-batch-status-failed = Error
ng-ui-batch-status-skipped = Skipped
ng-ui-batch-status-cancelled = Cancelled
ng-ui-batch-detail-auto = placed automatically
ng-ui-batch-detail-checked = position checked
ng-ui-batch-detail-qr = in the QR placeholder
ng-ui-batch-detail-field = in the signature field “{ $name }”
ng-ui-batch-detail-manual = page { $page }, placed by you
ng-ui-batch-detail-invisible = invisible
ng-ui-batch-detail-locked = Password-protected
ng-ui-batch-detail-notpdf = Not a PDF file
ng-ui-batch-detail-review = Note on the document – open and confirm it individually
ng-ui-batch-detail-signed = as { $file }
ng-ui-batch-detail-skipped = skipped by you
ng-ui-batch-detail-skipped-existing = target file already existed
ng-ui-batch-detail-cancelled = not signed
ng-ui-batch-detail-exists = “{ $file }” already exists
ng-ui-batch-detail-waiting = waiting for confirmation
ng-ui-batch-stage-connecting = connecting
ng-ui-batch-stage-login = login
ng-ui-batch-stage-certificate = reading the certificate
ng-ui-batch-stage-preparing = preparing
ng-ui-batch-stage-confirming = confirmation in progress
ng-ui-batch-stage-signing = signing
ng-ui-batch-stage-finishing = creating the signature
ng-ui-batch-stage-saving = saving
ng-ui-batch-stage-paused = paused
ng-ui-batch-progress = { $done } of { $total } signed
ng-ui-batch-sum-title = Batch complete
ng-ui-batch-sum-title-aborted = Batch cancelled
ng-ui-batch-sum-signed = { $signed } of { $total } signed
ng-ui-batch-sum-skipped = { $n } skipped
ng-ui-batch-sum-rejected = { $n } rejected
ng-ui-batch-sum-failed = { $n } failed
ng-ui-batch-sum-cancelled = { $n } cancelled
ng-ui-batch-sum-not-started = { $n } not started
ng-ui-batch-sum-folder = Saved in { $folder }.
ng-ui-batch-sum-next = Each saved next to its original.
ng-ui-batch-sum-app = Saved in this app’s documents.
ng-ui-batch-reveal = Show in folder
ng-ui-batch-download-zip = Download all as ZIP
ng-ui-batch-new = New batch
ng-ui-batch-back = Back to batch
ng-ui-batch-show = To the batch
ng-ui-batch-start-title =
    { $n ->
        [one] Sign one document
       *[other] Sign { $n } documents
    }
ng-ui-batch-start-folder = Save in
ng-ui-batch-start-folder-none = No folder chosen yet
ng-ui-batch-start-folder-pick = Choose folder …
ng-ui-batch-start-folder-change = Choose another folder …
ng-ui-batch-start-folder-dialog = Folder for the signed documents
ng-ui-batch-start-existing = If the target file already exists
ng-ui-batch-start-existing-count =
    { $n ->
        [one] The target file already exists for one document.
       *[other] The target file already exists for { $n } documents.
    }
ng-ui-batch-start-keep = Keep both
ng-ui-batch-start-keep-desc = The new file gets a number, e.g. “{ $example }”.
ng-ui-batch-start-overwrite = Overwrite
ng-ui-batch-start-overwrite-desc = The existing file is replaced by the signed one.
ng-ui-batch-start-skip = Skip
ng-ui-batch-start-skip-desc =
    { $n ->
        [one] This document is not signed – no confirmation in the app.
       *[other] These documents are not signed – no confirmation in the app.
    }
ng-ui-batch-start-skip-desc-local =
    { $n ->
        [one] This document is not signed.
       *[other] These documents are not signed.
    }
ng-ui-batch-start-go = Start signing
ng-ui-sc-batch-doc = Document { $index } of { $total } · { $name }
ng-ui-sc-batch-confirm = Confirmation { $index } of { $total } · { $name }
ng-ui-sc-batch-count = Confirmations
ng-ui-sc-batch-count-signed = Signed
ng-ui-sc-batch-count-value = { $done } of { $total }
ng-ui-sc-batch-list = All documents in the batch
ng-ui-sc-batch-skip = Skip document
ng-ui-sc-batch-abort = Cancel batch
ng-ui-sc-batch-abort-title = Cancel the batch?
ng-ui-sc-batch-abort-text = Documents already signed stay saved; the others are not signed. The password is forgotten.
ng-ui-sc-batch-abort-text-local = Documents already signed stay saved; the others are not signed.
ng-ui-sc-batch-abort-keep = Continue signing
ng-ui-sc-batch-pause-title = The batch is paused
ng-ui-sc-batch-pause-decide = Please decide how to continue.
ng-ui-sc-batch-pause-new-session = Try again opens a new session – you may need to log in again.
ng-ui-sc-batch-pause-new-session-unlock = Try again reopens the keystore – you may be asked for the password again.
ng-ui-sc-batch-pause-new-session-each = Try again opens a new session.
ng-ui-sc-batch-pause-skip = Skip

## Accessible mode (docs/analyse/06): header switch, placement without the preview, check before signing
ng-ui-shell-a11y = Accessible mode
ng-ui-shell-a11y-desc = Placement without the preview, detailed announcements and a check before signing.
ng-ui-shell-a11y-on = Accessible mode on. Choose how to place the signature under “Signature placement” on the right.
ng-ui-shell-a11y-off = Accessible mode off.
ng-ui-ws-announce-zone = Position: { $zone }.
ng-ui-insp-pos-title = Signature placement
ng-ui-insp-pos-open = Place without the preview …
ng-ui-insp-pos-method = How should the signature be placed?
ng-ui-insp-pos-auto = Automatically
ng-ui-insp-pos-auto-desc = Below the last text on the last page
ng-ui-insp-pos-auto-do = Place automatically
ng-ui-insp-pos-placeholder = Placeholder in the document
ng-ui-insp-pos-placeholder-desc = { $n ->
        [0] QR code placeholder or signature field – none found
        [one] QR code placeholder or signature field – one found
       *[other] QR code placeholder or signature field – { $n } found
    }
ng-ui-insp-pos-coords = Position in millimetres
ng-ui-insp-pos-coords-desc = Enter the page and distances, e.g. from instructions
ng-ui-insp-pos-free = Free area or move it yourself
ng-ui-insp-pos-free-desc = Free areas of the page, arrow keys or dragging
ng-ui-insp-pos-found = Found in the document
ng-ui-insp-pos-field-item = Signature field “{ $name }”
ng-ui-insp-pos-qr-item = QR code placeholder { $id }
ng-ui-insp-pos-qr-item-noid = QR code placeholder
ng-ui-insp-pos-item-where = Page { $page }, { $zone }
ng-ui-insp-pos-none = No QR code placeholder and no empty signature field found.
ng-ui-insp-pos-qr-search-hint = PIDA only looks for QR code placeholders with “Search for signature placeholders automatically” (Settings › Document).
ng-ui-insp-pos-settings-doc = Settings › Document
ng-ui-insp-pos-page = Page
ng-ui-insp-pos-x = From the left (mm)
ng-ui-insp-pos-y = From the bottom (mm)
ng-ui-insp-pos-coords-hint = Measured is the top left corner of the signature block from the bottom left corner of the page – as in PDF-AS. Page { $page }: { $pw } × { $ph } mm, signature block { $bw } × { $bh } mm.
ng-ui-insp-pos-coords-apply = Apply position
ng-ui-insp-pos-err-page = Page 1 to { $max }.
ng-ui-insp-pos-err-x = 0 to { $max } mm, so that the block fits on the page.
ng-ui-insp-pos-err-y = { $min } to { $max } mm (top edge of the block).
ng-ui-insp-pos-free-list = Free areas the block fits into
ng-ui-insp-pos-free-item = Free area { $n }: page { $page }, { $zone }, { $w } × { $h } mm
ng-ui-insp-pos-free-none = No free area is known yet for the loaded pages.
ng-ui-insp-pos-free-hint = Or drag the signature block in the preview or move it with the arrow keys.
ng-ui-insp-pos-to-block = Go to the signature block
ng-ui-insp-pos-current = Current position
ng-ui-insp-pos-step-standard = 15 pt (standard)
ng-ui-insp-pos-quick = Adjust announcements and controls
ng-ui-insp-pos-all-settings = All accessibility settings
ng-ui-insp-check-title = Check before signing
ng-ui-insp-check-placement = Placement
ng-ui-insp-check-block = Signature block
ng-ui-insp-check-block-size = { $w } × { $h } mm
ng-ui-insp-check-invisible = Invisible – no signature block
ng-ui-insp-check-signer = Sign with
ng-ui-insp-check-overlap = Warning: the signature block covers content of the page. Whoever receives the document can then no longer read it.
ng-ui-insp-check-back = Back to placement
ng-ui-insp-check-sign = Sign

## Create QR placeholder (ADR 0004 addendum 24)
ng-ui-shell-qr-title = Create QR placeholder
ng-ui-shell-qr-subtitle = For documents others will sign: the placeholder determines where the signature goes.
ng-ui-shell-qr-id = Number
ng-ui-shell-qr-id-hint = For several signatures, use a separate number each, e.g. 0001 and 0002 – and tell the signers their number.
ng-ui-shell-qr-profile = Signature block
ng-ui-shell-qr-profile-standard = Standard
ng-ui-shell-qr-profile-standard-desc = Signature block for signatures of persons
ng-ui-shell-qr-profile-amtssignatur = Official signature
ng-ui-shell-qr-profile-amtssignatur-desc = For public authorities
ng-ui-shell-qr-profile-signer = As set by the signer
ng-ui-shell-qr-profile-signer-desc = Their own signature block, e.g. with an emblem
ng-ui-shell-qr-lang = Language of the signature block
ng-ui-shell-qr-lang-caption = Language of the caption
ng-ui-shell-qr-alt = QR code placeholder number { $id }, signature block { $profile }, { $lang }
ng-ui-shell-qr-howto = Insert the image into your document as wide as the signature block should be (saved at 8.5 cm). When signing, the signature block replaces the placeholder – in PIDA, PDF-Over and PDF-AS.
ng-ui-shell-qr-save-png = Save as PNG
ng-ui-shell-qr-save-svg = Save as SVG
ng-ui-shell-qr-share-png = Share PNG
ng-ui-shell-qr-share-svg = Share SVG
ng-ui-shell-cmd-qr = Create QR placeholder …
    .keywords = qr placeholder signature template prepare signers several document
ng-ui-shell-cmd-demo = Start demo
    .keywords = demo sample example try test document
ng-ui-start-demo = Try it with the demo document
ng-ui-start-qr-title = Prepare a document for others
ng-ui-start-qr-text = A QR placeholder determines where the signature goes – also for several signers.
ng-ui-start-qr-action = Create QR placeholder
