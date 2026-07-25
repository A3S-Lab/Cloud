use super::resource_claim_rows::{SlotLeaseRow, SlotLeaseSelection};
use super::schema::{ResourceClaimSlots, ResourceClaims, ResourceSlotLeases};
use crate::infrastructure::{
    execute, fetch_optional, is_unique_violation, require_one_row, PostgresPersistenceError,
};
use crate::modules::shared_kernel::domain::{canonical_timestamp, NodeCommandId, RepositoryError};
use crate::modules::workloads::domain::entities::{
    ResourceAllocation, ResourceClaim, ResourceClaimReservation, ResourceSlotBinding,
    ResourceSlotRequest,
};
use crate::modules::workloads::domain::repositories::capacity_unavailable;
use a3s_orm::{insert_into, select_from, update_table, InsertRow, PostgresTransaction, Query};
use uuid::Uuid;

pub(super) async fn reserve_slots(
    transaction: &PostgresTransaction,
    reservation: &ResourceClaimReservation,
) -> Result<Vec<ResourceSlotBinding>, PostgresPersistenceError> {
    let reserved_at = canonical_timestamp(reservation.reserved_at);
    let mut slots = Vec::with_capacity(reservation.slots.len());
    for request in &reservation.slots {
        lock_slot(transaction, reservation, request).await?;
        if request.kind.is_shared_capacity() {
            validate_shared_capacity(transaction, reservation, request).await?;
        }
        let existing = fetch_optional::<SlotLeaseRow, _>(
            transaction,
            select_from::<ResourceSlotLeases>()
                .select(SlotLeaseSelection)
                .filter(
                    ResourceSlotLeases::organization_id()
                        .eq(reservation.binding.organization_id.as_uuid()),
                )
                .filter(ResourceSlotLeases::node_id().eq(reservation.node_id.as_uuid()))
                .filter(ResourceSlotLeases::resource_kind().eq(request.kind.as_str()))
                .filter(
                    ResourceSlotLeases::stable_resource_id()
                        .eq(request.stable_resource_id.as_str()),
                ),
        )
        .await?;
        let slot = match existing {
            Some(lease) => {
                validate_lease_identity(
                    &lease,
                    reservation,
                    request.kind.as_str(),
                    &request.stable_resource_id,
                )?;
                if lease.active_claim_id.is_some() {
                    return Err(slot_conflict(&request.stable_resource_id));
                }
                if reserved_at < lease.updated_at {
                    return Err(RepositoryError::Conflict(
                        "hard resource reservation time regressed behind its slot ledger".into(),
                    )
                    .into());
                }
                let generation = lease.slot_generation.checked_add(1).ok_or_else(|| {
                    PostgresPersistenceError::Invariant(
                        "hard resource slot generation overflowed".into(),
                    )
                })?;
                let fence_token = Uuid::new_v4();
                let rows = execute(
                    transaction,
                    update_table::<ResourceSlotLeases>()
                        .set(ResourceSlotLeases::slot_generation(), generation)
                        .set(ResourceSlotLeases::fence_token(), fence_token)
                        .set(
                            ResourceSlotLeases::active_claim_id(),
                            (!request.kind.is_shared_capacity())
                                .then_some(reservation.id.as_uuid()),
                        )
                        .set(ResourceSlotLeases::updated_at(), reserved_at)
                        .filter(
                            ResourceSlotLeases::organization_id()
                                .eq(reservation.binding.organization_id.as_uuid()),
                        )
                        .filter(ResourceSlotLeases::node_id().eq(reservation.node_id.as_uuid()))
                        .filter(ResourceSlotLeases::resource_kind().eq(request.kind.as_str()))
                        .filter(
                            ResourceSlotLeases::stable_resource_id()
                                .eq(request.stable_resource_id.as_str()),
                        )
                        .filter(ResourceSlotLeases::slot_generation().eq(lease.slot_generation))
                        .filter(ResourceSlotLeases::active_claim_id().is_null()),
                )
                .await?;
                if rows != 1 {
                    return Err(slot_conflict(&request.stable_resource_id));
                }
                ResourceSlotBinding {
                    kind: request.kind,
                    stable_resource_id: request.stable_resource_id.clone(),
                    allocation: request.allocation.clone(),
                    slot_generation: generation,
                    fence_token,
                }
            }
            None => {
                let fence_token = Uuid::new_v4();
                let inserted = execute(
                    transaction,
                    insert_into::<ResourceSlotLeases>()
                        .value(
                            ResourceSlotLeases::organization_id(),
                            reservation.binding.organization_id.as_uuid(),
                        )
                        .value(ResourceSlotLeases::node_id(), reservation.node_id.as_uuid())
                        .value(ResourceSlotLeases::resource_kind(), request.kind.as_str())
                        .value(
                            ResourceSlotLeases::stable_resource_id(),
                            request.stable_resource_id.as_str(),
                        )
                        .value(ResourceSlotLeases::slot_generation(), 1_u64)
                        .value(ResourceSlotLeases::fence_token(), fence_token)
                        .value(
                            ResourceSlotLeases::active_claim_id(),
                            (!request.kind.is_shared_capacity())
                                .then_some(reservation.id.as_uuid()),
                        )
                        .value(ResourceSlotLeases::updated_at(), reserved_at)
                        .on_conflict((
                            ResourceSlotLeases::organization_id(),
                            ResourceSlotLeases::node_id(),
                            ResourceSlotLeases::resource_kind(),
                            ResourceSlotLeases::stable_resource_id(),
                        ))
                        .do_nothing(),
                )
                .await?;
                if inserted != 1 {
                    return Err(slot_conflict(&request.stable_resource_id));
                }
                ResourceSlotBinding {
                    kind: request.kind,
                    stable_resource_id: request.stable_resource_id.clone(),
                    allocation: request.allocation.clone(),
                    slot_generation: 1,
                    fence_token,
                }
            }
        };
        slots.push(slot);
    }
    Ok(slots)
}

