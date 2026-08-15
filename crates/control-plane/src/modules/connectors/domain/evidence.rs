use super::{
    maximum_connector_retry_after, ConnectorExecutionReceipt, ConnectorExecutionRequest,
    ConnectorRevision, MAXIMUM_CONNECTOR_BODY_BYTES,
};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, ConnectorProfileId, ConnectorRevisionId, EnvironmentId, OrganizationId,
    ProjectId, Sha256Digest,
};
use chrono::{DateTime, Utc};
use std::fmt;
use std::time::Duration;
use uuid::Uuid;

pub const MAXIMUM_CONNECTOR_EXECUTION_EVIDENCE_PAGE_SIZE: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorExecutionOutcome {
    Accepted,
    Retryable,
    Rejected,
}

impl ConnectorExecutionOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Retryable => "retryable",
            Self::Rejected => "rejected",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "accepted" => Ok(Self::Accepted),
            "retryable" => Ok(Self::Retryable),
            "rejected" => Ok(Self::Rejected),
            _ => Err("Connector execution evidence outcome is unsupported".into()),
        }
    }
}

/// One immutable terminal fact for one exact Connector execution attempt.
///
/// This record deliberately contains no endpoint, address, header, body,
/// credential, provider response text, retry counter, lease, or scheduler
/// state. Flow or the owning durable consumer remains the attempt and retry
/// authority.
#[derive(Clone, PartialEq, Eq)]
pub struct ConnectorExecutionEvidence {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    profile_id: ConnectorProfileId,
    revision_id: ConnectorRevisionId,
    attempt_id: Uuid,
    request_digest: Sha256Digest,
    request_body_bytes: u64,
    outcome: ConnectorExecutionOutcome,
    response_status: Option<u16>,
    response_digest: Option<Sha256Digest>,
    response_body_bytes: Option<u64>,
    retry_after: Option<Duration>,
    started_at: DateTime<Utc>,
    completed_at: DateTime<Utc>,
}

