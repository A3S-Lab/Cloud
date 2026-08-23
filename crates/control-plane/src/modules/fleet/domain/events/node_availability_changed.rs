use crate::modules::fleet::domain::entities::Node;
use crate::modules::fleet::domain::value_objects::NodeState;
use crate::modules::shared_kernel::domain::{canonical_timestamp, NodeId, OrganizationId};
use a3s_cloud_contracts::DomainEventEnvelope;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const NODE_UNAVAILABLE_EVENT_KEY: &str = "fleet.node.unavailable";
pub const NODE_AVAILABILITY_RESOLVED_EVENT_KEY: &str = "fleet.node.availability-resolved";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeAvailabilityFactStatus {
    Unavailable,
    Resolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeAvailabilityResolutionReason {
    HeartbeatRestored,
    NodeRevoked,
}

impl NodeAvailabilityResolutionReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HeartbeatRestored => "heartbeat_restored",
            Self::NodeRevoked => "node_revoked",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeAvailabilitySnapshot {
    pub organization_id: OrganizationId,
    pub node_id: NodeId,
    pub state: NodeState,
    pub node_aggregate_version: u64,
    pub last_observed_at: DateTime<Utc>,
}

impl NodeAvailabilitySnapshot {
    pub fn from_node(node: &Node) -> Self {
        Self {
            organization_id: node.organization_id,
            node_id: node.id,
            state: node.state,
            node_aggregate_version: node.aggregate_version,
            last_observed_at: node.last_observed_at,
        }
    }

    fn validate(self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.node_id.as_uuid().is_nil()
            || self.node_aggregate_version == 0
            || canonical_timestamp(self.last_observed_at) != self.last_observed_at
        {
            return Err("Node availability snapshot identity is inconsistent".into());
        }
        Ok(())
    }

    const fn participates(self) -> bool {
        matches!(self.state, NodeState::Ready | NodeState::Draining)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeAvailabilityFiring {
    pub organization_id: OrganizationId,
    pub node_id: NodeId,
    pub event_id: Uuid,
    pub phase_version: u64,
    pub node_aggregate_version: u64,
    pub last_observed_at: DateTime<Utc>,
    pub timeout_deadline_at: DateTime<Utc>,
    pub detected_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeAvailabilityChanged {
    pub organization_id: OrganizationId,
    pub node_id: NodeId,
    pub node_aggregate_version: u64,
    pub availability_phase: u64,
    pub status: NodeAvailabilityFactStatus,
    pub resolution_reason: Option<NodeAvailabilityResolutionReason>,
    pub last_observed_at: DateTime<Utc>,
    pub timeout_deadline_at: DateTime<Utc>,
    pub detected_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

impl NodeAvailabilityChanged {
    pub fn unavailable_envelope(
        snapshot: NodeAvailabilitySnapshot,
        timeout_deadline_at: DateTime<Utc>,
        detected_at: DateTime<Utc>,
    ) -> Result<DomainEventEnvelope, String> {
        snapshot.validate()?;
        let timeout_deadline_at = canonical_timestamp(timeout_deadline_at);
        let detected_at = canonical_timestamp(detected_at);
        if !snapshot.participates()
            || timeout_deadline_at <= snapshot.last_observed_at
            || detected_at <= timeout_deadline_at
        {
            return Err("Node unavailable fact does not cross its strict deadline".into());
        }
        let availability_phase = node_availability_phase_version(
            snapshot.node_aggregate_version,
            NodeAvailabilityFactStatus::Unavailable,
        )?;
        let event_id = Self::deterministic_event_id(
            snapshot.node_id,
            NODE_UNAVAILABLE_EVENT_KEY,
            availability_phase,
        );
        let payload = Self {
            organization_id: snapshot.organization_id,
            node_id: snapshot.node_id,
            node_aggregate_version: snapshot.node_aggregate_version,
            availability_phase,
            status: NodeAvailabilityFactStatus::Unavailable,
            resolution_reason: None,
            last_observed_at: snapshot.last_observed_at,
            timeout_deadline_at,
            detected_at,
            resolved_at: None,
        };
        Ok(DomainEventEnvelope {
            event_id,
            event_key: NODE_UNAVAILABLE_EVENT_KEY.into(),
            schema_version: 1,
            organization_id: snapshot.organization_id.as_uuid(),
            aggregate_id: snapshot.node_id.as_uuid(),
            aggregate_version: availability_phase,
            occurred_at: detected_at,
            correlation_id: event_id,
            causation_id: None,
            payload: serde_json::to_value(payload)
                .map_err(|error| format!("could not encode Node unavailable fact: {error}"))?,
        })
    }

    pub fn resolved_envelope(
        snapshot: NodeAvailabilitySnapshot,
        firing: NodeAvailabilityFiring,
        reason: NodeAvailabilityResolutionReason,
        resolved_at: DateTime<Utc>,
    ) -> Result<DomainEventEnvelope, String> {
        snapshot.validate()?;
        let resolved_at = canonical_timestamp(resolved_at);
        let valid_transition = match reason {
            NodeAvailabilityResolutionReason::HeartbeatRestored => {
                snapshot.participates() && snapshot.last_observed_at > firing.last_observed_at
            }
            NodeAvailabilityResolutionReason::NodeRevoked => {
                snapshot.state == NodeState::Revoked
                    && snapshot.last_observed_at == firing.last_observed_at
            }
        };
        if firing.organization_id != snapshot.organization_id
            || firing.node_id != snapshot.node_id
            || firing.event_id.is_nil()
            || firing.phase_version == 0
            || firing.node_aggregate_version == 0
            || canonical_timestamp(firing.last_observed_at) != firing.last_observed_at
            || canonical_timestamp(firing.timeout_deadline_at) != firing.timeout_deadline_at
            || canonical_timestamp(firing.detected_at) != firing.detected_at
            || firing.timeout_deadline_at <= firing.last_observed_at
            || firing.detected_at <= firing.timeout_deadline_at
            || resolved_at < firing.detected_at
            || !valid_transition
        {
            return Err("Node availability resolution does not match its firing".into());
        }
        let availability_phase = node_availability_phase_version(
            snapshot.node_aggregate_version,
            NodeAvailabilityFactStatus::Resolved,
        )?;
        if availability_phase <= firing.phase_version {
            return Err("Node availability resolution does not advance its firing".into());
        }
        let event_id = Self::deterministic_event_id(
            snapshot.node_id,
            NODE_AVAILABILITY_RESOLVED_EVENT_KEY,
            availability_phase,
        );
        let payload = Self {
            organization_id: snapshot.organization_id,
            node_id: snapshot.node_id,
            node_aggregate_version: snapshot.node_aggregate_version,
            availability_phase,
            status: NodeAvailabilityFactStatus::Resolved,
            resolution_reason: Some(reason),
            last_observed_at: snapshot.last_observed_at,
            timeout_deadline_at: firing.timeout_deadline_at,
            detected_at: firing.detected_at,
            resolved_at: Some(resolved_at),
        };
        Ok(DomainEventEnvelope {
            event_id,
            event_key: NODE_AVAILABILITY_RESOLVED_EVENT_KEY.into(),
            schema_version: 1,
            organization_id: snapshot.organization_id.as_uuid(),
            aggregate_id: snapshot.node_id.as_uuid(),
            aggregate_version: availability_phase,
            occurred_at: resolved_at,
            correlation_id: firing.event_id,
            causation_id: Some(firing.event_id),
            payload: serde_json::to_value(payload).map_err(|error| {
                format!("could not encode Node availability resolution: {error}")
            })?,
        })
    }

    pub fn firing(event: &DomainEventEnvelope) -> Result<NodeAvailabilityFiring, String> {
        let payload = Self::decode_envelope(event)?;
        if payload.status != NodeAvailabilityFactStatus::Unavailable {
            return Err("Node availability firing selected a resolution fact".into());
        }
        Ok(NodeAvailabilityFiring {
            organization_id: payload.organization_id,
            node_id: payload.node_id,
            event_id: event.event_id,
            phase_version: payload.availability_phase,
            node_aggregate_version: payload.node_aggregate_version,
            last_observed_at: payload.last_observed_at,
            timeout_deadline_at: payload.timeout_deadline_at,
            detected_at: payload.detected_at,
        })
    }

    pub fn decode_envelope(event: &DomainEventEnvelope) -> Result<Self, String> {
        let expected_status = match event.event_key.as_str() {
            NODE_UNAVAILABLE_EVENT_KEY => NodeAvailabilityFactStatus::Unavailable,
            NODE_AVAILABILITY_RESOLVED_EVENT_KEY => NodeAvailabilityFactStatus::Resolved,
            _ => return Err("Node availability event key is unsupported".into()),
        };
        let payload: Self = serde_json::from_value(event.payload.clone())
            .map_err(|error| format!("Node availability payload is invalid: {error}"))?;
        let expected_phase =
            node_availability_phase_version(payload.node_aggregate_version, expected_status)?;
        let phase_shape_is_valid = match expected_status {
            NodeAvailabilityFactStatus::Unavailable => {
                payload.resolution_reason.is_none()
                    && payload.resolved_at.is_none()
                    && payload.timeout_deadline_at > payload.last_observed_at
                    && event.causation_id.is_none()
                    && event.correlation_id == event.event_id
                    && event.occurred_at == payload.detected_at
            }
            NodeAvailabilityFactStatus::Resolved => {
                payload.resolution_reason.is_some()
                    && payload.resolved_at == Some(event.occurred_at)
                    && match payload.resolution_reason {
                        Some(NodeAvailabilityResolutionReason::HeartbeatRestored) => {
                            // Schema v1 carries the current observation and the firing deadline,
                            // but not the firing observation. Its strict advancement is therefore
                            // enforced while producing the owner fact and cannot be reconstructed
                            // from this envelope alone. Node clocks may also lead control-plane
                            // receipt time, so no current-observation ordering is inferred here.
                            true
                        }
                        Some(NodeAvailabilityResolutionReason::NodeRevoked) => {
                            payload.timeout_deadline_at > payload.last_observed_at
                        }
                        None => false,
                    }
                    && event.causation_id == Some(event.correlation_id)
                    && !event.correlation_id.is_nil()
            }
        };
        if event.schema_version != 1
            || event.event_id.is_nil()
            || event.organization_id.is_nil()
            || event.aggregate_id.is_nil()
            || canonical_timestamp(event.occurred_at) != event.occurred_at
            || payload.organization_id.as_uuid() != event.organization_id
            || payload.node_id.as_uuid() != event.aggregate_id
            || payload.organization_id.as_uuid().is_nil()
            || payload.node_id.as_uuid().is_nil()
            || payload.status != expected_status
            || payload.availability_phase != expected_phase
            || event.aggregate_version != expected_phase
            || canonical_timestamp(payload.last_observed_at) != payload.last_observed_at
            || canonical_timestamp(payload.timeout_deadline_at) != payload.timeout_deadline_at
            || canonical_timestamp(payload.detected_at) != payload.detected_at
            || payload.detected_at <= payload.timeout_deadline_at
            || payload.resolved_at.is_some_and(|resolved_at| {
                canonical_timestamp(resolved_at) != resolved_at || resolved_at < payload.detected_at
            })
            || !phase_shape_is_valid
            || Self::deterministic_event_id(payload.node_id, &event.event_key, expected_phase)
                != event.event_id
        {
            return Err("Node availability event identity is inconsistent".into());
        }
        Ok(payload)
    }

    pub fn deterministic_event_id(node_id: NodeId, event_key: &str, phase_version: u64) -> Uuid {
        Uuid::new_v5(
            &node_id.as_uuid(),
            format!("{event_key}:{phase_version}").as_bytes(),
        )
    }
}

pub fn node_availability_phase_version(
    node_aggregate_version: u64,
    status: NodeAvailabilityFactStatus,
) -> Result<u64, String> {
    let doubled = node_aggregate_version
        .checked_mul(2)
        .filter(|version| *version <= i64::MAX as u64)
        .ok_or_else(|| "Node availability phase exceeds supported range".to_owned())?;
    match status {
        NodeAvailabilityFactStatus::Unavailable if doubled > 0 => Ok(doubled),
        NodeAvailabilityFactStatus::Resolved => doubled
            .checked_sub(1)
            .filter(|version| *version > 0)
            .ok_or_else(|| "Node availability phase must be positive".to_owned()),
        NodeAvailabilityFactStatus::Unavailable => {
            Err("Node availability phase must be positive".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn snapshot(version: u64, observed_at: DateTime<Utc>) -> NodeAvailabilitySnapshot {
        NodeAvailabilitySnapshot {
            organization_id: OrganizationId::new(),
            node_id: NodeId::new(),
            state: NodeState::Ready,
            node_aggregate_version: version,
            last_observed_at: observed_at,
        }
    }

    #[test]
    fn strict_deadline_and_phase_encoding_order_repeated_lifecycles() {
        let observed_at = canonical_timestamp(Utc::now());
        let deadline = observed_at + Duration::seconds(30);
        let first = snapshot(2, observed_at);
        assert!(NodeAvailabilityChanged::unavailable_envelope(first, deadline, deadline).is_err());

        let firing = NodeAvailabilityChanged::unavailable_envelope(
            first,
            deadline,
            deadline + Duration::microseconds(1),
        )
        .expect("strictly overdue firing");
        assert_eq!(firing.aggregate_version, 4);
        assert_eq!(
            NodeAvailabilityChanged::decode_envelope(&firing)
                .expect("typed firing")
                .status,
            NodeAvailabilityFactStatus::Unavailable
        );

        let firing_identity = NodeAvailabilityChanged::firing(&firing).expect("open firing");
        let recovered = NodeAvailabilitySnapshot {
            last_observed_at: observed_at + Duration::seconds(40),
            node_aggregate_version: 3,
            ..first
        };
        let resolution = NodeAvailabilityChanged::resolved_envelope(
            recovered,
            firing_identity,
            NodeAvailabilityResolutionReason::HeartbeatRestored,
            deadline + Duration::seconds(11),
        )
        .expect("heartbeat resolution");
        assert_eq!(resolution.aggregate_version, 5);
        assert_eq!(resolution.causation_id, Some(firing.event_id));
        assert_eq!(
            NodeAvailabilityChanged::decode_envelope(&resolution)
                .expect("typed heartbeat resolution")
                .resolution_reason,
            Some(NodeAvailabilityResolutionReason::HeartbeatRestored)
        );

        let delayed_observation = NodeAvailabilitySnapshot {
            last_observed_at: observed_at + Duration::seconds(5),
            node_aggregate_version: 3,
            ..first
        };
        let delayed_resolution = NodeAvailabilityChanged::resolved_envelope(
            delayed_observation,
            firing_identity,
            NodeAvailabilityResolutionReason::HeartbeatRestored,
            deadline + Duration::seconds(12),
        )
        .expect("delayed heartbeat still advances the firing observation");
        assert_eq!(
            NodeAvailabilityChanged::decode_envelope(&delayed_resolution)
                .expect("typed delayed-heartbeat resolution")
                .last_observed_at,
            delayed_observation.last_observed_at
        );

        let clock_ahead_observation = NodeAvailabilitySnapshot {
            last_observed_at: observed_at + Duration::seconds(50),
            ..delayed_observation
        };
        let clock_ahead_resolution = NodeAvailabilityChanged::resolved_envelope(
            clock_ahead_observation,
            firing_identity,
            NodeAvailabilityResolutionReason::HeartbeatRestored,
            deadline + Duration::seconds(13),
        )
        .expect("advancing heartbeat may carry a clock-ahead observation");
        NodeAvailabilityChanged::decode_envelope(&clock_ahead_resolution)
            .expect("typed clock-ahead heartbeat resolution");

        let second = NodeAvailabilityChanged::unavailable_envelope(
            recovered,
            recovered.last_observed_at + Duration::seconds(30),
            recovered.last_observed_at + Duration::seconds(31),
        )
        .expect("second firing");
        assert_eq!(second.aggregate_version, 6);
        assert!(firing.aggregate_version < resolution.aggregate_version);
        assert!(resolution.aggregate_version < second.aggregate_version);
        assert_ne!(firing.event_id, second.event_id);
    }

    #[test]
    fn resolution_requires_a_real_heartbeat_or_explicit_revocation() {
        let observed_at = canonical_timestamp(Utc::now());
        let active = snapshot(2, observed_at);
        let firing = NodeAvailabilityChanged::unavailable_envelope(
            active,
            observed_at + Duration::seconds(10),
            observed_at + Duration::seconds(11),
        )
        .expect("firing");
        let open = NodeAvailabilityChanged::firing(&firing).expect("open firing");

        let state_only = NodeAvailabilitySnapshot {
            state: NodeState::Draining,
            node_aggregate_version: 3,
            ..active
        };
        assert!(NodeAvailabilityChanged::resolved_envelope(
            state_only,
            open,
            NodeAvailabilityResolutionReason::HeartbeatRestored,
            observed_at + Duration::seconds(12),
        )
        .is_err());

        let revoked = NodeAvailabilitySnapshot {
            state: NodeState::Revoked,
            node_aggregate_version: 3,
            ..active
        };
        let resolution = NodeAvailabilityChanged::resolved_envelope(
            revoked,
            open,
            NodeAvailabilityResolutionReason::NodeRevoked,
            observed_at + Duration::seconds(12),
        )
        .expect("revocation resolution");
        assert_eq!(resolution.aggregate_version, 5);
        assert_eq!(
            NodeAvailabilityChanged::decode_envelope(&resolution)
                .expect("typed resolution")
                .resolution_reason,
            Some(NodeAvailabilityResolutionReason::NodeRevoked)
        );
    }

    #[test]
    fn payload_is_closed_and_excludes_private_node_material() {
        let observed_at = canonical_timestamp(Utc::now());
        let event = NodeAvailabilityChanged::unavailable_envelope(
            snapshot(2, observed_at),
            observed_at + Duration::seconds(10),
            observed_at + Duration::seconds(11),
        )
        .expect("firing");
        let encoded = event.payload.to_string().to_ascii_lowercase();
        for forbidden in [
            "capabilities",
            "inventory",
            "command",
            "log",
            "metric",
            "provider",
            "credential",
            "diagnostic",
            "agentversion",
        ] {
            assert!(!encoded.contains(forbidden), "payload leaked {forbidden}");
        }
    }
}
