use crate::modules::fleet::application::{IRuntimeNodeEvidenceQueryPort, RuntimeNodeEvidenceQuery};
use crate::modules::identity::application::{
    IWorkloadRuntimeEvidenceCandidatePort, WorkloadRuntimeEvidenceRequest,
};
use crate::modules::identity::domain::entities::WorkloadRuntimeEvidenceCandidate;
use crate::modules::shared_kernel::domain::{RepositoryError, Sha256Digest};
use crate::modules::workloads::application::{BoundRuntimeClaimQuery, IBoundRuntimeClaimQueryPort};
use a3s_runtime::{RuntimeAttestationBinding, RuntimeConsumerRequirements};
use async_trait::async_trait;
use std::sync::Arc;

/// The sole Identity anti-corruption adapter for Workloads Claim, Fleet Node
/// session, and Runtime provider evidence. It creates no lifecycle, cache,
/// retry, lock, queue, event, or provider-specific parser.
pub struct OwnerWorkloadRuntimeEvidenceAdapter {
    workloads: Arc<dyn IBoundRuntimeClaimQueryPort>,
    fleet: Arc<dyn IRuntimeNodeEvidenceQueryPort>,
}

impl OwnerWorkloadRuntimeEvidenceAdapter {
    pub fn new(
        workloads: Arc<dyn IBoundRuntimeClaimQueryPort>,
        fleet: Arc<dyn IRuntimeNodeEvidenceQueryPort>,
    ) -> Self {
        Self { workloads, fleet }
    }
}

#[async_trait]
impl IWorkloadRuntimeEvidenceCandidatePort for OwnerWorkloadRuntimeEvidenceAdapter {
    async fn read_candidate(
        &self,
        request: WorkloadRuntimeEvidenceRequest,
    ) -> Result<WorkloadRuntimeEvidenceCandidate, RepositoryError> {
        request.validate().map_err(admission_error)?;
        let workload_query = BoundRuntimeClaimQuery::new(
            request.organization_id(),
            request.project_id(),
            request.environment_id(),
            request.workload_id(),
            request.workload_revision_id(),
            request.resource_claim_id(),
        )
        .map_err(admission_error)?;
        let workload = self
            .workloads
            .find_bound_runtime_claim(workload_query)
            .await?
            .ok_or(RepositoryError::NotFound)?;
        workload.validate().map_err(owner_fact_error)?;
        if workload.organization_id() != request.organization_id()
            || workload.project_id() != request.project_id()
            || workload.environment_id() != request.environment_id()
            || workload.workload_id() != request.workload_id()
            || workload.workload_revision_id() != request.workload_revision_id()
            || workload.resource_claim_id() != request.resource_claim_id()
        {
            return Err(owner_fact_error(
                "Workloads changed the requested Claim or workload lineage".into(),
            ));
        }

        let fleet_query = RuntimeNodeEvidenceQuery::new(
            request.organization_id(),
            request.node_pool_id(),
            workload.node_id(),
            workload.runtime_spec().unit_id.clone(),
            workload.runtime_spec().generation,
            request.evaluated_at(),
        )
        .map_err(admission_error)?;
        let fleet = self
            .fleet
            .find_runtime_node_evidence(fleet_query)
            .await?
            .ok_or(RepositoryError::NotFound)?;
        fleet.validate().map_err(owner_fact_error)?;
        if fleet.organization_id() != request.organization_id()
            || fleet.node_pool_id() != request.node_pool_id()
            || fleet.node_id() != workload.node_id()
        {
            return Err(owner_fact_error(
                "Fleet changed the requested organization, NodePool, or Node identity".into(),
            ));
        }

        let spec = workload.runtime_spec();
        if spec.class != request.runtime_class()
            || spec.isolation != request.isolation_level()
            || spec.semantics_profile_digest.as_deref()
                != Some(request.semantics_profile_digest().as_str())
            || spec.identity_attachment_digest.as_deref()
                != Some(request.identity_attachment_digest().as_str())
        {
            return Err(admission_error(
                "Workloads Runtime specification drifted from Identity policy".into(),
            ));
        }
        let requirements = RuntimeConsumerRequirements::new(request.runtime_class())
            .require_semantics_profile()
            .require_identity_attestation();
        requirements
            .admit_spec(spec, fleet.runtime_capabilities())
            .map_err(|error| admission_error(error.to_string()))?;
        requirements
            .accept_observation(spec, fleet.runtime_observation())
            .map_err(|error| admission_error(error.to_string()))?;
        let runtime =
            RuntimeAttestationBinding::from_observation(spec, fleet.runtime_observation())
                .map_err(admission_error)?;
        if runtime.provider_build != fleet.runtime_capabilities().provider_build
            || runtime.observed_at_ms != fleet.runtime_observation().observed_at_ms
        {
            return Err(admission_error(
                "Runtime attestation drifted from the current Fleet capability session".into(),
            ));
        }
        let identity_attachment_digest =
            Sha256Digest::parse(runtime.identity_attachment_digest.clone())
                .map_err(admission_error)?;
        let runtime_spec_digest =
            Sha256Digest::parse(runtime.spec_digest.clone()).map_err(admission_error)?;
        let runtime_attestation_binding_digest =
            Sha256Digest::parse(runtime.digest().map_err(admission_error)?)
                .map_err(admission_error)?;
        let provider_attestation_digest =
            Sha256Digest::parse(runtime.provider_attestation.digest.clone())
                .map_err(admission_error)?;

        let candidate = WorkloadRuntimeEvidenceCandidate {
            installation_id: request.installation_id(),
            organization_id: workload.organization_id(),
            project_id: workload.project_id(),
            environment_id: workload.environment_id(),
            workload_id: workload.workload_id(),
            workload_revision_id: workload.workload_revision_id(),
            resource_claim_id: workload.resource_claim_id(),
            resource_claim_generation: workload.resource_claim_generation(),
            resource_claim_aggregate_version: workload.resource_claim_aggregate_version(),
            resource_claim_digest: workload.resource_claim_digest().clone(),
            resource_binding_digest: workload.resource_binding_digest().clone(),
            node_pool_id: fleet.node_pool_id(),
            node_pool_aggregate_version: fleet.node_pool_aggregate_version(),
            node_pool_spec_digest: fleet.node_pool_spec_digest().clone(),
            node_id: fleet.node_id(),
            node_aggregate_version: fleet.node_aggregate_version(),
            agent_instance_id: fleet.agent_instance_id(),
            node_capabilities_digest: fleet.node_capabilities_digest().clone(),
            node_last_observed_at: fleet.node_last_observed_at(),
            runtime_report_id: fleet.runtime_report_id(),
            runtime_unit_id: runtime.unit_id,
            runtime_generation: runtime.generation,
            runtime_class: runtime.class,
            isolation_level: runtime.isolation,
            semantics_profile_digest: request.semantics_profile_digest().clone(),
            identity_attachment_digest,
            runtime_spec_digest,
            runtime_attestation_binding_digest,
            provider_attestation_digest,
            provider_resource_id: runtime.provider_resource_id,
            provider_build: runtime.provider_build,
            runtime_state: runtime.state,
            runtime_observed_at: fleet.runtime_observed_at(),
            runtime_received_at: fleet.runtime_received_at(),
        };
        request
            .validate_candidate(&candidate)
            .map_err(admission_error)?;
        Ok(candidate)
    }
}

