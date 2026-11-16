//! Ein Durchlauf der Signaturvorbereitung für beliebige Bytes – gemeinsam für den Fuzz-Lauf im Test
//! (`tests/fuzz_prepare.rs`) und das Fuzz-Ziel `pades_prepare` (`fuzz/fuzz_targets/pades_prepare.rs`, dort per
//! `#[path]` eingebunden). Deshalb ohne die übrigen Test-Hilfen: nur pades, pdf-cos, cades, sigblock und der Keystore.

use std::sync::OnceLock;

use pida_backend_keystore::LoadedKey;
use pida_cades::RawSigner;
use pida_pades::{PlanOptions, SignPlan, analyze, cert_fields, plan_signature, prepare, reject_open_error};
use pida_pdf_cos::{Limits, SourcePdf};
use pida_sigblock::{FontProvider, ProfileStore, SigBlockInput};

/// Profil des sichtbaren Blocks in diesem Durchlauf.
const PROFILE: &str = "SIGNATURBLOCK_SMALL_DE";

fn store() -> &'static ProfileStore {
    static S: OnceLock<ProfileStore> = OnceLock::new();
    S.get_or_init(|| ProfileStore::builtin().expect("eingebaute Profile"))
}

fn fonts() -> &'static FontProvider {
    static F: OnceLock<FontProvider> = OnceLock::new();
    F.get_or_init(FontProvider::builtin)
}

/// Ein Fall: öffnen, analysieren, planen (ohne Renderer), vorbereiten (unsichtbar und sichtbar) und den Hash über
/// die signierten Bereiche bilden. Fehler sind erlaubt, Panics nicht.
pub fn run_case(data: Vec<u8>, key: &LoadedKey, limits: &Limits) {
    let src = match SourcePdf::open(data, limits) {
        Ok(s) => s,
        Err(e) => {
            let _ = reject_open_error(&e);
            return;
        }
    };
    let Ok(report) = analyze(&src) else { return };
    let _ = pida_pades::empty_fields(&report);
    let chain = key.chain().clone();
    let time = pida_pades::local_signing_time();
    if let Ok(p) = prepare(&src, &SignPlan::invisible(time), Some(&chain)) {
        let _ = p.digest(key.algorithm().digest());
    }
    let opts = PlanOptions::new(time);
    let profile = store().profile(PROFILE).expect("Profil");
    let input = SigBlockInput::new(cert_fields(&chain.signer), opts.signing_time);
    if let Ok(planned) = plan_signature(&src, &report, profile, &input, &opts, fonts(), &**store().resources(), None)
        && let Ok(p) = prepare(&src, &planned.plan, Some(&chain))
    {
        let _ = p.digest(key.algorithm().digest());
    }
}
