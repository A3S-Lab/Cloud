//! Project validated usage lifecycle events into request facts and daily rollups.
//!
//! Rollups are rebuildable aggregates over terminal request facts. They never
//! invent tokens or outcomes that the lifecycle event did not carry. Exact
//! event-id redelivery must not double-count a rollup.

use a3s_cloud_contracts::{
    InferenceUsageBatchV1, InferenceUsageEndpointV1, InferenceUsageLifecycleEventV1,
    InferenceUsageLifecycleKindV1, InferenceUsageMeasurementCompletenessV1,
    InferenceUsageTerminalOutcomeV1,
};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use std::collections::HashMap;
use uuid::Uuid;

/// Durable prompt-free request fact keyed by `request_id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceUsageRequestFact {
    pub request_id: Uuid,
    pub gateway_id: Uuid,
    pub environment_id: Uuid,
    pub credential_id: Uuid,
    pub credential_generation: u64,
    pub route_id: Uuid,
    pub route_policy_revision: u64,
    pub endpoint: InferenceUsageEndpointV1,
    pub model_alias: String,
    pub model_id: Uuid,
    pub started_at: DateTime<Utc>,
    pub terminated_at: Option<DateTime<Utc>>,
    pub outcome: Option<InferenceUsageTerminalOutcomeV1>,
    pub http_status: Option<u16>,
    pub duration_ms: Option<u64>,
    pub measurement_completeness: Option<InferenceUsageMeasurementCompletenessV1>,
    pub total_tokens: Option<u64>,
    pub attempt_count: u32,
}

impl InferenceUsageRequestFact {
    pub fn is_terminal(&self) -> bool {
        self.outcome.is_some()
    }

    pub fn rollup_day(&self) -> NaiveDate {
        self.started_at.date_naive()
    }

    pub fn rollup_key(&self) -> InferenceUsageDailyRollupKey {
        InferenceUsageDailyRollupKey {
            day: self.rollup_day(),
            environment_id: self.environment_id,
            model_id: self.model_id,
            endpoint: self.endpoint,
        }
    }
}

/// Dimensions for one rebuildable daily rollup bucket.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InferenceUsageDailyRollupKey {
    pub day: NaiveDate,
    pub environment_id: Uuid,
    pub model_id: Uuid,
    pub endpoint: InferenceUsageEndpointV1,
}

/// Rebuildable daily aggregate. Counts advance only when a request first
/// becomes terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceUsageDailyRollup {
    pub key: InferenceUsageDailyRollupKey,
    pub request_count: u64,
    pub succeeded_count: u64,
    pub failed_count: u64,
    pub fallback_count: u64,
    pub cancelled_count: u64,
    pub disconnected_count: u64,
    pub unknown_measurement_count: u64,
    pub upstream_usage_count: u64,
    pub total_tokens: u64,
}

impl InferenceUsageDailyRollup {
    pub fn empty(key: InferenceUsageDailyRollupKey) -> Self {
        Self {
            key,
            request_count: 0,
            succeeded_count: 0,
            failed_count: 0,
            fallback_count: 0,
            cancelled_count: 0,
            disconnected_count: 0,
            unknown_measurement_count: 0,
            upstream_usage_count: 0,
            total_tokens: 0,
        }
    }

    pub fn apply_terminal_request(
        &mut self,
        fact: &InferenceUsageRequestFact,
    ) -> Result<(), String> {
        let outcome = fact
            .outcome
            .ok_or_else(|| "daily rollup requires a terminal request fact".to_string())?;
        self.request_count = self.request_count.saturating_add(1);
        match outcome {
            InferenceUsageTerminalOutcomeV1::Succeeded => {
                self.succeeded_count = self.succeeded_count.saturating_add(1);
            }
            InferenceUsageTerminalOutcomeV1::Failed => {
                self.failed_count = self.failed_count.saturating_add(1);
            }
            InferenceUsageTerminalOutcomeV1::Fallback => {
                self.fallback_count = self.fallback_count.saturating_add(1);
            }
            InferenceUsageTerminalOutcomeV1::Cancelled => {
                self.cancelled_count = self.cancelled_count.saturating_add(1);
            }
            InferenceUsageTerminalOutcomeV1::Disconnected => {
                self.disconnected_count = self.disconnected_count.saturating_add(1);
            }
        }
        match fact.measurement_completeness {
            Some(InferenceUsageMeasurementCompletenessV1::Unknown) | None => {
                self.unknown_measurement_count = self.unknown_measurement_count.saturating_add(1);
            }
            Some(InferenceUsageMeasurementCompletenessV1::UpstreamUsage) => {
                self.upstream_usage_count = self.upstream_usage_count.saturating_add(1);
            }
        }
        if let Some(tokens) = fact.total_tokens {
            self.total_tokens = self.total_tokens.saturating_add(tokens);
        }
        Ok(())
    }
}

