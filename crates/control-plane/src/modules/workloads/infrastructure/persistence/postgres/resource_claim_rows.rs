use super::schema::{ResourceClaimSlots, ResourceClaims, ResourceSlotLeases};
use crate::modules::shared_kernel::domain::{
    DeploymentId, EnvironmentId, NodeCommandId, NodeId, OrganizationId, ProjectId, RepositoryError,
    ResourceClaimId, WorkloadId, WorkloadReplicaId, WorkloadReplicaMemberId,
};
use crate::modules::workloads::domain::entities::{
    ResourceAllocation, ResourceClaim, ResourceClaimReleaseEvidence, ResourceClaimState,
    ResourceKind, ResourceSlotBinding,
};
use a3s_orm::expression::Selection;
use a3s_orm::{DecodeError, Expression, FromRow, FromValue, Row};
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub(super) struct ClaimWithSlotSelection;

impl Selection for ClaimWithSlotSelection {
    type Output = ClaimWithSlotRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            ResourceClaims::id().expression(),
            ResourceClaims::organization_id().expression(),
            ResourceClaims::project_id().expression(),
            ResourceClaims::environment_id().expression(),
            ResourceClaims::workload_id().expression(),
            ResourceClaims::deployment_id().expression(),
            ResourceClaims::replica_id().expression(),
            ResourceClaims::replica_generation().expression(),
            ResourceClaims::member_id().expression(),
            ResourceClaims::placement_generation().expression(),
            ResourceClaims::node_id().expression(),
            ResourceClaims::inventory_generation().expression(),
            ResourceClaims::inventory_digest().expression(),
            ResourceClaims::runtime_unit_id().expression(),
            ResourceClaims::runtime_generation().expression(),
            ResourceClaims::topology_digest().expression(),
            ResourceClaims::reservation_digest().expression(),
            ResourceClaims::claim_generation().expression(),
            ResourceClaims::claim_digest().expression(),
            ResourceClaims::state().expression(),
            ResourceClaims::prepare_command_id().expression(),
            ResourceClaims::prepared_binding_digest().expression(),
            ResourceClaims::release_command_id().expression(),
            ResourceClaims::release_evidence().expression(),
            ResourceClaims::failure().expression(),
            ResourceClaims::aggregate_version().expression(),
            ResourceClaims::created_at().expression(),
            ResourceClaims::updated_at().expression(),
            ResourceClaims::prepared_at().expression(),
            ResourceClaims::bound_at().expression(),
            ResourceClaims::release_requested_at().expression(),
            ResourceClaims::released_at().expression(),
            ResourceClaims::orphaned_at().expression(),
            ResourceClaimSlots::claim_id().expression(),
            ResourceClaimSlots::ordinal().expression(),
            ResourceClaimSlots::organization_id().expression(),
            ResourceClaimSlots::node_id().expression(),
            ResourceClaimSlots::resource_kind().expression(),
            ResourceClaimSlots::stable_resource_id().expression(),
            ResourceClaimSlots::allocation().expression(),
            ResourceClaimSlots::slot_generation().expression(),
            ResourceClaimSlots::fence_token().expression(),
            ResourceClaimSlots::created_at().expression(),
            ResourceClaimSlots::released_at().expression(),
        ]
    }
}

