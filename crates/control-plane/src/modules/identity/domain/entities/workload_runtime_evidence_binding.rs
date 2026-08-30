use super::AcceptedWorkloadIdentityPolicyRevision;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, InstallationId, NodeId, NodePoolId, OrganizationId,
    ProjectId, ResourceClaimId, Sha256Digest, WorkloadId, WorkloadIdentityPolicyId,
    WorkloadIdentityPolicyRevisionId, WorkloadRevisionId,
};
use a3s_cloud_contracts::{RuntimeIsolationLevel, RuntimeUnitClass, RuntimeUnitState};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkloadRuntimeEvidenceBindingId(Uuid);

impl WorkloadRuntimeEvidenceBindingId {
    pub(in crate::modules::identity) const fn from_uuid(value: Uuid) -> Self {
        Self(value)
    }

    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

pub const WORKLOAD_RUNTIME_EVIDENCE_BINDING_SCHEMA: &str =
    "cloud.identity.workload-runtime-evidence-binding.v1";
pub const WORKLOAD_RUNTIME_EVIDENCE_RECORD_SCHEMA: &str =
    "cloud.identity.workload-runtime-evidence-record.v1";
pub const MAX_WORKLOAD_RUNTIME_EVIDENCE_AGE_SECONDS: i64 = 120;
const MAX_PORTABLE_EVIDENCE_VERSION: u64 = 9_007_199_254_740_991;

const WORKLOAD_RUNTIME_EVIDENCE_BINDING_NAMESPACE: Uuid = Uuid::from_bytes([
    0x99, 0xa7, 0x12, 0xd1, 0x53, 0x6f, 0x43, 0xa8, 0x9b, 0x21, 0xa4, 0xa6, 0x06, 0x36, 0xc1, 0xe8,
]);

/// Identity's normalized copy of exact facts obtained through its one future
/// Workloads/Fleet anti-corruption port. It owns no foreign lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkloadRuntimeEvidenceCandidate {
    pub installation_id: InstallationId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub workload_id: WorkloadId,
    pub workload_revision_id: WorkloadRevisionId,
    pub resource_claim_id: ResourceClaimId,
    pub resource_claim_generation: u64,
    pub resource_claim_aggregate_version: u64,
    pub resource_claim_digest: Sha256Digest,
    pub resource_binding_digest: Sha256Digest,
    pub node_pool_id: NodePoolId,
    pub node_pool_aggregate_version: u64,
    pub node_pool_spec_digest: Sha256Digest,
    pub node_id: NodeId,
    pub node_aggregate_version: u64,
    pub agent_instance_id: Uuid,
    pub node_capabilities_digest: Sha256Digest,
    pub node_last_observed_at: DateTime<Utc>,
    pub runtime_report_id: Uuid,
    pub runtime_unit_id: String,
    pub runtime_generation: u64,
    pub runtime_class: RuntimeUnitClass,
    pub isolation_level: RuntimeIsolationLevel,
    pub semantics_profile_digest: Sha256Digest,
    pub identity_attachment_digest: Sha256Digest,
    pub runtime_spec_digest: Sha256Digest,
    pub runtime_attestation_binding_digest: Sha256Digest,
    pub provider_attestation_digest: Sha256Digest,
    pub provider_resource_id: String,
    pub provider_build: String,
    pub runtime_state: RuntimeUnitState,
    pub runtime_observed_at: DateTime<Utc>,
    pub runtime_received_at: DateTime<Utc>,
}

impl WorkloadRuntimeEvidenceCandidate {
    fn canonicalize(&mut self) {
        self.node_last_observed_at = canonical_timestamp(self.node_last_observed_at);
        self.runtime_observed_at = canonical_timestamp(self.runtime_observed_at);
        self.runtime_received_at = canonical_timestamp(self.runtime_received_at);
    }

