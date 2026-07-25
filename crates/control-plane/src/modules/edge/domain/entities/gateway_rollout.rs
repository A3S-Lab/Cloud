use crate::modules::edge::domain::{
    GatewayPublication, GatewayPublicationState, GatewayReplicaRecovery, GatewayRolloutPolicy,
    GatewayScope,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, GatewayCertificateId, GatewayRolloutId, GatewayScopeId, NodeCommandId,
    NodeId,
};
use a3s_cloud_contracts::{GatewayAckState, NodeGatewayAck, NodeGatewaySnapshotObservation};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayReplicaRolloutState {
    Pending,
    Applied,
    Rejected,
    Unavailable,
}

impl GatewayReplicaRolloutState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Applied => "applied",
            Self::Rejected => "rejected",
            Self::Unavailable => "unavailable",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "pending" => Ok(Self::Pending),
            "applied" => Ok(Self::Applied),
            "rejected" => Ok(Self::Rejected),
            "unavailable" => Ok(Self::Unavailable),
            _ => Err(format!(
                "unsupported Gateway replica rollout state {value:?}"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayReplicaRollout {
    pub node_id: NodeId,
    pub revision: u64,
    pub command_id: NodeCommandId,
    pub snapshot_digest: String,
    pub snapshot_expires_at: DateTime<Utc>,
    pub gateway_certificate_id: Option<GatewayCertificateId>,
    pub state: GatewayReplicaRolloutState,
    pub failure: Option<String>,
    pub acknowledged_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<GatewayReplicaRecovery>,
}

impl GatewayReplicaRollout {
    fn stage(publication: &GatewayPublication) -> Result<Self, String> {
        publication.snapshot()?;
        if publication.state != GatewayPublicationState::Pending
            || publication.failure.is_some()
            || publication.acknowledged_at.is_some()
        {
            return Err("Gateway rollout publication must be pending".into());
        }
        Ok(Self {
            node_id: publication.node_id,
            revision: publication.revision,
            command_id: publication.command_id,
            snapshot_digest: publication.snapshot_digest.clone(),
            snapshot_expires_at: publication.snapshot_expires_at,
            gateway_certificate_id: publication
                .certificate_request
                .as_ref()
                .map(|request| GatewayCertificateId::from_uuid(request.certificate_id)),
            state: GatewayReplicaRolloutState::Pending,
            failure: None,
            acknowledged_at: None,
            recovery: None,
        })
    }

    fn acknowledge(&mut self, acknowledgement: &NodeGatewayAck) -> Result<bool, String> {
        acknowledgement.validate()?;
        if acknowledgement.node_id != self.node_id.as_uuid()
            || acknowledgement.gateway_id != self.node_id.as_uuid()
            || acknowledgement.command_id != self.command_id.as_uuid()
            || acknowledgement.revision != self.revision
            || acknowledgement.snapshot_digest != self.snapshot_digest
            || acknowledgement.expires_at != self.snapshot_expires_at
        {
            return Err("Gateway acknowledgement does not match its exact rollout replica".into());
        }
        let acknowledged_at = canonical_timestamp(acknowledgement.acknowledged_at);
        let (next, failure) = match acknowledgement.state {
            GatewayAckState::Applied => (GatewayReplicaRolloutState::Applied, None),
            GatewayAckState::Rejected => (
                GatewayReplicaRolloutState::Rejected,
                acknowledgement.message.clone(),
            ),
        };
        if self.state == next
            && self.failure == failure
            && self.acknowledged_at == Some(acknowledged_at)
        {
            return Ok(false);
        }
        if self.state != GatewayReplicaRolloutState::Pending {
            return Err("Gateway rollout replica already has a different terminal outcome".into());
        }
        self.state = next;
        self.failure = failure;
        self.acknowledged_at = Some(acknowledged_at);
        Ok(true)
    }

    fn mark_unavailable(
        &mut self,
        failure: &str,
        observed_at: DateTime<Utc>,
    ) -> Result<bool, String> {
        validate_failure(failure)?;
        let observed_at = canonical_timestamp(observed_at);
        if self.state == GatewayReplicaRolloutState::Unavailable
            && self.failure.as_deref() == Some(failure)
            && self.acknowledged_at == Some(observed_at)
        {
            return Ok(false);
        }
        if self.state != GatewayReplicaRolloutState::Pending {
            return Err("only a pending Gateway rollout replica can become unavailable".into());
        }
        self.state = GatewayReplicaRolloutState::Unavailable;
        self.failure = Some(failure.into());
        self.acknowledged_at = Some(observed_at);
        self.recovery = Some(GatewayReplicaRecovery::required(failure, observed_at)?);
        Ok(true)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayRolloutState {
    Pending,
    Ready,
    Succeeded,
    Degraded,
}

impl GatewayRolloutState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::Succeeded => "succeeded",
            Self::Degraded => "degraded",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "pending" => Ok(Self::Pending),
            "ready" => Ok(Self::Ready),
            "succeeded" => Ok(Self::Succeeded),
            "degraded" => Ok(Self::Degraded),
            _ => Err(format!("unsupported Gateway rollout state {value:?}")),
        }
    }

    pub const fn terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Degraded)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayRollout {
    pub id: GatewayRolloutId,
    pub gateway_scope_id: GatewayScopeId,
    pub membership_generation: u64,
    pub generation: u64,
    pub correlation_id: Uuid,
    pub policy: GatewayRolloutPolicy,
    pub replicas: Vec<GatewayReplicaRollout>,
    pub state: GatewayRolloutState,
    pub ready_replicas: u32,
    pub unavailable_replicas: u32,
    pub aggregate_version: u64,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

impl GatewayRollout {
    pub fn stage(
        id: GatewayRolloutId,
        scope: &GatewayScope,
        generation: u64,
        publications: &[GatewayPublication],
        started_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        Self::stage_with_policy(
            id,
            scope,
            generation,
            scope.rollout_policy,
            publications,
            started_at,
        )
    }

    pub fn stage_rollback(
        id: GatewayRolloutId,
        scope: &GatewayScope,
        generation: u64,
        publications: &[GatewayPublication],
        started_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let desired_replicas = u32::try_from(scope.member_node_ids.len())
            .map_err(|_| "Gateway rollback replica count exceeds supported bounds".to_string())?;
        let policy = GatewayRolloutPolicy::new(desired_replicas, 0, scope.member_node_ids.len())?;
        Self::stage_with_policy(id, scope, generation, policy, publications, started_at)
    }

    fn stage_with_policy(
        id: GatewayRolloutId,
        scope: &GatewayScope,
        generation: u64,
        policy: GatewayRolloutPolicy,
        publications: &[GatewayPublication],
        started_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        scope.validate()?;
        policy.validate(scope.member_node_ids.len())?;
        if id.as_uuid().is_nil() || generation == 0 {
            return Err("Gateway rollout identity and generation must be positive".into());
        }
        if publications.len() != scope.member_node_ids.len() {
            return Err("Gateway rollout must publish to every desired scope member".into());
        }
        let started_at = canonical_timestamp(started_at);
        let correlation_id = publications
            .first()
            .map(|publication| publication.command_correlation_id)
            .ok_or_else(|| "Gateway rollout must contain at least one publication".to_string())?;
        if correlation_id.is_nil()
            || publications.iter().any(|publication| {
                publication.command_correlation_id != correlation_id
                    || publication.command_issued_at != started_at
            })
        {
            return Err(
                "Gateway rollout publications must share one issue time and correlation ID".into(),
            );
        }
        let mut replicas = publications
            .iter()
            .map(GatewayReplicaRollout::stage)
            .collect::<Result<Vec<_>, _>>()?;
        replicas.sort_by_key(|replica| replica.node_id);
        if replicas
            .windows(2)
            .any(|replicas| replicas[0].node_id == replicas[1].node_id)
        {
            return Err("Gateway rollout contains a duplicate physical member".into());
        }
        let mut desired_members = scope.member_node_ids.clone();
        desired_members.sort();
        if replicas
            .iter()
            .map(|replica| replica.node_id)
            .ne(desired_members)
        {
            return Err("Gateway rollout publications do not match scope membership".into());
        }
        let rollout = Self {
            id,
            gateway_scope_id: scope.id,
            membership_generation: scope.membership_generation,
            generation,
            correlation_id,
            policy,
            replicas,
            state: GatewayRolloutState::Pending,
            ready_replicas: 0,
            unavailable_replicas: 0,
            aggregate_version: 1,
            started_at,
            completed_at: None,
        };
        rollout.validate()?;
        Ok(rollout)
    }

    pub fn acknowledge(&mut self, acknowledgement: &NodeGatewayAck) -> Result<bool, String> {
        let acknowledged_at = canonical_timestamp(acknowledgement.acknowledged_at);
        if acknowledged_at < self.started_at {
            return Err("Gateway rollout acknowledgement predates rollout staging".into());
        }
        if self.state.terminal() {
            return self.replay_terminal_acknowledgement(acknowledgement);
        }
        let mut next = self.clone();
        let replica = next
            .replicas
            .iter_mut()
            .find(|replica| {
                replica.node_id.as_uuid() == acknowledgement.node_id
                    && replica.command_id.as_uuid() == acknowledgement.command_id
            })
            .ok_or_else(|| "Gateway acknowledgement does not belong to this rollout".to_string())?;
        if !replica.acknowledge(acknowledgement)? {
            return Ok(false);
        }
        let next_version = next
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Gateway rollout version space is exhausted".to_string())?;
        next.recompute(acknowledged_at)?;
        next.aggregate_version = next_version;
        next.validate()?;
        *self = next;
        Ok(true)
    }

    pub fn mark_unavailable(
        &mut self,
        node_id: NodeId,
        failure: &str,
        observed_at: DateTime<Utc>,
    ) -> Result<bool, String> {
        if self.state.terminal() {
            return Err("terminal Gateway rollout cannot accept a new unavailable result".into());
        }
        let observed_at = canonical_timestamp(observed_at);
        if observed_at < self.started_at {
            return Err("Gateway rollout unavailability predates rollout staging".into());
        }
        let mut next = self.clone();
        let replica = next
            .replicas
            .iter_mut()
            .find(|replica| replica.node_id == node_id)
            .ok_or_else(|| "Gateway rollout does not contain this member".to_string())?;
        if !replica.mark_unavailable(failure, observed_at)? {
            return Ok(false);
        }
        let next_version = next
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Gateway rollout version space is exhausted".to_string())?;
        next.recompute(observed_at)?;
        next.aggregate_version = next_version;
        next.validate()?;
        *self = next;
        Ok(true)
    }

    pub fn stage_recovery_observation(
        &mut self,
        node_id: NodeId,
        command_id: NodeCommandId,
        issued_at: DateTime<Utc>,
        not_after: DateTime<Utc>,
    ) -> Result<bool, String> {
        let mut next = self.clone();
        let replica = next.unavailable_replica_mut(node_id)?;
        let recovery = replica
            .recovery
            .as_mut()
            .ok_or_else(|| "unavailable Gateway replica omitted its recovery state".to_string())?;
        if !recovery.stage_observation(command_id, issued_at, not_after)? {
            return Ok(false);
        }
        next.advance_version()?;
        next.validate()?;
        *self = next;
        Ok(true)
    }

    pub fn record_recovery_observation(
        &mut self,
        node_id: NodeId,
        candidate: &GatewayPublication,
        prior: Option<&GatewayPublication>,
        observation: NodeGatewaySnapshotObservation,
    ) -> Result<bool, String> {
        let mut next = self.clone();
        let replica = next.unavailable_replica_mut(node_id)?;
        if candidate.node_id != replica.node_id
            || candidate.revision != replica.revision
            || candidate.command_id != replica.command_id
            || candidate.snapshot_digest != replica.snapshot_digest
            || candidate.snapshot_expires_at != replica.snapshot_expires_at
        {
            return Err("Gateway recovery candidate does not match its rollout replica".into());
        }
        let recovery = replica
            .recovery
            .as_mut()
            .ok_or_else(|| "unavailable Gateway replica omitted its recovery state".to_string())?;
        if !recovery.record_observation(node_id, candidate, prior, observation)? {
            return Ok(false);
        }
        next.advance_version()?;
        next.validate()?;
        *self = next;
        Ok(true)
    }

    pub fn record_recovery_command_failure(
        &mut self,
        node_id: NodeId,
        command_id: NodeCommandId,
        failure: impl Into<String>,
        retryable: bool,
        failed_at: DateTime<Utc>,
    ) -> Result<bool, String> {
        let mut next = self.clone();
        let replica = next.unavailable_replica_mut(node_id)?;
        let recovery = replica
            .recovery
            .as_mut()
            .ok_or_else(|| "unavailable Gateway replica omitted its recovery state".to_string())?;
        if !recovery.record_command_failure(command_id, failure, retryable, failed_at)? {
            return Ok(false);
        }
        next.advance_version()?;
        next.validate()?;
        *self = next;
        Ok(true)
    }

    pub fn required_ready(&self) -> Result<u32, String> {
        self.policy.required_ready(self.replicas.len())
    }

    pub fn serves_traffic(&self) -> Result<bool, String> {
        Ok(self.ready_replicas >= self.required_ready()?)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.id.as_uuid().is_nil()
            || self.gateway_scope_id.as_uuid().is_nil()
            || self.membership_generation == 0
            || self.generation == 0
            || self.correlation_id.is_nil()
            || self.aggregate_version == 0
        {
            return Err("Gateway rollout identity is invalid".into());
        }
        self.policy.validate(self.replicas.len())?;
        if self
            .replicas
            .windows(2)
            .any(|replicas| replicas[0].node_id >= replicas[1].node_id)
            || self.replicas.iter().any(|replica| {
                replica.node_id.as_uuid().is_nil()
                    || replica.command_id.as_uuid().is_nil()
                    || replica.revision == 0
                    || !valid_sha256(&replica.snapshot_digest)
                    || replica.snapshot_expires_at <= self.started_at
                    || replica
                        .gateway_certificate_id
                        .is_some_and(|certificate_id| certificate_id.as_uuid().is_nil())
                    || !replica_state_is_consistent(replica)
            })
        {
            return Err("Gateway rollout replica projection is invalid".into());
        }
        let ready_replicas = count_replicas(&self.replicas, GatewayReplicaRolloutState::Applied)?;
        let unavailable_replicas = u32::try_from(
            self.replicas
                .iter()
                .filter(|replica| {
                    matches!(
                        replica.state,
                        GatewayReplicaRolloutState::Rejected
                            | GatewayReplicaRolloutState::Unavailable
                    )
                })
                .count(),
        )
        .map_err(|_| "Gateway unavailable replica count exceeds supported bounds".to_string())?;
        if self.ready_replicas != ready_replicas
            || self.unavailable_replicas != unavailable_replicas
        {
            return Err("Gateway rollout aggregate counters are inconsistent".into());
        }
        let pending = self
            .replicas
            .iter()
            .any(|replica| replica.state == GatewayReplicaRolloutState::Pending);
        let required_ready = self.required_ready()?;
        let state_is_consistent = match self.state {
            GatewayRolloutState::Pending => {
                pending && ready_replicas < required_ready && self.completed_at.is_none()
            }
            GatewayRolloutState::Ready => {
                pending
                    && ready_replicas >= required_ready
                    && ready_replicas < u32::try_from(self.replicas.len()).unwrap_or(u32::MAX)
                    && self.completed_at.is_none()
            }
            GatewayRolloutState::Succeeded => {
                !pending
                    && unavailable_replicas == 0
                    && ready_replicas == u32::try_from(self.replicas.len()).unwrap_or(u32::MAX)
                    && self.completed_at.is_some()
            }
            GatewayRolloutState::Degraded => {
                !pending && unavailable_replicas > 0 && self.completed_at.is_some()
            }
        };
        if !state_is_consistent
            || self
                .completed_at
                .is_some_and(|completed_at| completed_at < self.started_at)
        {
            return Err("Gateway rollout aggregate state is inconsistent".into());
        }
        Ok(())
    }

    fn recompute(&mut self, observed_at: DateTime<Utc>) -> Result<(), String> {
        self.ready_replicas = count_replicas(&self.replicas, GatewayReplicaRolloutState::Applied)?;
        self.unavailable_replicas = u32::try_from(
            self.replicas
                .iter()
                .filter(|replica| {
                    matches!(
                        replica.state,
                        GatewayReplicaRolloutState::Rejected
                            | GatewayReplicaRolloutState::Unavailable
                    )
                })
                .count(),
        )
        .map_err(|_| "Gateway unavailable replica count exceeds supported bounds".to_string())?;
        let pending = self
            .replicas
            .iter()
            .any(|replica| replica.state == GatewayReplicaRolloutState::Pending);
        if !pending {
            self.state = if self.unavailable_replicas == 0 {
                GatewayRolloutState::Succeeded
            } else {
                GatewayRolloutState::Degraded
            };
            self.completed_at = Some(canonical_timestamp(observed_at));
        } else if self.ready_replicas >= self.required_ready()? {
            self.state = GatewayRolloutState::Ready;
            self.completed_at = None;
        } else {
            self.state = GatewayRolloutState::Pending;
            self.completed_at = None;
        }
        Ok(())
    }

    fn replay_terminal_acknowledgement(
        &mut self,
        acknowledgement: &NodeGatewayAck,
    ) -> Result<bool, String> {
        let replica = self
            .replicas
            .iter_mut()
            .find(|replica| {
                replica.node_id.as_uuid() == acknowledgement.node_id
                    && replica.command_id.as_uuid() == acknowledgement.command_id
            })
            .ok_or_else(|| "Gateway acknowledgement does not belong to this rollout".to_string())?;
        if replica.acknowledge(acknowledgement)? {
            return Err("terminal Gateway rollout changed during acknowledgement replay".into());
        }
        Ok(false)
    }

    fn unavailable_replica_mut(
        &mut self,
        node_id: NodeId,
    ) -> Result<&mut GatewayReplicaRollout, String> {
        let replica = self
            .replicas
            .iter_mut()
            .find(|replica| replica.node_id == node_id)
            .ok_or_else(|| "Gateway rollout does not contain this member".to_string())?;
        if replica.state != GatewayReplicaRolloutState::Unavailable {
            return Err("only an unavailable Gateway replica can recover physical state".into());
        }
        Ok(replica)
    }

    fn advance_version(&mut self) -> Result<(), String> {
        self.aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Gateway rollout version space is exhausted".to_string())?;
        Ok(())
    }
}

fn replica_state_is_consistent(replica: &GatewayReplicaRollout) -> bool {
    match replica.state {
        GatewayReplicaRolloutState::Pending => {
            replica.failure.is_none()
                && replica.acknowledged_at.is_none()
                && replica.recovery.is_none()
        }
        GatewayReplicaRolloutState::Applied => {
            replica.failure.is_none()
                && replica.acknowledged_at.is_some()
                && replica.recovery.is_none()
        }
        GatewayReplicaRolloutState::Rejected => {
            replica.failure.is_some()
                && replica.acknowledged_at.is_some()
                && replica.recovery.is_none()
        }
        GatewayReplicaRolloutState::Unavailable => {
            replica.failure.is_some()
                && replica.acknowledged_at.is_some()
                && replica
                    .recovery
                    .as_ref()
                    .is_some_and(|recovery| recovery.validate().is_ok())
        }
    }
}

fn count_replicas(
    replicas: &[GatewayReplicaRollout],
    expected: GatewayReplicaRolloutState,
) -> Result<u32, String> {
    u32::try_from(
        replicas
            .iter()
            .filter(|replica| replica.state == expected)
            .count(),
    )
    .map_err(|_| "Gateway replica count exceeds supported bounds".into())
}

fn validate_failure(failure: &str) -> Result<(), String> {
    if failure.is_empty()
        || failure.len() > 16 * 1024
        || failure.contains(['\0', '\r', '\n'])
        || failure.trim() != failure
    {
        return Err("Gateway rollout failure must be a bounded single-line value".into());
    }
    Ok(())
}

fn valid_sha256(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}
