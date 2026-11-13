/**
 * Mock-Kern, Bereich Arbeitsbereich (UI-A): Beispieldokumente, Seitenkacheln und Blockvorschau als SVG,
 * Bearbeitungszustand der Platzierung und Projektion nach `DocumentView` (ADR 0004 §3.3).
 *
 * Die Dokumente sind wie im Prototyp in pt gesetzt (Ursprung **oben links**, `Rect`), damit Inhaltsrahmen,
 * freie Flächen, Einrastlinien und Überdeckung ohne Renderer berechnet werden können; projiziert wird im
 * Koordinatensystem des Kerns (`PAGE_ABS`, Ursprung links unten, `RectPt`). Die Startlage folgt der
 * PDF-AS-Regel wie `PositionChoice::Auto` (waagrecht zentriert, 20 pt unter dem untersten Inhalt der
 * letzten Seite; ohne Platz auf einer neuen Seite, bei signierten Dokumenten auf der letzten Seite).
 * Texte und Personen sind frei erfunden (keine echten Personen, Firmen oder Verträge).
 */
import type {
  BlocksView,
  BlockView,
  DocConstraintsView,
  DocId,
  DocInfo,
  DocumentDetectionView,
  DocumentPhase,
  DocumentView,
  ExistingSignatureView,
  FieldTargetView,
  GuideView,
  Lang,
  PageLayoutView,
  Placement,
  PlacementCheckView,
  PlacementOrigin,
  PlacementTarget,
  PriorStatusView,
  QrTargetView,
  RectPt,
  Settings,
} from '../types';
import { A4, escapeXml, svgDataUrl } from './shared';

/* ================================================================ Dokumente */

/** Rechteck in pt, Ursprung oben links (nur intern). */
export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export type Block =
  | { kind: 'letterhead'; rect: Rect; org: string; lines: string[] }
  | { kind: 'title'; rect: Rect; text: string }
  | { kind: 'subtitle'; rect: Rect; text: string }
  | { kind: 'heading'; rect: Rect; text: string }
  | { kind: 'para'; rect: Rect; text: string }
  | { kind: 'list'; rect: Rect; items: string[] }
  | { kind: 'parties'; rect: Rect; lines: string[] }
  | { kind: 'table'; rect: Rect; columns: string[]; rows: string[][]; numeric: boolean[]; totalRow: boolean }
  | { kind: 'form'; rect: Rect; rows: [string, string][] }
  | { kind: 'sigline'; rect: Rect; left: [string, string]; right: [string, string] }
  | { kind: 'footer'; rect: Rect; text: string }
  | { kind: 'signature'; rect: Rect; signer: string; time: string };

export interface PageSpec {
  w: number;
  h: number;
  blocks: Block[];
}

export interface MockField {
  name: string;
  page: number;
  rect: Rect;
  locked?: boolean;
}

export interface MockExisting {
  field_name: string;
  signer: string | null;
  signing_time: string | null;
  status: PriorStatusView;
  page: number | null;
  rect: Rect | null;
}

export interface MockDocSpec {
  pages: PageSpec[];
  fields: MockField[];
  qr: { page: number; rect: Rect; id: string } | null;
  existing: MockExisting[];
  sizeBytes: number;
  /** mit Benutzerpasswort geschützt (Kern: `error-PDFPwdProtected`) */
  locked: boolean;
  /** DocMDP-zertifiziert (nur vorhandene Felder oder unsichtbar) */
  certified: boolean;
  /** PDF/A-Kennung im XMP (`pdfaid:part`/`conformance`); fehlt = kein PDF/A-Dokument */
  pdfa?: { part: number; conformance: string | null } | null;
}

export interface MockDoc {
  id: DocId;
  path: string;
  name: string;
  spec: MockDocSpec;
  /** bereits signiert (vorhandene Signaturen bzw. in dieser Sitzung signiert) */
  signed: boolean;
}

const W = A4[0];
const H = A4[1];
/** Satzspiegel der Beispielseiten (pt) */
const MARGIN = { top: 56, right: 56, bottom: 48, left: 56 } as const;
const CONTENT_W = W - MARGIN.left - MARGIN.right;
const PARA = { size: 9, line: 13, charFactor: 0.56 } as const;

/** Zeilen eines Absatzes (gieriger Umbruch mit vorsichtiger Zeichenbreite). */
export function wrapText(text: string, width: number, fontSize: number = PARA.size): string[] {
  const cpl = Math.max(8, Math.floor(width / (fontSize * PARA.charFactor)));
  const lines: string[] = [];
  let cur = '';
  for (const word of text.split(/\s+/).filter(Boolean)) {
    if (!cur) cur = word;
    else if (cur.length + 1 + word.length <= cpl) cur += ` ${word}`;
    else {
      lines.push(cur);
      cur = word;
    }
  }
  if (cur) lines.push(cur);
  return lines.length > 0 ? lines : [''];
}

/** Setzt Blöcke von oben nach unten in den Satzspiegel. */
class PageBuilder {
  private y: number = MARGIN.top;
  private readonly blocks: Block[] = [];

  letterhead(org: string, lines: string[]) {
    const h = 16 + lines.length * 11;
    this.blocks.push({ kind: 'letterhead', rect: { x: MARGIN.left, y: this.y, w: CONTENT_W, h }, org, lines });
    this.y += h + 22;
    return this;
  }
  title(text: string) {
    this.blocks.push({ kind: 'title', rect: { x: MARGIN.left, y: this.y, w: CONTENT_W, h: 24 }, text });
    this.y += 28;
    return this;
  }
  subtitle(text: string) {
    this.blocks.push({ kind: 'subtitle', rect: { x: MARGIN.left, y: this.y, w: CONTENT_W, h: 14 }, text });
    this.y += 28;
    return this;
  }
  heading(text: string) {
    this.y += 8;
    this.blocks.push({ kind: 'heading', rect: { x: MARGIN.left, y: this.y, w: CONTENT_W, h: 15 }, text });
    this.y += 19;
    return this;
  }
  para(text: string) {
    const h = wrapText(text, CONTENT_W).length * PARA.line;
    this.blocks.push({ kind: 'para', rect: { x: MARGIN.left, y: this.y, w: CONTENT_W, h }, text });
    this.y += h + 8;
    return this;
  }
  list(items: string[]) {
    const h = items.reduce((s, it) => s + wrapText(it, CONTENT_W - 16).length * PARA.line + 3, 0);
    this.blocks.push({ kind: 'list', rect: { x: MARGIN.left, y: this.y, w: CONTENT_W, h }, items });
    this.y += h + 8;
    return this;
  }
  parties(lines: string[]) {
    const h = lines.length * 14;
    this.blocks.push({ kind: 'parties', rect: { x: MARGIN.left, y: this.y, w: CONTENT_W, h }, lines });
    this.y += h + 12;
    return this;
  }
  table(columns: string[], rows: string[][], numeric: boolean[], totalRow = false) {
    const h = 22 + rows.length * 19;
    this.blocks.push({ kind: 'table', rect: { x: MARGIN.left, y: this.y, w: CONTENT_W, h }, columns, rows, numeric, totalRow });
    this.y += h + 12;
    return this;
  }
  form(rows: [string, string][]) {
    const h = rows.length * 24;
    this.blocks.push({ kind: 'form', rect: { x: MARGIN.left, y: this.y, w: CONTENT_W, h }, rows });
    this.y += h + 10;
    return this;
  }
  sigline(left: [string, string], right: [string, string]) {
    this.y += 26;
    this.blocks.push({ kind: 'sigline', rect: { x: MARGIN.left, y: this.y, w: CONTENT_W, h: 44 }, left, right });
    this.y += 44 + 8;
    return this;
  }
  signature(signer: string, time: string, x: number, w = 230, h = 82) {
    this.y += 14;
    const rect = { x, y: this.y, w, h };
    this.blocks.push({ kind: 'signature', rect, signer, time });
    this.y += h + 8;
    return rect;
  }
  get cursor() {
    return this.y;
  }
  footer(text: string) {
    this.blocks.push({ kind: 'footer', rect: { x: MARGIN.left, y: H - 34, w: CONTENT_W, h: 12 }, text });
    return this;
  }
  build(): PageSpec {
    return { w: W, h: H, blocks: [...this.blocks] };
  }
}

const page = () => new PageBuilder();

const MUSTER_IT = {
  org: 'Muster IT-Dienstleistungen GmbH',
  lines: ['Praterstraße 12 · 1020 Wien · FN 512345a · UID ATU12345678', 'office@muster-it.example · +43 1 234 56 78'],
};

function spec(pages: PageSpec[], extra: Partial<MockDocSpec> = {}): MockDocSpec {
  return { pages, fields: [], qr: null, existing: [], sizeBytes: 214 * 1024, locked: false, certified: false, ...extra };
}

