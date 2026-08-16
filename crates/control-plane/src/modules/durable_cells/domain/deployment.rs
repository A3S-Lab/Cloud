use super::{DurableCellProjectionIdentity, DurableCellProviderBinding, DurableCellStorageBinding};
use crate::modules::shared_kernel::domain::{canonical_timestamp, PrincipalId, Sha256Digest};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Immutable correlation intent for projecting one Durable Cell application
/// revision through the existing S0, Workloads, Operations, and Fleet owners.
///
/// This record deliberately has no lifecycle/status field. The referenced
/// Workload Deployment and Operation remain the only execution authorities;
/// persisting this intent before invoking them makes process-death recovery an
/// idempotent replay instead of another rollout controller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DurableCellDeployment {
    pub projection: DurableCellProjectionIdentity,
    pub storage: DurableCellStorageBinding,
    pub provider: DurableCellProviderBinding,
    pub placement_policy_digest: Sha256Digest,
    pub requested_by: PrincipalId,
    pub request_id: Uuid,
    pub requested_at: DateTime<Utc>,
}

impl DurableCellDeployment {
    pub fn bind(
        projection: DurableCellProjectionIdentity,
        storage: DurableCellStorageBinding,
        provider: DurableCellProviderBinding,
        placement_policy_digest: Sha256Digest,
        requested_by: PrincipalId,
        request_id: Uuid,
        requested_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let deployment = Self {
            projection,
            storage,
            provider,
            placement_policy_digest,
            requested_by,
            request_id,
            requested_at: canonical_timestamp(requested_at),
        };
        deployment.validate()?;
        Ok(deployment)
    }

    pub fn restore(mut self) -> Result<Self, String> {
        self.requested_at = canonical_timestamp(self.requested_at);
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.projection.validate()?;
        self.storage.validate()?;
        self.provider.validate()?;
        if self.storage.organization_id != self.projection.organization_id
            || self.storage.project_id != self.projection.project_id
            || self.storage.environment_id != self.projection.environment_id
            || self.storage.application_id != self.projection.application_id
            || self.storage.application_revision_id != self.projection.application_revision_id
            || self.storage.application_revision_number
                != self.projection.application_revision_number
            || self.storage.application_definition_digest
                != self.projection.application_definition_digest
            || self.storage.storage_namespace_id != self.projection.storage_namespace_id
            || self.provider.application_id != self.projection.application_id
            || self.provider.application_revision_id != self.projection.application_revision_id
            || self.provider.application_revision_number
                != self.projection.application_revision_number
            || self.provider.application_definition_digest
                != self.projection.application_definition_digest
            || self.provider.workload_id != self.projection.workload_id
            || self.provider.workload_revision_id != self.projection.workload_revision_id
            || Sha256Digest::parse(self.placement_policy_digest.as_str())?
                != self.placement_policy_digest
            || self.requested_by.as_uuid().is_nil()
            || self.request_id.is_nil()
            || self.requested_at != canonical_timestamp(self.requested_at)
        {
            return Err("Durable Cell deployment correlation is invalid".into());
        }
        Ok(())
    }
}
