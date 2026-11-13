//! `pida-pdf-cos` – Fassade um `lopdf` (nur Parser und Objektmodell).
//!
//! Verantwortung (Plan §3.2, §6.1, §6.3; docs/analyse/03 §4.13, §10 Punkt 1):
//! - lenientes Lesen eines Eingabe-PDFs aus unveränderlichen Bytes ([`SourcePdf::open`]),
//! - Revisionen und xref-Typ (Tabelle, Stream, Hybrid) für den Append-only-Writer,
//! - Standard-Security-Handler (RC4/AES R2–R6, nur ohne User-Passwort) und Rechteprüfung wie
//!   PDF-AS `PdfBoxUtils` ([`Rights::may_sign`]),
//! - Seitengeometrie ([`PageInfo`]), Signaturfelder ([`SigField`]) und Bild-XObject-Aufrufe
//!   ([`ImageInvocation`]) als **reine Datenstrukturen**, die andere Pakete in Tests selbst bauen
//!   können (ADR 0001 §5.3),
//! - Grenzen gegen Dekompressionsbomben und Rekursion ([`Limits`]).
//!
//! Umsetzung (docs/adr/0010): eigener lenienter COS-Lexer für Querverweisketten, Objekte an Offsets
//! und Content-Streams; das lopdf-Objektmodell wird nach der eigenen Querverweistabelle korrigiert
//! (Hybrid-Abschnitte, falsche Offsets → [`XrefHealth`]). Zusätzlich: PDF/A-Kennung, DocMDP,
//! AcroForm-Kennzahlen, Bilddekodierung für die QR-Erkennung ([`SourcePdf::decode_image`]) und die
//! Originalschreibweise von Reals für den Writer ([`SourcePdf::real_literals`]).
//!
//! Andere Crates verwenden lopdf ausschließlich über den Re-Export [`lopdf`] dieses Crates, damit die
//! gepinnte Version (0.45.x) an genau einer Stelle festgelegt ist.
//!
//! Ownership: Paket **A** (zusammen mit `pida-pdf-incremental`).

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

pub use lopdf;
pub use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat};

mod content;
mod crypt;
mod fields;
mod filters;
mod image;
mod lexer;
mod pages;
mod text;
mod xmp;
mod xref;

pub use content::{IDENTITY, multiply as matrix_multiply};
pub use image::{DecodedImage, PixelFormat};
pub use text::{decode_text_string, encode_text_string};
pub use xmp::{PdfAIdentification, parse_pdfa_identification};

/// Fehler beim Lesen/Analysieren eines PDFs.
#[derive(Debug, thiserror::Error)]
pub enum CosError {
    /// Ein-/Ausgabefehler.
    #[error("E/A-Fehler: {0}")]
    Io(#[from] std::io::Error),
    /// Keine PDF-Datei (kein `%PDF-`-Header, keine Objekte).
    #[error("keine PDF-Datei")]
    NotAPdf,
    /// Syntaxfehler, die auch der leniente Leser nicht überbrücken kann.
    #[error("PDF nicht lesbar: {0}")]
    Parse(String),
    /// Dokument verlangt ein User-Passwort (Annahmeklasse K5, Plan §6.1).
    #[error("PDF ist mit einem Benutzerpasswort geschützt")]
    PasswordRequired,
    /// Public-Key- oder unbekannter Security-Handler (K5, Plan §6.3).
    #[error("nicht unterstützter Security-Handler: {0}")]
    UnsupportedSecurityHandler(String),
    /// Verschlüsselung ist defekt oder nicht entschlüsselbar.
    #[error("Entschlüsselung fehlgeschlagen: {0}")]
    Decryption(String),
    /// Eine Grenze aus [`Limits`] wurde überschritten.
    #[error("Grenze überschritten: {what} (Limit {limit})")]
    LimitExceeded {
        /// Welche Grenze.
        what: &'static str,
        /// Wert der Grenze.
        limit: u64,
    },
    /// Referenziertes Objekt fehlt.
    #[error("Objekt {0:?} fehlt")]
    MissingObject(ObjectId),
    /// Struktur entspricht nicht der Erwartung (z. B. Seitenbaum defekt).
    #[error("ungültige PDF-Struktur: {0}")]
    InvalidStructure(String),
    /// Dokument ohne Seiten (`Cannot sign an empty document`, docs/analyse/03 §7).
    #[error("PDF hat keine Seiten")]
    NoPages,
}

/// Ergebnis-Alias.
pub type Result<T, E = CosError> = std::result::Result<T, E>;

// ------------------------------------------------------------------------------------------------
// Geometrie
// ------------------------------------------------------------------------------------------------

/// Rechteck im PDF-User-Space (`[llx lly urx ury]`), Einheit pt.
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Rect {
    /// linke untere Ecke x
    pub llx: f32,
    /// linke untere Ecke y
    pub lly: f32,
    /// rechte obere Ecke x
    pub urx: f32,
    /// rechte obere Ecke y
    pub ury: f32,
}

impl Rect {
    /// Neues Rechteck.
    pub const fn new(llx: f32, lly: f32, urx: f32, ury: f32) -> Self {
        Self { llx, lly, urx, ury }
    }

    /// Normalisiert (kleinere Koordinate zuerst), wie ISO 32000-1 §7.9.5 verlangt.
    pub fn normalized(self) -> Self {
        Self {
            llx: self.llx.min(self.urx),
            lly: self.lly.min(self.ury),
            urx: self.llx.max(self.urx),
            ury: self.lly.max(self.ury),
        }
    }

    /// Breite (nicht normalisiert: `urx - llx`).
    pub fn width(&self) -> f32 {
        self.urx - self.llx
    }

    /// Höhe (nicht normalisiert: `ury - lly`).
    pub fn height(&self) -> f32 {
        self.ury - self.lly
    }

    /// `true` bei Fläche 0 (unsichtbare Signatur hat `[0 0 0 0]`, docs/analyse/03 §4.6).
    pub fn is_empty(&self) -> bool {
        self.width() == 0.0 || self.height() == 0.0
    }
}

/// Seitenrotation (`/Rotate`), normalisiert auf 0/90/180/270.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub enum Rotation {
    /// 0°
    #[default]
    R0,
    /// 90° im Uhrzeigersinn
    R90,
    /// 180°
    R180,
    /// 270° im Uhrzeigersinn
    R270,
}

impl Rotation {
    /// Normalisiert beliebige Vielfache von 90 (auch negative); andere Werte ergeben `None`.
    pub fn from_degrees(deg: i64) -> Option<Self> {
        if deg % 90 != 0 {
            return None;
        }
        Some(match deg.rem_euclid(360) {
            0 => Rotation::R0,
            90 => Rotation::R90,
            180 => Rotation::R180,
            _ => Rotation::R270,
        })
    }

    /// Grad (0, 90, 180, 270).
    pub fn degrees(self) -> u16 {
        match self {
            Rotation::R0 => 0,
            Rotation::R90 => 90,
            Rotation::R180 => 180,
            Rotation::R270 => 270,
        }
    }

    /// `true` bei 90/270 (Breite und Höhe vertauscht, docs/analyse/01 F-40).
    pub fn swaps_axes(self) -> bool {
        matches!(self, Rotation::R90 | Rotation::R270)
    }
}

/// Affine Matrix `[a b c d e f]` (PDF-Konvention).
pub type Matrix = [f32; 6];

/// Geometrie einer Seite (docs/analyse/04 §4.6/§4.7).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PageInfo {
    /// 0-basierter Seitenindex.
    pub index: usize,
    /// Objekt-ID des Seitendictionarys.
    pub object_id: ObjectId,
    /// `/MediaBox` (geerbt aufgelöst).
    pub media_box: Rect,
    /// `/CropBox` (geerbt aufgelöst; fehlt sie, gleich MediaBox).
    pub crop_box: Rect,
    /// `/Rotate` (geerbt aufgelöst).
    pub rotation: Rotation,
    /// `/UserUnit` (Default 1.0).
    pub user_unit: f32,
    /// `/Annots` ist ein indirektes Array (dann wird das Array-Objekt fortgeschrieben, Plan §6.2).
    pub annots_indirect: Option<ObjectId>,
}

impl PageInfo {
    /// Sichtbare Seitengröße (CropBox, bei 90/270° vertauscht) – Grundlage für Positionierung und UI
    /// (docs/analyse/04 §4.6).
    pub fn visible_size(&self) -> (f32, f32) {
        let b = self.crop_box.normalized();
        if self.rotation.swaps_axes() { (b.height(), b.width()) } else { (b.width(), b.height()) }
    }
}

