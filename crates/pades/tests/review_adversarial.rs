//! Adversarialer Review (Welle 2, Paket I1): Randfälle und Angriffsflächen der Signier-Pipeline,
//! jeweils mit externen Orakeln gegengeprüft, wo es eine Ausgabe gibt (qpdf, openssl, pyHanko,
//! PDFium über `run_case`; fehlende Werkzeuge überspringen die jeweilige Prüfung).
//!
//! - `finish_cms` erzwingt das Zertifikat aus `prepare` auch gegen eine abweichende Erwartung des
//!   Aufrufers (PDF-AS „Certificates missmatch!“, Plan §3.6 I3).
//! - Die Begleitdatei von `Prepared::persist` ist integritätsgeschützt (Erwartungen für I3/I5/I6).
//! - K2-Befunde mit Vorsignaturen lehnt auch `prepare` ab (Plan §6.1 K2 → K5).
//! - Ein von mehreren Seiten geteiltes `/Annots`-Array wird nicht verlängert.
//! - CMS genau so groß wie die Reserve passt, ein Byte mehr nicht (docs/analyse/03 §9.4).
//! - Vorhandenes Feld mit eigenem Widget-Kind und hierarchischem Namen (Plan §6.4 Korpusfälle).
//! - Unlesbares Signaturdictionary zählt als Vorsignatur (keine neue Seite, Befund).

mod common;

use common::*;
use pida_cades::{CmsError, CmsProfile, Expect, ExpectedSigner};
use pida_pades::{
    CheckContext, FieldTarget, FinishError, PadesError, Prepared, Reserve, SignPlan, analyze, codes, empty_fields,
    prepare,
};
use pida_pdf_cos::{Dictionary, Limits, Object, SourcePdf, StringFormat};
use pida_pdf_incremental::{IncrementBuilder, IncrementOptions};

fn name(s: &str) -> Object {
    Object::Name(s.as_bytes().to_vec())
}

fn text(s: &str) -> Object {
    Object::String(s.as_bytes().to_vec(), StringFormat::Literal)
}

fn simple() -> SourcePdf {
    open(&testdata().join("pdf/pdfas/pdf-as-legacy/simple.pdf"))
}

