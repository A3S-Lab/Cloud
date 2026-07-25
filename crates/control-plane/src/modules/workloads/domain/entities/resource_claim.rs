use super::resource_allocation::{
    validate_slot_bindings, validate_slot_evidence, validate_slot_requests, ResourceSlotBinding,
    ResourceSlotEvidence, ResourceSlotRequest,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DeploymentId, EnvironmentId, NodeCommandId, NodeId, OrganizationId,
    ProjectId, ResourceClaimId, WorkloadId, WorkloadReplicaId, WorkloadReplicaMemberId,
};
use crate::modules::workloads::domain::entities::DeploymentReplicaBinding;
use a3s_cloud_contracts::{NodeResourceClaimBinding, NodeResourceInventory};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceClaimState {
    ReservedInDb,
    PreparingOnAgent,
    PreparedOnAgent,
    BoundToRuntimeUnit,
    Releasing,
    Released,
    Orphaned,
}

impl ResourceClaimState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReservedInDb => "reserved_in_db",
            Self::PreparingOnAgent => "preparing_on_agent",
            Self::PreparedOnAgent => "prepared_on_agent",
            Self::BoundToRuntimeUnit => "bound_to_runtime_unit",
            Self::Releasing => "releasing",
            Self::Released => "released",
            Self::Orphaned => "orphaned",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "reserved_in_db" => Ok(Self::ReservedInDb),
            "preparing_on_agent" => Ok(Self::PreparingOnAgent),
            "prepared_on_agent" => Ok(Self::PreparedOnAgent),
            "bound_to_runtime_unit" => Ok(Self::BoundToRuntimeUnit),
            "releasing" => Ok(Self::Releasing),
            "released" => Ok(Self::Released),
            "orphaned" => Ok(Self::Orphaned),
            _ => Err(format!("unsupported hard resource claim state {value:?}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceClaimBindingEvidence {
    pub runtime_unit_id: String,
    pub runtime_generation: u64,
    pub binding_digest: String,
    pub slots: Vec<ResourceSlotEvidence>,
    pub observed_at: DateTime<Utc>,
}

impl ResourceClaimBindingEvidence {
    pub fn validate(&self) -> Result<(), String> {
        validate_runtime_identity(&self.runtime_unit_id, self.runtime_generation)?;
        validate_sha256(&self.binding_digest, "Runtime resource binding digest")?;
        validate_slot_evidence(&self.slots)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResourceClaimReleaseEvidence {
    DatabaseReservationCancelled {
        reservation_digest: String,
        slots: Vec<ResourceSlotEvidence>,
        observed_at: DateTime<Utc>,
    },
    AgentReleased {
        command_id: NodeCommandId,
        slots: Vec<ResourceSlotEvidence>,
        evidence_digest: String,
        observed_at: DateTime<Utc>,
    },
    ProviderNotFound {
        slots: Vec<ResourceSlotEvidence>,
        evidence_digest: String,
        observed_at: DateTime<Utc>,
    },
    ComputeFenced {
        instance_generation: u64,
        slots: Vec<ResourceSlotEvidence>,
        evidence_digest: String,
        observed_at: DateTime<Utc>,
    },
}

impl ResourceClaimReleaseEvidence {
    pub fn validate(&self) -> Result<(), String> {
        if let Self::DatabaseReservationCancelled {
            reservation_digest,
            slots,
            observed_at,
        } = self
        {
            validate_sha256(reservation_digest, "cancelled database reservation digest")?;
            validate_slot_evidence(slots)?;
            if observed_at.timestamp_millis() <= 0 {
                return Err("resource release evidence time is invalid".into());
            }
            return Ok(());
        }
        let (slots, evidence_digest, observed_at) = match self {
            Self::DatabaseReservationCancelled { .. } => unreachable!(),
            Self::AgentReleased {
                command_id,
                slots,
                evidence_digest,
                observed_at,
            } => {
                if command_id.as_uuid().is_nil() {
                    return Err("resource release command identity is invalid".into());
                }
                (slots, evidence_digest, observed_at)
            }
            Self::ProviderNotFound {
                slots,
                evidence_digest,
                observed_at,
            } => (slots, evidence_digest, observed_at),
            Self::ComputeFenced {
                instance_generation,
                slots,
                evidence_digest,
                observed_at,
            } => {
                if *instance_generation == 0 {
                    return Err("compute fencing generation must be positive".into());
                }
                (slots, evidence_digest, observed_at)
            }
        };
        validate_slot_evidence(slots)?;
        validate_sha256(evidence_digest, "resource release evidence digest")?;
        if observed_at.timestamp_millis() <= 0 {
            return Err("resource release evidence time is invalid".into());
        }
        Ok(())
    }

    pub fn slots(&self) -> &[ResourceSlotEvidence] {
        match self {
            Self::DatabaseReservationCancelled { slots, .. }
            | Self::AgentReleased { slots, .. }
            | Self::ProviderNotFound { slots, .. }
            | Self::ComputeFenced { slots, .. } => slots,
        }
    }

    pub const fn observed_at(&self) -> DateTime<Utc> {
        match self {
            Self::DatabaseReservationCancelled { observed_at, .. }
            | Self::AgentReleased { observed_at, .. }
            | Self::ProviderNotFound { observed_at, .. }
            | Self::ComputeFenced { observed_at, .. } => *observed_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceClaimReservation {
    pub id: ResourceClaimId,
    pub binding: DeploymentReplicaBinding,
    pub node_id: NodeId,
    pub inventory: NodeResourceInventory,
    pub topology_digest: String,
    pub slots: Vec<ResourceSlotRequest>,
    pub reserved_at: DateTime<Utc>,
}

impl ResourceClaimReservation {
    pub fn validate(&self) -> Result<(), String> {
        self.inventory.validate()?;
        if self.id.as_uuid().is_nil()
            || self.binding.organization_id.as_uuid().is_nil()
            || self.binding.project_id.as_uuid().is_nil()
            || self.binding.environment_id.as_uuid().is_nil()
            || self.binding.workload_id.as_uuid().is_nil()
            || self.binding.deployment_id.as_uuid().is_nil()
            || self.binding.revision_id.as_uuid().is_nil()
            || self.binding.replica_id.as_uuid().is_nil()
            || self.binding.replica_generation == 0
            || self.binding.member_id.as_uuid().is_nil()
            || self.binding.node_id != Some(self.node_id)
            || self.binding.placement_generation == 0
            || self.binding.runtime_generation != self.binding.replica_generation
            || self.inventory.node_id != self.node_id.as_uuid()
            || canonical_timestamp(self.reserved_at) < self.binding.updated_at
        {
            return Err("resource claim reservation identity or generation is invalid".into());
        }
        validate_sha256(&self.topology_digest, "resource topology digest")?;
        validate_runtime_identity(
            &self.binding.runtime_unit_id,
            self.binding.runtime_generation,
        )?;
        validate_slot_requests(&self.slots)?;
        for request in &self.slots {
            let inventory_slot = self
                .inventory
                .slots
                .iter()
                .find(|slot| {
                    slot.kind == request.kind
                        && slot.stable_resource_id == request.stable_resource_id
                })
                .ok_or_else(|| {
                    format!(
                        "resource request {} is absent from the bound node inventory",
                        request.stable_resource_id
                    )
                })?;
            if !inventory_slot.allocation.contains(&request.allocation) {
                return Err(format!(
                    "resource request {} exceeds the bound node inventory allocation",
                    request.stable_resource_id
                ));
            }
        }
        Ok(())
    }

    pub fn matches(&self, claim: &ResourceClaim) -> bool {
        self.id == claim.id
            && self.binding.organization_id == claim.organization_id
            && self.binding.project_id == claim.project_id
            && self.binding.environment_id == claim.environment_id
            && self.binding.workload_id == claim.workload_id
            && self.binding.deployment_id == claim.deployment_id
            && self.binding.replica_id == claim.replica_id
            && self.binding.replica_generation == claim.replica_generation
            && self.binding.member_id == claim.member_id
            && self.binding.placement_generation == claim.placement_generation
            && self.node_id == claim.node_id
            && self.inventory.generation == claim.inventory_generation
            && self.inventory.digest == claim.inventory_digest
            && self.binding.runtime_unit_id == claim.runtime_unit_id
            && self.binding.runtime_generation == claim.runtime_generation
            && self.topology_digest == claim.topology_digest
            && slot_requests_match_bindings(&self.slots, &claim.slots)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceClaim {
    pub id: ResourceClaimId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub workload_id: WorkloadId,
    pub deployment_id: DeploymentId,
    pub replica_id: WorkloadReplicaId,
    pub replica_generation: u64,
    pub member_id: WorkloadReplicaMemberId,
    pub placement_generation: u64,
    pub node_id: NodeId,
    pub inventory_generation: u64,
    pub inventory_digest: String,
    pub runtime_unit_id: String,
    pub runtime_generation: u64,
    pub topology_digest: String,
    pub slots: Vec<ResourceSlotBinding>,
    pub reservation_digest: String,
    pub claim_generation: u64,
    pub claim_digest: String,
    pub state: ResourceClaimState,
    pub prepare_command_id: Option<NodeCommandId>,
    pub prepared_binding_digest: Option<String>,
    pub release_command_id: Option<NodeCommandId>,
    pub release_evidence: Option<ResourceClaimReleaseEvidence>,
    pub failure: Option<String>,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub prepared_at: Option<DateTime<Utc>>,
    pub bound_at: Option<DateTime<Utc>>,
    pub release_requested_at: Option<DateTime<Utc>>,
    pub released_at: Option<DateTime<Utc>>,
    pub orphaned_at: Option<DateTime<Utc>>,
}

impl ResourceClaim {
    pub fn reserve(
        reservation: &ResourceClaimReservation,
        slots: Vec<ResourceSlotBinding>,
    ) -> Result<Self, String> {
        reservation.validate()?;
        if !slot_requests_match_bindings(&reservation.slots, &slots) {
            return Err("reserved resource slots do not match the canonical request".into());
        }
        validate_slot_bindings(&slots)?;
        let binding = &reservation.binding;
        let reserved_at = canonical_timestamp(reservation.reserved_at);
        let mut claim = Self {
            id: reservation.id,
            organization_id: binding.organization_id,
            project_id: binding.project_id,
            environment_id: binding.environment_id,
            workload_id: binding.workload_id,
            deployment_id: binding.deployment_id,
            replica_id: binding.replica_id,
            replica_generation: binding.replica_generation,
            member_id: binding.member_id,
            placement_generation: binding.placement_generation,
            node_id: reservation.node_id,
            inventory_generation: reservation.inventory.generation,
            inventory_digest: reservation.inventory.digest.clone(),
            runtime_unit_id: binding.runtime_unit_id.clone(),
            runtime_generation: binding.runtime_generation,
            topology_digest: reservation.topology_digest.clone(),
            slots,
            reservation_digest: String::new(),
            claim_generation: 1,
            claim_digest: String::new(),
            state: ResourceClaimState::ReservedInDb,
            prepare_command_id: None,
            prepared_binding_digest: None,
            release_command_id: None,
            release_evidence: None,
            failure: None,
            aggregate_version: 1,
            created_at: reserved_at,
            updated_at: reserved_at,
            prepared_at: None,
            bound_at: None,
            release_requested_at: None,
            released_at: None,
            orphaned_at: None,
        };
        claim.reservation_digest = claim.calculate_reservation_digest()?;
        claim.claim_digest = claim.calculate_claim_digest()?;
        claim.validate()?;
        Ok(claim)
    }

    pub fn begin_preparation(
        &mut self,
        command_id: NodeCommandId,
        at: DateTime<Utc>,
    ) -> Result<(), String> {
        let at = self.canonical_time(at)?;
        if self.state == ResourceClaimState::PreparingOnAgent {
            return if self.prepare_command_id == Some(command_id) {
                Ok(())
            } else {
                Err("resource claim preparation command cannot change".into())
            };
        }
        if self.state != ResourceClaimState::ReservedInDb || command_id.as_uuid().is_nil() {
            return Err("resource claim cannot begin agent preparation".into());
        }
        self.state = ResourceClaimState::PreparingOnAgent;
        self.prepare_command_id = Some(command_id);
        self.bump(at)?;
        Ok(())
    }

    pub fn record_prepared(
        &mut self,
        command_id: NodeCommandId,
        binding_digest: String,
        at: DateTime<Utc>,
    ) -> Result<(), String> {
        let at = self.canonical_time(at)?;
        validate_sha256(&binding_digest, "prepared resource binding digest")?;
        if self.state == ResourceClaimState::PreparedOnAgent {
            return if self.prepare_command_id == Some(command_id)
                && self.prepared_binding_digest.as_ref() == Some(&binding_digest)
            {
                Ok(())
            } else {
                Err("prepared resource claim evidence cannot change".into())
            };
        }
        if self.state != ResourceClaimState::PreparingOnAgent
            || self.prepare_command_id != Some(command_id)
        {
            return Err("resource claim preparation acknowledgement is stale".into());
        }
        self.state = ResourceClaimState::PreparedOnAgent;
        self.prepared_binding_digest = Some(binding_digest);
        self.prepared_at = Some(at);
        self.bump(at)?;
        Ok(())
    }

    pub fn bind(
        &mut self,
        evidence: ResourceClaimBindingEvidence,
        at: DateTime<Utc>,
    ) -> Result<(), String> {
        let at = self.canonical_time(at)?;
        evidence.validate()?;
        if self.state == ResourceClaimState::BoundToRuntimeUnit {
            return if self.binding_evidence_matches(&evidence) {
                Ok(())
            } else {
                Err("bound resource claim evidence cannot change".into())
            };
        }
        if self.state != ResourceClaimState::PreparedOnAgent
            || !self.binding_evidence_matches(&evidence)
            || evidence.observed_at > at
        {
            return Err(
                "Runtime resource binding evidence does not match the prepared claim".into(),
            );
        }
        self.state = ResourceClaimState::BoundToRuntimeUnit;
        self.bound_at = Some(canonical_timestamp(evidence.observed_at));
        self.bump(at)?;
        Ok(())
    }

    pub fn begin_release(
        &mut self,
        command_id: NodeCommandId,
        at: DateTime<Utc>,
    ) -> Result<(), String> {
        let at = self.canonical_time(at)?;
        if self.state == ResourceClaimState::Releasing {
            return if self.release_command_id == Some(command_id) {
                Ok(())
            } else {
                Err("resource claim release command cannot change".into())
            };
        }
        if !matches!(
            self.state,
            ResourceClaimState::PreparedOnAgent
                | ResourceClaimState::BoundToRuntimeUnit
                | ResourceClaimState::Orphaned
        ) || command_id.as_uuid().is_nil()
        {
            return Err("resource claim cannot begin release".into());
        }
        self.claim_generation = self
            .claim_generation
            .checked_add(1)
            .ok_or_else(|| "resource claim generation overflowed".to_string())?;
        self.claim_digest = self.calculate_claim_digest()?;
        self.state = ResourceClaimState::Releasing;
        self.release_command_id = Some(command_id);
        self.release_requested_at = Some(at);
        self.bump(at)?;
        Ok(())
    }

    pub fn record_released(
        &mut self,
        evidence: ResourceClaimReleaseEvidence,
        at: DateTime<Utc>,
    ) -> Result<(), String> {
        let at = self.canonical_time(at)?;
        evidence.validate()?;
        if self.state == ResourceClaimState::Released {
            return if self.release_evidence.as_ref() == Some(&evidence) {
                Ok(())
            } else {
                Err("released resource claim evidence cannot change".into())
            };
        }
        let database_cancellation_matches = matches!(
            &evidence,
            ResourceClaimReleaseEvidence::DatabaseReservationCancelled {
                reservation_digest,
                ..
            } if self.state == ResourceClaimState::ReservedInDb
                && reservation_digest == &self.reservation_digest
        );
        let issued_release_matches = matches!(
            self.state,
            ResourceClaimState::Releasing | ResourceClaimState::Orphaned
        ) && !matches!(
            evidence,
            ResourceClaimReleaseEvidence::DatabaseReservationCancelled { .. }
        );
        if (!database_cancellation_matches && !issued_release_matches)
            || evidence.observed_at() > at
            || !self.slot_evidence_matches(evidence.slots())
        {
            return Err("resource claim release evidence is stale or incomplete".into());
        }
        if let ResourceClaimReleaseEvidence::AgentReleased { command_id, .. } = &evidence {
            if self.state != ResourceClaimState::Releasing
                || self.release_command_id != Some(*command_id)
            {
                return Err("agent release acknowledgement has the wrong command identity".into());
            }
        }
        self.state = ResourceClaimState::Released;
        self.released_at = Some(canonical_timestamp(evidence.observed_at()));
        self.release_evidence = Some(evidence);
        self.failure = None;
        self.bump(at)?;
        Ok(())
    }

    pub fn cancel_database_reservation(&mut self, at: DateTime<Utc>) -> Result<(), String> {
        self.record_released(
            ResourceClaimReleaseEvidence::DatabaseReservationCancelled {
                reservation_digest: self.reservation_digest.clone(),
                slots: self.slot_evidence(),
                observed_at: canonical_timestamp(at),
            },
            at,
        )
    }

    pub fn orphan(&mut self, failure: String, at: DateTime<Utc>) -> Result<(), String> {
        let at = self.canonical_time(at)?;
        validate_failure(&failure)?;
        if self.state == ResourceClaimState::Orphaned {
            return if self.failure.as_ref() == Some(&failure) {
                Ok(())
            } else {
                Err("orphaned resource claim failure cannot change".into())
            };
        }
        if self.state == ResourceClaimState::Released {
            return Err("released resource claim cannot become orphaned".into());
        }
        self.state = ResourceClaimState::Orphaned;
        self.failure = Some(failure);
        self.orphaned_at = Some(at);
        self.bump(at)?;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.id.as_uuid().is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.workload_id.as_uuid().is_nil()
            || self.deployment_id.as_uuid().is_nil()
            || self.replica_id.as_uuid().is_nil()
            || self.replica_generation == 0
            || self.member_id.as_uuid().is_nil()
            || self.placement_generation == 0
            || self.node_id.as_uuid().is_nil()
            || self.inventory_generation == 0
            || self.runtime_generation != self.replica_generation
            || self.claim_generation == 0
            || self.aggregate_version == 0
            || self.updated_at < self.created_at
        {
            return Err("resource claim identity, generation, or timestamps are invalid".into());
        }
        validate_sha256(&self.inventory_digest, "resource inventory digest")?;
        validate_sha256(&self.topology_digest, "resource topology digest")?;
        validate_sha256(&self.reservation_digest, "resource reservation digest")?;
        validate_sha256(&self.claim_digest, "resource claim digest")?;
        validate_runtime_identity(&self.runtime_unit_id, self.runtime_generation)?;
        validate_slot_bindings(&self.slots)?;
        if self.calculate_reservation_digest()? != self.reservation_digest
            || self.calculate_claim_digest()? != self.claim_digest
        {
            return Err("resource claim digest does not match its exact binding".into());
        }
        if let Some(digest) = &self.prepared_binding_digest {
            validate_sha256(digest, "prepared resource binding digest")?;
        }
        if let Some(failure) = &self.failure {
            validate_failure(failure)?;
        }
        if let Some(evidence) = &self.release_evidence {
            evidence.validate()?;
            if !self.slot_evidence_matches(evidence.slots()) {
                return Err("stored resource release evidence has the wrong slots".into());
            }
        }
        let state_valid = match self.state {
            ResourceClaimState::ReservedInDb => {
                self.prepare_command_id.is_none()
                    && self.prepared_binding_digest.is_none()
                    && self.prepared_at.is_none()
                    && self.bound_at.is_none()
                    && self.release_command_id.is_none()
                    && self.release_requested_at.is_none()
                    && self.release_evidence.is_none()
                    && self.released_at.is_none()
                    && self.failure.is_none()
                    && self.orphaned_at.is_none()
            }
            ResourceClaimState::PreparingOnAgent => {
                self.prepare_command_id.is_some()
                    && self.prepared_binding_digest.is_none()
                    && self.prepared_at.is_none()
                    && self.bound_at.is_none()
                    && self.release_command_id.is_none()
                    && self.release_evidence.is_none()
                    && self.failure.is_none()
            }
            ResourceClaimState::PreparedOnAgent => {
                self.prepare_command_id.is_some()
                    && self.prepared_binding_digest.is_some()
                    && self.prepared_at.is_some()
                    && self.bound_at.is_none()
                    && self.release_command_id.is_none()
                    && self.release_evidence.is_none()
                    && self.failure.is_none()
            }
            ResourceClaimState::BoundToRuntimeUnit => {
                self.prepare_command_id.is_some()
                    && self.prepared_binding_digest.is_some()
                    && self.prepared_at.is_some()
                    && self.bound_at.is_some()
                    && self.release_command_id.is_none()
                    && self.release_evidence.is_none()
                    && self.failure.is_none()
            }
            ResourceClaimState::Releasing => {
                self.release_command_id.is_some()
                    && self.release_requested_at.is_some()
                    && self.release_evidence.is_none()
                    && self.released_at.is_none()
            }
            ResourceClaimState::Released => {
                self.release_evidence.is_some()
                    && self.released_at.is_some()
                    && self.failure.is_none()
            }
            ResourceClaimState::Orphaned => {
                self.failure.is_some()
                    && self.orphaned_at.is_some()
                    && self.release_evidence.is_none()
                    && self.released_at.is_none()
            }
        };
        if !state_valid {
            return Err("resource claim state does not match its durable evidence".into());
        }
        Ok(())
    }

    pub fn slot_evidence(&self) -> Vec<ResourceSlotEvidence> {
        self.slots
            .iter()
            .map(ResourceSlotBinding::evidence)
            .collect()
    }

    pub fn node_binding(
        &self,
        agent_instance_id: uuid::Uuid,
    ) -> Result<NodeResourceClaimBinding, String> {
        self.validate()?;
        let binding = NodeResourceClaimBinding {
            schema: NodeResourceClaimBinding::SCHEMA.into(),
            claim_id: self.id.as_uuid(),
            node_id: self.node_id.as_uuid(),
            agent_instance_id,
            inventory_generation: self.inventory_generation,
            inventory_digest: self.inventory_digest.clone(),
            runtime_unit_id: self.runtime_unit_id.clone(),
            runtime_generation: self.runtime_generation,
            topology_digest: self.topology_digest.clone(),
            slots: self.slots.clone(),
        };
        binding.validate()?;
        Ok(binding)
    }

    fn slot_evidence_matches(&self, evidence: &[ResourceSlotEvidence]) -> bool {
        let expected = self.slot_evidence();
        evidence == expected.as_slice()
    }

    fn binding_evidence_matches(&self, evidence: &ResourceClaimBindingEvidence) -> bool {
        evidence.runtime_unit_id == self.runtime_unit_id
            && evidence.runtime_generation == self.runtime_generation
            && self.prepared_binding_digest.as_ref() == Some(&evidence.binding_digest)
            && evidence.slots == self.slot_evidence()
    }

    fn calculate_reservation_digest(&self) -> Result<String, String> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Slot<'a> {
            kind: super::resource_allocation::ResourceKind,
            stable_resource_id: &'a str,
            allocation: &'a super::resource_allocation::ResourceAllocation,
        }
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Document<'a> {
            claim_id: ResourceClaimId,
            organization_id: OrganizationId,
            project_id: ProjectId,
            environment_id: EnvironmentId,
            workload_id: WorkloadId,
            deployment_id: DeploymentId,
            replica_id: WorkloadReplicaId,
            replica_generation: u64,
            member_id: WorkloadReplicaMemberId,
            placement_generation: u64,
            node_id: NodeId,
            inventory_generation: u64,
            inventory_digest: &'a str,
            runtime_unit_id: &'a str,
            runtime_generation: u64,
            topology_digest: &'a str,
            slots: Vec<Slot<'a>>,
        }
        digest(&Document {
            claim_id: self.id,
            organization_id: self.organization_id,
            project_id: self.project_id,
            environment_id: self.environment_id,
            workload_id: self.workload_id,
            deployment_id: self.deployment_id,
            replica_id: self.replica_id,
            replica_generation: self.replica_generation,
            member_id: self.member_id,
            placement_generation: self.placement_generation,
            node_id: self.node_id,
            inventory_generation: self.inventory_generation,
            inventory_digest: &self.inventory_digest,
            runtime_unit_id: &self.runtime_unit_id,
            runtime_generation: self.runtime_generation,
            topology_digest: &self.topology_digest,
            slots: self
                .slots
                .iter()
                .map(|slot| Slot {
                    kind: slot.kind,
                    stable_resource_id: &slot.stable_resource_id,
                    allocation: &slot.allocation,
                })
                .collect(),
        })
    }

    fn calculate_claim_digest(&self) -> Result<String, String> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Document<'a> {
            reservation_digest: &'a str,
            claim_generation: u64,
            slots: &'a [ResourceSlotBinding],
        }
        digest(&Document {
            reservation_digest: &self.reservation_digest,
            claim_generation: self.claim_generation,
            slots: &self.slots,
        })
    }

    fn canonical_time(&self, at: DateTime<Utc>) -> Result<DateTime<Utc>, String> {
        let at = canonical_timestamp(at);
        if at < self.updated_at {
            return Err("resource claim update time regressed".into());
        }
        Ok(at)
    }

    fn bump(&mut self, at: DateTime<Utc>) -> Result<(), String> {
        self.aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "resource claim version overflowed".to_string())?;
        self.updated_at = at;
        Ok(())
    }
}

fn digest(value: &impl Serialize) -> Result<String, String> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| format!("could not encode resource claim digest input: {error}"))?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn validate_runtime_identity(unit_id: &str, generation: u64) -> Result<(), String> {
    if unit_id.trim().is_empty()
        || unit_id.len() > 512
        || unit_id.contains(['\0', '\r', '\n'])
        || generation == 0
    {
        return Err("resource claim Runtime identity is invalid".into());
    }
    Ok(())
}

fn validate_sha256(value: &str, label: &str) -> Result<(), String> {
    if !value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }) {
        return Err(format!("{label} is invalid"));
    }
    Ok(())
}

fn validate_failure(value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 16 * 1024 || value.contains(['\0', '\r', '\n']) {
        return Err("resource claim failure is invalid".into());
    }
    Ok(())
}

fn slot_requests_match_bindings(
    requests: &[ResourceSlotRequest],
    bindings: &[ResourceSlotBinding],
) -> bool {
    requests.len() == bindings.len()
        && requests.iter().zip(bindings).all(|(request, binding)| {
            request.kind == binding.kind
                && request.stable_resource_id == binding.stable_resource_id
                && request.allocation == binding.allocation
        })
}