// ------------------------------------------------------------------------------------------------
// Revisionen, xref, Dateiende
// ------------------------------------------------------------------------------------------------

/// Art des Querverweises einer Revision (docs/analyse/03 Phase E Schritt 8; Plan §6.1 K1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum XrefKind {
    /// klassische `xref`-Tabelle mit `trailer`
    Table,
    /// `/Type /XRef`-Stream
    Stream,
    /// Tabelle mit `/XRefStm` im Trailer (Hybrid)
    Hybrid,
}

/// Eine Revision (Original oder Increment) im Byte-Strom.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Revision {
    /// 0 = älteste Revision.
    pub index: usize,
    /// Wert hinter `startxref` dieser Revision.
    pub startxref: u64,
    /// Offset direkt nach `%%EOF` (inkl. EOL) dieser Revision.
    pub end_offset: u64,
    /// xref-Art dieser Revision.
    pub xref_kind: XrefKind,
    /// `/Prev` des Trailers.
    pub prev: Option<u64>,
    /// `/Size` des Trailers.
    pub size: u32,
}

/// Was nach dem letzten `%%EOF` steht (Plan §6.1 „Weitere Sonderfälle“).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TailKind {
    /// Nichts.
    Clean,
    /// Nur Leerraum, Zeilenumbrüche oder Nullbytes – K1, das Increment wird dahinter angehängt.
    Whitespace,
    /// Echter Fremdinhalt – K2.
    Garbage,
}

/// Dateiende-Befund.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TailInfo {
    /// Art.
    pub kind: TailKind,
    /// Offset nach dem letzten `%%EOF`.
    pub eof_offset: u64,
    /// Länge des Rests.
    pub len: u64,
}

// ------------------------------------------------------------------------------------------------
// Verschlüsselung und Rechte
// ------------------------------------------------------------------------------------------------

/// Security-Handler laut `/Filter` im Encrypt-Dictionary.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SecurityHandler {
    /// `/Standard` – unterstützt (ohne User-Passwort).
    Standard,
    /// `/Adobe.PubSec` – K5.
    PublicKey,
    /// Anderer Handler – K5.
    Other(String),
}

/// Verschlüsselungsverfahren.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CryptAlgorithm {
    /// RC4 40 Bit (R2)
    Rc4_40,
    /// RC4 128 Bit (R3/R4)
    Rc4_128,
    /// AES-128 (AESV2)
    Aes128,
    /// AES-256 (AESV3, R5/R6)
    Aes256,
    /// `/Identity`
    Identity,
    /// Unbekannt
    Unknown(String),
}

/// Benutzerrechte `/P` (ISO 32000-1 Tabelle 22).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Permissions(pub u32);

impl Permissions {
    /// Alle Rechte (unverschlüsseltes Dokument).
    pub const ALL: Permissions = Permissions(0xFFFF_FFFC);

    fn bit(self, n: u32) -> bool {
        self.0 & (1 << (n - 1)) != 0
    }
    /// Bit 3: Drucken
    pub fn can_print(self) -> bool {
        self.bit(3)
    }
    /// Bit 4: Ändern
    pub fn can_modify(self) -> bool {
        self.bit(4)
    }
    /// Bit 5: Extrahieren
    pub fn can_extract(self) -> bool {
        self.bit(5)
    }
    /// Bit 6: Annotationen/Formulare ändern
    pub fn can_modify_annotations(self) -> bool {
        self.bit(6)
    }
    /// Bit 9: Formulare ausfüllen (auch Signieren)
    pub fn can_fill_in_form(self) -> bool {
        self.bit(9)
    }
    /// Bit 10: Barrierefreiheit
    pub fn can_extract_for_accessibility(self) -> bool {
        self.bit(10)
    }
    /// Bit 11: Zusammenstellen
    pub fn can_assemble(self) -> bool {
        self.bit(11)
    }
    /// Bit 12: hochwertiges Drucken
    pub fn can_print_high_quality(self) -> bool {
        self.bit(12)
    }
}

/// Befund zur Verschlüsselung (Plan §6.3).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EncryptionInfo {
    /// Handler.
    pub handler: SecurityHandler,
    /// `/V`
    pub v: u8,
    /// `/R`
    pub r: u8,
    /// Schlüssellänge in Bit.
    pub length_bits: u16,
    /// Verfahren für Strings/Streams (`/StrF`/`/StmF` bzw. V/R).
    pub algorithm: CryptAlgorithm,
    /// `/P`
    pub permissions: Permissions,
    /// `/EncryptMetadata`
    pub encrypt_metadata: bool,
    /// Objekt-ID von `/Encrypt`, falls indirekt.
    pub encrypt_dict_id: Option<ObjectId>,
}

/// Rechte für das Signieren (docs/analyse/03 §4.13, Plan §6.1 K3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Rights {
    /// Dokument ist verschlüsselt.
    pub encrypted: bool,
    /// Wirksame Rechte wie PDFBox `getCurrentAccessPermission()`: bei unverschlüsselten Dokumenten
    /// und bei verschlüsselten, deren Owner-Passwort leer ist, [`Permissions::ALL`] (Owner-Rechte);
    /// sonst `/P` ([`EncryptionInfo::permissions`] bleibt immer der Rohwert).
    pub permissions: Permissions,
}

impl Rights {
    /// Rechteprüfung wie PDF-AS `PdfBoxUtils.checkPDFPermissions`:
    /// unverschlüsselt → erlaubt; sonst `(canModify && canModifyAnnotations) || canFillInForm`.
    pub fn may_sign(&self) -> bool {
        !self.encrypted
            || (self.permissions.can_modify() && self.permissions.can_modify_annotations())
            || self.permissions.can_fill_in_form()
    }
}

/// Ziel der Objektverschlüsselung.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CryptTarget {
    /// String-Objekt
    String,
    /// Stream-Inhalt
    Stream,
}

/// Verschlüsselt neue Objekte mit dem objektspezifischen Schlüssel des vorhandenen Handlers.
///
/// Wird von `pida-pdf-incremental` benutzt (Plan §6.3): neue Strings und Streams werden
/// verschlüsselt, `/Contents` des Signaturdictionarys **nie**. Innerer Aufbau ist Sache von Paket A.
///
/// `Debug` zeigt nur das Verfahren, nie den Dateischlüssel (ADR 0001 Regel 9).
pub struct Encryptor {
    inner: crypt::EncryptorInner,
}

impl std::fmt::Debug for Encryptor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Encryptor").field("algorithm", &self.inner.algorithm).finish_non_exhaustive()
    }
}

impl Encryptor {
    /// Verschlüsselt `data` für Objekt `id`.
    ///
    /// Bei AES wird ein zufälliger IV vorangestellt (ISO 32000-1 §7.6.2); bei `/Identity`
    /// bleiben die Daten unverändert.
    pub fn encrypt(&self, id: ObjectId, data: &[u8], target: CryptTarget) -> Result<Vec<u8>> {
        self.inner.encrypt(id, data, target)
    }

    /// Entschlüsselt `data` für Objekt `id` (Gegenstück zu [`Self::encrypt`], z. B. für Prüfungen).
    pub fn decrypt(&self, id: ObjectId, data: &[u8], target: CryptTarget) -> Result<Vec<u8>> {
        self.inner.decrypt(id, data, target)
    }

    /// Verfahren für Streams (`/StmF` bzw. V/R).
    pub fn algorithm(&self) -> &CryptAlgorithm {
        &self.inner.algorithm
    }
}

// ------------------------------------------------------------------------------------------------
// Felder und Bilder
// ------------------------------------------------------------------------------------------------

/// Widget eines Signaturfelds.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SigWidget {
    /// Objekt-ID des Widgets (gleich `SigField::field_id`, wenn Feld und Widget verschmolzen sind).
    pub id: ObjectId,
    /// Seite (über `/P` oder Suche in `/Annots`); `None`, wenn nicht auffindbar.
    pub page_index: Option<usize>,
    /// `/Rect`
    pub rect: Rect,
    /// `/F` (Annotation-Flags)
    pub annot_flags: u32,
}

