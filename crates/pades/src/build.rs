//! Aufbau des Increments für `prepare()` (docs/analyse/03 §3 Phase C/D, §4.3–§4.11; Plan §6.4,
//! §6.9).
//!
//! Alle Änderungen an vorhandenen Objekten laufen über [`Edits`] (Kopie beim ersten Zugriff,
//! genau ein `update_object` je Objekt), neue Objekte bekommen ihre IDs vom `IncrementBuilder`.

use std::collections::{BTreeSet, HashMap};
use std::io::Write as _;
use std::sync::Arc;

use pida_pdf_cos::{
    Dictionary, Object, ObjectId, PageInfo, Rect, SigField, SourcePdf, Stream, StringFormat, encode_text_string,
};
use pida_pdf_incremental::{Increment, IncrementBuilder, IncrementOptions, SignatureDictSpec, format_pdf_date};
use pida_sigblock::Appearance;

use crate::analyze::{FIELD_PREFIX, next_field_number};
use crate::plan::{FieldTarget, SignPlan, Visibility};
use crate::util::{as_ref_id, int_of, literal, name};
use crate::{PadesError, icc};

/// Hinweis: PDF/UA-Ergänzung übersprungen (Struktur nicht unterstützt bzw. Vorsignaturen).
pub const NOTICE_PDFUA_SKIPPED: &str = "pades-pdfua-skipped";
/// Hinweis: OutputIntent nicht ergänzt, weil das Dokument bereits signiert ist.
pub const NOTICE_OUTPUT_INTENT_SKIPPED: &str = "pades-output-intent-skipped";

/// `/DA` eines neuen bzw. leeren AcroForms (PDFBox `AcroFormDefaultsProcessor`).
const DEFAULT_DA: &str = "/Helv 0 Tf 0 g ";
/// ProcSet der Appearance (Template-`/DR`).
const PROC_SET: [&str; 5] = ["PDF", "Text", "ImageC", "ImageB", "ImageI"];

/// Ergebnis des Aufbaus.
pub(crate) struct BuildOutput {
    pub increment: Increment,
    pub meta: BuildMeta,
}

/// Was `prepare()` über das Increment weiß (für Selbstprüfung und Persistenz).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct BuildMeta {
    /// voll qualifizierter Feldname
    pub field_name: String,
    /// Feld
    pub field_id: ObjectId,
    /// Widget
    pub widget_id: ObjectId,
    /// Signaturdictionary
    pub sig_dict_id: ObjectId,
    /// neue Seite
    pub new_page_id: Option<ObjectId>,
    /// Seite des Widgets (0-basiert), falls bekannt
    pub page_index: Option<usize>,
    /// neues Feld (sonst vorhandenes befüllt)
    pub new_field: bool,
    /// Hinweise (`pades-…`)
    pub notices: Vec<String>,
}

/// Änderungen an vorhandenen Objekten (Kopie beim ersten Zugriff).
struct Edits<'a> {
    src: &'a SourcePdf,
    order: Vec<ObjectId>,
    objs: HashMap<ObjectId, Object>,
}

impl<'a> Edits<'a> {
    fn new(src: &'a SourcePdf) -> Self {
        Self { src, order: Vec::new(), objs: HashMap::new() }
    }

    /// Aktueller Stand eines vorhandenen Objekts (bearbeitet oder Original).
    fn current(&self, id: ObjectId) -> Result<Object, PadesError> {
        match self.objs.get(&id) {
            Some(o) => Ok(o.clone()),
            None => Ok(self.src.get_object(id)?.clone()),
        }
    }

    fn dict(&self, id: ObjectId) -> Result<Dictionary, PadesError> {
        match self.current(id)? {
            Object::Dictionary(d) => Ok(d),
            _ => Err(PadesError::InvalidPlan(format!("Objekt {} {} ist kein Dictionary", id.0, id.1))),
        }
    }

    fn array(&self, id: ObjectId) -> Result<Vec<Object>, PadesError> {
        match self.current(id)? {
            Object::Array(a) => Ok(a),
            _ => Err(PadesError::InvalidPlan(format!("Objekt {} {} ist kein Array", id.0, id.1))),
        }
    }

    fn put(&mut self, id: ObjectId, obj: Object) {
        if !self.objs.contains_key(&id) {
            self.order.push(id);
        }
        self.objs.insert(id, obj);
    }

    /// Schreibt nur Objekte, die sich tatsächlich geändert haben.
    fn flush(mut self, b: &mut IncrementBuilder<'_>) -> Result<(), PadesError> {
        for id in std::mem::take(&mut self.order) {
            let Some(obj) = self.objs.remove(&id) else { continue };
            if self.src.get_object(id).is_ok_and(|orig| crate::util::same(orig, &obj)) {
                continue;
            }
            b.update_object(id, obj)?;
        }
        Ok(())
    }
}

/// Ort eines Dictionaries: indirekt (eigenes Objekt) oder direkt in einem Elternobjekt.
enum Loc {
    Indirect(ObjectId),
    Direct,
    Missing,
}