/** Dienstleistungsvertrag, 3 Seiten (Kernablauf); wie im Prototyp ein PDF/A-1b-Dokument. */
export function contractSpec(): MockDocSpec {
  const f = (n: number) => `Dienstleistungsvertrag DV-2026-0412 · Seite ${n} von 3`;
  const p1 = page()
    .letterhead(MUSTER_IT.org, MUSTER_IT.lines)
    .title('Dienstleistungsvertrag')
    .subtitle('Vertragsnummer DV-2026-0412 · Betrieb und Wartung der Kundenplattform')
    .parties([
      'abgeschlossen zwischen',
      'Stadtwerke Beispielstadt GmbH, Hauptplatz 1, 4020 Linz',
      '– im Folgenden „Auftraggeber“ –',
      'und',
      'Muster IT-Dienstleistungen GmbH, Praterstraße 12, 1020 Wien',
      '– im Folgenden „Auftragnehmer“ –',
    ])
    .heading('§ 1 Vertragsgegenstand')
    .para(
      'Der Auftragnehmer übernimmt für den Auftraggeber den Betrieb, die Wartung und die Weiterentwicklung der Kundenplattform „Mein Stadtwerk“ einschließlich der Schnittstellen zu Abrechnung und Zählerdatenmanagement. Art und Umfang der Leistungen ergeben sich aus diesem Vertrag sowie aus der Leistungsbeschreibung in Anlage 1, die einen integrierenden Bestandteil des Vertrags bildet.',
    )
    .heading('§ 2 Leistungsumfang')
    .para('Die Leistungen des Auftragnehmers umfassen insbesondere:')
    .list([
      'den Betrieb der Plattform in einem Rechenzentrum in Österreich mit einer Verfügbarkeit von mindestens 99,5 % im Kalendermonat,',
      'das Einspielen von Sicherheits- und Funktionsupdates innerhalb der in Anlage 1 vereinbarten Fristen,',
      'die Störungsbehebung während der Servicezeiten Montag bis Freitag von 7:00 bis 18:00 Uhr,',
      'monatliche Berichte über Verfügbarkeit, Störungen und durchgeführte Änderungen.',
    ])
    .para('Leistungen, die über den vereinbarten Umfang hinausgehen, erbringt der Auftragnehmer nur nach schriftlicher Beauftragung durch den Auftraggeber zu den Stundensätzen gemäß § 4.')
    .heading('§ 3 Vertragsdauer')
    .para(
      'Der Vertrag tritt mit Unterzeichnung durch beide Vertragsparteien in Kraft und wird auf unbestimmte Zeit abgeschlossen. Die Leistungserbringung beginnt am 1. Jänner 2027. Die ersten 24 Monate gelten als Mindestvertragsdauer, während der eine ordentliche Kündigung ausgeschlossen ist.',
    )
    .footer(f(1))
    .build();
  const p2 = page()
    .heading('§ 4 Vergütung')
    .para('Für die Leistungen nach diesem Vertrag erhält der Auftragnehmer die folgende Vergütung. Alle Beträge verstehen sich in Euro zuzüglich der gesetzlichen Umsatzsteuer.')
    .table(
      ['Leistung', 'Abrechnung', 'Betrag (netto)'],
      [
        ['Einrichtung und Datenmigration', 'einmalig', '4.800,00 €'],
        ['Betrieb und Wartung', 'monatlich', '1.250,00 €'],
        ['Rufbereitschaft außerhalb der Servicezeiten', 'monatlich', '320,00 €'],
        ['Weiterentwicklung', 'je Stunde', '95,00 €'],
        ['Summe laufend pro Monat', '', '1.570,00 €'],
      ],
      [false, false, true],
      true,
    )
    .para(
      'Die monatliche Vergütung ist jeweils im Voraus bis zum fünften Werktag des Monats zur Zahlung fällig. Die Beträge werden jährlich zum 1. Jänner entsprechend dem von Statistik Austria verlautbarten Verbraucherpreisindex 2020 wertgesichert.',
    )
    .heading('§ 5 Abnahme')
    .para(
      'Die Einrichtung gilt als abgenommen, wenn der Auftraggeber nicht binnen zehn Werktagen nach Mitteilung der Betriebsbereitschaft schriftlich wesentliche Mängel rügt. Unwesentliche Mängel hindern die Abnahme nicht und werden im Rahmen der Wartung behoben.',
    )
    .heading('§ 6 Gewährleistung und Haftung')
    .para(
      'Der Auftragnehmer leistet Gewähr dafür, dass die Plattform die in Anlage 1 beschriebenen Eigenschaften aufweist. Die Haftung für leichte Fahrlässigkeit ist, ausgenommen bei Personenschäden, ausgeschlossen. Die Haftung für Folgeschäden und entgangenen Gewinn ist mit der Höhe der Jahresvergütung begrenzt.',
    )
    .heading('§ 7 Datenschutz')
    .para(
      'Soweit der Auftragnehmer personenbezogene Daten im Auftrag verarbeitet, schließen die Parteien eine Vereinbarung gemäß Art. 28 DSGVO ab (Anlage 2). Der Auftragnehmer verarbeitet diese Daten ausschließlich innerhalb der Europäischen Union und setzt dem Stand der Technik entsprechende technische und organisatorische Maßnahmen ein.',
    )
    .footer(f(2))
    .build();
  const p3 = page()
    .heading('§ 8 Vertraulichkeit')
    .para(
      'Die Vertragsparteien verpflichten sich, alle im Rahmen dieses Vertrags erlangten Informationen über die jeweils andere Partei vertraulich zu behandeln. Diese Verpflichtung gilt auch nach Beendigung des Vertragsverhältnisses für die Dauer von fünf Jahren fort.',
    )
    .heading('§ 9 Kündigung')
    .para(
      'Nach Ablauf der Mindestvertragsdauer kann der Vertrag von jeder Partei unter Einhaltung einer Frist von sechs Monaten zum Ende eines Kalendervierteljahres gekündigt werden. Das Recht zur außerordentlichen Kündigung aus wichtigem Grund bleibt unberührt.',
    )
    .heading('§ 10 Schlussbestimmungen')
    .para(
      'Änderungen und Ergänzungen dieses Vertrags bedürfen der Schriftform; qualifiziert elektronisch signierte Erklärungen erfüllen dieses Erfordernis. Es gilt österreichisches Recht unter Ausschluss der Verweisungsnormen. Gerichtsstand ist Linz.',
    )
    .sigline(['Linz, am', 'Für den Auftraggeber'], ['Wien, am', 'Für den Auftragnehmer'])
    .footer(f(3))
    .build();
  return spec([p1, p2, p3], { pdfa: { part: 1, conformance: 'B' } });
}

/** Leeres Dokument, 1 Seite. */
export function blankSpec(): MockDocSpec {
  return spec([{ w: W, h: H, blocks: [] }], { sizeBytes: 3 * 1024 });
}

/** Antrag mit leerem Signaturfeld auf Seite 1 (2 Seiten). */
export function fieldSpec(): MockDocSpec {
  const b1 = page()
    .letterhead('Fördergesellschaft Beispielland', ['Abteilung Wirtschaft und Innovation · Landhausplatz 3 · 6020 Innsbruck'])
    .title('Antrag auf Förderung')
    .subtitle('Programm „Digital starten 2026“ · Antragsnummer DS-26-01873')
    .heading('1. Angaben zur Antragstellerin / zum Antragsteller')
    .form([
      ['Name / Firma', 'Max Mustermann e.U.'],
      ['Anschrift', 'Mariahilfer Straße 88/4, 1070 Wien'],
      ['UID-Nummer', 'ATU87654321'],
      ['E-Mail', 'max@mustermann.example'],
    ])
    .heading('2. Vorhaben')
    .form([
      ['Projekttitel', 'Online-Terminbuchung und digitale Rechnungslegung'],
      ['Projektzeitraum', '01.11.2026 – 30.06.2027'],
      ['Beantragte Förderung', '12.400,00 €'],
    ])
    .heading('3. Erklärung')
    .para(
      'Ich bestätige, dass die Angaben in diesem Antrag vollständig und richtig sind, und nehme zur Kenntnis, dass unrichtige Angaben zur Rückforderung der Förderung führen können. Ich stimme der Verarbeitung meiner Daten zur Abwicklung des Antrags zu.',
    );
  const fieldY = b1.cursor + 18;
  const p1 = b1.footer('Antrag DS-26-01873 · Seite 1 von 2').build();
  p1.blocks.push({ kind: 'subtitle', rect: { x: W - MARGIN.right - 240, y: fieldY - 14, w: 240, h: 11 }, text: 'Unterschrift der Antragstellerin / des Antragstellers' });
  const p2 = page()
    .heading('4. Beilagen')
    .list([
      'Kostenaufstellung mit Angeboten von mindestens zwei Anbietern,',
      'Projektbeschreibung (höchstens drei Seiten),',
      'Bestätigung über die Eintragung im Gewerberegister bzw. Firmenbuch.',
    ])
    .heading('5. Hinweise zur Einreichung')
    .para(
      'Der Antrag ist vollständig ausgefüllt und qualifiziert elektronisch signiert über das Förderportal einzureichen. Anträge ohne gültige Signatur können nicht bearbeitet werden. Die Bearbeitungsdauer beträgt in der Regel sechs bis acht Wochen.',
    )
    .footer('Antrag DS-26-01873 · Seite 2 von 2')
    .build();
  return spec([p1, p2], {
    sizeBytes: 162 * 1024,
    fields: [{ name: 'Unterschrift_Antragsteller', page: 0, rect: { x: W - MARGIN.right - 240, y: fieldY, w: 240, h: 86 } }],
  });
}

/**
 * Formular mit unplausiblen leeren Signaturfeldern wie `manySignFields.pdf` aus den PDF-AS-Testdaten (2 Seiten
 * Letter, ADR 0004 Nachtrag 16): ein übliches Feld (Seite 1), winzige und schmale Felder und ein seitengroßes Feld
 * auf Seite 2 – angeboten werden nur die plausiblen.
 */
export function manyFieldsSpec(): MockDocSpec {
  const LW = 612;
  const LH = 792;
  // PDF-Rechtecke [llx lly urx ury] → Mock-Rechteck (y von oben)
  const r = (llx: number, lly: number, urx: number, ury: number): Rect => ({ x: llx, y: LH - ury, w: urx - llx, h: ury - lly });
  const p1: PageSpec = {
    w: LW,
    h: LH,
    blocks: [
      { kind: 'title', rect: { x: 220, y: 120, w: 340, h: 24 }, text: 'Formular mit Signaturfeldern' },
      { kind: 'para', rect: { x: 56, y: 330, w: 500, h: 60 }, text: 'Testformular mit mehreren leeren Signaturfeldern unterschiedlicher Größe.' },
    ],
  };
  const p2: PageSpec = { w: LW, h: LH, blocks: [{ kind: 'subtitle', rect: { x: 220, y: 90, w: 200, h: 11 }, text: 'Name | Course Title | Date' }] };
  return spec([p1, p2], {
    sizeBytes: 178 * 1024,
    fields: [
      { name: 'Signature_0', page: 0, rect: r(35.655, 694.467, 183.239, 766.262) },
      { name: 'Signature_1', page: 0, rect: r(236.496, 750.458, 251.389, 765.359) },
      { name: 'Signature_2', page: 0, rect: r(224.31, 621.77, 612, 635.767) },
      { name: 'Signature_3', page: 0, rect: r(177.372, 424.447, 190.912, 670.084) },
      { name: 'Signature_4', page: 0, rect: r(349.779, 357.168, 352.938, 581.131) },
      { name: 'Signature_5', page: 0, rect: r(426.053, 454.7, 596.655, 604.611) },
      { name: 'Signature_6', page: 0, rect: r(324.053, 708.014, 383.177, 758.135) },
      { name: 'Signature_7', page: 1, rect: r(0, 0, 612, 792) },
    ],
  });
}