pub(super) async fn insert_claim(
    transaction: &PostgresTransaction,
    claim: &ResourceClaim,
) -> Result<(), PostgresPersistenceError> {
    let release_evidence = claim
        .release_evidence
        .as_ref()
        .map(serde_json::to_value)
        .transpose()?;
    let inserted = execute(
        transaction,
        insert_into::<ResourceClaims>()
            .value(ResourceClaims::id(), claim.id.as_uuid())
            .value(
                ResourceClaims::organization_id(),
                claim.organization_id.as_uuid(),
            )
            .value(ResourceClaims::project_id(), claim.project_id.as_uuid())
            .value(
                ResourceClaims::environment_id(),
                claim.environment_id.as_uuid(),
            )
            .value(ResourceClaims::workload_id(), claim.workload_id.as_uuid())
            .value(
                ResourceClaims::deployment_id(),
                claim.deployment_id.as_uuid(),
            )
            .value(ResourceClaims::replica_id(), claim.replica_id.as_uuid())
            .value(
                ResourceClaims::replica_generation(),
                claim.replica_generation,
            )
            .value(ResourceClaims::member_id(), claim.member_id.as_uuid())
            .value(
                ResourceClaims::placement_generation(),
                claim.placement_generation,
            )
            .value(ResourceClaims::node_id(), claim.node_id.as_uuid())
            .value(
                ResourceClaims::inventory_generation(),
                claim.inventory_generation,
            )
            .value(
                ResourceClaims::inventory_digest(),
                claim.inventory_digest.as_str(),
            )
            .value(
                ResourceClaims::runtime_unit_id(),
                claim.runtime_unit_id.as_str(),
            )
            .value(
                ResourceClaims::runtime_generation(),
                claim.runtime_generation,
            )
            .value(
                ResourceClaims::topology_digest(),
                claim.topology_digest.as_str(),
            )
            .value(
                ResourceClaims::reservation_digest(),
                claim.reservation_digest.as_str(),
            )
            .value(ResourceClaims::claim_generation(), claim.claim_generation)
            .value(ResourceClaims::claim_digest(), claim.claim_digest.as_str())
            .value(ResourceClaims::state(), claim.state.as_str())
            .value(
                ResourceClaims::prepare_command_id(),
                claim.prepare_command_id.map(NodeCommandId::as_uuid),
            )
            .value(
                ResourceClaims::prepared_binding_digest(),
                claim.prepared_binding_digest.clone(),
            )
            .value(
                ResourceClaims::release_command_id(),
                claim.release_command_id.map(NodeCommandId::as_uuid),
            )
            .value(ResourceClaims::release_evidence(), release_evidence)
            .value(ResourceClaims::failure(), claim.failure.clone())
            .value(ResourceClaims::aggregate_version(), claim.aggregate_version)
            .value(ResourceClaims::created_at(), claim.created_at)
            .value(ResourceClaims::updated_at(), claim.updated_at)
            .value(ResourceClaims::prepared_at(), claim.prepared_at)
            .value(ResourceClaims::bound_at(), claim.bound_at)
            .value(
                ResourceClaims::release_requested_at(),
                claim.release_requested_at,
            )
            .value(ResourceClaims::released_at(), claim.released_at)
            .value(ResourceClaims::orphaned_at(), claim.orphaned_at),
    )
    .await;
    match inserted {
        Ok(rows) => require_one_row("resource claim", rows)?,
        Err(error) if is_unique_violation(&error) => {
            return Err(RepositoryError::Conflict(
                "resource claim identity or active replica member is already in use".into(),
            )
            .into())
        }
        Err(error) => return Err(error),
    }

    let mut rows = Vec::with_capacity(claim.slots.len());
    for (ordinal, slot) in claim.slots.iter().enumerate() {
        let ordinal = u32::try_from(ordinal).map_err(|_| {
            PostgresPersistenceError::Invariant("resource claim slot ordinal overflowed".into())
        })?;
        rows.push(
            InsertRow::new()
                .value(ResourceClaimSlots::claim_id(), claim.id.as_uuid())
                .value(ResourceClaimSlots::ordinal(), ordinal)
                .value(
                    ResourceClaimSlots::organization_id(),
                    claim.organization_id.as_uuid(),
                )
                .value(ResourceClaimSlots::node_id(), claim.node_id.as_uuid())
                .value(ResourceClaimSlots::resource_kind(), slot.kind.as_str())
                .value(
                    ResourceClaimSlots::stable_resource_id(),
                    slot.stable_resource_id.as_str(),
                )
                .value(
                    ResourceClaimSlots::allocation(),
                    serde_json::to_value(&slot.allocation)?,
                )
                .value(ResourceClaimSlots::slot_generation(), slot.slot_generation)
                .value(ResourceClaimSlots::fence_token(), slot.fence_token)
                .value(ResourceClaimSlots::created_at(), claim.created_at)
                .value(ResourceClaimSlots::released_at(), claim.released_at),
        );
    }
    let inserted = execute(transaction, insert_into::<ResourceClaimSlots>().rows(rows)).await;
    match inserted {
        Ok(count) if usize::try_from(count).ok() == Some(claim.slots.len()) => Ok(()),
        Ok(count) => Err(PostgresPersistenceError::Invariant(format!(
            "writing resource claim slots affected {count} rows"
        ))),
        Err(error) if is_unique_violation(&error) => Err(RepositoryError::Conflict(
            "one or more hard resource slots are already claimed".into(),
        )
        .into()),
        Err(error) => Err(error),
    }
}

