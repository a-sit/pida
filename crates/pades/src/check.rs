//! Invariante I6: Das Increment enthält nur erlaubte Änderungen (Plan §3.6).
//!
//! Die Objekte des Increments bestimmt die Prüfung **unabhängig vom Writer** aus dem Querverweis
//! der Ausgabe (`SourcePdf::object_locations`: Offset ≥ Länge des Originals bzw. Object Stream
//! im Increment). Jedes fortgeschriebene Objekt wird mit seiner Originalfassung verglichen und nur
//! zugelassen, wenn sich ausschließlich Schlüssel seiner Rolle ändern:
//!
//! | Rolle (aus dem Original bestimmt) | erlaubte Änderung |
//! |---|---|
//! | Katalog | `/AcroForm` (direkt: Regeln wie AcroForm), `/OutputIntents` (nur wenn vorher keine), `/StructTreeRoot` (direkt: Regeln wie StructTreeRoot) |
//! | AcroForm | `/Fields` nur verlängert, `/SigFlags` = alt \| 3, `/DR` nur ergänzt, `/DA` nur, wenn es fehlte bzw. leer war |
//! | Seite | `/Annots` nur verlängert (geteiltes indirektes Array → direkte Kopie), `/Tabs` |
//! | Seitenbaum-Wurzel (nur bei neuer Seite) | `/Kids` nur verlängert, `/Count` + 1 |
//! | befülltes Feld/Widget | `/V` (vorher leer, jetzt das neue Signaturdictionary), `/AP`, `/F` = alt \| 4, `/StructParent` (nur neu) |
//! | `/DR` und dessen `/Font`/`/XObject` (direkt oder indirekt) | nur hinzugefügte Schlüssel (`/FRM1` darf ersetzt werden) |
//! | StructTreeRoot | `/ParentTreeNextKey`, `/ParentTree` (neu bzw. nur verlängert) |
//! | erstes Strukturelement (Eltern des neuen StructElem) | `/K` nur verlängert |
//! | ParentTree-Wurzel | `/Nums`, `/Kids` nur verlängert |
//! | Arrays: eigenes `/Annots` einer Seite, `/Fields`, `/K` des Elternelements, `/Nums`/`/Kids` des ParentTree, `/Kids` der Seitenbaum-Wurzel (nur bei neuer Seite) | nur angehängt; **jedes andere** Array (z. B. `/Contents`) ist eine Verletzung |
//!
//! Alle übrigen fortgeschriebenen Objekte sind eine Verletzung (keine Heuristik nach Schlüsseln).
//!
//! Neue Objekte: genau ein Signaturdictionary (das neue), keine neuen Kataloge, Seitenbäume oder
//! Object Streams, eine neue Seite nur, wenn sie geplant war. Der Trailer behält `/Root`, `/Info`
//! und `/Encrypt`.

use std::collections::{BTreeSet, HashSet};

use pida_pdf_cos::{Dictionary, Object, ObjectId, ObjectLocation, SourcePdf};

use crate::signed::{Invariant, InvariantViolation, SignedMeta};
use crate::util::{append_only, as_ref_id, changed_keys, key_text, same};

fn violation(detail: impl Into<String>) -> InvariantViolation {
    InvariantViolation::new(Invariant::I6AllowedChanges, detail)
}

/// Steht bei `off` der Objektkopf `n g obj` (führender Leerraum erlaubt)?
fn header_at(data: &[u8], off: usize, id: ObjectId) -> bool {
    let Some(rest) = data.get(off..) else { return false };
    let start = rest.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(rest.len());
    let head = format!("{} {} obj", id.0, id.1);
    rest[start..].starts_with(head.as_bytes())
}

/// Rollen der Objekte, die ein Signier-Increment fortschreiben darf (aus dem **Original**).
#[derive(Default)]
struct Roles {
    catalog: Option<ObjectId>,
    acroform: Option<ObjectId>,
    pages_root: Option<ObjectId>,
    /// `/Kids`-Array der Seitenbaum-Wurzel (indirekt; nur bei neuer Seite verlängerbar)
    pages_kids: Option<ObjectId>,
    pages: HashSet<ObjectId>,
    /// `/DR` des AcroForms (indirekt)
    dr: Option<ObjectId>,
    /// `/DR /Font` bzw. `/DR /XObject` (indirekt)
    dr_children: HashSet<ObjectId>,
    struct_root: Option<ObjectId>,
    /// erstes Strukturelement (Elternelement des neuen StructElem, `build::first_struct_elem`)
    struct_parent: Option<ObjectId>,
    /// ParentTree-Wurzel (indirekt)
    parent_tree: Option<ObjectId>,
    /// Arrays, die nur verlängert werden dürfen: eigenes `/Annots` einer Seite, `/Fields`,
    /// `/K` des Elternelements, `/Nums` bzw. `/Kids` des ParentTree
    arrays: HashSet<ObjectId>,
}

