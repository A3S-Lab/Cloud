use super::{
    ConnectorExecutionAttempt, ConnectorExecutionAttemptBinding, ConnectorExecutionAttemptState,
    ConnectorExecutionEvidence, ConnectorExecutionOutcome, ConnectorExecutionRecoveryState,
};
use crate::modules::shared_kernel::domain::{canonical_timestamp, PrincipalId};
use chrono::{DateTime, Utc};

pub const CONNECTOR_EXECUTION_ATTEMPT_RESOLUTION_REASON_MAX_BYTES: usize = 1024;

/// Immutable operator conclusion for a provider attempt whose result cannot be determined.
///
/// The resolution never claims provider success or rejection and never authorizes another
/// provider call. Its exact terminal evidence remains bound to the original request and fence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorExecutionAttemptResolution {
    binding: ConnectorExecutionAttemptBinding,
    dispatch_started_at: DateTime<Utc>,
    outcome_deadline_at: DateTime<Utc>,
    reason: String,
    resolved_by: PrincipalId,
    resolved_at: DateTime<Utc>,
}

impl ConnectorExecutionAttemptResolution {
    pub fn new(
        attempt: &ConnectorExecutionAttempt,
        reason: impl Into<String>,
        resolved_by: PrincipalId,
        resolved_at: DateTime<Utc>,
    ) -> Result<(Self, ConnectorExecutionEvidence), String> {
        attempt.validate()?;
        let resolution = Self {
            binding: attempt.binding().clone(),
            dispatch_started_at: attempt
                .dispatch_started_at()
                .ok_or_else(|| "Connector execution dispatch start is missing".to_owned())?,
            outcome_deadline_at: attempt
                .outcome_deadline_at()
                .ok_or_else(|| "Connector execution dispatch deadline is missing".to_owned())?,
            reason: normalize_connector_execution_attempt_resolution_reason(reason)?,
            resolved_by,
            resolved_at: canonical_timestamp(resolved_at),
        };
        resolution.validate_against(attempt)?;
        let evidence = ConnectorExecutionEvidence::indeterminate(attempt, resolution.resolved_at)?;
        resolution.validate_evidence(&evidence)?;
        Ok((resolution, evidence))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        binding: ConnectorExecutionAttemptBinding,
        dispatch_started_at: DateTime<Utc>,
        outcome_deadline_at: DateTime<Utc>,
        reason: impl Into<String>,
        resolved_by: PrincipalId,
        resolved_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let resolution = Self {
            binding,
            dispatch_started_at,
            outcome_deadline_at,
            reason: reason.into(),
            resolved_by,
            resolved_at,
        };
        resolution.validate()?;
        Ok(resolution)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.binding.validate()?;
        if self.dispatch_started_at != canonical_timestamp(self.dispatch_started_at)
            || self.outcome_deadline_at != canonical_timestamp(self.outcome_deadline_at)
            || self.resolved_at != canonical_timestamp(self.resolved_at)
            || self.outcome_deadline_at <= self.dispatch_started_at
            || self.resolved_at < self.outcome_deadline_at
            || self.resolved_by.as_uuid().is_nil()
            || normalize_connector_execution_attempt_resolution_reason(self.reason.as_str())?
                != self.reason
        {
            return Err("Connector execution attempt resolution is invalid".into());
        }
        Ok(())
    }

    pub fn validate_against(&self, attempt: &ConnectorExecutionAttempt) -> Result<(), String> {
        self.validate()?;
        attempt.validate()?;
        if attempt.state() != ConnectorExecutionAttemptState::Dispatching
            || attempt.recovery_state(self.resolved_at)
                != ConnectorExecutionRecoveryState::Indeterminate
            || attempt.binding() != &self.binding
            || attempt.dispatch_started_at() != Some(self.dispatch_started_at)
            || attempt.outcome_deadline_at() != Some(self.outcome_deadline_at)
        {
            return Err(
                "Connector execution attempt resolution does not match an indeterminate attempt"
                    .into(),
            );
        }
        Ok(())
    }

    pub fn validate_evidence(&self, evidence: &ConnectorExecutionEvidence) -> Result<(), String> {
        self.validate()?;
        evidence.validate()?;
        if evidence.outcome() != ConnectorExecutionOutcome::Indeterminate
            || !self.binding.matches_evidence(evidence)
            || evidence.started_at() != self.dispatch_started_at
            || evidence.completed_at() != self.resolved_at
        {
            return Err("Connector execution attempt resolution evidence is inconsistent".into());
        }
        Ok(())
    }

