use crate::modules::shared_kernel::domain::{InstallationId, Sha256Digest, TrustDomainId};
use a3s_acl::builder::{integer, list, string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Block, Document, Value};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

pub const TRUST_DOMAIN_CONTRACT_SCHEMA: &str = "cloud.identity.trust-domain.v1";
pub const TRUST_DOMAIN_CONTRACT_MAX_ACL_BYTES: usize = 32 * 1024;
pub const MIN_WORKLOAD_CREDENTIAL_LIFETIME_SECONDS: u32 = 60;
pub const MAX_WORKLOAD_CREDENTIAL_LIFETIME_SECONDS: u32 = 3_600;
pub const MAX_TRUST_DOMAIN_ATTESTATION_PROFILES: usize = 16;
pub const MAX_TRUST_DOMAIN_FEDERATION_BUNDLES: usize = 16;

const TRUST_DOMAIN_BLOCK: &str = "trust_domain";
const MAX_ACL_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TrustDomainName(String);

impl TrustDomainName {
    pub fn parse(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 253
            || value != value.to_ascii_lowercase()
            || value.starts_with('.')
            || value.ends_with('.')
            || value.contains("..")
            || value.split('.').any(|label| {
                label.is_empty()
                    || label.len() > 63
                    || !label.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
                    })
                    || label.starts_with('-')
                    || label.ends_with('-')
            })
        {
            return Err(
                "trust-domain name must be a canonical lowercase DNS authority without a port"
                    .into(),
            );
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadIdentityFormat {
    X509Svid,
    JwtSvid,
}

impl WorkloadIdentityFormat {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::X509Svid => "x509_svid",
            Self::JwtSvid => "jwt_svid",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "x509_svid" => Ok(Self::X509Svid),
            "jwt_svid" => Ok(Self::JwtSvid),
            _ => Err("workload identity format is unsupported".into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadIdentityRevocationMode {
    Expiry,
    EpochAndExpiry,
}

impl WorkloadIdentityRevocationMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Expiry => "expiry",
            Self::EpochAndExpiry => "epoch_and_expiry",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "expiry" => Ok(Self::Expiry),
            "epoch_and_expiry" => Ok(Self::EpochAndExpiry),
            _ => Err("workload identity revocation mode is unsupported".into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustDomainContractSpec {
    pub installation_id: InstallationId,
    pub trust_domain_id: TrustDomainId,
    pub name: TrustDomainName,
    pub provider_profile_digest: Sha256Digest,
    pub trust_bundle_digest: Sha256Digest,
    pub node_attestation_profile_digests: Vec<Sha256Digest>,
    pub identity_formats: Vec<WorkloadIdentityFormat>,
    pub max_credential_lifetime_seconds: u32,
    pub rotation_overlap_seconds: u32,
    pub revocation_mode: WorkloadIdentityRevocationMode,
    pub federation_bundle_digests: Vec<Sha256Digest>,
}

impl TrustDomainContractSpec {
    fn normalize(mut self) -> Result<Self, String> {
        self.name = TrustDomainName::parse(self.name.as_str())?;
        validate_digest(&self.provider_profile_digest, "provider profile")?;
        validate_digest(&self.trust_bundle_digest, "trust bundle")?;
        normalize_digest_set(
            &mut self.node_attestation_profile_digests,
            1,
            MAX_TRUST_DOMAIN_ATTESTATION_PROFILES,
            "node-attestation profiles",
        )?;
        normalize_digest_set(
            &mut self.federation_bundle_digests,
            0,
            MAX_TRUST_DOMAIN_FEDERATION_BUNDLES,
            "federation bundles",
        )?;
        normalize_format_set(&mut self.identity_formats)?;
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.installation_id.as_uuid().is_nil()
            || self.trust_domain_id.as_uuid().is_nil()
            || !(MIN_WORKLOAD_CREDENTIAL_LIFETIME_SECONDS
                ..=MAX_WORKLOAD_CREDENTIAL_LIFETIME_SECONDS)
                .contains(&self.max_credential_lifetime_seconds)
            || self.rotation_overlap_seconds > self.max_credential_lifetime_seconds / 2
            || !self
                .identity_formats
                .contains(&WorkloadIdentityFormat::X509Svid)
        {
            return Err("trust-domain identity or credential bounds are invalid".into());
        }
        TrustDomainName::parse(self.name.as_str())?;
        validate_digest(&self.provider_profile_digest, "provider profile")?;
        validate_digest(&self.trust_bundle_digest, "trust bundle")?;
        validate_normalized_digest_set(
            &self.node_attestation_profile_digests,
            1,
            MAX_TRUST_DOMAIN_ATTESTATION_PROFILES,
            "node-attestation profiles",
        )?;
        validate_normalized_digest_set(
            &self.federation_bundle_digests,
            0,
            MAX_TRUST_DOMAIN_FEDERATION_BUNDLES,
            "federation bundles",
        )?;
        let mut formats = self.identity_formats.clone();
        normalize_format_set(&mut formats)?;
        if formats != self.identity_formats {
            return Err("trust-domain identity formats are not canonical".into());
        }
        Ok(())
    }
}

/// Canonical Identity-owned trust-domain configuration.
///
/// It contains public provider identity, trust and policy only. Root private
/// keys, workload credentials, node evidence and provider database state are
/// deliberately excluded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustDomainContract {
    spec: TrustDomainContractSpec,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl TrustDomainContract {
    pub fn from_spec(spec: TrustDomainContractSpec) -> Result<Self, String> {
        let spec = spec.normalize()?;
        let document = contract_document(&spec)?;
        let canonical_acl = format!("{}\n", generate_acl(&document));
        if canonical_acl.len() > TRUST_DOMAIN_CONTRACT_MAX_ACL_BYTES {
            return Err("trust-domain ACL exceeds its storage bound".into());
        }
        let reparsed = parse_acl(&canonical_acl)
            .map_err(|error| format!("generated trust-domain ACL is invalid: {error}"))?;
        let digest = Sha256Digest::parse(
            canonical_digest(&reparsed)
                .map_err(|error| format!("trust-domain ACL is not canonicalizable: {error}"))?,
        )?;
        Ok(Self {
            spec,
            canonical_acl,
            digest,
        })
    }

    pub fn parse_acl(source: &str) -> Result<Self, String> {
        let normalized =
            normalize_source(source, TRUST_DOMAIN_CONTRACT_MAX_ACL_BYTES, "trust-domain")?;
        let document = parse_acl(&normalized)
            .map_err(|error| format!("trust-domain ACL is invalid: {error}"))?;
        let value = Self::from_spec(parse_contract(&document)?)?;
        if value.canonical_acl != normalized {
            return Err("trust-domain ACL is not canonical".into());
        }
        Ok(value)
    }

    pub fn restore(source: &str, stored_digest: &str) -> Result<Self, String> {
        let value = Self::parse_acl(source)?;
        if value.digest.as_str() != stored_digest {
            return Err("stored trust-domain ACL and digest do not match".into());
        }
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if Self::restore(self.canonical_acl(), self.digest.as_str())? != *self {
            return Err("trust-domain contract drifted from canonical ACL".into());
        }
        Ok(())
    }

    pub const fn spec(&self) -> &TrustDomainContractSpec {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn contract_document(spec: &TrustDomainContractSpec) -> Result<Document, String> {
    Ok(Document {
        blocks: vec![BlockBuilder::new(TRUST_DOMAIN_BLOCK)
            .attr(
                "federation_bundle_digests",
                digest_list(&spec.federation_bundle_digests),
            )
            .attr(
                "identity_formats",
                list(
                    spec.identity_formats
                        .iter()
                        .map(|format| string(format.as_str()))
                        .collect(),
                ),
            )
            .attr("installation_id", string(&spec.installation_id.to_string()))
            .attr(
                "max_credential_lifetime_seconds",
                acl_integer(
                    "max_credential_lifetime_seconds",
                    u64::from(spec.max_credential_lifetime_seconds),
                    false,
                )?,
            )
            .attr("name", string(spec.name.as_str()))
            .attr(
                "node_attestation_profile_digests",
                digest_list(&spec.node_attestation_profile_digests),
            )
            .attr(
                "provider_profile_digest",
                string(spec.provider_profile_digest.as_str()),
            )
            .attr("revocation_mode", string(spec.revocation_mode.as_str()))
            .attr(
                "rotation_overlap_seconds",
                acl_integer(
                    "rotation_overlap_seconds",
                    u64::from(spec.rotation_overlap_seconds),
                    true,
                )?,
            )
            .attr("schema", string(TRUST_DOMAIN_CONTRACT_SCHEMA))
            .attr(
                "trust_bundle_digest",
                string(spec.trust_bundle_digest.as_str()),
            )
            .attr("trust_domain_id", string(&spec.trust_domain_id.to_string()))
            .build()],
    })
}

fn parse_contract(document: &Document) -> Result<TrustDomainContractSpec, String> {
    if document.blocks.len() != 1 {
        return Err("trust-domain ACL must contain exactly one top-level block".into());
    }
    let block = &document.blocks[0];
    strict_block(
        block,
        TRUST_DOMAIN_BLOCK,
        &[
            "federation_bundle_digests",
            "identity_formats",
            "installation_id",
            "max_credential_lifetime_seconds",
            "name",
            "node_attestation_profile_digests",
            "provider_profile_digest",
            "revocation_mode",
            "rotation_overlap_seconds",
            "schema",
            "trust_bundle_digest",
            "trust_domain_id",
        ],
        &[],
    )?;
    require_exact_string(block, "schema", TRUST_DOMAIN_CONTRACT_SCHEMA)?;
    Ok(TrustDomainContractSpec {
        installation_id: InstallationId::from_uuid(required_uuid(block, "installation_id")?),
        trust_domain_id: TrustDomainId::from_uuid(required_uuid(block, "trust_domain_id")?),
        name: TrustDomainName::parse(required_string(block, "name")?)?,
        provider_profile_digest: required_digest(block, "provider_profile_digest")?,
        trust_bundle_digest: required_digest(block, "trust_bundle_digest")?,
        node_attestation_profile_digests: required_digest_list(
            block,
            "node_attestation_profile_digests",
        )?,
        identity_formats: required_string_list(block, "identity_formats")?
            .iter()
            .map(|value| WorkloadIdentityFormat::parse(value))
            .collect::<Result<Vec<_>, _>>()?,
        max_credential_lifetime_seconds: required_u32(
            block,
            "max_credential_lifetime_seconds",
            false,
        )?,
        rotation_overlap_seconds: required_u32(block, "rotation_overlap_seconds", true)?,
        revocation_mode: WorkloadIdentityRevocationMode::parse(&required_string(
            block,
            "revocation_mode",
        )?)?,
        federation_bundle_digests: required_digest_list(block, "federation_bundle_digests")?,
    })
}

pub(super) fn normalize_source(
    source: &str,
    maximum_bytes: usize,
    label: &str,
) -> Result<String, String> {
    if source.is_empty() || source.len() > maximum_bytes {
        return Err(format!("{label} ACL size is invalid"));
    }
    if source.replace("\r\n", "").contains('\r') {
        return Err(format!("{label} ACL contains a bare carriage return"));
    }
    Ok(source.replace("\r\n", "\n"))
}

pub(super) fn strict_block(
    block: &Block,
    expected_name: &str,
    attributes: &[&str],
    children: &[&str],
) -> Result<(), String> {
    if block.name != expected_name
        || !block.labels.is_empty()
        || block.attributes.len() != attributes.len()
        || block
            .attributes
            .keys()
            .any(|key| !attributes.contains(&key.as_str()))
        || block.blocks.len() != children.len()
        || block
            .blocks
            .iter()
            .any(|child| !children.contains(&child.name.as_str()))
    {
        return Err(format!(
            "identity ACL block {expected_name:?} shape is invalid"
        ));
    }
    Ok(())
}

pub(super) fn exact_child<'a>(root: &'a Block, name: &str) -> Result<&'a Block, String> {
    let mut matches = root.blocks.iter().filter(|block| block.name == name);
    let value = matches
        .next()
        .ok_or_else(|| format!("identity ACL block {name:?} is required"))?;
    if matches.next().is_some() {
        return Err(format!("identity ACL block {name:?} must be unique"));
    }
    Ok(value)
}

pub(super) fn required_value<'a>(block: &'a Block, name: &str) -> Result<&'a Value, String> {
    block
        .attributes
        .get(name)
        .ok_or_else(|| format!("identity ACL field {name:?} is required"))
}

pub(super) fn required_string(block: &Block, name: &str) -> Result<String, String> {
    required_value(block, name)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("identity ACL field {name:?} must be a string"))
}

pub(super) fn required_string_list(block: &Block, name: &str) -> Result<Vec<String>, String> {
    let Value::List(values) = required_value(block, name)? else {
        return Err(format!("identity ACL field {name:?} must be a list"));
    };
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("identity ACL field {name:?} must contain strings"))
        })
        .collect()
}