fn resolved_dict<'a>(src: &'a SourcePdf, o: &'a Object) -> Option<&'a Dictionary> {
    match src.resolve(o).ok()? {
        Object::Dictionary(d) => Some(d),
        _ => None,
    }
}

fn collect_roles(src: &SourcePdf) -> Roles {
    let mut r = Roles { catalog: src.catalog_id().ok(), ..Roles::default() };
    r.pages = src.pages().iter().map(|p| p.object_id).collect();
    for p in src.pages() {
        // ein von mehreren Seiten geteiltes Array wird nie verlängert (`build::register_annot`)
        if let Some(a) = crate::build::own_annots_array(src, p) {
            r.arrays.insert(a);
        }
    }
    let Ok(catalog) = src.catalog() else { return r };
    r.pages_root = catalog.get(b"Pages").ok().and_then(as_ref_id);
    if let Some(root) = r.pages_root
        && let Ok(Object::Dictionary(d)) = src.get_object(root)
    {
        r.pages_kids = d.get(b"Kids").ok().and_then(as_ref_id);
    }
    if let Ok(acro_obj) = catalog.get(b"AcroForm") {
        r.acroform = as_ref_id(acro_obj);
        if let Some(acro) = resolved_dict(src, acro_obj) {
            if let Some(f) = acro.get(b"Fields").ok().and_then(as_ref_id) {
                r.arrays.insert(f);
            }
            if let Ok(dr_obj) = acro.get(b"DR") {
                r.dr = as_ref_id(dr_obj);
                if let Some(dr) = resolved_dict(src, dr_obj) {
                    for k in [&b"Font"[..], b"XObject"] {
                        if let Some(id) = dr.get(k).ok().and_then(as_ref_id) {
                            r.dr_children.insert(id);
                        }
                    }
                }
            }
        }
    }
    if let Ok(st_obj) = catalog.get(b"StructTreeRoot") {
        r.struct_root = as_ref_id(st_obj);
        if let Some(st) = resolved_dict(src, st_obj) {
            if let Some(k) = st.get(b"K").ok()
                && let Some(parent) = crate::build::first_struct_elem(src, k)
            {
                r.struct_parent = Some(parent);
                if let Ok(Object::Dictionary(pd)) = src.get_object(parent)
                    && let Some(kid_arr) = pd.get(b"K").ok().and_then(as_ref_id)
                {
                    r.arrays.insert(kid_arr);
                }
            }
            if let Ok(pt_obj) = st.get(b"ParentTree") {
                r.parent_tree = as_ref_id(pt_obj);
                if let Some(pt) = resolved_dict(src, pt_obj) {
                    for k in [&b"Nums"[..], b"Kids"] {
                        if let Some(id) = pt.get(k).ok().and_then(as_ref_id) {
                            r.arrays.insert(id);
                        }
                    }
                }
            }
        }
    }
    r
}

/// Prüft ein fortgeschriebenes Dictionary: nur `allowed` ändern sich.
fn only_keys(id: ObjectId, old: &Dictionary, new: &Dictionary, allowed: &[&[u8]]) -> Result<(), InvariantViolation> {
    for k in changed_keys(old, new) {
        if !allowed.contains(&k.as_slice()) {
            return Err(violation(format!("Objekt {} {}: Schlüssel {} geändert", id.0, id.1, key_text(&k))));
        }
    }
    Ok(())
}

/// Nur hinzugefügte Schlüssel (bzw. ersetzbare `replaceable`).
fn only_added(
    id: ObjectId,
    old: &Dictionary,
    new: &Dictionary,
    replaceable: &[&[u8]],
) -> Result<(), InvariantViolation> {
    for (k, v) in old.iter() {
        match new.get(k) {
            Ok(w) if same(v, w) => {}
            Ok(_) if replaceable.contains(&k.as_slice()) => {}
            _ => return Err(violation(format!("Objekt {} {}: Schlüssel {} geändert", id.0, id.1, key_text(k)))),
        }
    }
    Ok(())
}