/// Signaturfeld im AcroForm-Baum (docs/analyse/03 §4.4/§4.5, Plan §6.4).
///
/// `Debug` zeigt die Feldnamen nur als Länge: Namen sind Dokumentinhalt und können Personendaten
/// tragen (ADR 0001 Regel 9; `Debug` von `pades::Analysis` enthält die Felder).
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SigField {
    /// Voll qualifizierter Name (`a.b.c`), auch für hierarchische Felder (Abweichung zu PDF-AS, Register).
    pub qualified_name: String,
    /// `/T` des Blatts.
    pub partial_name: Option<String>,
    /// Objekt-ID des Felds.
    pub field_id: ObjectId,
    /// Tiefe im Feldbaum (0 = oberste Ebene von `/Fields`).
    pub depth: usize,
    /// Widgets (direkt verschmolzen oder `/Kids`).
    pub widgets: Vec<SigWidget>,
    /// `/V` vorhanden (Feld ist signiert).
    pub signed: bool,
    /// Objekt-ID des Signaturdictionarys `/V`, falls indirekt.
    pub value_id: Option<ObjectId>,
    /// `/Lock` vorhanden.
    pub has_lock: bool,
    /// `/SV` (Seed Value) vorhanden.
    pub has_seed_value: bool,
    /// `/Ff`
    pub field_flags: u32,
}

impl std::fmt::Debug for SigField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SigField")
            .field("qualified_name_len", &self.qualified_name.chars().count())
            .field("partial_name_len", &self.partial_name.as_ref().map(|n| n.chars().count()))
            .field("field_id", &self.field_id)
            .field("depth", &self.depth)
            .field("widgets", &self.widgets)
            .field("signed", &self.signed)
            .field("value_id", &self.value_id)
            .field("has_lock", &self.has_lock)
            .field("has_seed_value", &self.has_seed_value)
            .field("field_flags", &self.field_flags)
            .finish()
    }
}

/// Aufruf eines Bild-XObjects per `Do` im Seiteninhalt (für QR-Platzhalter, docs/analyse/04 §5.1).
///
/// Nur `Do` auf `/Subtype /Image` zählt; Inline-Bilder (`BI … EI`) werden wie bei PDF-AS ignoriert.
/// `pida-placement` ordnet die Einträge den PDFium-Bildobjekten (`pida_render::ImageObject`)
/// **primär über [`ImageInvocation::ctm`]** zu, `order` ist nur Tie-Breaker (ADR 0001 §8).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ImageInvocation {
    /// 0-basierter Seitenindex.
    pub page_index: usize,
    /// 0-basierte laufende Nummer unter den Bild-`Do`-Aufrufen der Seite in Content-Stream-Reihenfolge
    /// (Tiefensuche: Aufrufe in Form-XObjects – nur mit `descend_forms` – an der Stelle des
    /// Form-`Do`; mehrfach aufgerufene XObjects zählen je Aufruf).
    pub order: usize,
    /// Ressourcenname (z. B. `Im3`) – wird als `/Location` der Signatur gesetzt.
    pub resource_name: String,
    /// Objekt-ID des XObjects, falls indirekt.
    pub xobject_id: Option<ObjectId>,
    /// CTM beim `Do` im Seitenraum (Bild = Einheitsquadrat; bei Form-XObjects mit deren `/Matrix`
    /// und dem CTM des Form-`Do` verkettet).
    pub ctm: Matrix,
    /// `/Width`
    pub pixel_width: u32,
    /// `/Height`
    pub pixel_height: u32,
    /// Innerhalb eines Form-XObjects gefunden (nur mit Flag ausgewertet, Plan §6.8).
    pub inside_form_xobject: bool,
}

// ------------------------------------------------------------------------------------------------
// Grenzen
// ------------------------------------------------------------------------------------------------

/// Grenzen gegen feindliche Eingaben (Plan §7.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// Maximale Dateigröße.
    pub max_file_size: u64,
    /// Maximale Objektanzahl.
    pub max_objects: usize,
    /// Maximale dekomprimierte Größe eines Streams.
    pub max_decoded_stream: u64,
    /// Maximales Verhältnis dekomprimiert/komprimiert.
    pub max_decompression_ratio: u32,
    /// Maximale Rekursionstiefe (Seitenbaum, Feldbaum, Form-XObjects).
    pub max_depth: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_file_size: 2_000_000_000,
            max_objects: 5_000_000,
            max_decoded_stream: 512 * 1024 * 1024,
            max_decompression_ratio: 1000,
            max_depth: 64,
        }
    }
}

// ------------------------------------------------------------------------------------------------
// Zusätzliche Befunde (additiv zur Fundament-API)
// ------------------------------------------------------------------------------------------------

/// Zustand des Querverweises (Grundlage für K1/K2, Plan §6.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum XrefHealth {
    /// Kette vollständig lesbar, alle Offsets stimmen.
    Intact,
    /// Kette lesbar, aber Offsets/`/Prev` mussten korrigiert oder Objekte per Suche gefunden werden
    /// (K2). Ein normales Increment ist möglich; die Vorrevision bleibt so defekt wie zuvor.
    Repaired,
    /// Kein verwertbares `startxref`: das Objektverzeichnis wurde durch Suche rekonstruiert (K2).
    /// Ein Increment mit `/Prev` ist nicht möglich.
    Reconstructed,
}

/// DocMDP-Zertifizierung (`/Perms /DocMDP`, ISO 32000-1 §12.8.2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct DocMdp {
    /// `/TransformParams /P` (1, 2, 3; Default 2)
    pub permission: u8,
    /// Objekt-ID des zertifizierenden Signaturdictionarys, falls indirekt
    pub signature_id: Option<ObjectId>,
}

/// Kennzahlen des AcroForm-Dictionarys.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AcroFormInfo {
    /// Objekt-ID, falls indirekt (sonst direkt im Katalog eingebettet)
    pub id: Option<ObjectId>,
    /// Anzahl der Einträge in `/Fields`
    pub field_count: usize,
    /// `/SigFlags`
    pub sig_flags: u32,
    /// `/NeedAppearances true`
    pub need_appearances: bool,
    /// `/XFA` vorhanden
    pub has_xfa: bool,
    /// `/DR` vorhanden
    pub has_dr: bool,
    /// `/DA` vorhanden
    pub has_da: bool,
}

/// Lage eines Objekts laut zusammengeführtem Querverweis (neueste Revision gewinnt).
///
/// Additiv (Welle 2, `pida-pades`): Grundlage der Whitelist-Prüfung I6, die die Objekte eines
/// Increments unabhängig vom Writer aus dem Querverweis der Ausgabe bestimmt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ObjectLocation {
    /// Objekt steht an diesem **absoluten** Offset der Datei (Offset-Basis eingerechnet).
    Offset(u64),
    /// Objekt liegt in einem Object Stream.
    InStream {
        /// Objektnummer des Object Streams
        stream: u32,
        /// Index im Object Stream
        index: u32,
    },
}

/// Element eines Pfads innerhalb eines Objekts (Dictionary-Schlüssel bzw. Array-Index).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PathSegment {
    /// Wert unter einem Dictionary-Schlüssel (bei Streams: im Stream-Dictionary)
    Key(Vec<u8>),
    /// Element eines Arrays
    Index(usize),
}

/// Originalschreibweise der Gleitkommazahlen eines Objekts.
///
/// lopdf hält Reals nur als `f32`; `595.303937007874` würde beim Fortschreiben eines Objekts zu
/// `595.303955`. Prüfer wie pyHanko vergleichen Werte exakt und werten das als Änderung. Der Writer
/// verwendet deshalb für unveränderte Werte (gleicher Pfad, gleicher `f32`) den Originaltext –
/// wie PDFBox, das `COSFloat`-Text beibehält. `Debug` zeigt nur die Anzahl.
#[derive(Clone, Default, PartialEq)]
pub struct RealLiterals {
    map: std::collections::HashMap<Vec<PathSegment>, (f32, String)>,
}

impl std::fmt::Debug for RealLiterals {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RealLiterals").field("len", &self.map.len()).finish()
    }
}

impl RealLiterals {
    /// Originaltext für den Wert `value` an `path`, falls er dort unverändert steht.
    pub fn get(&self, path: &[PathSegment], value: f32) -> Option<&str> {
        let (v, text) = self.map.get(path)?;
        (v.to_bits() == value.to_bits()).then_some(text.as_str())
    }

