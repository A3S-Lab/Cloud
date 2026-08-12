use super::{ManageNodePool, NodePoolMutation, NodePoolMutationResult};
use crate::modules::fleet::domain::entities::NodePool;
use crate::modules::fleet::domain::events::{NodePoolChangeKind, NodePoolChanged};
use crate::modules::fleet::domain::repositories::{INodePoolRepository, NodePoolWrite};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, IdempotencyRequest, NodeId, ResourceName,
};
use a3s_boot::{BootError, CommandHandler, CqrsContext};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use std::sync::Arc;

pub struct ManageNodePoolHandler {
    node_pools: Arc<dyn INodePoolRepository>,
}

impl ManageNodePoolHandler {
    pub fn new(node_pools: Arc<dyn INodePoolRepository>) -> Self {
        Self { node_pools }
    }
}

impl CommandHandler<ManageNodePool> for ManageNodePoolHandler {
    fn execute(
        &self,
        command: ManageNodePool,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<NodePoolMutationResult>>>
    {
        let node_pools = Arc::clone(&self.node_pools);
        Box::pin(async move {
            if !command.resource_access.is_organization_wide() {
                return Ok(Err(ApplicationError::Forbidden(
                    "node pool policy requires organization-wide access".into(),
                )));
            }
            let prepared = match PreparedMutation::new(command.mutation) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            let canonical = serde_json::to_vec(
                &prepared.canonical(command.organization_id, command.node_pool_id),
            )
            .map_err(|error| BootError::Internal(error.to_string()))?;
            let idempotency = match IdempotencyRequest::new(
                prepared.scope(command.organization_id, command.node_pool_id),
                command.idempotency_key,
                &canonical,
            ) {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match node_pools.replay(&idempotency).await {
                Ok(Some(node_pool)) => {
                    return Ok(Ok(NodePoolMutationResult {
                        node_pool,
                        replayed: true,
                    }))
                }
                Ok(None) => {}
                Err(error) => return Ok(Err(error.into())),
            }
            let (node_pool, expected_version, change) = match prepared {
                PreparedMutation::Create {
                    name,
                    member_node_ids,
                } => match NodePool::create(
                    command.node_pool_id,
                    command.organization_id,
                    name,
                    member_node_ids,
                    command.requested_at,
                ) {
                    Ok(pool) => (pool, None, NodePoolChangeKind::Created),
                    Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
                },
                PreparedMutation::AddMembers {
                    expected_version,
                    member_node_ids,
                } => {
                    let mut pool = match node_pools
                        .find(command.organization_id, command.node_pool_id)
                        .await
                    {
                        Ok(value) => value,
                        Err(error) => return Ok(Err(error.into())),
                    };
                    if let Err(error) = pool.add_members(member_node_ids, command.requested_at) {
                        return Ok(Err(ApplicationError::Conflict(error)));
                    }
                    (
                        pool,
                        Some(expected_version),
                        NodePoolChangeKind::MembersAdded,
                    )
                }
                PreparedMutation::RequestMemberRemoval {
                    expected_version,
                    member_node_ids,
                } => {
                    let mut pool = match node_pools
                        .find(command.organization_id, command.node_pool_id)
                        .await
                    {
                        Ok(value) => value,
                        Err(error) => return Ok(Err(error.into())),
                    };
                    if let Err(error) =
                        pool.request_member_removal(member_node_ids, command.requested_at)
                    {
                        return Ok(Err(ApplicationError::Conflict(error)));
                    }
                    (
                        pool,
                        Some(expected_version),
                        NodePoolChangeKind::MemberRemovalRequested,
                    )
                }
                PreparedMutation::ScheduleMaintenance {
                    expected_version,
                    target_node_ids,
                    starts_at,
                    ends_at,
                    reason,
                } => {
                    let mut pool = match node_pools
                        .find(command.organization_id, command.node_pool_id)
                        .await
                    {
                        Ok(value) => value,
                        Err(error) => return Ok(Err(error.into())),
                    };
                    if let Err(error) = pool.schedule_maintenance(
                        target_node_ids,
                        starts_at,
                        ends_at,
                        reason,
                        command.requested_at,
                    ) {
                        return Ok(Err(ApplicationError::Invalid(error)));
                    }
                    (
                        pool,
                        Some(expected_version),
                        NodePoolChangeKind::MaintenanceScheduled,
                    )
                }
                PreparedMutation::CancelMaintenance {
                    expected_version,
                    maintenance_generation,
                } => {
                    let mut pool = match node_pools
                        .find(command.organization_id, command.node_pool_id)
                        .await
                    {
                        Ok(value) => value,
                        Err(error) => return Ok(Err(error.into())),
                    };
                    if let Err(error) =
                        pool.cancel_maintenance(maintenance_generation, command.requested_at)
                    {
                        return Ok(Err(ApplicationError::Conflict(error)));
                    }
                    (
                        pool,
                        Some(expected_version),
                        NodePoolChangeKind::MaintenanceCancelled,
                    )
                }
            };
            let event = NodePoolChanged::envelope(
                &node_pool,
                change,
                node_pool.updated_at,
                command.request_id,
            )
            .map_err(|error| BootError::Internal(error.to_string()))?;
            match node_pools
                .save(NodePoolWrite {
                    pool: node_pool,
                    expected_version,
                    event,
                    idempotency,
                })
                .await
            {
                Ok(result) => Ok(Ok(NodePoolMutationResult {
                    node_pool: result.value,
                    replayed: result.replayed,
                })),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

enum PreparedMutation {
    Create {
        name: ResourceName,
        member_node_ids: Vec<NodeId>,
    },
    AddMembers {
        expected_version: u64,
        member_node_ids: Vec<NodeId>,
    },
    RequestMemberRemoval {
        expected_version: u64,
        member_node_ids: Vec<NodeId>,
    },
    ScheduleMaintenance {
        expected_version: u64,
        target_node_ids: Vec<NodeId>,
        starts_at: DateTime<Utc>,
        ends_at: DateTime<Utc>,
        reason: String,
    },
    CancelMaintenance {
        expected_version: u64,
        maintenance_generation: u64,
    },
}

impl PreparedMutation {
    fn new(mutation: NodePoolMutation) -> Result<Self, String> {
        match mutation {
            NodePoolMutation::Create {
                name,
                member_node_ids,
            } => Ok(Self::Create {
                name: ResourceName::parse(name)?,
                member_node_ids: canonical_ids(member_node_ids)?,
            }),
            NodePoolMutation::AddMembers {
                expected_version,
                member_node_ids,
            } => {
                require_version(expected_version)?;
                Ok(Self::AddMembers {
                    expected_version,
                    member_node_ids: canonical_ids(member_node_ids)?,
                })
            }
            NodePoolMutation::RequestMemberRemoval {
                expected_version,
                member_node_ids,
            } => {
                require_version(expected_version)?;
                Ok(Self::RequestMemberRemoval {
                    expected_version,
                    member_node_ids: canonical_ids(member_node_ids)?,
                })
            }
            NodePoolMutation::ScheduleMaintenance {
                expected_version,
                target_node_ids,
                starts_at,
                ends_at,
                reason,
            } => {
                require_version(expected_version)?;
                Ok(Self::ScheduleMaintenance {
                    expected_version,
                    target_node_ids: canonical_ids(target_node_ids)?,
                    starts_at: canonical_timestamp(starts_at),
                    ends_at: canonical_timestamp(ends_at),
                    reason: reason.trim().to_owned(),
                })
            }
            NodePoolMutation::CancelMaintenance {
                expected_version,
                maintenance_generation,
            } => {
                require_version(expected_version)?;
                require_version(maintenance_generation)?;
                Ok(Self::CancelMaintenance {
                    expected_version,
                    maintenance_generation,
                })
            }
        }
    }

    fn scope(
        &self,
        organization_id: crate::modules::shared_kernel::domain::OrganizationId,
        pool_id: crate::modules::shared_kernel::domain::NodePoolId,
    ) -> String {
        match self {
            Self::Create { .. } => format!("organizations/{organization_id}/node-pools"),
            Self::AddMembers { .. } => {
                format!("organizations/{organization_id}/node-pools/{pool_id}/members")
            }
            Self::RequestMemberRemoval { .. } => {
                format!("organizations/{organization_id}/node-pools/{pool_id}/members/removal")
            }
            Self::ScheduleMaintenance { .. } => {
                format!("organizations/{organization_id}/node-pools/{pool_id}/maintenance")
            }
            Self::CancelMaintenance { .. } => {
                format!("organizations/{organization_id}/node-pools/{pool_id}/maintenance/cancel")
            }
        }
    }

    fn canonical(
        &self,
        organization_id: crate::modules::shared_kernel::domain::OrganizationId,
        pool_id: crate::modules::shared_kernel::domain::NodePoolId,
    ) -> Value {
        match self {
            Self::Create {
                name,
                member_node_ids,
            } => json!({
                "action": "create",
                "organizationId": organization_id,
                "name": name.as_str(),
                "memberNodeIds": member_node_ids,
            }),
            Self::AddMembers {
                expected_version,
                member_node_ids,
            } => json!({
                "action": "addMembers",
                "organizationId": organization_id,
                "nodePoolId": pool_id,
                "expectedVersion": expected_version,
                "memberNodeIds": member_node_ids,
            }),
            Self::RequestMemberRemoval {
                expected_version,
                member_node_ids,
            } => json!({
                "action": "requestMemberRemoval",
                "organizationId": organization_id,
                "nodePoolId": pool_id,
                "expectedVersion": expected_version,
                "memberNodeIds": member_node_ids,
            }),
            Self::ScheduleMaintenance {
                expected_version,
                target_node_ids,
                starts_at,
                ends_at,
                reason,
            } => json!({
                "action": "scheduleMaintenance",
                "organizationId": organization_id,
                "nodePoolId": pool_id,
                "expectedVersion": expected_version,
                "targetNodeIds": target_node_ids,
                "startsAt": starts_at,
                "endsAt": ends_at,
                "reason": reason,
            }),
            Self::CancelMaintenance {
                expected_version,
                maintenance_generation,
            } => json!({
                "action": "cancelMaintenance",
                "organizationId": organization_id,
                "nodePoolId": pool_id,
                "expectedVersion": expected_version,
                "maintenanceGeneration": maintenance_generation,
            }),
        }
    }
}

fn canonical_ids(mut node_ids: Vec<NodeId>) -> Result<Vec<NodeId>, String> {
    node_ids.sort_unstable();
    node_ids.dedup();
    if node_ids.is_empty() || node_ids.iter().any(|node_id| node_id.as_uuid().is_nil()) {
        return Err("node IDs must contain at least one non-nil value".into());
    }
    Ok(node_ids)
}

fn require_version(version: u64) -> Result<(), String> {
    if version == 0 {
        Err("expected version and maintenance generation must be positive".into())
    } else {
        Ok(())
    }
}