/// Arrays in einem Dictionary nur verlängert (direkt eingebettet). Ein indirektes Array darf
/// durch eine direkte, verlängerte Kopie ersetzt werden (geteiltes `/Annots`).
fn arrays_appended(src: &SourcePdf, old: &Dictionary, new: &Dictionary, keys: &[&[u8]]) -> bool {
    keys.iter().all(|k| match (old.get(k), new.get(k)) {
        (Ok(Object::Array(a)), Ok(Object::Array(b))) => append_only(a, b),
        (Ok(Object::Reference(r)), Ok(Object::Array(b))) => {
            matches!(src.get_object(*r), Ok(Object::Array(a)) if append_only(a, b))
        }
        // ein direkter Wert, der kein Array ist (beschädigt), wird wie fehlend durch ein neues
        // Array mit genau dem neuen Eintrag ersetzt (`build::update_acroform`, PDFBox)
        (Ok(x), Ok(Object::Array(b))) if !matches!(x, Object::Array(_) | Object::Reference(_)) => b.len() == 1,
        (Err(_), _) => true,
        (Ok(a), Ok(b)) => same(a, b),
        (Ok(_), Err(_)) => false,
    })
}

fn int_value(src: &SourcePdf, d: &Dictionary, key: &[u8]) -> Option<i64> {
    d.get(key).ok().and_then(|o| crate::util::int_of(src, o))
}

/// `/DR`: nur hinzugefügte Einträge; in direkt eingebetteten `/Font`/`/XObject` ebenso
/// (`/FRM1` darf ersetzt werden, PDFBox `assignAcroFormDefaultResource`). Ein `/Font`, das kein
/// Dictionary ist (auch nicht über einen Verweis), darf durch ein neues direktes Dictionary ersetzt
/// werden (`build::fix_dr`, wie PDFBox).
fn check_dr(src: &SourcePdf, id: ObjectId, old: &Dictionary, new: &Dictionary) -> Result<(), InvariantViolation> {
    for (k, v) in old.iter() {
        match (v, new.get(k)) {
            (_, Ok(w)) if same(v, w) => {}
            (Object::Dictionary(a), Ok(Object::Dictionary(b))) if k == b"Font" || k == b"XObject" => {
                only_added(id, a, b, &[b"FRM1"])?
            }
            (_, Ok(Object::Dictionary(_))) if k == b"Font" && !matches!(src.resolve(v), Ok(Object::Dictionary(_))) => {}
            _ => return Err(violation(format!("Objekt {} {}: /DR-Eintrag {} geändert", id.0, id.1, key_text(k)))),
        }
    }
    Ok(())
}

/// AcroForm (direkt oder indirekt): `/Fields` nur verlängert, `/SigFlags` = alt | 3, `/DR` nur
/// ergänzt, `/DA` nur gesetzt, wenn es fehlte bzw. leer war.
fn check_acroform(src: &SourcePdf, id: ObjectId, o: &Dictionary, n: &Dictionary) -> Result<(), InvariantViolation> {
    only_keys(id, o, n, &[b"Fields", b"SigFlags", b"DR", b"DA"])?;
    if !arrays_appended(src, o, n, &[b"Fields"]) {
        return Err(violation("/Fields nicht nur verlängert"));
    }
    if int_value(src, n, b"SigFlags") != Some(int_value(src, o, b"SigFlags").unwrap_or(0) | 3) {
        return Err(violation("/SigFlags unerwartet"));
    }
    match (o.get(b"DR").ok(), n.get(b"DR").ok()) {
        (Some(a), Some(b)) if same(a, b) => {}
        (None, Some(Object::Dictionary(_)) | None) => {}
        (Some(Object::Dictionary(a)), Some(Object::Dictionary(b))) => check_dr(src, id, a, b)?,
        // Verweis auf ein fehlendes bzw. kaputtes /DR: neues direktes /DR (`build::update_acroform`)
        (Some(Object::Reference(r)), Some(Object::Dictionary(_)))
            if !matches!(src.get_object(*r), Ok(Object::Dictionary(_))) => {}
        _ => return Err(violation("/DR des AcroForms unerwartet ersetzt")),
    }
    // wie `build::update_acroform` (`str_key`): ein `/DA`, das kein String ist, gilt als fehlend
    let old_da_empty =
        !matches!(o.get(b"DA").ok().map(|x| src.resolve(x)), Some(Ok(Object::String(s, _))) if !s.is_empty());
    if !old_da_empty && !matches!((o.get(b"DA"), n.get(b"DA")), (Ok(a), Ok(b)) if same(a, b)) {
        return Err(violation("/DA des AcroForms geändert"));
    }
    Ok(())
}

