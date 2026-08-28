//! Versioned distributed-inference observation contracts.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

pub const POWER_WORKER_OBSERVATION_SCHEMA: &str = "a3s.power.worker-observation.v1";

const MAX_SAFE_ACL_INTEGER: u64 = (1_u64 << 53) - 1;
const MAX_OBSERVATION_VALIDITY_SECONDS: i64 = 300;
const MAX_OBSERVATION_CLOCK_SKEW_SECONDS: i64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InferenceServingPhase {
    Aggregated,
    Prefill,
    Decode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PowerWorkerCapabilities {
    pub phases: Vec<InferenceServingPhase>,
    pub prompt_cache: bool,
    pub state_transfer: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PowerAdmissionObservation {
    pub active_limit: Option<u64>,
    pub active: u64,
    pub waiting: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PowerPromptCacheObservation {
    pub supported: bool,
    pub entries: u64,
    pub capacity: u64,
    pub pressure_basis_points: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PowerTransferHealth {
    Unsupported,
    Ready,
    Degraded,
    Unavailable,
}

/// The closed `worker` object returned by A3S Power's health endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PowerWorkerObservation {
    pub schema: String,
    pub worker_epoch: Uuid,
    pub observation_generation: u64,
    pub observed_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub capabilities: PowerWorkerCapabilities,
    pub ready_phases: Vec<InferenceServingPhase>,
    pub admission: PowerAdmissionObservation,
    pub prompt_cache: PowerPromptCacheObservation,
    pub transfer_health: PowerTransferHealth,
}

impl PowerWorkerObservation {
    /// Validate Power-owned facts at the Cloud collection boundary.
    pub fn validate_at(&self, collected_at: DateTime<Utc>) -> Result<(), String> {
        if self.schema != POWER_WORKER_OBSERVATION_SCHEMA {
            return Err(format!(
                "unsupported Power worker observation schema {:?}",
                self.schema
            ));
        }
        if self.worker_epoch.is_nil()
            || self.observation_generation == 0
            || self.observation_generation > MAX_SAFE_ACL_INTEGER
        {
            return Err("Power worker observation epoch or generation is invalid".into());
        }
        let validity = self.expires_at - self.observed_at;
        if validity <= Duration::zero()
            || validity > Duration::seconds(MAX_OBSERVATION_VALIDITY_SECONDS)
            || self.expires_at <= collected_at
            || self.observed_at
                > collected_at + Duration::seconds(MAX_OBSERVATION_CLOCK_SKEW_SECONDS)
        {
            return Err("Power worker observation freshness window is invalid".into());
        }
        validate_phases(&self.capabilities.phases, false, "capability")?;
        validate_phases(&self.ready_phases, true, "ready")?;
        if self
            .ready_phases
            .iter()
            .any(|phase| !self.capabilities.phases.contains(phase))
        {
            return Err("Power worker exposes a ready phase outside its capabilities".into());
        }
        validate_admission(self.admission)?;
        validate_prompt_cache(self.prompt_cache, self.capabilities.prompt_cache)?;
        if self.capabilities.state_transfer
            == matches!(self.transfer_health, PowerTransferHealth::Unsupported)
        {
            return Err("Power worker transfer capability and health are inconsistent".into());
        }
        Ok(())
    }
}

fn validate_phases(
    phases: &[InferenceServingPhase],
    allow_empty: bool,
    label: &str,
) -> Result<(), String> {
    if (!allow_empty && phases.is_empty())
        || phases.len() > 3
        || phases.iter().copied().collect::<HashSet<_>>().len() != phases.len()
    {
        return Err(format!("Power worker {label} phases are invalid"));
    }
    Ok(())
}

fn validate_admission(observation: PowerAdmissionObservation) -> Result<(), String> {
    if observation.active > MAX_SAFE_ACL_INTEGER
        || observation.waiting > MAX_SAFE_ACL_INTEGER
        || observation.active_limit.is_some_and(|limit| {
            limit == 0 || limit > MAX_SAFE_ACL_INTEGER || observation.active > limit
        })
    {
        return Err("Power worker admission counters are invalid".into());
    }
    Ok(())
}

fn validate_prompt_cache(
    observation: PowerPromptCacheObservation,
    capable: bool,
) -> Result<(), String> {
    let expected_pressure = cache_pressure_basis_points(observation.entries, observation.capacity);
    if observation.entries > MAX_SAFE_ACL_INTEGER
        || observation.capacity > MAX_SAFE_ACL_INTEGER
        || observation.entries > observation.capacity
        || observation.pressure_basis_points > 10_000
        || observation.pressure_basis_points != expected_pressure
        || observation.supported != capable
        || (!capable && (observation.entries != 0 || observation.capacity != 0))
    {
        return Err("Power worker prompt-cache observation is inconsistent".into());
    }
    Ok(())
}

fn cache_pressure_basis_points(entries: u64, capacity: u64) -> u16 {
    if capacity == 0 {
        return if entries == 0 { 0 } else { 10_000 };
    }
    let pressure = u128::from(entries)
        .saturating_mul(10_000)
        .checked_div(u128::from(capacity))
        .unwrap_or(10_000)
        .min(10_000);
    u16::try_from(pressure).unwrap_or(10_000)
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
