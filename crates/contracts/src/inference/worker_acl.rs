//! Inference worker ACL projection for Cloud → Gateway policy.
//!
//! Gateway already parses this shape under managed `inference.workers`. Cloud
//! contracts own the byte form; Inference + Power observation authority must
//! supply the projections. Edge must not invent worker or catalog facts.

use super::{
    InferenceRouteAclProjection, InferenceServingPhase, PowerTransferHealth,
    POWER_WORKER_OBSERVATION_SCHEMA,
};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

const MAX_SAFE_ACL_INTEGER: u64 = (1_u64 << 53) - 1;
const MAX_OBSERVATION_VALIDITY_SECONDS: i64 = 300;
const MAX_OBSERVATION_CLOCK_SKEW_SECONDS: i64 = 30;
const MAX_CERTIFIED_LATENCY_MS: u64 = 60_000;
const MAX_UNIT_ID_LEN: usize = 128;

/// One Power worker observation projected into Gateway managed ACL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InferenceWorkerAclProjection {
    pub unit_id: String,
    pub target_id: Uuid,
    pub generation: u64,
    pub schema: String,
    pub worker_epoch: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_profile_sha256: Option<String>,
    pub observation_generation: u64,
    pub observed_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub phases: Vec<InferenceServingPhase>,
    pub prompt_cache_capable: bool,
    pub state_transfer_capable: bool,
    pub ready_phases: Vec<InferenceServingPhase>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_limit: Option<u64>,
    pub active: u64,
    pub waiting: u64,
    pub prompt_cache_supported: bool,
    pub prompt_cache_entries: u64,
    pub prompt_cache_capacity: u64,
    pub prompt_cache_pressure_basis_points: u16,
    pub transfer_health: PowerTransferHealth,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub certified_latency_ms: Option<u64>,
}

impl InferenceWorkerAclProjection {
    /// Validate worker identity and Power observation integrity at projection time.
    pub fn validate_at(
        &self,
        projected_at: DateTime<Utc>,
        policy_expires_at: DateTime<Utc>,
    ) -> Result<(), String> {
        validate_unit_id(&self.unit_id)?;
        if self.target_id.is_nil() {
            return Err("inference worker target_id must not be nil".into());
        }
        if self.generation == 0 || self.generation > MAX_SAFE_ACL_INTEGER {
            return Err(format!(
                "inference worker generation must be in 1..={MAX_SAFE_ACL_INTEGER}"
            ));
        }
        if self.schema != POWER_WORKER_OBSERVATION_SCHEMA {
            return Err(format!(
                "inference worker schema {:?} is unsupported; expected {POWER_WORKER_OBSERVATION_SCHEMA}",
                self.schema
            ));
        }
        if self.worker_epoch.is_nil()
            || self.observation_generation == 0
            || self.observation_generation > MAX_SAFE_ACL_INTEGER
        {
            return Err("inference worker epoch or observation_generation is invalid".into());
        }
        if self
            .execution_profile_sha256
            .as_deref()
            .is_some_and(|digest| !valid_sha256(digest))
        {
            return Err("inference worker execution_profile_sha256 is invalid".into());
        }
        let needs_profile = self.phases.iter().any(|phase| {
            matches!(
                phase,
                InferenceServingPhase::Prefill | InferenceServingPhase::Decode
            )
        });
        if needs_profile
            && self
                .execution_profile_sha256
                .as_deref()
                .is_none_or(|digest| !valid_sha256(digest))
        {
            return Err(
                "inference worker with prefill/decode phases requires a valid execution_profile_sha256"
                    .into(),
            );
        }
        let validity = self.expires_at - self.observed_at;
        if validity <= Duration::zero()
            || validity > Duration::seconds(MAX_OBSERVATION_VALIDITY_SECONDS)
            || self.expires_at > policy_expires_at
            || self.expires_at <= projected_at
            || self.observed_at
                > projected_at + Duration::seconds(MAX_OBSERVATION_CLOCK_SKEW_SECONDS)
        {
            return Err("inference worker freshness window is invalid".into());
        }
        validate_phases(&self.phases, false, "capability")?;
        validate_phases(&self.ready_phases, true, "ready")?;
        if self
            .ready_phases
            .iter()
            .any(|phase| !self.phases.contains(phase))
        {
            return Err("inference worker ready phase is outside capabilities".into());
        }
        if self.active > MAX_SAFE_ACL_INTEGER
            || self.waiting > MAX_SAFE_ACL_INTEGER
            || self.active_limit.is_some_and(|limit| {
                limit == 0 || limit > MAX_SAFE_ACL_INTEGER || self.active > limit
            })
        {
            return Err("inference worker admission counters are invalid".into());
        }
        let expected_pressure =
            cache_pressure_basis_points(self.prompt_cache_entries, self.prompt_cache_capacity);
        if self.prompt_cache_entries > MAX_SAFE_ACL_INTEGER
            || self.prompt_cache_capacity > MAX_SAFE_ACL_INTEGER
            || self.prompt_cache_entries > self.prompt_cache_capacity
            || self.prompt_cache_pressure_basis_points > 10_000
            || self.prompt_cache_pressure_basis_points != expected_pressure
            || self.prompt_cache_capable != self.prompt_cache_supported
            || (!self.prompt_cache_capable
                && (self.prompt_cache_entries != 0 || self.prompt_cache_capacity != 0))
        {
            return Err("inference worker prompt-cache observation is inconsistent".into());
        }
        if self.state_transfer_capable
            == matches!(self.transfer_health, PowerTransferHealth::Unsupported)
        {
            return Err("inference worker transfer capability and health are inconsistent".into());
        }
        if self
            .certified_latency_ms
            .is_some_and(|latency| latency == 0 || latency > MAX_CERTIFIED_LATENCY_MS)
        {
            return Err("inference worker certified_latency_ms is invalid".into());
        }
        Ok(())
    }
}

