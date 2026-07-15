use crate::modules::fleet::domain::value_objects::{
    NodeAvailability, NodeCapabilities, NodeName, NodeState,
};
use crate::modules::shared_kernel::domain::{NodeId, OrganizationId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub organization_id: OrganizationId,
    pub name: NodeName,
    pub state: NodeState,
    pub agent_instance_id: Uuid,
    pub agent_version: String,
    pub capabilities: NodeCapabilities,
    pub enrolled_at: DateTime<Utc>,
    pub last_observed_at: DateTime<Utc>,
    pub last_sequence: u64,
    pub aggregate_version: u64,
}

impl Node {
    pub fn enroll(
        id: NodeId,
        organization_id: OrganizationId,
        name: NodeName,
        agent_instance_id: Uuid,
        agent_version: impl Into<String>,
        capabilities: NodeCapabilities,
        enrolled_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        if agent_instance_id.is_nil() {
            return Err("agent instance ID must not be nil".into());
        }
        let agent_version = agent_version.into();
        if agent_version.is_empty()
            || agent_version.len() > 255
            || agent_version.contains(['\0', '\r', '\n'])
        {
            return Err("agent version is invalid".into());
        }
        Ok(Self {
            id,
            organization_id,
            name,
            state: NodeState::Pending,
            agent_instance_id,
            agent_version,
            capabilities,
            enrolled_at,
            last_observed_at: enrolled_at,
            last_sequence: 0,
            aggregate_version: 1,
        })
    }

    pub fn mark_ready(&mut self) -> Result<(), String> {
        match self.state {
            NodeState::Pending | NodeState::Draining => {
                self.state = NodeState::Ready;
                self.aggregate_version += 1;
                Ok(())
            }
            NodeState::Ready => Ok(()),
            NodeState::Revoked => Err("revoked node cannot become ready".into()),
        }
    }

    pub fn drain(&mut self) -> Result<(), String> {
        match self.state {
            NodeState::Pending | NodeState::Ready => {
                self.state = NodeState::Draining;
                self.aggregate_version += 1;
                Ok(())
            }
            NodeState::Draining => Ok(()),
            NodeState::Revoked => Err("revoked node cannot drain".into()),
        }
    }

    pub fn revoke(&mut self) {
        if self.state != NodeState::Revoked {
            self.state = NodeState::Revoked;
            self.aggregate_version += 1;
        }
    }

    pub fn availability_at(
        &self,
        now: DateTime<Utc>,
        heartbeat_timeout: chrono::Duration,
    ) -> NodeAvailability {
        if now - self.last_observed_at > heartbeat_timeout {
            NodeAvailability::Offline
        } else {
            NodeAvailability::Online
        }
    }

    pub fn accepts_new_work_at(
        &self,
        now: DateTime<Utc>,
        heartbeat_timeout: chrono::Duration,
    ) -> bool {
        self.state == NodeState::Ready
            && self.availability_at(now, heartbeat_timeout) == NodeAvailability::Online
    }
}