/// Increment über `src`: der Callback bekommt Builder und Katalog (wird danach fortgeschrieben).
fn derive(src: &SourcePdf, f: impl FnOnce(&mut IncrementBuilder<'_>, &mut Dictionary)) -> SourcePdf {
    let mut b = IncrementBuilder::new(src, IncrementOptions::default());
    let mut catalog = src.catalog().unwrap().clone();
    f(&mut b, &mut catalog);
    b.update_object(src.catalog_id().unwrap(), Object::Dictionary(catalog)).unwrap();
    let inc = b.build().unwrap();
    SourcePdf::open(inc.to_vec(src.bytes()), &Limits::default()).unwrap()
}

/// Ein CMS eines fremden Schlüssels über die ByteRange-Daten wird auch dann abgelehnt, wenn der
/// Aufrufer genau diesen fremden Signer erwartet: das Zertifikat aus `prepare` (Signaturblock,
/// `/Name`) ist maßgeblich.
#[test]
fn finish_cms_enforces_prepare_time_certificate() {
    let src = simple();
    let signer_a = KeystoreSigner::open("test-rsa2048");
    let mut signer_b = KeystoreSigner::open("test-ecp256");
    let plan = SignPlan::invisible(pida_pades::local_signing_time());
    let prepared = prepare(&src, &plan, Some(&signer_a.chain)).unwrap();
    let cms = signer_b.cms_for(&prepared);
    let expect = Expect { signer: ExpectedSigner::Certificate(signer_b.chain.signer.clone()), ..Expect::default() };
    let err = prepared.finish_cms(&cms, &expect).unwrap_err();
    assert!(matches!(err, FinishError::Cms(CmsError::UnexpectedSigner)), "{err:?}");
    assert_eq!(err.code(), "cms-unexpected-signer");

    // ohne Erwartung des Aufrufers ebenso
    let prepared = prepare(&src, &plan, Some(&signer_a.chain)).unwrap();
    let cms = signer_b.cms_for(&prepared);
    let err = prepared.finish_cms(&cms, &Expect::default()).unwrap_err();
    assert!(matches!(err, FinishError::Cms(CmsError::UnexpectedSigner)), "{err:?}");

    // ohne Zertifikat in `prepare` (Modell B, unsichtbar): Erwartung des Aufrufers gilt
    let mut plan_b = plan.clone();
    plan_b.reserve = Reserve::ModelB;
    let prepared = prepare(&src, &plan_b, None).unwrap();
    let cms = signer_b.cms_for(&prepared);
    let signed = prepared.finish_cms(&cms, &expect).expect("Signer wie erwartet");
    let ctx = CheckContext { source: &src, expected_signer: None, renderer: None };
    signed.self_check(&ctx).unwrap();
    // die Selbstprüfung mit fremder Erwartung schlägt fehl (I3)
    let ctx = CheckContext { source: &src, expected_signer: Some(&signer_a.chain.signer), renderer: None };
    assert_eq!(signed.self_check(&ctx).unwrap_err().invariant, pida_pades::Invariant::I3Cms);
}

/// Manipulierte Begleitdatei (erwarteter Signer, Feldname, neue Seite) wird beim Fortsetzen
/// erkannt, nicht nur manipulierte Offsets.
#[test]
fn persisted_sidecar_is_integrity_protected() {
    let dir = tempfile::tempdir().unwrap();
    let src = simple();
    let signer = KeystoreSigner::open("test-rsa2048");
    let other = KeystoreSigner::open("test-ecp256");
    let plan = SignPlan::invisible(pida_pades::local_signing_time());
    let prepared = prepare(&src, &plan, Some(&signer.chain)).unwrap();
    let handle = prepared.persist(dir.path()).unwrap();
    let json_path = handle.path.with_extension("json");
    let original = std::fs::read_to_string(&json_path).unwrap();
    let tamper = |f: &dyn Fn(&mut serde_json::Value)| {
        let mut v: serde_json::Value = serde_json::from_str(&original).unwrap();
        f(&mut v);
        std::fs::write(&json_path, v.to_string()).unwrap();
        let r = Prepared::restore(&handle, &src);
        assert!(matches!(r, Err(PadesError::InvalidState(_))), "{r:?}");
    };
    let other_hex: String = other.chain.signer.as_der().iter().map(|b| format!("{b:02x}")).collect();
    tamper(&|v| v["meta"]["expected_signer"] = serde_json::json!(other_hex));
    tamper(&|v| v["meta"]["expected_signer"] = serde_json::Value::Null);
    tamper(&|v| v["meta"]["page_count_before"] = serde_json::json!(5));
    tamper(&|v| v["meta"]["build"]["new_page_id"] = serde_json::json!([999, 0]));
    tamper(&|v| v["profile_id"] = serde_json::json!("AMTSSIGNATURBLOCK_DE_RECOMMENDED"));
    // Fehlermeldungen enthalten keine Inhalte der Begleitdatei
    std::fs::write(&json_path, "{\"format\": \"PDF-AS Signatur Max Mustermann\"").unwrap();
    let e = Prepared::restore(&handle, &src).unwrap_err();
    assert!(!e.to_string().contains("Mustermann"), "{e}");
    std::fs::write(&json_path, &original).unwrap();
    assert!(Prepared::restore(&handle, &src).is_ok());
    Prepared::discard_persisted(&handle).unwrap();
}

/// Fremdinhalt nach `%%EOF` bei bereits signiertem Dokument ist K5 – auch `prepare` lehnt ab.
#[test]
fn k2_findings_on_signed_documents_are_rejected_by_prepare() {
    let path = testdata().join("pdf/pdfas/pdf-as-pdfbox-2/new_qr_2_signed.pdf");
    let mut bytes = std::fs::read(&path).unwrap();
    bytes.extend_from_slice(b"Dies ist Fremdinhalt nach dem Dateiende, der keine PDF-Syntax ist.\n");
    let src = SourcePdf::open(bytes, &Limits::default()).unwrap();
    let report = analyze(&src).unwrap();
    assert!(report.has_finding(codes::TAIL_GARBAGE), "{:?}", report.findings);
    assert_eq!(report.class, pida_pades::AcceptanceClass::K5Rejected);
    let signer = KeystoreSigner::open("test-rsa2048");
    let plan = SignPlan::invisible(pida_pades::local_signing_time());
    match prepare(&src, &plan, Some(&signer.chain)) {
        Err(PadesError::Rejected(c)) => assert_eq!(c, codes::TAIL_GARBAGE),
        other => panic!("nicht abgelehnt: {other:?}"),
    }
}

/// Zwei Seiten teilen ein indirektes `/Annots`-Array: das Widget erscheint nur auf der Zielseite.
#[test]
fn shared_annots_array_is_not_extended_for_other_pages() {
    let or = Oracles::detect();
    let work = tempfile::tempdir().unwrap();
    let base = open(&testdata().join("pdf/pdfas-suites/POS_LAST/example.pdf"));
    assert_eq!(base.page_count(), 2);
    let pages: Vec<_> = base.pages().iter().map(|p| p.object_id).collect();
    let doc = derive(&base, |b, _| {
        let arr = b.add_object(Object::Array(Vec::new()));
        for id in &pages {
            let mut d = base.get_object(*id).unwrap().as_dict().unwrap().clone();
            d.set("Annots", Object::Reference(arr));
            b.update_object(*id, Object::Dictionary(d)).unwrap();
        }
    });
    assert!(doc.pages()[0].annots_indirect.is_some());
    assert_eq!(doc.pages()[0].annots_indirect, doc.pages()[1].annots_indirect);
    let bytes = doc.bytes().to_vec();
    let orig = work.path().join("shared-annots.pdf");
    std::fs::write(&orig, &bytes).unwrap();
    let mut signer = KeystoreSigner::open("test-ecp256");
    let adjust = |opts: &mut pida_pades::PlanOptions, _: &pida_pades::DocReport, _: &SourcePdf| {
        opts.position = pida_placement::TablePos::parse("p:1;x:100;y:700").unwrap();
    };
    let mut outcomes = vec![run_case(
        &or,
        &mut signer,
        &orig,
        &bytes,
        "SIGNATURBLOCK_SMALL_DE",
        CmsProfile::PdfasCompat,
        &adjust,
        work.path(),
        "shared-annots",
        "geteiltes /Annots",
    )];
    let trust: Vec<_> = KEYS.iter().map(|(_, s)| key_cert_pem(s)).collect();
    pyhanko_all(&or, &mut outcomes, &trust);
    assert_all_ok(&outcomes);
    let out = SourcePdf::open(outcomes[0].signed.as_ref().unwrap().bytes(), &Limits::default()).unwrap();
    let field = out.signature_fields().unwrap().into_iter().find(|f| f.signed).unwrap();
    let annots_of = |i: usize| -> Vec<Object> {
        let d = out.get_object(out.pages()[i].object_id).unwrap().as_dict().unwrap().clone();
        out.resolve(d.get(b"Annots").unwrap()).unwrap().as_array().unwrap().clone()
    };
    assert!(annots_of(0).contains(&Object::Reference(field.field_id)), "Zielseite ohne Widget");
    assert!(!annots_of(1).contains(&Object::Reference(field.field_id)), "Widget auch auf Seite 2");
}

/// CMS mit genau der Reservegröße passt, eines mit einem Byte mehr ergibt `cms-too-large`
/// (RSA: konstante CMS-Länge).
#[test]
fn cms_exactly_reserve_size_fits_one_byte_more_does_not() {
    let src = simple();
    let mut signer = KeystoreSigner::open("test-rsa2048");
    let mut plan = SignPlan::invisible(pida_pades::local_signing_time());
    plan.reserve = Reserve::Bytes(16 * 1024);
    let probe = prepare(&src, &plan, Some(&signer.chain)).unwrap();
    let len = signer.cms_for(&probe).len();
    plan.reserve = Reserve::Bytes(len);
    let exact = prepare(&src, &plan, Some(&signer.chain)).unwrap();
    assert_eq!(exact.reserve(), len);
    let signed = signer.sign(exact).expect("CMS genau in der Reserve");
    signed
        .self_check(&CheckContext { source: &src, expected_signer: Some(&signer.chain.signer), renderer: None })
        .unwrap();
    let br = signed.byte_range().0;
    let bytes = signed.bytes();
    assert!(
        bytes[br[1] as usize + 1..br[2] as usize - 1].iter().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_lowercase())
    );
    plan.reserve = Reserve::Bytes(len - 1);
    let small = prepare(&src, &plan, Some(&signer.chain)).unwrap();
    match signer.sign(small) {
        Err(e) => assert!(e.contains("cms-too-large"), "{e}"),
        Ok(_) => panic!("CMS größer als die Reserve eingebettet"),
    }
}

