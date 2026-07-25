use super::replicas;
use super::resource_claim_rows::{restore_claim, ClaimWithSlotRow, ClaimWithSlotSelection};
use super::resource_claim_writes;
use super::schema::{ResourceClaimSlots, ResourceClaims};
use crate::infrastructure::{fetch_all, transaction_error, PostgresPersistenceError};
use crate::modules::shared_kernel::domain::{
    IdempotentWrite, NodeCommandId, OrganizationId, RepositoryError, ResourceClaimId,
};
use crate::modules::workloads::domain::entities::{
    ResourceClaim, ResourceClaimBindingEvidence, ResourceClaimReleaseEvidence,
    ResourceClaimReservation,
};
use crate::modules::workloads::domain::repositories::IResourceClaimRepository;
use a3s_orm::{
    select_from, Database, OrderDirection, PostgresDialect, PostgresExecutor, PostgresTransaction,
    Query,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Clone)]
pub struct PostgresResourceClaimRepository {
    executor: PostgresExecutor,
}

impl PostgresResourceClaimRepository {
    pub const fn new(executor: PostgresExecutor) -> Self {
        Self { executor }
    }

    async fn mutate(
        &self,
        organization_id: OrganizationId,
        claim_id: ResourceClaimId,
        expected_version: u64,
        mutation: ClaimMutation,
    ) -> Result<ResourceClaim, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(mutate_in_transaction(
                    transaction,
                    organization_id,
                    claim_id,
                    expected_version,
                    mutation,
                ))
            })
            .await
            .map_err(transaction_error)
    }
}

#[async_trait]
impl IResourceClaimRepository for PostgresResourceClaimRepository {
    async fn reserve(
        &self,
        reservation: ResourceClaimReservation,
    ) -> Result<IdempotentWrite<ResourceClaim>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(reserve_in_transaction(transaction, reservation))
            })
            .await
            .map_err(transaction_error)
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        claim_id: ResourceClaimId,
    ) -> Result<ResourceClaim, RepositoryError> {
        let rows = Database::new(PostgresDialect, self.executor.clone())
            .fetch_all_as(claim_query(organization_id, claim_id))
            .await
            .map_err(|error| RepositoryError::Storage(error.to_string()))?
            .rows;
        restore_claim(rows)?.ok_or(RepositoryError::NotFound)
    }

    async fn begin_preparation(
        &self,
        organization_id: OrganizationId,
        claim_id: ResourceClaimId,
        expected_version: u64,
        command_id: NodeCommandId,
        at: DateTime<Utc>,
    ) -> Result<ResourceClaim, RepositoryError> {
        self.mutate(
            organization_id,
            claim_id,
            expected_version,
            ClaimMutation::BeginPreparation { command_id, at },
        )
        .await
    }

    async fn record_prepared(
        &self,
        organization_id: OrganizationId,
        claim_id: ResourceClaimId,
        expected_version: u64,
        command_id: NodeCommandId,
        binding_digest: String,
        at: DateTime<Utc>,
    ) -> Result<ResourceClaim, RepositoryError> {
        self.mutate(
            organization_id,
            claim_id,
            expected_version,
            ClaimMutation::RecordPrepared {
                command_id,
                binding_digest,
                at,
            },
        )
        .await
    }

    async fn bind(
        &self,
        organization_id: OrganizationId,
        claim_id: ResourceClaimId,
        expected_version: u64,
        evidence: ResourceClaimBindingEvidence,
        at: DateTime<Utc>,
    ) -> Result<ResourceClaim, RepositoryError> {
        self.mutate(
            organization_id,
            claim_id,
            expected_version,
            ClaimMutation::Bind { evidence, at },
        )
        .await
    }

    async fn begin_release(
        &self,
        organization_id: OrganizationId,
        claim_id: ResourceClaimId,
        expected_version: u64,
        command_id: NodeCommandId,
        at: DateTime<Utc>,
    ) -> Result<ResourceClaim, RepositoryError> {
        self.mutate(
            organization_id,
            claim_id,
            expected_version,
            ClaimMutation::BeginRelease { command_id, at },
        )
        .await
    }

    async fn record_released(
        &self,
        organization_id: OrganizationId,
        claim_id: ResourceClaimId,
        expected_version: u64,
        evidence: ResourceClaimReleaseEvidence,
        at: DateTime<Utc>,
    ) -> Result<ResourceClaim, RepositoryError> {
        self.mutate(
            organization_id,
            claim_id,
            expected_version,
            ClaimMutation::RecordReleased { evidence, at },
        )
        .await
    }

    async fn orphan(
        &self,
        organization_id: OrganizationId,
        claim_id: ResourceClaimId,
        expected_version: u64,
        failure: String,
        at: DateTime<Utc>,
    ) -> Result<ResourceClaim, RepositoryError> {
        self.mutate(
            organization_id,
            claim_id,
            expected_version,
            ClaimMutation::Orphan { failure, at },
        )
        .await
    }
}