pub(super) fn required_bool(block: &Block, name: &str) -> Result<bool, String> {
    match required_value(block, name)? {
        Value::Bool(value) => Ok(*value),
        _ => Err(format!("identity ACL field {name:?} must be a boolean")),
    }
}

pub(super) fn require_exact_string(
    block: &Block,
    name: &str,
    expected: &str,
) -> Result<(), String> {
    if required_string(block, name)? != expected {
        return Err(format!(
            "identity ACL field {name:?} must be exactly {expected:?}"
        ));
    }
    Ok(())
}

pub(super) fn required_uuid(block: &Block, name: &str) -> Result<Uuid, String> {
    let value = Uuid::parse_str(&required_string(block, name)?)
        .map_err(|_| format!("identity ACL field {name:?} must be a UUID"))?;
    if value.is_nil() {
        return Err(format!("identity ACL field {name:?} cannot be nil"));
    }
    Ok(value)
}

pub(super) fn required_digest(block: &Block, name: &str) -> Result<Sha256Digest, String> {
    Sha256Digest::parse(required_string(block, name)?)
        .map_err(|_| format!("identity ACL field {name:?} must be a SHA-256 digest"))
}

pub(super) fn required_digest_list(block: &Block, name: &str) -> Result<Vec<Sha256Digest>, String> {
    required_string_list(block, name)?
        .into_iter()
        .map(|value| {
            Sha256Digest::parse(value)
                .map_err(|_| format!("identity ACL field {name:?} must contain SHA-256 digests"))
        })
        .collect()
}