/// Vorhandenes leeres Feld, hierarchisch (`Unterschriften.Kunde`) mit eigenem Widget-Kind: das
/// Widget wird mit neuer `/AP` fortgeschrieben, `/V` am Feld; Orakel wie im Korpus.
#[test]
fn existing_hierarchical_field_with_kid_widget() {
    let or = Oracles::detect();
    let work = tempfile::tempdir().unwrap();
    let base = simple();
    let page = base.pages()[0].object_id;
    let mut ids = (None, None);
    let doc = derive(&base, |b, catalog| {
        let parent = b.allocate_id();
        let field = b.allocate_id();
        let widget = b.allocate_id();
        let mut p = Dictionary::new();
        p.set("T", text("Unterschriften"));
        p.set("Kids", Object::Array(vec![Object::Reference(field)]));
        let mut f = Dictionary::new();
        f.set("FT", name("Sig"));
        f.set("T", text("Kunde"));
        f.set("Parent", Object::Reference(parent));
        f.set("Kids", Object::Array(vec![Object::Reference(widget)]));
        let mut w = Dictionary::new();
        w.set("Type", name("Annot"));
        w.set("Subtype", name("Widget"));
        w.set("Rect", Object::Array(vec![300.into(), 100.into(), 530.into(), 150.into()]));
        w.set("F", Object::Integer(4));
        w.set("P", Object::Reference(page));
        w.set("Parent", Object::Reference(field));
        b.insert_new(parent, Object::Dictionary(p)).unwrap();
        b.insert_new(field, Object::Dictionary(f)).unwrap();
        b.insert_new(widget, Object::Dictionary(w)).unwrap();
        let mut pd = base.get_object(page).unwrap().as_dict().unwrap().clone();
        pd.set("Annots", Object::Array(vec![Object::Reference(widget)]));
        b.update_object(page, Object::Dictionary(pd)).unwrap();
        let mut acro = Dictionary::new();
        acro.set("Fields", Object::Array(vec![Object::Reference(parent)]));
        catalog.set("AcroForm", Object::Dictionary(acro));
        ids = (Some(field), Some(widget));
    });
    let report = analyze(&doc).unwrap();
    let cand = empty_fields(&report).into_iter().next().expect("leeres hierarchisches Feld");
    assert_eq!(cand.qualified_name, "Unterschriften.Kunde");
    assert_eq!((Some(cand.field_id), Some(cand.widget_id)), ids);
    let bytes = doc.bytes().to_vec();
    let orig = work.path().join("hier.pdf");
    std::fs::write(&orig, &bytes).unwrap();
    let mut outcomes = Vec::new();
    for (i, (profile, stem)) in
        [("SIGNATURBLOCK_SMALL_DE", "test-rsa2048"), ("INVISIBLE", "test-ecp384-title")].iter().enumerate()
    {
        let mut signer = KeystoreSigner::open(stem);
        let target = FieldTarget::Existing {
            field_id: cand.field_id,
            widget_id: cand.widget_id,
            qualified_name: cand.qualified_name.clone(),
        };
        let adjust = move |opts: &mut pida_pades::PlanOptions, _: &pida_pades::DocReport, _: &SourcePdf| {
            opts.field = target.clone();
        };
        let o = run_case(
            &or,
            &mut signer,
            &orig,
            &bytes,
            profile,
            CmsProfile::PadesBB,
            &adjust,
            work.path(),
            &format!("hier-{i}"),
            &format!("hierarchisches Feld ({profile})"),
        );
        assert_eq!(o.field, "Unterschriften.Kunde");
        if let Some(s) = &o.signed {
            let out = SourcePdf::open(s.bytes(), &Limits::default()).unwrap();
            let w = out.get_object(cand.widget_id).unwrap().as_dict().unwrap().clone();
            assert!(w.has(b"AP"), "Widget-Kind ohne neue /AP");
            let f = out.get_object(cand.field_id).unwrap().as_dict().unwrap().clone();
            assert!(f.has(b"V") && !w.has(b"V"), "/V gehört an das Feld");
        }
        outcomes.push(o);
    }
    let trust: Vec<_> = KEYS.iter().map(|(_, s)| key_cert_pem(s)).collect();
    pyhanko_all(&or, &mut outcomes, &trust);
    assert_all_ok(&outcomes);
}