impl ConnectorExecutionEvidence {
    pub fn accepted(
        revision: &ConnectorRevision,
        request: &ConnectorExecutionRequest,
        receipt: &ConnectorExecutionReceipt,
        started_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        if request.connector_revision_id() != revision.id
            || receipt.connector_revision_id() != revision.id
            || receipt.attempt_id() != request.attempt_id()
        {
            return Err("Connector execution evidence does not match its exact attempt".into());
        }
        Self::build(
            revision,
            request,
            ConnectorExecutionOutcome::Accepted,
            Some(receipt.status()),
            Some(Sha256Digest::from_bytes(receipt.response_body())),
            Some(receipt.response_body().len() as u64),
            None,
            started_at,
            receipt.accepted_at(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn retryable(
        revision: &ConnectorRevision,
        request: &ConnectorExecutionRequest,
        response_status: Option<u16>,
        retry_after: Option<Duration>,
        started_at: DateTime<Utc>,
        completed_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        Self::build(
            revision,
            request,
            ConnectorExecutionOutcome::Retryable,
            response_status,
            None,
            None,
            retry_after,
            started_at,
            completed_at,
        )
    }

    pub fn rejected(
        revision: &ConnectorRevision,
        request: &ConnectorExecutionRequest,
        response_status: Option<u16>,
        started_at: DateTime<Utc>,
        completed_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        Self::build(
            revision,
            request,
            ConnectorExecutionOutcome::Rejected,
            response_status,
            None,
            None,
            None,
            started_at,
            completed_at,
        )
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
        outcome: ConnectorExecutionOutcome,
        response_status: Option<u16>,
        response_digest: Option<Sha256Digest>,
        response_body_bytes: Option<u64>,
        retry_after_seconds: Option<u64>,
        started_at: DateTime<Utc>,
        completed_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let evidence = Self {
            organization_id,
            project_id,
            environment_id,
            profile_id,
            revision_id,
            attempt_id,
            request_digest,
            request_body_bytes,
            outcome,
            response_status,
            response_digest,
            response_body_bytes,
            retry_after: retry_after_seconds.map(Duration::from_secs),
            started_at,
            completed_at,
        };
        evidence.validate()?;
        Ok(evidence)
    }

    #[allow(clippy::too_many_arguments)]
    fn build(
        revision: &ConnectorRevision,
        request: &ConnectorExecutionRequest,
        outcome: ConnectorExecutionOutcome,
        response_status: Option<u16>,
        response_digest: Option<Sha256Digest>,
        response_body_bytes: Option<u64>,
        retry_after: Option<Duration>,
        started_at: DateTime<Utc>,
        completed_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        revision.validate()?;
        request.validate()?;
        if request.connector_revision_id() != revision.id {
            return Err("Connector execution evidence does not match its exact revision".into());
        }
        let evidence = Self {
            organization_id: revision.organization_id,
            project_id: revision.project_id,
            environment_id: revision.environment_id,
            profile_id: revision.profile_id,
            revision_id: revision.id,
            attempt_id: request.attempt_id(),
            request_digest: request.evidence_digest(),
            request_body_bytes: request.body().len() as u64,
            outcome,
            response_status,
            response_digest,
            response_body_bytes,
            retry_after,
            started_at: canonical_timestamp(started_at),
            completed_at: canonical_timestamp(completed_at),
        };
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.profile_id.as_uuid().is_nil()
            || self.revision_id.as_uuid().is_nil()
            || self.attempt_id.is_nil()
        {
            return Err("Connector execution evidence identity is invalid".into());
        }
        if Sha256Digest::parse(self.request_digest.as_str())? != self.request_digest
            || self.request_body_bytes > MAXIMUM_CONNECTOR_BODY_BYTES as u64
            || self.started_at != canonical_timestamp(self.started_at)
            || self.completed_at != canonical_timestamp(self.completed_at)
            || self.completed_at < self.started_at
        {
            return Err("Connector execution evidence request or time is invalid".into());
        }
        if self
            .response_status
            .is_some_and(|status| !(100..=599).contains(&status))
            || self
                .response_body_bytes
                .is_some_and(|bytes| bytes > MAXIMUM_CONNECTOR_BODY_BYTES as u64)
            || self.response_digest.as_ref().is_some_and(|digest| {
                Sha256Digest::parse(digest.as_str()).ok().as_ref() != Some(digest)
            })
            || self.retry_after.is_some_and(|value| {
                value.subsec_nanos() != 0 || value > maximum_connector_retry_after()
            })
        {
            return Err("Connector execution evidence response is invalid".into());
        }
        match self.outcome {
            ConnectorExecutionOutcome::Accepted
                if self
                    .response_status
                    .is_some_and(|status| (200..=299).contains(&status))
                    && self.response_digest.is_some()
                    && self.response_body_bytes.is_some()
                    && self.retry_after.is_none() =>
            {
                Ok(())
            }
            ConnectorExecutionOutcome::Retryable
                if self
                    .response_status
                    .is_none_or(|status| !(200..=299).contains(&status))
                    && self.response_digest.is_none()
                    && self.response_body_bytes.is_none() =>
            {
                Ok(())
            }
            ConnectorExecutionOutcome::Rejected
                if self
                    .response_status
                    .is_none_or(|status| !(200..=299).contains(&status))
                    && self.response_digest.is_none()
                    && self.response_body_bytes.is_none()
                    && self.retry_after.is_none() =>
            {
                Ok(())
            }
            _ => Err("Connector execution evidence outcome fields are inconsistent".into()),
        }
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

    pub const fn outcome(&self) -> ConnectorExecutionOutcome {
        self.outcome
    }

    pub const fn response_status(&self) -> Option<u16> {
        self.response_status
    }

    pub const fn response_digest(&self) -> Option<&Sha256Digest> {
        self.response_digest.as_ref()
    }

    pub const fn response_body_bytes(&self) -> Option<u64> {
        self.response_body_bytes
    }

    pub const fn retry_after(&self) -> Option<Duration> {
        self.retry_after
    }

    pub const fn started_at(&self) -> DateTime<Utc> {
        self.started_at
    }

    pub const fn completed_at(&self) -> DateTime<Utc> {
        self.completed_at
    }
}

impl fmt::Debug for ConnectorExecutionEvidence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConnectorExecutionEvidence")
            .field("organization_id", &self.organization_id)
            .field("project_id", &self.project_id)
            .field("environment_id", &self.environment_id)
            .field("profile_id", &self.profile_id)
            .field("revision_id", &self.revision_id)
            .field("attempt_id", &self.attempt_id)
            .field("request_body_bytes", &self.request_body_bytes)
            .field("outcome", &self.outcome)
            .field("response_status", &self.response_status)
            .field("response_body_bytes", &self.response_body_bytes)
            .field("retry_after", &self.retry_after)
            .field("started_at", &self.started_at)
            .field("completed_at", &self.completed_at)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectorExecutionEvidenceCursor {
    pub completed_at: DateTime<Utc>,
    pub attempt_id: Uuid,
}

impl ConnectorExecutionEvidenceCursor {
    pub fn after(evidence: &ConnectorExecutionEvidence) -> Self {
        Self {
            completed_at: evidence.completed_at,
            attempt_id: evidence.attempt_id,
        }
    }

    pub fn validate(self) -> Result<Self, String> {
        if self.attempt_id.is_nil() || self.completed_at != canonical_timestamp(self.completed_at) {
            return Err("Connector execution evidence cursor is invalid".into());
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectorExecutionEvidencePage {
    pub evidence: Vec<ConnectorExecutionEvidence>,
    pub next_cursor: Option<ConnectorExecutionEvidenceCursor>,
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

    fn revision(created_at: DateTime<Utc>) -> ConnectorRevision {
        ConnectorRevision::initial(
            OrganizationId::new(),
            ProjectId::new(),
            EnvironmentId::new(),
            ConnectorProfileId::new(),
            ConnectorRevisionId::new(),
            ConnectorDefinition::Http(
                ConnectorHttpDefinition::from_spec(ConnectorHttpDefinitionSpec {
                    destination: ConnectorHttpDestination::LiteralHttps {
                        endpoint: "https://hooks.example.test/evidence".into(),
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
            created_at,
        )
        .expect("revision")
    }

    #[test]
    fn accepted_evidence_binds_the_exact_request_and_redacts_digests() {
        let started_at = Utc::now();
        let revision = revision(started_at);
        let request = ConnectorExecutionRequest::new(
            revision.id,
            Uuid::now_v7(),
            "application/json",
            b"top-secret-request".to_vec(),
        )
        .expect("request")
        .with_header("x-example", "top-secret-header")
        .expect("header");
        let receipt = ConnectorExecutionReceipt::accepted(
            revision.id,
            request.attempt_id(),
            started_at + chrono::Duration::milliseconds(5),
            202,
            Some("application/json".into()),
            b"top-secret-response".to_vec(),
        )
        .expect("receipt");
        let evidence =
            ConnectorExecutionEvidence::accepted(&revision, &request, &receipt, started_at)
                .expect("evidence");

        assert_eq!(evidence.outcome(), ConnectorExecutionOutcome::Accepted);
        assert_eq!(evidence.response_status(), Some(202));
        assert_eq!(evidence.request_body_bytes(), 18);
        assert_eq!(evidence.response_body_bytes(), Some(19));
        let debug = format!("{evidence:?}");
        assert!(!debug.contains(evidence.request_digest().as_str()));
        assert!(!debug.contains(evidence.response_digest().expect("digest").as_str()));
        assert!(!debug.contains("top-secret"));
    }

    #[test]
    fn request_digest_changes_for_headers_body_and_signing_input() {
        let revision = revision(Utc::now());
        let attempt_id = Uuid::now_v7();
        let base = ConnectorExecutionRequest::new(
            revision.id,
            attempt_id,
            "application/json",
            b"one".to_vec(),
        )
        .expect("base");
        let header = base
            .clone()
            .with_header("x-example", "one")
            .expect("header");
        let signing = base
            .clone()
            .with_signing_input(b"one".to_vec())
            .expect("signing");
        let body = ConnectorExecutionRequest::new(
            revision.id,
            attempt_id,
            "application/json",
            b"two".to_vec(),
        )
        .expect("body");

        assert_ne!(base.evidence_digest(), header.evidence_digest());
        assert_ne!(base.evidence_digest(), signing.evidence_digest());
        assert_ne!(base.evidence_digest(), body.evidence_digest());
    }

    #[test]
    fn terminal_outcome_invariants_are_closed_and_bounded() {
        let now = Utc::now();
        let revision = revision(now);
        let request = ConnectorExecutionRequest::new(
            revision.id,
            Uuid::now_v7(),
            "application/json",
            Vec::new(),
        )
        .expect("request");
        assert!(ConnectorExecutionEvidence::retryable(
            &revision,
            &request,
            Some(503),
            Some(Duration::from_secs(60)),
            now,
            now,
        )
        .is_ok());
        assert!(ConnectorExecutionEvidence::retryable(
            &revision,
            &request,
            Some(200),
            None,
            now,
            now,
        )
        .is_err());
        assert!(ConnectorExecutionEvidence::retryable(
            &revision,
            &request,
            None,
            Some(maximum_connector_retry_after() + Duration::from_secs(1)),
            now,
            now,
        )
        .is_err());
        assert!(ConnectorExecutionEvidence::rejected(
            &revision,
            &request,
            None,
            now + chrono::Duration::seconds(1),
            now,
        )
        .is_err());
    }
}