/// ParentTree-Knoten: nur `/Nums` bzw. `/Kids` verlängert.
fn check_number_tree(src: &SourcePdf, id: ObjectId, o: &Dictionary, n: &Dictionary) -> Result<(), InvariantViolation> {
    only_keys(id, o, n, &[b"Nums", b"Kids"])?;
    if !arrays_appended(src, o, n, &[b"Nums", b"Kids"]) {
        return Err(violation("Number Tree nicht nur verlängert"));
    }
    Ok(())
}

/// StructTreeRoot (direkt oder indirekt): `/ParentTreeNextKey`, `/ParentTree` (neu bzw. nur
/// verlängert).
fn check_struct_root(src: &SourcePdf, id: ObjectId, o: &Dictionary, n: &Dictionary) -> Result<(), InvariantViolation> {
    only_keys(id, o, n, &[b"ParentTreeNextKey", b"ParentTree"])?;
    match (o.get(b"ParentTree").ok(), n.get(b"ParentTree").ok()) {
        (Some(a), Some(b)) if same(a, b) => Ok(()),
        (None, Some(Object::Reference(_))) => Ok(()),
        (Some(Object::Dictionary(a)), Some(Object::Dictionary(b))) => check_number_tree(src, id, a, b),
        _ => Err(violation("/ParentTree unerwartet ersetzt")),
    }
}

/// `/K` des Elternelements: Array nur verlängert, einzelnes Kind → Array mit dem alten Kind
/// zuerst, fehlend → neues Array; indirektes Array bleibt als Verweis.
fn struct_kids_appended(o: &Dictionary, n: &Dictionary) -> bool {
    match (o.get(b"K").ok(), n.get(b"K").ok()) {
        (Some(Object::Array(a)), Some(Object::Array(b))) => append_only(a, b),
        (Some(a), Some(b)) if same(a, b) => true,
        (None | Some(Object::Null), Some(Object::Array(_))) => true,
        (Some(x), Some(Object::Array(b))) => b.len() == 2 && same(&b[0], x),
        _ => false,
    }
}

/// Katalog: `/AcroForm` (direkt: Regeln wie AcroForm), `/OutputIntents` (nur wenn vorher
/// keine), `/StructTreeRoot` (direkt: Regeln wie StructTreeRoot).
fn check_catalog(src: &SourcePdf, id: ObjectId, o: &Dictionary, n: &Dictionary) -> Result<(), InvariantViolation> {
    let had_intents = matches!(o.get(b"OutputIntents").ok().and_then(|x| src.resolve(x).ok()), Some(Object::Array(a)) if !a.is_empty());
    let mut allowed: Vec<&[u8]> = vec![b"AcroForm", b"StructTreeRoot"];
    if !had_intents {
        allowed.push(b"OutputIntents");
    }
    only_keys(id, o, n, &allowed)?;
    match (o.get(b"AcroForm").ok(), n.get(b"AcroForm").ok()) {
        (Some(a), Some(b)) if same(a, b) => {}
        (None, None) => {}
        (old, Some(Object::Dictionary(nd))) => {
            let empty = Dictionary::new();
            let od = match old {
                None => &empty,
                Some(Object::Dictionary(d)) => d,
                // Verweis auf ein fehlendes bzw. kaputtes AcroForm bzw. ein Wert, der kein
                // Dictionary ist: neues direktes (`build::update_acroform`, PDFBox)
                Some(x) if !matches!(src.resolve(x), Ok(Object::Dictionary(_))) => &empty,
                Some(_) => return Err(violation("/AcroForm des Katalogs unerwartet ersetzt")),
            };
            check_acroform(src, id, od, nd)?;
        }
        _ => return Err(violation("/AcroForm des Katalogs unerwartet ersetzt")),
    }
    match (o.get(b"StructTreeRoot").ok(), n.get(b"StructTreeRoot").ok()) {
        (Some(a), Some(b)) if same(a, b) => Ok(()),
        (None, None) => Ok(()),
        (Some(Object::Dictionary(a)), Some(Object::Dictionary(b))) => check_struct_root(src, id, a, b),
        _ => Err(violation("/StructTreeRoot des Katalogs unerwartet ersetzt")),
    }
}

