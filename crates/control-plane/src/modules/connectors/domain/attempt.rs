use super::{
    ConnectorExecutionEvidence, ConnectorExecutionRequest, ConnectorRevision,
    MAXIMUM_CONNECTOR_BODY_BYTES,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, ConnectorProfileId, ConnectorRevisionId, EnvironmentId, OrganizationId,
    ProjectId, Sha256Digest,
};
use chrono::{DateTime, Duration, Utc};
use std::fmt;
use uuid::Uuid;

pub const DEFAULT_CONNECTOR_EXECUTION_ATTEMPT_PAGE_SIZE: usize = 50;
pub const MAXIMUM_CONNECTOR_EXECUTION_ATTEMPT_PAGE_SIZE: usize = 100;
pub const MAXIMUM_CONNECTOR_EXECUTION_RESERVATION_SECONDS: i64 = 30;
pub const MAXIMUM_CONNECTOR_EXECUTION_OUTCOME_SECONDS: i64 = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorExecutionAttemptState {
    Reserved,
    Dispatching,
    Terminal,
}

impl ConnectorExecutionAttemptState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Reserved => "reserved",
            Self::Dispatching => "dispatching",
            Self::Terminal => "terminal",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "reserved" => Ok(Self::Reserved),
            "dispatching" => Ok(Self::Dispatching),
            "terminal" => Ok(Self::Terminal),
            _ => Err("Connector execution attempt state is unsupported".into()),
        }
    }
}

/// Recovery meaning of the durable attempt at one observed time.
///
/// Only `ReservationExpired` permits another fence to reserve the same logical
/// attempt. An expired dispatch is `Indeterminate`: neither this value nor the
/// repository authorizes another provider call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorExecutionRecoveryState {
    Reserved,
    ReservationExpired,
    InFlight,
    Indeterminate,
    Completed,
}

impl ConnectorExecutionRecoveryState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Reserved => "reserved",
            Self::ReservationExpired => "reservation_expired",
            Self::InFlight => "in_flight",
            Self::Indeterminate => "indeterminate",
            Self::Completed => "completed",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ConnectorExecutionAttemptBinding {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    profile_id: ConnectorProfileId,
    revision_id: ConnectorRevisionId,
    attempt_id: Uuid,
    request_digest: Sha256Digest,
    request_body_bytes: u64,
}

impl ConnectorExecutionAttemptBinding {
    pub fn from_exact(
        revision: &ConnectorRevision,
        request: &ConnectorExecutionRequest,
    ) -> Result<Self, String> {
        revision.validate()?;
        request.validate()?;
        if request.connector_revision_id() != revision.id {
            return Err("Connector execution attempt does not match its exact revision".into());
        }
        let binding = Self {
            organization_id: revision.organization_id,
            project_id: revision.project_id,
            environment_id: revision.environment_id,
            profile_id: revision.profile_id,
            revision_id: revision.id,
            attempt_id: request.attempt_id(),
            request_digest: request.evidence_digest(),
            request_body_bytes: request.body().len() as u64,
        };
        binding.validate()?;
        Ok(binding)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
        profile_id: ConnectorProfileId,
        revision_id: ConnectorRevisionId,
        attempt_id: Uuid,
        request_digest: Sha256Digest,
        request_body_bytes: u64,
    ) -> Result<Self, String> {
        let binding = Self {
            organization_id,
            project_id,
            environment_id,
            profile_id,
            revision_id,
            attempt_id,
            request_digest,
            request_body_bytes,
        };
        binding.validate()?;
        Ok(binding)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.profile_id.as_uuid().is_nil()
            || self.revision_id.as_uuid().is_nil()
            || self.attempt_id.is_nil()
            || Sha256Digest::parse(self.request_digest.as_str())? != self.request_digest
            || self.request_body_bytes > MAXIMUM_CONNECTOR_BODY_BYTES as u64
        {
            return Err("Connector execution attempt binding is invalid".into());
        }
        Ok(())
    }

    pub const fn organization_id(&self) -> OrganizationId {
        self.organization_id
    }

    pub const fn project_id(&self) -> ProjectId {
        self.project_id
    }

    pub const fn environment_id(&self) -> EnvironmentId {
        self.environment_id
    }

    pub const fn profile_id(&self) -> ConnectorProfileId {
        self.profile_id
    }

