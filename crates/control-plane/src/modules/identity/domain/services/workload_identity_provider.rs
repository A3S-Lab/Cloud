use crate::modules::identity::domain::value_objects::{
    TrustDomainContract, TrustDomainName, WorkloadIdentityFormat, WorkloadIdentityRevocationMode,
    MAX_TRUST_DOMAIN_FEDERATION_BUNDLES, MAX_WORKLOAD_IDENTITY_PROVIDER_ATTESTATION_PROFILES,
    MAX_WORKLOAD_IDENTITY_PROVIDER_CREDENTIAL_LIFETIME_SECONDS,
    MIN_WORKLOAD_CREDENTIAL_LIFETIME_SECONDS,
};
use crate::modules::shared_kernel::domain::Sha256Digest;
use async_trait::async_trait;
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadIdentityProviderInspection {
    pub provider_profile_digest: Sha256Digest,
    pub trust_domain_name: TrustDomainName,
    pub observed_trust_bundle_digest: Sha256Digest,
    pub observed_federation_bundle_digests: Vec<Sha256Digest>,
    pub observed_identity_formats: Vec<WorkloadIdentityFormat>,
    pub declared_node_attestation_profile_digests: Vec<Sha256Digest>,
    pub declared_max_credential_lifetime_seconds: u32,
    pub declared_supports_revocation_epochs: bool,
}

impl WorkloadIdentityProviderInspection {
    pub fn validate(&self) -> Result<(), String> {
        validate_digest(&self.provider_profile_digest, "provider profile")?;
        TrustDomainName::parse(self.trust_domain_name.as_str())?;
        validate_digest(&self.observed_trust_bundle_digest, "observed trust bundle")?;
        validate_set(
            &self.observed_federation_bundle_digests,
            0,
            MAX_TRUST_DOMAIN_FEDERATION_BUNDLES,
            "observed federation bundles",
        )?;
        for digest in &self.observed_federation_bundle_digests {
            validate_digest(digest, "observed federation bundle")?;
        }
        validate_set(
            &self.observed_identity_formats,
            1,
            2,
            "observed identity formats",
        )?;
        validate_set(
            &self.declared_node_attestation_profile_digests,
            1,
            MAX_WORKLOAD_IDENTITY_PROVIDER_ATTESTATION_PROFILES,
            "node-attestation profiles",
        )?;
        for digest in &self.declared_node_attestation_profile_digests {
            validate_digest(digest, "node-attestation profile")?;
        }
        if !(MIN_WORKLOAD_CREDENTIAL_LIFETIME_SECONDS
            ..=MAX_WORKLOAD_IDENTITY_PROVIDER_CREDENTIAL_LIFETIME_SECONDS)
            .contains(&self.declared_max_credential_lifetime_seconds)
        {
            return Err("workload identity provider credential bound is invalid".into());
        }
        Ok(())
    }