/// Ein signiertes Feld, dessen `/V` kein lesbares Signaturdictionary ist, zählt trotzdem als
/// Vorsignatur: Befund, keine neue Seite (wie `prepare`).
#[test]
fn unreadable_signature_dictionary_counts_as_prior_signature() {
    let base = simple();
    let doc = derive(&base, |b, catalog| {
        let mut w = Dictionary::new();
        w.set("FT", name("Sig"));
        w.set("Type", name("Annot"));
        w.set("Subtype", name("Widget"));
        w.set("T", text("Kaputt"));
        w.set("Rect", Object::Array(vec![0.into(), 0.into(), 0.into(), 0.into()]));
        w.set("V", Object::Integer(42));
        let id = b.add_object(Object::Dictionary(w));
        let mut acro = Dictionary::new();
        acro.set("Fields", Object::Array(vec![Object::Reference(id)]));
        acro.set("SigFlags", Object::Integer(3));
        catalog.set("AcroForm", Object::Dictionary(acro));
    });
    let report = analyze(&doc).unwrap();
    assert!(report.is_signed(), "{report:?}");
    assert!(report.has_finding(codes::PRIOR_SIGNATURE_INVALID), "{:?}", report.findings);
}

/// AcroForm mit indirektem `/DR`, dessen direktes `/Font` kein `/Helv` hat: der PDFBox-Fixup
/// ergänzt `/Helv`/`/ZaDb` – das ist eine erlaubte Änderung (I6), die Vorgängerfassung meldete
/// hier fälschlich eine Verletzung, das Signieren schlug fehl.
#[test]
fn indirect_dr_with_direct_font_dictionary() {
    let or = Oracles::detect();
    let work = tempfile::tempdir().unwrap();
    let base = simple();
    let doc = derive(&base, |b, catalog| {
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
        acro.set("Fields", Object::Array(Vec::new()));
        acro.set("DR", Object::Reference(dr));
        acro.set("DA", text("/F1 10 Tf 0 g"));
        catalog.set("AcroForm", Object::Dictionary(acro));
    });
    let bytes = doc.bytes().to_vec();
    let orig = work.path().join("dr.pdf");
    std::fs::write(&orig, &bytes).unwrap();
    let mut signer = KeystoreSigner::open("test-rsa2048");
    let noop = |_: &mut pida_pades::PlanOptions, _: &pida_pades::DocReport, _: &SourcePdf| {};
    let mut outcomes = vec![run_case(
        &or,
        &mut signer,
        &orig,
        &bytes,
        "SIGNATURBLOCK_SMALL_DE",
        CmsProfile::PdfasCompat,
        &noop,
        work.path(),
        "dr",
        "indirektes /DR",
    )];
    let trust: Vec<_> = KEYS.iter().map(|(_, s)| key_cert_pem(s)).collect();
    pyhanko_all(&or, &mut outcomes, &trust);
    assert_all_ok(&outcomes);
    // /DA bleibt (war gesetzt), /Helv ergänzt
    let out = SourcePdf::open(outcomes[0].signed.as_ref().unwrap().bytes(), &Limits::default()).unwrap();
    let acro = out.catalog().unwrap().get(b"AcroForm").unwrap().as_dict().unwrap().clone();
    let da = out.resolve(acro.get(b"DA").unwrap()).unwrap().clone();
    assert_eq!(da, text("/F1 10 Tf 0 g"));
    let dr = out.resolve(acro.get(b"DR").unwrap()).unwrap().as_dict().unwrap().clone();
    let fonts = dr.get(b"Font").unwrap().as_dict().unwrap().clone();
    assert!(fonts.has(b"F1") && fonts.has(b"Helv") && fonts.has(b"ZaDb"));
}