    pub fn validate(&self) -> Result<(), String> {
        for (label, value) in [
            ("resource Claim", &self.resource_claim_digest),
            ("resource binding", &self.resource_binding_digest),
            ("node pool", &self.node_pool_spec_digest),
            ("node capabilities", &self.node_capabilities_digest),
            ("Runtime semantics", &self.semantics_profile_digest),
            ("identity attachment", &self.identity_attachment_digest),
            ("Runtime specification", &self.runtime_spec_digest),
            (
                "Runtime attestation binding",
                &self.runtime_attestation_binding_digest,
            ),
            ("provider attestation", &self.provider_attestation_digest),
        ] {
            if Sha256Digest::parse(value.as_str())? != *value {
                return Err(format!("workload Runtime {label} digest is not canonical"));
            }
        }
        if self.installation_id.as_uuid().is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.workload_id.as_uuid().is_nil()
            || self.workload_revision_id.as_uuid().is_nil()
            || self.resource_claim_id.as_uuid().is_nil()
            || self.resource_claim_generation == 0
            || self.resource_claim_generation > MAX_PORTABLE_EVIDENCE_VERSION
            || self.resource_claim_aggregate_version == 0
            || self.resource_claim_aggregate_version > MAX_PORTABLE_EVIDENCE_VERSION
            || self.node_pool_id.as_uuid().is_nil()
            || self.node_pool_aggregate_version == 0
            || self.node_pool_aggregate_version > MAX_PORTABLE_EVIDENCE_VERSION
            || self.node_id.as_uuid().is_nil()
            || self.node_aggregate_version == 0
            || self.node_aggregate_version > MAX_PORTABLE_EVIDENCE_VERSION
            || self.agent_instance_id.is_nil()
            || self.runtime_report_id.is_nil()
            || self.runtime_generation == 0
            || self.runtime_generation > MAX_PORTABLE_EVIDENCE_VERSION
        {
            return Err("workload Runtime evidence identity or generation is invalid".into());
        }
        validate_single_line("Runtime unit ID", &self.runtime_unit_id, 512)?;
        validate_single_line("provider resource ID", &self.provider_resource_id, 1024)?;
        validate_single_line("provider build", &self.provider_build, 255)?;
        if self.runtime_state != RuntimeUnitState::Running {
            return Err("workload Runtime evidence is not a running Unit observation".into());
        }
        if self.node_last_observed_at.timestamp_millis() <= 0
            || self.runtime_observed_at.timestamp_millis() <= 0
            || self.runtime_received_at.timestamp_millis() <= 0
            || self.node_last_observed_at != canonical_timestamp(self.node_last_observed_at)
            || self.runtime_observed_at != canonical_timestamp(self.runtime_observed_at)
            || self.runtime_received_at != canonical_timestamp(self.runtime_received_at)
            || self.runtime_received_at < self.runtime_observed_at
            || self.node_last_observed_at < self.runtime_observed_at
        {
            return Err("workload Runtime evidence timestamps are invalid or reordered".into());
        }
        Ok(())
    }

    fn validate_fresh_at(&self, evaluated_at: DateTime<Utc>) -> Result<(), String> {
        let evaluated_at = canonical_timestamp(evaluated_at);
        if evaluated_at < self.runtime_received_at
            || evaluated_at < self.node_last_observed_at
            || evaluated_at - self.runtime_observed_at
                > Duration::seconds(MAX_WORKLOAD_RUNTIME_EVIDENCE_AGE_SECONDS)
            || evaluated_at - self.node_last_observed_at
                > Duration::seconds(MAX_WORKLOAD_RUNTIME_EVIDENCE_AGE_SECONDS)
        {
            return Err("workload Runtime or Node evidence is stale at admission".into());
        }
        Ok(())
    }
}

/// Deterministic Identity-owned evidence fact for WI2-C1.
///
/// This binds an accepted policy to one exact Workloads Claim, Fleet Node
/// session/capability snapshot, and provider-attested Runtime Unit generation.
/// V1 deliberately contains no Node hardware-attestation binding and therefore
/// cannot authorize credential issuance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkloadRuntimeEvidenceBinding {
    pub schema: String,
    pub id: WorkloadRuntimeEvidenceBindingId,
    pub policy_id: WorkloadIdentityPolicyId,
    pub policy_revision_id: WorkloadIdentityPolicyRevisionId,
    pub policy_revision_number: u64,
    pub policy_digest: Sha256Digest,
    pub candidate: WorkloadRuntimeEvidenceCandidate,
    pub node_attestation_binding_digest: Option<Sha256Digest>,
    pub binding_digest: Sha256Digest,
}

