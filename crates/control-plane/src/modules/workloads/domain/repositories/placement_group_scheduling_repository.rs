use crate::modules::shared_kernel::domain::{
    DeploymentId, IdempotentWrite, NodeId, OrganizationId, RepositoryError,
    WorkloadPlacementGroupId, WorkloadReplicaMemberId,
};
use crate::modules::workloads::domain::entities::{
    Deployment, DeploymentReplicaBinding, MAX_WORKLOAD_PLACEMENT_GROUP_MEMBERS,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlacementGroupMemberPlacement {
    pub ordinal: u32,
    pub member_id: WorkloadReplicaMemberId,
    pub node_id: NodeId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacementGroupSchedulingWrite {
    pub organization_id: OrganizationId,
    pub deployment_id: DeploymentId,
    pub expected_deployment_version: u64,
    pub group_id: WorkloadPlacementGroupId,
    pub group_plan_digest: String,
    pub placements: Vec<PlacementGroupMemberPlacement>,
    pub scheduled_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacementGroupCancellationWrite {
    pub organization_id: OrganizationId,
    pub deployment_id: DeploymentId,
    pub expected_deployment_version: u64,
    pub group_id: WorkloadPlacementGroupId,
    pub group_plan_digest: String,
    pub cancelled_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementGroupPlacement {
    pub deployment: Deployment,
    pub member_bindings: Vec<DeploymentReplicaBinding>,
}

impl PlacementGroupSchedulingWrite {
    pub fn validate(&self) -> Result<(), String> {
        validate_identity(
            self.organization_id,
            self.deployment_id,
            self.expected_deployment_version,
            self.group_id,
            &self.group_plan_digest,
        )?;
        if self.placements.len() < 2
            || self.placements.len() > MAX_WORKLOAD_PLACEMENT_GROUP_MEMBERS as usize
            || self.scheduled_at.timestamp_millis() <= 0
        {
            return Err("placement-group scheduling write is invalid".into());
        }
        let mut member_ids = BTreeSet::new();
        let mut node_ids = BTreeSet::new();
        for (expected_ordinal, placement) in self.placements.iter().enumerate() {
            let expected_ordinal = u32::try_from(expected_ordinal)
                .map_err(|_| "placement-group scheduling ordinal overflowed")?;
            if placement.ordinal != expected_ordinal
                || placement.member_id.as_uuid().is_nil()
                || placement.node_id.as_uuid().is_nil()
                || !member_ids.insert(placement.member_id)
                || !node_ids.insert(placement.node_id)
            {
                return Err("placement-group member placements are not canonical".into());
            }
        }
        Ok(())
    }
}

impl PlacementGroupCancellationWrite {
    pub fn validate(&self) -> Result<(), String> {
        validate_identity(
            self.organization_id,
            self.deployment_id,
            self.expected_deployment_version,
            self.group_id,
            &self.group_plan_digest,
        )?;
        if self.cancelled_at.timestamp_millis() <= 0 {
            return Err("placement-group cancellation time is invalid".into());
        }
        Ok(())
    }
}

#[async_trait]
pub trait IWorkloadPlacementGroupSchedulingRepository: Send + Sync {
    async fn schedule_placement_group(
        &self,
        write: PlacementGroupSchedulingWrite,
    ) -> Result<IdempotentWrite<PlacementGroupPlacement>, RepositoryError>;

    async fn cancel_placement_group(
        &self,
        write: PlacementGroupCancellationWrite,
    ) -> Result<IdempotentWrite<PlacementGroupPlacement>, RepositoryError>;
}

fn validate_identity(
    organization_id: OrganizationId,
    deployment_id: DeploymentId,
    expected_deployment_version: u64,
    group_id: WorkloadPlacementGroupId,
    group_plan_digest: &str,
) -> Result<(), String> {
    if organization_id.as_uuid().is_nil()
        || deployment_id.as_uuid().is_nil()
        || expected_deployment_version == 0
        || group_id.as_uuid().is_nil()
        || !is_sha256_digest(group_plan_digest)
    {
        return Err("placement-group scheduling identity is invalid".into());
    }
    Ok(())
}

fn is_sha256_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduling_write_requires_canonical_members_on_distinct_nodes() {
        let first_node = NodeId::new();
        let mut write = PlacementGroupSchedulingWrite {
            organization_id: OrganizationId::new(),
            deployment_id: DeploymentId::new(),
            expected_deployment_version: 2,
            group_id: WorkloadPlacementGroupId::new(),
            group_plan_digest: format!("sha256:{}", "a".repeat(64)),
            placements: vec![
                PlacementGroupMemberPlacement {
                    ordinal: 0,
                    member_id: WorkloadReplicaMemberId::new(),
                    node_id: first_node,
                },
                PlacementGroupMemberPlacement {
                    ordinal: 1,
                    member_id: WorkloadReplicaMemberId::new(),
                    node_id: NodeId::new(),
                },
            ],
            scheduled_at: Utc::now(),
        };
        assert!(write.validate().is_ok());

        write.placements[1].node_id = first_node;
        assert_eq!(
            write.validate(),
            Err("placement-group member placements are not canonical".into())
        );
    }
}