async fn reserve_in_transaction(
    transaction: &PostgresTransaction,
    reservation: ResourceClaimReservation,
) -> Result<IdempotentWrite<ResourceClaim>, PostgresPersistenceError> {
    reservation.validate().map_err(RepositoryError::Conflict)?;
    lock_claim_id(transaction, reservation.id).await?;
    if let Some(existing) = find_claim_in_transaction(
        transaction,
        reservation.binding.organization_id,
        reservation.id,
    )
    .await?
    {
        if !reservation.matches(&existing) {
            return Err(RepositoryError::IdempotencyConflict.into());
        }
        return Ok(IdempotentWrite {
            value: existing,
            replayed: true,
        });
    }
    let persisted_binding = replicas::binding_in_transaction(
        transaction,
        reservation.binding.organization_id,
        reservation.binding.deployment_id,
    )
    .await?
    .ok_or(RepositoryError::NotFound)?;
    if persisted_binding != reservation.binding {
        return Err(RepositoryError::Conflict(
            "resource claim reservation does not match the durable replica binding".into(),
        )
        .into());
    }
    let slots = resource_claim_writes::reserve_slots(transaction, &reservation).await?;
    let claim = ResourceClaim::reserve(&reservation, slots).map_err(RepositoryError::Conflict)?;
    resource_claim_writes::insert_claim(transaction, &claim).await?;
    Ok(IdempotentWrite {
        value: claim,
        replayed: false,
    })
}

async fn mutate_in_transaction(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    claim_id: ResourceClaimId,
    expected_version: u64,
    mutation: ClaimMutation,
) -> Result<ResourceClaim, PostgresPersistenceError> {
    lock_claim_id(transaction, claim_id).await?;
    let current = find_claim_in_transaction(transaction, organization_id, claim_id)
        .await?
        .ok_or(RepositoryError::NotFound)?;
    let mut next = current.clone();
    mutation
        .apply(&mut next)
        .map_err(RepositoryError::Conflict)?;
    if next == current {
        return Ok(current);
    }
    if current.aggregate_version != expected_version {
        return Err(RepositoryError::Conflict(format!(
            "resource claim changed from expected version {expected_version} to {}",
            current.aggregate_version
        ))
        .into());
    }
    resource_claim_writes::persist_claim(transaction, &next, expected_version).await?;
    if next.state == crate::modules::workloads::domain::entities::ResourceClaimState::Released {
        resource_claim_writes::release_slots(transaction, &next).await?;
    }
    Ok(next)
}

async fn find_claim_in_transaction(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    claim_id: ResourceClaimId,
) -> Result<Option<ResourceClaim>, PostgresPersistenceError> {
    let rows =
        fetch_all::<ClaimWithSlotRow, _>(transaction, claim_query(organization_id, claim_id))
            .await?;
    restore_claim(rows).map_err(Into::into)
}

fn claim_query(
    organization_id: OrganizationId,
    claim_id: ResourceClaimId,
) -> impl Query<Output = ClaimWithSlotRow> {
    select_from::<ResourceClaims>()
        .select(ClaimWithSlotSelection)
        .inner_join::<ResourceClaimSlots>(
            ResourceClaims::id().eq_column(ResourceClaimSlots::claim_id()),
        )
        .filter(ResourceClaims::organization_id().eq(organization_id.as_uuid()))
        .filter(ResourceClaims::id().eq(claim_id.as_uuid()))
        .order_by(ResourceClaimSlots::ordinal(), OrderDirection::Asc)
}

async fn lock_claim_id(
    transaction: &PostgresTransaction,
    claim_id: ResourceClaimId,
) -> Result<(), PostgresPersistenceError> {
    transaction
        .advisory_xact_lock("a3s.cloud.resource-claim", &claim_id.to_string())
        .await?;
    Ok(())
}

enum ClaimMutation {
    BeginPreparation {
        command_id: NodeCommandId,
        at: DateTime<Utc>,
    },
    RecordPrepared {
        command_id: NodeCommandId,
        binding_digest: String,
        at: DateTime<Utc>,
    },
    Bind {
        evidence: ResourceClaimBindingEvidence,
        at: DateTime<Utc>,
    },
    BeginRelease {
        command_id: NodeCommandId,
        at: DateTime<Utc>,
    },
    RecordReleased {
        evidence: ResourceClaimReleaseEvidence,
        at: DateTime<Utc>,
    },
    Orphan {
        failure: String,
        at: DateTime<Utc>,
    },
}

impl ClaimMutation {
    fn apply(self, claim: &mut ResourceClaim) -> Result<(), String> {
        match self {
            Self::BeginPreparation { command_id, at } => claim.begin_preparation(command_id, at),
            Self::RecordPrepared {
                command_id,
                binding_digest,
                at,
            } => claim.record_prepared(command_id, binding_digest, at),
            Self::Bind { evidence, at } => claim.bind(evidence, at),
            Self::BeginRelease { command_id, at } => claim.begin_release(command_id, at),
            Self::RecordReleased { evidence, at } => claim.record_released(evidence, at),
            Self::Orphan { failure, at } => claim.orphan(failure, at),
        }
    }
}
