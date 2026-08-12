use super::{
    EffectivePlacementPolicy, PlacementTopology, ServiceTemplate, Workload, WorkloadDesiredState,
    WorkloadReplica, WorkloadReplicaLifecycle, WorkloadReplicaMember, WorkloadRevision,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, OrganizationId, ProjectId, WorkloadId,
    WorkloadPlacementGroupId, WorkloadReplicaId, WorkloadReplicaMemberId, WorkloadRevisionId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const PLACEMENT_GROUP_ID_DOMAIN: &str = "a3s.cloud.workload-placement-group.v1";
const PLACEMENT_GROUP_PLAN_SCHEMA: &str = "a3s.cloud.workload-placement-group-plan.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadPlacementGroupState {
    Planned,
}

impl WorkloadPlacementGroupState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "planned" => Ok(Self::Planned),
            _ => Err(format!(
                "unsupported Workload placement-group state {value:?}"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadPlacementGroupMemberRole {
    Leader,
    Worker,
}

impl WorkloadPlacementGroupMemberRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Leader => "leader",
            Self::Worker => "worker",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "leader" => Ok(Self::Leader),
            "worker" => Ok(Self::Worker),
            _ => Err(format!(
                "unsupported Workload placement-group member role {value:?}"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkloadPlacementGroupMemberPlan {
    pub member_id: WorkloadReplicaMemberId,
    pub ordinal: u32,
    pub role: WorkloadPlacementGroupMemberRole,
    pub runtime_unit_id: String,
    pub template: ServiceTemplate,
    pub template_digest: String,
}

impl WorkloadPlacementGroupMemberPlan {
    fn validate(&self, replica_id: WorkloadReplicaId) -> Result<(), String> {
        self.template.validate()?;
        let expected_role = if self.ordinal == 0 {
            WorkloadPlacementGroupMemberRole::Leader
        } else {
            WorkloadPlacementGroupMemberRole::Worker
        };
        if WorkloadReplicaMember::deterministic_id(replica_id, self.ordinal).ok()
            != Some(self.member_id)
            || self.role != expected_role
            || self.runtime_unit_id.trim().is_empty()
            || self.runtime_unit_id.len() > 512
            || self.runtime_unit_id.contains(['\0', '\r', '\n'])
            || self.template.digest()? != self.template_digest
        {
            return Err("Workload placement-group member plan is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkloadPlacementGroup {
    pub id: WorkloadPlacementGroupId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub workload_id: WorkloadId,
    pub revision_id: WorkloadRevisionId,
    pub revision_generation: u64,
    pub replica_id: WorkloadReplicaId,
    pub replica_generation: u64,
    pub policy_generation: u64,
    pub placement_policy_digest: String,
    pub plan_schema: String,
    pub plan_digest: String,
    pub state: WorkloadPlacementGroupState,
    pub members: Vec<WorkloadPlacementGroupMemberPlan>,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadPlacementGroupWrite {
    pub group: WorkloadPlacementGroup,
    pub replica_members: Vec<WorkloadReplicaMember>,
}

impl WorkloadPlacementGroupWrite {
    pub fn validate(&self) -> Result<(), String> {
        self.group.validate()?;
        if self.replica_members.len() != self.group.members.len() {
            return Err("Workload placement-group write has the wrong member count".into());
        }
        for (stored, planned) in self.replica_members.iter().zip(&self.group.members) {
            self.group.validate_replica_member_identity(stored)?;
            if stored.id != planned.member_id
                || stored.ordinal != planned.ordinal
                || stored.node_id.is_some()
                || stored.placement_generation != 0
                || stored.aggregate_version != 1
                || stored.updated_at != stored.created_at
            {
                return Err("Workload placement-group write member is inconsistent".into());
            }
        }
        Ok(())
    }
}

impl WorkloadPlacementGroup {
    pub fn plan(
        workload: &Workload,
        policy: &EffectivePlacementPolicy,
        revision: &WorkloadRevision,
        replica: &WorkloadReplica,
        templates: Vec<ServiceTemplate>,
        created_at: DateTime<Utc>,
    ) -> Result<WorkloadPlacementGroupWrite, String> {
        policy.validate()?;
        replica.validate()?;
        let created_at = canonical_timestamp(created_at);
        if templates.len() != policy.members_per_replica() as usize {
            return Err("Workload placement-group planning context is inconsistent".into());
        }
        if templates.first() != Some(revision.resolved_template()?) {
            return Err(
                "Workload placement-group leader must use the exact revision template".into(),
            );
        }

        let mut replica_members = Vec::with_capacity(templates.len());
        let mut members = Vec::with_capacity(templates.len());
        for (ordinal, template) in templates.into_iter().enumerate() {
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| "Workload placement-group member ordinal overflowed")?;
            let member = WorkloadReplicaMember::for_ordinal(workload, replica, ordinal)?;
            let role = if ordinal == 0 {
                WorkloadPlacementGroupMemberRole::Leader
            } else {
                WorkloadPlacementGroupMemberRole::Worker
            };
            let runtime_unit_id = replica.runtime_unit_id_for_member(revision, &member)?;
            let template_digest = template.digest()?;
            members.push(WorkloadPlacementGroupMemberPlan {
                member_id: member.id,
                ordinal,
                role,
                runtime_unit_id,
                template,
                template_digest,
            });
            replica_members.push(member);
        }

        let id = Self::deterministic_id(replica.id, replica.generation);
        let mut group = Self {
            id,
            organization_id: workload.organization_id,
            project_id: workload.project_id,
            environment_id: workload.environment_id,
            workload_id: workload.id,
            revision_id: revision.id,
            revision_generation: revision.generation,
            replica_id: replica.id,
            replica_generation: replica.generation,
            policy_generation: policy.generation(),
            placement_policy_digest: policy.digest().to_owned(),
            plan_schema: PLACEMENT_GROUP_PLAN_SCHEMA.into(),
            plan_digest: String::new(),
            state: WorkloadPlacementGroupState::Planned,
            members,
            aggregate_version: 1,
            created_at,
            updated_at: created_at,
        };
        group.plan_digest = group.calculate_plan_digest()?;
        group.validate_context(workload, policy, revision, replica)?;
        let write = WorkloadPlacementGroupWrite {
            group,
            replica_members,
        };
        write.validate()?;
        Ok(write)
    }

    pub fn validate_context(
        &self,
        workload: &Workload,
        policy: &EffectivePlacementPolicy,
        revision: &WorkloadRevision,
        replica: &WorkloadReplica,
    ) -> Result<(), String> {
        self.validate()?;
        policy.validate()?;
        replica.validate()?;
        let leader_template = revision.resolved_template()?;
        if workload.desired_state != WorkloadDesiredState::Running
            || workload.organization_id != self.organization_id
            || workload.project_id != self.project_id
            || workload.environment_id != self.environment_id
            || workload.id != self.workload_id
            || revision.workload_id != workload.id
            || revision.id != self.revision_id
            || revision.generation != self.revision_generation
            || self.members.first().map(|member| &member.template) != Some(leader_template)
            || replica.organization_id != workload.organization_id
            || replica.project_id != workload.project_id
            || replica.environment_id != workload.environment_id
            || replica.workload_id != workload.id
            || replica.id != self.replica_id
            || replica.revision_id != revision.id
            || replica.revision_generation != revision.generation
            || replica.generation != self.replica_generation
            || replica.lifecycle != WorkloadReplicaLifecycle::Desired
            || replica.ordinal >= policy.desired_replicas()
            || policy.generation() != self.policy_generation
            || policy.digest() != self.placement_policy_digest
            || policy.topology() != PlacementTopology::MultiNode
            || policy.members_per_replica() as usize != self.members.len()
            || self.created_at
                < workload
                    .updated_at
                    .max(revision.created_at)
                    .max(replica.updated_at)
        {
            return Err("Workload placement-group planning context is inconsistent".into());
        }
        Ok(())
    }

    pub(crate) fn validate_replica_member_identity(
        &self,
        member: &WorkloadReplicaMember,
    ) -> Result<(), String> {
        member.validate()?;
        let planned = usize::try_from(member.ordinal)
            .ok()
            .and_then(|ordinal| self.members.get(ordinal));
        if planned.is_none_or(|planned| {
            planned.ordinal != member.ordinal || planned.member_id != member.id
        }) || member.organization_id != self.organization_id
            || member.project_id != self.project_id
            || member.environment_id != self.environment_id
            || member.workload_id != self.workload_id
            || member.replica_id != self.replica_id
        {
            return Err("Workload placement-group replica member identity is inconsistent".into());
        }
        Ok(())
    }

    pub(crate) fn validate_available_replica_member(
        &self,
        member: &WorkloadReplicaMember,
    ) -> Result<(), String> {
        self.validate_replica_member_identity(member)?;
        if member.node_id.is_some() || member.updated_at > self.created_at {
            return Err("Workload placement-group replica member is not available".into());
        }
        Ok(())
    }

    pub fn deterministic_id(
        replica_id: WorkloadReplicaId,
        replica_generation: u64,
    ) -> WorkloadPlacementGroupId {
        let name = format!("{PLACEMENT_GROUP_ID_DOMAIN}:{replica_generation}");
        WorkloadPlacementGroupId::from_uuid(Uuid::new_v5(&replica_id.as_uuid(), name.as_bytes()))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.id.as_uuid().is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.workload_id.as_uuid().is_nil()
            || self.revision_id.as_uuid().is_nil()
            || self.revision_generation == 0
            || self.replica_id.as_uuid().is_nil()
            || self.replica_generation == 0
            || self.policy_generation == 0
            || self.id != Self::deterministic_id(self.replica_id, self.replica_generation)
            || !is_sha256_digest(&self.placement_policy_digest)
            || self.plan_schema != PLACEMENT_GROUP_PLAN_SCHEMA
            || self.state != WorkloadPlacementGroupState::Planned
            || self.members.len() < 2
            || self.members.len() > super::MAX_WORKLOAD_PLACEMENT_GROUP_MEMBERS as usize
            || self.aggregate_version == 0
            || self.updated_at < self.created_at
        {
            return Err("Workload placement-group identity or state is invalid".into());
        }
        for (ordinal, member) in self.members.iter().enumerate() {
            let ordinal = u32::try_from(ordinal)
                .map_err(|_| "Workload placement-group member ordinal overflowed")?;
            if member.ordinal != ordinal {
                return Err("Workload placement-group members are not canonical".into());
            }
            member.validate(self.replica_id)?;
        }
        if !is_sha256_digest(&self.plan_digest) || self.calculate_plan_digest()? != self.plan_digest
        {
            return Err("Workload placement-group plan digest is invalid".into());
        }
        Ok(())
    }

    pub fn same_plan(&self, candidate: &Self) -> bool {
        self.id == candidate.id
            && self.organization_id == candidate.organization_id
            && self.project_id == candidate.project_id
            && self.environment_id == candidate.environment_id
            && self.workload_id == candidate.workload_id
            && self.revision_id == candidate.revision_id
            && self.revision_generation == candidate.revision_generation
            && self.replica_id == candidate.replica_id
            && self.replica_generation == candidate.replica_generation
            && self.policy_generation == candidate.policy_generation
            && self.placement_policy_digest == candidate.placement_policy_digest
            && self.plan_schema == candidate.plan_schema
            && self.plan_digest == candidate.plan_digest
            && self.members == candidate.members
    }

    fn calculate_plan_digest(&self) -> Result<String, String> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct DigestMember<'a> {
            member_id: WorkloadReplicaMemberId,
            ordinal: u32,
            role: WorkloadPlacementGroupMemberRole,
            runtime_unit_id: &'a str,
            template_digest: &'a str,
        }

        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct DigestDocument<'a> {
            schema: &'a str,
            group_id: WorkloadPlacementGroupId,
            organization_id: OrganizationId,
            project_id: ProjectId,
            environment_id: EnvironmentId,
            workload_id: WorkloadId,
            revision_id: WorkloadRevisionId,
            revision_generation: u64,
            replica_id: WorkloadReplicaId,
            replica_generation: u64,
            policy_generation: u64,
            placement_policy_digest: &'a str,
            members: Vec<DigestMember<'a>>,
        }

        let members = self
            .members
            .iter()
            .map(|member| DigestMember {
                member_id: member.member_id,
                ordinal: member.ordinal,
                role: member.role,
                runtime_unit_id: &member.runtime_unit_id,
                template_digest: &member.template_digest,
            })
            .collect();
        let encoded = serde_json::to_vec(&DigestDocument {
            schema: &self.plan_schema,
            group_id: self.id,
            organization_id: self.organization_id,
            project_id: self.project_id,
            environment_id: self.environment_id,
            workload_id: self.workload_id,
            revision_id: self.revision_id,
            revision_generation: self.revision_generation,
            replica_id: self.replica_id,
            replica_generation: self.replica_generation,
            policy_generation: self.policy_generation,
            placement_policy_digest: &self.placement_policy_digest,
            members,
        })
        .map_err(|error| format!("could not encode Workload placement-group plan: {error}"))?;
        Ok(format!("sha256:{:x}", Sha256::digest(encoded)))
    }
}

fn is_sha256_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}