    /// Anzahl der erfassten Reals.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Keine Reals erfasst.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

// ------------------------------------------------------------------------------------------------
// SourcePdf
// ------------------------------------------------------------------------------------------------

/// Unveränderliches Eingabe-PDF: Original-Bytes plus geparstes Objektmodell.
///
/// Die Original-Bytes bleiben unverändert und sind der Präfix jeder Ausgabe (Invariante I1,
/// Plan §3.6). Das `lopdf::Document` ist bei verschlüsselten Dateien bereits entschlüsselt
/// (`/Contents` von Signaturdictionaries bleibt roh, ISO 32000-2 §7.6.1).
///
/// `Debug` zeigt nur Kennzahlen, nie Dokumentinhalt (ADR 0001 §4 Regel 9).
pub struct SourcePdf {
    bytes: Arc<[u8]>,
    document: Document,
    revisions: Vec<Revision>,
    tail: TailInfo,
    encryption: Option<EncryptionInfo>,
    encryptor: Option<Encryptor>,
    rights: Rights,
    pages: Vec<PageInfo>,
    version: String,
    trailer: Dictionary,
    health: XrefHealth,
    offset_base: u64,
    next_number: u32,
    limits: Limits,
    xref: BTreeMap<u32, xref::Entry>,
    /// tatsächlicher Offset (relativ zur Basis) des neuesten Querverweisabschnitts
    prev_offset: u64,
}

impl std::fmt::Debug for SourcePdf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourcePdf")
            .field("len", &self.bytes.len())
            .field("revisions", &self.revisions.len())
            .field("tail", &self.tail)
            .field("encryption", &self.encryption)
            .field("rights", &self.rights)
            .field("pages", &self.pages.len())
            .field("health", &self.health)
            .finish_non_exhaustive()
    }
}

fn map_lopdf(e: lopdf::Error) -> CosError {
    match e {
        lopdf::Error::Decryption(d) => CosError::Decryption(d.to_string()),
        lopdf::Error::InvalidPassword => CosError::PasswordRequired,
        lopdf::Error::UnsupportedSecurityHandler(h) => {
            CosError::UnsupportedSecurityHandler(String::from_utf8_lossy(&h).into_owned())
        }
        lopdf::Error::Decompress(lopdf::DecompressError::MemoryLimitExceeded { limit }) => {
            CosError::LimitExceeded { what: "dekomprimierte Stream-Größe", limit: limit as u64 }
        }
        other => CosError::Parse(other.to_string()),
    }
}

fn header_version(data: &[u8], header_offset: usize) -> String {
    let rest = &data[header_offset + 5..data.len().min(header_offset + 16)];
    let v: String = rest.iter().take_while(|b| b.is_ascii_digit() || **b == b'.').map(|&b| char::from(b)).collect();
    if v.is_empty() { "1.0".into() } else { v }
}

fn version_key(v: &str) -> (u32, u32) {
    let mut it = v.split('.');
    let major = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (major, minor)
}

/// Filter für lopdfs Ladevorgang: Streams mit unzulässigen Predictor-Parametern werden verworfen,
/// bevor lopdf sie (als Object Stream) ungeprüft dekodiert (`filters::MAX_PREDICTOR_ROW`).
///
/// lopdf 0.45 wertet den Rückgabewert nur bei Objekten **aus** Object Streams aus (dort nie
/// Streams, ISO 32000-1 §7.5.7); bei Objekten an Offsets zählt nur `None` = verwerfen. Für Streams
/// genügt deshalb ein Platzhalter, ohne den Inhalt zu kopieren.
fn lopdf_load_filter(id: ObjectId, obj: &mut Object) -> Option<(ObjectId, Object)> {
    match obj {
        Object::Stream(s) if !filters::stream_params_sane(&s.dict) => None,
        Object::Stream(_) => Some((id, Object::Null)),
        other => Some((id, other.clone())),
    }
}

/// Prüft vor dem Laden die Dictionaries aller Object-Stream-Container der Querverweistabelle
/// (lopdf dekodiert sie bei verschlüsselten Dokumenten ohne Filteraufruf).
fn check_container_params(data: &[u8], merged: &BTreeMap<u32, xref::Entry>, base: u64) -> Result<()> {
    let containers: std::collections::BTreeSet<u32> = merged
        .values()
        .filter_map(|e| match e {
            xref::Entry::Compressed { stream, .. } => Some(*stream),
            _ => None,
        })
        .collect();
    for c in containers {
        let Some(xref::Entry::InUse { offset, .. }) = merged.get(&c) else { continue };
        let Some(abs) = offset.checked_add(base).and_then(|a| usize::try_from(a).ok()) else { continue };
        let Some((_, after)) = lexer::object_header_at(data, abs) else { continue };
        if let Some(Object::Dictionary(d)) = lexer::Lexer::new(data, after).parse_object(true)
            && !filters::stream_params_sane(&d)
        {
            return Err(filters::insane_predictor());
        }
    }
    Ok(())
}

/// Lädt fehlende bzw. von lopdf falsch zugeordnete Objekte nach der eigenen Querverweistabelle nach.
/// Liefert `true`, wenn Offsets nicht stimmten und Objekte per Suche gefunden werden mussten.
#[allow(clippy::too_many_arguments)]
fn fixup_objects(
    doc: &mut Document,
    data: &[u8],
    merged: &BTreeMap<u32, xref::Entry>,
    base: u64,
    encrypt_id: Option<ObjectId>,
    decryptor: Option<&crypt::Decryptor>,
    limits: &Limits,
    index: &lexer::EndstreamIndex,
) -> bool {
    use lopdf::xref::XrefEntry;
    let mut repaired = false;
    let mut scan: Option<BTreeMap<ObjectId, usize>> = None;
    let mut objstm_cache: BTreeMap<u32, Option<lopdf::ObjectStream>> = BTreeMap::new();
    let max_stream = usize::try_from(limits.max_decoded_stream).unwrap_or(usize::MAX);
    // Hat lopdf mit anderen Crypt-Filtern entschlüsselt, ist keine seiner Fassungen verwendbar.
    let clobbered =
        if decryptor.is_some_and(crypt::Decryptor::lopdf_deviates) { Clobbered::All } else { clobbered_ids(doc, data) };

    // Zwei Durchläufe: erst Objekte an Offsets (darunter Object-Stream-Container), dann komprimierte.
    for (&num, entry) in merged
        .iter()
        .filter(|(_, e)| matches!(e, xref::Entry::InUse { .. }))
        .chain(merged.iter().filter(|(_, e)| matches!(e, xref::Entry::Compressed { .. })))
    {
        match *entry {
            xref::Entry::InUse { offset, generation } => {
                let id = (num, generation);
                if Some(id) == encrypt_id {
                    continue; // lopdf hält das Encrypt-Dictionary bewusst nicht im Objektmodell
                }
                let lopdf_ok = matches!(
                    doc.reference_table.get(num),
                    Some(XrefEntry::Normal { offset: o, generation: g }) if u64::from(*o) == offset && *g == generation
                );
                if lopdf_ok && !clobbered.contains(id) && doc.objects.contains_key(&id) {
                    continue;
                }
                let Some(abs) = offset.checked_add(base).and_then(|a| usize::try_from(a).ok()) else { continue };
                let header_ok = lexer::object_header_at(data, abs).is_some_and(|(hid, _)| hid == id);
                let pos = if header_ok {
                    Some(abs)
                } else {
                    repaired = true;
                    let map = scan.get_or_insert_with(|| scan_object_headers(data));
                    map.get(&id).copied()
                };
                let Some(pos) = pos else { continue };
                let resolve_len = |rid: ObjectId| doc.objects.get(&rid).and_then(|o| o.as_i64().ok());
                let Some(io) = lexer::parse_indirect_object(data, pos, &resolve_len, index) else { continue };
                let mut obj = io.object;
                if let Some(d) = decryptor {
                    let _ = d.decrypt_object(id, &mut obj);
                }
                doc.objects.insert(id, obj);
            }
            xref::Entry::Compressed { stream, .. } => {
                let id = (num, 0);
                let lopdf_ok = matches!(
                    doc.reference_table.get(num),
                    Some(XrefEntry::Compressed { container, .. }) if *container == stream
                );
                if lopdf_ok && !clobbered.contains(id) && doc.objects.contains_key(&id) {
                    continue;
                }
                let os = objstm_cache.entry(stream).or_insert_with(|| {
                    doc.objects
                        .get(&(stream, 0))
                        .and_then(|o| o.as_stream().ok())
                        .filter(|s| filters::stream_params_sane(&s.dict))
                        .and_then(|s| lopdf::ObjectStream::new_with_limit(s, Some(max_stream)).ok())
                });
                match os.as_ref().and_then(|os| os.objects.get(&id)) {
                    Some(obj) => {
                        doc.objects.insert(id, obj.clone());
                    }
                    // Container ist kein lesbarer Object Stream bzw. enthält das Objekt nicht: der
                    // Eintrag ist falsch (Mutationstest: Container = Seite) – eine Fassung von
                    // anderswo (lopdf) stammt nicht aus dem Querverweis ⇒ K2 (repariert)
                    None => repaired = true,
                }
            }
            xref::Entry::Free => {}
        }
    }
    repaired
}