pub(super) async fn persist_claim(
    transaction: &PostgresTransaction,
    claim: &ResourceClaim,
    expected_version: u64,
) -> Result<(), PostgresPersistenceError> {
    let release_evidence = claim
        .release_evidence
        .as_ref()
        .map(serde_json::to_value)
        .transpose()?;
    let rows = execute(
        transaction,
        update_table::<ResourceClaims>()
            .set(ResourceClaims::claim_generation(), claim.claim_generation)
            .set(ResourceClaims::claim_digest(), claim.claim_digest.as_str())
            .set(ResourceClaims::state(), claim.state.as_str())
            .set(
                ResourceClaims::prepare_command_id(),
                claim.prepare_command_id.map(NodeCommandId::as_uuid),
            )
            .set(
                ResourceClaims::prepared_binding_digest(),
                claim.prepared_binding_digest.clone(),
            )
            .set(
                ResourceClaims::release_command_id(),
                claim.release_command_id.map(NodeCommandId::as_uuid),
            )
            .set(ResourceClaims::release_evidence(), release_evidence)
            .set(ResourceClaims::failure(), claim.failure.clone())
            .set(ResourceClaims::aggregate_version(), claim.aggregate_version)
            .set(ResourceClaims::updated_at(), claim.updated_at)
            .set(ResourceClaims::prepared_at(), claim.prepared_at)
            .set(ResourceClaims::bound_at(), claim.bound_at)
            .set(
                ResourceClaims::release_requested_at(),
                claim.release_requested_at,
            )
            .set(ResourceClaims::released_at(), claim.released_at)
            .set(ResourceClaims::orphaned_at(), claim.orphaned_at)
            .filter(ResourceClaims::id().eq(claim.id.as_uuid()))
            .filter(ResourceClaims::organization_id().eq(claim.organization_id.as_uuid()))
            .filter(ResourceClaims::aggregate_version().eq(expected_version)),
    )
    .await?;
    if rows == 1 {
        Ok(())
    } else {
        Err(RepositoryError::Conflict(format!(
            "resource claim changed from expected version {expected_version}"
        ))
        .into())
    }
}

