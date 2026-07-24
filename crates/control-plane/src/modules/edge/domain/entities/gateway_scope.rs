use crate::modules::edge::domain::GatewayRolloutPolicy;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, EnvironmentId, GatewayScopeId, NodeId, OrganizationId, ProjectId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayScope {
    pub id: GatewayScopeId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    /// Bootstrap primary retained by the current node-local route compiler.
    pub node_id: NodeId,
    /// Ordered desired physical Gateway membership. The primary is always first.
    pub member_node_ids: Vec<NodeId>,
    pub membership_generation: u64,
    pub rollout_policy: GatewayRolloutPolicy,
    pub aggregate_version: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl GatewayScope {
    pub fn create(
        id: GatewayScopeId,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        node_id: NodeId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        Self::create_replicated(
            id,
            organization_id,
            project_id,
            environment_id,
            node_id,
            vec![node_id],
            GatewayRolloutPolicy::single_replica(),
            created_at,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_replicated(
        id: GatewayScopeId,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        node_id: NodeId,
        mut member_node_ids: Vec<NodeId>,
        rollout_policy: GatewayRolloutPolicy,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        if id.as_uuid().is_nil()
            || organization_id.as_uuid().is_nil()
            || project_id.as_uuid().is_nil()
            || environment_id.as_uuid().is_nil()
            || node_id.as_uuid().is_nil()
            || member_node_ids
                .iter()
                .any(|member| member.as_uuid().is_nil())
        {
            return Err("Gateway scope identities must not be nil".into());
        }
        if !member_node_ids.contains(&node_id) {
            return Err("Gateway scope members must contain the bootstrap primary node".into());
        }
        member_node_ids.sort();
        if member_node_ids
            .windows(2)
            .any(|members| members[0] == members[1])
        {
            return Err("Gateway scope members must be unique".into());
        }
        rollout_policy.validate(member_node_ids.len())?;
        member_node_ids.retain(|member| *member != node_id);
        member_node_ids.insert(0, node_id);
        let created_at = canonical_timestamp(created_at);
        Ok(Self {
            id,
            organization_id,
            project_id,
            environment_id,
            node_id,
            member_node_ids,
            membership_generation: 1,
            rollout_policy,
            aggregate_version: 1,
            created_at,
            updated_at: created_at,
        })
    }

    pub fn owns(
        &self,
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        node_id: NodeId,
    ) -> bool {
        self.organization_id == organization_id
            && self.project_id == project_id
            && self.environment_id == environment_id
            && self.node_id == node_id
    }

    pub fn contains_member(&self, node_id: NodeId) -> bool {
        self.member_node_ids.contains(&node_id)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.id.as_uuid().is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.node_id.as_uuid().is_nil()
            || self.membership_generation == 0
            || self.aggregate_version == 0
            || self.updated_at < self.created_at
            || self.member_node_ids.first() != Some(&self.node_id)
            || self
                .member_node_ids
                .iter()
                .any(|member| member.as_uuid().is_nil())
        {
            return Err("Gateway scope state is invalid".into());
        }
        let mut sorted_members = self.member_node_ids.clone();
        sorted_members.sort();
        if sorted_members
            .windows(2)
            .any(|members| members[0] == members[1])
        {
            return Err("Gateway scope members must be unique".into());
        }
        let mut canonical_members = sorted_members;
        canonical_members.retain(|member| *member != self.node_id);
        canonical_members.insert(0, self.node_id);
        if self.member_node_ids != canonical_members {
            return Err("Gateway scope members are not in canonical order".into());
        }
        self.rollout_policy.validate(self.member_node_ids.len())
    }
}