    pub fn binding(&self) -> &ConnectorExecutionAttemptBinding {
        &self.binding
    }

    pub const fn dispatch_started_at(&self) -> DateTime<Utc> {
        self.dispatch_started_at
    }

    pub const fn outcome_deadline_at(&self) -> DateTime<Utc> {
        self.outcome_deadline_at
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }

    pub const fn resolved_by(&self) -> PrincipalId {
        self.resolved_by
    }

    pub const fn resolved_at(&self) -> DateTime<Utc> {
        self.resolved_at
    }
}

pub(crate) fn normalize_connector_execution_attempt_resolution_reason(
    reason: impl Into<String>,
) -> Result<String, String> {
    let reason = reason.into();
    let normalized = reason.trim().to_owned();
    if normalized.is_empty()
        || normalized.len() > CONNECTOR_EXECUTION_ATTEMPT_RESOLUTION_REASON_MAX_BYTES
        || normalized.chars().any(char::is_control)
    {
        return Err("Connector execution attempt resolution reason is invalid".into());
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::connectors::domain::{
        ConnectorDefinition, ConnectorHttpAuthentication, ConnectorHttpDefinition,
        ConnectorHttpDefinitionSpec, ConnectorHttpDestination, ConnectorHttpMethod,
        ConnectorHttpStatusPolicy,
    };
    use crate::modules::connectors::domain::{
        ConnectorExecutionAttempt, ConnectorExecutionAttemptBinding, ConnectorExecutionRequest,
        ConnectorRevision,
    };
    use crate::modules::shared_kernel::domain::{
        ConnectorProfileId, ConnectorRevisionId, EnvironmentId, OrganizationId, ProjectId,
    };
    use chrono::Duration;
    use uuid::Uuid;

    fn dispatching(now: DateTime<Utc>) -> ConnectorExecutionAttempt {
        let revision = ConnectorRevision::initial(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            ConnectorProfileId::new(),
            ConnectorRevisionId::new(),
            ConnectorDefinition::Http(
                ConnectorHttpDefinition::from_spec(ConnectorHttpDefinitionSpec {
                    destination: ConnectorHttpDestination::LiteralHttps {
                        endpoint: "https://hooks.example.test/recovery".into(),
                    },
                    method: ConnectorHttpMethod::Post,
                    request_content_type: "application/json".into(),
                    maximum_request_bytes: 1024,
                    maximum_response_bytes: 1024,
                    timeout_milliseconds: 1_000,
                    status_policy: ConnectorHttpStatusPolicy::standard_webhook(),
                    authentication: ConnectorHttpAuthentication::None,
                })
                .expect("definition"),
            ),
            PrincipalId::new(),
            now,
        )
        .expect("revision");
        let request = ConnectorExecutionRequest::new(
            revision.id,
            Uuid::now_v7(),
            "application/json",
            b"bounded".to_vec(),
        )
        .expect("request");
        ConnectorExecutionAttempt::restore(
            ConnectorExecutionAttemptBinding::from_exact(&revision, &request).expect("binding"),
            ConnectorExecutionAttemptState::Dispatching,
            1,
            Uuid::now_v7(),
            now,
            now + Duration::seconds(30),
            Some(now + Duration::seconds(1)),
            Some(now + Duration::seconds(11)),
            None,
            now,
        )
        .expect("attempt")
    }

    #[test]
    fn resolution_is_exact_indeterminate_and_body_free() {
        let now = canonical_timestamp(Utc::now());
        let attempt = dispatching(now);
        assert!(ConnectorExecutionAttemptResolution::new(
            &attempt,
            "too early",
            PrincipalId::new(),
            now + Duration::seconds(10),
        )
        .is_err());

        let (resolution, evidence) = ConnectorExecutionAttemptResolution::new(
            &attempt,
            "  provider outcome unavailable  ",
            PrincipalId::new(),
            now + Duration::seconds(12),
        )
        .expect("resolution");
        assert_eq!(resolution.reason(), "provider outcome unavailable");
        assert_eq!(evidence.outcome(), ConnectorExecutionOutcome::Indeterminate);
        assert_eq!(evidence.response_status(), None);
        assert_eq!(evidence.response_digest(), None);
        assert_eq!(evidence.retry_after(), None);
        resolution
            .validate_evidence(&evidence)
            .expect("exact evidence");
    }
}
