use super::{TrustDomainName, MIN_WORKLOAD_CREDENTIAL_LIFETIME_SECONDS};
use crate::modules::shared_kernel::domain::Sha256Digest;
use a3s_acl::builder::{boolean, integer, list, string, BlockBuilder};
use a3s_acl::{canonical_digest, generate_acl, parse_acl, Block, Document, Value};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const PROFILE_BLOCK: &str = "workload_identity_provider";
const PROFILE_LABEL: &str = "spiffe_https_web";
const PROFILE_ATTRIBUTES: [&str; 7] = [
    "bundle_endpoint_url",
    "max_credential_lifetime_seconds",
    "node_attestation_profile_digests",
    "schema",
    "supports_revocation_epochs",
    "tls_trust_anchor_digest",
    "trust_domain",
];
pub const WORKLOAD_IDENTITY_PROVIDER_PROFILE_SCHEMA: &str = "cloud.identity.workload-provider.v1";
pub const WORKLOAD_IDENTITY_PROVIDER_PROFILE_MAX_ACL_BYTES: usize = 16 * 1024;
pub const MAX_WORKLOAD_IDENTITY_PROVIDER_ATTESTATION_PROFILES: usize = 64;
pub const MAX_WORKLOAD_IDENTITY_PROVIDER_CREDENTIAL_LIFETIME_SECONDS: u32 = 86_400;

/// One immutable, non-secret workload-identity provider binding.
///
/// The profile is Identity's semantic reference to replaceable Infrastructure.
/// It contains no private key, bearer credential, provider registration row,
/// connection timeout, local file path, or mutable provider observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkloadIdentityProviderProfileSpec {
    pub trust_domain: TrustDomainName,
    pub bundle_endpoint_url: String,
    /// Empty selects the normal Web PKI root set. A digest selects one exact
    /// locally supplied PEM trust-anchor bundle.
    pub tls_trust_anchor_digest: Option<Sha256Digest>,
    pub node_attestation_profile_digests: Vec<Sha256Digest>,
    pub max_credential_lifetime_seconds: u32,
    pub supports_revocation_epochs: bool,
}