fn loc_of(o: Option<&Object>) -> Loc {
    match o {
        Some(Object::Reference(id)) => Loc::Indirect(*id),
        Some(Object::Dictionary(_)) => Loc::Direct,
        _ => Loc::Missing,
    }
}

fn real_array(v: &[f32]) -> Object {
    Object::Array(v.iter().map(|x| Object::Real(*x)).collect())
}

fn rect_object(r: Rect) -> Object {
    real_array(&[r.llx, r.lly, r.urx, r.ury])
}

/// Standard-14-Fontdictionaries für `/DR` (PDFBox `PDType1Font`).
fn helv_font() -> Object {
    let mut d = Dictionary::new();
    d.set("Type", name("Font"));
    d.set("Subtype", name("Type1"));
    d.set("BaseFont", name("Helvetica"));
    d.set("Encoding", name("WinAnsiEncoding"));
    Object::Dictionary(d)
}

fn zadb_font() -> Object {
    let mut d = Dictionary::new();
    d.set("Type", name("Font"));
    d.set("Subtype", name("Type1"));
    d.set("BaseFont", name("ZapfDingbats"));
    Object::Dictionary(d)
}

fn proc_set() -> Object {
    Object::Array(PROC_SET.iter().map(|n| name(n)).collect())
}

/// Ganzzahl eines Schlüssels (aufgelöst).
fn int_key(src: &SourcePdf, d: &Dictionary, key: &[u8]) -> Option<i64> {
    d.get(key).ok().and_then(|o| int_of(src, o))
}

/// Text eines Schlüssels (aufgelöst, Rohbytes).
fn str_key<'b>(src: &'b SourcePdf, d: &'b Dictionary, key: &[u8]) -> Option<&'b [u8]> {
    match src.resolve(d.get(key).ok()?).ok()? {
        Object::String(b, _) => Some(b.as_slice()),
        _ => None,
    }
}

/// Zielfeld gesperrt: `ReadOnly` oder durch `/Lock`/FieldMDP einer vorhandenen Signatur (wie
/// `analyze`).
fn is_locked(src: &SourcePdf, fields: &[SigField], f: &SigField) -> bool {
    f.field_flags & 1 != 0 || crate::analyze::locked_field_names(src, fields).contains(&f.qualified_name)
}

/// Geerbtes Seitenattribut (über `/Parent`), aufgelöst und kopiert.
fn inherited_attr(src: &SourcePdf, page_id: ObjectId, key: &[u8]) -> Option<Object> {
    let mut id = page_id;
    let mut seen = BTreeSet::new();
    for _ in 0..64 {
        if !seen.insert(id) {
            return None;
        }
        let d = src.get_object(id).ok()?.as_dict().ok()?;
        if let Ok(v) = d.get(key) {
            return src.resolve(v).ok().cloned();
        }
        id = as_ref_id(d.get(b"Parent").ok()?)?;
    }
    None
}

/// MediaBox der neuen Seite: Originalobjekt der letzten Seite (Schreibweise bleibt), sonst Rect.
fn media_box_of(src: &SourcePdf, last: &PageInfo) -> Object {
    match inherited_attr(src, last.object_id, b"MediaBox") {
        Some(Object::Array(a))
            if a.len() == 4 && a.iter().all(|o| matches!(o, Object::Integer(_) | Object::Real(_))) =>
        {
            Object::Array(a)
        }
        _ => rect_object(last.media_box),
    }
}

