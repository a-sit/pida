//! Vorhandene Signaturen: Signaturdictionary lesen, ByteRange-Daten streamen, Status ermitteln
//! (Invariante I4, Plan §3.6; `cades::verify_detached` mit `AlgorithmPolicy::PriorSignature`).

use std::io::Read;

use pida_cades::{AlgorithmPolicy, CmsError, CmsInfo, CmsReport, Expect, ExpectedSigner, verify_detached};
use pida_pdf_cos::{Dictionary, Object, ObjectId, SigField, SourcePdf, decode_text_string};
use pida_pdf_incremental::ByteRange;

use crate::signed::PriorStatus;

/// Inhalt eines Signaturdictionarys (ohne Personendaten im `Debug`).
#[derive(Clone)]
pub(crate) struct SigDictInfo {
    /// ID des Signaturdictionarys (falls indirekt)
    pub id: Option<ObjectId>,
    /// `/Type` (`Sig`, `DocTimeStamp`)
    pub kind: Option<Vec<u8>>,
    /// `/SubFilter`
    pub sub_filter: Option<String>,
    /// `/ByteRange` (alle Zahlen)
    pub byte_range_raw: Option<Vec<i64>>,
    /// `/Contents` (roh, mit Padding)
    pub contents: Vec<u8>,
    /// `/Location`
    pub location: Option<String>,
}

impl std::fmt::Debug for SigDictInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SigDictInfo")
            .field("id", &self.id)
            .field("sub_filter", &self.sub_filter)
            .field("byte_range", &self.byte_range_raw)
            .field("contents_len", &self.contents.len())
            .finish_non_exhaustive()
    }
}

impl SigDictInfo {
    /// ByteRange mit genau vier nichtnegativen Zahlen.
    pub fn byte_range(&self) -> Option<ByteRange> {
        let v = self.byte_range_raw.as_ref()?;
        if v.len() != 4 || v.iter().any(|x| *x < 0) {
            return None;
        }
        Some(ByteRange([v[0] as u64, v[1] as u64, v[2] as u64, v[3] as u64]))
    }

    /// `/Contents` ohne `0`-Padding (DER), falls trennbar.
    pub fn der(&self) -> Vec<u8> {
        pida_cades::trim_contents_padding(&self.contents).map(<[u8]>::to_vec).unwrap_or_default()
    }

    /// Ende der abgedeckten Bytes (`max(off + len)` über alle Paare).
    pub fn covered_end(&self) -> Option<u64> {
        let v = self.byte_range_raw.as_ref()?;
        if v.is_empty() || !v.len().is_multiple_of(2) || v.iter().any(|x| *x < 0) {
            return None;
        }
        v.chunks(2).map(|p| (p[0] as u64).checked_add(p[1] as u64)).collect::<Option<Vec<_>>>()?.into_iter().max()
    }
}

/// Signaturdictionary eines (signierten) Felds.
pub(crate) fn read_sig_dict(src: &SourcePdf, field: &SigField) -> Option<SigDictInfo> {
    let (id, dict) = match field.value_id {
        Some(id) => (Some(id), src.get_object(id).ok()?.as_dict().ok()?.clone()),
        None => {
            let fd = src.get_object(field.field_id).ok()?.as_dict().ok()?;
            let v = src.resolve(fd.get(b"V").ok()?).ok()?;
            (None, v.as_dict().ok()?.clone())
        }
    };
    Some(sig_info_from_dict(src, id, &dict))
}

pub(crate) fn sig_info_from_dict(src: &SourcePdf, id: Option<ObjectId>, dict: &Dictionary) -> SigDictInfo {
    let name = |k: &[u8]| {
        dict.get(k).ok().and_then(|o| src.resolve(o).ok()).and_then(|o| o.as_name().ok()).map(<[u8]>::to_vec)
    };
    let byte_range_raw = dict.get(b"ByteRange").ok().and_then(|o| src.resolve(o).ok()).and_then(|o| match o {
        Object::Array(a) => a.iter().map(|x| crate::util::int_of(src, x)).collect::<Option<Vec<i64>>>(),
        _ => None,
    });
    let contents = match dict.get(b"Contents").ok().and_then(|o| src.resolve(o).ok()) {
        Some(Object::String(b, _)) => b.clone(),
        _ => Vec::new(),
    };
    SigDictInfo {
        id,
        kind: name(b"Type"),
        sub_filter: name(b"SubFilter").map(|n| String::from_utf8_lossy(&n).into_owned()),
        byte_range_raw,
        contents,
        location: dict.get(b"Location").ok().and_then(|o| src.resolve(o).ok()).and_then(decode_text_string),
    }
}

