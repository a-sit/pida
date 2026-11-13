//! Test-Hilfen (Feature `testing`): eine Fake-PAdES-Engine, die statt eines PDF-Increments das CMS
//! an den Originalinhalt hängt (`Inhalt ‖ CMS`). Signaturen sind echt (cades), nur die PDF-Struktur
//! fehlt – damit lassen sich alle Abläufe des Kerns ohne `pida-pades` prüfen.

use pida_fs::PathExt as _;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use pida_cades::{AssembleOptions, CertChain, Digest, DigestAlg, Expect, ExpectedSigner, RawSignature, SignedAttrs};
use pida_pades::{
    AcceptanceClass, CheckContext, CheckReport, DocReport, ExcludedRange, Finding, FinishError, Invariant,
    InvariantViolation, OutputInfo, OutputTarget, PadesError, PreparedHandle, Reserve, SignPlan, XfaKind,
};
use pida_pdf_cos::SourcePdf;
use time::OffsetDateTime;

use crate::engine::{PadesEngine, PreparedDoc, SignedDoc};

/// Fake-Engine.
#[derive(Debug)]
pub struct FakeEngine {
    /// aufgezeichnete Pläne
    pub plans: Mutex<Vec<SignPlan>>,
    /// Klasse der Analyse (Standard K1)
    pub class: Mutex<Option<AcceptanceClass>>,
    /// Reserve für `Reserve::ModelB`
    pub model_b_reserve: AtomicUsize,
    /// Selbstprüfung schlägt fehl (I1)
    pub fail_check: AtomicBool,
    /// Anzahl `prepare`
    pub prepares: AtomicUsize,
    /// Verzögerung der Analyse in Millisekunden (Wettläufe mit Nutzeraktionen prüfen)
    pub analyze_delay_ms: AtomicU64,
    /// Befunde der Analyse (z. B. Warnungen mit Abbruchmöglichkeit)
    pub findings: Mutex<Vec<Finding>>,
    /// Anzahl erfolgreicher `restore` (Mobile, „Signatur fortsetzen“)
    pub restores: AtomicUsize,
}

impl Default for FakeEngine {
    fn default() -> Self {
        Self {
            plans: Mutex::new(Vec::new()),
            class: Mutex::new(None),
            model_b_reserve: AtomicUsize::new(32 * 1024),
            fail_check: AtomicBool::new(false),
            prepares: AtomicUsize::new(0),
            analyze_delay_ms: AtomicU64::new(0),
            findings: Mutex::new(Vec::new()),
            restores: AtomicUsize::new(0),
        }
    }
}

impl FakeEngine {
    /// Neue Fake-Engine.
    pub fn new() -> std::sync::Arc<FakeEngine> {
        std::sync::Arc::new(FakeEngine::default())
    }

    /// Letzter Plan.
    pub fn last_plan(&self) -> SignPlan {
        self.plans
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .last()
            .cloned()
            .expect("prepare aufgerufen")
    }
}

impl PadesEngine for FakeEngine {
    fn analyze(&self, src: &SourcePdf) -> Result<DocReport, PadesError> {
        let delay = self.analyze_delay_ms.load(Ordering::SeqCst);
        if delay > 0 {
            std::thread::sleep(std::time::Duration::from_millis(delay));
        }
        let class =
            self.class.lock().unwrap_or_else(std::sync::PoisonError::into_inner).unwrap_or(AcceptanceClass::K1Regular);
        Ok(DocReport {
            class,
            findings: self.findings.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone(),
            page_count: src.page_count(),
            xref_kind: src.xref_kind(),
            tail: src.tail().kind,
            encrypted: src.encryption().is_some(),
            may_sign: true,
            fields: src.signature_fields()?,
            signatures: vec![],
            doc_mdp: None,
            pdfa: None,
            tagged: false,
            xfa: XfaKind::None,
            usage_rights: false,
            need_appearances: false,
            next_field_number: 1,
        })
    }

    fn prepare(
        &self,
        src: &SourcePdf,
        plan: &SignPlan,
        _cert: Option<&CertChain>,
    ) -> Result<Box<dyn PreparedDoc>, PadesError> {
        self.plans.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(plan.clone());
        self.prepares.fetch_add(1, Ordering::SeqCst);
        let reserve = match plan.reserve {
            Reserve::Bytes(n) => n,
            Reserve::ModelA => 8 * 1024,
            Reserve::ModelB => self.model_b_reserve.load(Ordering::SeqCst),
            Reserve::ModelBAtLeast(n) => n.max(self.model_b_reserve.load(Ordering::SeqCst)),
        };
        Ok(Box::new(FakePrepared {
            content: src.bytes().to_vec(),
            signing_time: plan.signing_time,
            reserve,
            fail_check: self.fail_check.load(Ordering::SeqCst),
        }))
    }