pub(super) async fn release_slots(
    transaction: &PostgresTransaction,
    claim: &ResourceClaim,
) -> Result<(), PostgresPersistenceError> {
    let released_at = claim.released_at.ok_or_else(|| {
        PostgresPersistenceError::Invariant(
            "released resource claim omitted its release time".into(),
        )
    })?;
    for slot in &claim.slots {
        lock_bound_slot(transaction, claim, slot).await?;
        if slot.kind.is_shared_capacity() {
            let lease = fetch_optional::<SlotLeaseRow, _>(
                transaction,
                select_from::<ResourceSlotLeases>()
                    .select(SlotLeaseSelection)
                    .filter(
                        ResourceSlotLeases::organization_id().eq(claim.organization_id.as_uuid()),
                    )
                    .filter(ResourceSlotLeases::node_id().eq(claim.node_id.as_uuid()))
                    .filter(ResourceSlotLeases::resource_kind().eq(slot.kind.as_str()))
                    .filter(
                        ResourceSlotLeases::stable_resource_id()
                            .eq(slot.stable_resource_id.as_str()),
                    ),
            )
            .await?
            .ok_or_else(|| {
                PostgresPersistenceError::Invariant(
                    "shared resource claim references a missing slot ledger".into(),
                )
            })?;
            if lease.active_claim_id.is_some()
                || lease.slot_generation < slot.slot_generation
                || (lease.slot_generation == slot.slot_generation
                    && lease.fence_token != slot.fence_token)
            {
                return Err(PostgresPersistenceError::Invariant(
                    "shared resource slot ledger fencing identity is inconsistent".into(),
                ));
            }
            continue;
        }
        let rows = execute(
            transaction,
            update_table::<ResourceSlotLeases>()
                .set(ResourceSlotLeases::active_claim_id(), None::<Uuid>)
                .set(ResourceSlotLeases::updated_at(), released_at)
                .filter(ResourceSlotLeases::organization_id().eq(claim.organization_id.as_uuid()))
                .filter(ResourceSlotLeases::node_id().eq(claim.node_id.as_uuid()))
                .filter(ResourceSlotLeases::resource_kind().eq(slot.kind.as_str()))
                .filter(
                    ResourceSlotLeases::stable_resource_id().eq(slot.stable_resource_id.as_str()),
                )
                .filter(ResourceSlotLeases::slot_generation().eq(slot.slot_generation))
                .filter(ResourceSlotLeases::fence_token().eq(slot.fence_token))
                .filter(ResourceSlotLeases::active_claim_id().eq(Some(claim.id.as_uuid()))),
        )
        .await?;
        require_one_row("active hard resource slot lease", rows)?;
    }
    let rows = execute(
        transaction,
        update_table::<ResourceClaimSlots>()
            .set(ResourceClaimSlots::released_at(), Some(released_at))
            .filter(ResourceClaimSlots::claim_id().eq(claim.id.as_uuid()))
            .filter(ResourceClaimSlots::released_at().is_null()),
    )
    .await?;
    if usize::try_from(rows).ok() == Some(claim.slots.len()) {
        Ok(())
    } else {
        Err(PostgresPersistenceError::Invariant(format!(
            "releasing resource claim slots affected {rows} rows"
        )))
    }
}