pub(crate) fn allowed_changes(src: &SourcePdf, new: &SourcePdf, meta: &SignedMeta) -> Result<(), InvariantViolation> {
    let base = src.len();
    let build = &meta.prepared.build;
    let roles = collect_roles(src);

    // Trailer
    for key in [&b"Root"[..], b"Info", b"Encrypt"] {
        let a = src.trailer().get(key).ok();
        let b = new.trailer().get(key).ok();
        let equal = match (a, b) {
            (None, None) => true,
            (Some(x), Some(y)) => same(x, y),
            _ => false,
        };
        if !equal {
            return Err(violation(format!("Trailer-Schlüssel {} geändert", key_text(key))));
        }
    }

    // Objekte des Increments laut Querverweis der Ausgabe
    let locations = new.object_locations();
    let total = new.len();
    let mut in_increment: BTreeSet<ObjectId> = BTreeSet::new();
    let mut new_streams: HashSet<u32> = HashSet::new();
    for (id, loc) in &locations {
        // nur Offsets innerhalb des Increments, an denen auch der Objektkopf steht (defekte
        // Originale können Offsets hinter dem Dateiende oder mitten ins Increment eintragen –
        // solche Einträge gehören nicht zum Increment)
        if let ObjectLocation::Offset(off) = loc
            && (base..total).contains(off)
            && header_at(new.bytes(), *off as usize, *id)
        {
            in_increment.insert(*id);
            new_streams.insert(id.0);
        }
    }
    for (id, loc) in &locations {
        if let ObjectLocation::InStream { stream, .. } = loc
            && new_streams.contains(stream)
        {
            return Err(violation(format!("Objekt {} {} liegt in einem neuen Object Stream", id.0, id.1)));
        }
    }
    let old_ids: HashSet<ObjectId> = src.document().objects.keys().copied().collect();

    let mut sig_dicts = 0usize;
    let mut new_pages = 0usize;
    for id in &in_increment {
        let Ok(obj) = new.get_object(*id) else {
            // xref-Stream des Increments (lopdf hält ihn nicht im Objektmodell); ein
            // fortgeschriebenes Objekt muss dagegen lesbar sein, sonst bliebe es ungeprüft
            if old_ids.contains(id) {
                return Err(violation(format!("fortgeschriebenes Objekt {} {} nicht lesbar", id.0, id.1)));
            }
            continue;
        };
        let dict = match obj {
            Object::Dictionary(d) => Some(d),
            Object::Stream(s) => Some(&s.dict),
            _ => None,
        };
        if dict.is_some_and(|d| d.has_type(b"Sig") || d.has_type(b"DocTimeStamp")) {
            if *id != build.sig_dict_id {
                return Err(violation(format!("weiteres Signaturdictionary {} {}", id.0, id.1)));
            }
            sig_dicts += 1;
            continue;
        }
        if !old_ids.contains(id) {
            // neues Objekt
            if let Some(d) = dict {
                if d.has_type(b"Catalog") || d.has_type(b"Pages") || d.has_type(b"ObjStm") {
                    return Err(violation(format!("unerlaubtes neues Objekt {} {}", id.0, id.1)));
                }
                if d.has_type(b"XRef") {
                    continue;
                }
                if d.has_type(b"Page") {
                    if build.new_page_id != Some(*id) {
                        return Err(violation(format!("ungeplante neue Seite {} {}", id.0, id.1)));
                    }
                    new_pages += 1;
                }
            }
            continue;
        }
        // fortgeschriebenes Objekt
        let old = src.get_object(*id).map_err(|_| violation("Original-Objekt nicht lesbar"))?;
        check_changed(src, *id, old, obj, &roles, meta)?;
    }
    if sig_dicts != 1 {
        return Err(violation(format!("{sig_dicts} Signaturdictionaries im Increment statt einem")));
    }
    if build.new_page_id.is_some() && new_pages != 1 {
        return Err(violation("geplante neue Seite fehlt im Increment"));
    }
    Ok(())
}