/// Render worker blocks for insertion inside a managed `inference` policy.
///
/// Workers are sorted by `unit_id`. Callers must compose them through
/// [`super::render_inference_policy_acl_with_routes_and_workers`].
pub fn render_inference_worker_acl_blocks(
    workers: &[InferenceWorkerAclProjection],
    projected_at: DateTime<Utc>,
    policy_expires_at: DateTime<Utc>,
) -> Result<String, String> {
    let mut ordered = workers.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|worker| worker.unit_id.as_str());
    let mut seen = HashSet::new();
    let mut out = String::new();
    for worker in ordered {
        worker.validate_at(projected_at, policy_expires_at)?;
        if !seen.insert(worker.unit_id.as_str()) {
            return Err(format!(
                "inference worker unit_id '{}' is not unique",
                worker.unit_id
            ));
        }
        out.push_str(&render_one_worker(worker));
    }
    Ok(out)
}

/// Ensure every worker target binding exists on a projected route model target.
pub fn require_workers_bound_to_routes(
    workers: &[InferenceWorkerAclProjection],
    routes: &[InferenceRouteAclProjection],
) -> Result<(), String> {
    if workers.is_empty() {
        return Ok(());
    }
    if routes.is_empty() {
        return Err("inference workers require at least one route projection".into());
    }
    let target_ids = routes
        .iter()
        .flat_map(|route| {
            route
                .models
                .iter()
                .flat_map(|model| model.targets.iter().map(|target| target.target_id))
        })
        .collect::<HashSet<_>>();
    for worker in workers {
        if !target_ids.contains(&worker.target_id) {
            return Err(format!(
                "inference worker '{}' target_id {} is not bound to any projected route target",
                worker.unit_id, worker.target_id
            ));
        }
    }
    Ok(())
}