fn admission_error(error: String) -> RepositoryError {
    RepositoryError::Conflict(format!(
        "workload Runtime evidence admission rejected: {error}"
    ))
}

fn owner_fact_error(error: String) -> RepositoryError {
    RepositoryError::Storage(format!("invalid workload Runtime owner fact: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::fleet::published::{RuntimeNodeEvidence, RUNTIME_NODE_EVIDENCE_SCHEMA};
    use crate::modules::identity::domain::entities::{
        AcceptedWorkloadIdentityPolicyRevision, WorkloadRuntimeEvidenceBinding,
    };
    use crate::modules::identity::domain::value_objects::{
        PrivateServiceName, WorkloadIdentityAudience, WorkloadIdentityFormat,
        WorkloadIdentityPolicyContract, WorkloadIdentityPolicySpec, WorkloadProductRole,
    };
    use crate::modules::shared_kernel::domain::{
        canonical_timestamp, EnvironmentId, InstallationId, NodeId, NodePoolId, OrganizationId,
        PrincipalId, ProjectId, ResourceClaimId, TrustDomainId, TrustDomainRevisionId, WorkloadId,
        WorkloadIdentityPolicyId, WorkloadRevisionId,
    };
    use crate::modules::workloads::published::{BoundRuntimeClaim, BOUND_RUNTIME_CLAIM_SCHEMA};
    use a3s_runtime::contract::{
        ArtifactRef, IsolationLevel, NetworkMode, ResourceControl, ResourceLimits, RestartPolicy,
        RuntimeCapabilities, RuntimeEvidence, RuntimeFeature, RuntimeNetworkSpec,
        RuntimeObservation, RuntimeProcessSpec, RuntimeUnitClass, RuntimeUnitSpec,
        RuntimeUnitState,
    };
    use a3s_runtime::ProviderId;
    use chrono::Utc;
    use std::collections::BTreeMap;
    use uuid::Uuid;

    struct FixedWorkloadPort {
        value: BoundRuntimeClaim,
    }

    #[async_trait]
    impl IBoundRuntimeClaimQueryPort for FixedWorkloadPort {
        async fn find_bound_runtime_claim(
            &self,
            _query: BoundRuntimeClaimQuery,
        ) -> Result<Option<BoundRuntimeClaim>, RepositoryError> {
            Ok(Some(self.value.clone()))
        }
    }

    struct FixedFleetPort {
        value: RuntimeNodeEvidence,
    }

    #[async_trait]
    impl IRuntimeNodeEvidenceQueryPort for FixedFleetPort {
        async fn find_runtime_node_evidence(
            &self,
            _query: RuntimeNodeEvidenceQuery,
        ) -> Result<Option<RuntimeNodeEvidence>, RepositoryError> {
            Ok(Some(self.value.clone()))
        }
    }

    #[tokio::test]
    async fn adapter_composes_exact_owner_facts_through_runtime_contract() {
        let now = canonical_timestamp(Utc::now());
        let policy = policy(now);
        let claim_id = ResourceClaimId::new();
        let (workload, fleet) = owner_facts(&policy, claim_id, now);
        let adapter = OwnerWorkloadRuntimeEvidenceAdapter::new(
            Arc::new(FixedWorkloadPort { value: workload }),
            Arc::new(FixedFleetPort { value: fleet }),
        );

        let candidate = adapter
            .read_candidate(
                WorkloadRuntimeEvidenceRequest::for_policy(&policy, claim_id, now)
                    .expect("evidence request"),
            )
            .await
            .expect("normalized candidate");
        candidate.validate().expect("valid normalized candidate");
        assert_eq!(candidate.resource_claim_id, claim_id);
        assert_eq!(
            candidate.identity_attachment_digest,
            *policy.contract.digest()
        );
        let binding =
            WorkloadRuntimeEvidenceBinding::bind_runtime_component(&policy, candidate, now)
                .expect("Identity evidence binding");
        assert!(!binding.authorizes_credential_issuance());
    }

    #[tokio::test]
    async fn adapter_rejects_owner_identity_substitution() {
        let now = canonical_timestamp(Utc::now());
        let policy = policy(now);
        let claim_id = ResourceClaimId::new();
        let (workload, fleet) = owner_facts(&policy, claim_id, now);
        let mut forged = serde_json::to_value(workload).expect("workload owner fact");
        forged["resourceClaimId"] = serde_json::to_value(ResourceClaimId::new()).expect("Claim ID");
        let forged: BoundRuntimeClaim =
            serde_json::from_value(forged).expect("syntactically valid owner fact");
        let adapter = OwnerWorkloadRuntimeEvidenceAdapter::new(
            Arc::new(FixedWorkloadPort { value: forged }),
            Arc::new(FixedFleetPort { value: fleet }),
        );

        assert!(matches!(
            adapter
                .read_candidate(
                    WorkloadRuntimeEvidenceRequest::for_policy(&policy, claim_id, now)
                        .expect("evidence request")
                )
                .await,
            Err(RepositoryError::Storage(message))
                if message.contains("changed the requested Claim")
        ));
    }

    #[tokio::test]
    async fn adapter_rejects_fleet_tenant_substitution() {
        let now = canonical_timestamp(Utc::now());
        let policy = policy(now);
        let claim_id = ResourceClaimId::new();
        let (workload, fleet) = owner_facts(&policy, claim_id, now);
        let mut forged = serde_json::to_value(fleet).expect("Fleet owner fact");
        forged["organizationId"] =
            serde_json::to_value(OrganizationId::new()).expect("organization ID");
        let forged: RuntimeNodeEvidence =
            serde_json::from_value(forged).expect("syntactically valid Fleet owner fact");
        let adapter = OwnerWorkloadRuntimeEvidenceAdapter::new(
            Arc::new(FixedWorkloadPort { value: workload }),
            Arc::new(FixedFleetPort { value: forged }),
        );

        assert!(matches!(
            adapter
                .read_candidate(
                    WorkloadRuntimeEvidenceRequest::for_policy(&policy, claim_id, now)
                        .expect("evidence request")
                )
                .await,
            Err(RepositoryError::Storage(message))
                if message.contains("changed the requested organization")
        ));
    }

    #[tokio::test]
    async fn adapter_rejects_every_policy_controlled_runtime_spec_drift() {
        let now = canonical_timestamp(Utc::now());
        let policy = policy(now);
        let claim_id = ResourceClaimId::new();
        let (workload, fleet) = owner_facts(&policy, claim_id, now);
        let mutations = [
            ("class", serde_json::json!("task")),
            ("isolation", serde_json::json!("sandbox")),
            (
                "semantics_profile_digest",
                serde_json::to_value(digest('8')).expect("semantics digest"),
            ),
            (
                "identity_attachment_digest",
                serde_json::to_value(digest('9')).expect("attachment digest"),
            ),
        ];

        for (field, value) in mutations {
            let mut forged = serde_json::to_value(&workload).expect("Workloads owner fact");
            forged["runtimeSpec"][field] = value;
            let forged: BoundRuntimeClaim =
                serde_json::from_value(forged).expect("syntactically valid Workloads owner fact");
            let adapter = OwnerWorkloadRuntimeEvidenceAdapter::new(
                Arc::new(FixedWorkloadPort { value: forged }),
                Arc::new(FixedFleetPort {
                    value: fleet.clone(),
                }),
            );

            assert!(
                adapter
                    .read_candidate(
                        WorkloadRuntimeEvidenceRequest::for_policy(&policy, claim_id, now)
                            .expect("evidence request")
                    )
                    .await
                    .is_err(),
                "Runtime specification drift in {field} must fail closed"
            );
        }
    }

    fn policy(now: chrono::DateTime<Utc>) -> AcceptedWorkloadIdentityPolicyRevision {
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
            isolation_level: IsolationLevel::Confidential,
            attestation_profile_digest: digest('b'),
            confidential_compute: true,
            identity_formats: vec![WorkloadIdentityFormat::X509Svid],
            credential_lifetime_seconds: 300,
            rotate_before_expiry_seconds: 60,
            drain_on_rotation_failure: true,
            revoke_on_stop: true,
            audiences: vec![WorkloadIdentityAudience::parse("model.internal").expect("audience")],
            service_names: vec![
                PrivateServiceName::parse("agent.prod.internal").expect("service name")
            ],
            peer_policy_revision_digests: vec![],
        })
        .expect("policy contract");
        AcceptedWorkloadIdentityPolicyRevision::accept(contract, 1, PrincipalId::new(), now)
            .expect("accepted policy")
    }

    fn owner_facts(
        policy: &AcceptedWorkloadIdentityPolicyRevision,
        claim_id: ResourceClaimId,
        now: chrono::DateTime<Utc>,
    ) -> (BoundRuntimeClaim, RuntimeNodeEvidence) {
        let policy_spec = policy.contract.spec();
        let node_id = NodeId::new();
        let unit_id = "workload:agent:replica:1";
        let runtime_spec = RuntimeUnitSpec {
            schema: RuntimeUnitSpec::SCHEMA.into(),
            unit_id: unit_id.into(),
            generation: 3,
            class: RuntimeUnitClass::Service,
            artifact: artifact('c'),
            process: RuntimeProcessSpec {
                command: vec!["/app/agent".into()],
                args: vec![],
                working_directory: Some("/app".into()),
                environment: BTreeMap::new(),
            },
            mounts: vec![],
            secrets: vec![],
            network: RuntimeNetworkSpec {
                mode: NetworkMode::None,
                ports: vec![],
            },
            resources: ResourceLimits {
                cpu_millis: 500,
                memory_bytes: 256 * 1024 * 1024,
                pids: 128,
                ephemeral_storage_bytes: None,
                execution_timeout_ms: None,
            },
            isolation: IsolationLevel::Confidential,
            health: None,
            service_lifecycle: None,
            restart: RestartPolicy::Always,
            outputs: vec![],
            semantics_profile_digest: Some(policy_spec.semantics_profile_digest.to_string()),
            identity_attachment_digest: Some(policy.contract.digest().to_string()),
        };
        runtime_spec.validate().expect("Runtime specification");
        let provider_build = "a3s-box/0.5.0";
        let capabilities = RuntimeCapabilities {
            schema: RuntimeCapabilities::SCHEMA.into(),
            provider_id: ProviderId::parse("a3s-box").expect("provider ID"),
            provider_build: provider_build.into(),
            unit_classes: vec![RuntimeUnitClass::Service],
            artifact_media_types: vec![runtime_spec.artifact.media_type.clone()],
            isolation_levels: vec![IsolationLevel::Confidential],
            network_modes: vec![NetworkMode::None],
            mount_kinds: vec![],
            health_check_kinds: vec![],
            resource_controls: vec![
                ResourceControl::Cpu,
                ResourceControl::Memory,
                ResourceControl::Pids,
            ],
            features: vec![
                RuntimeFeature::DurableIdentity,
                RuntimeFeature::Attestation,
                RuntimeFeature::IdentityAttachment,
            ],
        };
        capabilities.validate().expect("Runtime capabilities");
        let observed_at_ms = u64::try_from(now.timestamp_millis()).expect("positive time");
        let runtime_observed_at = chrono::DateTime::<Utc>::from_timestamp_millis(
            i64::try_from(observed_at_ms).expect("supported time"),
        )
        .expect("Runtime protocol time");
        let observation = RuntimeObservation {
            schema: RuntimeObservation::SCHEMA.into(),
            unit_id: unit_id.into(),
            generation: runtime_spec.generation,
            spec_digest: runtime_spec.digest().expect("spec digest"),
            class: RuntimeUnitClass::Service,
            state: RuntimeUnitState::Running,
            provider_resource_id: Some("box:confidential:agent-1".into()),
            provider_build: Some(provider_build.into()),
            observed_at_ms,
            started_at_ms: Some(observed_at_ms),
            finished_at_ms: None,
            health: None,
            liveness: None,
            outputs: vec![],
            usage: None,
            evidence: Some(RuntimeEvidence {
                provider_build: provider_build.into(),
                spec_digest: runtime_spec.digest().expect("spec digest"),
                semantics_profile_digest: runtime_spec.semantics_profile_digest.clone(),
                identity_attachment_digest: runtime_spec.identity_attachment_digest.clone(),
                claims: BTreeMap::new(),
            }),
            provider_attestation: Some(artifact('d')),
            failure: None,
        };
        observation
            .validate_against(&runtime_spec)
            .expect("Runtime observation");

        let workload: BoundRuntimeClaim = serde_json::from_value(serde_json::json!({
            "schema": BOUND_RUNTIME_CLAIM_SCHEMA,
            "organizationId": policy_spec.organization_id,
            "projectId": policy_spec.project_id,
            "environmentId": policy_spec.environment_id,
            "workloadId": policy_spec.workload_id,
            "workloadRevisionId": policy_spec.workload_revision_id,
            "resourceClaimId": claim_id,
            "resourceClaimGeneration": 3,
            "resourceClaimAggregateVersion": 7,
            "resourceClaimDigest": digest('e'),
            "resourceBindingDigest": digest('f'),
            "nodeId": node_id,
            "runtimeSpec": runtime_spec,
        }))
        .expect("Workloads owner fact");
        workload.validate().expect("valid Workloads owner fact");

        let capabilities_document = serde_json::to_value(&capabilities).expect("capabilities");
        let capabilities_digest = Sha256Digest::from_bytes(
            &serde_json::to_vec(&capabilities_document).expect("capability bytes"),
        );
        let fleet: RuntimeNodeEvidence = serde_json::from_value(serde_json::json!({
            "schema": RUNTIME_NODE_EVIDENCE_SCHEMA,
            "organizationId": policy_spec.organization_id,
            "nodePoolId": policy_spec.node_pool_id,
            "nodePoolAggregateVersion": 5,
            "nodePoolSpecDigest": digest('1'),
            "nodeId": node_id,
            "nodeAggregateVersion": 11,
            "agentInstanceId": Uuid::now_v7(),
            "nodeCapabilitiesDigest": capabilities_digest,
            "nodeLastObservedAt": now,
            "runtimeCapabilities": capabilities,
            "runtimeReportId": Uuid::now_v7(),
            "runtimeObservedAt": runtime_observed_at,
            "runtimeReceivedAt": now,
            "runtimeObservation": observation,
        }))
        .expect("Fleet owner fact");
        fleet.validate().expect("valid Fleet owner fact");
        (workload, fleet)
    }

    fn artifact(byte: char) -> ArtifactRef {
        ArtifactRef {
            uri: format!(
                "oci://registry.example/a3s/agent@sha256:{}",
                byte.to_string().repeat(64)
            ),
            digest: format!("sha256:{}", byte.to_string().repeat(64)),
            media_type: "application/vnd.oci.image.manifest.v1+json".into(),
        }
    }

    fn digest(byte: char) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", byte.to_string().repeat(64))).expect("digest")
    }
}
