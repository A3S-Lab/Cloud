use crate::modules::fleet::domain::entities::NodePool;
use crate::modules::shared_kernel::domain::{NodePoolId, OrganizationId};
use a3s_cloud_contracts::DomainEventEnvelope;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodePoolChangeKind {
    Created,
    MembersAdded,
    MemberRemovalRequested,
    MembersRemoved,
    MaintenanceScheduled,
    MaintenanceCancelled,
}

impl NodePoolChangeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::MembersAdded => "members_added",
            Self::MemberRemovalRequested => "member_removal_requested",
            Self::MembersRemoved => "members_removed",
            Self::MaintenanceScheduled => "maintenance_scheduled",
            Self::MaintenanceCancelled => "maintenance_cancelled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodePoolChanged {
    pub organization_id: OrganizationId,
    pub node_pool_id: NodePoolId,
    pub change: NodePoolChangeKind,
    pub spec_digest: String,
    pub maintenance_generation: Option<u64>,
}

impl NodePoolChanged {
    pub fn envelope(
        pool: &NodePool,
        change: NodePoolChangeKind,
        occurred_at: DateTime<Utc>,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "fleet.node-pool.changed".into(),
            schema_version: 1,
            scope: a3s_cloud_contracts::CloudScopeRef::Organization {
                organization_id: pool.organization_id.as_uuid(),
            },
            aggregate_id: pool.id.as_uuid(),
            aggregate_version: pool.aggregate_version,
            occurred_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(Self {
                organization_id: pool.organization_id,
                node_pool_id: pool.id,
                change,
                spec_digest: pool.spec_digest.clone(),
                maintenance_generation: pool.maintenance.as_ref().map(|window| window.generation),
            })?,
        })
    }
}