/// Objekte, deren Fassung im lopdf-Objektmodell nicht sicher vom eigenen Querverweiseintrag stammt.
///
/// lopdf 0.45 legt jedes Objekt unter der Kennung seines **Objektkopfs** ab, nicht unter der Nummer
/// des Querverweiseintrags, und spätere Einträge überschreiben frühere. Zeigt ein Eintrag (z. B.
/// nach einem verschobenen Unterabschnitt `1 2` statt `0 2`) auf den Kopf eines anderen Objekts,
/// ersetzt lopdf dessen gültige Fassung durch die alte – obwohl die Tabelle für dieses Objekt
/// stimmt. Ohne diese Prüfung sähe der Re-Parse eines Increments den alten Katalog (I5).
enum Clobbered {
    /// diese Kennungen können überschrieben worden sein
    Ids(std::collections::HashSet<ObjectId>),
    /// ein Eintrag zeigt auf einen Kopf, den der eigene Lexer nicht liest: alle Objekte nachladen
    All,
}

impl Clobbered {
    fn contains(&self, id: ObjectId) -> bool {
        match self {
            Clobbered::Ids(ids) => ids.contains(&id),
            Clobbered::All => true,
        }
    }
}

fn clobbered_ids(doc: &Document, data: &[u8]) -> Clobbered {
    use lopdf::xref::XrefEntry;
    let mut ids = std::collections::HashSet::new();
    for (&num, entry) in &doc.reference_table.entries {
        let XrefEntry::Normal { offset, generation } = *entry else { continue };
        let Ok(pos) = usize::try_from(offset) else { continue };
        if pos >= data.len() {
            continue; // lopdf liest dort nichts
        }
        match lexer::object_header_at(data, pos) {
            Some((hid, _)) if hid == (num, generation) => {}
            Some((hid, _)) => {
                ids.insert(hid);
            }
            // lopdf überspringt vor dem Kopf auch Kommentare; was der eigene Lexer nicht als Kopf
            // erkennt, kann lopdf trotzdem unter einer unbekannten Kennung geladen haben.
            None if lopdf_may_parse_header(data, pos) => return Clobbered::All,
            None => {}
        }
    }
    Clobbered::Ids(ids)
}

/// Grobe Vorprüfung: Beginnt an `pos` (nach Leerraum und Kommentaren) eine Ziffer, kann lopdf dort
/// einen Objektkopf gelesen haben.
fn lopdf_may_parse_header(data: &[u8], pos: usize) -> bool {
    let mut p = pos;
    loop {
        while p < data.len() && lexer::is_whitespace(data[p]) {
            p += 1;
        }
        if data.get(p) == Some(&b'%') {
            while p < data.len() && !matches!(data[p], b'\r' | b'\n') {
                p += 1;
            }
            continue;
        }
        return data.get(p).is_some_and(u8::is_ascii_digit);
    }
}

/// Alle Objektköpfe `n g obj` am Zeilenanfang (spätere Vorkommen gewinnen).
fn scan_object_headers(data: &[u8]) -> BTreeMap<ObjectId, usize> {
    let mut map = BTreeMap::new();
    let mut i = 0;
    while i < data.len() {
        let line_start = i == 0 || matches!(data[i - 1], b'\n' | b'\r');
        if line_start
            && data[i].is_ascii_digit()
            && let Some((id, after)) = lexer::object_header_at(data, i)
        {
            map.insert(id, i);
            i = after;
            continue;
        }
        i += 1;
    }
    map
}

/// Ersetzt in verschlüsselten Dokumenten `/Contents` von Signaturdictionaries durch den Rohwert
/// (lopdf entschlüsselt ihn fälschlich mit) und entschlüsselt die übrigen Strings einzeln.
fn fixup_signature_dicts(
    doc: &mut Document,
    data: &[u8],
    merged: &BTreeMap<u32, xref::Entry>,
    base: u64,
    decryptor: Option<&crypt::Decryptor>,
    index: &lexer::EndstreamIndex,
) {
    let Some(decryptor) = decryptor else { return };
    let candidates: Vec<ObjectId> = doc
        .objects
        .iter()
        .filter(|(_, o)| matches!(o, Object::Dictionary(d) if crypt::is_signature_dict(d)))
        .map(|(id, _)| *id)
        .collect();
    for id in candidates {
        let Some(xref::Entry::InUse { offset, generation }) = merged.get(&id.0) else { continue };
        if *generation != id.1 {
            continue;
        }
        let Some(abs) = offset.checked_add(base).and_then(|a| usize::try_from(a).ok()) else { continue };
        let Some(io) = lexer::parse_indirect_object(data, abs, &|_| None, index) else { continue };
        if io.id != id {
            continue;
        }
        if let Object::Dictionary(raw) = io.object {
            let fixed = crypt::decrypt_signature_dict(decryptor, id, &raw);
            doc.objects.insert(id, Object::Dictionary(fixed));
        }
    }
}

