use crate::modules::shared_kernel::domain::{
    IdempotentWrite, NodeCommandId, NodeId, OrganizationId, RepositoryError, ResourceClaimId,
};
use crate::modules::workloads::domain::entities::{
    AtomicResourceClaimReservation, ResourceClaim, ResourceClaimBindingEvidence,
    ResourceClaimReleaseEvidence, ResourceClaimReservation, ResourceClaimState, ResourceKind,
    ResourceSlotBinding,
};
use crate::modules::workloads::domain::repositories::{
    capacity_unavailable, placement_unavailable, IResourceClaimRepository,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Default)]
pub struct InMemoryResourceClaimRepository {
    state: RwLock<State>,
}

#[derive(Clone, Default)]
struct State {
    claims: BTreeMap<ResourceClaimId, ResourceClaim>,
    slots: BTreeMap<SlotKey, SlotLease>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SlotKey {
    organization_id: OrganizationId,
    node_id: NodeId,
    kind: ResourceKind,
    stable_resource_id: String,
}

#[derive(Debug, Clone)]
struct SlotLease {
    generation: u64,
    fence_token: Uuid,
    active_claim_id: Option<ResourceClaimId>,
}

impl InMemoryResourceClaimRepository {
    pub fn new() -> Self {
        Self::default()
    }

    async fn mutate(
        &self,
        organization_id: OrganizationId,
        claim_id: ResourceClaimId,
        expected_version: u64,
        transition: impl FnOnce(&mut ResourceClaim) -> Result<(), String>,
    ) -> Result<ResourceClaim, RepositoryError> {
        let mut state = self.state.write().await;
        let current = state
            .claims
            .get(&claim_id)
            .filter(|claim| claim.organization_id == organization_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)?;
        let mut next = current.clone();
        transition(&mut next).map_err(RepositoryError::Conflict)?;
        if next == current {
            return Ok(current);
        }
        if current.aggregate_version != expected_version {
            return Err(version_conflict(
                expected_version,
                current.aggregate_version,
            ));
        }
        if next.state == ResourceClaimState::Released {
            release_slots(&mut state, &next)?;
        }
        state.claims.insert(claim_id, next.clone());
        Ok(next)
    }
}

#[async_trait]
impl IResourceClaimRepository for InMemoryResourceClaimRepository {
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
        Ok(self.state.read().await.claims.values().any(|claim| {
            claim.organization_id == organization_id
                && claim.node_id == node_id
                && claim.state != ResourceClaimState::Released
        }))
    }

    async fn reserve(
        &self,
        reservation: ResourceClaimReservation,
    ) -> Result<IdempotentWrite<ResourceClaim>, RepositoryError> {
        let batch = AtomicResourceClaimReservation::single(reservation)
            .map_err(RepositoryError::Conflict)?;
        let result = self.reserve_atomically(batch).await?;
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
        let mut state = self.state.write().await;
        let existing = reservation
            .reservations()
            .iter()
            .filter_map(|member| state.claims.get(&member.id).map(|claim| (member, claim)))
            .collect::<Vec<_>>();
        if !existing.is_empty() {
            if existing.len() != reservation.reservations().len()
                || existing
                    .iter()
                    .any(|(member, claim)| !member.matches(claim))
            {
                return Err(RepositoryError::IdempotencyConflict);
            }
            return Ok(IdempotentWrite {
                value: existing
                    .into_iter()
                    .map(|(_, claim)| claim.clone())
                    .collect(),
                replayed: true,
            });
        }

        let mut next = state.clone();
        let mut claims = Vec::with_capacity(reservation.reservations().len());
        for member in reservation.into_reservations() {
            claims.push(reserve_new_claim(&mut next, member)?);
        }
        *state = next;
        Ok(IdempotentWrite {
            value: claims,
            replayed: false,
        })
    }

    async fn find(
        &self,
        organization_id: OrganizationId,
        claim_id: ResourceClaimId,
    ) -> Result<ResourceClaim, RepositoryError> {
        self.state
            .read()
            .await
            .claims
            .get(&claim_id)
            .filter(|claim| claim.organization_id == organization_id)
            .cloned()
            .ok_or(RepositoryError::NotFound)
    }

    async fn begin_preparation(
        &self,
        organization_id: OrganizationId,
        claim_id: ResourceClaimId,
        expected_version: u64,
        command_id: NodeCommandId,
        at: DateTime<Utc>,
    ) -> Result<ResourceClaim, RepositoryError> {
        self.mutate(organization_id, claim_id, expected_version, move |claim| {
            claim.begin_preparation(command_id, at)
        })
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
        self.mutate(organization_id, claim_id, expected_version, move |claim| {
            claim.record_prepared(command_id, binding_digest, at)
        })
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
        self.mutate(organization_id, claim_id, expected_version, move |claim| {
            claim.bind(evidence, at)
        })
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
        self.mutate(organization_id, claim_id, expected_version, move |claim| {
            claim.begin_release(command_id, at)
        })
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
        self.mutate(organization_id, claim_id, expected_version, move |claim| {
            claim.record_released(evidence, at)
        })
        .await
    }

    async fn cancel_database_reservation(
        &self,
        organization_id: OrganizationId,
        claim_id: ResourceClaimId,
        expected_version: u64,
        at: DateTime<Utc>,
    ) -> Result<ResourceClaim, RepositoryError> {
        self.mutate(organization_id, claim_id, expected_version, move |claim| {
            claim.cancel_database_reservation(at)
        })
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
        self.mutate(organization_id, claim_id, expected_version, move |claim| {
            claim.orphan(failure, at)
        })
        .await
    }
}