    pub const fn revision_id(&self) -> ConnectorRevisionId {
        self.revision_id
    }

    pub const fn attempt_id(&self) -> Uuid {
        self.attempt_id
    }

    pub const fn request_digest(&self) -> &Sha256Digest {
        &self.request_digest
    }

    pub const fn request_body_bytes(&self) -> u64 {
        self.request_body_bytes
    }

    pub fn matches_evidence(&self, evidence: &ConnectorExecutionEvidence) -> bool {
        self.organization_id == evidence.organization_id()
            && self.project_id == evidence.project_id()
            && self.environment_id == evidence.environment_id()
            && self.profile_id == evidence.profile_id()
            && self.revision_id == evidence.revision_id()
            && self.attempt_id == evidence.attempt_id()
            && &self.request_digest == evidence.request_digest()
            && self.request_body_bytes == evidence.request_body_bytes()
    }
}

impl fmt::Debug for ConnectorExecutionAttemptBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConnectorExecutionAttemptBinding")
            .field("organization_id", &self.organization_id)
            .field("project_id", &self.project_id)
            .field("environment_id", &self.environment_id)
            .field("profile_id", &self.profile_id)
            .field("revision_id", &self.revision_id)
            .field("attempt_id", &self.attempt_id)
            .field("request_body_bytes", &self.request_body_bytes)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ConnectorExecutionAttempt {
    binding: ConnectorExecutionAttemptBinding,
    state: ConnectorExecutionAttemptState,
    fence_generation: u64,
    fence_token: Uuid,
    reserved_at: DateTime<Utc>,
    lease_expires_at: DateTime<Utc>,
    dispatch_started_at: Option<DateTime<Utc>>,
    outcome_deadline_at: Option<DateTime<Utc>>,
    terminal_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

impl ConnectorExecutionAttempt {
    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        binding: ConnectorExecutionAttemptBinding,
        state: ConnectorExecutionAttemptState,
        fence_generation: u64,
        fence_token: Uuid,
        reserved_at: DateTime<Utc>,
        lease_expires_at: DateTime<Utc>,
        dispatch_started_at: Option<DateTime<Utc>>,
        outcome_deadline_at: Option<DateTime<Utc>>,
        terminal_at: Option<DateTime<Utc>>,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let attempt = Self {
            binding,
            state,
            fence_generation,
            fence_token,
            reserved_at,
            lease_expires_at,
            dispatch_started_at,
            outcome_deadline_at,
            terminal_at,
            created_at,
        };
        attempt.validate()?;
        Ok(attempt)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.binding.validate()?;
        if self.fence_generation == 0
            || self.fence_generation > i64::MAX as u64
            || self.fence_token.is_nil()
            || self.reserved_at != canonical_timestamp(self.reserved_at)
            || self.lease_expires_at != canonical_timestamp(self.lease_expires_at)
            || self.created_at != canonical_timestamp(self.created_at)
            || self.created_at > self.reserved_at
            || self.lease_expires_at <= self.reserved_at
            || self.lease_expires_at - self.reserved_at
                > Duration::seconds(MAXIMUM_CONNECTOR_EXECUTION_RESERVATION_SECONDS)
        {
            return Err("Connector execution attempt reservation is invalid".into());
        }
        for timestamp in [
            self.dispatch_started_at,
            self.outcome_deadline_at,
            self.terminal_at,
        ]
        .into_iter()
        .flatten()
        {
            if timestamp != canonical_timestamp(timestamp) {
                return Err("Connector execution attempt time is not canonical".into());
            }
        }
        match self.state {
            ConnectorExecutionAttemptState::Reserved
                if self.dispatch_started_at.is_none()
                    && self.outcome_deadline_at.is_none()
                    && self.terminal_at.is_none() =>
            {
                Ok(())
            }
            ConnectorExecutionAttemptState::Dispatching
                if self.valid_dispatch_window() && self.terminal_at.is_none() =>
            {
                Ok(())
            }
            ConnectorExecutionAttemptState::Terminal
                if self.terminal_at.is_some_and(|terminal_at| {
                    if let Some(dispatch_started_at) = self.dispatch_started_at {
                        self.outcome_deadline_at.is_some() && terminal_at >= dispatch_started_at
                    } else {
                        self.outcome_deadline_at.is_none()
                            && terminal_at >= self.reserved_at
                            && terminal_at <= self.lease_expires_at
                    }
                }) && (self.dispatch_started_at.is_none() || self.valid_dispatch_window()) =>
            {
                Ok(())
            }
            _ => Err("Connector execution attempt state fields are inconsistent".into()),
        }
    }

    fn valid_dispatch_window(&self) -> bool {
        self.dispatch_started_at
            .zip(self.outcome_deadline_at)
            .is_some_and(|(started_at, deadline_at)| {
                started_at >= self.reserved_at
                    && started_at < self.lease_expires_at
                    && deadline_at > started_at
                    && deadline_at - started_at
                        <= Duration::seconds(MAXIMUM_CONNECTOR_EXECUTION_OUTCOME_SECONDS)
            })
    }

    pub fn recovery_state(&self, observed_at: DateTime<Utc>) -> ConnectorExecutionRecoveryState {
        match self.state {
            ConnectorExecutionAttemptState::Reserved if observed_at < self.lease_expires_at => {
                ConnectorExecutionRecoveryState::Reserved
            }
            ConnectorExecutionAttemptState::Reserved => {
                ConnectorExecutionRecoveryState::ReservationExpired
            }
            ConnectorExecutionAttemptState::Dispatching
                if self
                    .outcome_deadline_at
                    .is_some_and(|deadline| observed_at < deadline) =>
            {
                ConnectorExecutionRecoveryState::InFlight
            }
            ConnectorExecutionAttemptState::Dispatching => {
                ConnectorExecutionRecoveryState::Indeterminate
            }
            ConnectorExecutionAttemptState::Terminal => ConnectorExecutionRecoveryState::Completed,
        }
    }

    pub fn fence(&self) -> ConnectorExecutionFence {
        ConnectorExecutionFence {
            binding: self.binding.clone(),
            generation: self.fence_generation,
            token: self.fence_token,
            reserved_at: self.reserved_at,
            lease_expires_at: self.lease_expires_at,
        }
    }

    pub fn binding(&self) -> &ConnectorExecutionAttemptBinding {
        &self.binding
    }

    pub const fn state(&self) -> ConnectorExecutionAttemptState {
        self.state
    }

    pub const fn fence_generation(&self) -> u64 {
        self.fence_generation
    }

    pub const fn reserved_at(&self) -> DateTime<Utc> {
        self.reserved_at
    }

    pub const fn lease_expires_at(&self) -> DateTime<Utc> {
        self.lease_expires_at
    }

    pub const fn dispatch_started_at(&self) -> Option<DateTime<Utc>> {
        self.dispatch_started_at
    }

    pub const fn outcome_deadline_at(&self) -> Option<DateTime<Utc>> {
        self.outcome_deadline_at
    }

    pub const fn terminal_at(&self) -> Option<DateTime<Utc>> {
        self.terminal_at
    }

    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
}