pub(super) fn required_u32(block: &Block, name: &str, allow_zero: bool) -> Result<u32, String> {
    let value = required_value(block, name)?
        .as_number()
        .ok_or_else(|| format!("identity ACL field {name:?} must be an integer"))?;
    if !value.is_finite()
        || value.fract() != 0.0
        || value < if allow_zero { 0.0 } else { 1.0 }
        || value > f64::from(u32::MAX)
    {
        return Err(format!(
            "identity ACL field {name:?} is outside its integer bounds"
        ));
    }
    Ok(value as u32)
}

pub(super) fn acl_integer(name: &str, value: u64, allow_zero: bool) -> Result<Value, String> {
    if value > MAX_ACL_SAFE_INTEGER || (!allow_zero && value == 0) {
        return Err(format!(
            "identity ACL field {name:?} is not exactly representable"
        ));
    }
    Ok(integer(value as i64))
}

pub(super) fn digest_list(values: &[Sha256Digest]) -> Value {
    list(values.iter().map(|value| string(value.as_str())).collect())
}

fn validate_digest(value: &Sha256Digest, label: &str) -> Result<(), String> {
    if Sha256Digest::parse(value.as_str())? != *value {
        return Err(format!("trust-domain {label} digest is not canonical"));
    }
    Ok(())
}