/// FieldMDP-Transformation im Signaturdictionary (`/Reference`) sperrt Felder auch ohne `/Lock`
/// am Feld: Befund `pdf-field-locked`, Feld nicht wählbar, `prepare` lehnt ab.
#[test]
fn field_mdp_reference_locks_fields_without_lock_dictionary() {
    let base = simple();
    let doc = derive(&base, |b, catalog| {
        let mut tp = Dictionary::new();
        tp.set("Type", name("TransformParams"));
        tp.set("Action", name("Include"));
        tp.set("Fields", Object::Array(vec![text("Zweite")]));
        tp.set("V", name("1.2"));
        let mut r = Dictionary::new();
        r.set("Type", name("SigRef"));
        r.set("TransformMethod", name("FieldMDP"));
        r.set("TransformParams", Object::Dictionary(tp));
        let mut sig = Dictionary::new();
        sig.set("Type", name("Sig"));
        sig.set("Filter", name("Adobe.PPKLite"));
        sig.set("SubFilter", name("ETSI.CAdES.detached"));
        sig.set("ByteRange", Object::Array(vec![0.into(), 10.into(), 20.into(), 10.into()]));
        sig.set("Contents", Object::String(vec![0x30, 0x03, 0x02, 0x01, 0x01], StringFormat::Hexadecimal));
        sig.set("Reference", Object::Array(vec![Object::Dictionary(r)]));
        let sig = b.add_object(Object::Dictionary(sig));
        let field = |t: &str, v: Option<Object>| {
            let mut w = Dictionary::new();
            w.set("FT", name("Sig"));
            w.set("Type", name("Annot"));
            w.set("Subtype", name("Widget"));
            w.set("F", Object::Integer(4));
            w.set("T", text(t));
            w.set("Rect", Object::Array(vec![100.into(), 100.into(), 300.into(), 150.into()]));
            if let Some(v) = v {
                w.set("V", v);
            }
            Object::Dictionary(w)
        };
        let first = b.add_object(field("Erste", Some(Object::Reference(sig))));
        let second = b.add_object(field("Zweite", None));
        let third = b.add_object(field("Dritte", None));
        let mut acro = Dictionary::new();
        acro.set(
            "Fields",
            Object::Array(vec![Object::Reference(first), Object::Reference(second), Object::Reference(third)]),
        );
        acro.set("SigFlags", Object::Integer(3));
        catalog.set("AcroForm", Object::Dictionary(acro));
    });
    let report = analyze(&doc).unwrap();
    assert!(report.has_finding(codes::FIELD_LOCKED), "{:?}", report.findings);
    let names: Vec<String> = empty_fields(&report).into_iter().map(|c| c.qualified_name).collect();
    assert_eq!(names, vec!["Dritte".to_string()]);
    let locked = report.fields.iter().find(|f| f.qualified_name == "Zweite").unwrap();
    let mut plan = SignPlan::invisible(pida_pades::local_signing_time());
    plan.field = FieldTarget::Existing {
        field_id: locked.field_id,
        widget_id: locked.widgets[0].id,
        qualified_name: locked.qualified_name.clone(),
    };
    assert!(matches!(prepare(&doc, &plan, None), Err(PadesError::FieldLocked(_))));
}