/** Mietvertrag, bereits signiert (Dr. Anna Gruber), 2 Seiten. */
export function signedSpec(): MockDocSpec {
  const p1 = page()
    .title('Mietvertrag')
    .subtitle('über die Wohnung Top 7, Lindengasse 5, 8010 Graz')
    .parties(['Vermieterin: Dr. Anna Gruber, Lindengasse 5, 8010 Graz', 'Mieter: Max Mustermann, geb. 01.01.1990, derzeit wohnhaft in 1070 Wien'])
    .heading('§ 1 Mietgegenstand')
    .para(
      'Vermietet wird die Wohnung Top 7 im dritten Stock des Hauses Lindengasse 5 mit einer Nutzfläche von 68 m², bestehend aus Vorraum, Küche, zwei Zimmern, Bad mit WC und Balkon, samt Kellerabteil Nr. 7.',
    )
    .heading('§ 2 Mietdauer')
    .para(
      'Das Mietverhältnis beginnt am 1. November 2026 und wird auf drei Jahre befristet abgeschlossen. Es endet am 31. Oktober 2029, ohne dass es einer Kündigung bedarf. Der Mieter kann nach Ablauf eines Jahres unter Einhaltung einer dreimonatigen Frist kündigen.',
    )
    .heading('§ 3 Mietzins')
    .table(
      ['Position', '', 'monatlich'],
      [
        ['Hauptmietzins', '', '780,00 €'],
        ['Betriebskosten (Akonto)', '', '165,00 €'],
        ['Umsatzsteuer 10 %', '', '94,50 €'],
        ['Gesamtmietzins', '', '1.039,50 €'],
      ],
      [false, false, true],
      true,
    )
    .footer('Mietvertrag Lindengasse 5 / Top 7 · Seite 1 von 2')
    .build();
  const b2 = page()
    .heading('§ 4 Kaution')
    .para(
      'Der Mieter leistet bei Unterzeichnung eine Kaution in Höhe von drei Bruttomonatsmieten. Die Kaution wird auf einem Sparbuch veranlagt und nach Rückgabe des Mietgegenstands samt Zinsen zurückgezahlt.',
    )
    .heading('§ 5 Erhaltung und Rückgabe')
    .para(
      'Der Mieter hat den Mietgegenstand pfleglich zu behandeln. Bei Ende des Mietverhältnisses ist die Wohnung geräumt und gereinigt, unter Berücksichtigung der gewöhnlichen Abnützung, zurückzustellen.',
    )
    .heading('§ 6 Unterschriften')
    .para('Die Vertragsparteien unterzeichnen diesen Vertrag mit qualifizierter elektronischer Signatur.');
  const time = '2026-10-12T14:03:51+02:00';
  const rect = b2.signature('Dr. Anna Gruber', time, MARGIN.left);
  const p2 = b2.footer('Mietvertrag Lindengasse 5 / Top 7 · Seite 2 von 2').build();
  return spec([p1, p2], {
    sizeBytes: 311 * 1024,
    existing: [{ field_name: 'Signature1', signer: 'Dr. Anna Gruber', signing_time: time, status: 'valid', page: 1, rect }],
  });
}

/** Angebot, 2 Seiten (Stapel). */
export function offerSpec(): MockDocSpec {
  const p1 = page()
    .letterhead(MUSTER_IT.org, MUSTER_IT.lines)
    .parties(['Stadtwerke Beispielstadt GmbH', 'z. H. Frau Mag. Petra Lechner', 'Hauptplatz 1, 4020 Linz'])
    .title('Angebot Nr. 2026-118')
    .subtitle('Wien, 15. Oktober 2026 · gültig bis 30. November 2026')
    .para(
      'Sehr geehrte Frau Mag. Lechner, vielen Dank für Ihre Anfrage. Gerne bieten wir Ihnen die Erweiterung der Kundenplattform um eine Terminbuchung für Zählertausch und Hausanschlüsse zu folgenden Konditionen an:',
    )
    .table(
      ['Position', 'Menge', 'Betrag (netto)'],
      [
        ['Konzeption und Abstimmung', '3 PT', '2.850,00 €'],
        ['Umsetzung Terminbuchung', '8 PT', '7.600,00 €'],
        ['Test, Abnahme und Schulung', '2 PT', '1.900,00 €'],
        ['Gesamt', '', '12.350,00 €'],
      ],
      [false, true, true],
      true,
    )
    .footer('Angebot 2026-118 · Seite 1 von 2')
    .build();
  const p2 = page()
    .heading('Konditionen')
    .para('Die Preise verstehen sich zuzüglich 20 % Umsatzsteuer. Zahlungsziel 30 Tage netto ab Rechnungsdatum. Es gelten die Bestimmungen des Dienstleistungsvertrags DV-2026-0412.')
    .para('Wir freuen uns auf Ihre Beauftragung und stehen für Rückfragen gerne zur Verfügung.')
    .para('Mit freundlichen Grüßen')
    .para('Muster IT-Dienstleistungen GmbH')
    .footer('Angebot 2026-118 · Seite 2 von 2')
    .build();
  return spec([p1, p2], { sizeBytes: 96 * 1024 });
}

/** Auftragsbestätigung mit QR-Platzhalter (PDF-AS-Marker). */
export function confirmationSpec(): MockDocSpec {
  const b = page()
    .letterhead(MUSTER_IT.org, MUSTER_IT.lines)
    .title('Auftragsbestätigung Nr. 4711')
    .subtitle('zu Angebot 2026-097 vom 2. Oktober 2026')
    .para('Wir bestätigen Ihren Auftrag vom 9. Oktober 2026 und erbringen die folgenden Leistungen zu den vereinbarten Konditionen:')
    .table(
      ['Leistung', 'Termin', 'Betrag (netto)'],
      [
        ['Sicherheitsaudit der Plattform', 'KW 45', '3.200,00 €'],
        ['Penetrationstest Schnittstellen', 'KW 46', '2.400,00 €'],
        ['Gesamt', '', '5.600,00 €'],
      ],
      [false, false, true],
      true,
    )
    .para('Bitte bestätigen Sie den Auftrag durch Ihre Signatur im dafür vorgesehenen Feld.');
  const y = b.cursor + 24;
  const p = b.footer('Auftragsbestätigung 4711 · Seite 1 von 1').build();
  return spec([p], { sizeBytes: 74 * 1024, qr: { page: 0, rect: { x: W - MARGIN.right - 230, y, w: 230, h: 82 }, id: '1' } });
}

/** Demodokument der Apps („Demo starten“, `public/PIDA-Demo.pdf`): eine Seite mit QR-Platzhalter Nr. 1. */
export function demoSpec(): MockDocSpec {
  const b = page()
    .title('PIDA – Demodokument')
    .subtitle('Demo document')
    .para('Dieses Dokument liegt PIDA bei, damit Sie das Signieren gefahrlos ausprobieren können. Unten ist ein QR-Platzhalter eingefügt: PIDA erkennt ihn beim Öffnen und setzt den Signaturblock genau an diese Stelle.')
    .para('Sie können die Position im Arbeitsbereich ändern und das Dokument mit ID Austria tatsächlich signieren. Die Signatur ist echt, das Dokument selbst hat keinen rechtlich bedeutsamen Inhalt.');
  const y = b.cursor + 24;
  const p = b.build();
  return spec([p], { sizeBytes: 2 * 1024, qr: { page: 0, rect: { x: MARGIN.left, y, w: 240, h: 48 }, id: '1' } });
}

/** Gehaltszettel, kennwortgeschützt. */
export function lockedSpec(): MockDocSpec {
  const p = page()
    .letterhead('Beispiel Handels GmbH', ['Personalverrechnung · Industriestraße 4 · 4600 Wels'])
    .title('Abrechnung September 2026')
    .footer('Vertraulich · Seite 1 von 1')
    .build();
  return spec([p], { sizeBytes: 184 * 1024, locked: true });
}

/** Beispieldokument zu einem Dateinamen (Szenarien des Prototyps, sonst Vertrag mit 3 Seiten). */
export function specForName(name: string, placeholder: 'none' | 'fields' | 'qr' = 'none'): MockDocSpec {
  const n = name.toLowerCase();
  let s: MockDocSpec;
  if (/pida-demo/.test(n)) s = demoSpec();
  else if (/gehaltszettel|kennwort|scan/.test(n)) s = lockedSpec();
  else if (/leer|blank/.test(n)) s = blankSpec();
  else if (/manysignfields|signaturfelder/.test(n)) s = manyFieldsSpec();
  else if (/antrag|foerder|förder/.test(n)) s = fieldSpec();
  else if (/mietvertrag|signiert|signed/.test(n)) s = signedSpec();
  else if (/angebot|offer/.test(n)) s = offerSpec();
  else if (/auftrag|bestaetigung|bestätigung/.test(n)) s = confirmationSpec();
  else s = contractSpec();
  if (placeholder === 'fields' && s.fields.length === 0 && !s.locked) {
    const last = s.pages.length - 1;
    s.fields.push({ name: 'Unterschrift_Auftraggeber', page: last, rect: { x: MARGIN.left, y: 640, w: 230, h: 82 } });
  }
  if (placeholder === 'qr' && !s.qr && !s.locked) {
    s.qr = { page: s.pages.length - 1, rect: { x: W - MARGIN.right - 230, y: 640, w: 230, h: 82 }, id: '1' };
  }
  return s;
}

/* ================================================================ Darstellung (SVG statt PDFium) */

const INK = '#1a1d22';
const MUTED = '#5b616c';
const RULE = '#d5d9df';
const FONT = 'Helvetica Neue, Helvetica, Arial, Liberation Sans, sans-serif';
const MONO = 'ui-monospace, SF Mono, Menlo, Consolas, monospace';

function text(x: number, y: number, s: string, size: number, opts: { weight?: number; fill?: string; anchor?: 'start' | 'middle' | 'end'; family?: string; italic?: boolean } = {}): string {
  return `<text x="${x.toFixed(1)}" y="${y.toFixed(1)}" font-family="${opts.family ?? FONT}" font-size="${size}"${opts.weight ? ` font-weight="${opts.weight}"` : ''}${opts.italic ? ' font-style="italic"' : ''} fill="${opts.fill ?? INK}"${opts.anchor ? ` text-anchor="${opts.anchor}"` : ''}>${escapeXml(s)}</text>`;
}

