use a3s_acl::{canonical_bytes, canonical_digest, parse_acl};

mod inventory;
mod model;
mod validation;

pub use model::{
    AppPlatformCapability, AppPlatformCapabilityAvailability, AppPlatformCapabilityCategory,
    AppPlatformGate, AppPlatformGateState, AppPlatformParityManifest, AppPlatformReference,
};
use validation::{
    exact_root_block, parse_entries, required_bool, required_string, validate_manifest,
};

/// Schema identifier for the first machine-checkable AI application parity baseline.
pub const APP_PLATFORM_PARITY_MANIFEST_SCHEMA: &str = "a3s.cloud.app-platform.parity-manifest.v1";

const MANIFEST_ID: &str = "application-platform-core-2026-08-13";
const MANIFEST_BASELINE: &str = "2026-08-13";
const MANIFEST_BLOCK: &str = "parity_manifest";
const REFERENCE_BLOCK: &str = "reference";
const GATE_BLOCK: &str = "gate";
const CAPABILITY_BLOCK: &str = "capability";
const MAX_MANIFEST_BYTES: usize = 256 * 1024;
const ROOT_ATTRIBUTES: [&str; 4] = ["baseline", "parity_claim", "public_claim_gate", "schema"];
const REFERENCE_ATTRIBUTES: [&str; 2] = ["observed_on", "url"];
const GATE_ATTRIBUTES: [&str; 2] = ["evidence", "state"];
const CAPABILITY_ATTRIBUTES: [&str; 8] = [
    "availability",
    "category",
    "dependencies",
    "evidence",
    "gate",
    "label",
    "owner",
    "references",
];
const EVIDENCE_KINDS: [&str; 4] = ["contract", "doc", "implementation", "test"];
const OWNERS: [&str; 15] = [
    "agents",
    "applications",
    "assets",
    "automations",
    "connectors",
    "edge_gateway",
    "executions",
    "files",
    "identity",
    "inference",
    "knowledge",
    "operations_telemetry",
    "platform",
    "use",
    "workflow",
];

impl AppPlatformParityManifest {
    /// Parses and validates one complete canonical v1 manifest.
    pub fn parse_acl(source: &str) -> Result<Self, String> {
        if source.is_empty() || source.len() > MAX_MANIFEST_BYTES {
            return Err("application-platform parity manifest ACL size is invalid".into());
        }
        if source.contains('\r') && !source.contains("\r\n") {
            return Err(
                "application-platform parity manifest contains a bare carriage return".into(),
            );
        }
        let normalized = source.replace("\r\n", "\n");
        let document = parse_acl(&normalized).map_err(|error| {
            format!("application-platform parity manifest ACL is invalid: {error}")
        })?;
        let canonical = canonical_bytes(&document).map_err(|error| {
            format!("application-platform parity manifest is not canonicalizable: {error}")
        })?;
        if normalized.as_bytes() != canonical {
            return Err("application-platform parity manifest ACL is not canonical".into());
        }
        let root = exact_root_block(&document)?;
        let baseline = required_string(root, "baseline")?;
        if baseline != MANIFEST_BASELINE {
            return Err(format!(
                "application-platform parity baseline must be {MANIFEST_BASELINE:?}"
            ));
        }
        let public_claim_gate = required_string(root, "public_claim_gate")?;
        let parity_claim = required_bool(root, "parity_claim")?;
        let schema = required_string(root, "schema")?;
        if schema != APP_PLATFORM_PARITY_MANIFEST_SCHEMA {
            return Err(format!(
                "application-platform parity manifest schema must be {APP_PLATFORM_PARITY_MANIFEST_SCHEMA:?}"
            ));
        }

        let entries = parse_entries(root)?;
        validate_manifest(
            &public_claim_gate,
            parity_claim,
            &entries.references,
            &entries.gates,
            &entries.capabilities,
        )?;
        let digest = canonical_digest(&document).map_err(|error| {
            format!("application-platform parity manifest digest failed: {error}")
        })?;
        let canonical_acl = String::from_utf8(canonical)
            .map_err(|_| "application-platform parity manifest is not UTF-8".to_owned())?;

        Ok(Self {
            baseline,
            public_claim_gate,
            parity_claim,
            references: entries.references,
            gates: entries.gates,
            capabilities: entries.capabilities,
            canonical_acl,
            digest,
        })
    }

    /// Restores a stored manifest only when its canonical semantic digest matches.
    pub fn restore(source: &str, stored_digest: &str) -> Result<Self, String> {
        let manifest = Self::parse_acl(source)?;
        if manifest.digest != stored_digest {
            return Err(
                "stored application-platform parity manifest ACL and digest do not match".into(),
            );
        }
        Ok(manifest)
    }
}
