//! Alle lesenden Auswertungen von [`SourcePdf`] in einem Durchlauf – gemeinsam für den Robustheitstest
//! (`tests/robustness.rs`) und das Fuzz-Ziel `open` (`fuzz/fuzz_targets/open.rs`, dort per `#[path]` eingebunden).
//! Neue lesende Methoden von `SourcePdf` hier ergänzen, dann decken beide sie ab.

use pida_pdf_cos::{Limits, Object, SourcePdf};

/// Ruft alle lesenden Auswertungen auf; Fehler sind erlaubt, Panics nicht. `true`, wenn das
/// Dokument geöffnet werden konnte.
pub fn exercise(bytes: Vec<u8>, limits: &Limits) -> bool {
    let Ok(src) = SourcePdf::open(bytes, limits) else { return false };
    let _ = format!("{src:?}");
    let _ = (src.pdf_version(), src.trailer_id(), src.info_id(), src.next_object_number(), src.tail());
    let _ = (src.xref_kind(), src.xref_health(), src.prev_offset(), src.revisions().len(), src.rights().may_sign());
    let _ = src.catalog_id();
    let _ = src.xmp_metadata();
    let _ = src.pdfa_identification();
    let _ = src.doc_mdp();
    let _ = (src.has_usage_rights(), src.needs_rendering(), src.is_tagged());
    let _ = src.acroform_info();
    let _ = src.signature_fields();
    for i in 0..src.page_count().min(8) {
        let page = src.pages()[i].clone();
        let _ = page.visible_size();
        let _ = src.page_content(page.object_id);
        for descend in [false, true] {
            if let Ok(inv) = src.image_invocations(i, descend) {
                for img in inv.iter().take(8) {
                    if let Some(id) = img.xobject_id {
                        let _ = src.decode_image(id).map(|d| (d.to_luma(), d.to_rgba()));
                    }
                }
            }
        }
    }
    let ids: Vec<_> = src.document().objects.keys().copied().take(400).collect();
    for id in ids {
        let _ = src.real_literals(id);
        if let Ok(Object::Stream(_)) = src.get_object(id) {
            let _ = src.decoded_stream(id);
        }
        if let Ok(o) = src.get_object(id) {
            let _ = src.resolve(o);
        }
    }
    if let Some(enc) = src.encryptor() {
        let c = enc.encrypt((1, 0), b"NUR TEST", pida_pdf_cos::CryptTarget::String);
        if let Ok(c) = c {
            let _ = enc.decrypt((1, 0), &c, pida_pdf_cos::CryptTarget::String);
        }
    }
    true
}