fn normalize_digest_set(
    values: &mut Vec<Sha256Digest>,
    minimum: usize,
    maximum: usize,
    label: &str,
) -> Result<(), String> {
    if values.len() < minimum || values.len() > maximum {
        return Err(format!("trust-domain {label} count is outside bounds"));
    }
    for value in values.iter() {
        validate_digest(value, label)?;
    }
    let unique = values.iter().cloned().collect::<BTreeSet<_>>();
    if unique.len() != values.len() {
        return Err(format!("trust-domain {label} contain duplicates"));
    }
    *values = unique.into_iter().collect();
    Ok(())
}

fn validate_normalized_digest_set(
    values: &[Sha256Digest],
    minimum: usize,
    maximum: usize,
    label: &str,
) -> Result<(), String> {
    let mut normalized = values.to_vec();
    normalize_digest_set(&mut normalized, minimum, maximum, label)?;
    if normalized != values {
        return Err(format!("trust-domain {label} are not canonical"));
    }
    Ok(())
}

fn normalize_format_set(values: &mut Vec<WorkloadIdentityFormat>) -> Result<(), String> {
    if values.is_empty() || values.len() > 2 {
        return Err("trust-domain identity format count is outside bounds".into());
    }
    let unique = values.iter().copied().collect::<BTreeSet<_>>();
    if unique.len() != values.len() {
        return Err("trust-domain identity formats contain duplicates".into());
    }
    *values = unique.into_iter().collect();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(byte: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", byte.to_string().repeat(64))).expect("digest")
    }

    fn spec() -> TrustDomainContractSpec {
        TrustDomainContractSpec {
            installation_id: InstallationId::new(),
            trust_domain_id: TrustDomainId::new(),
            name: TrustDomainName::parse("prod.example.internal").expect("name"),
            provider_profile_digest: digest('a'),
            trust_bundle_digest: digest('b'),
            node_attestation_profile_digests: vec![digest('d'), digest('c')],
            identity_formats: vec![
                WorkloadIdentityFormat::JwtSvid,
                WorkloadIdentityFormat::X509Svid,
            ],
            max_credential_lifetime_seconds: 900,
            rotation_overlap_seconds: 120,
            revocation_mode: WorkloadIdentityRevocationMode::EpochAndExpiry,
            federation_bundle_digests: vec![],
        }
    }

    #[test]
    fn canonical_acl_round_trips_and_normalizes_sets() {
        let contract = TrustDomainContract::from_spec(spec()).expect("contract");
        assert_eq!(
            contract.spec().identity_formats,
            vec![
                WorkloadIdentityFormat::X509Svid,
                WorkloadIdentityFormat::JwtSvid
            ]
        );
        assert_eq!(
            TrustDomainContract::parse_acl(contract.canonical_acl()).expect("round trip"),
            contract
        );
        contract.validate().expect("valid contract");
    }

    #[test]
    fn rejects_noncanonical_or_unsafe_trust_contracts() {
        assert!(TrustDomainName::parse("Prod.example.internal").is_err());
        assert!(TrustDomainName::parse("prod..internal").is_err());

        let mut missing_mtls = spec();
        missing_mtls.identity_formats = vec![WorkloadIdentityFormat::JwtSvid];
        assert!(TrustDomainContract::from_spec(missing_mtls).is_err());

        let mut excessive_overlap = spec();
        excessive_overlap.rotation_overlap_seconds = 451;
        assert!(TrustDomainContract::from_spec(excessive_overlap).is_err());

        let canonical = TrustDomainContract::from_spec(spec())
            .expect("contract")
            .canonical_acl()
            .to_owned();
        assert!(TrustDomainContract::parse_acl(&canonical.replace(
            "schema = \"cloud.identity.trust-domain.v1\"",
            "schema = \"cloud.identity.trust-domain.v1\"\n  unknown = true"
        ))
        .is_err());
        assert!(TrustDomainContract::parse_acl(canonical.trim_end()).is_err());
    }
}