function line(x1: number, y1: number, x2: number, y2: number, stroke = RULE, width = 0.6, dash?: string): string {
  return `<line x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}" stroke="${stroke}" stroke-width="${width}"${dash ? ` stroke-dasharray="${dash}"` : ''}/>`;
}

function blockSvg(b: Block): string {
  const r = b.rect;
  switch (b.kind) {
    case 'letterhead':
      return [
        `<rect x="${r.x}" y="${r.y + 3}" width="10" height="10" rx="2.5" fill="#24496e"/>`,
        text(r.x + 16, r.y + 12, b.org, 11, { weight: 700 }),
        ...b.lines.map((l, i) => text(r.x, r.y + 26 + i * 11, l, 7, { fill: MUTED })),
        line(r.x, r.y + r.h, r.x + r.w, r.y + r.h, RULE, 0.8),
      ].join('');
    case 'title':
      return text(r.x, r.y + 19, b.text, 19, { weight: 700 });
    case 'subtitle':
      return text(r.x, r.y + 10, b.text, 8.5, { fill: MUTED });
    case 'heading':
      return text(r.x, r.y + 11, b.text, 10, { weight: 700 });
    case 'para':
      return wrapText(b.text, r.w)
        .map((l, i) => text(r.x, r.y + 9.5 + i * PARA.line, l, PARA.size))
        .join('');
    case 'list': {
      let y = r.y;
      const out: string[] = [];
      for (const item of b.items) {
        const lines = wrapText(item, r.w - 16);
        out.push(`<circle cx="${r.x + 6}" cy="${y + 6.5}" r="1.4" fill="${INK}"/>`);
        lines.forEach((l, i) => out.push(text(r.x + 16, y + 9.5 + i * PARA.line, l, PARA.size)));
        y += lines.length * PARA.line + 3;
      }
      return out.join('');
    }
    case 'parties':
      return b.lines
        .map((l, i) => {
          const muted = /^(und|abgeschlossen|–)/.test(l);
          return text(r.x, r.y + 10 + i * 14, l, 9, muted ? { fill: MUTED, italic: true } : { weight: 600 });
        })
        .join('');
    case 'table': {
      const out: string[] = [];
      const cols = [r.w * 0.55, r.w * 0.2, r.w * 0.25];
      const xs = [r.x, r.x + (cols[0] ?? 0), r.x + (cols[0] ?? 0) + (cols[1] ?? 0)];
      b.columns.forEach((c, i) => {
        const right = b.numeric[i];
        const x = right ? (xs[i] ?? 0) + (cols[i] ?? 0) : (xs[i] ?? 0) + (i === 0 ? 0 : 6);
        out.push(text(x, r.y + 14, c, 8.5, { weight: 700, anchor: right ? 'end' : 'start' }));
      });
      out.push(line(r.x, r.y + 22, r.x + r.w, r.y + 22, INK, 1));
      b.rows.forEach((row, ri) => {
        const top = r.y + 22 + ri * 19;
        const total = b.totalRow && ri === b.rows.length - 1;
        if (total) out.push(line(r.x, top, r.x + r.w, top, INK, 1));
        row.forEach((cell, ci) => {
          const right = b.numeric[ci];
          const x = right ? (xs[ci] ?? 0) + (cols[ci] ?? 0) : (xs[ci] ?? 0) + (ci === 0 ? 0 : 6);
          out.push(text(x, top + 12.5, cell, 8.5, { weight: total ? 700 : undefined, anchor: right ? 'end' : 'start' }));
        });
        if (!total) out.push(line(r.x, top + 19, r.x + r.w, top + 19));
      });
      return out.join('');
    }
    case 'form':
      return b.rows
        .map(([k, v], i) => {
          const top = r.y + i * 24;
          return [text(r.x, top + 18, k.toUpperCase(), 7.5, { fill: MUTED }), text(r.x + 140, top + 18, v, 9), line(r.x, top + 24, r.x + r.w, top + 24)].join('');
        })
        .join('');
    case 'sigline': {
      const half = (r.w - 48) / 2;
      return [b.left, b.right]
        .map(([place, who], i) => {
          const x = r.x + i * (half + 48);
          return [
            text(x, r.y + 9, place, 8.5),
            line(x + 44, r.y + 9, x + half, r.y + 9, MUTED, 0.6, '1 2'),
            line(x, r.y + 30, x + half, r.y + 30, INK, 0.8),
            text(x, r.y + 41, who, 7.5, { fill: MUTED }),
          ].join('');
        })
        .join('');
    }
    case 'footer':
      return text(r.x + r.w / 2, r.y + 9, b.text, 7, { fill: MUTED, anchor: 'middle' });
    case 'signature':
      return signatureBlockSvg(r, { signer: b.signer, time: b.time, profile: 'SIGNATURBLOCK_SMALL', lang: 'de', note: null });
  }
}

/** Seitenkachel als SVG (`pida-render://page/…` im Kern; hier mit Text gesetzt). */
export function pageTileSvg(p: PageSpec | null, size: [number, number], scale: number): string {
  const [w, h] = size;
  const pw = Math.max(1, Math.round(w * scale));
  const ph = Math.max(1, Math.round(h * scale));
  const content = p ? p.blocks.map(blockSvg).join('') : '';
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${pw}" height="${ph}" viewBox="0 0 ${w} ${h}"><rect width="${w}" height="${h}" fill="#fff"/>${content}</svg>`;
  return svgDataUrl(svg);
}

/* ---------------------------------------------------------------- Signaturblock */

const CAPTIONS: Record<Lang, Record<'signer' | 'date' | 'verify' | 'note' | 'issuer' | 'serial' | 'method', string>> = {
  de: { signer: 'Unterzeichner', date: 'Datum/Zeit-UTC', verify: 'Prüfinformation', note: 'Hinweis', issuer: 'Aussteller-Zertifikat', serial: 'Serien-Nr.', method: 'Methode' },
  en: { signer: 'Signatory', date: 'Date/Time-UTC', verify: 'Verification', note: 'Note', issuer: 'Issuer certificate', serial: 'Serial no.', method: 'Method' },
};
const VERIFY_TEXT: Record<Lang, string> = {
  de: 'Informationen zur Prüfung der elektronischen Signatur finden Sie unter: https://www.signaturpruefung.gv.at',
  en: 'Information on the verification of the electronic signature can be found at: https://www.signaturpruefung.gv.at',
};
const DEFAULT_NOTE: Record<'small' | 'amt', Record<Lang, string>> = {
  small: {
    de: 'Dieses mit einer qualifizierten elektronischen Signatur versehene Dokument hat gemäß Art. 25 Abs. 2 der Verordnung (EU) Nr. 910/2014 vom 23. Juli 2014 („eIDAS-VO“) die gleiche Rechtswirkung wie ein handschriftlich unterschriebenes Dokument.',
    en: 'This document is signed with a qualified electronic signature. According to Art. 25 para. 2 of the Regulation (EU) No 910/2014 of 23 July 2014 ("eIDAS-Regulation") it shall have the equivalent legal effect of a handwritten signature.',
  },
  amt: {
    de: 'Dieses Dokument wurde amtssigniert. Auch ein Ausdruck dieses Dokuments hat gemäß § 20 E-Government-Gesetz die Beweiskraft einer öffentlichen Urkunde.',
    en: 'This document was signed/sealed with an official signature. According to § 20 Austrian E-Government-Act a printout of this document is assumed to be authentic.',
  },
};

/** Hinweistext des Blocks (`signature.note`). */
export function noteText(settings: Settings, lang: Lang): string {
  const n = settings.signature.note;
  if (n.mode === 'none') return '';
  if (n.mode === 'custom') return n.text;
  return DEFAULT_NOTE[settings.signature.profile === 'AMTSSIGNATURBLOCK' ? 'amt' : 'small'][lang];
}

/** Blockgröße in pt je Profil (angelehnt an PDF-AS: SMALL 230 × 82 bzw. 49 ohne Hinweis). */
export function blockSize(settings: Settings, lang: Lang): { width: number; height: number } {
  const note = noteText(settings, lang).trim().length > 0;
  switch (settings.signature.profile) {
    case 'AMTSSIGNATURBLOCK':
      return { width: 400, height: note ? 116 : 92 };
    case 'BASE_LOGO': {
      const s = (settings.signature.logo_only_size_mm * 72) / 25.4;
      return { width: s, height: s };
    }
    default:
      return { width: 230, height: note ? 82 : 49 };
  }
}

function emblemSvg(x: number, y: number, size: number): string {
  const s = size / 40;
  return `<g transform="translate(${x} ${y}) scale(${s})"><circle cx="20" cy="20" r="17.5" fill="#fff" stroke="#3a414d" stroke-width="1.6"/><circle cx="20" cy="20" r="13.6" fill="none" stroke="#3a414d" stroke-width="0.7" stroke-dasharray="1.2 1.4"/><path d="M12.5 20.8 17.6 25.6 27.8 14.6" fill="none" stroke="#1f2630" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"/><rect x="15" y="30.2" width="10" height="1.3" fill="#c8102e"/><rect x="15" y="32.6" width="10" height="1.3" fill="#c8102e"/></g>`;
}

