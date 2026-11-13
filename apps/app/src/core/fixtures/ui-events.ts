/**
 * Beispiel-Ereignisse aller `UiEvent`-Varianten, **typisiert** (der Compiler prüft sie gegen
 * `types.ts`). `contract.test.ts` schreibt bzw. vergleicht daraus `ui-events.json`; der Rust-Test
 * `apps/app/src-tauri/tests/serde_contract.rs` liest diese Datei als `pida_core_api::UiEvent`.
 * Die Zugangsdaten sind Testwerte ohne Bedeutung.
 */
import { defaultSettings } from '../defaults';
import type { Settings, UiEvent } from '../types';

const settings: Settings = {
  ...defaultSettings(),
  general: {
    ui_language: 'en',
    update_check: false,
    window_size: { width: 860, height: 900 },
    last_source_directory: '/Users/demo',
    recent_documents: false,
    first_run_completed: true,
  },
  signature: {
    profile: 'BASE_LOGO',
    block_language: 'de',
    note: { mode: 'custom', text: 'Eigener Hinweis' },
    pdfa_force: true,
    auto_position: false,
    emblem: '/Users/demo/logo.png',
    logo_only_size_mm: 30.5,
    cms_profile: 'pades_bb',
  },
  keystore: { enabled: true, file: '/Users/demo/keystore.p12', declared_type: 'PKCS12', alias: 'signer', password_storage: 'keychain', storage_chosen: false },
  proxy: { mode: 'manual', host: 'proxy.example.invalid', port: 3128, user: 'demo' },
  // unbekannte Einträge (Settings::unknown, flatten) müssen den Rundlauf überstehen
  'signature.future_key': 'bleibt',
  future: 42,
};

export const UI_EVENT_SAMPLES: UiEvent[] = [
  { type: 'open_files', paths: ['/Users/demo/Vertrag.pdf', 'C:\\Users\\demo\\Angebot.pdf'] },
  { type: 'answer_warning', proceed: true },
  { type: 'placeholder_answer', answer: 'Yes', field_index: 1 },
  { type: 'move_block', page: 1, x: 100.5, y: 210 },
  { type: 'nudge_block', dx: -1, dy: 1, fine: true, coarse: false },
  { type: 'change_page', page: 2 },
  { type: 'toggle_new_page' },
  { type: 'auto_place' },
  { type: 'confirm_position' },
  { type: 'choose_invisible' },
  { type: 'choose_backend', backend: 'atrust_native' },
  { type: 'submit_form', request: 1, answer: { kind: 'login', username: '+43664123456789', password: 'geheim-123', remember: true } },
  { type: 'submit_secret', request: 2, secret: 'geheim-123' },
  { type: 'answer_notice', request: 3, answer: 'Retry' },
  { type: 'open_signature_data', request: 4 },
  { type: 'cancel' },
  { type: 'retry' },
  { type: 'save', path: '/Users/demo/Vertrag_signed.pdf' },
  { type: 'confirm_overwrite', overwrite: false },
  { type: 'open_result' },
  { type: 'navigate', step: 'Positioning' },
  { type: 'open_settings' },
  { type: 'save_settings', settings },
  { type: 'close_settings' },
  { type: 'reset_settings' },
  { type: 'backgrounded' },
  { type: 'foregrounded' },
  { type: 'quit' },
  { type: 'dismiss_notice', id: 3 },
  { type: 'accept_migration', store_secrets: true },
  { type: 'decline_migration' },
  // Mobile (Plan §5.2): Teilen/Exportieren, „Signatur fortsetzen“, PKCS#12-Import
  { type: 'share_result' },
  { type: 'export_result' },
  { type: 'resume_signature' },
  { type: 'discard_resume' },
  { type: 'import_keystore', path: '/var/mobile/Containers/Data/Application/X/Library/Caches/import/test.p12', password: 'geheim-123', remember: true },
  { type: 'dismiss_keystore_import' },
  // ADR 0004 (neue Oberfläche „Arbeitsbereich“, Vertrag E0)
  { type: 'set_invisible', invisible: true },
  { type: 'use_target', target: { kind: 'field', index: 1 } },
  { type: 'inspect_pages', pages: [0, 1, 2] },
  { type: 'select_signer', backend: 'atrust_native', make_default: true },
  { type: 'sign_with', backend: 'atrust_native' },
  { type: 'show_batch' },
  { type: 'open_batch_item', doc: 8 },
  { type: 'back_to_batch' },
  { type: 'remove_from_batch', doc: 9 },
  { type: 'start_batch', backend: 'atrust_native', docs: [7, 8, 10], output_dir: '/Users/demo/Signiert', on_existing: 'keep_both' },
  { type: 'skip_batch_document' },
  { type: 'abort_batch' },
  { type: 'remove_recent', id: 1 },
  { type: 'clear_recent' },
  // Nachtrag 15: „Zuletzt signiert“ bietet, was „Fertig“ kann (statt des entfernten Prüfmodus)
  { type: 'open_recent', id: 2 },
  { type: 'reveal_recent', id: 2 },
  { type: 'share_recent', id: 3 },
  { type: 'sign_recent', id: 2 },
  {
    type: 'apply_settings',
    changes: { 'signature.profile': 'AMTSSIGNATURBLOCK', 'proxy.port': 3128, 'output.folder': null, 'signature.note': { mode: 'none' } },
    revision: 7,
  },
  { type: 'set_ui_language', language: 'en' },
  { type: 'update_first_run', default_backend: 'id_austria', profile: 'SIGNATURBLOCK_SMALL', block_language: 'de' },
  { type: 'finish_first_run' },
  { type: 'offer_migration' },
  // Nachtrag ADR 0004 (Keystore): Einträge mit Passwort lesen, „Passwort merken?“ beantworten
  { type: 'inspect_keystore', password: 'geheim-123', password_storage: 'keychain' },
  // Nachtrag 8: „Diesmal selbst platzieren“, „Für diesen Stapel automatisch positionieren“
  { type: 'unlock_placement' },
  { type: 'set_batch_auto_position', enabled: true, replace_manual: false },
  // Mehrfachsignatur: je Block eine eigene Signatur
  { type: 'add_block', page: 1 },
  { type: 'select_block', index: 0 },
  { type: 'remove_block', index: 1 },
  { type: 'fill_targets' },
  // Nachtrag 21: „Über PIDA“ – „Jetzt prüfen“ bzw. „Erneut prüfen“
  { type: 'check_for_updates', explicit: true },
];