impl SourcePdf {
    /// Liest ein PDF aus Bytes (leniente Variante, Plan §6.1 K1/K2).
    ///
    /// Fehler: [`CosError::PasswordRequired`], [`CosError::UnsupportedSecurityHandler`],
    /// [`CosError::NoPages`] usw. – `pida-pades::analyze` bildet sie auf K5 ab.
    pub fn open(bytes: impl Into<Arc<[u8]>>, limits: &Limits) -> Result<SourcePdf> {
        let bytes: Arc<[u8]> = bytes.into();
        if bytes.len() as u64 > limits.max_file_size {
            return Err(CosError::LimitExceeded { what: "Dateigröße", limit: limits.max_file_size });
        }
        let data: &[u8] = &bytes;
        let head = &data[..data.len().min(1024)];
        let header_offset = lexer::find(head, b"%PDF-", 0).ok_or(CosError::NotAPdf)?;
        let mut version = header_version(data, header_offset);

        let index = lexer::EndstreamIndex::new();
        let structure = xref::analyze(data, header_offset, limits, &index)?;
        let merged = structure.chain.merged();
        let base = structure.base;

        // Verschlüsselung vorab prüfen (Handler aus den Rohbytes; lopdf entfernt /Encrypt).
        let raw_trailer = structure.chain.effective_trailer();
        let mut pre_encryption: Option<(EncryptionInfo, bool)> = None;
        if let Some(t) = &raw_trailer
            && let Ok(enc) = t.get(b"Encrypt")
        {
            let (dict, enc_id) = match enc {
                Object::Dictionary(d) => (Some(d.clone()), None),
                Object::Reference(id) => {
                    let dict = match merged.get(&id.0) {
                        Some(xref::Entry::InUse { offset, .. }) => offset
                            .checked_add(base)
                            .and_then(|abs| usize::try_from(abs).ok())
                            .and_then(|abs| lexer::parse_indirect_object(data, abs, &|_| None, &index))
                            .and_then(|io| match io.object {
                                Object::Dictionary(d) => Some(d),
                                _ => None,
                            }),
                        _ => None,
                    };
                    (dict, Some(*id))
                }
                _ => (None, None),
            };
            let dict = dict.ok_or_else(|| CosError::Decryption("Encrypt-Dictionary nicht lesbar".into()))?;
            let info = crypt::encryption_info(&dict, enc_id);
            match &info.handler {
                SecurityHandler::Standard => {}
                SecurityHandler::PublicKey => return Err(CosError::UnsupportedSecurityHandler("Adobe.PubSec".into())),
                SecurityHandler::Other(n) => return Err(CosError::UnsupportedSecurityHandler(n.clone())),
            }
            let owner = crypt::empty_password_is_owner(&dict, t);
            pre_encryption = Some((info, owner));
        }

        if !structure.reconstructed {
            check_container_params(data, &merged, base)?;
        }
        let load = &data[..structure.load_end.min(data.len())];
        let options = lopdf::LoadOptions {
            password: None,
            filter: Some(lopdf_load_filter),
            strict: false,
            max_decompressed_size: Some(usize::try_from(limits.max_decoded_stream).unwrap_or(usize::MAX)),
        };
        let mut document = Document::load_mem_with_options(load, options).map_err(map_lopdf)?;
        let lopdf_reconstructed = document.xref_start == 0 && !structure.reconstructed;

        let mut owner_rights = false;
        let decryptor = document.encryption_state.clone().map(crypt::Decryptor::new).transpose()?;
        if decryptor.as_ref().is_some_and(crypt::Decryptor::lopdf_deviates) && structure.reconstructed {
            // Ohne eigene Querverweistabelle lässt sich lopdfs Entschlüsselung nicht ersetzen.
            return Err(CosError::Decryption("Crypt-Filter bei rekonstruiertem Querverweis nicht lesbar".into()));
        }
        let (encryption, encryptor) = match (pre_encryption, &decryptor) {
            (Some((info, owner)), Some(dec)) => {
                let enc = Encryptor { inner: crypt::EncryptorInner::new(dec, info.algorithm.clone()) };
                owner_rights = owner;
                (Some(info), Some(enc))
            }
            (Some((info, _)), None) => {
                return Err(if crypt::standard_supported(&info) {
                    CosError::PasswordRequired
                } else {
                    CosError::UnsupportedSecurityHandler(format!("Standard V{} R{}", info.v, info.r))
                });
            }
            // Rekonstruierter Querverweis: Befund aus lopdfs Zustand.
            (None, Some(dec)) => {
                let info = crypt::info_from_state(dec.state());
                let enc = Encryptor { inner: crypt::EncryptorInner::new(dec, info.algorithm.clone()) };
                (Some(info), Some(enc))
            }
            (None, None) if document.trailer.has(b"Encrypt") => return Err(CosError::PasswordRequired),
            (None, None) => (None, None),
        };

        let mut repaired = structure.chain.repaired || lopdf_reconstructed;
        if !structure.reconstructed {
            let encrypt_id = encryption.as_ref().and_then(|e| e.encrypt_dict_id);
            repaired |=
                fixup_objects(&mut document, data, &merged, base, encrypt_id, decryptor.as_ref(), limits, &index);
            fixup_signature_dicts(&mut document, data, &merged, base, decryptor.as_ref(), &index);
        }
        if document.objects.len() > limits.max_objects {
            return Err(CosError::LimitExceeded { what: "Objektanzahl", limit: limits.max_objects as u64 });
        }
        // Der Dateischlüssel lebt nur im `Encryptor` (eigene Debug-Implementierung); lopdfs
        // `EncryptionState` leitet `Debug` ab und darf über `document()` nicht sichtbar sein (Regel 9).
        document.encryption_state = None;
        let health = if structure.reconstructed {
            XrefHealth::Reconstructed
        } else if repaired {
            XrefHealth::Repaired
        } else {
            XrefHealth::Intact
        };

        // /Prev eines neuen Increments: korrigierter Offset des neuesten Abschnitts (bei K2 kann der
        // deklarierte startxref-Wert um einige Bytes daneben liegen).
        let prev_offset = structure
            .chain
            .sections
            .first()
            .map(|s| s.offset.saturating_sub(base))
            .unwrap_or_else(|| structure.revisions.last().map_or(0, |r| r.startxref));

        let trailer = match raw_trailer {
            Some(t) => t,
            None => document.trailer.clone(),
        };
        let catalog_id = match trailer.get(b"Root") {
            Ok(Object::Reference(id)) => *id,
            _ => return Err(CosError::InvalidStructure("Trailer ohne /Root".into())),
        };
        let catalog = match document.get_object(catalog_id) {
            Ok(Object::Dictionary(d)) => d,
            _ => return Err(CosError::MissingObject(catalog_id)),
        };
        if let Ok(Object::Name(v)) = catalog.get(b"Version")
            && let Ok(v) = std::str::from_utf8(v)
            && version_key(v) > version_key(&version)
        {
            version = v.to_string();
        }
        let pages = pages::collect_pages(&document, catalog, limits)?;
        if pages.is_empty() {
            return Err(CosError::NoPages);
        }

        // Wie PDFBox: Ist das leere Passwort auch das Owner-Passwort, gelten Owner-Rechte statt /P.
        let rights = Rights {
            encrypted: encryption.is_some(),
            permissions: match &encryption {
                Some(e) if !owner_rights => e.permissions,
                _ => Permissions::ALL,
            },
        };
        let size =
            trailer.get(b"Size").ok().and_then(|o| o.as_i64().ok()).unwrap_or(0).clamp(0, i64::from(u32::MAX)) as u32;
        let max_merged = merged.keys().next_back().map_or(0, |n| n.saturating_add(1));
        let max_loaded = document.objects.keys().next_back().map_or(0, |id| id.0.saturating_add(1));
        let next_number = size.max(max_merged).max(max_loaded).max(document.max_id.saturating_add(1)).max(1);

        Ok(SourcePdf {
            bytes: Arc::clone(&bytes),
            document,
            revisions: structure.revisions,
            tail: structure.tail,
            encryption,
            encryptor,
            rights,
            pages,
            version,
            trailer,
            health,
            offset_base: base,
            next_number,
            limits: limits.clone(),
            xref: merged,
            prev_offset,
        })
    }

    /// Liest eine Datei (Bytes vollständig oder per Memory-Map, Plan §6.2 „Speicher“).
    pub fn open_file(path: &Path, limits: &Limits) -> Result<SourcePdf> {
        let len = pida_fs::metadata(path)?.len();
        if len > limits.max_file_size {
            return Err(CosError::LimitExceeded { what: "Dateigröße", limit: limits.max_file_size });
        }
        let bytes = pida_fs::read(path)?;
        SourcePdf::open(bytes, limits)
    }

    /// Original-Bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Original-Bytes als geteilter Puffer (für Renderer und Ausgabe ohne Kopie).
    pub fn shared_bytes(&self) -> Arc<[u8]> {
        Arc::clone(&self.bytes)
    }

    /// Länge der Originaldatei.
    pub fn len(&self) -> u64 {
        self.bytes.len() as u64
    }

    /// `true` bei leerer Datei (praktisch nie, `open` lehnt sie ab).
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Geparstes (ggf. entschlüsseltes) Objektmodell.
    pub fn document(&self) -> &Document {
        &self.document
    }

    /// Revisionen, älteste zuerst.
    pub fn revisions(&self) -> &[Revision] {
        &self.revisions
    }

    /// Letzte Revision (bestimmt das Format des neuen Increments, Plan §6.1 K1).
    pub fn last_revision(&self) -> &Revision {
        self.revisions.last().expect("open() garantiert mindestens eine Revision")
    }

    /// xref-Art der letzten Revision.
    pub fn xref_kind(&self) -> XrefKind {
        self.last_revision().xref_kind
    }

    /// Dateiende-Befund.
    pub fn tail(&self) -> TailInfo {
        self.tail
    }

    /// Verschlüsselung, falls vorhanden.
    pub fn encryption(&self) -> Option<&EncryptionInfo> {
        self.encryption.as_ref()
    }

    /// Verschlüsseler für neue Objekte, falls das Dokument verschlüsselt ist.
    pub fn encryptor(&self) -> Option<&Encryptor> {
        self.encryptor.as_ref()
    }

    /// Rechte.
    pub fn rights(&self) -> Rights {
        self.rights
    }

    /// Seiten in Dokumentreihenfolge.
    pub fn pages(&self) -> &[PageInfo] {
        &self.pages
    }

    /// Anzahl Seiten.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// PDF-Version aus dem Header (bzw. `/Version` im Katalog, falls höher) – `iui.pdfVersion`.
    pub fn pdf_version(&self) -> &str {
        &self.version
    }

    /// Trailer der letzten Revision.
    ///
    /// Roh aus der Datei (nicht entschlüsselt), inklusive `/Encrypt`, `/Prev` und `/XRefStm`;
    /// bei xref-Streams ohne Strukturschlüssel (`/Type`, `/W`, `/Index`, `/Filter` …). Fehlende
    /// `/Root`, `/Info`, `/ID`, `/Encrypt` werden aus älteren Trailern ergänzt.
    pub fn trailer(&self) -> &Dictionary {
        &self.trailer
    }

    /// Objekt-ID des Katalogs (`/Root`).
    pub fn catalog_id(&self) -> Result<ObjectId> {
        match self.trailer.get(b"Root") {
            Ok(Object::Reference(id)) => Ok(*id),
            _ => Err(CosError::InvalidStructure("Trailer ohne /Root".into())),
        }
    }

    /// Objekt-ID von `/Info`, falls vorhanden.
    pub fn info_id(&self) -> Option<ObjectId> {
        match self.trailer.get(b"Info") {
            Ok(Object::Reference(id)) => Some(*id),
            _ => None,
        }
    }

    /// `/ID` des Trailers (`[ID0, ID1]`), falls vorhanden.
    pub fn trailer_id(&self) -> Option<[Vec<u8>; 2]> {
        let arr = self.trailer.get(b"ID").ok()?.as_array().ok()?;
        match arr.as_slice() {
            [Object::String(a, _), Object::String(b, _), ..] => Some([a.clone(), b.clone()]),
            _ => None,
        }
    }