/** Signaturblock wie PDF-AS (SIGNATURBLOCK_SMALL/AMTSSIGNATURBLOCK) als SVG-Fragment im Rechteck `r`. */
function signatureBlockSvg(r: Rect, o: { signer: string; time: string; profile: Settings['signature']['profile']; lang: Lang; note: string | null }): string {
  const amt = o.profile === 'AMTSSIGNATURBLOCK';
  const base = amt ? { w: 400, h: 116, main: 92 } : { w: 230, h: 82, main: 49 };
  const note = o.note ?? DEFAULT_NOTE[amt ? 'amt' : 'small'][o.lang];
  const hasNote = note.trim().length > 0;
  const bh = hasNote ? base.h : base.main;
  const s = Math.min(r.w / base.w, r.h / bh);
  const ox = r.x + (r.w - base.w * s) / 2;
  const oy = r.y + (r.h - bh * s) / 2;
  const c = CAPTIONS[o.lang];
  const rows: [string, string][] = amt
    ? [
        [c.signer, o.signer],
        [c.date, o.time],
        [c.issuer, 'a-sign-premium-mobile-07 (Demo-CA), A-Trust GmbH, AT'],
        [c.serial, '1 288 004 912'],
        [c.method, 'urn:pdfsigfilter:bka.gv.at:binaer:v1.1.0'],
        [c.verify, VERIFY_TEXT[o.lang]],
      ]
    : [
        [c.signer, o.signer],
        [c.date, o.time],
        [c.verify, VERIFY_TEXT[o.lang]],
      ];
  const emblemW = amt ? 62 : 42;
  const capW = amt ? 72 : 52;
  const rowH = amt ? 12 : 13;
  const out: string[] = [`<g transform="translate(${ox.toFixed(2)} ${oy.toFixed(2)}) scale(${s.toFixed(4)})">`];
  out.push(`<rect x="0" y="0" width="${base.w}" height="${bh}" fill="#fff" stroke="#6b7480" stroke-width="0.7"/>`);
  out.push(line(emblemW, 0, emblemW, base.main, '#9aa1ab', 0.6));
  out.push(emblemSvg((emblemW - Math.min(emblemW - 8, base.main - 8)) / 2, (base.main - Math.min(emblemW - 8, base.main - 8)) / 2, Math.min(emblemW - 8, base.main - 8)));
  rows.forEach(([k, v], i) => {
    const top = i * rowH;
    const last = i === rows.length - 1;
    const hRow = last ? base.main - top : rowH;
    out.push(line(emblemW + capW, top, emblemW + capW, top + hRow, '#c3c8cf', 0.5));
    if (!last) out.push(line(emblemW, top + hRow, base.w, top + hRow, '#c3c8cf', 0.5));
    out.push(text(emblemW + capW - 3, top + Math.min(hRow, rowH) / 2 + 2, k, 5, { weight: 700, anchor: 'end' }));
    const valLines = last ? wrapText(v, base.w - emblemW - capW - 6, 5.6) : [v];
    valLines.forEach((l, li) => out.push(text(emblemW + capW + 3, top + 7 + li * 6.3, l, 5, { family: MONO })));
  });
  if (hasNote) {
    out.push(line(0, base.main, base.w, base.main, '#9aa1ab', 0.6));
    out.push(line(emblemW, base.main, emblemW, bh, '#c3c8cf', 0.5));
    out.push(text(emblemW - 3, base.main + (bh - base.main) / 2 + 2, c.note, 5, { weight: 700, anchor: 'end' }));
    wrapText(note, base.w - emblemW - 6, 5.2).forEach((l, li) => out.push(text(emblemW + 3, base.main + 7.5 + li * 5.6, l, 4.6, { family: MONO })));
  }
  out.push('</g>');
  return out.join('');
}

/** Blockvorschau (`pida-render://block`, im Kern aus dem Appearance-Objekt) als SVG. */
export function blockPreviewSvg(settings: Settings, lang: Lang, scale: number, revision: number, signer = 'Max Mustermann', time = '2026-10-15T10:42:17+02:00'): string {
  const { width, height } = blockSize(settings, lang);
  const pw = Math.max(1, Math.round(width * scale));
  const ph = Math.max(1, Math.round(height * scale));
  let content: string;
  if (settings.signature.profile === 'BASE_LOGO') {
    content = `<rect x="0" y="0" width="${width}" height="${height}" fill="#fff"/>${emblemSvg(width * 0.1, height * 0.1, Math.min(width, height) * 0.8)}`;
  } else {
    content = signatureBlockSvg({ x: 0, y: 0, w: width, h: height }, { signer, time, profile: settings.signature.profile, lang, note: noteText(settings, lang) });
  }
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${pw}" height="${ph}" viewBox="0 0 ${width} ${height}" data-rev="${revision}">${content}</svg>`;
  return svgDataUrl(svg);
}

/* ================================================================ Bearbeitungszustand */

/** Platzierung im Arbeitsbereich (Spiegel von `WorkspaceEdit` + Lage des Kerns). */
export interface EditState {
  /** linke obere Ecke in `PAGE_ABS` (x = linke Kante, y = Oberkante von unten); `null` = keine Vorschau */
  placement: Placement | null;
  origin: PlacementOrigin;
  target: PlacementTarget;
  invisible: boolean;
  newPage: boolean;
  revision: number;
  clamped: boolean;
  /** zuletzt angefragte Seiten (`InspectPages`, ≤ 8) */
  inspected: number[];
  /** Seiten, deren Belegungsraster schon berechnet ist (simuliert asynchron) */
  ready: number[];
  /** sichtbares Profil verboten (unsichtbares Profil bzw. `-v`) */
  invisibleProfile: boolean;
  /**
   * Lage und Ziel stammen aus der Automatik und folgen den Einstellungen › Dokument (Kern:
   * `WorkspaceEdit::automatic`, Projektion `detection.auto_placed`); `false` nach jeder Platzierung durch die
   * Nutzerin (Ziehen, Tastatur, Seite, neue Seite, „Ins Feld setzen“, „Frei platzieren“/„Neu platzieren“)
   */
  automatic: boolean;
  /**
   * Einstellung „Automatische Positionierung“ (Kern: `WorkspaceEdit::lock`, Projektion
   * `placement_locked`; ADR 0004 Nachtrag 8): `locked` = Verschieben und Zielwahl ohne Wirkung, `released` =
   * „Diesmal selbst platzieren“ (nur dieses Dokument)
   */
  lock: PlacementLock;
  /**
   * Mehrfachsignatur (Kern: `WorkspaceEdit::others`): weitere Signaturblöcke in Reihenfolge, ohne den ausgewählten –
   * je Block eine eigene Signatur. Der ausgewählte Block steht wie bisher in `placement`/`target`/`origin`.
   */
  others?: MockBlock[];
  /** Stelle des ausgewählten Blocks in der Reihenfolge aller Blöcke (Kern: `WorkspaceEdit::active`) */
  active?: number;
}

/** Ein ruhender Signaturblock der Mehrfachsignatur (Kern: `pida_workflow::BlockSlot`). */
export interface MockBlock {
  placement: Placement;
  origin: PlacementOrigin;
  target: PlacementTarget;
}

/** Höchstzahl der Signaturblöcke je Dokument (Kern: `pida_workflow::MAX_BLOCKS`). */
export const MAX_BLOCKS = 10;

/** Sperre der Platzierung (Kern: `pida_workflow::PlacementLock`). */
export type PlacementLock = 'off' | 'locked' | 'released';

/**
 * Einstellung „Automatische Positionierung“ anwenden (Kern: `WorkspaceEdit::follow_auto_position`): an sperrt
 * ein Dokument, dessen Lage aus der Automatik stammt und das nicht aus der Stapelübersicht geöffnet wurde; aus
 * hebt die Sperre auf; „Diesmal selbst platzieren“ bleibt.
 */
export function followAutoPosition(edit: EditState, settings: Settings, fromBatch: boolean): PlacementLock {
  if (edit.lock === 'released') return 'released';
  if (!settings.signature.auto_position) return 'off';
  if (edit.lock === 'locked') return 'locked';
  return edit.automatic && !fromBatch ? 'locked' : 'off';
}

const LOWEST_GAP = 20;
/** Fußzeilenbereich der Beispielseiten, den die Freiraumsuche ausspart (PDF-AS `f`) */
const FOOTER_AREA = 40;

function pageSize(doc: MockDoc, page: number): [number, number] {
  const pages = doc.spec.pages;
  const p = pages[Math.min(page, pages.length - 1)];
  return p ? [p.w, p.h] : A4;
}

/** Unterkante des untersten Inhalts (ohne Fußzeile) von oben, 0 = leere Seite. */
function lowestBody(p: PageSpec | undefined): number {
  if (!p) return 0;
  return p.blocks.filter((b) => b.kind !== 'footer').reduce((m, b) => Math.max(m, b.rect.y + b.rect.h), 0);
}

/** Automatische Platzierung nach der PDF-AS-Regel (`PositionChoice::Auto`). */
export function autoPlacement(doc: MockDoc, block: { width: number; height: number }): { placement: Placement; newPage: boolean } {
  const pages = doc.spec.pages;
  const last = pages.length - 1;
  const [pw, ph] = pageSize(doc, last);
  const x = (pw - block.width) / 2;
  const lowest = lowestBody(pages[last]);
  const topFromTop = lowest > 0 ? lowest + LOWEST_GAP : LOWEST_GAP;
  if (topFromTop + block.height <= ph - FOOTER_AREA) {
    return { placement: { page_index: last, x, y: ph - topFromTop, width: block.width, height: block.height }, newPage: false };
  }
  if (!doc.signed) {
    return { placement: { page_index: pages.length, x, y: ph - LOWEST_GAP, width: block.width, height: block.height }, newPage: true };
  }
  return { placement: { page_index: last, x, y: ph - LOWEST_GAP, width: block.width, height: block.height }, newPage: false };
}

/** Rechteck (oben links) → `RectPt` (`PAGE_ABS`). */
export function toRectPt(r: Rect, pageHeight: number): RectPt {
  return { left: r.x, right: r.x + r.w, top: pageHeight - r.y, bottom: pageHeight - r.y - r.h };
}

/**
 * Schwellen eines angebotenen Felds (Kern: `pida_placement::FIELD_MIN_SCALE`, `FIELD_MIN_SHAPE`,
 * `FIELD_MAX_PAGE_SHARE`).
 */
const FIELD_MIN_SCALE = 0.2;
const FIELD_MIN_SHAPE = 0.125;
const FIELD_MAX_PAGE_SHARE = 0.5;

/**
 * Block im Signaturfeld wie im Kern (`pida_placement::field_fit`/`field_suitable`, ADR 0004 Nachtrag 16):
 * proportional, höchstens natürliche Größe, mittig; geeignet mit Maßstab ≥ ⅕, Formähnlichkeit ≥ ⅛ und höchstens der
 * halben Seite (`page` = Breite × Höhe der Seite des Felds).
 */