impl WorkloadRuntimeEvidenceBinding {
    pub fn bind_runtime_component(
        policy: &AcceptedWorkloadIdentityPolicyRevision,
        mut candidate: WorkloadRuntimeEvidenceCandidate,
        evaluated_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        policy.validate()?;
        candidate.canonicalize();
        candidate.validate()?;
        candidate.validate_fresh_at(evaluated_at)?;
        validate_candidate_against_policy(policy, &candidate)?;
        let mut value = Self {
            schema: WORKLOAD_RUNTIME_EVIDENCE_BINDING_SCHEMA.into(),
            id: WorkloadRuntimeEvidenceBindingId::from_uuid(Uuid::nil()),
            policy_id: policy.policy_id,
            policy_revision_id: policy.id,
            policy_revision_number: policy.revision_number,
            policy_digest: policy.contract.digest().clone(),
            candidate,
            node_attestation_binding_digest: None,
            binding_digest: Sha256Digest::parse(format!("sha256:{}", "0".repeat(64)))?,
        };
        value.binding_digest = value.calculate_binding_digest()?;
        value.id = Self::id_for(&value.binding_digest);
        value.validate_against_policy(policy)?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKLOAD_RUNTIME_EVIDENCE_BINDING_SCHEMA
            || self.id.as_uuid().is_nil()
            || self.policy_id.as_uuid().is_nil()
            || self.policy_revision_id.as_uuid().is_nil()
            || self.policy_revision_number == 0
            || self.node_attestation_binding_digest.is_some()
        {
            return Err(
                "workload Runtime evidence binding identity, schema, or WI2-C1 boundary is invalid"
                    .into(),
            );
        }
        self.candidate.validate()?;
        if Sha256Digest::parse(self.policy_digest.as_str())? != self.policy_digest
            || Sha256Digest::parse(self.binding_digest.as_str())? != self.binding_digest
            || self.candidate.identity_attachment_digest != self.policy_digest
            || self.calculate_binding_digest()? != self.binding_digest
            || Self::id_for(&self.binding_digest) != self.id
        {
            return Err(
                "workload Runtime evidence binding digest or deterministic ID drifted".into(),
            );
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::modules::identity) fn restore(
        schema: String,
        id: WorkloadRuntimeEvidenceBindingId,
        policy_id: WorkloadIdentityPolicyId,
        policy_revision_id: WorkloadIdentityPolicyRevisionId,
        policy_revision_number: u64,
        policy_digest: Sha256Digest,
        candidate: WorkloadRuntimeEvidenceCandidate,
        node_attestation_binding_digest: Option<Sha256Digest>,
        binding_digest: Sha256Digest,
    ) -> Result<Self, String> {
        let value = Self {
            schema,
            id,
            policy_id,
            policy_revision_id,
            policy_revision_number,
            policy_digest,
            candidate,
            node_attestation_binding_digest,
            binding_digest,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate_against_policy(
        &self,
        policy: &AcceptedWorkloadIdentityPolicyRevision,
    ) -> Result<(), String> {
        self.validate()?;
        policy.validate()?;
        if self.policy_id != policy.policy_id
            || self.policy_revision_id != policy.id
            || self.policy_revision_number != policy.revision_number
            || self.policy_digest != *policy.contract.digest()
        {
            return Err("workload Runtime evidence does not bind the exact policy revision".into());
        }
        validate_candidate_against_policy(policy, &self.candidate)
    }

    /// WI2-C1 is intentionally non-authorizing until Fleet supplies an exact
    /// hardware Node-attestation binding in a later versioned contract.
    pub const fn authorizes_credential_issuance(&self) -> bool {
        false
    }

    fn calculate_binding_digest(&self) -> Result<Sha256Digest, String> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct CanonicalBinding<'a> {
            schema: &'a str,
            policy_id: WorkloadIdentityPolicyId,
            policy_revision_id: WorkloadIdentityPolicyRevisionId,
            policy_revision_number: u64,
            policy_digest: &'a Sha256Digest,
            candidate: &'a WorkloadRuntimeEvidenceCandidate,
            node_attestation_binding_digest: &'a Option<Sha256Digest>,
        }
        let bytes = serde_json::to_vec(&CanonicalBinding {
            schema: &self.schema,
            policy_id: self.policy_id,
            policy_revision_id: self.policy_revision_id,
            policy_revision_number: self.policy_revision_number,
            policy_digest: &self.policy_digest,
            candidate: &self.candidate,
            node_attestation_binding_digest: &self.node_attestation_binding_digest,
        })
        .map_err(|error| format!("could not encode workload Runtime evidence: {error}"))?;
        Sha256Digest::parse(format!("sha256:{:x}", Sha256::digest(bytes)))
    }

    fn id_for(binding_digest: &Sha256Digest) -> WorkloadRuntimeEvidenceBindingId {
        WorkloadRuntimeEvidenceBindingId::from_uuid(Uuid::new_v5(
            &WORKLOAD_RUNTIME_EVIDENCE_BINDING_NAMESPACE,
            binding_digest.as_str().as_bytes(),
        ))
    }
}

/// Identity-owned immutable history record for one exact normalized Runtime
/// evidence fact. The admitted timestamp proves the original freshness check;
/// it never turns the historic record into current authorization state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkloadRuntimeEvidenceRecord {
    schema: String,
    binding: WorkloadRuntimeEvidenceBinding,
    admitted_at: DateTime<Utc>,
}