fn validate_lease_identity(
    lease: &SlotLeaseRow,
    reservation: &ResourceClaimReservation,
    resource_kind: &str,
    stable_resource_id: &str,
) -> Result<(), PostgresPersistenceError> {
    if lease.organization_id != reservation.binding.organization_id.as_uuid()
        || lease.node_id != reservation.node_id.as_uuid()
        || lease.resource_kind != resource_kind
        || lease.stable_resource_id != stable_resource_id
        || lease.slot_generation == 0
        || lease.fence_token.is_nil()
    {
        return Err(PostgresPersistenceError::Invariant(
            "stored hard resource slot lease is invalid".into(),
        ));
    }
    Ok(())
}

fn slot_conflict(stable_resource_id: &str) -> PostgresPersistenceError {
    capacity_unavailable(format!(
        "hard resource slot {stable_resource_id} is already claimed"
    ))
    .into()
}

async fn validate_shared_capacity(
    transaction: &PostgresTransaction,
    reservation: &ResourceClaimReservation,
    request: &ResourceSlotRequest,
) -> Result<(), PostgresPersistenceError> {
    let capacity = reservation
        .inventory
        .slots
        .iter()
        .find(|slot| {
            slot.kind == request.kind && slot.stable_resource_id == request.stable_resource_id
        })
        .and_then(|slot| slot.allocation.scalar_amount())
        .ok_or_else(|| {
            RepositoryError::Conflict(format!(
                "shared resource slot {} has no scalar inventory capacity",
                request.stable_resource_id
            ))
        })?;
    let requested = request.allocation.scalar_amount().ok_or_else(|| {
        RepositoryError::Conflict(format!(
            "shared resource slot {} has a non-scalar request",
            request.stable_resource_id
        ))
    })?;
    let allocations = crate::infrastructure::fetch_all::<serde_json::Value, _>(
        transaction,
        active_allocations_query(reservation, request),
    )
    .await?;
    let allocated = allocations.into_iter().try_fold(0_u64, |total, value| {
        let allocation = serde_json::from_value::<ResourceAllocation>(value).map_err(|error| {
            PostgresPersistenceError::Invariant(format!(
                "stored shared resource allocation is invalid: {error}"
            ))
        })?;
        let amount = allocation.scalar_amount().ok_or_else(|| {
            PostgresPersistenceError::Invariant(
                "stored shared resource allocation is not scalar".into(),
            )
        })?;
        total.checked_add(amount).ok_or_else(|| {
            PostgresPersistenceError::Invariant(
                "stored shared resource allocation total overflowed".into(),
            )
        })
    })?;
    if allocated
        .checked_add(requested)
        .is_none_or(|required| required > capacity)
    {
        return Err(capacity_unavailable(format!(
            "shared resource slot {} has insufficient remaining capacity",
            request.stable_resource_id
        ))
        .into());
    }
    Ok(())
}

fn active_allocations_query(
    reservation: &ResourceClaimReservation,
    request: &ResourceSlotRequest,
) -> impl Query<Output = serde_json::Value> {
    select_from::<ResourceClaimSlots>()
        .select(ResourceClaimSlots::allocation())
        .filter(
            ResourceClaimSlots::organization_id().eq(reservation.binding.organization_id.as_uuid()),
        )
        .filter(ResourceClaimSlots::node_id().eq(reservation.node_id.as_uuid()))
        .filter(ResourceClaimSlots::resource_kind().eq(request.kind.as_str()))
        .filter(ResourceClaimSlots::stable_resource_id().eq(request.stable_resource_id.as_str()))
        .filter(ResourceClaimSlots::released_at().is_null())
}

async fn lock_slot(
    transaction: &PostgresTransaction,
    reservation: &ResourceClaimReservation,
    request: &ResourceSlotRequest,
) -> Result<(), PostgresPersistenceError> {
    transaction
        .advisory_xact_lock(
            "a3s.cloud.resource-slot",
            &format!(
                "{}/{}/{}/{}",
                reservation.binding.organization_id,
                reservation.node_id,
                request.kind.as_str(),
                request.stable_resource_id
            ),
        )
        .await?;
    Ok(())
}

async fn lock_bound_slot(
    transaction: &PostgresTransaction,
    claim: &ResourceClaim,
    slot: &ResourceSlotBinding,
) -> Result<(), PostgresPersistenceError> {
    transaction
        .advisory_xact_lock(
            "a3s.cloud.resource-slot",
            &format!(
                "{}/{}/{}/{}",
                claim.organization_id,
                claim.node_id,
                slot.kind.as_str(),
                slot.stable_resource_id
            ),
        )
        .await?;
    Ok(())
}