export function fieldFit(field: RectPt, block: { width: number; height: number }, page: [number, number] = A4): { rect: RectPt; scale: number; suitable: boolean } {
  const fw = field.right - field.left;
  const fh = field.top - field.bottom;
  if (!(fw > 0 && fh > 0 && block.width > 0 && block.height > 0)) return { rect: field, scale: 0, suitable: false };
  const scale = Math.min(1, fw / block.width, fh / block.height);
  const w = block.width * scale;
  const h = block.height * scale;
  const left = field.left + (fw - w) / 2;
  const bottom = field.bottom + (fh - h) / 2;
  const ratio = fw / fh / (block.width / block.height);
  const shape = Math.min(ratio, 1 / ratio);
  const pageArea = page[0] * page[1];
  const shareOk = !(pageArea > 0) || fw * fh <= pageArea * (FIELD_MAX_PAGE_SHARE + 1e-4);
  const suitable = scale + 1e-4 >= FIELD_MIN_SCALE && shape + 1e-4 >= FIELD_MIN_SHAPE && shareOk;
  return { rect: { left, right: left + w, bottom, top: bottom + h }, scale, suitable };
}

function placementRect(p: Placement): RectPt {
  return { left: p.x, right: p.x + p.width, top: p.y, bottom: p.y - p.height };
}

function intersects(a: RectPt, b: RectPt, pad = 0): boolean {
  return a.left < b.right + pad && a.right + pad > b.left && a.bottom < b.top + pad && a.top + pad > b.bottom;
}

/** Klemmen auf die Seite (F-35 mit KR-F11). */
function clampPlacement(p: Placement, size: [number, number]): { placement: Placement; clamped: boolean } {
  const [pw, ph] = size;
  const x = Math.max(0, Math.min(p.x, pw - p.width));
  const y = Math.min(ph, Math.max(p.y, p.height));
  return { placement: { ...p, x, y }, clamped: Math.abs(x - p.x) > 0.01 || Math.abs(y - p.y) > 0.01 };
}

/** Einschränkungen des Dokuments (`DocConstraintsView`). */
export function constraintsFor(doc: MockDoc, edit: EditState): DocConstraintsView {
  const reasons: string[] = [];
  if (doc.signed) reasons.push('ng-constraint-signed');
  if (doc.spec.certified) reasons.push('ng-constraint-certified');
  if (edit.invisibleProfile) reasons.push('ng-constraint-invisible-profile');
  return {
    new_page_allowed: !doc.signed && !doc.spec.certified,
    visible_allowed: !edit.invisibleProfile,
    new_visible_field_allowed: !doc.spec.certified,
    reasons,
  };
}

/** Automatisches Ziel nach den Einstellungen › Dokument (Kern: `WorkspaceEdit::follow_settings`). */
function automaticTarget(doc: MockDoc, settings: Settings, invisibleProfile: boolean): PlacementTarget {
  const p = settings.placeholder;
  if (!p.search || invisibleProfile) return { kind: 'free' };
  if (p.kind === 'fields') {
    const index = doc.spec.fields.findIndex((f) => !f.locked);
    return index >= 0 ? { kind: 'field', index } : { kind: 'free' };
  }
  return doc.spec.qr ? { kind: 'qr' } : { kind: 'free' };
}

/** Lage im Ziel (Feld bzw. QR-Platzhalter: linke obere Ecke des Ziel-Rechtecks). */
function targetPlacement(doc: MockDoc, target: PlacementTarget, block: { width: number; height: number }): Placement | null {
  const r =
    target.kind === 'qr' && doc.spec.qr
      ? { page: doc.spec.qr.page, rect: doc.spec.qr.rect }
      : target.kind === 'field' && doc.spec.fields[target.index]
        ? { page: doc.spec.fields[target.index]!.page, rect: doc.spec.fields[target.index]!.rect }
        : null;
  if (!r) return null;
  const pr = toRectPt(r.rect, pageSize(doc, r.page)[1]);
  return { page_index: r.page, x: pr.left, y: pr.top, width: block.width, height: block.height };
}

/**
 * Start des Bearbeitens nach der Analyse (Arbeitsbereich-Modus, §3.3 „Regeln“): automatisch nach den
 * Einstellungen › Dokument – „QR-Code-Platzhalter“ → QR-Platzhalter, „Leere Signaturfelder“ → erstes leeres
 * Feld, sonst frei nach der PDF-AS-Regel.
 */
export function startEdit(doc: MockDoc, settings: Settings, lang: Lang): EditState {
  const invisibleProfile = settings.signature.profile === 'INVISIBLE';
  const block = blockSize(settings, lang);
  const base: EditState = {
    placement: null,
    origin: 'auto',
    target: { kind: 'free' },
    invisible: invisibleProfile,
    newPage: false,
    revision: 1,
    clamped: false,
    inspected: [],
    ready: [],
    invisibleProfile,
    automatic: true,
    lock: settings.signature.auto_position ? 'locked' : 'off',
  };
  const target = automaticTarget(doc, settings, invisibleProfile);
  const placement = targetPlacement(doc, target, block);
  if (placement) return { ...base, placement, origin: target.kind === 'qr' ? 'qr' : 'field', target };
  const auto = autoPlacement(doc, block);
  return { ...base, placement: auto.placement, newPage: auto.newPage };
}

function bump(edit: EditState, patch: Partial<EditState>): EditState {
  return { ...edit, ...patch, revision: edit.revision + 1 };
}

export function pageCount(doc: MockDoc, edit: EditState): number {
  return doc.spec.pages.length + (edit.newPage ? 1 : 0);
}

/** `MoveBlock` (absolut, Kern klemmt). */
export function moveBlock(doc: MockDoc, edit: EditState, page: number, x: number, y: number): EditState {
  if (!edit.placement) return edit;
  const target = Math.max(0, Math.min(pageCount(doc, edit) - 1, Math.trunc(page)));
  const { placement, clamped } = clampPlacement({ ...edit.placement, page_index: target, x, y }, pageSize(doc, target));
  return bump(edit, { placement, clamped, origin: 'user', target: { kind: 'free' }, automatic: false });
}

/** `NudgeBlock` (15/1/50 pt). */
export function nudgeBlock(doc: MockDoc, edit: EditState, dx: number, dy: number, fine: boolean, coarse: boolean): EditState {
  if (!edit.placement) return edit;
  const step = fine ? 1 : coarse ? 50 : 15;
  return moveBlock(doc, edit, edit.placement.page_index, edit.placement.x + dx * step, edit.placement.y + dy * step);
}

/** `ChangePage`. */
export function changePage(doc: MockDoc, edit: EditState, page: number): EditState {
  if (!edit.placement) return edit;
  const target = Math.max(0, Math.min(pageCount(doc, edit) - 1, Math.trunc(page)));
  if (target === edit.placement.page_index) return edit;
  const { placement, clamped } = clampPlacement({ ...edit.placement, page_index: target }, pageSize(doc, target));
  return bump(edit, { placement, clamped, origin: 'user', target: { kind: 'free' }, automatic: false });
}

/** `ToggleNewPage` (bei signierten Dokumenten gesperrt). */
export function toggleNewPage(doc: MockDoc, edit: EditState): EditState | null {
  if (!edit.placement) return edit;
  if (!constraintsFor(doc, edit).new_page_allowed && !edit.newPage) return null;
  const newPage = !edit.newPage;
  const page = newPage ? doc.spec.pages.length : doc.spec.pages.length - 1;
  const { placement } = clampPlacement({ ...edit.placement, page_index: page }, pageSize(doc, page));
  return bump(edit, { placement, newPage, target: { kind: 'free' }, origin: 'user', automatic: false });
}

/** `AutoPlace` bzw. `UseTarget{Free}`. */
export function autoPlace(doc: MockDoc, edit: EditState, block: { width: number; height: number }): EditState {
  const auto = autoPlacement(doc, block);
  return bump(edit, { placement: auto.placement, newPage: auto.newPage, origin: 'auto', target: { kind: 'free' }, clamped: false });
}

/**
 * `UseTarget` von der Nutzerin (Index geprüft, gesperrte Felder abgelehnt → `null`); danach folgt die Lage
 * nicht mehr den Einstellungen › Dokument (auch „Frei platzieren“).
 */
export function useTarget(doc: MockDoc, edit: EditState, target: PlacementTarget, block: { width: number; height: number }): EditState | null {
  if (target.kind === 'free') return { ...autoPlace(doc, edit, block), automatic: false };
  if (target.kind === 'field' && (!doc.spec.fields[target.index] || doc.spec.fields[target.index]!.locked)) return null;
  // Mehrfachsignatur: jedes Feld bzw. der QR-Platzhalter nur für einen Block
  if (targetTaken(edit, target)) return null;
  const placement = targetPlacement(doc, target, block);
  if (!placement) return null;
  return bump(edit, { placement, origin: target.kind === 'qr' ? 'qr' : 'field', target, newPage: false, invisible: false, automatic: false });
}

/**
 * Einstellungen geändert (`SettingsChanged`, sofort wirksam): eine Lage aus der Automatik folgt den
 * Einstellungen › Dokument (Ziel neu bestimmt; frei nach der PDF-AS-Regel neu gesetzt), sonst bleibt sie –
 * frei automatisch platzierte Blöcke werden mit der neuen Blockgröße neu gesetzt, von Hand platzierte nur
 * geklemmt (Kern: `follow_settings` und `SettingsChanged`).
 */
export function resizeBlock(doc: MockDoc, edit: EditState, settings: Settings, lang: Lang): EditState {
  const block = blockSize(settings, lang);
  const invisibleProfile = settings.signature.profile === 'INVISIBLE';
  if (!edit.placement) return { ...edit, invisibleProfile };
  const visibility = { invisibleProfile, invisible: invisibleProfile || (edit.invisible && !edit.invisibleProfile) };
  if (edit.automatic) {
    const target = automaticTarget(doc, settings, invisibleProfile);
    const placement = targetPlacement(doc, target, block);
    if (placement) return bump(edit, { placement, origin: target.kind === 'qr' ? 'qr' : 'field', target, newPage: false, clamped: false, ...visibility });
    return { ...autoPlace(doc, edit, block), ...visibility };
  }
  if (edit.origin === 'auto' && edit.target.kind === 'free') return { ...autoPlace(doc, edit, block), ...visibility };
  if (edit.target.kind !== 'free') return bump(edit, { placement: { ...edit.placement, width: block.width, height: block.height }, ...visibility });
  const { placement, clamped } = clampPlacement({ ...edit.placement, width: block.width, height: block.height }, pageSize(doc, edit.placement.page_index));
  return bump(edit, { placement, clamped, ...visibility });
}

/* ================================================================ Mehrfachsignatur (je Block eine Signatur) */

function sameTarget(a: PlacementTarget, b: PlacementTarget): boolean {
  return a.kind === b.kind && (a.kind !== 'field' || (b.kind === 'field' && a.index === b.index));
}