fn check_changed(
    src: &SourcePdf,
    id: ObjectId,
    old: &Object,
    new: &Object,
    roles: &Roles,
    meta: &SignedMeta,
) -> Result<(), InvariantViolation> {
    let build = &meta.prepared.build;
    match (old, new) {
        (Object::Array(a), Object::Array(b)) => {
            let allowed = roles.arrays.contains(&id) || (Some(id) == roles.pages_kids && build.new_page_id.is_some());
            if !allowed {
                return Err(violation(format!("Array {} {} unerwartet fortgeschrieben", id.0, id.1)));
            }
            if !append_only(a, b) {
                return Err(violation(format!("Array {} {} nicht nur verlängert", id.0, id.1)));
            }
            Ok(())
        }
        (Object::Dictionary(o), Object::Dictionary(n)) => {
            if Some(id) == roles.catalog {
                return check_catalog(src, id, o, n);
            }
            if Some(id) == roles.acroform {
                return check_acroform(src, id, o, n);
            }
            if roles.pages.contains(&id) {
                only_keys(id, o, n, &[b"Annots", b"Tabs"])?;
                if !arrays_appended(src, o, n, &[b"Annots"]) {
                    return Err(violation(format!("Seite {} {}: /Annots nicht nur verlängert", id.0, id.1)));
                }
                return Ok(());
            }
            if Some(id) == roles.pages_root && build.new_page_id.is_some() {
                only_keys(id, o, n, &[b"Kids", b"Count"])?;
                if !arrays_appended(src, o, n, &[b"Kids"]) {
                    return Err(violation("/Kids der Seitenbaum-Wurzel nicht nur verlängert"));
                }
                let before = int_value(src, o, b"Count").unwrap_or(src.page_count() as i64);
                if before.checked_add(1).is_none_or(|expected| int_value(src, n, b"Count") != Some(expected)) {
                    return Err(violation("/Count der Seitenbaum-Wurzel unerwartet"));
                }
                return Ok(());
            }
            if !build.new_field && (id == build.field_id || id == build.widget_id) {
                only_keys(id, o, n, &[b"V", b"AP", b"F", b"StructParent"])?;
                if id == build.field_id {
                    let had_value = o.get(b"V").is_ok_and(|v| !matches!(v, Object::Null));
                    if had_value || n.get(b"V").ok().and_then(as_ref_id) != Some(build.sig_dict_id) {
                        return Err(violation("/V des Zielfelds unerwartet"));
                    }
                }
                let changed = changed_keys(o, n);
                if changed.iter().any(|k| k == b"F")
                    && int_value(src, n, b"F") != Some(int_value(src, o, b"F").unwrap_or(0) | 4)
                {
                    return Err(violation("/F des Widgets unerwartet"));
                }
                if o.has(b"StructParent") && changed.iter().any(|k| k == b"StructParent") {
                    return Err(violation("/StructParent des Widgets geändert"));
                }
                return Ok(());
            }
            if Some(id) == roles.dr {
                return check_dr(src, id, o, n);
            }
            if roles.dr_children.contains(&id) {
                return only_added(id, o, n, &[b"FRM1"]);
            }
            if Some(id) == roles.struct_root {
                return check_struct_root(src, id, o, n);
            }
            if Some(id) == roles.struct_parent {
                only_keys(id, o, n, &[b"K"])?;
                if !struct_kids_appended(o, n) {
                    return Err(violation("/K des Strukturelements nicht nur verlängert"));
                }
                return Ok(());
            }
            if Some(id) == roles.parent_tree {
                return check_number_tree(src, id, o, n);
            }
            let ty = o.get(b"Type").ok().and_then(|t| t.as_name().ok()).map(key_text).unwrap_or_else(|| "-".into());
            let keys: Vec<String> = changed_keys(o, n).iter().map(|k| key_text(k)).collect();
            Err(violation(format!(
                "Objekt {} {} (Typ {ty}) unerwartet fortgeschrieben, Schlüssel {}",
                id.0,
                id.1,
                keys.join(" ")
            )))
        }
        _ if same(old, new) => Ok(()),
        _ => Err(violation(format!("Objekt {} {} unerwartet fortgeschrieben", id.0, id.1))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::BuildMeta;
    use crate::prepared::PreparedMeta;
    use pida_cades::{Certificate, CmsProfile};
    use pida_pdf_cos::{Limits, Stream, StringFormat};
    use pida_pdf_incremental::{IncrementBuilder, IncrementOptions, SignatureDictSpec};

    fn testdata(rel: &str) -> std::path::PathBuf {
        std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata")).join(rel)
    }

    fn simple() -> SourcePdf {
        SourcePdf::open_file(&testdata("pdf/pdfas/pdf-as-legacy/simple.pdf"), &Limits::default()).unwrap()
    }

    fn name(s: &str) -> Object {
        Object::Name(s.as_bytes().to_vec())
    }

    /// Testdokument als Increment über `src` (ohne Signatur).
    fn derive(src: &SourcePdf, f: impl FnOnce(&mut IncrementBuilder<'_>, &mut Dictionary)) -> SourcePdf {
        let mut b = IncrementBuilder::new(src, IncrementOptions::default());
        let mut catalog = src.catalog().unwrap().clone();
        f(&mut b, &mut catalog);
        b.update_object(src.catalog_id().unwrap(), Object::Dictionary(catalog)).unwrap();
        SourcePdf::open(b.build().unwrap().to_vec(src.bytes()), &Limits::default()).unwrap()
    }

    /// Signier-Increment (Signaturdictionary + `f`) über `src`, dann I6.
    fn check(src: &SourcePdf, f: impl FnOnce(&mut IncrementBuilder<'_>)) -> Result<(), InvariantViolation> {
        let mut b = IncrementBuilder::new(src, IncrementOptions::default());
        let sig = b.add_signature_dictionary(SignatureDictSpec::default()).unwrap();
        f(&mut b);
        let inc = b.build().unwrap();
        let new = SourcePdf::open(inc.to_vec(src.bytes()), &Limits::default()).unwrap();
        let pem = pida_fs::read_to_string(testdata("keystores/test-rsa2048.crt.pem")).unwrap();
        let meta = SignedMeta {
            prepared: PreparedMeta {
                build: BuildMeta {
                    field_name: "x".into(),
                    field_id: (99_999, 0),
                    widget_id: (99_999, 0),
                    sig_dict_id: sig,
                    new_page_id: None,
                    page_index: Some(0),
                    new_field: true,
                    notices: Vec::new(),
                },
                cms_profile: CmsProfile::PdfasCompat,
                include_intermediates: false,
                expected_signer: None,
                page_count_before: src.page_count(),
            },
            byte_range: inc.byte_range,
            contents_range: (inc.contents_range.start, inc.contents_range.end),
            signer: Certificate::from_pem(&pem).unwrap(),
        };
        allowed_changes(src, &new, &meta)
    }

    fn page0(src: &SourcePdf) -> (ObjectId, Dictionary) {
        let id = src.pages()[0].object_id;
        (id, src.get_object(id).unwrap().as_dict().unwrap().clone())
    }

    #[test]
    fn appending_to_an_unrelated_array_is_rejected() {
        // Seiteninhalt als indirektes Array: ein zusätzlicher Content-Stream ist kein Signieren
        let base = simple();
        let (pid, mut pd) = page0(&base);
        let src = derive(&base, |b, _| {
            let arr = b.add_object(Object::Array(vec![pd.get(b"Contents").unwrap().clone()]));
            pd.set("Contents", Object::Reference(arr));
            b.update_object(pid, Object::Dictionary(pd)).unwrap();
        });
        let (_, pd) = page0(&src);
        let arr = as_ref_id(pd.get(b"Contents").unwrap()).unwrap();
        let v = check(&src, |b| {
            let extra = b.add_object(Object::Stream(Stream::new(Dictionary::new(), b"0 0 m 100 100 l S".to_vec())));
            let mut a = src.get_object(arr).unwrap().as_array().unwrap().clone();
            a.push(Object::Reference(extra));
            b.update_object(arr, Object::Array(a)).unwrap();
        })
        .unwrap_err();
        assert!(v.detail.contains("unerwartet"), "{v}");
    }

    /// Dokument mit direktem AcroForm: nicht-terminales Feld mit `/Kids`, `/Fields` indirekt,
    /// `/DR` indirekt mit direktem `/Font` ohne `/Helv`.
    fn form_doc() -> (SourcePdf, ObjectId, ObjectId, ObjectId) {
        let base = simple();
        let mut ids = ((0, 0), (0, 0), (0, 0));
        let src = derive(&base, |b, catalog| {
            let mut parent = Dictionary::new();
            parent.set("T", Object::String(b"Gruppe".to_vec(), StringFormat::Literal));
            parent.set("Kids", Object::Array(Vec::new()));
            let parent = b.add_object(Object::Dictionary(parent));
            let fields = b.add_object(Object::Array(vec![Object::Reference(parent)]));
            let mut f1 = Dictionary::new();
            f1.set("Type", name("Font"));
            f1.set("Subtype", name("Type1"));
            f1.set("BaseFont", name("Courier"));
            let mut fonts = Dictionary::new();
            fonts.set("F1", Object::Dictionary(f1));
            let mut dr = Dictionary::new();
            dr.set("Font", Object::Dictionary(fonts));
            let dr = b.add_object(Object::Dictionary(dr));
            let mut acro = Dictionary::new();
            acro.set("Fields", Object::Reference(fields));
            acro.set("DR", Object::Reference(dr));
            catalog.set("AcroForm", Object::Dictionary(acro));
            ids = (parent, fields, dr);
        });
        (src, ids.0, ids.1, ids.2)
    }

    #[test]
    fn non_terminal_field_kids_are_not_a_number_tree() {
        let (src, parent, _, _) = form_doc();
        let v = check(&src, |b| {
            let w = b.add_object(Object::Dictionary(Dictionary::new()));
            let mut p = src.get_object(parent).unwrap().as_dict().unwrap().clone();
            p.set("Kids", Object::Array(vec![Object::Reference(w)]));
            b.update_object(parent, Object::Dictionary(p)).unwrap();
        })
        .unwrap_err();
        assert!(v.detail.contains("unerwartet fortgeschrieben"), "{v}");
    }

    #[test]
    fn direct_acroform_contents_are_checked() {
        let (src, _, fields, _) = form_doc();
        let cid = src.catalog_id().unwrap();
        let with_acro = |f: &dyn Fn(&mut Dictionary)| {
            check(&src, |b| {
                let mut cat = src.catalog().unwrap().clone();
                let mut acro = cat.get(b"AcroForm").unwrap().as_dict().unwrap().clone();
                f(&mut acro);
                cat.set("AcroForm", Object::Dictionary(acro));
                b.update_object(cid, Object::Dictionary(cat)).unwrap();
            })
        };
        let v = with_acro(&|a| a.set("NeedAppearances", Object::Boolean(true))).unwrap_err();
        assert!(v.detail.contains("/NeedAppearances"), "{v}");
        let v = with_acro(&|a| a.set("SigFlags", Object::Integer(1))).unwrap_err();
        assert!(v.detail.contains("/SigFlags"), "{v}");
        let v = with_acro(&|a| a.set("Fields", Object::Array(Vec::new()))).unwrap_err();
        assert!(v.detail.contains("/Fields"), "{v}");
        let v = with_acro(&|a| a.set("XFA", Object::Array(Vec::new()))).unwrap_err();
        assert!(v.detail.contains("/XFA"), "{v}");
        with_acro(&|a| {
            a.set("SigFlags", Object::Integer(3));
            a.set("DA", Object::String(b"/Helv 0 Tf 0 g ".to_vec(), StringFormat::Literal));
        })
        .expect("Signier-Änderungen am direkten AcroForm");
        // Rollen-Array durch etwas anderes ersetzt
        let v = check(&src, |b| b.update_object(fields, Object::Dictionary(Dictionary::new())).unwrap()).unwrap_err();
        assert!(v.detail.contains("unerwartet"), "{v}");
    }

    #[test]
    fn indirect_dr_with_direct_font_may_gain_default_fonts() {
        let (src, _, _, dr) = form_doc();
        let mut d = src.get_object(dr).unwrap().as_dict().unwrap().clone();
        let mut fonts = d.get(b"Font").unwrap().as_dict().unwrap().clone();
        fonts.set("Helv", Object::Dictionary(Dictionary::new()));
        d.set("Font", Object::Dictionary(fonts.clone()));
        let added = d.clone();
        check(&src, |b| b.update_object(dr, Object::Dictionary(added)).unwrap()).expect("/Helv ergänzt");
        // vorhandenen Font ersetzen ist keine Ergänzung
        fonts.set("F1", Object::Integer(1));
        d.set("Font", Object::Dictionary(fonts));
        let v = check(&src, |b| b.update_object(dr, Object::Dictionary(d)).unwrap()).unwrap_err();
        assert!(v.detail.contains("/F1"), "{v}");
    }
}