/// Reserve-Prüfungen und Aufbau. `reserve` = CMS-Bytes für `/Contents`.
pub(crate) fn build(src: &SourcePdf, plan: &SignPlan, reserve: usize) -> Result<BuildOutput, PadesError> {
    let fields = src.signature_fields()?;
    let signed_before = fields.iter().any(|f| f.signed);
    let doc_mdp = src.doc_mdp()?;
    // PDFBox-Fixup (`/DR /Font /Helv /ZaDb`, `/DA`) nicht bei PDF/A: nicht eingebettete Fonts
    // (KR-P09); Signaturfelder brauchen weder `/DA` noch Default-Fonts.
    let fixup = src.pdfa_identification()?.is_none();
    let catalog_id = src.catalog_id()?;
    let mut catalog = src.catalog()?.clone();
    let mut catalog_changed = false;
    let mut ed = Edits::new(src);
    let mut b = IncrementBuilder::new(src, IncrementOptions::default());
    let mut notices: Vec<String> = Vec::new();
    let mut new_objects: Vec<(ObjectId, Object)> = Vec::new();

    // Signaturdictionary (PDF-AS-Reihenfolge, serialisiert von pdf-incremental)
    let spec = SignatureDictSpec {
        signing_time: Some(format_pdf_date(plan.signing_time)),
        location: plan.location.clone(),
        name: plan.name.clone(),
        reason: plan.reason.clone(),
        contents_reserve: reserve,
        ..SignatureDictSpec::default()
    };
    let sig_id = b.add_signature_dictionary(spec)?;

    // Appearance (lokale Referenzen → echte IDs)
    let (appearance, block) = match &plan.visibility {
        Visibility::Visible(bp) => (Arc::clone(&bp.appearance), Some(bp)),
        Visibility::Invisible => (Arc::new(Appearance::invisible()), None),
    };
    if appearance.objects.is_empty()
        || appearance.holder >= appearance.objects.len()
        || appearance.inner >= appearance.objects.len()
    {
        return Err(PadesError::InvalidPlan("Erscheinungsbild ohne Holder-Form".into()));
    }
    let visible = block.is_some() && !appearance.inner_name.is_empty();
    let mat = appearance.materialize(&mut || b.allocate_id());
    let holder_id = mat.holder_id;
    let inner = visible.then(|| (mat.inner_name.clone(), mat.inner_id));
    new_objects.extend(mat.objects);
    let mut ap = Dictionary::new();
    ap.set("N", Object::Reference(holder_id));

    let pdfua_wanted = plan.pdfua && src.is_tagged();
    let meta = match &plan.field {
        FieldTarget::New { name: explicit } => {
            let field_name = match explicit {
                Some(n) if !n.is_empty() => n.clone(),
                _ => format!("{FIELD_PREFIX}{}", next_field_number(&fields, FIELD_PREFIX)),
            };
            if fields.iter().any(|f| f.qualified_name == field_name) {
                return Err(PadesError::InvalidPlan("Feldname ist bereits vergeben".into()));
            }
            // neue Felder liegen auf der obersten Ebene; ein Punkt wäre ein Hierarchietrenner
            if field_name.contains('.') {
                return Err(PadesError::InvalidPlan("Feldname darf keinen Punkt enthalten".into()));
            }
            // ein Erscheinungsbild ohne Inner-Form gilt als unsichtbar (erste Seite, Rect 0)
            let (page_index, new_page, rect) = match block {
                Some(bp) if visible => (bp.page_index, bp.new_page, bp.rect),
                _ => (0, false, Rect::default()),
            };
            // Zertifiziertes Dokument (DocMDP P=2/3): ein neues **sichtbares** Feld wertet die
            // Änderungsanalyse als `OTHER` (Zertifizierung verletzt, pyHanko-Gegenprobe im
            // Korpus); erlaubt sind vorhandene leere Felder und unsichtbare Signaturen (Plan §6.1 K4)
            if visible && doc_mdp.is_some() {
                return Err(PadesError::NewVisibleFieldNotAllowed);
            }
            if new_page {
                if signed_before || doc_mdp.is_some() {
                    return Err(PadesError::NewPageNotAllowed);
                }
                if page_index != src.page_count() {
                    return Err(PadesError::InvalidPlan("neue Seite muss nach der letzten Seite folgen".into()));
                }
            } else if page_index >= src.page_count() {
                return Err(PadesError::InvalidPlan(format!("Seite {page_index} existiert nicht")));
            }
            if ![rect.llx, rect.lly, rect.urx, rect.ury].iter().all(|v| v.is_finite()) {
                return Err(PadesError::InvalidPlan("Widget-Rechteck nicht endlich".into()));
            }
            let widget_id = b.allocate_id();
            let page_id = if new_page { b.allocate_id() } else { src.pages()[page_index].object_id };

            // PDF/UA (vor dem Widget: liefert /StructParent)
            let struct_parent = if pdfua_wanted && signed_before {
                notices.push(NOTICE_PDFUA_SKIPPED.into());
                None
            } else if pdfua_wanted {
                match inject_struct(src, &mut ed, &mut catalog, &mut catalog_changed, &mut b, widget_id, page_id) {
                    Ok((key, objs)) => {
                        new_objects.extend(objs);
                        Some(key)
                    }
                    Err(reason) => {
                        tracing::debug!("PDF/UA-Ergänzung übersprungen: {reason}");
                        notices.push(NOTICE_PDFUA_SKIPPED.into());
                        None
                    }
                }
            } else {
                None
            };

            let tu =
                if visible && !appearance.tu_text.is_empty() { appearance.tu_text.clone() } else { field_name.clone() };
            let mut w = Dictionary::new();
            w.set("FT", name("Sig"));
            w.set("Type", name("Annot"));
            w.set("Subtype", name("Widget"));
            w.set("F", Object::Integer(132));
            w.set("T", encode_text_string(&field_name));
            w.set("V", Object::Reference(sig_id));
            w.set("P", Object::Reference(page_id));
            w.set("Rect", if visible { rect_object(rect) } else { rect_object(Rect::default()) });
            w.set("AP", Object::Dictionary(ap));
            w.set("TU", encode_text_string(&tu));
            if let Some(k) = struct_parent {
                w.set("StructParent", Object::Integer(k));
            }
            new_objects.push((widget_id, Object::Dictionary(w)));

            if new_page {
                let pages_root = as_ref_id(
                    catalog.get(b"Pages").map_err(|_| PadesError::InvalidPlan("Katalog ohne /Pages".into()))?,
                )
                .ok_or_else(|| PadesError::InvalidPlan("/Pages ist keine Referenz".into()))?;
                let last = src.pages().last().ok_or(PadesError::Cos(pida_pdf_cos::CosError::NoPages))?;
                let mut page = Dictionary::new();
                page.set("Type", name("Page"));
                page.set("MediaBox", media_box_of(src, last));
                page.set("Resources", Object::Dictionary(Dictionary::new()));
                page.set("Rotate", Object::Integer(i64::from(last.rotation.degrees())));
                page.set("Parent", Object::Reference(pages_root));
                page.set("Annots", Object::Array(vec![Object::Reference(widget_id)]));
                if struct_parent.is_some() {
                    page.set("Tabs", name("S"));
                }
                new_objects.push((page_id, Object::Dictionary(page)));
                let mut root = ed.dict(pages_root)?;
                match root.get(b"Kids").ok().cloned() {
                    Some(Object::Reference(kid_arr)) => {
                        let mut a = ed.array(kid_arr)?;
                        a.push(Object::Reference(page_id));
                        ed.put(kid_arr, Object::Array(a));
                    }
                    Some(Object::Array(mut a)) => {
                        a.push(Object::Reference(page_id));
                        root.set("Kids", Object::Array(a));
                    }
                    _ => return Err(PadesError::InvalidPlan("/Pages ohne /Kids".into())),
                }
                let count = int_key(src, &root, b"Count").unwrap_or(src.page_count() as i64);
                root.set("Count", Object::Integer(count.saturating_add(1)));
                ed.put(pages_root, Object::Dictionary(root));
            } else {
                let page = &src.pages()[page_index];
                register_annot(src, &mut ed, page, widget_id)?;
                if struct_parent.is_some() {
                    set_tabs(&mut ed, page.object_id)?;
                }
            }
            update_acroform(src, &mut ed, &mut catalog, &mut catalog_changed, Some(widget_id), inner.as_ref(), fixup)?;
            BuildMeta {
                field_name,
                field_id: widget_id,
                widget_id,
                sig_dict_id: sig_id,
                new_page_id: new_page.then_some(page_id),
                page_index: Some(page_index),
                new_field: true,
                notices: Vec::new(),
            }
        }
        FieldTarget::Existing { field_id, widget_id, .. } => {
            let f = fields.iter().find(|f| f.field_id == *field_id).ok_or(PadesError::FieldNotFound)?;
            if f.signed {
                return Err(PadesError::FieldAlreadySigned(f.qualified_name.clone()));
            }
            if is_locked(src, &fields, f) {
                return Err(PadesError::FieldLocked(f.qualified_name.clone()));
            }
            let widget = f.widgets.iter().find(|w| w.id == *widget_id).ok_or(PadesError::FieldNotFound)?;
            if block.is_some_and(|bp| bp.new_page) {
                return Err(PadesError::NewPageNotAllowed);
            }
            // Feld: /V anhängen
            let mut fd = ed.dict(f.field_id)?;
            fd.set("V", Object::Reference(sig_id));
            ed.put(f.field_id, Object::Dictionary(fd));
            // Widget (verschmolzen oder eigenes Kind): /AP ersetzen, Print-Flag; /Rect bleibt
            let mut wd = ed.dict(widget.id)?;
            let flags = int_key(src, &wd, b"F").unwrap_or(0);
            wd.set("F", Object::Integer(flags | 4));
            wd.set("AP", Object::Dictionary(ap));
            let page_id = widget.page_index.and_then(|i| src.pages().get(i)).map(|p| p.object_id);
            let has_struct_parent = wd.has(b"StructParent");
            if pdfua_wanted
                && !has_struct_parent
                && let Some(page_id) = page_id
            {
                if signed_before {
                    notices.push(NOTICE_PDFUA_SKIPPED.into());
                } else {
                    match inject_struct(src, &mut ed, &mut catalog, &mut catalog_changed, &mut b, widget.id, page_id) {
                        Ok((key, objs)) => {
                            new_objects.extend(objs);
                            wd.set("StructParent", Object::Integer(key));
                            set_tabs(&mut ed, page_id)?;
                        }
                        Err(reason) => {
                            tracing::debug!("PDF/UA-Ergänzung übersprungen: {reason}");
                            notices.push(NOTICE_PDFUA_SKIPPED.into());
                        }
                    }
                }
            }
            ed.put(widget.id, Object::Dictionary(wd));
            update_acroform(src, &mut ed, &mut catalog, &mut catalog_changed, None, inner.as_ref(), fixup)?;
            BuildMeta {
                field_name: f.qualified_name.clone(),
                field_id: f.field_id,
                widget_id: widget.id,
                sig_dict_id: sig_id,
                new_page_id: None,
                page_index: widget.page_index,
                new_field: false,
                notices: Vec::new(),
            }
        }
    };

    // PDF/A: sRGB-OutputIntent nur ergänzen, wenn keiner vorhanden ist (Plan §6.9, KR-P02)
    if plan.add_output_intent_if_missing && visible && !has_output_intents(src, &catalog) {
        if signed_before {
            notices.push(NOTICE_OUTPUT_INTENT_SKIPPED.into());
        } else {
            let (intent_id, objs) = output_intent(&mut b)?;
            new_objects.extend(objs);
            catalog.set("OutputIntents", Object::Array(vec![Object::Reference(intent_id)]));
            catalog_changed = true;
        }
    }

    if catalog_changed {
        ed.put(catalog_id, Object::Dictionary(catalog));
    }
    for (id, obj) in new_objects {
        b.insert_new(id, obj)?;
    }
    ed.flush(&mut b)?;
    let increment = b.build()?;
    notices.dedup();
    Ok(BuildOutput { increment, meta: BuildMeta { notices, ..meta } })
}

