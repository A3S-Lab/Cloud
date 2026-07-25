use crate::modules::edge::domain::{
    GatewayReplicaRolloutState, GatewayRollout, GatewayRolloutState,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, GatewayRolloutId, GatewayScopeId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const MAX_ROLLBACK_FAILURE_BYTES: usize = 4 * 1024;
const ROLLBACK_ID_NAME: &[u8] = b"a3s-cloud.gateway-rollout.rollback.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayRolloutRollbackState {
    Required,
    Staged,
    Succeeded,
    Diverged,
}

impl GatewayRolloutRollbackState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Staged => "staged",
            Self::Succeeded => "succeeded",
            Self::Diverged => "diverged",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "required" => Ok(Self::Required),
            "staged" => Ok(Self::Staged),
            "succeeded" => Ok(Self::Succeeded),
            "diverged" => Ok(Self::Diverged),
            _ => Err(format!(
                "unsupported Gateway rollout rollback state {value:?}"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayRolloutRollback {
    pub failed_rollout_id: GatewayRolloutId,
    pub gateway_scope_id: GatewayScopeId,
    pub membership_generation: u64,
    pub failed_generation: u64,
    pub rollback_rollout_id: GatewayRolloutId,
    pub rollback_generation: u64,
    pub state: GatewayRolloutRollbackState,
    pub aggregate_version: u64,
    pub required_at: DateTime<Utc>,
    pub staged_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub failure: Option<String>,
}

impl GatewayRolloutRollback {
    pub fn required(failed: &GatewayRollout) -> Result<Self, String> {
        failed.validate()?;
        if failed.state != GatewayRolloutState::Degraded
            || failed.serves_traffic()?
            || failed.completed_at.is_none()
        {
            return Err(
                "Gateway rollback is required only for a terminal rollout below its readiness threshold"
                    .into(),
            );
        }
        let rollback_generation = failed
            .generation
            .checked_add(1)
            .ok_or_else(|| "Gateway rollback generation space is exhausted".to_string())?;
        let rollback = Self {
            failed_rollout_id: failed.id,
            gateway_scope_id: failed.gateway_scope_id,
            membership_generation: failed.membership_generation,
            failed_generation: failed.generation,
            rollback_rollout_id: Self::deterministic_rollout_id(failed.id),
            rollback_generation,
            state: GatewayRolloutRollbackState::Required,
            aggregate_version: 1,
            required_at: canonical_timestamp(
                failed
                    .completed_at
                    .ok_or_else(|| "failed Gateway rollout omitted completion time".to_string())?,
            ),
            staged_at: None,
            completed_at: None,
            failure: None,
        };
        rollback.validate()?;
        Ok(rollback)
    }

    pub fn deterministic_rollout_id(failed_rollout_id: GatewayRolloutId) -> GatewayRolloutId {
        GatewayRolloutId::from_uuid(Uuid::new_v5(&failed_rollout_id.as_uuid(), ROLLBACK_ID_NAME))
    }

    pub const fn blocks_scope(&self) -> bool {
        !matches!(self.state, GatewayRolloutRollbackState::Succeeded)
    }

    pub fn stage(&mut self, rollout: &GatewayRollout) -> Result<bool, String> {
        self.validate_rollout_identity(rollout)?;
        if rollout.state != GatewayRolloutState::Pending
            || rollout.aggregate_version != 1
            || rollout.started_at < self.required_at
            || rollout.completed_at.is_some()
            || rollout.ready_replicas != 0
            || rollout.unavailable_replicas != 0
            || rollout
                .replicas
                .iter()
                .any(|replica| replica.state != GatewayReplicaRolloutState::Pending)
        {
            return Err("Gateway rollback must stage one untouched exact-member rollout".into());
        }
        if self.state == GatewayRolloutRollbackState::Staged
            && self.staged_at == Some(rollout.started_at)
        {
            return Ok(false);
        }
        if self.state != GatewayRolloutRollbackState::Required {
            return Err("Gateway rollback cannot stage from its current state".into());
        }
        self.state = GatewayRolloutRollbackState::Staged;
        self.staged_at = Some(rollout.started_at);
        self.advance_version()?;
        self.validate()?;
        Ok(true)
    }

    pub fn succeed(&mut self, rollout: &GatewayRollout) -> Result<bool, String> {
        self.validate_rollout_identity(rollout)?;
        if self.state == GatewayRolloutRollbackState::Succeeded {
            if self.completed_at == rollout.completed_at
                && rollout.state == GatewayRolloutState::Succeeded
            {
                return Ok(false);
            }
            return Err("Gateway rollback already has different completion evidence".into());
        }
        if self.state != GatewayRolloutRollbackState::Staged
            || rollout.state != GatewayRolloutState::Succeeded
            || rollout.completed_at.is_none()
            || rollout
                .replicas
                .iter()
                .any(|replica| replica.state != GatewayReplicaRolloutState::Applied)
        {
            return Err(
                "Gateway rollback succeeds only after every exact member acknowledgement".into(),
            );
        }
        let completed_at = canonical_timestamp(
            rollout
                .completed_at
                .ok_or_else(|| "Gateway rollback rollout omitted completion time".to_string())?,
        );
        if completed_at < self.staged_at.unwrap_or(self.required_at) {
            return Err("Gateway rollback completion time regressed".into());
        }
        self.state = GatewayRolloutRollbackState::Succeeded;
        self.completed_at = Some(completed_at);
        self.failure = None;
        self.advance_version()?;
        self.validate()?;
        Ok(true)
    }

    pub fn diverge(
        &mut self,
        rollout: &GatewayRollout,
        failure: impl Into<String>,
    ) -> Result<bool, String> {
        self.validate_rollout_identity(rollout)?;
        let failure = validate_failure(failure.into())?;
        let completed_at = canonical_timestamp(
            rollout
                .completed_at
                .ok_or_else(|| "diverged Gateway rollback omitted completion time".to_string())?,
        );
        if self.state == GatewayRolloutRollbackState::Diverged
            && self.completed_at == Some(completed_at)
            && self.failure.as_deref() == Some(failure.as_str())
        {
            return Ok(false);
        }
        if self.state != GatewayRolloutRollbackState::Staged
            || rollout.state != GatewayRolloutState::Degraded
            || completed_at < self.staged_at.unwrap_or(self.required_at)
        {
            return Err("Gateway rollback divergence does not match its staged rollout".into());
        }
        self.state = GatewayRolloutRollbackState::Diverged;
        self.completed_at = Some(completed_at);
        self.failure = Some(failure);
        self.advance_version()?;
        self.validate()?;
        Ok(true)
    }

    pub fn diverge_before_staging(
        &mut self,
        failure: impl Into<String>,
        observed_at: DateTime<Utc>,
    ) -> Result<bool, String> {
        let failure = validate_failure(failure.into())?;
        let observed_at = canonical_timestamp(observed_at);
        if self.state == GatewayRolloutRollbackState::Diverged
            && self.staged_at.is_none()
            && self.completed_at == Some(observed_at)
            && self.failure.as_deref() == Some(failure.as_str())
        {
            return Ok(false);
        }
        if self.state != GatewayRolloutRollbackState::Required || observed_at < self.required_at {
            return Err(
                "Gateway rollback cannot diverge before staging from its current state".into(),
            );
        }
        self.state = GatewayRolloutRollbackState::Diverged;
        self.completed_at = Some(observed_at);
        self.failure = Some(failure);
        self.advance_version()?;
        self.validate()?;
        Ok(true)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.failed_rollout_id.as_uuid().is_nil()
            || self.gateway_scope_id.as_uuid().is_nil()
            || self.membership_generation == 0
            || self.failed_generation == 0
            || self.failed_generation.checked_add(1) != Some(self.rollback_generation)
            || self.rollback_rollout_id != Self::deterministic_rollout_id(self.failed_rollout_id)
            || self.rollback_rollout_id.as_uuid().is_nil()
            || self.aggregate_version == 0
            || self
                .staged_at
                .is_some_and(|staged_at| staged_at < self.required_at)
            || self.completed_at.is_some_and(|completed_at| {
                completed_at < self.staged_at.unwrap_or(self.required_at)
            })
            || self
                .failure
                .as_deref()
                .is_some_and(|failure| validate_failure(failure.to_owned()).is_err())
        {
            return Err("Gateway rollout rollback identity or timeline is invalid".into());
        }
        let state_is_consistent = match self.state {
            GatewayRolloutRollbackState::Required => {
                self.staged_at.is_none() && self.completed_at.is_none() && self.failure.is_none()
            }
            GatewayRolloutRollbackState::Staged => {
                self.staged_at.is_some() && self.completed_at.is_none() && self.failure.is_none()
            }
            GatewayRolloutRollbackState::Succeeded => {
                self.staged_at.is_some() && self.completed_at.is_some() && self.failure.is_none()
            }
            GatewayRolloutRollbackState::Diverged => {
                self.completed_at.is_some() && self.failure.is_some()
            }
        };
        if !state_is_consistent {
            return Err("Gateway rollout rollback state is inconsistent".into());
        }
        Ok(())
    }

    fn validate_rollout_identity(&self, rollout: &GatewayRollout) -> Result<(), String> {
        rollout.validate()?;
        let desired = rollout.replicas.len();
        let desired_u32 = u32::try_from(desired)
            .map_err(|_| "Gateway rollback replica count exceeds supported bounds".to_string())?;
        if rollout.id != self.rollback_rollout_id
            || rollout.gateway_scope_id != self.gateway_scope_id
            || rollout.membership_generation != self.membership_generation
            || rollout.generation != self.rollback_generation
            || rollout.correlation_id != self.rollback_rollout_id.as_uuid()
            || rollout.policy.min_ready != desired_u32
            || rollout.policy.max_unavailable != 0
            || rollout.required_ready()? != desired_u32
        {
            return Err(
                "Gateway rollback rollout identity or exact-member policy is invalid".into(),
            );
        }
        Ok(())
    }

    fn advance_version(&mut self) -> Result<(), String> {
        self.aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Gateway rollback version space is exhausted".to_string())?;
        Ok(())
    }
}

fn validate_failure(failure: String) -> Result<String, String> {
    if failure.is_empty()
        || failure.len() > MAX_ROLLBACK_FAILURE_BYTES
        || failure.contains(['\0', '\r', '\n'])
        || failure.trim() != failure
    {
        return Err("Gateway rollback failure must be a bounded single-line value".into());
    }
    Ok(failure)
}