    /// Nächste freie Objektnummer (`/Size` der letzten Revision bzw. höchste Nummer + 1).
    pub fn next_object_number(&self) -> u32 {
        self.next_number
    }

    /// Objekt holen.
    pub fn get_object(&self, id: ObjectId) -> Result<&Object> {
        self.document.get_object(id).map_err(|_| CosError::MissingObject(id))
    }

    /// Referenzen bis zum Zielobjekt auflösen (Tiefe begrenzt durch [`Limits::max_depth`]).
    pub fn resolve<'a>(&'a self, obj: &'a Object) -> Result<&'a Object> {
        let mut o = obj;
        for _ in 0..=self.limits.max_depth {
            match o {
                Object::Reference(id) => o = self.get_object(*id)?,
                _ => return Ok(o),
            }
        }
        Err(CosError::LimitExceeded { what: "Referenzkette", limit: u64::from(self.limits.max_depth) })
    }

    /// Dekodierter Stream-Inhalt mit Grenzen.
    pub fn decoded_stream(&self, id: ObjectId) -> Result<Vec<u8>> {
        let stream = self
            .get_object(id)?
            .as_stream()
            .map_err(|_| CosError::InvalidStructure(format!("Objekt {} {} ist kein Stream", id.0, id.1)))?;
        Ok(filters::decode_stream(&self.document, stream, &self.limits, false)?.data)
    }

    /// XMP-Metadaten des Katalogs (`/Metadata`), dekodiert – für PDF/A-Erkennung (Plan §6.9).
    pub fn xmp_metadata(&self) -> Result<Option<Vec<u8>>> {
        let catalog = self.catalog()?;
        match catalog.get(b"Metadata") {
            Ok(Object::Reference(id)) => match self.get_object(*id) {
                Ok(Object::Stream(_)) => Ok(Some(self.decoded_stream(*id)?)),
                _ => Ok(None),
            },
            Ok(Object::Stream(s)) => Ok(Some(filters::decode_stream(&self.document, s, &self.limits, false)?.data)),
            _ => Ok(None),
        }
    }

    /// Alle Signaturfelder (signiert und leer), inklusive `/Kids`-Widgets und hierarchischer Namen.
    pub fn signature_fields(&self) -> Result<Vec<SigField>> {
        fields::signature_fields(&self.document, self.catalog()?, &self.pages, &self.limits)
    }

    /// Alle `Do`-Aufrufe von Bild-XObjects auf einer Seite mit CTM (docs/analyse/04 §5.1).
    /// Form-XObjects werden nur mit `descend_forms = true` betreten.
    pub fn image_invocations(&self, page_index: usize, descend_forms: bool) -> Result<Vec<ImageInvocation>> {
        let page = self
            .pages
            .get(page_index)
            .ok_or_else(|| CosError::InvalidStructure(format!("Seite {page_index} existiert nicht")))?;
        let doc = &self.document;
        let resources = match pages::inherited_attr(doc, page.object_id, b"Resources", &self.limits) {
            Some(Object::Dictionary(d)) => Some(d),
            _ => None,
        };
        let content = self.page_content(page.object_id)?;
        let decode = |s: &Stream| filters::decode_stream(doc, s, &self.limits, false).map(|d| d.data);
        let mut it = content::Interpreter::new(doc, &self.limits, page_index, descend_forms, &decode);
        it.run(&content, resources, IDENTITY, false, 0)?;
        Ok(it.out)
    }

    /// Dekodierter, zusammengefügter Seiteninhalt (`/Contents` als Stream oder Array).
    /// Nicht dekodierbare Teilstreams werden ausgelassen; Grenzverletzungen sind Fehler. Die
    /// Gesamtlänge ist durch [`Limits::max_decoded_stream`] begrenzt (ein Array kann denselben
    /// großen Stream beliebig oft referenzieren).
    pub fn page_content(&self, page_id: ObjectId) -> Result<Vec<u8>> {
        let page = self
            .get_object(page_id)?
            .as_dict()
            .map_err(|_| CosError::InvalidStructure("Seite ist kein Dictionary".into()))?;
        let max = self.limits.max_decoded_stream;
        let mut out = Vec::new();
        let mut push = |s: &Stream| -> Result<()> {
            match filters::decode_stream(&self.document, s, &self.limits, false) {
                Ok(d) => {
                    if (out.len() as u64).saturating_add(d.data.len() as u64 + 1) > max {
                        return Err(CosError::LimitExceeded { what: "Länge des Seiteninhalts", limit: max });
                    }
                    out.extend_from_slice(&d.data);
                    out.push(b'\n');
                    Ok(())
                }
                Err(e @ CosError::LimitExceeded { .. }) => Err(e),
                Err(_) => Ok(()),
            }
        };
        match page.get(b"Contents").ok().map(|o| self.resolve(o)).transpose()? {
            Some(Object::Stream(s)) => push(s)?,
            Some(Object::Array(a)) => {
                for o in a {
                    if let Ok(Object::Stream(s)) = self.resolve(o) {
                        push(s)?;
                    }
                }
            }
            _ => {}
        }
        Ok(out)
    }

    // --------------------------------------------------------------------------------------------
    // Additive Erweiterungen (Paket A)
    // --------------------------------------------------------------------------------------------

    /// Zustand des Querverweises (K1/K2).
    pub fn xref_health(&self) -> XrefHealth {
        self.health
    }

    /// Basis der xref-Offsets: 0 (Normalfall) oder Offset des `%PDF-`-Headers, wenn der Datei
    /// Fremdinhalt vorangestellt ist und die Offsets relativ zum Header stehen. Der Writer
    /// schreibt neue Offsets in derselben Konvention.
    pub fn xref_offset_base(&self) -> u64 {
        self.offset_base
    }

    /// Wert für `/Prev` eines neuen Increments: Offset des neuesten Querverweisabschnitts in der
    /// Konvention von [`Self::xref_offset_base`]. Gleich `last_revision().startxref`, außer wenn
    /// `startxref` korrigiert werden musste ([`XrefHealth::Repaired`]).
    pub fn prev_offset(&self) -> u64 {
        self.prev_offset
    }

    /// Katalog-Dictionary.
    pub fn catalog(&self) -> Result<&Dictionary> {
        let id = self.catalog_id()?;
        self.get_object(id)?.as_dict().map_err(|_| CosError::InvalidStructure("Katalog ist kein Dictionary".into()))
    }

    /// PDF/A-Kennung aus den XMP-Metadaten des Katalogs.
    pub fn pdfa_identification(&self) -> Result<Option<PdfAIdentification>> {
        Ok(self.xmp_metadata()?.as_deref().and_then(parse_pdfa_identification))
    }

    /// DocMDP-Zertifizierung (`/Perms /DocMDP` → `/Reference` mit `/TransformMethod /DocMDP`).
    pub fn doc_mdp(&self) -> Result<Option<DocMdp>> {
        let catalog = self.catalog()?;
        let Some(Object::Dictionary(perms)) = catalog.get(b"Perms").ok().map(|o| self.resolve(o)).transpose()? else {
            return Ok(None);
        };
        let Ok(sig_ref) = perms.get(b"DocMDP") else { return Ok(None) };
        let signature_id = match sig_ref {
            Object::Reference(id) => Some(*id),
            _ => None,
        };
        let Ok(Object::Dictionary(sig)) = self.resolve(sig_ref) else { return Ok(None) };
        let mut permission = 2u8;
        if let Some(Object::Array(refs)) = sig.get(b"Reference").ok().map(|o| self.resolve(o)).transpose()? {
            for r in refs {
                let Ok(Object::Dictionary(sr)) = self.resolve(r) else { continue };
                if sr.get(b"TransformMethod").ok().and_then(|o| o.as_name().ok()) != Some(b"DocMDP") {
                    continue;
                }
                if let Some(Object::Dictionary(tp)) =
                    sr.get(b"TransformParams").ok().map(|o| self.resolve(o)).transpose()?
                    && let Ok(p) = tp.get(b"P").map(|o| self.resolve(o))
                    && let Ok(Object::Integer(p)) = p
                {
                    permission = (*p).clamp(1, 3) as u8;
                }
            }
        }
        Ok(Some(DocMdp { permission, signature_id }))
    }

    /// `/Perms /UR3` bzw. `/UR` (Usage Rights) vorhanden.
    pub fn has_usage_rights(&self) -> bool {
        let Ok(catalog) = self.catalog() else { return false };
        match catalog.get(b"Perms").ok().map(|o| self.resolve(o)) {
            Some(Ok(Object::Dictionary(p))) => p.has(b"UR3") || p.has(b"UR"),
            _ => false,
        }
    }