#[derive(Debug, Clone, PartialEq)]
struct StoredClaim {
    id: Uuid,
    organization_id: Uuid,
    project_id: Uuid,
    environment_id: Uuid,
    workload_id: Uuid,
    deployment_id: Uuid,
    replica_id: Uuid,
    replica_generation: u64,
    member_id: Uuid,
    placement_generation: u64,
    node_id: Uuid,
    inventory_generation: u64,
    inventory_digest: String,
    runtime_unit_id: String,
    runtime_generation: u64,
    topology_digest: String,
    reservation_digest: String,
    claim_generation: u64,
    claim_digest: String,
    state: String,
    prepare_command_id: Option<Uuid>,
    prepared_binding_digest: Option<String>,
    release_command_id: Option<Uuid>,
    release_evidence: Option<serde_json::Value>,
    failure: Option<String>,
    aggregate_version: u64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    prepared_at: Option<DateTime<Utc>>,
    bound_at: Option<DateTime<Utc>>,
    release_requested_at: Option<DateTime<Utc>>,
    released_at: Option<DateTime<Utc>>,
    orphaned_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
struct StoredSlot {
    claim_id: Uuid,
    ordinal: u32,
    organization_id: Uuid,
    node_id: Uuid,
    resource_kind: String,
    stable_resource_id: String,
    allocation: serde_json::Value,
    slot_generation: u64,
    fence_token: Uuid,
    created_at: DateTime<Utc>,
    released_at: Option<DateTime<Utc>>,
}

pub(super) struct ClaimWithSlotRow {
    claim: StoredClaim,
    slot: StoredSlot,
}

impl FromRow for ClaimWithSlotRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            claim: StoredClaim {
                id: decode(row, 0)?,
                organization_id: decode(row, 1)?,
                project_id: decode(row, 2)?,
                environment_id: decode(row, 3)?,
                workload_id: decode(row, 4)?,
                deployment_id: decode(row, 5)?,
                replica_id: decode(row, 6)?,
                replica_generation: decode(row, 7)?,
                member_id: decode(row, 8)?,
                placement_generation: decode(row, 9)?,
                node_id: decode(row, 10)?,
                inventory_generation: decode(row, 11)?,
                inventory_digest: decode(row, 12)?,
                runtime_unit_id: decode(row, 13)?,
                runtime_generation: decode(row, 14)?,
                topology_digest: decode(row, 15)?,
                reservation_digest: decode(row, 16)?,
                claim_generation: decode(row, 17)?,
                claim_digest: decode(row, 18)?,
                state: decode(row, 19)?,
                prepare_command_id: decode(row, 20)?,
                prepared_binding_digest: decode(row, 21)?,
                release_command_id: decode(row, 22)?,
                release_evidence: decode(row, 23)?,
                failure: decode(row, 24)?,
                aggregate_version: decode(row, 25)?,
                created_at: decode(row, 26)?,
                updated_at: decode(row, 27)?,
                prepared_at: decode(row, 28)?,
                bound_at: decode(row, 29)?,
                release_requested_at: decode(row, 30)?,
                released_at: decode(row, 31)?,
                orphaned_at: decode(row, 32)?,
            },
            slot: StoredSlot {
                claim_id: decode(row, 33)?,
                ordinal: decode(row, 34)?,
                organization_id: decode(row, 35)?,
                node_id: decode(row, 36)?,
                resource_kind: decode(row, 37)?,
                stable_resource_id: decode(row, 38)?,
                allocation: decode(row, 39)?,
                slot_generation: decode(row, 40)?,
                fence_token: decode(row, 41)?,
                created_at: decode(row, 42)?,
                released_at: decode(row, 43)?,
            },
        })
    }
}