fn render_one_worker(worker: &InferenceWorkerAclProjection) -> String {
    let observed_at = worker
        .observed_at
        .to_rfc3339_opts(SecondsFormat::Micros, true);
    let expires_at = worker
        .expires_at
        .to_rfc3339_opts(SecondsFormat::Micros, true);
    let phases = format_phases(&worker.phases);
    let ready_phases = format_phases(&worker.ready_phases);
    let mut block = format!(
        "\n  workers \"{}\" {{\n    target_id = \"{}\"\n    generation = {}\n    schema = \"{}\"\n    worker_epoch = \"{}\"\n",
        escape_acl_string(&worker.unit_id),
        worker.target_id,
        worker.generation,
        escape_acl_string(&worker.schema),
        worker.worker_epoch,
    );
    if let Some(profile) = &worker.execution_profile_sha256 {
        block.push_str(&format!(
            "    execution_profile_sha256 = \"{}\"\n",
            escape_acl_string(profile)
        ));
    }
    block.push_str(&format!(
        "    observation_generation = {}\n    observed_at = \"{observed_at}\"\n    expires_at = \"{expires_at}\"\n    phases = [{phases}]\n    prompt_cache_capable = {}\n    state_transfer_capable = {}\n    ready_phases = [{ready_phases}]\n",
        worker.observation_generation,
        if worker.prompt_cache_capable {
            "true"
        } else {
            "false"
        },
        if worker.state_transfer_capable {
            "true"
        } else {
            "false"
        },
    ));
    if let Some(limit) = worker.active_limit {
        block.push_str(&format!("    active_limit = {limit}\n"));
    }
    block.push_str(&format!(
        "    active = {}\n    waiting = {}\n    prompt_cache_supported = {}\n    prompt_cache_entries = {}\n    prompt_cache_capacity = {}\n    prompt_cache_pressure_basis_points = {}\n    transfer_health = \"{}\"\n",
        worker.active,
        worker.waiting,
        if worker.prompt_cache_supported {
            "true"
        } else {
            "false"
        },
        worker.prompt_cache_entries,
        worker.prompt_cache_capacity,
        worker.prompt_cache_pressure_basis_points,
        transfer_health_acl(worker.transfer_health),
    ));
    if let Some(latency) = worker.certified_latency_ms {
        block.push_str(&format!("    certified_latency_ms = {latency}\n"));
    }
    block.push_str("  }\n");
    block
}