impl WorkloadRuntimeEvidenceRecord {
    pub fn admit_runtime_component(
        policy: &AcceptedWorkloadIdentityPolicyRevision,
        candidate: WorkloadRuntimeEvidenceCandidate,
        admitted_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let admitted_at = canonical_timestamp(admitted_at);
        let value = Self {
            schema: WORKLOAD_RUNTIME_EVIDENCE_RECORD_SCHEMA.into(),
            binding: WorkloadRuntimeEvidenceBinding::bind_runtime_component(
                policy,
                candidate,
                admitted_at,
            )?,
            admitted_at,
        };
        value.validate_against_policy(policy)?;
        Ok(value)
    }

    pub(in crate::modules::identity) fn restore(
        schema: String,
        binding: WorkloadRuntimeEvidenceBinding,
        admitted_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let value = Self {
            schema,
            binding,
            admitted_at,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKLOAD_RUNTIME_EVIDENCE_RECORD_SCHEMA
            || self.admitted_at != canonical_timestamp(self.admitted_at)
            || self.admitted_at.timestamp_millis() <= 0
        {
            return Err(
                "workload Runtime evidence record schema or admission time is invalid".into(),
            );
        }
        self.binding.validate()?;
        self.binding.candidate.validate_fresh_at(self.admitted_at)?;
        Ok(())
    }

    pub fn validate_against_policy(
        &self,
        policy: &AcceptedWorkloadIdentityPolicyRevision,
    ) -> Result<(), String> {
        self.validate()?;
        self.binding.validate_against_policy(policy)?;
        if self.admitted_at < policy.accepted_at {
            return Err("workload Runtime evidence predates its accepted policy revision".into());
        }
        Ok(())
    }

    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub const fn binding(&self) -> &WorkloadRuntimeEvidenceBinding {
        &self.binding
    }

    pub const fn admitted_at(&self) -> DateTime<Utc> {
        self.admitted_at
    }

    /// C3b persists history only. WI2-C4 must introduce a new decision that
    /// re-reads every owner and adds Fleet hardware evidence before issuance.
    pub const fn authorizes_credential_issuance(&self) -> bool {
        false
    }
}

fn validate_candidate_against_policy(
    policy: &AcceptedWorkloadIdentityPolicyRevision,
    candidate: &WorkloadRuntimeEvidenceCandidate,
) -> Result<(), String> {
    let spec = policy.contract.spec();
    if candidate.installation_id != policy.installation_id
        || candidate.installation_id != spec.installation_id
        || candidate.organization_id != spec.organization_id
        || candidate.project_id != spec.project_id
        || candidate.environment_id != spec.environment_id
        || candidate.workload_id != spec.workload_id
        || candidate.workload_revision_id != spec.workload_revision_id
        || candidate.node_pool_id != spec.node_pool_id
        || candidate.runtime_class != spec.runtime_class
        || candidate.isolation_level != spec.isolation_level
        || candidate.semantics_profile_digest != spec.semantics_profile_digest
        || candidate.identity_attachment_digest != *policy.contract.digest()
    {
        return Err(
            "workload Runtime evidence crossed policy lineage, placement, semantics, or attachment"
                .into(),
        );
    }
    Ok(())
}

fn validate_single_line(label: &str, value: &str, max_bytes: usize) -> Result<(), String> {
    if value.is_empty()
        || value.len() > max_bytes
        || value.contains(['\0', '\r', '\n'])
        || value.trim() != value
    {
        return Err(format!("workload Runtime {label} is invalid"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::identity::domain::value_objects::{
        PrivateServiceName, WorkloadIdentityAudience, WorkloadIdentityFormat,
        WorkloadIdentityPolicyContract, WorkloadIdentityPolicySpec, WorkloadProductRole,
    };
    use crate::modules::shared_kernel::domain::{
        PrincipalId, TrustDomainId, TrustDomainRevisionId,
    };

    fn digest(byte: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", byte.to_string().repeat(64))).expect("digest")
    }

    fn policy(now: DateTime<Utc>) -> AcceptedWorkloadIdentityPolicyRevision {
        let contract = WorkloadIdentityPolicyContract::from_spec(WorkloadIdentityPolicySpec {
            installation_id: InstallationId::new(),
            trust_domain_id: TrustDomainId::new(),
            trust_domain_revision_id: TrustDomainRevisionId::new(),
            organization_id: OrganizationId::new(),
            project_id: ProjectId::new(),
            environment_id: EnvironmentId::new(),
            policy_id: WorkloadIdentityPolicyId::new(),
            workload_id: WorkloadId::new(),
            workload_revision_id: WorkloadRevisionId::new(),
            product_role: WorkloadProductRole::AgentService,
            runtime_class: RuntimeUnitClass::Service,
            semantics_profile_digest: digest('a'),
            node_pool_id: NodePoolId::new(),
            isolation_level: RuntimeIsolationLevel::Confidential,
            attestation_profile_digest: digest('b'),
            confidential_compute: true,
            identity_formats: vec![WorkloadIdentityFormat::X509Svid],
            credential_lifetime_seconds: 300,
            rotate_before_expiry_seconds: 60,
            drain_on_rotation_failure: true,
            revoke_on_stop: true,
            audiences: vec![WorkloadIdentityAudience::parse("model.internal").expect("audience")],
            service_names: vec![PrivateServiceName::parse("agent.prod.internal").expect("service")],
            peer_policy_revision_digests: vec![],
        })
        .expect("policy contract");
        AcceptedWorkloadIdentityPolicyRevision::accept(contract, 1, PrincipalId::new(), now)
            .expect("accepted policy")
    }

    fn candidate(
        policy: &AcceptedWorkloadIdentityPolicyRevision,
        now: DateTime<Utc>,
    ) -> WorkloadRuntimeEvidenceCandidate {
        let spec = policy.contract.spec();
        WorkloadRuntimeEvidenceCandidate {
            installation_id: spec.installation_id,
            organization_id: spec.organization_id,
            project_id: spec.project_id,
            environment_id: spec.environment_id,
            workload_id: spec.workload_id,
            workload_revision_id: spec.workload_revision_id,
            resource_claim_id: ResourceClaimId::new(),
            resource_claim_generation: 3,
            resource_claim_aggregate_version: 7,
            resource_claim_digest: digest('c'),
            resource_binding_digest: digest('d'),
            node_pool_id: spec.node_pool_id,
            node_pool_aggregate_version: 5,
            node_pool_spec_digest: digest('e'),
            node_id: NodeId::new(),
            node_aggregate_version: 11,
            agent_instance_id: Uuid::now_v7(),
            node_capabilities_digest: digest('f'),
            node_last_observed_at: now,
            runtime_report_id: Uuid::now_v7(),
            runtime_unit_id: "workload:agent:replica:1".into(),
            runtime_generation: 3,
            runtime_class: spec.runtime_class,
            isolation_level: spec.isolation_level,
            semantics_profile_digest: spec.semantics_profile_digest.clone(),
            identity_attachment_digest: policy.contract.digest().clone(),
            runtime_spec_digest: digest('1'),
            runtime_attestation_binding_digest: digest('2'),
            provider_attestation_digest: digest('3'),
            provider_resource_id: "box:confidential:agent-1".into(),
            provider_build: "a3s-box/0.5.0".into(),
            runtime_state: RuntimeUnitState::Running,
            runtime_observed_at: now,
            runtime_received_at: now,
        }
    }

    #[test]
    fn exact_runtime_component_binding_is_deterministic_but_non_authorizing() {
        let now = canonical_timestamp(Utc::now());
        let policy = policy(now);
        let candidate = candidate(&policy, now);
        let record_candidate = candidate.clone();
        let first =
            WorkloadRuntimeEvidenceBinding::bind_runtime_component(&policy, candidate.clone(), now)
                .expect("first binding");
        let replay =
            WorkloadRuntimeEvidenceBinding::bind_runtime_component(&policy, candidate, now)
                .expect("replayed binding");
        assert_eq!(first, replay);
        assert!(!first.authorizes_credential_issuance());
        assert!(first.node_attestation_binding_digest.is_none());

        let record =
            WorkloadRuntimeEvidenceRecord::admit_runtime_component(&policy, record_candidate, now)
                .expect("evidence record");
        record.validate().expect("valid evidence record");
        assert!(!record.authorizes_credential_issuance());
        assert_eq!(record.binding(), &first);
    }

    #[test]
    fn policy_attachment_or_owner_drift_fails_closed() {
        let now = canonical_timestamp(Utc::now());
        let policy = policy(now);
        let mut drifted = candidate(&policy, now);
        drifted.identity_attachment_digest = digest('9');
        assert!(
            WorkloadRuntimeEvidenceBinding::bind_runtime_component(&policy, drifted, now).is_err()
        );

        let mut crossed = candidate(&policy, now);
        crossed.node_pool_id = NodePoolId::new();
        assert!(
            WorkloadRuntimeEvidenceBinding::bind_runtime_component(&policy, crossed, now).is_err()
        );
    }

    #[test]
    fn stale_node_or_runtime_observation_fails_closed() {
        let now = canonical_timestamp(Utc::now());
        let policy = policy(now);
        let stale_at = now - Duration::seconds(MAX_WORKLOAD_RUNTIME_EVIDENCE_AGE_SECONDS + 1);
        let mut stale = candidate(&policy, stale_at);
        stale.runtime_received_at = stale_at;
        assert!(
            WorkloadRuntimeEvidenceBinding::bind_runtime_component(&policy, stale, now).is_err()
        );

        let mut reordered = candidate(&policy, now - Duration::seconds(2));
        reordered.runtime_received_at = now - Duration::seconds(1);
        reordered.node_last_observed_at = now - Duration::seconds(3);
        assert!(
            WorkloadRuntimeEvidenceBinding::bind_runtime_component(&policy, reordered, now)
                .is_err()
        );

        let candidate = candidate(&policy, now);
        assert!(WorkloadRuntimeEvidenceRecord::admit_runtime_component(
            &policy,
            candidate,
            now - Duration::milliseconds(1),
        )
        .is_err());
    }

    #[test]
    fn forged_binding_digest_or_node_attestation_upgrade_is_rejected() {
        let now = canonical_timestamp(Utc::now());
        let policy = policy(now);
        let mut binding = WorkloadRuntimeEvidenceBinding::bind_runtime_component(
            &policy,
            candidate(&policy, now),
            now,
        )
        .expect("binding");
        binding.binding_digest = digest('8');
        assert!(binding.validate().is_err());

        let mut unsupported = WorkloadRuntimeEvidenceBinding::bind_runtime_component(
            &policy,
            candidate(&policy, now),
            now,
        )
        .expect("binding");
        unsupported.node_attestation_binding_digest = Some(digest('7'));
        assert!(unsupported.validate().is_err());

        let mut detached = WorkloadRuntimeEvidenceBinding::bind_runtime_component(
            &policy,
            candidate(&policy, now),
            now,
        )
        .expect("binding");
        detached.candidate.identity_attachment_digest = digest('6');
        detached.binding_digest = detached.calculate_binding_digest().expect("binding digest");
        detached.id = WorkloadRuntimeEvidenceBinding::id_for(&detached.binding_digest);
        assert!(detached.validate().is_err());
    }
}