/** Ziel schon von einem anderen (nicht ausgewählten) Block belegt? (Kern: `WorkspaceEdit::target_taken`) */
export function targetTaken(edit: EditState, target: PlacementTarget): boolean {
  return target.kind !== 'free' && (edit.others ?? []).some((b) => sameTarget(b.target, target));
}

function activeIndex(edit: EditState): number {
  return Math.min(edit.active ?? 0, edit.others?.length ?? 0);
}

/** Rechteck eines ruhenden Blocks auf seiner Seite. */
function blockRect(doc: MockDoc, b: MockBlock): RectPt {
  if (b.target.kind === 'field') {
    const f = doc.spec.fields[b.target.index];
    if (f) return fieldFit(toRectPt(f.rect, pageSize(doc, f.page)[1]), { width: b.placement.width, height: b.placement.height }).rect;
  }
  if (b.target.kind === 'qr' && doc.spec.qr) return toRectPt(doc.spec.qr.rect, pageSize(doc, doc.spec.qr.page)[1]);
  return placementRect(b.placement);
}

/** `AddBlock`: neuer, frei platzierter Block an einer freien Stelle der Seite; er wird der ausgewählte. `null` = abgelehnt. */
export function addBlock(doc: MockDoc, edit: EditState, page: number | null, block: { width: number; height: number }): EditState | null | string {
  if (!edit.placement || edit.invisible) return edit;
  const others = edit.others ?? [];
  if (others.length + 1 >= MAX_BLOCKS) return 'ng-blocks-limit';
  const pages = doc.spec.pages.length;
  // mit „Auf neuer Seite“ auch auf der virtuellen neuen Seite (`== Seitenzahl`)
  const last = edit.newPage ? pages : pages - 1;
  const target = page !== null && Number.isInteger(page) && page >= 0 && page <= last ? page : Math.min(edit.placement.page_index, last);
  const [pw, ph] = pageSize(doc, target);
  const current: MockBlock = { placement: edit.placement, origin: edit.origin, target: edit.target };
  const taken = [...others, current].filter((b) => b.placement.page_index === target).map((b) => blockRect(doc, b));
  const gap = 6;
  const free = (x: number, top: number) => !taken.some((r) => intersects({ left: x, right: x + block.width, top, bottom: top - block.height }, r, gap));
  const margin = 36;
  const candidates: [number, number][] = [];
  for (let top = margin + block.height; top <= ph - margin; top += block.height + 2 * gap) {
    for (const x of [pw - block.width - margin, margin, (pw - block.width) / 2]) candidates.push([x, top]);
  }
  const [x, y] = candidates.find(([cx, top]) => free(cx, top)) ?? [edit.placement.x + 24, edit.placement.y - 24];
  const { placement } = clampPlacement({ page_index: target, x, y, width: block.width, height: block.height }, [pw, ph]);
  const at = activeIndex(edit);
  const next = [...others];
  next.splice(at, 0, current);
  return bump(edit, { others: next, active: next.length, placement, origin: 'user', target: { kind: 'free' }, automatic: false, clamped: false });
}

/**
 * Mehrfachsignatur mit „Auf neuer Seite“ (Kern: `order_blocks`): Nur die erste Signatur kann die Seite anhängen –
 * Blöcke auf der neuen Seite stehen deshalb zuerst (automatisch umnummeriert). Ohne neue Seite rücken Blöcke von der
 * virtuellen Seite auf die letzte vorhandene.
 */
export function orderBlocks(doc: MockDoc, edit: EditState): EditState {
  const others = edit.others ?? [];
  if (others.length === 0 || !edit.placement) return edit;
  const pages = doc.spec.pages.length;
  if (!edit.newPage) {
    if (!others.some((b) => b.placement.page_index >= pages)) return edit;
    return { ...edit, others: others.map((b) => (b.placement.page_index >= pages ? { ...b, placement: { ...b.placement, page_index: pages - 1 } } : b)) };
  }
  const order: (MockBlock | null)[] = [...others];
  order.splice(activeIndex(edit), 0, null);
  const onNew = (b: MockBlock | null) => (b ? b.placement.page_index : edit.placement!.page_index) >= pages;
  const sorted = [...order.filter(onNew), ...order.filter((b) => !onNew(b))];
  if (sorted.every((b, i) => b === order[i])) return edit;
  return { ...edit, active: sorted.indexOf(null), others: sorted.filter((b): b is MockBlock => b !== null) };
}

/** `SelectBlock`: der ausgewählte und der gewählte Block tauschen die Plätze. */
export function selectBlock(_doc: MockDoc, edit: EditState, index: number): EditState {
  const others = edit.others ?? [];
  const at = activeIndex(edit);
  if (!edit.placement || !Number.isInteger(index) || index < 0 || index > others.length || index === at) return edit;
  const next = [...others];
  const [chosen] = next.splice(index < at ? index : index - 1, 1);
  if (!chosen) return edit;
  next.splice(at < index ? at : at - 1, 0, { placement: edit.placement, origin: edit.origin, target: edit.target });
  return bump(edit, { others: next, active: index, placement: chosen.placement, origin: chosen.origin, target: chosen.target, clamped: false });
}

/** `RemoveBlock`: der letzte Block bleibt; wird der ausgewählte entfernt, ist der vorige (sonst der nächste) ausgewählt. */
export function removeBlock(_doc: MockDoc, edit: EditState, index: number): EditState {
  const others = edit.others ?? [];
  const at = activeIndex(edit);
  if (others.length === 0 || !Number.isInteger(index) || index < 0 || index > others.length) return edit;
  const next = [...others];
  if (index !== at) {
    next.splice(index < at ? index : index - 1, 1);
    return bump(edit, { others: next, active: index < at ? at - 1 : at });
  }
  const j = Math.max(0, at - 1);
  const [chosen] = next.splice(j, 1);
  if (!chosen) return edit;
  return bump(edit, { others: next, active: j, placement: chosen.placement, origin: chosen.origin, target: chosen.target, clamped: false });
}

/** Ziele, die „Alle Platzhalter belegen“ noch belegen würde (geeignete leere Felder, angebotener QR-Platzhalter). */
function freeTargets(doc: MockDoc, edit: EditState, block: { width: number; height: number }, qrOffered: boolean): PlacementTarget[] {
  const used = (t: PlacementTarget) => sameTarget(edit.target, t) || targetTaken(edit, t);
  const fields: PlacementTarget[] = doc.spec.fields
    .map((f, index) => ({ f, index }))
    .filter(({ f }) => !f.locked && fieldFit(toRectPt(f.rect, pageSize(doc, f.page)[1]), block, pageSize(doc, f.page)).suitable)
    .map(({ index }) => ({ kind: 'field', index }) as PlacementTarget);
  const qr: PlacementTarget[] = doc.spec.qr && qrOffered ? [{ kind: 'qr' }] : [];
  return [...fields, ...qr].filter((t) => !used(t));
}

/** `FillTargets`: je freiem Ziel ein Block; ein frei platzierter ausgewählter Block wandert in das erste Ziel. */
export function fillTargets(doc: MockDoc, edit: EditState, block: { width: number; height: number }, qrOffered: boolean): EditState {
  if (!edit.placement || edit.invisible) return edit;
  const targets = freeTargets(doc, edit, block, qrOffered);
  if (targets.length === 0) return edit;
  let next: EditState = edit;
  let rest = targets;
  if (edit.target.kind === 'free') {
    const first = targets[0]!;
    const placement = targetPlacement(doc, first, block);
    if (placement) next = { ...next, placement, origin: first.kind === 'qr' ? 'qr' : 'field', target: first, newPage: false };
    rest = targets.slice(1);
  }
  const others = [...(next.others ?? [])];
  for (const t of rest) {
    if (others.length + 1 >= MAX_BLOCKS) break;
    const placement = targetPlacement(doc, t, block);
    if (placement) others.push({ placement, origin: t.kind === 'qr' ? 'qr' : 'field', target: t });
  }
  return bump(next, { others, automatic: false });
}

/** Anzahl der Signaturen, die das Dokument erhält (unsichtbar: eine). */
export function blockCount(edit: EditState | null): number {
  return !edit || edit.invisible ? 1 : 1 + (edit.others?.length ?? 0);
}

/* ================================================================ Layout, Prüfung, Projektion */

/** Inhaltsflächen einer Seite in `PAGE_ABS`. */
function occupied(doc: MockDoc, page: number): RectPt[] {
  const p = doc.spec.pages[page];
  if (!p) return [];
  return p.blocks.map((b) => toRectPt(b.rect, p.h));
}

/** Layout einer Seite (Inhaltsrahmen, freie Flächen, Einrastlinien; synthetisch wie `placement::page_layout`). */
export function pageLayout(doc: MockDoc, page: number, block: { width: number; height: number }): PageLayoutView {
  const p = doc.spec.pages[page];
  const [w, h] = pageSize(doc, page);
  const body = p ? p.blocks.filter((b) => b.kind !== 'footer') : [];
  const guides: GuideView[] = [
    { axis: 'x', at: w / 2, kind: 'center_x' },
    { axis: 'y', at: h / 2, kind: 'center_y' },
  ];
  if (body.length === 0) {
    guides.push(
      { axis: 'x', at: 36, kind: 'margin_left' },
      { axis: 'x', at: w - 36, kind: 'margin_right' },
      { axis: 'y', at: h - 36, kind: 'margin_top' },
      { axis: 'y', at: 36, kind: 'margin_bottom' },
    );
    const free = [{ left: 36, bottom: 36, right: w - 36, top: h - 36 }].filter((r) => r.right - r.left >= block.width && r.top - r.bottom >= block.height);
    return { page, content: null, free, guides };
  }
  const rects = body.map((b) => toRectPt(b.rect, h));
  const content: RectPt = {
    left: Math.min(...rects.map((r) => r.left)),
    right: Math.max(...rects.map((r) => r.right)),
    top: Math.max(...rects.map((r) => r.top)),
    bottom: Math.min(...rects.map((r) => r.bottom)),
  };
  guides.push(
    { axis: 'x', at: content.left, kind: 'content_left' },
    { axis: 'x', at: content.right, kind: 'content_right' },
    { axis: 'y', at: content.top, kind: 'content_top' },
    { axis: 'y', at: content.bottom, kind: 'content_bottom' },
  );
  if (content.bottom - 18 - block.height >= 36) guides.push({ axis: 'y', at: content.bottom - 18, kind: 'below_content' });
  // Freie waagrechte Bänder zwischen den Inhalten (6 pt Abstand), in die der Block passt
  const all = occupied(doc, page).sort((a, b) => b.top - a.top);
  const bands: RectPt[] = [];
  let cursor = h - 36;
  for (const r of all) {
    const top = r.top + 6;
    if (cursor - top >= block.height) bands.push({ left: content.left, right: content.right, top: cursor, bottom: top });
    cursor = Math.min(cursor, r.bottom - 6);
  }
  if (cursor - 36 >= block.height) bands.push({ left: content.left, right: content.right, top: cursor, bottom: 36 });
  // zuerst: unter dem Inhalt (rechts unten), dann von unten nach oben
  const free = bands
    .filter((r) => r.right - r.left >= block.width)
    .sort((a, b) => (a.top <= content.bottom ? 0 : 1) - (b.top <= content.bottom ? 0 : 1) || a.bottom - b.bottom)
    .slice(0, 16);
  return { page, content, free, guides };
}

