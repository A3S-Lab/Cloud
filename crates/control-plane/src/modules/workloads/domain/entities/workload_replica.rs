use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DeploymentId, EnvironmentId, NodeCommandId, NodeId, OrganizationId,
    ProjectId, ResourceClaimId, WorkloadId, WorkloadReplicaId, WorkloadReplicaMemberId,
    WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    Deployment, DeploymentStatus, Workload, WorkloadPlacementGroupMemberPlan, WorkloadRevision,
    MAX_WORKLOAD_PLACEMENT_GROUP_MEMBERS, MAX_WORKLOAD_REPLICAS,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const CANONICAL_REPLICA_ORDINAL: u32 = 0;
const REPLICA_ID_DOMAIN: &str = "a3s.cloud.workload-replica.v1";
const REPLICA_MEMBER_ID_DOMAIN: &str = "a3s.cloud.workload-replica-member.v1";
const PLACEMENT_GROUP_CLAIM_ID_DOMAIN: &str = "a3s.cloud.placement-group-resource-claim.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadReplicaLifecycle {
    Desired,
    Retiring,
    Retired,
}

impl WorkloadReplicaLifecycle {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Desired => "desired",
            Self::Retiring => "retiring",
            Self::Retired => "retired",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "desired" => Ok(Self::Desired),
            "retiring" => Ok(Self::Retiring),
            "retired" => Ok(Self::Retired),
            _ => Err(format!("unsupported Workload replica lifecycle {value:?}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkloadReplica {
    pub id: WorkloadReplicaId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub workload_id: WorkloadId,
    pub ordinal: u32,
    pub revision_id: WorkloadRevisionId,
    pub revision_generation: u64,
    pub generation: u64,
    pub lifecycle: WorkloadReplicaLifecycle,
    pub evacuation_node_id: Option<NodeId>,
    pub retirement_command_id: Option<NodeCommandId>,
    pub runtime_fenced_at: Option<DateTime<Utc>>,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl WorkloadReplica {
    pub fn canonical(workload: &Workload, revision: &WorkloadRevision) -> Result<Self, String> {
        Self::for_ordinal(workload, revision, CANONICAL_REPLICA_ORDINAL)
    }

    pub fn for_ordinal(
        workload: &Workload,
        revision: &WorkloadRevision,
        ordinal: u32,
    ) -> Result<Self, String> {
        if revision.workload_id != workload.id || revision.generation == 0 {
            return Err("Workload replica revision is inconsistent".into());
        }
        let replica = Self {
            id: Self::deterministic_id(workload.id, ordinal)?,
            organization_id: workload.organization_id,
            project_id: workload.project_id,
            environment_id: workload.environment_id,
            workload_id: workload.id,
            ordinal,
            revision_id: revision.id,
            revision_generation: revision.generation,
            generation: revision.generation,
            lifecycle: WorkloadReplicaLifecycle::Desired,
            evacuation_node_id: None,
            retirement_command_id: None,
            runtime_fenced_at: None,
            aggregate_version: 1,
            created_at: workload.created_at,
            updated_at: revision.created_at.max(workload.created_at),
        };
        replica.validate()?;
        Ok(replica)
    }

    pub fn deterministic_id(
        workload_id: WorkloadId,
        ordinal: u32,
    ) -> Result<WorkloadReplicaId, String> {
        if ordinal >= MAX_WORKLOAD_REPLICAS {
            return Err(format!(
                "Workload replica ordinal must be smaller than {MAX_WORKLOAD_REPLICAS}"
            ));
        }
        if ordinal == CANONICAL_REPLICA_ORDINAL {
            return Ok(WorkloadReplicaId::from_uuid(workload_id.as_uuid()));
        }
        let name = format!("{REPLICA_ID_DOMAIN}:{ordinal}");
        Ok(WorkloadReplicaId::from_uuid(Uuid::new_v5(
            &workload_id.as_uuid(),
            name.as_bytes(),
        )))
    }

    pub fn runtime_unit_id(&self, revision: &WorkloadRevision) -> Result<String, String> {
        if revision.workload_id != self.workload_id
            || revision.id != self.revision_id
            || revision.generation != self.revision_generation
        {
            return Err("Workload replica Runtime identity has the wrong revision".into());
        }
        if self.ordinal == CANONICAL_REPLICA_ORDINAL {
            return Ok(revision.runtime_unit_id());
        }
        Ok(format!(
            "workload:{}:replica:{}:revision:{}",
            self.workload_id, self.id, revision.id
        ))
    }

    pub fn runtime_unit_id_for_member(
        &self,
        revision: &WorkloadRevision,
        member: &WorkloadReplicaMember,
    ) -> Result<String, String> {
        member.validate()?;
        if member.organization_id != self.organization_id
            || member.project_id != self.project_id
            || member.environment_id != self.environment_id
            || member.replica_id != self.id
            || member.workload_id != self.workload_id
            || WorkloadReplicaMember::deterministic_id(self.id, member.ordinal).ok()
                != Some(member.id)
        {
            return Err("Workload Runtime member identity is inconsistent".into());
        }
        let base = self.runtime_unit_id(revision)?;
        if member.ordinal == CANONICAL_REPLICA_ORDINAL {
            return Ok(base);
        }
        Ok(format!("{base}:member:{}", member.id))
    }

    pub fn advance(
        &mut self,
        revision: &WorkloadRevision,
        at: DateTime<Utc>,
    ) -> Result<(), String> {
        let at = canonical_timestamp(at);
        if revision.workload_id != self.workload_id
            || revision.generation <= self.revision_generation
            || self.lifecycle != WorkloadReplicaLifecycle::Desired
            || at < self.updated_at
        {
            return Err("Workload replica generation advance is invalid".into());
        }
        self.revision_id = revision.id;
        self.revision_generation = revision.generation;
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| "Workload replica generation overflowed".to_string())?;
        self.aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Workload replica version overflowed".to_string())?;
        self.updated_at = at;
        Ok(())
    }

    pub fn request_retirement(&mut self, at: DateTime<Utc>) -> Result<(), String> {
        let at = canonical_timestamp(at);
        if at < self.updated_at {
            return Err("Workload replica retirement time regressed".into());
        }
        if self.lifecycle != WorkloadReplicaLifecycle::Desired {
            self.updated_at = at;
            return Ok(());
        }
        self.lifecycle = WorkloadReplicaLifecycle::Retiring;
        self.evacuation_node_id = None;
        self.retirement_command_id = None;
        self.runtime_fenced_at = None;
        self.aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Workload replica version overflowed".to_string())?;
        self.updated_at = at;
        Ok(())
    }

    pub fn request_evacuation(
        &mut self,
        member: &WorkloadReplicaMember,
        node_id: NodeId,
        at: DateTime<Utc>,
    ) -> Result<(), String> {
        let at = canonical_timestamp(at);
        if node_id.as_uuid().is_nil()
            || member.replica_id != self.id
            || member.workload_id != self.workload_id
            || member.node_id != Some(node_id)
            || at < self.updated_at.max(member.updated_at)
        {
            return Err("Workload replica evacuation request is invalid".into());
        }
        if self.lifecycle == WorkloadReplicaLifecycle::Retiring
            && self.evacuation_node_id == Some(node_id)
        {
            return Ok(());
        }
        if self.lifecycle != WorkloadReplicaLifecycle::Desired {
            return Err("Workload replica is not eligible for evacuation".into());
        }
        self.lifecycle = WorkloadReplicaLifecycle::Retiring;
        self.evacuation_node_id = Some(node_id);
        self.retirement_command_id = None;
        self.runtime_fenced_at = None;
        self.aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Workload replica version overflowed".to_string())?;
        self.updated_at = at;
        Ok(())
    }

    pub fn dispatch_retirement(
        &mut self,
        command_id: NodeCommandId,
        at: DateTime<Utc>,
    ) -> Result<(), String> {
        let at = canonical_timestamp(at);
        if command_id.as_uuid().is_nil()
            || self.lifecycle != WorkloadReplicaLifecycle::Retiring
            || self.runtime_fenced_at.is_some()
            || at < self.updated_at
        {
            return Err("Workload replica retirement dispatch is invalid".into());
        }
        if self.retirement_command_id == Some(command_id) {
            return Ok(());
        }
        self.retirement_command_id = Some(command_id);
        self.aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Workload replica version overflowed".to_string())?;
        self.updated_at = at;
        Ok(())
    }

    pub fn record_runtime_fenced(
        &mut self,
        command_id: NodeCommandId,
        fenced_at: DateTime<Utc>,
    ) -> Result<(), String> {
        let fenced_at = canonical_timestamp(fenced_at);
        if self.lifecycle != WorkloadReplicaLifecycle::Retiring
            || self.retirement_command_id != Some(command_id)
            || fenced_at < self.updated_at
        {
            return Err("Workload replica Runtime fencing evidence is invalid".into());
        }
        if self.runtime_fenced_at.is_some() {
            return if self.runtime_fenced_at == Some(fenced_at) {
                Ok(())
            } else {
                Err("Workload replica Runtime fencing evidence cannot change".into())
            };
        }
        self.runtime_fenced_at = Some(fenced_at);
        self.aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Workload replica version overflowed".to_string())?;
        self.updated_at = fenced_at;
        Ok(())
    }

    pub fn complete_retirement(
        &mut self,
        member: &WorkloadReplicaMember,
        at: DateTime<Utc>,
    ) -> Result<(), String> {
        let at = canonical_timestamp(at);
        if member.replica_id != self.id
            || member.workload_id != self.workload_id
            || member.node_id.is_some()
            || self.lifecycle != WorkloadReplicaLifecycle::Retiring
            || self.evacuation_node_id.is_some()
            || self.retirement_command_id.is_some() != self.runtime_fenced_at.is_some()
            || at < self.updated_at.max(member.updated_at)
        {
            return Err("Workload replica cannot retire before its member is released".into());
        }
        self.lifecycle = WorkloadReplicaLifecycle::Retired;
        self.aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Workload replica version overflowed".to_string())?;
        self.updated_at = at;
        Ok(())
    }

    pub fn complete_evacuation(
        &mut self,
        member: &WorkloadReplicaMember,
        at: DateTime<Utc>,
    ) -> Result<(), String> {
        let at = canonical_timestamp(at);
        if member.replica_id != self.id
            || member.workload_id != self.workload_id
            || member.node_id.is_some()
            || self.lifecycle != WorkloadReplicaLifecycle::Retiring
            || self.evacuation_node_id.is_none()
            || self.retirement_command_id.is_none()
            || self.runtime_fenced_at.is_none()
            || at < self.updated_at.max(member.updated_at)
        {
            return Err("Workload replica cannot complete evacuation before fencing".into());
        }
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| "Workload replica generation overflowed".to_string())?;
        self.lifecycle = WorkloadReplicaLifecycle::Desired;
        self.evacuation_node_id = None;
        self.retirement_command_id = None;
        self.runtime_fenced_at = None;
        self.aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Workload replica version overflowed".to_string())?;
        self.updated_at = at;
        Ok(())
    }

    pub fn reactivate(
        &mut self,
        revision: &WorkloadRevision,
        at: DateTime<Utc>,
    ) -> Result<(), String> {
        let at = canonical_timestamp(at);
        if self.lifecycle != WorkloadReplicaLifecycle::Retired
            || revision.workload_id != self.workload_id
            || revision.generation < self.revision_generation
            || at < self.updated_at
        {
            return Err("Workload replica reactivation is invalid".into());
        }
        self.revision_id = revision.id;
        self.revision_generation = revision.generation;
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| "Workload replica generation overflowed".to_string())?;
        self.lifecycle = WorkloadReplicaLifecycle::Desired;
        self.evacuation_node_id = None;
        self.retirement_command_id = None;
        self.runtime_fenced_at = None;
        self.aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Workload replica version overflowed".to_string())?;
        self.updated_at = at;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.id.as_uuid().is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.workload_id.as_uuid().is_nil()
            || self.revision_id.as_uuid().is_nil()
            || Self::deterministic_id(self.workload_id, self.ordinal).ok() != Some(self.id)
            || self.revision_generation == 0
            || self.generation == 0
            || self.aggregate_version == 0
            || self.updated_at < self.created_at
            || self
                .evacuation_node_id
                .is_some_and(|node_id| node_id.as_uuid().is_nil())
            || self
                .runtime_fenced_at
                .is_some_and(|fenced_at| fenced_at < self.created_at || fenced_at > self.updated_at)
        {
            return Err("Workload replica is invalid".into());
        }
        let retirement_state_valid = match self.lifecycle {
            WorkloadReplicaLifecycle::Desired => {
                self.evacuation_node_id.is_none()
                    && self.retirement_command_id.is_none()
                    && self.runtime_fenced_at.is_none()
            }
            WorkloadReplicaLifecycle::Retiring => {
                self.runtime_fenced_at.is_none() || self.retirement_command_id.is_some()
            }
            WorkloadReplicaLifecycle::Retired => {
                self.evacuation_node_id.is_none()
                    && self.retirement_command_id.is_some() == self.runtime_fenced_at.is_some()
            }
        };
        if !retirement_state_valid {
            return Err("Workload replica retirement evidence is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkloadReplicaMember {
    pub id: WorkloadReplicaMemberId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub workload_id: WorkloadId,
    pub replica_id: WorkloadReplicaId,
    pub ordinal: u32,
    pub node_id: Option<NodeId>,
    pub placement_generation: u64,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl WorkloadReplicaMember {
    pub fn canonical(workload: &Workload, replica: &WorkloadReplica) -> Result<Self, String> {
        Self::for_replica(workload, replica)
    }

    pub fn for_replica(workload: &Workload, replica: &WorkloadReplica) -> Result<Self, String> {
        Self::for_ordinal(workload, replica, CANONICAL_REPLICA_ORDINAL)
    }

    pub fn for_ordinal(
        workload: &Workload,
        replica: &WorkloadReplica,
        ordinal: u32,
    ) -> Result<Self, String> {
        replica.validate()?;
        if replica.organization_id != workload.organization_id
            || replica.project_id != workload.project_id
            || replica.environment_id != workload.environment_id
            || replica.workload_id != workload.id
        {
            return Err("Workload replica member has the wrong replica".into());
        }
        let member = Self {
            id: Self::deterministic_id(replica.id, ordinal)?,
            organization_id: workload.organization_id,
            project_id: workload.project_id,
            environment_id: workload.environment_id,
            workload_id: workload.id,
            replica_id: replica.id,
            ordinal,
            node_id: None,
            placement_generation: 0,
            aggregate_version: 1,
            created_at: workload.created_at,
            updated_at: workload.created_at,
        };
        member.validate()?;
        Ok(member)
    }

    pub fn deterministic_id(
        replica_id: WorkloadReplicaId,
        ordinal: u32,
    ) -> Result<WorkloadReplicaMemberId, String> {
        if ordinal >= MAX_WORKLOAD_PLACEMENT_GROUP_MEMBERS {
            return Err(format!(
                "Workload placement-group member ordinal must be smaller than {MAX_WORKLOAD_PLACEMENT_GROUP_MEMBERS}"
            ));
        }
        if ordinal == CANONICAL_REPLICA_ORDINAL {
            return Ok(WorkloadReplicaMemberId::from_uuid(replica_id.as_uuid()));
        }
        let name = format!("{REPLICA_MEMBER_ID_DOMAIN}:{ordinal}");
        Ok(WorkloadReplicaMemberId::from_uuid(Uuid::new_v5(
            &replica_id.as_uuid(),
            name.as_bytes(),
        )))
    }

    pub fn place(&mut self, node_id: NodeId, at: DateTime<Utc>) -> Result<(), String> {
        let at = canonical_timestamp(at);
        if node_id.as_uuid().is_nil() || at < self.updated_at {
            return Err("Workload replica member placement is invalid".into());
        }
        match self.node_id {
            Some(existing) if existing == node_id => {
                self.updated_at = at;
                Ok(())
            }
            Some(_) => Err(
                "Workload replica member cannot move without explicit release or fencing".into(),
            ),
            None => {
                self.node_id = Some(node_id);
                self.placement_generation = self
                    .placement_generation
                    .checked_add(1)
                    .ok_or_else(|| "Workload placement generation overflowed".to_string())?;
                self.aggregate_version = self
                    .aggregate_version
                    .checked_add(1)
                    .ok_or_else(|| "Workload replica member version overflowed".to_string())?;
                self.updated_at = at;
                Ok(())
            }
        }
    }

    pub fn release_after_fencing(
        &mut self,
        node_id: NodeId,
        at: DateTime<Utc>,
    ) -> Result<(), String> {
        let at = canonical_timestamp(at);
        if at < self.updated_at || node_id.as_uuid().is_nil() {
            return Err("Workload replica member release is invalid".into());
        }
        match self.node_id {
            None => {
                self.updated_at = at;
                Ok(())
            }
            Some(existing) if existing != node_id => {
                Err("Workload replica member release changed its fenced node".into())
            }
            Some(_) => {
                self.node_id = None;
                self.aggregate_version = self
                    .aggregate_version
                    .checked_add(1)
                    .ok_or_else(|| "Workload replica member version overflowed".to_string())?;
                self.updated_at = at;
                Ok(())
            }
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.id.as_uuid().is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.workload_id.as_uuid().is_nil()
            || self.replica_id.as_uuid().is_nil()
            || Self::deterministic_id(self.replica_id, self.ordinal).ok() != Some(self.id)
            || self.node_id.is_some() && self.placement_generation == 0
            || self.aggregate_version == 0
            || self.updated_at < self.created_at
        {
            return Err("Workload replica member is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentReplicaBinding {
    pub deployment_id: DeploymentId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub workload_id: WorkloadId,
    pub revision_id: WorkloadRevisionId,
    pub replica_id: WorkloadReplicaId,
    pub replica_generation: u64,
    pub member_id: WorkloadReplicaMemberId,
    pub node_id: Option<NodeId>,
    pub placement_generation: u64,
    pub runtime_unit_id: String,
    pub runtime_generation: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl DeploymentReplicaBinding {
    pub fn create(
        deployment: &Deployment,
        revision: &WorkloadRevision,
        replica: &WorkloadReplica,
        member: &WorkloadReplicaMember,
    ) -> Result<Self, String> {
        let binding = Self::create_with_runtime_unit_id(
            deployment,
            replica,
            member,
            replica.runtime_unit_id(revision)?,
        );
        binding.validate_against(deployment, revision, replica, member)?;
        Ok(binding)
    }

    pub fn create_for_placement_group_member(
        deployment: &Deployment,
        revision: &WorkloadRevision,
        replica: &WorkloadReplica,
        member: &WorkloadReplicaMember,
        plan: &WorkloadPlacementGroupMemberPlan,
    ) -> Result<Self, String> {
        let binding = Self::create_with_runtime_unit_id(
            deployment,
            replica,
            member,
            plan.runtime_unit_id.clone(),
        );
        binding
            .validate_against_placement_group_member(deployment, revision, replica, member, plan)?;
        Ok(binding)
    }

    fn create_with_runtime_unit_id(
        deployment: &Deployment,
        replica: &WorkloadReplica,
        member: &WorkloadReplicaMember,
        runtime_unit_id: String,
    ) -> Self {
        Self {
            deployment_id: deployment.id,
            organization_id: replica.organization_id,
            project_id: replica.project_id,
            environment_id: replica.environment_id,
            workload_id: deployment.workload_id,
            revision_id: deployment.revision_id,
            replica_id: replica.id,
            replica_generation: replica.generation,
            member_id: member.id,
            node_id: deployment.node_id,
            placement_generation: member.placement_generation,
            runtime_unit_id,
            runtime_generation: replica.generation,
            created_at: deployment.requested_at,
            updated_at: deployment.updated_at,
        }
    }

    pub fn assign(
        &mut self,
        deployment: &Deployment,
        member: &WorkloadReplicaMember,
    ) -> Result<(), String> {
        if deployment.id != self.deployment_id
            || deployment.node_id != member.node_id
            || deployment.updated_at < self.updated_at
            || member.replica_id != self.replica_id
        {
            return Err("deployment replica placement binding is inconsistent".into());
        }
        self.node_id = deployment.node_id;
        self.placement_generation = member.placement_generation;
        self.updated_at = deployment.updated_at;
        Ok(())
    }

    pub fn assign_placement_group_member(
        &mut self,
        deployment: &Deployment,
        member: &WorkloadReplicaMember,
        plan: &WorkloadPlacementGroupMemberPlan,
    ) -> Result<(), String> {
        let node_id = member
            .node_id
            .ok_or_else(|| "placement-group member assignment omitted its node".to_string())?;
        if deployment.id != self.deployment_id
            || deployment.status != DeploymentStatus::Scheduled
            || deployment.node_id.is_none()
            || deployment.updated_at < self.updated_at
            || member.replica_id != self.replica_id
            || plan.member_id != member.id
            || plan.member_id != self.member_id
            || plan.runtime_unit_id != self.runtime_unit_id
            || plan.ordinal == 0 && deployment.node_id != Some(node_id)
            || self.node_id.is_some_and(|existing| existing != node_id)
        {
            return Err("deployment placement-group member assignment is inconsistent".into());
        }
        self.node_id = Some(node_id);
        self.placement_generation = member.placement_generation;
        self.updated_at = deployment.updated_at;
        Ok(())
    }

    pub fn propose_assignment(&self, node_id: NodeId, at: DateTime<Utc>) -> Result<Self, String> {
        let at = canonical_timestamp(at);
        if self.node_id.is_some() || node_id.as_uuid().is_nil() || at < self.updated_at {
            return Err("deployment replica binding cannot propose an initial assignment".into());
        }
        let mut candidate = self.clone();
        candidate.node_id = Some(node_id);
        if candidate.placement_generation == 0 {
            candidate.placement_generation = 1;
        }
        candidate.updated_at = at;
        Ok(candidate)
    }

    pub fn placement_group_resource_claim_id(&self) -> ResourceClaimId {
        let name = format!("{PLACEMENT_GROUP_CLAIM_ID_DOMAIN}:{}", self.member_id);
        ResourceClaimId::from_uuid(Uuid::new_v5(&self.deployment_id.as_uuid(), name.as_bytes()))
    }

    pub fn validate_against(
        &self,
        deployment: &Deployment,
        revision: &WorkloadRevision,
        replica: &WorkloadReplica,
        member: &WorkloadReplicaMember,
    ) -> Result<(), String> {
        self.validate_common(deployment, revision, replica, member)?;
        if self.node_id != deployment.node_id
            || self.node_id.is_some() && self.node_id != member.node_id
            || self.runtime_unit_id != replica.runtime_unit_id(revision)?
        {
            return Err("deployment replica binding is invalid".into());
        }
        Ok(())
    }

    pub fn validate_against_placement_group_member(
        &self,
        deployment: &Deployment,
        revision: &WorkloadRevision,
        replica: &WorkloadReplica,
        member: &WorkloadReplicaMember,
        plan: &WorkloadPlacementGroupMemberPlan,
    ) -> Result<(), String> {
        self.validate_common(deployment, revision, replica, member)?;
        let current_placement = self.node_id == member.node_id;
        let historical_release = matches!(
            deployment.status,
            DeploymentStatus::Failed | DeploymentStatus::Orphaned | DeploymentStatus::Cancelled
        ) && self.node_id.is_some()
            && member.node_id.is_none();
        if plan.member_id != member.id
            || plan.ordinal != member.ordinal
            || plan.runtime_unit_id != self.runtime_unit_id
            || plan.template.digest()? != plan.template_digest
            || replica.runtime_unit_id_for_member(revision, member)? != self.runtime_unit_id
            || !current_placement && !historical_release
            || plan.ordinal == 0 && self.node_id != deployment.node_id
            || deployment.node_id.is_none() && self.node_id.is_some()
            || deployment.node_id.is_some()
                && !deployment.status.is_terminal()
                && self.node_id.is_none()
        {
            return Err("deployment placement-group member binding is invalid".into());
        }
        Ok(())
    }

    /// Returns whether this immutable Deployment binding still names the
    /// current live replica-member placement.
    ///
    /// A binding intentionally survives Runtime fencing as historical
    /// evidence. Consumers that authorize node-scoped behavior must therefore
    /// join it to the current replica and member instead of treating the
    /// historical `node_id` as ongoing authority.
    pub fn is_current_runtime_assignment(
        &self,
        deployment: &Deployment,
        revision: &WorkloadRevision,
        replica: &WorkloadReplica,
        member: &WorkloadReplicaMember,
    ) -> Result<bool, String> {
        self.validate_lineage(deployment, revision, replica, member)?;
        if replica.revision_id != self.revision_id
            || replica.revision_generation != revision.generation
            || replica.lifecycle != WorkloadReplicaLifecycle::Desired
            || self.replica_generation != replica.generation
            || self.placement_generation != member.placement_generation
            || self.node_id != member.node_id
        {
            return Ok(false);
        }
        let Some(node_id) = self.node_id else {
            return Ok(false);
        };
        if self.runtime_unit_id != replica.runtime_unit_id_for_member(revision, member)?
            || member.ordinal == CANONICAL_REPLICA_ORDINAL && deployment.node_id != Some(node_id)
        {
            return Err("deployment replica live assignment is inconsistent".into());
        }
        Ok(true)
    }

    fn validate_lineage(
        &self,
        deployment: &Deployment,
        revision: &WorkloadRevision,
        replica: &WorkloadReplica,
        member: &WorkloadReplicaMember,
    ) -> Result<(), String> {
        replica.validate()?;
        member.validate()?;
        if self.deployment_id != deployment.id
            || self.organization_id != deployment.organization_id
            || self.organization_id != replica.organization_id
            || self.organization_id != member.organization_id
            || self.project_id != replica.project_id
            || self.project_id != member.project_id
            || self.environment_id != replica.environment_id
            || self.environment_id != member.environment_id
            || self.workload_id != deployment.workload_id
            || self.revision_id != deployment.revision_id
            || self.revision_id != revision.id
            || revision.workload_id != self.workload_id
            || revision.generation == 0
            || self.replica_id != replica.id
            || replica.workload_id != self.workload_id
            || self.member_id != member.id
            || member.replica_id != self.replica_id
            || member.workload_id != self.workload_id
            || self.replica_generation == 0
            || self.runtime_generation != self.replica_generation
            || self.node_id.is_some() && self.placement_generation == 0
            || self.runtime_unit_id.trim().is_empty()
            || self.runtime_unit_id.len() > 512
            || self.runtime_unit_id.contains(['\0', '\r', '\n'])
            || self.updated_at < self.created_at
        {
            return Err("deployment replica binding lineage is invalid".into());
        }
        Ok(())
    }

    fn validate_common(
        &self,
        deployment: &Deployment,
        revision: &WorkloadRevision,
        replica: &WorkloadReplica,
        member: &WorkloadReplicaMember,
    ) -> Result<(), String> {
        self.validate_lineage(deployment, revision, replica, member)?;
        if replica.revision_id != self.revision_id
            || replica.revision_generation != revision.generation
            || replica.lifecycle != WorkloadReplicaLifecycle::Desired
            || self.replica_generation != replica.generation
            || self.placement_generation != member.placement_generation
            || self.runtime_generation != replica.generation
        {
            return Err("deployment replica binding is invalid".into());
        }
        Ok(())
    }
}