/// Liest die ByteRange-Abschnitte von `data` nacheinander (gestreamt, ohne Kopie).
pub(crate) struct RangeReader<'a> {
    data: &'a [u8],
    ranges: Vec<(usize, usize)>,
    idx: usize,
    pos: usize,
}

impl<'a> RangeReader<'a> {
    /// `pairs` = `[off0, len0, off1, len1, …]`; `None`, wenn ein Abschnitt außerhalb liegt.
    pub fn new(data: &'a [u8], pairs: &[i64]) -> Option<Self> {
        if pairs.is_empty() || !pairs.len().is_multiple_of(2) {
            return None;
        }
        let mut ranges = Vec::with_capacity(pairs.len() / 2);
        for p in pairs.chunks(2) {
            let off = usize::try_from(p[0]).ok()?;
            let len = usize::try_from(p[1]).ok()?;
            if off.checked_add(len)? > data.len() {
                return None;
            }
            ranges.push((off, len));
        }
        Some(Self { data, ranges, idx: 0, pos: 0 })
    }
}

impl Read for RangeReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        while let Some(&(off, len)) = self.ranges.get(self.idx) {
            if self.pos >= len {
                self.idx += 1;
                self.pos = 0;
                continue;
            }
            let n = (len - self.pos).min(buf.len());
            buf[..n].copy_from_slice(&self.data[off + self.pos..off + self.pos + n]);
            self.pos += n;
            return Ok(n);
        }
        Ok(0)
    }
}

/// Verfahren, die der eigene Verifier prüfen kann.
fn verifiable_sub_filter(sf: Option<&str>) -> bool {
    matches!(sf, Some("ETSI.CAdES.detached" | "adbe.pkcs7.detached"))
}

/// Status einer Signatur über `data` (ganze Datei): gültig, ungültig oder nicht prüfbar; dazu
/// der Fehlercode bei ungültigen bzw. nicht prüfbaren Signaturen.
pub(crate) fn signature_status(data: &[u8], info: &SigDictInfo) -> (PriorStatus, Option<String>) {
    let check = signature_check(data, info);
    (check.status, check.code)
}

/// Ergebnis von [`signature_check`]. `Debug` ohne den Namen des Unterzeichners (Regel 9).
pub(crate) struct SignatureCheck {
    /// Status
    pub status: PriorStatus,
    /// Fehlercode bei ungültiger bzw. nicht prüfbarer Signatur
    pub code: Option<String>,
    /// CN des Signerzertifikats (nur bei kryptografisch geprüfter Signatur)
    pub signer_cn: Option<String>,
    /// `signing-time` des CMS (Angabe des Signierenden, kein geprüfter Zeitstempel)
    pub signing_time: Option<time::OffsetDateTime>,
}

impl std::fmt::Debug for SignatureCheck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignatureCheck")
            .field("status", &self.status)
            .field("code", &self.code)
            .field("signer_cn", &self.signer_cn.as_ref().map(|_| "<redigiert>"))
            .field("signing_time", &self.signing_time)
            .finish()
    }
}

/// Wie [`signature_status`], zusätzlich mit Unterzeichner (CN) und `signing-time` aus derselben
/// Prüfung (`verify_detached`, ADR 0004 §3.3 „vorhandene Signaturen“).
pub(crate) fn signature_check(data: &[u8], info: &SigDictInfo) -> SignatureCheck {
    let d = signature_details(data, info);
    let (signer_cn, signing_time) = match &d.report {
        Some(r) => (r.signer.subject_cn(), r.signing_time),
        None => (None, None),
    };
    SignatureCheck { status: d.status, code: d.code, signer_cn, signing_time }
}

/// Ergebnis von [`signature_details`]: Status wie [`signature_check`], dazu der vollständige Bericht der
/// kryptografischen Prüfung bzw. – ohne bestandene Prüfung – die ungeprüfte Struktur des CMS
/// ([`crate::verify_document`]). `Debug` ohne Zertifikatsinhalte (Regel 9).
pub(crate) struct SignatureDetails {
    /// Status
    pub status: PriorStatus,
    /// Fehlercode bei ungültiger bzw. nicht prüfbarer Signatur
    pub code: Option<String>,
    /// Bericht von `verify_detached` (nur bei bestandener kryptografischer Prüfung; mit Status `Invalid`, wenn
    /// der signierte Bereich Teile der Datei offen lässt)
    pub report: Option<CmsReport>,
    /// ungeprüfte Struktur (`parse_cms`), wenn die Prüfung scheiterte bzw. nicht stattfand
    pub info: Option<CmsInfo>,
}