/** Live-Prüfung der Platzierung. */
function check(doc: MockDoc, edit: EditState, sticker: RectPt | null, block: { width: number; height: number }): PlacementCheckView {
  const page = edit.placement?.page_index ?? 0;
  const known = edit.ready.includes(page) && page < doc.spec.pages.length;
  if (!sticker || edit.invisible)
    return { for_revision: edit.revision, overlaps_content: known ? false : null, overlaps_widget: false, clamped: edit.clamped, placeholder_uncovered: false, overlaps_block: false };
  const [, ph] = pageSize(doc, page);
  const targetRect =
    edit.target.kind === 'field'
      ? doc.spec.fields[edit.target.index]
      : edit.target.kind === 'qr' && doc.spec.qr
        ? { page: doc.spec.qr.page, rect: doc.spec.qr.rect }
        : null;
  const exclude = targetRect ? toRectPt(targetRect.rect, ph) : null;
  const content = occupied(doc, page).filter((r) => !exclude || !intersects(r, exclude));
  const overlapsContent = known ? content.some((r) => intersects(sticker, r)) : null;
  // leere Felder, die kein plausibles Ziel sind (seitengroß, 3 pt schmal), sind auch kein Hindernis (wie der Kern)
  const widgets = doc.spec.fields
    .filter((f, i) => f.page === page && !(edit.target.kind === 'field' && edit.target.index === i))
    .map((f) => toRectPt(f.rect, ph))
    .filter((r) => fieldFit(r, block, pageSize(doc, page)).suitable)
    .concat(doc.spec.existing.filter((e) => e.page === page && e.rect).map((e) => toRectPt(e.rect as Rect, ph)));
  // Mehrfachsignatur: der ausgewählte Block überdeckt einen anderen Block desselben Dokuments
  const overlapsBlock = (edit.others ?? []).some((b) => b.placement.page_index === page && intersects(sticker, blockRect(doc, b)));
  return {
    for_revision: edit.revision,
    overlaps_content: overlapsContent,
    overlaps_widget: widgets.some((r) => intersects(sticker, r)),
    clamped: edit.clamped,
    placeholder_uncovered: false,
    overlaps_block: overlapsBlock,
  };
}

function mm(pt: number): string {
  return String(Math.round((pt * 25.4) / 72));
}

export function docInfo(doc: MockDoc): DocInfo {
  return { id: doc.id, name: doc.name, page_count: doc.spec.pages.length, page_sizes: doc.spec.pages.map((p) => [p.w, p.h] as [number, number]), signed: doc.signed };
}

/** Amtssignatur ohne eigene Bildmarke? (wie `CoreTask::emblem_placeholder`) */
export function emblemPlaceholder(settings: Settings): boolean {
  return settings.signature.profile === 'AMTSSIGNATURBLOCK' && !settings.signature.emblem;
}

/** Projektion des aktuellen Dokuments (`ViewState::document`). */
export function projectDocument(doc: MockDoc, edit: EditState, phase: DocumentPhase, settings: Settings, lang: Lang, previewRevision: number): DocumentView {
  const size = blockSize(settings, lang);
  // Amtssignatur ohne eigene Bildmarke: neutrale Platzhalter-Bildmarke (wie `emblem_placeholder` im Kern, Backlog Nr. 10)
  const block: BlockView = { width: size.width, height: size.height, preview_revision: previewRevision, emblem_placeholder: emblemPlaceholder(settings) && !edit.invisible, emblem_choosable: true, language: lang };
  const p = edit.placement;
  let sticker: RectPt | null = null;
  if (p && !edit.invisible) {
    if (edit.target.kind === 'field') {
      // im Feld proportional, höchstens natürliche Größe, mittig (wie der Kern schreibt, Nachtrag 16)
      const f = doc.spec.fields[edit.target.index];
      sticker = f ? fieldFit(toRectPt(f.rect, pageSize(doc, f.page)[1]), size).rect : placementRect(p);
    } else if (edit.target.kind === 'qr' && doc.spec.qr) {
      sticker = toRectPt(doc.spec.qr.rect, pageSize(doc, doc.spec.qr.page)[1]);
    } else {
      sticker = placementRect(p);
    }
  }
  const fields: FieldTargetView[] = doc.spec.fields.map((f, index) => {
    const rect = toRectPt(f.rect, pageSize(doc, f.page)[1]);
    const fit = fieldFit(rect, size, pageSize(doc, f.page));
    return { index, name: f.name, page: f.page, rect, selectable: !f.locked, suitable: fit.suitable, block: fit.rect };
  });
  const qrSpec = doc.spec.qr;
  // angeboten nur mit Platzhaltersuche „QR-Code-Platzhalter“; ein von Hand gewähltes QR-Ziel bleibt
  const qrOffered = (settings.placeholder.search && settings.placeholder.kind === 'marker') || (edit.target.kind === 'qr' && !edit.automatic);
  const qr: QrTargetView | null = qrSpec && qrOffered ? { page: qrSpec.page, rect: toRectPt(qrSpec.rect, pageSize(doc, qrSpec.page)[1]), id: qrSpec.id } : null;
  const existing: ExistingSignatureView[] = doc.spec.existing.map((e) => ({
    field_name: e.field_name,
    signer: e.signer,
    signing_time: e.signing_time,
    status: e.status,
    page: e.page,
    rect: e.rect && e.page !== null ? toRectPt(e.rect, pageSize(doc, e.page)[1]) : null,
  }));
  const pages = pageCount(doc, edit);
  const layoutPages = [...new Set([...edit.inspected, ...(p ? [p.page_index] : [])])]
    .filter((i) => i < doc.spec.pages.length && edit.ready.includes(i))
    .sort((a, b) => a - b)
    .slice(0, 9);
  const announce: Record<string, string> = {};
  if (p) {
    const r = sticker ?? placementRect(p);
    announce.page = String(p.page_index + 1);
    announce.pages = String(pages);
    announce.left = mm(r.left);
    announce.top = mm(r.top);
    announce.bottom = mm(r.bottom);
  }
  return {
    doc: docInfo(doc),
    size_bytes: doc.spec.sizeBytes,
    phase,
    revision: edit.revision,
    block,
    placement:
      edit.target.kind === 'field' && sticker && p
        ? { page_index: p.page_index, x: sticker.left, y: sticker.top, width: sticker.right - sticker.left, height: sticker.top - sticker.bottom }
        : (p ?? { page_index: doc.spec.pages.length - 1, x: 0, y: 0, width: size.width, height: size.height }),
    origin: edit.origin,
    target: edit.target,
    invisible: edit.invisible,
    new_page: edit.newPage,
    sticker,
    fields,
    qr,
    existing,
    constraints: constraintsFor(doc, edit),
    layout: layoutPages.map((i) => pageLayout(doc, i, size)),
    check: check(doc, edit, sticker, size),
    announce,
    detection: detectionFor(doc, edit, settings),
    placement_locked: edit.lock === 'locked',
    // neue Seite: Größe der letzten Seite (Mock ohne CropBox)
    new_page_size: pageSize(doc, doc.spec.pages.length - 1),
    blocks: blocksView(doc, edit, phase, sticker, size, qr !== null),
  };
}

/** Mehrfachsignatur: alle Blöcke in ihrer Reihenfolge (Kern: `BlocksView`). */
function blocksView(doc: MockDoc, edit: EditState, phase: DocumentPhase, sticker: RectPt | null, block: { width: number; height: number }, qrOffered: boolean): BlocksView {
  const others = edit.invisible ? [] : (edit.others ?? []);
  const at = edit.invisible ? 0 : activeIndex(edit);
  const active = { index: 0, active: true, page: edit.placement?.page_index ?? 0, rect: sticker, target: edit.target };
  const items = others.map((b) => ({ index: 0, active: false, page: b.placement.page_index, rect: blockRect(doc, b) as RectPt | null, target: b.target }));
  items.splice(at, 0, active);
  items.forEach((item, i) => (item.index = i));
  const open = phase === 'editing' && !edit.invisible && edit.lock !== 'locked' && edit.placement !== null;
  return {
    items,
    limit: MAX_BLOCKS,
    can_add: open && items.length < MAX_BLOCKS && constraintsFor(doc, edit).new_visible_field_allowed,
    free_targets: open ? freeTargets(doc, edit, block, qrOffered).length : 0,
  };
}

/** Profil mit PDF/A-Variante (`SIGNATURBLOCK_SMALL_*_PDFA`); Amtssignatur, „nur Bildmarke“, unsichtbar nicht. */
function profileHasPdfa(settings: Settings): boolean {
  return settings.signature.profile === 'SIGNATURBLOCK_SMALL';
}

/** Erkanntes und automatisch Entschiedenes (Kern: `DocumentDetectionView`, PDF/A wie `plan::pdfa_mode`). */
export function detectionFor(doc: MockDoc, edit: EditState, settings: Settings): DocumentDetectionView {
  const pdfa = doc.spec.pdfa ? { ...doc.spec.pdfa } : null;
  const detected = pdfa !== null && (pdfa.part <= 3 || profileHasPdfa(settings));
  const forced = pdfa === null && settings.signature.pdfa_force && profileHasPdfa(settings);
  return { pdfa, pdfa_decision: detected ? 'detected' : forced ? 'forced' : 'none', auto_placed: edit.automatic };
}