/// Apply one lifecycle event onto an optional existing request fact.
///
/// Returns the next fact and whether this transition newly opened a terminal
/// state (the only moment a daily rollup may advance).
pub fn project_lifecycle_event(
    existing: Option<&InferenceUsageRequestFact>,
    gateway_id: Uuid,
    event: &InferenceUsageLifecycleEventV1,
) -> Result<(InferenceUsageRequestFact, bool), String> {
    event.validate()?;
    if gateway_id.is_nil() {
        return Err("usage projection gateway ID is invalid".into());
    }

    match event.kind {
        InferenceUsageLifecycleKindV1::RequestStarted => {
            if let Some(existing) = existing {
                if existing.request_id != event.request.request_id {
                    return Err("usage projection changed request identity".into());
                }
                return Ok((existing.clone(), false));
            }
            Ok((
                InferenceUsageRequestFact {
                    request_id: event.request.request_id,
                    gateway_id,
                    environment_id: event.request.environment_id,
                    credential_id: event.request.credential_id,
                    credential_generation: event.request.credential_generation,
                    route_id: event.request.route_id,
                    route_policy_revision: event.request.route_policy_revision,
                    endpoint: event.request.endpoint,
                    model_alias: event.request.model_alias.clone(),
                    model_id: event.request.model_id,
                    started_at: event.occurred_at,
                    terminated_at: None,
                    outcome: None,
                    http_status: None,
                    duration_ms: None,
                    measurement_completeness: None,
                    total_tokens: None,
                    attempt_count: 0,
                },
                false,
            ))
        }
        InferenceUsageLifecycleKindV1::AttemptStarted => {
            let mut fact = existing
                .cloned()
                .ok_or_else(|| "attempt_started without request_started".to_string())?;
            ensure_same_request(&fact, event)?;
            if fact.is_terminal() {
                return Ok((fact, false));
            }
            fact.attempt_count = fact.attempt_count.saturating_add(1);
            Ok((fact, false))
        }
        InferenceUsageLifecycleKindV1::AttemptTerminal => {
            let fact = existing
                .cloned()
                .ok_or_else(|| "attempt_terminal without request_started".to_string())?;
            ensure_same_request(&fact, event)?;
            // Attempt terminals refine attempt history only; request rollups key
            // off request_terminal so partial attempts never invent request totals.
            Ok((fact, false))
        }
        InferenceUsageLifecycleKindV1::RequestTerminal => {
            let mut fact = existing
                .cloned()
                .ok_or_else(|| "request_terminal without request_started".to_string())?;
            ensure_same_request(&fact, event)?;
            if fact.is_terminal() {
                return Ok((fact, false));
            }
            fact.terminated_at = Some(event.occurred_at);
            fact.outcome = event.outcome;
            fact.http_status = event.http_status;
            fact.duration_ms = event.duration_ms;
            fact.measurement_completeness = event.measurement_completeness;
            fact.total_tokens = event.total_tokens;
            Ok((fact, true))
        }
    }
}

fn ensure_same_request(
    fact: &InferenceUsageRequestFact,
    event: &InferenceUsageLifecycleEventV1,
) -> Result<(), String> {
    if fact.request_id != event.request.request_id {
        return Err("usage projection changed request identity".into());
    }
    if fact.environment_id != event.request.environment_id
        || fact.credential_id != event.request.credential_id
        || fact.route_id != event.request.route_id
        || fact.model_id != event.request.model_id
        || fact.endpoint != event.request.endpoint
    {
        return Err("usage projection changed immutable request descriptors".into());
    }
    Ok(())
}

/// Inclusive UTC calendar-day bounds helper for rollup queries.
pub fn utc_day(timestamp: DateTime<Utc>) -> NaiveDate {
    NaiveDate::from_ymd_opt(timestamp.year(), timestamp.month(), timestamp.day())
        .expect("chrono DateTime yields a valid NaiveDate")
}