/** Weitere Antworten (alle `FormAnswerView`-Varianten, `save` ohne Pfad, Platzhalter ohne Feld). */
export const UI_EVENT_EXTRA: UiEvent[] = [
  { type: 'submit_form', request: 5, answer: { kind: 'tan', tan: '917253' } },
  { type: 'submit_form', request: 6, answer: { kind: 'switch_to_sms' } },
  { type: 'submit_form', request: 7, answer: { kind: 'switch_to_fido2' } },
  { type: 'submit_form', request: 8, answer: { kind: 'start_fido2' } },
  { type: 'submit_form', request: 9, answer: { kind: 'continue' } },
  { type: 'save', path: null },
  { type: 'placeholder_answer', answer: 'Cancel', field_index: null },
  { type: 'answer_notice', request: 10, answer: 'Ok' },
  { type: 'navigate', step: 'Open' },
  { type: 'choose_backend', backend: 'keystore' },
  { type: 'answer_warning', proceed: false },
  { type: 'accept_migration', store_secrets: false },
  { type: 'import_keystore', path: '/data/user/0/at.asit.pida.app/cache/import/k.p12', password: null, remember: false },
  // ADR 0004: übrige Varianten der neuen Datentypen
  { type: 'use_target', target: { kind: 'free' } },
  { type: 'use_target', target: { kind: 'qr' } },
  { type: 'inspect_pages', pages: [] },
  { type: 'select_signer', backend: null, make_default: true },
  { type: 'sign_with', backend: 'keystore' },
  { type: 'start_batch', backend: 'keystore', docs: [7], output_dir: null, on_existing: 'overwrite' },
  { type: 'start_batch', backend: 'local_bku', docs: [], output_dir: null, on_existing: 'skip' },
  { type: 'apply_settings', changes: {}, revision: 0 },
  { type: 'set_ui_language', language: null },
  // „QR-Platzhalter erstellen“ (ADR 0004 Nachtrag 24)
  { type: 'export_qr_placeholder', spec: { id: '0001', profile: 'standard', language: 'de' }, format: 'png', path: '/Users/demo/qr-platzhalter-0001.png' },
  { type: 'export_qr_placeholder', spec: { id: '2', profile: 'signer', language: 'en' }, format: 'svg', path: null },
  // Einstellungen › Barrierefreiheit (Schalter „Barrierefreie Bedienung“, docs/analyse/06)
  {
    type: 'set_accessibility',
    accessibility: { mode: true, start: 'methods', announce: 'detailed', announce_after_move: true, keyboard_step: 'mm5', check_before_sign: true },
  },
  { type: 'update_first_run', default_backend: null, profile: 'AMTSSIGNATURBLOCK', block_language: null },
  { type: 'submit_secret', request: 3, secret: 'geheim-123', password_storage: 'none' },
  { type: 'inspect_keystore', password: null },
  // Nachtrag 9: „+ PDF“ – „Weitere PDFs hinzufügen“, „Anderes PDF öffnen“
  { type: 'open_files', paths: ['/Users/demo/Angebot.pdf', '/Users/demo/Auftrag.pdf'], mode: 'add' },
  { type: 'open_files', paths: ['/Users/demo/Vertrag.pdf'], mode: 'replace' },
  // Nachtrag 10: „Zur Startseite“ parkt offene Arbeit – „Weiter bearbeiten“, „Schließen“
  { type: 'go_home' },
  { type: 'resume_parked' },
  { type: 'discard_parked' },
  // Nachtrag 12: Ersteinrichtung „Später einrichten“; von außen übergebene Dateien (Shell), Hinweis zum Teilen (iOS)
  { type: 'postpone_first_run' },
  { type: 'files_received', paths: ['/private/var/mobile/Containers/Data/Application/X/Library/Caches/inbox/Vertrag.pdf'] },
  { type: 'dismiss_share_hint' },
  // Nachtrag 15: „Erneut signieren“ nach der Rückfrage bei geparkter Arbeit
  { type: 'sign_recent', id: 3, mode: 'add' },
  { type: 'sign_recent', id: 3, mode: 'replace' },
  // Nachtrag 21: beim Öffnen von „Über PIDA“ (der Kern entscheidet, ob er prüft)
  { type: 'check_for_updates', explicit: false },
];