impl std::fmt::Debug for SignatureDetails {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignatureDetails")
            .field("status", &self.status)
            .field("code", &self.code)
            .field("verified", &self.report.is_some())
            .field("parsed", &self.info.is_some())
            .finish()
    }
}

/// `/ByteRange [0 a b c]` mit genau zwei Abschnitten, Beginn bei 0, ohne Überlappung, innerhalb der Datei.
/// Ein Bereich, der nicht bei 0 beginnt, ließe den Anfang der Datei ungeschützt.
pub(crate) fn byte_range_well_formed(info: &SigDictInfo, len: u64) -> bool {
    let Some(br) = info.byte_range() else { return false };
    let [a, b, c, d] = br.0;
    a == 0 && b.checked_add(a).is_some_and(|e| e <= c) && c.checked_add(d).is_some_and(|e| e <= len)
}

/// Die Lücke zwischen den beiden Abschnitten der `/ByteRange` ist **genau** der Hex-String `/Contents`
/// dieses Signaturdictionarys (`<…>` mit denselben Bytes, ohne weitere Zeichen). Sonst lägen ungeschützte
/// Bytes mitten im signierten Bereich – dort ließen sich z. B. weitere Schlüssel des Signaturdictionarys
/// oder Objekte unterbringen (Signature Wrapping); pyHanko verlangt dasselbe („contiguous“).
pub(crate) fn gap_is_contents(data: &[u8], info: &SigDictInfo) -> bool {
    let Some(br) = info.byte_range() else { return false };
    let [_, first_len, second_start, _] = br.0;
    let (Ok(start), Ok(end)) = (usize::try_from(first_len), usize::try_from(second_start)) else { return false };
    data.get(start..end).is_some_and(|gap| gap_is_hex_string(gap, &info.contents))
}

/// Ist `gap` (die Bytes zwischen den beiden Abschnitten einer `/ByteRange`) genau der Hex-String `<…>` mit den
/// Bytes `contents` (Wert von `/Contents` samt Padding), ohne weitere Zeichen? Für Prüfwerkzeuge außerhalb
/// der Engine (CLI `pida verify`), damit sie dieselbe Regel anwenden wie `verify_document` und der Inspektor.
pub fn gap_is_hex_string(gap: &[u8], contents: &[u8]) -> bool {
    let [b'<', hex @ .., b'>'] = gap else { return false };
    let nibble = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let (pairs, rest) = hex.as_chunks::<2>();
    rest.is_empty()
        && pairs.len() == contents.len()
        && pairs
            .iter()
            .zip(contents)
            .all(|([h, l], byte)| matches!((nibble(*h), nibble(*l)), (Some(h), Some(l)) if (h << 4 | l) == *byte))
}