/// Project newly inserted batch records into request facts and daily rollups.
pub fn project_inserted_usage_records(
    facts: &mut HashMap<Uuid, InferenceUsageRequestFact>,
    rollups: &mut HashMap<InferenceUsageDailyRollupKey, InferenceUsageDailyRollup>,
    gateway_id: Uuid,
    batch: &InferenceUsageBatchV1,
    inserted_event_ids: &[Uuid],
) -> Result<(), String> {
    use a3s_cloud_contracts::InferenceUsageLifecycleEventV1;
    use std::collections::HashSet;

    let inserted: HashSet<Uuid> = inserted_event_ids.iter().copied().collect();
    for record in &batch.records {
        if !inserted.contains(&record.event_id) {
            continue;
        }
        let payload = record.payload()?;
        let event = InferenceUsageLifecycleEventV1::decode(&payload)?;
        let existing = facts.get(&event.request.request_id);
        let (next, newly_terminal) = project_lifecycle_event(existing, gateway_id, &event)?;
        if newly_terminal {
            let key = next.rollup_key();
            let rollup = rollups
                .entry(key.clone())
                .or_insert_with(|| InferenceUsageDailyRollup::empty(key));
            rollup.apply_terminal_request(&next)?;
        }
        facts.insert(next.request_id, next);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use a3s_cloud_contracts::{InferenceUsageAttemptEvidenceV1, InferenceUsageRequestEvidenceV1};

    fn request() -> InferenceUsageRequestEvidenceV1 {
        InferenceUsageRequestEvidenceV1 {
            request_id: Uuid::from_u128(1),
            correlation_id: "corr".into(),
            environment_id: Uuid::from_u128(2),
            credential_id: Uuid::from_u128(3),
            credential_generation: 1,
            route_id: Uuid::from_u128(4),
            route_policy_revision: 1,
            endpoint: InferenceUsageEndpointV1::ChatCompletions,
            model_alias: "alias".into(),
            model_id: Uuid::from_u128(5),
        }
    }

    fn started(at: &str) -> InferenceUsageLifecycleEventV1 {
        InferenceUsageLifecycleEventV1 {
            schema: InferenceUsageLifecycleEventV1::SCHEMA.into(),
            kind: InferenceUsageLifecycleKindV1::RequestStarted,
            occurred_at: DateTime::parse_from_rfc3339(at)
                .unwrap()
                .with_timezone(&Utc),
            request: request(),
            attempt: None,
            outcome: None,
            http_status: None,
            duration_ms: None,
            measurement_completeness: None,
            total_tokens: None,
        }
    }

    fn terminal(at: &str, tokens: Option<u64>) -> InferenceUsageLifecycleEventV1 {
        InferenceUsageLifecycleEventV1 {
            schema: InferenceUsageLifecycleEventV1::SCHEMA.into(),
            kind: InferenceUsageLifecycleKindV1::RequestTerminal,
            occurred_at: DateTime::parse_from_rfc3339(at)
                .unwrap()
                .with_timezone(&Utc),
            request: request(),
            attempt: None,
            outcome: Some(InferenceUsageTerminalOutcomeV1::Succeeded),
            http_status: Some(200),
            duration_ms: Some(12),
            measurement_completeness: Some(if tokens.is_some() {
                InferenceUsageMeasurementCompletenessV1::UpstreamUsage
            } else {
                InferenceUsageMeasurementCompletenessV1::Unknown
            }),
            total_tokens: tokens,
        }
    }

    #[test]
    fn start_then_terminal_advances_rollup_once() {
        let gateway_id = Uuid::from_u128(9);
        let (fact, newly) =
            project_lifecycle_event(None, gateway_id, &started("2026-01-02T10:00:00Z")).unwrap();
        assert!(!newly);
        assert!(!fact.is_terminal());

        let (fact, newly) = project_lifecycle_event(
            Some(&fact),
            gateway_id,
            &terminal("2026-01-02T10:00:01Z", Some(7)),
        )
        .unwrap();
        assert!(newly);
        assert!(fact.is_terminal());

        let mut rollup = InferenceUsageDailyRollup::empty(fact.rollup_key());
        rollup.apply_terminal_request(&fact).unwrap();
        assert_eq!(rollup.request_count, 1);
        assert_eq!(rollup.succeeded_count, 1);
        assert_eq!(rollup.upstream_usage_count, 1);
        assert_eq!(rollup.total_tokens, 7);
        assert_eq!(rollup.key.day, NaiveDate::from_ymd_opt(2026, 1, 2).unwrap());

        let (fact, newly) = project_lifecycle_event(
            Some(&fact),
            gateway_id,
            &terminal("2026-01-02T10:00:02Z", Some(99)),
        )
        .unwrap();
        assert!(!newly);
        assert_eq!(fact.total_tokens, Some(7));
    }

    #[test]
    fn attempt_started_increments_without_rollup() {
        let gateway_id = Uuid::from_u128(9);
        let (fact, _) =
            project_lifecycle_event(None, gateway_id, &started("2026-01-02T10:00:00Z")).unwrap();
        let mut attempt = started("2026-01-02T10:00:00Z");
        attempt.kind = InferenceUsageLifecycleKindV1::AttemptStarted;
        attempt.attempt = Some(InferenceUsageAttemptEvidenceV1 {
            attempt_id: Uuid::from_u128(11),
            target_id: Uuid::from_u128(12),
        });
        let (fact, newly) = project_lifecycle_event(Some(&fact), gateway_id, &attempt).unwrap();
        assert!(!newly);
        assert_eq!(fact.attempt_count, 1);
    }

    #[test]
    fn request_terminal_without_start_fails_closed() {
        let err = project_lifecycle_event(
            None,
            Uuid::from_u128(9),
            &terminal("2026-01-02T10:00:01Z", None),
        )
        .unwrap_err();
        assert!(err.contains("without request_started"));
    }
}
