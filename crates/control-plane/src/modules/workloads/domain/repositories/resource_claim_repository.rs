use crate::modules::shared_kernel::domain::{
    IdempotentWrite, NodeCommandId, NodeId, OrganizationId, RepositoryError, ResourceClaimId,
};
use crate::modules::workloads::domain::entities::{
    ResourceClaim, ResourceClaimBindingEvidence, ResourceClaimReleaseEvidence,
    ResourceClaimReservation,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

const CAPACITY_UNAVAILABLE_PREFIX: &str = "resource capacity unavailable: ";
const PLACEMENT_UNAVAILABLE_PREFIX: &str = "replica placement unavailable: ";

pub(crate) fn capacity_unavailable(message: impl Into<String>) -> RepositoryError {
    RepositoryError::Conflict(format!("{CAPACITY_UNAVAILABLE_PREFIX}{}", message.into()))
}

pub(crate) fn is_capacity_unavailable(error: &RepositoryError) -> bool {
    matches!(
        error,
        RepositoryError::Conflict(message) if message.starts_with(CAPACITY_UNAVAILABLE_PREFIX)
    )
}

pub(crate) fn placement_unavailable(message: impl Into<String>) -> RepositoryError {
    RepositoryError::Conflict(format!("{PLACEMENT_UNAVAILABLE_PREFIX}{}", message.into()))
}

pub(crate) fn is_placement_unavailable(error: &RepositoryError) -> bool {
    matches!(
        error,
        RepositoryError::Conflict(message) if message.starts_with(PLACEMENT_UNAVAILABLE_PREFIX)
    )
}

#[async_trait]
pub trait IResourceClaimRepository: Send + Sync {
    async fn has_active_claims(
        &self,
        organization_id: OrganizationId,
        node_id: NodeId,
    ) -> Result<bool, RepositoryError>;

    async fn reserve(
        &self,
        reservation: ResourceClaimReservation,
    ) -> Result<IdempotentWrite<ResourceClaim>, RepositoryError>;

    async fn find(
        &self,
        organization_id: OrganizationId,
        claim_id: ResourceClaimId,
    ) -> Result<ResourceClaim, RepositoryError>;

    async fn begin_preparation(
        &self,
        organization_id: OrganizationId,
        claim_id: ResourceClaimId,
        expected_version: u64,
        command_id: NodeCommandId,
        at: DateTime<Utc>,
    ) -> Result<ResourceClaim, RepositoryError>;

    async fn record_prepared(
        &self,
        organization_id: OrganizationId,
        claim_id: ResourceClaimId,
        expected_version: u64,
        command_id: NodeCommandId,
        binding_digest: String,
        at: DateTime<Utc>,
    ) -> Result<ResourceClaim, RepositoryError>;

    async fn bind(
        &self,
        organization_id: OrganizationId,
        claim_id: ResourceClaimId,
        expected_version: u64,
        evidence: ResourceClaimBindingEvidence,
        at: DateTime<Utc>,
    ) -> Result<ResourceClaim, RepositoryError>;

    async fn begin_release(
        &self,
        organization_id: OrganizationId,
        claim_id: ResourceClaimId,
        expected_version: u64,
        command_id: NodeCommandId,
        at: DateTime<Utc>,
    ) -> Result<ResourceClaim, RepositoryError>;

    async fn record_released(
        &self,
        organization_id: OrganizationId,
        claim_id: ResourceClaimId,
        expected_version: u64,
        evidence: ResourceClaimReleaseEvidence,
        at: DateTime<Utc>,
    ) -> Result<ResourceClaim, RepositoryError>;

    async fn cancel_database_reservation(
        &self,
        organization_id: OrganizationId,
        claim_id: ResourceClaimId,
        expected_version: u64,
        at: DateTime<Utc>,
    ) -> Result<ResourceClaim, RepositoryError>;

    async fn orphan(
        &self,
        organization_id: OrganizationId,
        claim_id: ResourceClaimId,
        expected_version: u64,
        failure: String,
        at: DateTime<Utc>,
    ) -> Result<ResourceClaim, RepositoryError>;
}