impl fmt::Debug for ConnectorExecutionAttempt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConnectorExecutionAttempt")
            .field("binding", &self.binding)
            .field("state", &self.state)
            .field("fence_generation", &self.fence_generation)
            .field("fence", &"redacted")
            .field("reserved_at", &self.reserved_at)
            .field("lease_expires_at", &self.lease_expires_at)
            .field("dispatch_started_at", &self.dispatch_started_at)
            .field("outcome_deadline_at", &self.outcome_deadline_at)
            .field("terminal_at", &self.terminal_at)
            .field("created_at", &self.created_at)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ConnectorExecutionFence {
    binding: ConnectorExecutionAttemptBinding,
    generation: u64,
    token: Uuid,
    reserved_at: DateTime<Utc>,
    lease_expires_at: DateTime<Utc>,
}

impl ConnectorExecutionFence {
    pub fn validate(&self) -> Result<(), String> {
        self.binding.validate()?;
        if self.generation == 0
            || self.generation > i64::MAX as u64
            || self.token.is_nil()
            || self.reserved_at != canonical_timestamp(self.reserved_at)
            || self.lease_expires_at != canonical_timestamp(self.lease_expires_at)
            || self.lease_expires_at <= self.reserved_at
            || self.lease_expires_at - self.reserved_at
                > Duration::seconds(MAXIMUM_CONNECTOR_EXECUTION_RESERVATION_SECONDS)
        {
            return Err("Connector execution fence is invalid".into());
        }
        Ok(())
    }