/// Zertifizierte Dokumente (DocMDP P=2/3, mit pyHanko erzeugt: `testdata/pades/make_certified.py`):
/// vorhandenes leeres Feld (sichtbar/unsichtbar) und neue unsichtbare Signatur lassen die
/// Zertifizierung bei `FORM_FILLING` (pyHanko `docmdp_ok`); ein neues **sichtbares** Feld
/// (pyHanko: `OTHER`, Zertifizierung verletzt) lehnen Planer und `prepare` ab (Plan §6.1 K4).
#[test]
fn certified_documents_allow_existing_fields_and_invisible_only() {
    let or = Oracles::detect();
    let work = tempfile::tempdir().unwrap();
    let trust: Vec<_> = KEYS.iter().map(|(_, s)| key_cert_pem(s)).collect();
    let mut outcomes = Vec::new();
    for p in [2u8, 3] {
        let path = testdata().join(format!("pades/certified-docmdp-p{p}.pdf"));
        let bytes = std::fs::read(&path).unwrap();
        let src = open(&path);
        let report = analyze(&src).unwrap();
        assert_eq!(report.doc_mdp.map(|d| d.permission), Some(p));
        assert_eq!(report.class, pida_pades::AcceptanceClass::K4Restricted);
        assert!(!report.allows_new_visible_field());
        let cand = empty_fields(&report).into_iter().next().expect("leeres Feld „Leer“");
        let target = FieldTarget::Existing {
            field_id: cand.field_id,
            widget_id: cand.widget_id,
            qualified_name: cand.qualified_name.clone(),
        };

        // neues sichtbares Feld: Planer und prepare lehnen ab
        let signer = KeystoreSigner::open("test-rsa2048");
        let opts = pida_pades::PlanOptions::new(pida_pades::local_signing_time());
        let renderer = open_renderer(&src);
        let err = plan_for(&src, &report, "SIGNATURBLOCK_SMALL_DE", &signer.chain.signer, &opts, renderer.as_deref())
            .unwrap_err();
        assert_eq!(err.code(), "pades-new-visible-field-not-allowed");
        let mut doc_without_mdp = report.clone();
        doc_without_mdp.doc_mdp = None;
        let planned =
            plan_for(&src, &doc_without_mdp, "SIGNATURBLOCK_SMALL_DE", &signer.chain.signer, &opts, None).unwrap();
        assert!(matches!(
            prepare(&src, &planned.plan, Some(&signer.chain)),
            Err(PadesError::NewVisibleFieldNotAllowed)
        ));

        for (i, (profile, existing)) in
            [("SIGNATURBLOCK_SMALL_DE", true), ("INVISIBLE", true), ("INVISIBLE", false)].iter().enumerate()
        {
            let mut signer = KeystoreSigner::open(KEYS[i].1);
            let t = target.clone();
            let ex = *existing;
            let adjust = move |opts: &mut pida_pades::PlanOptions, _: &pida_pades::DocReport, _: &SourcePdf| {
                if ex {
                    opts.field = t.clone();
                }
            };
            outcomes.push(run_case(
                &or,
                &mut signer,
                &path,
                &bytes,
                profile,
                CmsProfile::PadesBB,
                &adjust,
                work.path(),
                &format!("p{p}-{i}"),
                &format!("DocMDP P={p}, {profile}, vorhandenes Feld: {existing}"),
            ));
        }
    }
    pyhanko_all(&or, &mut outcomes, &trust);
    assert_all_ok(&outcomes);
    // die Zertifizierung bleibt gültig und erlaubt (docmdp_ok)
    if let Some(py) = &or.python {
        let outs: Vec<_> = outcomes.iter().filter_map(|o| o.out.clone()).collect();
        let r = pyhanko_batch(py, &outs, &trust).unwrap();
        for o in &outs {
            let cert = r[&o.to_string_lossy().to_string()]
                .iter()
                .find(|s| s.field.as_deref() == Some("Zertifizierung"))
                .expect("Zertifizierungssignatur");
            assert!(cert.intact && cert.valid && cert.docmdp_ok == Some(true), "{cert:?}");
            assert_eq!(cert.modification_level.as_deref(), Some("FORM_FILLING"), "{cert:?}");
        }
    }
}

