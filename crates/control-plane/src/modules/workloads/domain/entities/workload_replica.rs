use crate::modules::shared_kernel::domain::{
    canonical_timestamp, DeploymentId, EnvironmentId, NodeId, OrganizationId, ProjectId,
    WorkloadId, WorkloadReplicaId, WorkloadReplicaMemberId, WorkloadRevisionId,
};
use crate::modules::workloads::domain::entities::{
    Deployment, Workload, WorkloadRevision, MAX_WORKLOAD_REPLICAS,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const CANONICAL_REPLICA_ORDINAL: u32 = 0;
const REPLICA_ID_DOMAIN: &str = "a3s.cloud.workload-replica.v1";

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
        self.aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Workload replica version overflowed".to_string())?;
        self.updated_at = at;
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
        {
            return Err("Workload replica is invalid".into());
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
        if replica.workload_id != workload.id {
            return Err("Workload replica member has the wrong replica".into());
        }
        let member = Self {
            id: WorkloadReplicaMemberId::from_uuid(replica.id.as_uuid()),
            organization_id: workload.organization_id,
            project_id: workload.project_id,
            environment_id: workload.environment_id,
            workload_id: workload.id,
            replica_id: replica.id,
            ordinal: CANONICAL_REPLICA_ORDINAL,
            node_id: None,
            placement_generation: 0,
            aggregate_version: 1,
            created_at: workload.created_at,
            updated_at: workload.created_at,
        };
        member.validate()?;
        Ok(member)
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
                "single-node Workload replica cannot move without explicit release or fencing"
                    .into(),
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
            || self.ordinal != CANONICAL_REPLICA_ORDINAL
            || self.id.as_uuid() != self.replica_id.as_uuid()
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
        let binding = Self {
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
            runtime_unit_id: replica.runtime_unit_id(revision)?,
            runtime_generation: replica.generation,
            created_at: deployment.requested_at,
            updated_at: deployment.updated_at,
        };
        binding.validate_against(deployment, revision, replica, member)?;
        Ok(binding)
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

    pub fn validate_against(
        &self,
        deployment: &Deployment,
        revision: &WorkloadRevision,
        replica: &WorkloadReplica,
        member: &WorkloadReplicaMember,
    ) -> Result<(), String> {
        if self.deployment_id != deployment.id
            || self.organization_id != deployment.organization_id
            || self.workload_id != deployment.workload_id
            || self.revision_id != deployment.revision_id
            || self.revision_id != revision.id
            || revision.workload_id != self.workload_id
            || self.replica_id != replica.id
            || replica.workload_id != self.workload_id
            || replica.revision_id != self.revision_id
            || replica.revision_generation != revision.generation
            || replica.lifecycle != WorkloadReplicaLifecycle::Desired
            || self.replica_generation != replica.generation
            || self.member_id != member.id
            || member.replica_id != self.replica_id
            || member.workload_id != self.workload_id
            || self.node_id != deployment.node_id
            || self.node_id.is_some() && self.node_id != member.node_id
            || self.placement_generation != member.placement_generation
            || self.runtime_unit_id != replica.runtime_unit_id(revision)?
            || self.runtime_generation != replica.generation
            || self.runtime_unit_id.trim().is_empty()
            || self.runtime_unit_id.len() > 512
            || self.runtime_unit_id.contains(['\0', '\r', '\n'])
            || self.runtime_generation == 0
            || self.updated_at < self.created_at
        {
            return Err("deployment replica binding is invalid".into());
        }
        Ok(())
    }
}
