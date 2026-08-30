use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, NodePoolId, OrganizationId, ProjectId, Sha256Digest,
    WorkloadId, WorkloadRevisionId,
};
use a3s_cloud_contracts::{RuntimeIsolationLevel, RuntimeUnitClass};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const WORKLOAD_RUNTIME_EXECUTION_AUTHORIZATION_SCHEMA: &str =
    "a3s.cloud.workload-runtime-execution-authorization.v1";

/// Identity-owned immutable projection of the current accepted policy for one
/// logical Workload. The policy lifecycle and credential rules do not cross
/// the bounded-context boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkloadRuntimeExecutionAuthorization {
    schema: String,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    workload_id: WorkloadId,
    workload_revision_id: WorkloadRevisionId,
    node_pool_id: NodePoolId,
    runtime_class: RuntimeUnitClass,
    isolation_level: RuntimeIsolationLevel,
    semantics_profile_digest: Sha256Digest,
    identity_attachment_digest: Sha256Digest,
    authorized_at: DateTime<Utc>,
}

pub(in crate::modules::identity) struct ValidatedWorkloadRuntimeExecutionAuthorizationProjection {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub workload_id: WorkloadId,
    pub workload_revision_id: WorkloadRevisionId,
    pub node_pool_id: NodePoolId,
    pub runtime_class: RuntimeUnitClass,
    pub isolation_level: RuntimeIsolationLevel,
    pub semantics_profile_digest: Sha256Digest,
    pub identity_attachment_digest: Sha256Digest,
    pub authorized_at: DateTime<Utc>,
}

impl WorkloadRuntimeExecutionAuthorization {
    pub(in crate::modules::identity) fn from_validated_policy(
        projection: ValidatedWorkloadRuntimeExecutionAuthorizationProjection,
    ) -> Result<Self, String> {
        let value = Self {
            schema: WORKLOAD_RUNTIME_EXECUTION_AUTHORIZATION_SCHEMA.into(),
            organization_id: projection.organization_id,
            project_id: projection.project_id,
            environment_id: projection.environment_id,
            workload_id: projection.workload_id,
            workload_revision_id: projection.workload_revision_id,
            node_pool_id: projection.node_pool_id,
            runtime_class: projection.runtime_class,
            isolation_level: projection.isolation_level,
            semantics_profile_digest: projection.semantics_profile_digest,
            identity_attachment_digest: projection.identity_attachment_digest,
            authorized_at: canonical_timestamp(projection.authorized_at),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKLOAD_RUNTIME_EXECUTION_AUTHORIZATION_SCHEMA
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.workload_id.as_uuid().is_nil()
            || self.workload_revision_id.as_uuid().is_nil()
            || self.node_pool_id.as_uuid().is_nil()
            || self.authorized_at != canonical_timestamp(self.authorized_at)
            || Sha256Digest::parse(self.semantics_profile_digest.as_str())?
                != self.semantics_profile_digest
            || Sha256Digest::parse(self.identity_attachment_digest.as_str())?
                != self.identity_attachment_digest
        {
            return Err("workload Runtime execution authorization is invalid".into());
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

    pub const fn node_pool_id(&self) -> NodePoolId {
        self.node_pool_id
    }

    pub const fn runtime_class(&self) -> RuntimeUnitClass {
        self.runtime_class
    }

    pub const fn isolation_level(&self) -> RuntimeIsolationLevel {
        self.isolation_level
    }

    pub const fn semantics_profile_digest(&self) -> &Sha256Digest {
        &self.semantics_profile_digest
    }

    pub const fn identity_attachment_digest(&self) -> &Sha256Digest {
        &self.identity_attachment_digest
    }

    pub const fn authorized_at(&self) -> DateTime<Utc> {
        self.authorized_at
    }
}
