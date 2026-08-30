use crate::modules::shared_kernel::domain::{
    EnvironmentId, NodeId, OrganizationId, ProjectId, ResourceClaimId, Sha256Digest, WorkloadId,
    WorkloadRevisionId,
};
use a3s_runtime::contract::RuntimeUnitSpec;
use serde::{Deserialize, Serialize};

pub const BOUND_RUNTIME_CLAIM_SCHEMA: &str = "a3s.cloud.bound-runtime-claim.v1";

/// Workloads-owned immutable projection of one ResourceClaim that is still
/// bound to an exact Runtime Unit generation. Placement attempts, commands,
/// inventory, capacity slots, release state, and owner aggregates are absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BoundRuntimeClaim {
    schema: String,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    workload_id: WorkloadId,
    workload_revision_id: WorkloadRevisionId,
    resource_claim_id: ResourceClaimId,
    resource_claim_generation: u64,
    resource_claim_aggregate_version: u64,
    resource_claim_digest: Sha256Digest,
    resource_binding_digest: Sha256Digest,
    node_id: NodeId,
    runtime_spec: RuntimeUnitSpec,
}

pub(in crate::modules::workloads) struct ValidatedBoundRuntimeClaimProjection {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub workload_id: WorkloadId,
    pub workload_revision_id: WorkloadRevisionId,
    pub resource_claim_id: ResourceClaimId,
    pub resource_claim_generation: u64,
    pub resource_claim_aggregate_version: u64,
    pub resource_claim_digest: String,
    pub resource_binding_digest: String,
    pub node_id: NodeId,
    pub runtime_spec: RuntimeUnitSpec,
}

impl BoundRuntimeClaim {
    pub(in crate::modules::workloads) fn from_validated_claim(
        projection: ValidatedBoundRuntimeClaimProjection,
    ) -> Result<Self, String> {
        let value = Self {
            schema: BOUND_RUNTIME_CLAIM_SCHEMA.into(),
            organization_id: projection.organization_id,
            project_id: projection.project_id,
            environment_id: projection.environment_id,
            workload_id: projection.workload_id,
            workload_revision_id: projection.workload_revision_id,
            resource_claim_id: projection.resource_claim_id,
            resource_claim_generation: projection.resource_claim_generation,
            resource_claim_aggregate_version: projection.resource_claim_aggregate_version,
            resource_claim_digest: Sha256Digest::parse(projection.resource_claim_digest)?,
            resource_binding_digest: Sha256Digest::parse(projection.resource_binding_digest)?,
            node_id: projection.node_id,
            runtime_spec: projection.runtime_spec,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.runtime_spec.validate()?;
        if self.schema != BOUND_RUNTIME_CLAIM_SCHEMA
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.workload_id.as_uuid().is_nil()
            || self.workload_revision_id.as_uuid().is_nil()
            || self.resource_claim_id.as_uuid().is_nil()
            || self.resource_claim_generation == 0
            || self.resource_claim_aggregate_version == 0
            || self.node_id.as_uuid().is_nil()
            || self.runtime_spec.generation == 0
            || Sha256Digest::parse(self.resource_claim_digest.as_str())?
                != self.resource_claim_digest
            || Sha256Digest::parse(self.resource_binding_digest.as_str())?
                != self.resource_binding_digest
        {
            return Err("bound Runtime Claim identity, generation, or digest is invalid".into());
        }
        Ok(())
    }

    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }

    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }

    pub const fn environment_id(&self) -> EnvironmentId {
        self.environment_id
    }

    pub const fn workload_id(&self) -> WorkloadId {
        self.workload_id
    }

    pub const fn workload_revision_id(&self) -> WorkloadRevisionId {
        self.workload_revision_id
    }

    pub const fn resource_claim_id(&self) -> ResourceClaimId {
        self.resource_claim_id
    }

    pub const fn resource_claim_generation(&self) -> u64 {
        self.resource_claim_generation
    }

    pub const fn resource_claim_aggregate_version(&self) -> u64 {
        self.resource_claim_aggregate_version
    }

    pub const fn resource_claim_digest(&self) -> &Sha256Digest {
        &self.resource_claim_digest
    }

    pub const fn resource_binding_digest(&self) -> &Sha256Digest {
        &self.resource_binding_digest
    }

    pub const fn node_id(&self) -> NodeId {
        self.node_id
    }

    pub const fn runtime_spec(&self) -> &RuntimeUnitSpec {
        &self.runtime_spec
    }
}