    pub fn binding(&self) -> &ConnectorExecutionAttemptBinding {
        &self.binding
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub const fn token(&self) -> Uuid {
        self.token
    }

    pub const fn reserved_at(&self) -> DateTime<Utc> {
        self.reserved_at
    }

    pub const fn lease_expires_at(&self) -> DateTime<Utc> {
        self.lease_expires_at
    }
}

impl fmt::Debug for ConnectorExecutionFence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConnectorExecutionFence")
            .field("binding", &self.binding)
            .field("generation", &self.generation)
            .field("token", &"redacted")
            .field("reserved_at", &self.reserved_at)
            .field("lease_expires_at", &self.lease_expires_at)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorExecutionAttemptRecord {
    pub attempt: ConnectorExecutionAttempt,
    pub evidence: Option<ConnectorExecutionEvidence>,
}

impl ConnectorExecutionAttemptRecord {
    pub fn new(
        attempt: ConnectorExecutionAttempt,
        evidence: Option<ConnectorExecutionEvidence>,
    ) -> Result<Self, String> {
        attempt.validate()?;
        match (attempt.state, evidence.as_ref()) {
            (ConnectorExecutionAttemptState::Terminal, Some(stored_evidence))
                if attempt.binding.matches_evidence(stored_evidence)
                    && attempt.terminal_at == Some(stored_evidence.completed_at()) =>
            {
                Ok(Self { attempt, evidence })
            }
            (ConnectorExecutionAttemptState::Reserved, None)
            | (ConnectorExecutionAttemptState::Dispatching, None) => Ok(Self { attempt, evidence }),
            _ => Err("Connector execution attempt and evidence are inconsistent".into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectorExecutionAttemptCursor {
    pub created_at: DateTime<Utc>,
    pub attempt_id: Uuid,
}

impl ConnectorExecutionAttemptCursor {
    pub fn after(attempt: &ConnectorExecutionAttempt) -> Self {
        Self {
            created_at: attempt.created_at,
            attempt_id: attempt.binding.attempt_id,
        }
    }

    pub fn validate(self) -> Result<Self, String> {
        if self.attempt_id.is_nil() || self.created_at != canonical_timestamp(self.created_at) {
            return Err("Connector execution attempt cursor is invalid".into());
        }
        Ok(self)
    }

    pub fn encode(self) -> String {
        format!(
            "v1:{}:{}",
            self.created_at.timestamp_micros(),
            self.attempt_id
        )
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        let invalid = || "Connector execution attempt cursor is invalid".to_owned();
        if value.is_empty() || value.len() > 128 || value.contains(['\0', '\r', '\n']) {
            return Err(invalid());
        }
        let mut parts = value.split(':');
        if parts.next() != Some("v1") {
            return Err(invalid());
        }
        let created_at = parts
            .next()
            .and_then(|part| part.parse::<i64>().ok())
            .and_then(DateTime::<Utc>::from_timestamp_micros)
            .ok_or_else(invalid)?;
        let attempt_id = parts
            .next()
            .and_then(|part| Uuid::parse_str(part).ok())
            .filter(|value| !value.is_nil())
            .ok_or_else(invalid)?;
        if parts.next().is_some() {
            return Err(invalid());
        }
        Self {
            created_at,
            attempt_id,
        }
        .validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorExecutionAttemptPage {
    pub attempts: Vec<ConnectorExecutionAttemptRecord>,
    pub next_cursor: Option<ConnectorExecutionAttemptCursor>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::connectors::domain::{
        ConnectorDefinition, ConnectorHttpAuthentication, ConnectorHttpDefinition,
        ConnectorHttpDefinitionSpec, ConnectorHttpDestination, ConnectorHttpMethod,
        ConnectorHttpStatusPolicy,
    };
    use crate::modules::shared_kernel::domain::PrincipalId;

    fn exact(now: DateTime<Utc>) -> (ConnectorRevision, ConnectorExecutionRequest) {
        let revision = ConnectorRevision::initial(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            ConnectorProfileId::new(),
            ConnectorRevisionId::new(),
            ConnectorDefinition::Http(
                ConnectorHttpDefinition::from_spec(ConnectorHttpDefinitionSpec {
                    destination: ConnectorHttpDestination::LiteralHttps {
                        endpoint: "https://hooks.example.test/fenced".into(),
                    },
                    method: ConnectorHttpMethod::Post,
                    request_content_type: "application/json".into(),
                    maximum_request_bytes: 1024,
                    maximum_response_bytes: 1024,
                    timeout_milliseconds: 5_000,
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
            br#"{"bounded":true}"#.to_vec(),
        )
        .expect("request");
        (revision, request)
    }

    #[test]
    fn recovery_never_turns_an_expired_dispatch_into_a_reservation() {
        let now = canonical_timestamp(Utc::now());
        let (revision, request) = exact(now);
        let binding =
            ConnectorExecutionAttemptBinding::from_exact(&revision, &request).expect("binding");
        let dispatch_started_at = now + Duration::seconds(1);
        let outcome_deadline_at = dispatch_started_at + Duration::seconds(10);
        let attempt = ConnectorExecutionAttempt::restore(
            binding,
            ConnectorExecutionAttemptState::Dispatching,
            1,
            Uuid::now_v7(),
            now,
            now + Duration::seconds(30),
            Some(dispatch_started_at),
            Some(outcome_deadline_at),
            None,
            now,
        )
        .expect("attempt");

        assert_eq!(
            attempt.recovery_state(outcome_deadline_at - Duration::microseconds(1)),
            ConnectorExecutionRecoveryState::InFlight
        );
        assert_eq!(
            attempt.recovery_state(outcome_deadline_at),
            ConnectorExecutionRecoveryState::Indeterminate
        );
        let debug = format!("{attempt:?}");
        assert!(!debug.contains(attempt.binding.request_digest().as_str()));
        assert!(!debug.contains(&attempt.fence().token().to_string()));
    }

    #[test]
    fn only_an_expired_preflight_reservation_is_recoverable() {
        let now = canonical_timestamp(Utc::now());
        let (revision, request) = exact(now);
        let attempt = ConnectorExecutionAttempt::restore(
            ConnectorExecutionAttemptBinding::from_exact(&revision, &request).expect("binding"),
            ConnectorExecutionAttemptState::Reserved,
            1,
            Uuid::now_v7(),
            now,
            now + Duration::seconds(30),
            None,
            None,
            None,
            now,
        )
        .expect("attempt");

        assert_eq!(
            attempt.recovery_state(now + Duration::seconds(29)),
            ConnectorExecutionRecoveryState::Reserved
        );
        assert_eq!(
            attempt.recovery_state(now + Duration::seconds(30)),
            ConnectorExecutionRecoveryState::ReservationExpired
        );
    }

    #[test]
    fn state_rejects_late_preflight_terminal_and_oversized_windows() {
        let now = canonical_timestamp(Utc::now());
        let (revision, request) = exact(now);
        let binding =
            ConnectorExecutionAttemptBinding::from_exact(&revision, &request).expect("binding");
        assert!(ConnectorExecutionAttempt::restore(
            binding.clone(),
            ConnectorExecutionAttemptState::Terminal,
            1,
            Uuid::now_v7(),
            now,
            now + Duration::seconds(5),
            None,
            None,
            Some(now + Duration::seconds(6)),
            now,
        )
        .is_err());
        assert!(ConnectorExecutionAttempt::restore(
            binding,
            ConnectorExecutionAttemptState::Dispatching,
            1,
            Uuid::now_v7(),
            now,
            now + Duration::seconds(30),
            Some(now + Duration::seconds(1)),
            Some(now + Duration::seconds(1 + MAXIMUM_CONNECTOR_EXECUTION_OUTCOME_SECONDS + 1),),
            None,
            now,
        )
        .is_err());
    }

    #[test]
    fn attempt_cursor_round_trips_and_rejects_untrusted_input() {
        let cursor = ConnectorExecutionAttemptCursor {
            created_at: canonical_timestamp(Utc::now()),
            attempt_id: Uuid::now_v7(),
        };
        assert_eq!(
            ConnectorExecutionAttemptCursor::parse(&cursor.encode()),
            Ok(cursor)
        );
        for invalid in [
            "",
            "v2:1:00000000-0000-0000-0000-000000000001",
            "v1:nope:nope",
        ] {
            assert!(ConnectorExecutionAttemptCursor::parse(invalid).is_err());
        }
    }
}