impl WorkloadIdentityProviderProfileSpec {
    fn normalize(mut self) -> Result<Self, String> {
        self.trust_domain = TrustDomainName::parse(self.trust_domain.as_str())?;
        self.bundle_endpoint_url = canonical_bundle_endpoint_url(&self.bundle_endpoint_url)?;
        if let Some(digest) = &self.tls_trust_anchor_digest {
            validate_digest(digest, "TLS trust anchor")?;
        }
        normalize_digest_set(&mut self.node_attestation_profile_digests)?;
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        TrustDomainName::parse(self.trust_domain.as_str())?;
        if self.bundle_endpoint_url != canonical_bundle_endpoint_url(&self.bundle_endpoint_url)? {
            return Err("workload identity provider bundle endpoint URL is not canonical".into());
        }
        if let Some(digest) = &self.tls_trust_anchor_digest {
            validate_digest(digest, "TLS trust anchor")?;
        }
        validate_normalized_digest_set(&self.node_attestation_profile_digests)?;
        if !(MIN_WORKLOAD_CREDENTIAL_LIFETIME_SECONDS
            ..=MAX_WORKLOAD_IDENTITY_PROVIDER_CREDENTIAL_LIFETIME_SECONDS)
            .contains(&self.max_credential_lifetime_seconds)
        {
            return Err("workload identity provider credential lifetime is outside bounds".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkloadIdentityProviderProfile {
    spec: WorkloadIdentityProviderProfileSpec,
    canonical_acl: String,
    digest: Sha256Digest,
}

impl WorkloadIdentityProviderProfile {
    pub fn from_spec(spec: WorkloadIdentityProviderProfileSpec) -> Result<Self, String> {
        let spec = spec.normalize()?;
        let document = profile_document(&spec);
        let canonical_acl = generate_acl(&document);
        if canonical_acl.len() > WORKLOAD_IDENTITY_PROVIDER_PROFILE_MAX_ACL_BYTES {
            return Err("workload identity provider profile ACL exceeds its storage bound".into());
        }
        let reparsed = parse_acl(&canonical_acl).map_err(|error| {
            format!("generated workload identity provider profile ACL is invalid: {error}")
        })?;
        let digest = Sha256Digest::parse(canonical_digest(&reparsed).map_err(|error| {
            format!("workload identity provider profile is not canonicalizable: {error}")
        })?)?;
        Ok(Self {
            spec,
            canonical_acl,
            digest,
        })
    }

    pub fn parse_acl(source: &str) -> Result<Self, String> {
        if source.is_empty() || source.len() > WORKLOAD_IDENTITY_PROVIDER_PROFILE_MAX_ACL_BYTES {
            return Err("workload identity provider profile ACL size is invalid".into());
        }
        let document = parse_acl(source).map_err(|error| {
            format!("workload identity provider profile ACL is invalid: {error}")
        })?;
        let profile = Self::from_spec(parse_profile(&document)?)?;
        if profile.canonical_acl != source {
            return Err("workload identity provider profile ACL is not canonical".into());
        }
        Ok(profile)
    }

    pub fn restore(source: &str, stored_digest: &str) -> Result<Self, String> {
        let profile = Self::parse_acl(source)?;
        if profile.digest.as_str() != stored_digest {
            return Err(
                "stored workload identity provider profile ACL and digest do not match".into(),
            );
        }
        Ok(profile)
    }

    pub fn validate(&self) -> Result<(), String> {
        if Self::restore(self.canonical_acl(), self.digest.as_str())? != *self {
            return Err("workload identity provider profile drifted from canonical ACL".into());
        }
        Ok(())
    }

    pub const fn spec(&self) -> &WorkloadIdentityProviderProfileSpec {
        &self.spec
    }

    pub fn canonical_acl(&self) -> &str {
        &self.canonical_acl
    }

    pub const fn digest(&self) -> &Sha256Digest {
        &self.digest
    }
}

fn profile_document(spec: &WorkloadIdentityProviderProfileSpec) -> Document {
    Document {
        blocks: vec![BlockBuilder::new(PROFILE_BLOCK)
            .label(PROFILE_LABEL)
            .attr("schema", string(WORKLOAD_IDENTITY_PROVIDER_PROFILE_SCHEMA))
            .attr("trust_domain", string(spec.trust_domain.as_str()))
            .attr("bundle_endpoint_url", string(&spec.bundle_endpoint_url))
            .attr(
                "tls_trust_anchor_digest",
                string(
                    spec.tls_trust_anchor_digest
                        .as_ref()
                        .map_or("", Sha256Digest::as_str),
                ),
            )
            .attr(
                "node_attestation_profile_digests",
                list(
                    spec.node_attestation_profile_digests
                        .iter()
                        .map(|digest| string(digest.as_str()))
                        .collect(),
                ),
            )
            .attr(
                "max_credential_lifetime_seconds",
                integer(i64::from(spec.max_credential_lifetime_seconds)),
            )
            .attr(
                "supports_revocation_epochs",
                boolean(spec.supports_revocation_epochs),
            )
            .build()],
    }
}

fn parse_profile(document: &Document) -> Result<WorkloadIdentityProviderProfileSpec, String> {
    if document.blocks.len() != 1 {
        return Err("workload identity provider profile must contain exactly one block".into());
    }
    let block = &document.blocks[0];
    if block.name != PROFILE_BLOCK
        || block.labels.as_slice() != [PROFILE_LABEL]
        || !block.blocks.is_empty()
        || block.attributes.len() != PROFILE_ATTRIBUTES.len()
        || block
            .attributes
            .keys()
            .any(|key| !PROFILE_ATTRIBUTES.contains(&key.as_str()))
    {
        return Err("workload identity provider profile block shape is invalid".into());
    }
    require_exact_string(block, "schema", WORKLOAD_IDENTITY_PROVIDER_PROFILE_SCHEMA)?;
    let tls_trust_anchor_digest = required_string(block, "tls_trust_anchor_digest")?;
    Ok(WorkloadIdentityProviderProfileSpec {
        trust_domain: TrustDomainName::parse(required_string(block, "trust_domain")?)?,
        bundle_endpoint_url: required_string(block, "bundle_endpoint_url")?,
        tls_trust_anchor_digest: if tls_trust_anchor_digest.is_empty() {
            None
        } else {
            Some(Sha256Digest::parse(tls_trust_anchor_digest)?)
        },
        node_attestation_profile_digests: required_string_list(
            block,
            "node_attestation_profile_digests",
        )?
        .into_iter()
        .map(Sha256Digest::parse)
        .collect::<Result<_, _>>()?,
        max_credential_lifetime_seconds: required_u32(block, "max_credential_lifetime_seconds")?,
        supports_revocation_epochs: required_boolean(block, "supports_revocation_epochs")?,
    })
}

fn required_string(block: &Block, name: &str) -> Result<String, String> {
    match block.attributes.get(name) {
        Some(Value::String(value)) => Ok(value.clone()),
        Some(_) => Err(format!(
            "workload identity provider profile field {name:?} must be a string"
        )),
        None => Err(format!(
            "workload identity provider profile field {name:?} is required"
        )),
    }
}

fn require_exact_string(block: &Block, name: &str, expected: &str) -> Result<(), String> {
    if required_string(block, name)? != expected {
        return Err(format!(
            "workload identity provider profile field {name:?} must be exactly {expected:?}"
        ));
    }
    Ok(())
}

fn required_string_list(block: &Block, name: &str) -> Result<Vec<String>, String> {
    let Some(Value::List(values)) = block.attributes.get(name) else {
        return Err(format!(
            "workload identity provider profile field {name:?} must be a list"
        ));
    };
    values
        .iter()
        .map(|value| match value {
            Value::String(value) => Ok(value.clone()),
            _ => Err(format!(
                "workload identity provider profile field {name:?} must contain strings"
            )),
        })
        .collect()
}

fn required_boolean(block: &Block, name: &str) -> Result<bool, String> {
    match block.attributes.get(name) {
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(format!(
            "workload identity provider profile field {name:?} must be a boolean"
        )),
        None => Err(format!(
            "workload identity provider profile field {name:?} is required"
        )),
    }
}

fn required_u32(block: &Block, name: &str) -> Result<u32, String> {
    let number = block
        .attributes
        .get(name)
        .and_then(Value::as_number)
        .ok_or_else(|| {
            format!("workload identity provider profile field {name:?} must be an integer")
        })?;
    if !number.is_finite() || number.fract() != 0.0 || number < 0.0 || number > f64::from(u32::MAX)
    {
        return Err(format!(
            "workload identity provider profile field {name:?} is outside integer bounds"
        ));
    }
    Ok(number as u32)
}

fn canonical_bundle_endpoint_url(value: &str) -> Result<String, String> {
    if value.is_empty() || value.len() > 2048 || value.contains(['\0', '\r', '\n', ' ', '\t']) {
        return Err("workload identity provider bundle endpoint URL is invalid".into());
    }
    let endpoint = url::Url::parse(value)
        .map_err(|_| "workload identity provider bundle endpoint URL is invalid".to_owned())?;
    if endpoint.scheme() != "https"
        || endpoint.host_str().is_none()
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.fragment().is_some()
    {
        return Err(
            "workload identity provider requires an HTTPS bundle endpoint without userinfo or fragment"
                .into(),
        );
    }
    Ok(endpoint.to_string())
}

fn validate_digest(value: &Sha256Digest, label: &str) -> Result<(), String> {
    if Sha256Digest::parse(value.as_str())? != *value {
        return Err(format!(
            "workload identity provider {label} digest is not canonical"
        ));
    }
    Ok(())
}

fn normalize_digest_set(values: &mut Vec<Sha256Digest>) -> Result<(), String> {
    if values.is_empty() || values.len() > MAX_WORKLOAD_IDENTITY_PROVIDER_ATTESTATION_PROFILES {
        return Err(
            "workload identity provider node-attestation profile count is outside bounds".into(),
        );
    }
    for value in values.iter() {
        validate_digest(value, "node-attestation profile")?;
    }
    let canonical = values.iter().cloned().collect::<BTreeSet<_>>();
    if canonical.len() != values.len() {
        return Err(
            "workload identity provider node-attestation profiles contain duplicates".into(),
        );
    }
    *values = canonical.into_iter().collect();
    Ok(())
}

fn validate_normalized_digest_set(values: &[Sha256Digest]) -> Result<(), String> {
    let mut normalized = values.to_vec();
    normalize_digest_set(&mut normalized)?;
    if normalized != values {
        return Err(
            "workload identity provider node-attestation profiles are not canonical".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(byte: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", byte.to_string().repeat(64))).expect("digest")
    }

    fn spec() -> WorkloadIdentityProviderProfileSpec {
        WorkloadIdentityProviderProfileSpec {
            trust_domain: TrustDomainName::parse("prod.example.internal").expect("trust domain"),
            bundle_endpoint_url: "https://identity.example.internal:8443/bundle?format=jwk".into(),
            tls_trust_anchor_digest: Some(digest('a')),
            node_attestation_profile_digests: vec![digest('c'), digest('b')],
            max_credential_lifetime_seconds: 900,
            supports_revocation_epochs: false,
        }
    }

    #[test]
    fn canonical_acl_round_trips_and_normalizes_profile_sets() {
        let profile = WorkloadIdentityProviderProfile::from_spec(spec()).expect("profile");
        assert_eq!(
            profile.spec().node_attestation_profile_digests,
            vec![digest('b'), digest('c')]
        );
        assert_eq!(
            WorkloadIdentityProviderProfile::parse_acl(profile.canonical_acl())
                .expect("round trip"),
            profile
        );
        profile.validate().expect("valid profile");
    }

    #[test]
    fn rejects_unsafe_endpoints_duplicate_profiles_and_acl_drift() {
        for endpoint in [
            "http://identity.example.internal/bundle",
            "https://user@identity.example.internal/bundle",
            "https://identity.example.internal/bundle#fragment",
        ] {
            let mut invalid = spec();
            invalid.bundle_endpoint_url = endpoint.into();
            assert!(WorkloadIdentityProviderProfile::from_spec(invalid).is_err());
        }

        let mut duplicate = spec();
        duplicate.node_attestation_profile_digests = vec![digest('b'), digest('b')];
        assert!(WorkloadIdentityProviderProfile::from_spec(duplicate).is_err());

        let canonical = WorkloadIdentityProviderProfile::from_spec(spec())
            .expect("profile")
            .canonical_acl()
            .to_owned();
        assert!(
            WorkloadIdentityProviderProfile::parse_acl(&canonical.replace(
                "schema = \"cloud.identity.workload-provider.v1\"",
                "schema = \"cloud.identity.workload-provider.v1\"\n  unknown = true"
            ))
            .is_err()
        );
        assert!(WorkloadIdentityProviderProfile::parse_acl(&format!("{canonical}\n")).is_err());
    }
}
