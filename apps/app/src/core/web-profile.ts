/**
 * Web-Variante (Konzept Webdienst): was die Oberfläche im Browser anbietet. Die Web-Brücke (`web.ts`) setzt das
 * Profil beim Start; außerhalb des Browsers bleibt es `null`. Im Web wird nichts gespeichert – Einstellungen, die
 * nur mit Speichern Sinn haben (Signiermittel, Speichern, Darstellung, Erweitert, Zertifikats-Cache, „Zuletzt
 * signiert“), sind ausgeblendet; Ergebnisse kommen als Download (Stapel als ZIP).
 */
export interface WebProfile {
  /** Keystore als Signiermittel (verstecktes Testmerkmal `?test-keystore`, nicht in der Oberfläche angeboten) */
  keystore: boolean;
}

let profile: WebProfile | null = null;

/** Profil der Web-Variante; `null` = App (Desktop, Mobile) bzw. Mock. */
export function webProfile(): WebProfile | null {
  return profile;
}

/**
 * Betriebssystem für Bedienhinweise („⌘-Klick“ bzw. „Strg-Klick“): Der Kern der Web-Variante kennt das System des
 * Browsers nicht – am Mac zählt dort die Erkennung der Oberfläche.
 */
export function hintOs(os: string, isMac: boolean): string {
  return profile && isMac ? 'macos' : os;
}

/** Nur die Web-Brücke (und Tests). */
export function setWebProfile(p: WebProfile | null): void {
  profile = p;
}