    /// Katalog `/NeedsRendering true` (dynamisches XFA).
    pub fn needs_rendering(&self) -> bool {
        self.catalog().ok().and_then(|c| c.get(b"NeedsRendering").ok()).and_then(|o| o.as_bool().ok()).unwrap_or(false)
    }

    /// `/StructTreeRoot` mit Kindern (`/K`) vorhanden (PDF/UA-Ergänzung, docs/analyse/03 §4.9).
    pub fn is_tagged(&self) -> bool {
        let Ok(catalog) = self.catalog() else { return false };
        match catalog.get(b"StructTreeRoot").ok().map(|o| self.resolve(o)) {
            Some(Ok(Object::Dictionary(st))) => match st.get(b"K").ok().map(|o| self.resolve(o)) {
                Some(Ok(Object::Array(a))) => !a.is_empty(),
                Some(Ok(Object::Null)) | None => false,
                Some(Ok(_)) => true,
                Some(Err(_)) => false,
            },
            _ => false,
        }
    }

    /// Kennzahlen des AcroForm-Dictionarys, falls vorhanden.
    pub fn acroform_info(&self) -> Result<Option<AcroFormInfo>> {
        let catalog = self.catalog()?;
        let Ok(acro_obj) = catalog.get(b"AcroForm") else { return Ok(None) };
        let id = match acro_obj {
            Object::Reference(id) => Some(*id),
            _ => None,
        };
        let Ok(Object::Dictionary(acro)) = self.resolve(acro_obj) else { return Ok(None) };
        let field_count = match acro.get(b"Fields").ok().map(|o| self.resolve(o)) {
            Some(Ok(Object::Array(a))) => a.len(),
            _ => 0,
        };
        let int = |k: &[u8]| match acro.get(k).ok().map(|o| self.resolve(o)) {
            Some(Ok(Object::Integer(i))) => Some(*i),
            _ => None,
        };
        let need_appearances =
            matches!(acro.get(b"NeedAppearances").ok().map(|o| self.resolve(o)), Some(Ok(Object::Boolean(true))));
        Ok(Some(AcroFormInfo {
            id,
            field_count,
            sig_flags: int(b"SigFlags").map(|v| (v & 0xFFFF_FFFF) as u32).unwrap_or(0),
            need_appearances,
            has_xfa: acro.has(b"XFA"),
            has_dr: acro.has(b"DR"),
            has_da: acro.has(b"DA"),
        }))
    }

    /// Dekodiert ein Bild-XObject (für die QR-Erkennung; inkl. `/SMask` als Alpha).
    pub fn decode_image(&self, id: ObjectId) -> Result<DecodedImage> {
        let stream = self
            .get_object(id)?
            .as_stream()
            .map_err(|_| CosError::InvalidStructure(format!("Objekt {} {} ist kein Stream", id.0, id.1)))?;
        self.decode_image_stream(stream)
    }

    /// Dekodiert ein Bild aus einem (z. B. direkt eingebetteten) Bild-Stream.
    pub fn decode_image_stream(&self, stream: &Stream) -> Result<DecodedImage> {
        image::decode_image(&self.document, stream, &self.limits, true)
    }

    /// Grenzen, mit denen das Dokument geöffnet wurde.
    pub fn limits(&self) -> &Limits {
        &self.limits
    }

    /// Lage aller benutzten Objekte laut zusammengeführtem Querverweis (neueste Revision gewinnt),
    /// nach Objektnummer sortiert; freie Einträge fehlen. Bei Object Streams hat die ID immer
    /// Generation 0.
    ///
    /// Additiv (Welle 2): `pida-pades` bestimmt damit die Objekte eines Increments (Invariante
    /// I6) – Objekte mit Offset ≥ Länge des Originals bzw. in einem neuen Object Stream.
    pub fn object_locations(&self) -> Vec<(ObjectId, ObjectLocation)> {
        self.xref
            .iter()
            .filter_map(|(&num, e)| match *e {
                xref::Entry::InUse { offset, generation } => {
                    Some(((num, generation), ObjectLocation::Offset(offset.saturating_add(self.offset_base))))
                }
                xref::Entry::Compressed { stream, index } => {
                    Some(((num, 0), ObjectLocation::InStream { stream, index }))
                }
                xref::Entry::Free => None,
            })
            .collect()
    }

    /// `/Location`-Werte der Signaturdictionaries aller **signierten** Signaturfelder in
    /// Feldreihenfolge (ohne Duplikate) – das sind bei PDF-AS die Ressourcennamen benutzter
    /// QR-Platzhalter (`placement::PlaceholderQuery::used_names`, docs/analyse/04 §5.2).
    ///
    /// Additiv (Welle 2, Wunsch aus Paket F): `SigField` trägt die Werte nicht. Fehlende oder
    /// nicht lesbare Signaturdictionaries werden übersprungen.
    pub fn signature_locations(&self) -> Result<Vec<String>> {
        let mut out: Vec<String> = Vec::new();
        for f in self.signature_fields()?.iter().filter(|f| f.signed) {
            let value = match f.value_id {
                Some(id) => self.get_object(id).ok(),
                None => self
                    .get_object(f.field_id)
                    .ok()
                    .and_then(|o| o.as_dict().ok())
                    .and_then(|d| d.get(b"V").ok())
                    .and_then(|v| self.resolve(v).ok()),
            };
            let Some(Object::Dictionary(sig)) = value else { continue };
            let Some(loc) = sig.get(b"Location").ok().and_then(|o| self.resolve(o).ok()).and_then(decode_text_string)
            else {
                continue;
            };
            if !out.contains(&loc) {
                out.push(loc);
            }
        }
        Ok(out)
    }

    /// Originalschreibweise der Reals eines Objekts (siehe [`RealLiterals`]); leer, wenn das
    /// Objekt nicht über den Querverweis auffindbar ist.
    pub fn real_literals(&self, id: ObjectId) -> RealLiterals {
        let mut lits = RealLiterals::default();
        match self.xref.get(&id.0) {
            Some(xref::Entry::InUse { offset, generation }) if *generation == id.1 => {
                let Some(abs) = offset.checked_add(self.offset_base).and_then(|a| usize::try_from(a).ok()) else {
                    return lits;
                };
                if let Some((hid, after)) = lexer::object_header_at(&self.bytes, abs)
                    && hid == id
                {
                    lexer::collect_real_literals(&self.bytes, after, &mut lits.map);
                }
            }
            Some(xref::Entry::Compressed { stream, .. }) if id.1 == 0 => {
                let Ok(Object::Stream(container)) = self.get_object((*stream, 0)) else { return lits };
                let Ok(decoded) = filters::decode_stream(&self.document, container, &self.limits, false) else {
                    return lits;
                };
                let n = container.dict.get(b"N").ok().and_then(|o| o.as_i64().ok()).unwrap_or(0).max(0) as usize;
                let first =
                    container.dict.get(b"First").ok().and_then(|o| o.as_i64().ok()).unwrap_or(0).max(0) as usize;
                let mut lx = lexer::Lexer::new(&decoded.data, 0);
                for _ in 0..n.min(1_000_000) {
                    let (Some(lexer::Token::Int(num)), Some(lexer::Token::Int(off))) =
                        (lx.next_token(), lx.next_token())
                    else {
                        break;
                    };
                    if num == i64::from(id.0) && off >= 0 {
                        lexer::collect_real_literals(&decoded.data, first + off as usize, &mut lits.map);
                        break;
                    }
                }
            }
            _ => {}
        }
        lits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rights_like_pdfbox_utils() {
        assert!(Rights { encrypted: false, permissions: Permissions(0) }.may_sign());
        // nur Formulare ausfüllen
        assert!(Rights { encrypted: true, permissions: Permissions(1 << 8) }.may_sign());
        // Ändern + Annotationen
        assert!(Rights { encrypted: true, permissions: Permissions((1 << 3) | (1 << 5)) }.may_sign());
        // nur Ändern
        assert!(!Rights { encrypted: true, permissions: Permissions(1 << 3) }.may_sign());
        // nichts
        assert!(!Rights { encrypted: true, permissions: Permissions(0xFFFF_F0C0) }.may_sign());
    }

    #[test]
    fn version_compare() {
        assert!(version_key("1.7") > version_key("1.4"));
        assert!(version_key("2.0") > version_key("1.7"));
        assert_eq!(header_version(b"%PDF-1.5\n", 0), "1.5");
    }
}