/// Schlüssel eines Number Trees (`/Nums`, rekursiv `/Kids`) in Dokumentreihenfolge.
fn number_tree_keys(src: &SourcePdf, node: &Dictionary, out: &mut Vec<i64>) {
    if let Ok(nums) = node.get(b"Nums") {
        for pair in src.resolve(nums).unwrap().as_array().unwrap().chunks(2) {
            out.push(src.resolve(&pair[0]).unwrap().as_i64().unwrap());
        }
    }
    if let Ok(kids) = node.get(b"Kids") {
        for k in src.resolve(kids).unwrap().as_array().unwrap() {
            number_tree_keys(src, src.resolve(k).unwrap().as_dict().unwrap(), out);
        }
    }
}

/// Veralteter `/ParentTreeNextKey` (0, obwohl der ParentTree schon Schlüssel hat): der neue
/// Eintrag kollidiert nicht und bleibt sortiert; Orakel wie im Korpus.
#[test]
fn stale_parent_tree_next_key_does_not_collide() {
    let or = Oracles::detect();
    let work = tempfile::tempdir().unwrap();
    let base = open(&testdata().join("pdf/pdfas/pdf-as-cli/rotate_180.pdf"));
    let st_id = match base.catalog().unwrap().get(b"StructTreeRoot").unwrap() {
        Object::Reference(id) => *id,
        other => panic!("StructTreeRoot direkt: {other:?}"),
    };
    let doc = derive(&base, |b, _| {
        let mut st = base.get_object(st_id).unwrap().as_dict().unwrap().clone();
        st.set("ParentTreeNextKey", Object::Integer(0));
        b.update_object(st_id, Object::Dictionary(st)).unwrap();
    });
    let keys_before = {
        let st = doc.get_object(st_id).unwrap().as_dict().unwrap();
        let mut k = Vec::new();
        number_tree_keys(&doc, doc.resolve(st.get(b"ParentTree").unwrap()).unwrap().as_dict().unwrap(), &mut k);
        k
    };
    assert!(!keys_before.is_empty() && keys_before.contains(&0), "{keys_before:?}");
    let bytes = doc.bytes().to_vec();
    let orig = work.path().join("stale-key.pdf");
    std::fs::write(&orig, &bytes).unwrap();
    let mut signer = KeystoreSigner::open("test-ecp256");
    let noop = |_: &mut pida_pades::PlanOptions, _: &pida_pades::DocReport, _: &SourcePdf| {};
    let mut outcomes = vec![run_case(
        &or,
        &mut signer,
        &orig,
        &bytes,
        "SIGNATURBLOCK_SMALL_DE",
        CmsProfile::PdfasCompat,
        &noop,
        work.path(),
        "stale-key",
        "veralteter /ParentTreeNextKey",
    )];
    let trust: Vec<_> = KEYS.iter().map(|(_, s)| key_cert_pem(s)).collect();
    pyhanko_all(&or, &mut outcomes, &trust);
    assert_all_ok(&outcomes);
    let out = SourcePdf::open(outcomes[0].signed.as_ref().unwrap().bytes(), &Limits::default()).unwrap();
    let field = out.signature_fields().unwrap().into_iter().find(|f| f.signed).unwrap();
    let w = out.get_object(field.field_id).unwrap().as_dict().unwrap().clone();
    let key = w.get(b"StructParent").unwrap().as_i64().unwrap();
    assert!(keys_before.iter().all(|k| *k < key), "{key} kollidiert mit {keys_before:?}");
    let st = out.get_object(st_id).unwrap().as_dict().unwrap();
    let mut after = Vec::new();
    number_tree_keys(&out, out.resolve(st.get(b"ParentTree").unwrap()).unwrap().as_dict().unwrap(), &mut after);
    assert!(after.windows(2).all(|w| w[0] < w[1]), "nicht sortiert/eindeutig: {after:?}");
    assert_eq!(out.resolve(st.get(b"ParentTreeNextKey").unwrap()).unwrap().as_i64().unwrap(), key + 1);
}