pub(super) fn restore_claim(
    rows: Vec<ClaimWithSlotRow>,
) -> Result<Option<ResourceClaim>, RepositoryError> {
    let Some(first) = rows.first() else {
        return Ok(None);
    };
    let stored = first.claim.clone();
    let mut slots = Vec::with_capacity(rows.len());
    for (index, row) in rows.into_iter().enumerate() {
        if row.claim != stored
            || row.slot.claim_id != stored.id
            || row.slot.organization_id != stored.organization_id
            || row.slot.node_id != stored.node_id
            || usize::try_from(row.slot.ordinal).ok() != Some(index)
            || row.slot.created_at != stored.created_at
        {
            return Err(storage(
                "stored resource claim rows do not form one canonical aggregate",
            ));
        }
        if stored.state == ResourceClaimState::Released.as_str() {
            if row.slot.released_at != stored.released_at {
                return Err(storage(
                    "released resource claim slots have inconsistent release times",
                ));
            }
        } else if row.slot.released_at.is_some() {
            return Err(storage(
                "active resource claim has a prematurely released slot",
            ));
        }
        slots.push(ResourceSlotBinding {
            kind: ResourceKind::parse(&row.slot.resource_kind).map_err(RepositoryError::Storage)?,
            stable_resource_id: row.slot.stable_resource_id,
            allocation: serde_json::from_value::<ResourceAllocation>(row.slot.allocation)
                .map_err(|error| storage(error.to_string()))?,
            slot_generation: row.slot.slot_generation,
            fence_token: row.slot.fence_token,
        });
    }
    let release_evidence = stored
        .release_evidence
        .map(serde_json::from_value::<ResourceClaimReleaseEvidence>)
        .transpose()
        .map_err(|error| storage(error.to_string()))?;
    let claim = ResourceClaim {
        id: ResourceClaimId::from_uuid(stored.id),
        organization_id: OrganizationId::from_uuid(stored.organization_id),
        project_id: ProjectId::from_uuid(stored.project_id),
        environment_id: EnvironmentId::from_uuid(stored.environment_id),
        workload_id: WorkloadId::from_uuid(stored.workload_id),
        deployment_id: DeploymentId::from_uuid(stored.deployment_id),
        replica_id: WorkloadReplicaId::from_uuid(stored.replica_id),
        replica_generation: stored.replica_generation,
        member_id: WorkloadReplicaMemberId::from_uuid(stored.member_id),
        placement_generation: stored.placement_generation,
        node_id: NodeId::from_uuid(stored.node_id),
        inventory_generation: stored.inventory_generation,
        inventory_digest: stored.inventory_digest,
        runtime_unit_id: stored.runtime_unit_id,
        runtime_generation: stored.runtime_generation,
        topology_digest: stored.topology_digest,
        slots,
        reservation_digest: stored.reservation_digest,
        claim_generation: stored.claim_generation,
        claim_digest: stored.claim_digest,
        state: ResourceClaimState::parse(&stored.state).map_err(RepositoryError::Storage)?,
        prepare_command_id: stored.prepare_command_id.map(NodeCommandId::from_uuid),
        prepared_binding_digest: stored.prepared_binding_digest,
        release_command_id: stored.release_command_id.map(NodeCommandId::from_uuid),
        release_evidence,
        failure: stored.failure,
        aggregate_version: stored.aggregate_version,
        created_at: stored.created_at,
        updated_at: stored.updated_at,
        prepared_at: stored.prepared_at,
        bound_at: stored.bound_at,
        release_requested_at: stored.release_requested_at,
        released_at: stored.released_at,
        orphaned_at: stored.orphaned_at,
    };
    claim.validate().map_err(RepositoryError::Storage)?;
    Ok(Some(claim))
}

pub(super) struct SlotLeaseSelection;

impl Selection for SlotLeaseSelection {
    type Output = SlotLeaseRow;

    fn expressions(self) -> Vec<Expression> {
        vec![
            ResourceSlotLeases::organization_id().expression(),
            ResourceSlotLeases::node_id().expression(),
            ResourceSlotLeases::resource_kind().expression(),
            ResourceSlotLeases::stable_resource_id().expression(),
            ResourceSlotLeases::slot_generation().expression(),
            ResourceSlotLeases::fence_token().expression(),
            ResourceSlotLeases::active_claim_id().expression(),
            ResourceSlotLeases::updated_at().expression(),
        ]
    }
}

pub(super) struct SlotLeaseRow {
    pub organization_id: Uuid,
    pub node_id: Uuid,
    pub resource_kind: String,
    pub stable_resource_id: String,
    pub slot_generation: u64,
    pub fence_token: Uuid,
    pub active_claim_id: Option<Uuid>,
    pub updated_at: DateTime<Utc>,
}

impl FromRow for SlotLeaseRow {
    fn from_row(row: &impl Row) -> Result<Self, DecodeError> {
        Ok(Self {
            organization_id: decode(row, 0)?,
            node_id: decode(row, 1)?,
            resource_kind: decode(row, 2)?,
            stable_resource_id: decode(row, 3)?,
            slot_generation: decode(row, 4)?,
            fence_token: decode(row, 5)?,
            active_claim_id: decode(row, 6)?,
            updated_at: decode(row, 7)?,
        })
    }
}

fn decode<T: FromValue>(row: &impl Row, index: usize) -> Result<T, DecodeError> {
    T::from_value(
        row.value(index)
            .ok_or(DecodeError::MissingColumn { index })?,
        index,
    )
}

fn storage(error: impl Into<String>) -> RepositoryError {
    RepositoryError::Storage(error.into())
}