    fn restore(&self, handle: &PreparedHandle, src: &SourcePdf) -> Result<Box<dyn PreparedDoc>, PadesError> {
        let invalid = |m: &str| PadesError::InvalidState(m.to_owned());
        let bytes = pida_fs::read(&handle.path)?;
        if DigestAlg::Sha256.digest(&bytes).value != handle.sha256 {
            return Err(invalid("Prüfsumme stimmt nicht"));
        }
        let state: FakeState = serde_json::from_slice(&bytes).map_err(|_| invalid("Zustand nicht lesbar"))?;
        if state.content != src.bytes() {
            return Err(invalid("Original passt nicht"));
        }
        let signing_time =
            OffsetDateTime::from_unix_timestamp(state.signing_time_unix).map_err(|_| invalid("Zeitpunkt"))?;
        self.restores.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(FakePrepared {
            content: state.content,
            signing_time,
            reserve: state.reserve,
            fail_check: self.fail_check.load(Ordering::SeqCst),
        }))
    }
}

/// Persistierter Zustand der Fake-Engine (JSON).
#[derive(serde::Serialize, serde::Deserialize)]
struct FakeState {
    content: Vec<u8>,
    signing_time_unix: i64,
    reserve: usize,
}

/// „Vorbereitetes PDF“: Inhalt + 2 Byte Lücke.
#[derive(Debug)]
pub struct FakePrepared {
    content: Vec<u8>,
    signing_time: OffsetDateTime,
    reserve: usize,
    fail_check: bool,
}

impl PreparedDoc for FakePrepared {
    fn excluded_range(&self) -> ExcludedRange {
        let n = self.content.len() as u64;
        ExcludedRange { from: n, to: n + 1 }
    }
    fn total_len(&self) -> u64 {
        self.content.len() as u64 + 2
    }
    fn reserve(&self) -> usize {
        self.reserve
    }
    fn signing_time(&self) -> OffsetDateTime {
        self.signing_time
    }
    fn blacked_out(&self) -> Box<dyn std::io::Read + Send + '_> {
        Box::new(std::io::Read::chain(self.content.as_slice(), &[0u8, 0u8][..]))
    }
    fn digest(&self, alg: DigestAlg) -> Digest {
        alg.digest(&self.content)
    }
    fn finish_cms(self: Box<Self>, cms: &[u8], expect: &Expect) -> Result<Box<dyn SignedDoc>, FinishError> {
        if cms.len() > self.reserve {
            return Err(FinishError::CmsTooLarge { needed: cms.len(), reserve: self.reserve });
        }
        pida_cades::verify_detached(cms, &mut self.content.as_slice(), expect)?;
        Ok(Box::new(FakeSigned { bytes: [self.content.as_slice(), cms].concat(), fail_check: self.fail_check }))
    }
    fn finish_raw(
        self: Box<Self>,
        attrs: &SignedAttrs,
        signature: &RawSignature,
        chain: &CertChain,
    ) -> Result<Box<dyn SignedDoc>, FinishError> {
        let cms = pida_cades::assemble(
            attrs,
            signature,
            chain,
            &AssembleOptions { profile: attrs.profile, include_intermediates: false },
        )?;
        let expect = Expect { signer: ExpectedSigner::Certificate(chain.signer.clone()), ..Expect::default() };
        self.finish_cms(&cms, &expect)
    }
    fn persist(&self, dir: &std::path::Path) -> Result<PreparedHandle, PadesError> {
        let state = FakeState {
            content: self.content.clone(),
            signing_time_unix: self.signing_time.unix_timestamp(),
            reserve: self.reserve,
        };
        let bytes = serde_json::to_vec(&state).map_err(|e| PadesError::InvalidState(e.to_string()))?;
        let path = dir.join("fake-prepared.json");
        pida_fs::write(&path, &bytes)?;
        Ok(PreparedHandle { path, sha256: DigestAlg::Sha256.digest(&bytes).value })
    }
}

/// „Signiertes PDF“: Inhalt ‖ CMS.
#[derive(Debug)]
pub struct FakeSigned {
    bytes: Vec<u8>,
    fail_check: bool,
}

impl SignedDoc for FakeSigned {
    fn self_check(&self, _ctx: &CheckContext<'_>) -> Result<CheckReport, InvariantViolation> {
        if self.fail_check {
            return Err(InvariantViolation { invariant: Invariant::I1Prefix, detail: "Testfehler".into() });
        }
        Ok(CheckReport { checked: vec![Invariant::I3Cms], prior: vec![], warnings: vec![] })
    }
    fn write_atomically(&self, target: &OutputTarget) -> Result<OutputInfo, PadesError> {
        if target.path.fs_exists() && !target.overwrite {
            return Err(PadesError::Io(std::io::Error::from(std::io::ErrorKind::AlreadyExists)));
        }
        pida_fs::write(&target.path, &self.bytes)?;
        Ok(OutputInfo { path: target.path.clone(), len: self.bytes.len() as u64, sha256: "fake".into() })
    }
    fn len(&self) -> u64 {
        self.bytes.len() as u64
    }
    fn bytes(&self) -> Vec<u8> {
        self.bytes.clone()
    }
}

/// Prüft eine von der Fake-Engine geschriebene Datei: CMS über den Originalinhalt.
pub fn verify_fake_output(path: &std::path::Path, original: &[u8]) -> pida_cades::CmsReport {
    let bytes = pida_fs::read(path).expect("Ausgabe lesbar");
    assert!(bytes.starts_with(original), "Original ist Präfix (I1)");
    let cms = &bytes[original.len()..];
    pida_cades::verify_detached(cms, &mut &original[..], &Expect::default()).expect("CMS gültig")
}
