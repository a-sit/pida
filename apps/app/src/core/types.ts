/**
 * Bridge-Typen der Oberfläche. **Einzige Quelle** ist `crates/core-api/src/view.rs`: Die Typen werden
 * mit ts-rs nach `generated.ts` erzeugt (`PIDA_BLESS=1 cargo test -p pida-core-api --test
 * ts_bindings`) und hier nur weitergereicht; der Rust-Test bricht ab, sobald `generated.ts` nicht
 * mehr zu den Rust-Typen passt. Hier stehen nur abgeleitete Hilfstypen und -funktionen.
 *
 * Abbildungsregeln (serde_json): `Option<T>` → `T | null` (beim Senden immer angeben), `PathBuf` →
 * `string`, Zahlen → `number`, Enums ohne serde-Attribut extern getaggt (Unit-Varianten als String,
 * Daten-Varianten als `{ "Variante": … }`), `#[serde(tag = …)]` intern getaggt.
 *
 * Die Konsistenz der serde-Ausgabe mit diesen Typen prüfen zusätzlich `src/core/contract.test.ts`
 * (Beispiel-JSON aus `apps/app/src-tauri/tests/serde_contract.rs`) und der Rust-Test selbst
 * (TS-Beispiele → `UiEvent`).
 *
 * Neue Oberfläche „Arbeitsbereich“ (ADR 0004, Vertrag E0): Alle Typen aus ADR 0004 §3 stehen in
 * `generated.ts`; neue Einheits-Enums sind `snake_case`-Strings, Enums mit Daten intern getaggt (`kind`,
 * Status-Enums `state`). Bis zur Umsetzung im Kern (K1–K4) kommen sie mit den Standardwerten aus
 * `defaults.ts` (`workspaceDefaults`, `defaultSigningProgress`, `defaultErrorContext`).
 */
export type * from './generated';

import type { BatchItemStatusView, NativeForm, PlacementTarget, StepView, UiEvent, ViewState } from './generated';

/** Name eines Schritts (`StepView['step']`). */
export type StepName = StepView['step'];

/** Name eines Ereignisses. */
export type UiEventType = UiEvent['type'];

/** Status eines Stapeleintrags ohne Daten (`BatchItemStatusView['state']`, ADR 0004 §3.8). */
export type BatchItemState = BatchItemStatusView['state'];

/** Art des Platzierungsziels (`PlacementTarget['kind']`, ADR 0004 §3.3). */
export type PlacementTargetKind = PlacementTarget['kind'];

/** Daten einer Maske (`NativeForm`) mit dem Variantennamen `K`. */
export type FormData<K extends string> = Extract<NativeForm, Record<K, unknown>> extends Record<K, infer D> ? D : never;

/** Mobile Oberfläche (iOS/Android: Teilen statt Ordner, nur ID Austria – kein Keystore, keine lokale BKU –, kein FIDO2). */
export function isMobile(view: Pick<ViewState, 'platform'>): boolean {
  return view.platform?.mobile === true;
}

/** Wo Updates herkommen: auf Android der Play Store, auf iOS TestFlight; am Desktop die Release-Seite (`null`). */
export function updateStore(view: Pick<ViewState, 'platform'>): 'play' | 'testflight' | null {
  if (!isMobile(view)) return null;
  return view.platform?.os === 'android' ? 'play' : 'testflight';
}

/** Variante einer extern getaggten Enum mit Daten (`{ "Name": … }`) bzw. der Unit-String. */
export function variantName(value: string | Record<string, unknown>): string {
  if (typeof value === 'string') return value;
  const keys = Object.keys(value);
  return keys[0] ?? '';
}
