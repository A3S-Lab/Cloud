use crate::modules::edge::domain::GatewayPublication;
use crate::modules::shared_kernel::domain::{canonical_timestamp, NodeCommandId, NodeId};
use a3s_cloud_contracts::{
    AppliedGatewaySnapshot, GatewaySnapshotObservationState, NodeGatewaySnapshotObservation,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const APPLYING_FAILURE: &str = "Gateway snapshot observation remained in progress";
const DIVERGED_FAILURE: &str =
    "Gateway applied state does not match the candidate or its known prior publication";
const MAX_RECOVERY_FAILURE_BYTES: usize = 4 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayReplicaRecoveryState {
    Required,
    Observing,
    Observed,
    Diverged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayReplicaRecovery {
    pub state: GatewayReplicaRecoveryState,
    pub attempt: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_id: Option<NodeCommandId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_issued_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_not_after: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation: Option<NodeGatewaySnapshotObservation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
    pub updated_at: DateTime<Utc>,
}

impl GatewayReplicaRecovery {
    pub fn required(
        failure: impl Into<String>,
        required_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let recovery = Self {
            state: GatewayReplicaRecoveryState::Required,
            attempt: 0,
            command_id: None,
            command_issued_at: None,
            command_not_after: None,
            observation: None,
            failure: Some(validate_failure(failure.into())?),
            updated_at: canonical_timestamp(required_at),
        };
        recovery.validate()?;
        Ok(recovery)
    }

    pub fn stage_observation(
        &mut self,
        command_id: NodeCommandId,
        issued_at: DateTime<Utc>,
        not_after: DateTime<Utc>,
    ) -> Result<bool, String> {
        if self.state != GatewayReplicaRecoveryState::Required {
            return Err("only a required Gateway recovery can stage an observation".into());
        }
        let issued_at = canonical_timestamp(issued_at);
        let not_after = canonical_timestamp(not_after);
        if command_id.as_uuid().is_nil() || issued_at < self.updated_at || not_after <= issued_at {
            return Err("Gateway recovery observation command is invalid".into());
        }
        let attempt = self
            .attempt
            .checked_add(1)
            .ok_or_else(|| "Gateway recovery observation attempt space is exhausted".to_string())?;
        self.state = GatewayReplicaRecoveryState::Observing;
        self.attempt = attempt;
        self.command_id = Some(command_id);
        self.command_issued_at = Some(issued_at);
        self.command_not_after = Some(not_after);
        self.observation = None;
        self.failure = None;
        self.updated_at = issued_at;
        self.validate()?;
        Ok(true)
    }

    pub fn record_observation(
        &mut self,
        node_id: NodeId,
        candidate: &GatewayPublication,
        prior: Option<&GatewayPublication>,
        observation: NodeGatewaySnapshotObservation,
    ) -> Result<bool, String> {
        let observation = canonicalize_observation(observation);
        if self.state != GatewayReplicaRecoveryState::Observing {
            return self.replay_observation(node_id, candidate, prior, &observation);
        }
        self.validate_observation_identity(node_id, candidate, &observation)?;
        if observation.observed_at < self.updated_at {
            return Err("Gateway recovery observation time regressed".into());
        }

        self.updated_at = observation.observed_at;
        self.observation = Some(observation.clone());
        match observation.state {
            GatewaySnapshotObservationState::Applying => {
                self.state = GatewayReplicaRecoveryState::Required;
                self.failure = Some(APPLYING_FAILURE.into());
            }
            _ if observed_state_is_known(candidate, prior, &observation)? => {
                self.state = GatewayReplicaRecoveryState::Observed;
                self.failure = None;
            }
            _ => {
                self.state = GatewayReplicaRecoveryState::Diverged;
                self.failure = Some(DIVERGED_FAILURE.into());
            }
        }
        self.validate()?;
        Ok(true)
    }

    pub fn record_command_failure(
        &mut self,
        command_id: NodeCommandId,
        failure: impl Into<String>,
        retryable: bool,
        failed_at: DateTime<Utc>,
    ) -> Result<bool, String> {
        let failed_at = canonical_timestamp(failed_at);
        let failure = validate_failure(failure.into())?;
        if self.state == GatewayReplicaRecoveryState::Required
            && self.command_id == Some(command_id)
            && self.failure.as_deref() == Some(failure.as_str())
            && self.updated_at == failed_at
        {
            return Ok(false);
        }
        if self.state == GatewayReplicaRecoveryState::Diverged
            && self.command_id == Some(command_id)
            && self.failure.as_deref() == Some(failure.as_str())
            && self.updated_at == failed_at
        {
            return Ok(false);
        }
        if self.state != GatewayReplicaRecoveryState::Observing
            || self.command_id != Some(command_id)
            || failed_at < self.updated_at
        {
            return Err(
                "Gateway recovery command failure does not match its active attempt".into(),
            );
        }
        self.state = if retryable {
            GatewayReplicaRecoveryState::Required
        } else {
            GatewayReplicaRecoveryState::Diverged
        };
        self.observation = None;
        self.failure = Some(failure);
        self.updated_at = failed_at;
        self.validate()?;
        Ok(true)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.failure.as_deref().is_some_and(|failure| {
            failure.is_empty()
                || failure.len() > MAX_RECOVERY_FAILURE_BYTES
                || failure.contains(['\0', '\r', '\n'])
                || failure.trim() != failure
        }) {
            return Err("Gateway recovery failure must be a bounded single-line value".into());
        }
        if self.attempt == 0 {
            if self.state != GatewayReplicaRecoveryState::Required
                || self.command_id.is_some()
                || self.command_issued_at.is_some()
                || self.command_not_after.is_some()
                || self.observation.is_some()
                || self.failure.is_none()
            {
                return Err("initial Gateway recovery state is inconsistent".into());
            }
            return Ok(());
        }
        let (Some(command_id), Some(issued_at), Some(not_after)) = (
            self.command_id,
            self.command_issued_at,
            self.command_not_after,
        ) else {
            return Err("Gateway recovery attempt omitted its command identity".into());
        };
        if command_id.as_uuid().is_nil() || not_after <= issued_at || self.updated_at < issued_at {
            return Err("Gateway recovery command validity is inconsistent".into());
        }
        if let Some(observation) = &self.observation {
            observation.validate()?;
            if observation.command_id != command_id.as_uuid()
                || canonical_timestamp(observation.observed_at) != self.updated_at
            {
                return Err("Gateway recovery observation evidence is inconsistent".into());
            }
        }
        match self.state {
            GatewayReplicaRecoveryState::Required => {
                if self.failure.is_none()
                    || self.observation.as_ref().is_some_and(|observation| {
                        observation.state != GatewaySnapshotObservationState::Applying
                    })
                {
                    return Err("required Gateway recovery evidence is inconsistent".into());
                }
            }
            GatewayReplicaRecoveryState::Observing => {
                if self.observation.is_some()
                    || self.failure.is_some()
                    || self.updated_at != issued_at
                {
                    return Err("active Gateway recovery observation is inconsistent".into());
                }
            }
            GatewayReplicaRecoveryState::Observed => {
                if self.observation.is_none() || self.failure.is_some() {
                    return Err("observed Gateway recovery evidence is incomplete".into());
                }
            }
            GatewayReplicaRecoveryState::Diverged => {
                if self.failure.is_none() {
                    return Err("diverged Gateway recovery requires a bounded failure".into());
                }
            }
        }
        Ok(())
    }

    fn validate_observation_identity(
        &self,
        node_id: NodeId,
        candidate: &GatewayPublication,
        observation: &NodeGatewaySnapshotObservation,
    ) -> Result<(), String> {
        let command_id = self
            .command_id
            .ok_or_else(|| "Gateway recovery observation command is missing".to_string())?;
        let command_not_after = self
            .command_not_after
            .ok_or_else(|| "Gateway recovery observation expiry is missing".to_string())?;
        observation.validate()?;
        candidate.snapshot()?;
        if candidate.node_id != node_id
            || observation.command_id != command_id.as_uuid()
            || observation.node_id != node_id.as_uuid()
            || observation.gateway_id != node_id.as_uuid()
            || observation.revision != candidate.revision
            || observation.snapshot_digest != candidate.snapshot_digest
            || observation.observed_at > command_not_after
        {
            return Err("Gateway recovery observation does not match its candidate command".into());
        }
        Ok(())
    }

    fn replay_observation(
        &self,
        node_id: NodeId,
        candidate: &GatewayPublication,
        prior: Option<&GatewayPublication>,
        observation: &NodeGatewaySnapshotObservation,
    ) -> Result<bool, String> {
        self.validate_observation_identity(node_id, candidate, observation)?;
        if self.observation.as_ref() == Some(observation) {
            let known = observed_state_is_known(candidate, prior, observation)?;
            let expected_state = match observation.state {
                GatewaySnapshotObservationState::Applying => GatewayReplicaRecoveryState::Required,
                _ if known => GatewayReplicaRecoveryState::Observed,
                _ => GatewayReplicaRecoveryState::Diverged,
            };
            if self.state == expected_state {
                return Ok(false);
            }
        }
        Err("Gateway recovery already has different observation evidence".into())
    }
}

fn observed_state_is_known(
    candidate: &GatewayPublication,
    prior: Option<&GatewayPublication>,
    observation: &NodeGatewaySnapshotObservation,
) -> Result<bool, String> {
    candidate.snapshot()?;
    if let Some(prior) = prior {
        prior.snapshot()?;
        if prior.node_id != candidate.node_id || prior.revision >= candidate.revision {
            return Err("Gateway recovery prior publication is inconsistent".into());
        }
    }
    let Some(applied) = &observation.applied else {
        return Ok(matches!(
            observation.state,
            GatewaySnapshotObservationState::Rejected
                | GatewaySnapshotObservationState::NotApplied
                | GatewaySnapshotObservationState::Uninitialized
        ));
    };
    Ok(applied_matches_publication(applied, candidate)
        || prior.is_some_and(|publication| applied_matches_publication(applied, publication)))
}

fn applied_matches_publication(
    applied: &AppliedGatewaySnapshot,
    publication: &GatewayPublication,
) -> bool {
    applied.gateway_id == publication.node_id.as_uuid()
        && applied.revision == publication.revision
        && applied.expected_revision == publication.expected_revision
        && applied.snapshot_digest == publication.snapshot_digest
        && canonical_timestamp(applied.issued_at) == publication.command_issued_at
        && canonical_timestamp(applied.expires_at) == publication.snapshot_expires_at
}

fn canonicalize_observation(
    mut observation: NodeGatewaySnapshotObservation,
) -> NodeGatewaySnapshotObservation {
    observation.observed_at = canonical_timestamp(observation.observed_at);
    if let Some(applied) = &mut observation.applied {
        applied.issued_at = canonical_timestamp(applied.issued_at);
        applied.expires_at = canonical_timestamp(applied.expires_at);
        applied.applied_at = canonical_timestamp(applied.applied_at);
    }
    observation
}

fn validate_failure(failure: String) -> Result<String, String> {
    if failure.is_empty()
        || failure.len() > MAX_RECOVERY_FAILURE_BYTES
        || failure.contains(['\0', '\r', '\n'])
        || failure.trim() != failure
    {
        return Err("Gateway recovery failure must be a bounded single-line value".into());
    }
    Ok(failure)
}