/// Prüft eine Signatur über `data` (ganze Datei) wie [`signature_check`] und behält den Bericht
/// (`CmsReport`: Zertifikat, Verfahren, Hinweise wie SHA-1) bzw. die ungeprüfte Struktur des CMS.
pub(crate) fn signature_details(data: &[u8], info: &SigDictInfo) -> SignatureDetails {
    let fail = |status, code: &str| SignatureDetails {
        status,
        code: Some(code.into()),
        report: None,
        info: pida_cades::parse_cms(&info.contents).ok(),
    };
    if info.kind.as_deref() == Some(b"DocTimeStamp") || !verifiable_sub_filter(info.sub_filter.as_deref()) {
        return fail(PriorStatus::NotVerifiable, "pdf-signature-subfilter");
    }
    let Some(pairs) = info.byte_range_raw.as_deref() else {
        return fail(PriorStatus::Invalid, "pdf-signature-byterange");
    };
    // Mehr als die Datei kann eine ordentliche /ByteRange nicht abdecken: viele (überlappende) Paare würden sonst
    // beim bloßen Öffnen beliebig viel Hash-Arbeit erzwingen.
    let covered = pairs.chunks(2).try_fold(0u64, |sum, p| sum.checked_add(u64::try_from(*p.get(1)?).ok()?));
    if covered.is_none_or(|sum| sum > data.len() as u64) {
        return fail(PriorStatus::Invalid, "pdf-signature-byterange");
    }
    let Some(mut reader) = RangeReader::new(data, pairs) else {
        return fail(PriorStatus::Invalid, "pdf-signature-byterange");
    };
    let expect = Expect {
        signer: ExpectedSigner::Any,
        require_signing_certificate: false,
        max_len: None,
        policy: AlgorithmPolicy::PriorSignature,
    };
    match verify_detached(&info.contents, &mut reader, &expect) {
        // kryptografisch passend, aber der signierte Bereich lässt Teile der Datei offen (Anfang, Ende bzw.
        // Bytes in der Lücke neben /Contents – Signature Wrapping): verändert, wie in `verify_document`
        Ok(report) if !(byte_range_well_formed(info, data.len() as u64) && gap_is_contents(data, info)) => {
            SignatureDetails {
                status: PriorStatus::Invalid,
                code: Some(crate::verify::CODE_SIGNATURE_BYTERANGE.into()),
                report: Some(report),
                info: None,
            }
        }
        Ok(report) => SignatureDetails { status: PriorStatus::Valid, code: None, report: Some(report), info: None },
        Err(e @ CmsError::UnsupportedAlgorithm(_)) => fail(PriorStatus::NotVerifiable, e.code()),
        Err(e) => fail(PriorStatus::Invalid, e.code()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_reader_streams_pairs() {
        let data = b"0123456789";
        let mut r = RangeReader::new(data, &[0, 3, 6, 4]).unwrap();
        let mut out = Vec::new();
        r.read_to_end(&mut out).unwrap();
        assert_eq!(out, b"0126789");
        assert!(RangeReader::new(data, &[0, 3, 8, 4]).is_none());
        assert!(RangeReader::new(data, &[0, 3, 1]).is_none());
        assert!(RangeReader::new(data, &[-1, 3]).is_none());
    }

    #[test]
    fn covered_end_and_byte_range() {
        let info = SigDictInfo {
            id: None,
            kind: Some(b"Sig".to_vec()),
            sub_filter: None,
            byte_range_raw: Some(vec![0, 10, 20, 5]),
            contents: vec![],
            location: None,
        };
        assert_eq!(info.covered_end(), Some(25));
        assert_eq!(info.byte_range(), Some(ByteRange([0, 10, 20, 5])));
        assert_eq!(signature_status(b"", &info).0, PriorStatus::NotVerifiable);
        let bad = SigDictInfo { sub_filter: Some("ETSI.CAdES.detached".into()), ..info };
        assert_eq!(signature_status(b"short", &bad).0, PriorStatus::Invalid);
    }

    #[test]
    fn byte_range_must_start_at_zero_and_stay_inside() {
        let info = |br: Vec<i64>| SigDictInfo {
            id: None,
            kind: Some(b"Sig".to_vec()),
            sub_filter: Some("ETSI.CAdES.detached".into()),
            byte_range_raw: Some(br),
            contents: Vec::new(),
            location: None,
        };
        assert!(byte_range_well_formed(&info(vec![0, 10, 20, 5]), 25));
        assert!(!byte_range_well_formed(&info(vec![1, 10, 20, 5]), 25), "Anfang ungeschützt");
        assert!(!byte_range_well_formed(&info(vec![0, 21, 20, 5]), 25), "überlappend");
        assert!(!byte_range_well_formed(&info(vec![0, 10, 20, 6]), 25), "über das Dateiende");
        assert!(!byte_range_well_formed(&info(vec![0, 10, 20, 5, 30, 1]), 31), "drei Abschnitte");
        assert!(!byte_range_well_formed(&info(vec![0, i64::MAX, i64::MAX, 1]), 25));
    }

    #[test]
    fn gap_must_be_exactly_the_contents_string() {
        let info = |br: Vec<i64>, contents: &[u8]| SigDictInfo {
            id: None,
            kind: Some(b"Sig".to_vec()),
            sub_filter: Some("ETSI.CAdES.detached".into()),
            byte_range_raw: Some(br),
            contents: contents.to_vec(),
            location: None,
        };
        let data = b"AAAA<30a0FF00>BBBB";
        let contents = [0x30, 0xA0, 0xFF, 0x00];
        assert!(gap_is_contents(data, &info(vec![0, 4, 14, 4], &contents)));
        // andere Bytes, kürzerer String mit Fremdinhalt dahinter, Lücke ohne Klammern
        assert!(!gap_is_contents(data, &info(vec![0, 4, 14, 4], &[0x30, 0xA0, 0xFF, 0x01])));
        assert!(!gap_is_contents(b"AAAA<30a0>/X 1BBBB", &info(vec![0, 4, 14, 4], &[0x30, 0xA0])));
        assert!(!gap_is_contents(data, &info(vec![0, 5, 13, 5], &contents[..3])));
        // Leerraum im Hex-String (Länge passt nicht) bzw. Lücke außerhalb der Datei
        assert!(!gap_is_contents(b"AAAA<30 a0 FF>BBBB", &info(vec![0, 4, 14, 4], &contents[..3])));
        assert!(!gap_is_contents(data, &info(vec![0, 4, 40, 4], &contents)));
        assert!(!gap_is_contents(data, &info(vec![0, 14, 4, 4], &contents)));
    }
}