fn format_phases(phases: &[InferenceServingPhase]) -> String {
    phases
        .iter()
        .map(|phase| format!("\"{}\"", phase_acl(*phase)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn phase_acl(phase: InferenceServingPhase) -> &'static str {
    match phase {
        InferenceServingPhase::Aggregated => "aggregated",
        InferenceServingPhase::Prefill => "prefill",
        InferenceServingPhase::Decode => "decode",
    }
}

fn transfer_health_acl(health: PowerTransferHealth) -> &'static str {
    match health {
        PowerTransferHealth::Unsupported => "unsupported",
        PowerTransferHealth::Ready => "ready",
        PowerTransferHealth::Degraded => "degraded",
        PowerTransferHealth::Unavailable => "unavailable",
    }
}

fn validate_unit_id(unit_id: &str) -> Result<(), String> {
    if unit_id.is_empty() || unit_id.len() > MAX_UNIT_ID_LEN {
        return Err(format!(
            "inference worker unit_id must contain 1 to {MAX_UNIT_ID_LEN} characters"
        ));
    }
    if !unit_id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_' || byte == b'.')
    {
        return Err(
            "inference worker unit_id must use ASCII letters, digits, '-', '_', or '.'".into(),
        );
    }
    Ok(())
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
        return Err(format!("inference worker {label} phases are invalid"));
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

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn escape_acl_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::{
        InferenceEndpointAcl, InferenceGrantAclProjection, InferenceLimitsAclProjection,
        InferenceModelAclProjection, InferenceTargetAclProjection,
    };
    use chrono::TimeZone;

    fn projected_at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2099, 1, 1, 0, 0, 0).unwrap()
    }

    fn policy_expires() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2099, 1, 1, 1, 0, 0).unwrap()
    }

    fn aggregated_worker() -> InferenceWorkerAclProjection {
        InferenceWorkerAclProjection {
            unit_id: "power-unit-1".into(),
            target_id: Uuid::parse_str("66666666-6666-4666-8666-666666666666").unwrap(),
            generation: 5,
            schema: POWER_WORKER_OBSERVATION_SCHEMA.into(),
            worker_epoch: Uuid::parse_str("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa").unwrap(),
            execution_profile_sha256: None,
            observation_generation: 9,
            observed_at: projected_at() - Duration::seconds(1),
            expires_at: projected_at() + Duration::seconds(14),
            phases: vec![InferenceServingPhase::Aggregated],
            prompt_cache_capable: true,
            state_transfer_capable: false,
            ready_phases: vec![InferenceServingPhase::Aggregated],
            active_limit: Some(8),
            active: 2,
            waiting: 1,
            prompt_cache_supported: true,
            prompt_cache_entries: 2,
            prompt_cache_capacity: 8,
            prompt_cache_pressure_basis_points: 2500,
            transfer_health: PowerTransferHealth::Unsupported,
            certified_latency_ms: Some(42),
        }
    }

    fn route_with_target(target_id: Uuid) -> InferenceRouteAclProjection {
        InferenceRouteAclProjection {
            route_id: Uuid::parse_str("44444444-4444-4444-8444-444444444444").unwrap(),
            router: "inference".into(),
            environment_id: Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap(),
            policy_revision: 11,
            models: vec![InferenceModelAclProjection {
                alias: "chat-model".into(),
                model_id: Uuid::parse_str("55555555-5555-4555-8555-555555555555").unwrap(),
                targets: vec![InferenceTargetAclProjection {
                    target_id,
                    service: "model-service".into(),
                    upstream_model: "internal/model-v1".into(),
                    priority: 0,
                    weight: 100,
                }],
            }],
            grants: vec![InferenceGrantAclProjection {
                credential_id: Uuid::parse_str("33333333-3333-4333-8333-333333333333").unwrap(),
                credential_generation: 3,
                models: vec!["chat-model".into()],
                endpoints: vec![InferenceEndpointAcl::Models],
                limits: InferenceLimitsAclProjection {
                    max_concurrent_requests: 2,
                    requests_per_minute: 60,
                    request_burst: 2,
                    tokens_per_minute: 10_000,
                },
            }],
        }
    }

    #[test]
    fn renders_deterministic_aggregated_worker_block() {
        let rendered = render_inference_worker_acl_blocks(
            &[aggregated_worker()],
            projected_at(),
            policy_expires(),
        )
        .unwrap();
        assert!(rendered.contains("workers \"power-unit-1\""));
        assert!(rendered.contains("schema = \"a3s.power.worker-observation.v1\""));
        assert!(rendered.contains("phases = [\"aggregated\"]"));
        assert!(rendered.contains("transfer_health = \"unsupported\""));
        assert!(rendered.contains("certified_latency_ms = 42"));
        assert!(!rendered.contains("execution_profile_sha256"));
    }

    #[test]
    fn rejects_unknown_schema_stale_window_and_orphan_target() {
        let mut schema = aggregated_worker();
        schema.schema = "a3s.power.worker-observation.v0".into();
        assert!(
            render_inference_worker_acl_blocks(&[schema], projected_at(), policy_expires())
                .unwrap_err()
                .contains("unsupported")
        );

        let mut stale = aggregated_worker();
        stale.expires_at = projected_at() - Duration::seconds(1);
        assert!(
            render_inference_worker_acl_blocks(&[stale], projected_at(), policy_expires())
                .unwrap_err()
                .contains("freshness")
        );

        let worker = aggregated_worker();
        let other_target = Uuid::parse_str("77777777-7777-4777-8777-777777777777").unwrap();
        assert!(
            require_workers_bound_to_routes(&[worker], &[route_with_target(other_target)])
                .unwrap_err()
                .contains("not bound")
        );
    }

    #[test]
    fn prefill_requires_profile_and_binds_to_route_target() {
        let mut prefill = aggregated_worker();
        prefill.unit_id = "power-prefill-1".into();
        prefill.phases = vec![InferenceServingPhase::Prefill];
        prefill.ready_phases = vec![InferenceServingPhase::Prefill];
        prefill.state_transfer_capable = true;
        prefill.transfer_health = PowerTransferHealth::Ready;
        assert!(render_inference_worker_acl_blocks(
            &[prefill.clone()],
            projected_at(),
            policy_expires()
        )
        .unwrap_err()
        .contains("execution_profile_sha256"));

        prefill.execution_profile_sha256 =
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into());
        render_inference_worker_acl_blocks(&[prefill.clone()], projected_at(), policy_expires())
            .unwrap();
        require_workers_bound_to_routes(
            &[prefill],
            &[route_with_target(
                Uuid::parse_str("66666666-6666-4666-8666-666666666666").unwrap(),
            )],
        )
        .unwrap();
    }
}