/// `/ParentTreeNextKey` mit dem größtmöglichen Wert: Der nächste Schlüssel wäre nicht darstellbar. Die
/// PDF/UA-Ergänzung entfällt dann mit Hinweis (wie bei anderen unbrauchbaren Strukturbäumen), statt überzulaufen
/// und einen unsinnigen Schlüssel ins signierte Dokument zu schreiben (Argus R-03).
#[test]
fn parent_tree_next_key_at_the_limit_skips_the_pdfua_addition() {
    let base = open(&testdata().join("pdf/pdfas/pdf-as-cli/rotate_180.pdf"));
    let st_id = match base.catalog().unwrap().get(b"StructTreeRoot").unwrap() {
        Object::Reference(id) => *id,
        other => panic!("StructTreeRoot direkt: {other:?}"),
    };
    let doc = derive(&base, |b, _| {
        let mut st = base.get_object(st_id).unwrap().as_dict().unwrap().clone();
        st.set("ParentTreeNextKey", Object::Integer(i64::MAX));
        b.update_object(st_id, Object::Dictionary(st)).unwrap();
    });
    let report = analyze(&doc).unwrap();
    let signer = KeystoreSigner::open("test-ecp256");
    let opts = pida_pades::PlanOptions::new(pida_pades::local_signing_time());
    let planned = plan_for(&doc, &report, "SIGNATURBLOCK_SMALL_DE", &signer.chain.signer, &opts, None).unwrap();
    let prepared = pida_pades::prepare(&doc, &planned.plan, Some(&signer.chain)).unwrap();
    assert!(prepared.notices().iter().any(|n| n == pida_pades::NOTICE_PDFUA_SKIPPED), "{:?}", prepared.notices());
}