/// Indirektes `/Annots`-Array, das nur diese Seite benutzt (ein von mehreren Seiten geteiltes
/// Array darf nicht verlängert werden, sonst erschiene das Widget auf allen diesen Seiten).
pub(crate) fn own_annots_array(src: &SourcePdf, page: &PageInfo) -> Option<ObjectId> {
    let id = page.annots_indirect?;
    (src.pages().iter().filter(|p| p.annots_indirect == Some(id)).count() == 1).then_some(id)
}

/// Hängt das Widget an `/Annots` der Seite (eigenes indirektes Array → Array-Objekt
/// fortschreiben; geteiltes Array → direkte Kopie in der Seite).
fn register_annot(src: &SourcePdf, ed: &mut Edits<'_>, page: &PageInfo, widget: ObjectId) -> Result<(), PadesError> {
    if let Some(arr_id) = own_annots_array(src, page) {
        let mut a = ed.array(arr_id)?;
        a.push(Object::Reference(widget));
        ed.put(arr_id, Object::Array(a));
        return Ok(());
    }
    let mut pd = ed.dict(page.object_id)?;
    let mut annots = match pd.get(b"Annots") {
        Ok(Object::Array(a)) => a.clone(),
        Ok(o) => src.resolve(o).ok().and_then(|o| o.as_array().ok()).cloned().unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    annots.push(Object::Reference(widget));
    pd.set("Annots", Object::Array(annots));
    ed.put(page.object_id, Object::Dictionary(pd));
    Ok(())
}

/// `/Tabs /S` (PDF/UA, Tabulatorreihenfolge nach Struktur).
fn set_tabs(ed: &mut Edits<'_>, page_id: ObjectId) -> Result<(), PadesError> {
    let mut pd = ed.dict(page_id)?;
    if pd.get(b"Tabs").ok().and_then(|o| o.as_name().ok()) != Some(b"S") {
        pd.set("Tabs", name("S"));
        ed.put(page_id, Object::Dictionary(pd));
    }
    Ok(())
}

fn has_output_intents(src: &SourcePdf, catalog: &Dictionary) -> bool {
    matches!(catalog.get(b"OutputIntents").ok().and_then(|o| src.resolve(o).ok()), Some(Object::Array(a)) if !a.is_empty())
}

/// sRGB-OutputIntent wie PDF-AS (`/S /GTS_PDFA1`, `sRGB IEC61966-2.1`, `http://www.color.org`).
fn output_intent(b: &mut IncrementBuilder<'_>) -> Result<(ObjectId, Vec<(ObjectId, Object)>), PadesError> {
    let profile = icc::srgb_profile();
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(profile)?;
    let data = z.finish()?;
    let mut sd = Dictionary::new();
    sd.set("N", Object::Integer(3));
    sd.set("Filter", name("FlateDecode"));
    sd.set("Length", Object::Integer(data.len() as i64));
    let icc_id = b.allocate_id();
    let intent_id = b.allocate_id();
    let mut oi = Dictionary::new();
    oi.set("Type", name("OutputIntent"));
    oi.set("S", name("GTS_PDFA1"));
    oi.set("OutputCondition", literal(icc::OUTPUT_CONDITION));
    oi.set("OutputConditionIdentifier", literal(icc::OUTPUT_CONDITION));
    oi.set("RegistryName", literal(icc::REGISTRY));
    oi.set("Info", literal(icc::OUTPUT_CONDITION));
    oi.set("DestOutputProfile", Object::Reference(icc_id));
    Ok((intent_id, vec![(icc_id, Object::Stream(Stream::new(sd, data))), (intent_id, Object::Dictionary(oi))]))
}

/// Neues `/DR` (Template-Ressourcen + PDFBox-Fixup): `/XObject /FRM1`, `/ProcSet`, `/Font`
/// `/Helv` `/ZaDb` (Fonts nur mit `fixup`, siehe KR-P09).
fn new_dr(inner: Option<&(String, ObjectId)>, fixup: bool) -> Dictionary {
    let mut dr = Dictionary::new();
    if let Some((n, id)) = inner {
        let mut xo = Dictionary::new();
        xo.set(n.as_bytes().to_vec(), Object::Reference(*id));
        dr.set("XObject", Object::Dictionary(xo));
        dr.set("ProcSet", proc_set());
    }
    if fixup {
        let mut fonts = Dictionary::new();
        fonts.set("Helv", helv_font());
        fonts.set("ZaDb", zadb_font());
        dr.set("Font", Object::Dictionary(fonts));
    }
    dr
}

/// Ergänzt fehlende `/Helv`/`/ZaDb` (nur mit `fixup`) und – nur wenn `/DR` bereits ein
/// `/XObject`-Dictionary hat – `/FRM1` (PDFBox `assignAcroFormDefaultResource`).
fn fix_dr(
    ed: &mut Edits<'_>,
    dr: &mut Dictionary,
    inner: Option<&(String, ObjectId)>,
    fixup: bool,
) -> Result<(), PadesError> {
    let ensure_fonts = |f: &mut Dictionary| {
        if !f.has(b"Helv") {
            f.set("Helv", helv_font());
        }
        if !f.has(b"ZaDb") {
            f.set("ZaDb", zadb_font());
        }
    };
    match dr.get(b"Font").ok().cloned() {
        _ if !fixup => {}
        // Verweis auf ein Font-Dictionary: dort ergänzen. Ein Wert, der kein Dictionary ist (auch
        // ein Verweis auf etwas anderes), wird wie bei PDFBox (`getCOSDictionary` = null) durch
        // ein neues direktes Dictionary ersetzt (`check::check_dr` lässt genau das zu).
        Some(Object::Reference(fid)) if matches!(ed.current(fid), Ok(Object::Dictionary(_))) => {
            let mut f = ed.dict(fid)?;
            ensure_fonts(&mut f);
            ed.put(fid, Object::Dictionary(f));
        }
        Some(Object::Dictionary(mut f)) => {
            ensure_fonts(&mut f);
            dr.set("Font", Object::Dictionary(f));
        }
        _ => {
            let mut f = Dictionary::new();
            ensure_fonts(&mut f);
            dr.set("Font", Object::Dictionary(f));
        }
    }
    if let Some((n, id)) = inner {
        match dr.get(b"XObject").ok().cloned() {
            Some(Object::Reference(xid)) => {
                if let Ok(mut x) = ed.dict(xid) {
                    x.set(n.as_bytes().to_vec(), Object::Reference(*id));
                    ed.put(xid, Object::Dictionary(x));
                }
            }
            Some(Object::Dictionary(mut x)) => {
                x.set(n.as_bytes().to_vec(), Object::Reference(*id));
                dr.set("XObject", Object::Dictionary(x));
            }
            _ => {}
        }
    }
    Ok(())
}

/// AcroForm neu (direkt im Katalog) bzw. fortschreiben: `/Fields` (+ neues Feld), `/SigFlags 3`,
/// `/DR`, `/DA` (docs/analyse/03 §4.3, Phase D Schritte 5–11).
fn update_acroform(
    src: &SourcePdf,
    ed: &mut Edits<'_>,
    catalog: &mut Dictionary,
    catalog_changed: &mut bool,
    new_widget: Option<ObjectId>,
    inner: Option<&(String, ObjectId)>,
    fixup: bool,
) -> Result<(), PadesError> {
    let mut loc = loc_of(catalog.get(b"AcroForm").ok());
    // Verweis auf ein fehlendes oder kaputtes (kein Dictionary) AcroForm: nicht überschreiben
    // (das Objekt kann anderswo referenziert sein), sondern ein neues direkt im Katalog anlegen
    if let Loc::Indirect(id) = loc
        && !matches!(ed.current(id), Ok(Object::Dictionary(_)))
    {
        loc = Loc::Missing;
    }
    let mut acro = match &loc {
        Loc::Indirect(id) => match ed.current(*id)? {
            Object::Dictionary(d) => d,
            _ => Dictionary::new(),
        },
        Loc::Direct => catalog.get(b"AcroForm").ok().and_then(|o| o.as_dict().ok()).cloned().unwrap_or_default(),
        Loc::Missing => Dictionary::new(),
    };
    let created = matches!(loc, Loc::Missing);

    if let Some(w) = new_widget {
        match acro.get(b"Fields").ok().cloned() {
            Some(Object::Reference(fid)) => {
                let mut a = ed.array(fid)?;
                a.push(Object::Reference(w));
                ed.put(fid, Object::Array(a));
            }
            Some(Object::Array(mut a)) => {
                a.push(Object::Reference(w));
                acro.set("Fields", Object::Array(a));
            }
            _ => acro.set("Fields", Object::Array(vec![Object::Reference(w)])),
        }
    } else if !acro.has(b"Fields") {
        acro.set("Fields", Object::Array(Vec::new()));
    }
    let flags = int_key(src, &acro, b"SigFlags").unwrap_or(0);
    if flags | 3 != flags || !matches!(acro.get(b"SigFlags"), Ok(Object::Integer(_))) {
        acro.set("SigFlags", Object::Integer(flags | 3));
    }
    let set_new_dr = |acro: &mut Dictionary| {
        let dr = new_dr(inner, fixup);
        if !dr.is_empty() {
            acro.set("DR", Object::Dictionary(dr));
        }
    };
    match acro.get(b"DR").ok().cloned() {
        Some(Object::Reference(drid)) => match ed.current(drid)? {
            Object::Dictionary(mut dr) => {
                fix_dr(ed, &mut dr, inner, fixup)?;
                ed.put(drid, Object::Dictionary(dr));
            }
            _ => set_new_dr(&mut acro),
        },
        Some(Object::Dictionary(mut dr)) => {
            fix_dr(ed, &mut dr, inner, fixup)?;
            acro.set("DR", Object::Dictionary(dr));
        }
        _ => set_new_dr(&mut acro),
    }
    if fixup && str_key(src, &acro, b"DA").is_none_or(|s| s.is_empty()) {
        acro.set("DA", Object::String(DEFAULT_DA.as_bytes().to_vec(), StringFormat::Literal));
    }

    match loc {
        Loc::Indirect(id) => ed.put(id, Object::Dictionary(acro)),
        Loc::Direct | Loc::Missing => {
            let old = catalog.get(b"AcroForm").ok().cloned();
            let new = Object::Dictionary(acro);
            if created || old.as_ref().is_none_or(|o| !crate::util::same(o, &new)) {
                catalog.set("AcroForm", new);
                *catalog_changed = true;
            }
        }
    }
    Ok(())
}

/// Größter Schlüssel eines Number Trees (`/Nums`, rekursiv `/Kids`).
fn max_number_key(src: &SourcePdf, node: &Dictionary, depth: u32, seen: &mut BTreeSet<ObjectId>) -> Option<i64> {
    if depth > 32 {
        return None;
    }
    let mut best: Option<i64> = None;
    if let Some(Object::Array(nums)) = node.get(b"Nums").ok().and_then(|o| src.resolve(o).ok()) {
        for k in nums.iter().step_by(2) {
            if let Some(v) = int_of(src, k) {
                best = Some(best.map_or(v, |b| b.max(v)));
            }
        }
    }
    if let Some(Object::Array(kids)) = node.get(b"Kids").ok().and_then(|o| src.resolve(o).ok()) {
        for k in kids {
            if let Some(id) = as_ref_id(k)
                && !seen.insert(id)
            {
                continue;
            }
            if let Ok(Object::Dictionary(kd)) = src.resolve(k)
                && let Some(v) = max_number_key(src, kd, depth + 1, seen)
            {
                best = Some(best.map_or(v, |b| b.max(v)));
            }
        }
    }
    best
}

/// Erstes Strukturelement unter `/StructTreeRoot /K` (indirekt; PDF-AS `docElement`).
pub(crate) fn first_struct_elem(src: &SourcePdf, k: &Object) -> Option<ObjectId> {
    let is_elem = |id: ObjectId| {
        src.get_object(id)
            .ok()
            .and_then(|o| o.as_dict().ok())
            .is_some_and(|d| d.has(b"S") && !d.has_type(b"OBJR") && !d.has_type(b"MCR"))
    };
    match k {
        Object::Reference(id) => match src.get_object(*id).ok()? {
            Object::Array(a) => a.iter().filter_map(as_ref_id).find(|id| is_elem(*id)),
            Object::Dictionary(_) if is_elem(*id) => Some(*id),
            _ => None,
        },
        Object::Array(a) => a.iter().filter_map(as_ref_id).find(|id| is_elem(*id)),
        _ => None,
    }
}

/// PDF/UA-Struktur wie PDF-AS (`PPS:427-570`, docs/analyse/03 §4.9): neues StructElem `/Form`
/// mit OBJR unter dem ersten Strukturelement, ParentTree-Eintrag, `/ParentTreeNextKey`.
/// Liefert den `/StructParent`-Schlüssel und die neuen Objekte; Fehler = Grund (ohne Inhalt).
fn inject_struct(
    src: &SourcePdf,
    ed: &mut Edits<'_>,
    catalog: &mut Dictionary,
    catalog_changed: &mut bool,
    b: &mut IncrementBuilder<'_>,
    widget_id: ObjectId,
    page_id: ObjectId,
) -> Result<(i64, Vec<(ObjectId, Object)>), String> {
    let st_loc = loc_of(catalog.get(b"StructTreeRoot").ok());
    let mut root = match &st_loc {
        Loc::Indirect(id) => ed.dict(*id).map_err(|e| e.to_string())?,
        Loc::Direct => catalog.get(b"StructTreeRoot").ok().and_then(|o| o.as_dict().ok()).cloned().unwrap_or_default(),
        Loc::Missing => return Err("kein StructTreeRoot".into()),
    };
    let k = root.get(b"K").map_err(|_| "StructTreeRoot ohne /K".to_string())?.clone();
    let doc_elem = first_struct_elem(src, &k).ok_or("kein indirektes Strukturelement unter /K")?;

    // ParentTree prüfen, bevor IDs vergeben werden
    enum Pt {
        Nums(Option<ObjectId>, Option<ObjectId>),
        Kids(Option<ObjectId>, Option<ObjectId>),
        Missing,
    }
    let (pt_id, pt_dict) = match root.get(b"ParentTree").ok() {
        Some(Object::Reference(id)) => (Some(*id), ed.dict(*id).map_err(|e| e.to_string())?),
        Some(Object::Dictionary(d)) => (None, d.clone()),
        _ => (None, Dictionary::new()),
    };
    let pt_kind = if !root.has(b"ParentTree") {
        Pt::Missing
    } else {
        match (pt_dict.get(b"Nums").ok(), pt_dict.get(b"Kids").ok()) {
            (Some(n), None) => Pt::Nums(pt_id, as_ref_id(n)),
            (None, Some(k)) => Pt::Kids(pt_id, as_ref_id(k)),
            _ => return Err("ParentTree weder nur /Nums noch nur /Kids (error.pdf.sig.pdfua.1)".into()),
        }
    };
    // PDF-AS: `/ParentTreeNextKey`, sonst größter Schlüssel + 1. Ein veralteter
    // `/ParentTreeNextKey` (kleiner als vorhandene Schlüssel) ergäbe einen doppelten bzw. nicht
    // sortierten Eintrag im Number Tree – daher das Maximum aus beiden.
    let after_max = max_number_key(src, &pt_dict, 0, &mut BTreeSet::new()).map_or(0, |m| m.saturating_add(1));
    let key = match int_key(src, &root, b"ParentTreeNextKey") {
        Some(k) if k >= 0 => k.max(after_max),
        _ => after_max,
    };
    // alle Lesezugriffe vor der ID-Vergabe (eine vergebene, nicht gesetzte ID wäre ein Fehler)
    if let Pt::Nums(_, Some(r)) | Pt::Kids(_, Some(r)) = &pt_kind {
        ed.array(*r).map_err(|e| e.to_string())?;
    }
    let mut parent = ed.dict(doc_elem).map_err(|e| e.to_string())?;

    let elem_id = b.allocate_id();
    let mut objs = Vec::new();
    let mut objr = Dictionary::new();
    objr.set("Type", name("OBJR"));
    objr.set("Pg", Object::Reference(page_id));
    objr.set("Obj", Object::Reference(widget_id));
    let mut attrs = Dictionary::new();
    attrs.set("O", name("Layout"));
    attrs.set("Placement", name("Block"));
    let mut elem = Dictionary::new();
    elem.set("Type", name("StructElem"));
    elem.set("S", name("Form"));
    elem.set("P", Object::Reference(doc_elem));
    elem.set("T", literal("Signature Table"));
    elem.set("Pg", Object::Reference(page_id));
    elem.set("K", Object::Array(vec![Object::Dictionary(objr)]));
    elem.set("A", Object::Dictionary(attrs));
    objs.push((elem_id, Object::Dictionary(elem)));

    // an das Elternelement anhängen
    let new_k = match parent.get(b"K").ok().cloned() {
        None | Some(Object::Null) => Object::Array(vec![Object::Reference(elem_id)]),
        Some(Object::Array(mut a)) => {
            a.push(Object::Reference(elem_id));
            Object::Array(a)
        }
        Some(Object::Reference(r)) if matches!(ed.current(r), Ok(Object::Array(_))) => {
            let mut a = ed.array(r).map_err(|e| e.to_string())?;
            a.push(Object::Reference(elem_id));
            ed.put(r, Object::Array(a));
            Object::Reference(r)
        }
        Some(other) => Object::Array(vec![other, Object::Reference(elem_id)]),
    };
    parent.set("K", new_k);
    ed.put(doc_elem, Object::Dictionary(parent));

    // ParentTree
    let entry = [Object::Integer(key), Object::Reference(elem_id)];
    match pt_kind {
        Pt::Missing => {
            let pid = b.allocate_id();
            let mut pt = Dictionary::new();
            pt.set("Nums", Object::Array(entry.to_vec()));
            objs.push((pid, Object::Dictionary(pt)));
            root.set("ParentTree", Object::Reference(pid));
        }
        Pt::Nums(pt_id, nums_ref) => {
            let mut pt = pt_dict;
            match nums_ref {
                Some(r) => {
                    let mut a = ed.array(r).map_err(|e| e.to_string())?;
                    a.extend(entry);
                    ed.put(r, Object::Array(a));
                }
                None => {
                    let mut a = pt.get(b"Nums").ok().and_then(|o| o.as_array().ok()).cloned().unwrap_or_default();
                    a.extend(entry);
                    pt.set("Nums", Object::Array(a));
                }
            }
            match pt_id {
                Some(id) => ed.put(id, Object::Dictionary(pt)),
                None => root.set("ParentTree", Object::Dictionary(pt)),
            }
        }
        Pt::Kids(pt_id, kids_ref) => {
            let leaf_id = b.allocate_id();
            let mut leaf = Dictionary::new();
            leaf.set("Nums", Object::Array(entry.to_vec()));
            leaf.set("Limits", Object::Array(vec![Object::Integer(key), Object::Integer(key)]));
            objs.push((leaf_id, Object::Dictionary(leaf)));
            let mut pt = pt_dict;
            match kids_ref {
                Some(r) => {
                    let mut a = ed.array(r).map_err(|e| e.to_string())?;
                    a.push(Object::Reference(leaf_id));
                    ed.put(r, Object::Array(a));
                }
                None => {
                    let mut a = pt.get(b"Kids").ok().and_then(|o| o.as_array().ok()).cloned().unwrap_or_default();
                    a.push(Object::Reference(leaf_id));
                    pt.set("Kids", Object::Array(a));
                }
            }
            match pt_id {
                Some(id) => ed.put(id, Object::Dictionary(pt)),
                None => root.set("ParentTree", Object::Dictionary(pt)),
            }
        }
    }
    root.set("ParentTreeNextKey", Object::Integer(key + 1));
    match st_loc {
        Loc::Indirect(id) => ed.put(id, Object::Dictionary(root)),
        _ => {
            catalog.set("StructTreeRoot", Object::Dictionary(root));
            *catalog_changed = true;
        }
    }
    Ok((key, objs))
}
