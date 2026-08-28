use crate::modules::identity::domain::value_objects::{
    TrustDomainContract, WorkloadIdentityPolicyContract,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, InstallationId, PrincipalId, TrustDomainId, TrustDomainRevisionId,
    WorkloadIdentityPolicyId, WorkloadIdentityPolicyRevisionId,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

const MAX_PORTABLE_REVISION_NUMBER: u64 = 9_007_199_254_740_991;
const TRUST_DOMAIN_REVISION_NAMESPACE: Uuid = Uuid::from_bytes([
    0x76, 0x84, 0xe1, 0x13, 0x47, 0x90, 0x43, 0xa6, 0xb7, 0xeb, 0x5c, 0xc0, 0x78, 0x0e, 0x3c, 0x91,
]);
const WORKLOAD_POLICY_REVISION_NAMESPACE: Uuid = Uuid::from_bytes([
    0x0e, 0x6f, 0x35, 0xbb, 0x93, 0x6f, 0x45, 0x23, 0x8a, 0xb3, 0x69, 0xf1, 0xa9, 0x78, 0x29, 0x63,
]);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedTrustDomainRevision {
    pub installation_id: InstallationId,
    pub trust_domain_id: TrustDomainId,
    pub id: TrustDomainRevisionId,
    pub revision_number: u64,
    pub contract: TrustDomainContract,
    pub accepted_by: PrincipalId,
    pub accepted_at: DateTime<Utc>,
}

impl AcceptedTrustDomainRevision {
    pub fn revision_id_for(
        trust_domain_id: TrustDomainId,
        revision_number: u64,
        contract: &TrustDomainContract,
    ) -> Result<TrustDomainRevisionId, String> {
        contract.validate()?;
        validate_revision_number(revision_number)?;
        if trust_domain_id.as_uuid().is_nil() || trust_domain_id != contract.spec().trust_domain_id
        {
            return Err("trust-domain revision owner identity is invalid".into());
        }
        Ok(TrustDomainRevisionId::from_uuid(deterministic_revision_id(
            TRUST_DOMAIN_REVISION_NAMESPACE,
            trust_domain_id.as_uuid(),
            revision_number,
            contract.digest().as_str(),
        )))
    }

    pub fn accept(
        contract: TrustDomainContract,
        revision_number: u64,
        accepted_by: PrincipalId,
        accepted_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        contract.validate()?;
        let spec = contract.spec();
        let installation_id = spec.installation_id;
        let trust_domain_id = spec.trust_domain_id;
        let id = Self::revision_id_for(trust_domain_id, revision_number, &contract)?;
        let value = Self {
            installation_id,
            trust_domain_id,
            id,
            revision_number,
            contract,
            accepted_by,
            accepted_at: canonical_timestamp(accepted_at),
        };
        value.validate()?;
        Ok(value)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        installation_id: InstallationId,
        trust_domain_id: TrustDomainId,
        id: TrustDomainRevisionId,
        revision_number: u64,
        canonical_acl: &str,
        stored_digest: &str,
        accepted_by: PrincipalId,
        accepted_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let value = Self {
            installation_id,
            trust_domain_id,
            id,
            revision_number,
            contract: TrustDomainContract::restore(canonical_acl, stored_digest)?,
            accepted_by,
            accepted_at: canonical_timestamp(accepted_at),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.contract.validate()?;
        validate_revision_number(self.revision_number)?;
        let spec = self.contract.spec();
        if self.installation_id.as_uuid().is_nil()
            || self.trust_domain_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.accepted_by.as_uuid().is_nil()
            || self.accepted_at != canonical_timestamp(self.accepted_at)
            || self.installation_id != spec.installation_id
            || self.trust_domain_id != spec.trust_domain_id
            || self.id
                != Self::revision_id_for(
                    self.trust_domain_id,
                    self.revision_number,
                    &self.contract,
                )?
        {
            return Err("accepted trust-domain revision identity or state is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedWorkloadIdentityPolicyRevision {
    pub installation_id: InstallationId,
    pub policy_id: WorkloadIdentityPolicyId,
    pub id: WorkloadIdentityPolicyRevisionId,
    pub revision_number: u64,
    pub contract: WorkloadIdentityPolicyContract,
    pub accepted_by: PrincipalId,
    pub accepted_at: DateTime<Utc>,
}

impl AcceptedWorkloadIdentityPolicyRevision {
    pub fn revision_id_for(
        policy_id: WorkloadIdentityPolicyId,
        revision_number: u64,
        contract: &WorkloadIdentityPolicyContract,
    ) -> Result<WorkloadIdentityPolicyRevisionId, String> {
        contract.validate()?;
        validate_revision_number(revision_number)?;
        if policy_id.as_uuid().is_nil() || policy_id != contract.spec().policy_id {
            return Err("workload identity policy revision owner identity is invalid".into());
        }
        Ok(WorkloadIdentityPolicyRevisionId::from_uuid(
            deterministic_revision_id(
                WORKLOAD_POLICY_REVISION_NAMESPACE,
                policy_id.as_uuid(),
                revision_number,
                contract.digest().as_str(),
            ),
        ))
    }

    pub fn accept(
        contract: WorkloadIdentityPolicyContract,
        revision_number: u64,
        accepted_by: PrincipalId,
        accepted_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        contract.validate()?;
        let installation_id = contract.spec().installation_id;
        let policy_id = contract.spec().policy_id;
        let id = Self::revision_id_for(policy_id, revision_number, &contract)?;
        let value = Self {
            installation_id,
            policy_id,
            id,
            revision_number,
            contract,
            accepted_by,
            accepted_at: canonical_timestamp(accepted_at),
        };
        value.validate()?;
        Ok(value)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        installation_id: InstallationId,
        policy_id: WorkloadIdentityPolicyId,
        id: WorkloadIdentityPolicyRevisionId,
        revision_number: u64,
        canonical_acl: &str,
        stored_digest: &str,
        accepted_by: PrincipalId,
        accepted_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let value = Self {
            installation_id,
            policy_id,
            id,
            revision_number,
            contract: WorkloadIdentityPolicyContract::restore(canonical_acl, stored_digest)?,
            accepted_by,
            accepted_at: canonical_timestamp(accepted_at),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.contract.validate()?;
        validate_revision_number(self.revision_number)?;
        let spec = self.contract.spec();
        if self.installation_id.as_uuid().is_nil()
            || self.policy_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.accepted_by.as_uuid().is_nil()
            || self.accepted_at != canonical_timestamp(self.accepted_at)
            || self.installation_id != spec.installation_id
            || self.policy_id != spec.policy_id
            || self.id
                != Self::revision_id_for(self.policy_id, self.revision_number, &self.contract)?
        {
            return Err(
                "accepted workload identity policy revision identity or state is invalid".into(),
            );
        }
        Ok(())
    }

    pub fn validate_against_trust_domain(
        &self,
        trust_domain: &AcceptedTrustDomainRevision,
    ) -> Result<(), String> {
        self.validate()?;
        trust_domain.validate()?;
        self.contract
            .spec()
            .validate_against_trust_domain(&trust_domain.contract)
    }
}

fn validate_revision_number(revision_number: u64) -> Result<(), String> {
    if revision_number == 0 || revision_number > MAX_PORTABLE_REVISION_NUMBER {
        return Err("identity revision number is outside portable bounds".into());
    }
    Ok(())
}

fn deterministic_revision_id(
    namespace: Uuid,
    owner_id: Uuid,
    revision_number: u64,
    contract_digest: &str,
) -> Uuid {
    let mut identity = Vec::with_capacity(24 + contract_digest.len());
    identity.extend_from_slice(owner_id.as_bytes());
    identity.extend_from_slice(&revision_number.to_be_bytes());
    identity.extend_from_slice(contract_digest.as_bytes());
    Uuid::new_v5(&namespace, &identity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::value_objects::{
        PrivateServiceName, TrustDomainContractSpec, TrustDomainName, WorkloadIdentityAudience,
        WorkloadIdentityFormat, WorkloadIdentityPolicySpec, WorkloadIdentityRevocationMode,
        WorkloadProductRole,
    };
    use crate::modules::shared_kernel::domain::{
        EnvironmentId, NodePoolId, OrganizationId, ProjectId, Sha256Digest, WorkloadId,
        WorkloadRevisionId,
    };
    use a3s_cloud_contracts::{RuntimeIsolationLevel, RuntimeUnitClass};

    fn digest(byte: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", byte.to_string().repeat(64))).expect("digest")
    }

    fn contracts() -> (TrustDomainContract, WorkloadIdentityPolicyContract) {
        let installation_id = InstallationId::new();
        let trust_domain_id = TrustDomainId::new();
        let trust = TrustDomainContract::from_spec(TrustDomainContractSpec {
            installation_id,
            trust_domain_id,
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
        let policy = WorkloadIdentityPolicyContract::from_spec(WorkloadIdentityPolicySpec {
            installation_id,
            trust_domain_id,
            organization_id: OrganizationId::new(),
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
            policy_id: WorkloadIdentityPolicyId::new(),
            workload_id: WorkloadId::new(),
            workload_revision_id: WorkloadRevisionId::new(),
            product_role: WorkloadProductRole::AgentService,
            runtime_class: RuntimeUnitClass::Service,
            semantics_profile_digest: digest('d'),
            node_pool_id: NodePoolId::new(),
            isolation_level: RuntimeIsolationLevel::Container,
            attestation_profile_digest: digest('c'),
            confidential_compute: false,
            identity_formats: vec![WorkloadIdentityFormat::X509Svid],
            credential_lifetime_seconds: 300,
            rotate_before_expiry_seconds: 60,
            drain_on_rotation_failure: true,
            revoke_on_stop: true,
            audiences: vec![WorkloadIdentityAudience::parse("model.internal").expect("audience")],
            service_names: vec![PrivateServiceName::parse("agent.prod.internal").expect("service")],
            peer_policy_revision_digests: vec![],
        })
        .expect("policy");
        (trust, policy)
    }

    #[test]
    fn revision_identity_is_deterministic_and_contract_bound() {
        let (trust_contract, policy_contract) = contracts();
        let principal_id = PrincipalId::new();
        let now = Utc::now();
        let trust =
            AcceptedTrustDomainRevision::accept(trust_contract.clone(), 1, principal_id, now)
                .expect("trust revision");
        let repeated = AcceptedTrustDomainRevision::accept(trust_contract, 1, principal_id, now)
            .expect("repeat");
        assert_eq!(trust.id, repeated.id);

        let policy =
            AcceptedWorkloadIdentityPolicyRevision::accept(policy_contract, 1, principal_id, now)
                .expect("policy revision");
        policy
            .validate_against_trust_domain(&trust)
            .expect("admitted policy");
    }

    #[test]
    fn rejects_nonportable_or_forged_revision_identity() {
        let (trust_contract, _) = contracts();
        assert!(AcceptedTrustDomainRevision::accept(
            trust_contract.clone(),
            0,
            PrincipalId::new(),
            Utc::now()
        )
        .is_err());
        let mut revision =
            AcceptedTrustDomainRevision::accept(trust_contract, 1, PrincipalId::new(), Utc::now())
                .expect("revision");
        revision.id = TrustDomainRevisionId::new();
        assert!(revision.validate().is_err());
    }
}
