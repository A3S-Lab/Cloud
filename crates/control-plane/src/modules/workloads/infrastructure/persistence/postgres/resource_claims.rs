use super::resource_claim_rows::{restore_claim, ClaimWithSlotRow, ClaimWithSlotSelection};
use super::resource_claim_writes;
use super::schema::{ResourceClaimSlots, ResourceClaims, WorkloadControls};
use super::{deployment_group_bindings, replicas};
use crate::infrastructure::{
    fetch_all, fetch_optional, transaction_error, PostgresPersistenceError,
};
use crate::modules::fleet::infrastructure::{
    node_pool_placement_is_eligible, require_current_inventory,
};
use crate::modules::shared_kernel::domain::{
    IdempotentWrite, NodeCommandId, NodeId, NodePoolId, OrganizationId, RepositoryError,
    ResourceClaimId, WorkloadId,
};
use crate::modules::workloads::domain::entities::{
    AtomicResourceClaimReservation, ResourceClaim, ResourceClaimBindingEvidence,
    ResourceClaimReleaseEvidence, ResourceClaimReservation, ResourceClaimState,
};
use crate::modules::workloads::domain::repositories::{
    placement_unavailable, IResourceClaimRepository,
};
use a3s_orm::{
    select_from, Database, OrderDirection, PostgresDialect, PostgresExecutor, PostgresTransaction,
    Query,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

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
    async fn has_active_claims(
        &self,
        organization_id: OrganizationId,
        node_id: NodeId,
    ) -> Result<bool, RepositoryError> {
        if node_id.as_uuid().is_nil() {
            return Err(RepositoryError::Conflict(
                "resource claim node is invalid".into(),
            ));
        }
        Database::new(PostgresDialect, self.executor.clone())
            .fetch_optional_as(
                select_from::<ResourceClaims>()
                    .select(ResourceClaims::id())
                    .filter(ResourceClaims::organization_id().eq(organization_id.as_uuid()))
                    .filter(ResourceClaims::node_id().eq(node_id.as_uuid()))
                    .filter(ResourceClaims::state().ne(ResourceClaimState::Released.as_str()))
                    .limit(1),
            )
            .await
            .map(|claim_id| claim_id.is_some())
            .map_err(|error| RepositoryError::Storage(error.to_string()))
    }

    async fn reserve(
        &self,
        reservation: ResourceClaimReservation,
    ) -> Result<IdempotentWrite<ResourceClaim>, RepositoryError> {
        let reservation = AtomicResourceClaimReservation::single(reservation)
            .map_err(RepositoryError::Conflict)?;
        let result = self.reserve_atomically(reservation).await?;
        let mut claims = result.value;
        if claims.len() != 1 {
            return Err(RepositoryError::Storage(
                "single resource reservation returned an invalid Claim count".into(),
            ));
        }
        Ok(IdempotentWrite {
            value: claims.remove(0),
            replayed: result.replayed,
        })
    }

    async fn reserve_atomically(
        &self,
        reservation: AtomicResourceClaimReservation,
    ) -> Result<IdempotentWrite<Vec<ResourceClaim>>, RepositoryError> {
        self.executor
            .transaction(move |transaction| {
                Box::pin(reserve_atomically_in_transaction(transaction, reservation))
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

    async fn cancel_database_reservation(
        &self,
        organization_id: OrganizationId,
        claim_id: ResourceClaimId,
        expected_version: u64,
        at: DateTime<Utc>,
    ) -> Result<ResourceClaim, RepositoryError> {
        self.mutate(
            organization_id,
            claim_id,
            expected_version,
            ClaimMutation::CancelDatabaseReservation { at },
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

async fn reserve_atomically_in_transaction(
    transaction: &PostgresTransaction,
    reservation: AtomicResourceClaimReservation,
) -> Result<IdempotentWrite<Vec<ResourceClaim>>, PostgresPersistenceError> {
    let reservations = reservation.into_reservations();
    let mut claim_ids = reservations
        .iter()
        .map(|member| member.id)
        .collect::<Vec<_>>();
    claim_ids.sort_unstable();
    for claim_id in claim_ids {
        lock_claim_id(transaction, claim_id).await?;
    }

    let mut existing = Vec::with_capacity(reservations.len());
    for member in &reservations {
        if let Some(claim) =
            find_claim_in_transaction(transaction, member.binding.organization_id, member.id)
                .await?
        {
            existing.push((member, claim));
        }
    }
    if !existing.is_empty() {
        if existing.len() != reservations.len()
            || existing
                .iter()
                .any(|(member, claim)| !member.matches(claim))
        {
            return Err(RepositoryError::IdempotencyConflict.into());
        }
        return Ok(IdempotentWrite {
            value: existing.into_iter().map(|(_, claim)| claim).collect(),
            replayed: true,
        });
    }

    let mut claims = Vec::with_capacity(reservations.len());
    for member in reservations {
        claims.push(reserve_new_claim_in_transaction(transaction, member).await?);
    }
    Ok(IdempotentWrite {
        value: claims,
        replayed: false,
    })
}

async fn reserve_new_claim_in_transaction(
    transaction: &PostgresTransaction,
    reservation: ResourceClaimReservation,
) -> Result<ResourceClaim, PostgresPersistenceError> {
    require_node_pool_placement_eligible(
        transaction,
        reservation.binding.organization_id,
        reservation.binding.workload_id,
        reservation.node_id,
    )
    .await?;
    let persisted_binding = deployment_group_bindings::find_member_binding_in_transaction(
        transaction,
        reservation.binding.organization_id,
        reservation.binding.deployment_id,
        reservation.binding.member_id,
    )
    .await?
    .ok_or(RepositoryError::NotFound)?;
    let member = replicas::member_in_transaction(
        transaction,
        reservation.binding.organization_id,
        reservation.binding.replica_id,
        reservation.binding.member_id,
    )
    .await?
    .ok_or(RepositoryError::NotFound)?;
    let member_matches = match member.node_id {
        Some(node_id) => {
            node_id == reservation.node_id
                && member.placement_generation == reservation.binding.placement_generation
        }
        None => {
            member.placement_generation.checked_add(1)
                == Some(reservation.binding.placement_generation)
        }
    };
    let binding_matches = persisted_binding == reservation.binding
        || (member_matches
            && persisted_binding
                .propose_assignment(reservation.node_id, reservation.binding.updated_at)
                .is_ok_and(|candidate| candidate == reservation.binding));
    if !binding_matches {
        return Err(RepositoryError::Conflict(
            "resource claim reservation does not match the durable or proposed initial replica binding"
                .into(),
        )
        .into());
    }
    require_replica_anti_affinity(transaction, &reservation).await?;
    require_current_inventory(
        transaction,
        reservation.binding.organization_id,
        &reservation.inventory,
    )
    .await?;
    let slots = resource_claim_writes::reserve_slots(transaction, &reservation).await?;
    let claim = ResourceClaim::reserve(&reservation, slots).map_err(RepositoryError::Conflict)?;
    resource_claim_writes::insert_claim(transaction, &claim).await?;
    Ok(claim)
}

pub(super) async fn require_node_pool_placement_eligible(
    transaction: &PostgresTransaction,
    organization_id: OrganizationId,
    workload_id: WorkloadId,
    node_id: NodeId,
) -> Result<(), PostgresPersistenceError> {
    let node_pool_id = fetch_optional::<Option<Uuid>, _>(
        transaction,
        select_from::<WorkloadControls>()
            .select(WorkloadControls::node_pool_id())
            .filter(WorkloadControls::organization_id().eq(organization_id.as_uuid()))
            .filter(WorkloadControls::workload_id().eq(workload_id.as_uuid()))
            .limit(1),
    )
    .await?
    .ok_or(RepositoryError::NotFound)?
    .map(NodePoolId::from_uuid);
    if !node_pool_placement_is_eligible(transaction, organization_id, node_pool_id, node_id).await?
    {
        return Err(placement_unavailable(
            "node pool membership changed before resource placement was committed",
        )
        .into());
    }
    Ok(())
}

async fn require_replica_anti_affinity(
    transaction: &PostgresTransaction,
    reservation: &ResourceClaimReservation,
) -> Result<(), PostgresPersistenceError> {
    let binding = &reservation.binding;
    transaction
        .advisory_xact_lock(
            "a3s.cloud.workload-replica-anti-affinity",
            &format!(
                "{}:{}:{}",
                binding.organization_id, binding.workload_id, reservation.node_id
            ),
        )
        .await?;
    let conflicting_claim = fetch_optional::<Uuid, _>(
        transaction,
        select_from::<ResourceClaims>()
            .select(ResourceClaims::id())
            .filter(ResourceClaims::organization_id().eq(binding.organization_id.as_uuid()))
            .filter(ResourceClaims::workload_id().eq(binding.workload_id.as_uuid()))
            .filter(ResourceClaims::node_id().eq(reservation.node_id.as_uuid()))
            .filter(ResourceClaims::replica_id().ne(binding.replica_id.as_uuid()))
            .filter(ResourceClaims::state().ne(ResourceClaimState::Released.as_str()))
            .limit(1),
    )
    .await?;
    if conflicting_claim.is_some() {
        return Err(placement_unavailable(
            "required anti-affinity excludes a node with another active Workload replica",
        )
        .into());
    }
    Ok(())
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
    CancelDatabaseReservation {
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
            Self::CancelDatabaseReservation { at } => claim.cancel_database_reservation(at),
            Self::Orphan { failure, at } => claim.orphan(failure, at),
        }
    }
}