fn reserve_new_claim(
    state: &mut State,
    reservation: ResourceClaimReservation,
) -> Result<ResourceClaim, RepositoryError> {
    if state.claims.contains_key(&reservation.id) {
        return Err(RepositoryError::IdempotencyConflict);
    }
    if state.claims.values().any(|claim| {
        claim.organization_id == reservation.binding.organization_id
            && claim.workload_id == reservation.binding.workload_id
            && claim.node_id == reservation.node_id
            && claim.replica_id != reservation.binding.replica_id
            && claim.state != ResourceClaimState::Released
    }) {
        return Err(placement_unavailable(
            "required anti-affinity excludes a node with another active Workload replica",
        ));
    }

    let mut reserved_slots = Vec::with_capacity(reservation.slots.len());
    for request in &reservation.slots {
        let key = SlotKey {
            organization_id: reservation.binding.organization_id,
            node_id: reservation.node_id,
            kind: request.kind,
            stable_resource_id: request.stable_resource_id.clone(),
        };
        if request.kind.is_shared_capacity() {
            validate_shared_capacity(state, &reservation, request)?;
        } else if state
            .slots
            .get(&key)
            .is_some_and(|lease| lease.active_claim_id.is_some())
        {
            return Err(capacity_unavailable(format!(
                "hard resource slot {} is already claimed",
                request.stable_resource_id
            )));
        }
        let generation = state.slots.get(&key).map_or(Ok(1), |lease| {
            lease.generation.checked_add(1).ok_or_else(|| {
                RepositoryError::Storage("hard resource slot generation overflowed".into())
            })
        })?;
        reserved_slots.push((
            key,
            ResourceSlotBinding {
                kind: request.kind,
                stable_resource_id: request.stable_resource_id.clone(),
                allocation: request.allocation.clone(),
                slot_generation: generation,
                fence_token: Uuid::new_v4(),
            },
        ));
    }

    let claim = ResourceClaim::reserve(
        &reservation,
        reserved_slots
            .iter()
            .map(|(_, slot)| slot.clone())
            .collect(),
    )
    .map_err(RepositoryError::Conflict)?;
    for (key, slot) in reserved_slots {
        state.slots.insert(
            key,
            SlotLease {
                generation: slot.slot_generation,
                fence_token: slot.fence_token,
                active_claim_id: (!slot.kind.is_shared_capacity()).then_some(claim.id),
            },
        );
    }
    state.claims.insert(claim.id, claim.clone());
    Ok(claim)
}

fn release_slots(state: &mut State, claim: &ResourceClaim) -> Result<(), RepositoryError> {
    for slot in &claim.slots {
        let key = SlotKey {
            organization_id: claim.organization_id,
            node_id: claim.node_id,
            kind: slot.kind,
            stable_resource_id: slot.stable_resource_id.clone(),
        };
        let lease = state.slots.get(&key).ok_or_else(|| {
            RepositoryError::Storage("resource claim references a missing slot lease".into())
        })?;
        let identity_matches = if slot.kind.is_shared_capacity() {
            lease.generation >= slot.slot_generation
                && (lease.generation != slot.slot_generation
                    || lease.fence_token == slot.fence_token)
                && lease.active_claim_id.is_none()
        } else {
            lease.active_claim_id == Some(claim.id)
                && lease.generation == slot.slot_generation
                && lease.fence_token == slot.fence_token
        };
        if !identity_matches {
            return Err(RepositoryError::Storage(
                "resource claim slot lease fencing identity is inconsistent".into(),
            ));
        }
    }
    for slot in &claim.slots {
        if slot.kind.is_shared_capacity() {
            continue;
        }
        let key = SlotKey {
            organization_id: claim.organization_id,
            node_id: claim.node_id,
            kind: slot.kind,
            stable_resource_id: slot.stable_resource_id.clone(),
        };
        let lease = state.slots.get_mut(&key).ok_or_else(|| {
            RepositoryError::Storage(
                "verified resource claim slot lease disappeared before release".into(),
            )
        })?;
        lease.active_claim_id = None;
    }
    Ok(())
}

fn validate_shared_capacity(
    state: &State,
    reservation: &ResourceClaimReservation,
    request: &crate::modules::workloads::domain::entities::ResourceSlotRequest,
) -> Result<(), RepositoryError> {
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
    let allocated = state
        .claims
        .values()
        .filter(|claim| {
            claim.organization_id == reservation.binding.organization_id
                && claim.node_id == reservation.node_id
                && claim.state != ResourceClaimState::Released
        })
        .flat_map(|claim| &claim.slots)
        .filter(|slot| {
            slot.kind == request.kind && slot.stable_resource_id == request.stable_resource_id
        })
        .try_fold(0_u64, |total, slot| {
            let amount = slot.allocation.scalar_amount().ok_or_else(|| {
                RepositoryError::Storage(
                    "stored shared resource claim has a non-scalar allocation".into(),
                )
            })?;
            total.checked_add(amount).ok_or_else(|| {
                RepositoryError::Storage("shared resource allocation total overflowed".into())
            })
        })?;
    if allocated
        .checked_add(requested)
        .is_none_or(|required| required > capacity)
    {
        return Err(capacity_unavailable(format!(
            "shared resource slot {} has insufficient remaining capacity",
            request.stable_resource_id
        )));
    }
    Ok(())
}

fn version_conflict(expected: u64, actual: u64) -> RepositoryError {
    RepositoryError::Conflict(format!(
        "resource claim changed from expected version {expected} to {actual}"
    ))
}