    pub fn admits(&self, trust_domain: &TrustDomainContract) -> Result<(), String> {
        self.validate()?;
        trust_domain.validate()?;
        let trust = trust_domain.spec();
        if self.provider_profile_digest != trust.provider_profile_digest
            || self.trust_domain_name != trust.name
            || self.observed_trust_bundle_digest != trust.trust_bundle_digest
            || self.observed_federation_bundle_digests != trust.federation_bundle_digests
            || self.declared_max_credential_lifetime_seconds < trust.max_credential_lifetime_seconds
            || !trust
                .identity_formats
                .iter()
                .all(|format| self.observed_identity_formats.contains(format))
            || !trust
                .node_attestation_profile_digests
                .iter()
                .all(|profile| {
                    self.declared_node_attestation_profile_digests
                        .contains(profile)
                })
            || (trust.revocation_mode == WorkloadIdentityRevocationMode::EpochAndExpiry
                && !self.declared_supports_revocation_epochs)
        {
            return Err(
                "workload identity provider cannot satisfy the trust-domain contract".into(),
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WorkloadIdentityProviderError {
    #[error("workload identity provider profile is not configured: {0}")]
    NotConfigured(String),
    #[error("workload identity provider is unavailable: {0}")]
    Unavailable(String),
    #[error("workload identity provider rejected the binding: {0}")]
    Rejected(String),
    #[error("workload identity provider does not support the requested contract: {0}")]
    Unsupported(String),
    #[error("workload identity provider returned an invalid observation: {0}")]
    InvalidObservation(String),
}

/// Replaceable Infrastructure port for inspecting a configured identity
/// provider. An inspection keeps provider-profile declarations distinct from
/// facts observed at the external bundle endpoint. It deliberately exposes no
/// private key or provider registration database. Issuance and local workload
/// delivery are added only by WI3 after exact Fleet/Runtime attestation exists.
#[async_trait]
pub trait IWorkloadIdentityProviderService: Send + Sync {
    async fn inspect(
        &self,
        provider_profile_digest: &Sha256Digest,
    ) -> Result<WorkloadIdentityProviderInspection, WorkloadIdentityProviderError>;
}

fn validate_digest(value: &Sha256Digest, label: &str) -> Result<(), String> {
    if Sha256Digest::parse(value.as_str())? != *value {
        return Err(format!(
            "workload identity provider {label} is not canonical"
        ));
    }
    Ok(())
}

fn validate_set<T: Clone + Ord + PartialEq>(
    values: &[T],
    minimum: usize,
    maximum: usize,
    label: &str,
) -> Result<(), String> {
    if values.len() < minimum || values.len() > maximum {
        return Err(format!(
            "workload identity provider {label} count is outside bounds"
        ));
    }
    let canonical = values.iter().cloned().collect::<BTreeSet<_>>();
    if canonical.len() != values.len() || canonical.into_iter().collect::<Vec<_>>() != values {
        return Err(format!(
            "workload identity provider {label} are not a canonical set"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::value_objects::{
        TrustDomainContractSpec, TrustDomainName,
    };
    use crate::modules::shared_kernel::domain::{InstallationId, TrustDomainId};

    fn digest(byte: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", byte.to_string().repeat(64))).expect("digest")
    }

    #[test]
    fn exact_inspection_admits_the_trust_contract() {
        let trust = TrustDomainContract::from_spec(TrustDomainContractSpec {
            installation_id: InstallationId::new(),
            trust_domain_id: TrustDomainId::new(),
            name: TrustDomainName::parse("prod.example.internal").expect("name"),
            provider_profile_digest: digest('a'),
            trust_bundle_digest: digest('b'),
            node_attestation_profile_digests: vec![digest('c')],
            identity_formats: vec![WorkloadIdentityFormat::X509Svid],
            max_credential_lifetime_seconds: 600,
            rotation_overlap_seconds: 60,
            revocation_mode: WorkloadIdentityRevocationMode::EpochAndExpiry,
            federation_bundle_digests: vec![],
        })
        .expect("trust");
        let inspection = WorkloadIdentityProviderInspection {
            provider_profile_digest: digest('a'),
            trust_domain_name: TrustDomainName::parse("prod.example.internal").expect("name"),
            observed_trust_bundle_digest: digest('b'),
            observed_federation_bundle_digests: vec![],
            observed_identity_formats: vec![WorkloadIdentityFormat::X509Svid],
            declared_node_attestation_profile_digests: vec![digest('c')],
            declared_max_credential_lifetime_seconds: 900,
            declared_supports_revocation_epochs: true,
        };
        inspection.admits(&trust).expect("admitted");
    }

    #[test]
    fn provider_inspection_drift_fails_closed() {
        let trust = TrustDomainContract::from_spec(TrustDomainContractSpec {
            installation_id: InstallationId::new(),
            trust_domain_id: TrustDomainId::new(),
            name: TrustDomainName::parse("prod.example.internal").expect("name"),
            provider_profile_digest: digest('a'),
            trust_bundle_digest: digest('b'),
            node_attestation_profile_digests: vec![digest('c')],
            identity_formats: vec![WorkloadIdentityFormat::X509Svid],
            max_credential_lifetime_seconds: 600,
            rotation_overlap_seconds: 60,
            revocation_mode: WorkloadIdentityRevocationMode::EpochAndExpiry,
            federation_bundle_digests: vec![],
        })
        .expect("trust");
        let inspection = WorkloadIdentityProviderInspection {
            provider_profile_digest: digest('a'),
            trust_domain_name: TrustDomainName::parse("prod.example.internal").expect("name"),
            observed_trust_bundle_digest: digest('9'),
            observed_federation_bundle_digests: vec![],
            observed_identity_formats: vec![WorkloadIdentityFormat::X509Svid],
            declared_node_attestation_profile_digests: vec![digest('c')],
            declared_max_credential_lifetime_seconds: 900,
            declared_supports_revocation_epochs: true,
        };
        assert!(inspection.admits(&trust).is_err());
    }

    #[test]
    fn federation_bundle_observation_must_match_exactly() {
        let trust = TrustDomainContract::from_spec(TrustDomainContractSpec {
            installation_id: InstallationId::new(),
            trust_domain_id: TrustDomainId::new(),
            name: TrustDomainName::parse("prod.example.internal").expect("name"),
            provider_profile_digest: digest('a'),
            trust_bundle_digest: digest('b'),
            node_attestation_profile_digests: vec![digest('c')],
            identity_formats: vec![WorkloadIdentityFormat::X509Svid],
            max_credential_lifetime_seconds: 600,
            rotation_overlap_seconds: 60,
            revocation_mode: WorkloadIdentityRevocationMode::EpochAndExpiry,
            federation_bundle_digests: vec![digest('d')],
        })
        .expect("trust");
        let inspection = WorkloadIdentityProviderInspection {
            provider_profile_digest: digest('a'),
            trust_domain_name: TrustDomainName::parse("prod.example.internal").expect("name"),
            observed_trust_bundle_digest: digest('b'),
            observed_federation_bundle_digests: vec![digest('e')],
            observed_identity_formats: vec![WorkloadIdentityFormat::X509Svid],
            declared_node_attestation_profile_digests: vec![digest('c')],
            declared_max_credential_lifetime_seconds: 900,
            declared_supports_revocation_epochs: true,
        };
        assert!(inspection.admits(&trust).is_err());
    }
}
