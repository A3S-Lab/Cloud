use crate::modules::connectors::application::{
    ConnectorExecutionAttemptResolutionMutationResult, ConnectorProfileMutationResult,
    ConnectorRevisionRevocationMutationResult,
};
use crate::modules::connectors::domain::{
    ConnectorExecutionAttemptPage, ConnectorExecutionAttemptRecord,
    ConnectorExecutionAttemptResolution, ConnectorProfile, ConnectorRecord, ConnectorRevision,
    ConnectorRevisionRevocation,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateConnectorProfileRequest {
    pub name: String,
    pub definition_acl: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviseConnectorProfileRequest {
    pub expected_version: u64,
    pub definition_acl: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevokeConnectorRevisionRequest {
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResolveConnectorExecutionAttemptRequest {
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorExecutionAttemptResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub profile_id: Uuid,
    pub revision_id: Uuid,
    pub attempt_id: Uuid,
    pub request_digest: String,
    pub request_body_bytes: u64,
    pub state: String,
    pub recovery_state: String,
    pub reserved_at: DateTime<Utc>,
    pub lease_expires_at: DateTime<Utc>,
    pub dispatch_started_at: Option<DateTime<Utc>>,
    pub outcome_deadline_at: Option<DateTime<Utc>>,
    pub terminal_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub observed_at: DateTime<Utc>,
    pub evidence_outcome: Option<String>,
    pub response_status: Option<u16>,
    pub response_digest: Option<String>,
    pub response_body_bytes: Option<u64>,
    pub retry_after_seconds: Option<u64>,
    pub evidence_started_at: Option<DateTime<Utc>>,
    pub evidence_completed_at: Option<DateTime<Utc>>,
}

impl ConnectorExecutionAttemptResponse {
    pub fn from_record(
        record: ConnectorExecutionAttemptRecord,
        observed_at: DateTime<Utc>,
    ) -> Self {
        let binding = record.attempt.binding();
        let evidence = record.evidence.as_ref();
        Self {
            organization_id: binding.organization_id().as_uuid(),
            project_id: binding.project_id().as_uuid(),
            environment_id: binding.environment_id().as_uuid(),
            profile_id: binding.profile_id().as_uuid(),
            revision_id: binding.revision_id().as_uuid(),
            attempt_id: binding.attempt_id(),
            request_digest: binding.request_digest().to_string(),
            request_body_bytes: binding.request_body_bytes(),
            state: record.attempt.state().as_str().into(),
            recovery_state: record.attempt.recovery_state(observed_at).as_str().into(),
            reserved_at: record.attempt.reserved_at(),
            lease_expires_at: record.attempt.lease_expires_at(),
            dispatch_started_at: record.attempt.dispatch_started_at(),
            outcome_deadline_at: record.attempt.outcome_deadline_at(),
            terminal_at: record.attempt.terminal_at(),
            created_at: record.attempt.created_at(),
            observed_at,
            evidence_outcome: evidence.map(|value| value.outcome().as_str().into()),
            response_status: evidence.and_then(|value| value.response_status()),
            response_digest: evidence
                .and_then(|value| value.response_digest())
                .map(ToString::to_string),
            response_body_bytes: evidence.and_then(|value| value.response_body_bytes()),
            retry_after_seconds: evidence
                .and_then(|value| value.retry_after())
                .map(|value| value.as_secs()),
            evidence_started_at: evidence.map(|value| value.started_at()),
            evidence_completed_at: evidence.map(|value| value.completed_at()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorExecutionAttemptPageResponse {
    pub attempts: Vec<ConnectorExecutionAttemptResponse>,
    pub next_cursor: Option<String>,
}

impl ConnectorExecutionAttemptPageResponse {
    pub fn from_page(page: ConnectorExecutionAttemptPage, observed_at: DateTime<Utc>) -> Self {
        Self {
            attempts: page
                .attempts
                .into_iter()
                .map(|attempt| ConnectorExecutionAttemptResponse::from_record(attempt, observed_at))
                .collect(),
            next_cursor: page.next_cursor.map(|cursor| cursor.encode()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorExecutionAttemptResolutionResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub profile_id: Uuid,
    pub revision_id: Uuid,
    pub attempt_id: Uuid,
    pub request_digest: String,
    pub request_body_bytes: u64,
    pub dispatch_started_at: DateTime<Utc>,
    pub outcome_deadline_at: DateTime<Utc>,
    pub resolution: String,
    pub reason: String,
    pub resolved_by: Uuid,
    pub resolved_at: DateTime<Utc>,
}

impl From<ConnectorExecutionAttemptResolution> for ConnectorExecutionAttemptResolutionResponse {
    fn from(resolution: ConnectorExecutionAttemptResolution) -> Self {
        let binding = resolution.binding();
        Self {
            organization_id: binding.organization_id().as_uuid(),
            project_id: binding.project_id().as_uuid(),
            environment_id: binding.environment_id().as_uuid(),
            profile_id: binding.profile_id().as_uuid(),
            revision_id: binding.revision_id().as_uuid(),
            attempt_id: binding.attempt_id(),
            request_digest: binding.request_digest().to_string(),
            request_body_bytes: binding.request_body_bytes(),
            dispatch_started_at: resolution.dispatch_started_at(),
            outcome_deadline_at: resolution.outcome_deadline_at(),
            resolution: "indeterminate".into(),
            reason: resolution.reason().to_owned(),
            resolved_by: resolution.resolved_by().as_uuid(),
            resolved_at: resolution.resolved_at(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorExecutionAttemptResolutionMutationResponse {
    pub resolution: ConnectorExecutionAttemptResolutionResponse,
    pub replayed: bool,
}

impl From<ConnectorExecutionAttemptResolutionMutationResult>
    for ConnectorExecutionAttemptResolutionMutationResponse
{
    fn from(result: ConnectorExecutionAttemptResolutionMutationResult) -> Self {
        Self {
            resolution: result.resolution.into(),
            replayed: result.replayed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorProfileResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub profile_id: Uuid,
    pub name: String,
    pub current_revision_id: Uuid,
    pub current_revision_number: u64,
    pub current_revision_digest: String,
    pub aggregate_version: u64,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<ConnectorProfile> for ConnectorProfileResponse {
    fn from(profile: ConnectorProfile) -> Self {
        Self {
            organization_id: profile.organization_id.as_uuid(),
            project_id: profile.project_id.as_uuid(),
            environment_id: profile.environment_id.as_uuid(),
            profile_id: profile.id.as_uuid(),
            name: profile.name.as_str().to_owned(),
            current_revision_id: profile.current_revision_id.as_uuid(),
            current_revision_number: profile.current_revision_number,
            current_revision_digest: profile.current_revision_digest.as_str().to_owned(),
            aggregate_version: profile.aggregate_version,
            created_by: profile.created_by.as_uuid(),
            created_at: profile.created_at,
            updated_at: profile.updated_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorRevisionResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub profile_id: Uuid,
    pub revision_id: Uuid,
    pub revision_number: u64,
    pub parent_revision_id: Option<Uuid>,
    pub parent_digest: Option<String>,
    pub definition_kind: String,
    pub definition_schema: String,
    pub definition_acl: String,
    pub definition_digest: String,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

impl From<ConnectorRevision> for ConnectorRevisionResponse {
    fn from(revision: ConnectorRevision) -> Self {
        Self {
            organization_id: revision.organization_id.as_uuid(),
            project_id: revision.project_id.as_uuid(),
            environment_id: revision.environment_id.as_uuid(),
            profile_id: revision.profile_id.as_uuid(),
            revision_id: revision.id.as_uuid(),
            revision_number: revision.revision_number,
            parent_revision_id: revision.parent_revision_id.map(|value| value.as_uuid()),
            parent_digest: revision
                .parent_digest
                .map(|value| value.as_str().to_owned()),
            definition_kind: revision.definition.kind().to_owned(),
            definition_schema: revision.definition.schema().to_owned(),
            definition_acl: revision.definition.canonical_acl().to_owned(),
            definition_digest: revision.definition.digest().as_str().to_owned(),
            created_by: revision.created_by.as_uuid(),
            created_at: revision.created_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorRevisionRevocationResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub environment_id: Uuid,
    pub profile_id: Uuid,
    pub revision_id: Uuid,
    pub revision_number: u64,
    pub definition_digest: String,
    pub reason: String,
    pub revoked_by: Uuid,
    pub revoked_at: DateTime<Utc>,
}

impl From<ConnectorRevisionRevocation> for ConnectorRevisionRevocationResponse {
    fn from(revocation: ConnectorRevisionRevocation) -> Self {
        Self {
            organization_id: revocation.organization_id.as_uuid(),
            project_id: revocation.project_id.as_uuid(),
            environment_id: revocation.environment_id.as_uuid(),
            profile_id: revocation.profile_id.as_uuid(),
            revision_id: revocation.revision_id.as_uuid(),
            revision_number: revocation.revision_number,
            definition_digest: revocation.definition_digest.to_string(),
            reason: revocation.reason,
            revoked_by: revocation.revoked_by.as_uuid(),
            revoked_at: revocation.revoked_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorRevisionRevocationMutationResponse {
    pub revocation: ConnectorRevisionRevocationResponse,
    pub replayed: bool,
}

impl From<ConnectorRevisionRevocationMutationResult>
    for ConnectorRevisionRevocationMutationResponse
{
    fn from(result: ConnectorRevisionRevocationMutationResult) -> Self {
        Self {
            revocation: result.revocation.into(),
            replayed: result.replayed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorProfileRecordResponse {
    pub profile: ConnectorProfileResponse,
    pub revision: ConnectorRevisionResponse,
}

impl From<ConnectorRecord> for ConnectorProfileRecordResponse {
    fn from(record: ConnectorRecord) -> Self {
        Self {
            profile: record.profile.into(),
            revision: record.revision.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorProfileMutationResponse {
    pub record: ConnectorProfileRecordResponse,
    pub replayed: bool,
}

impl From<ConnectorProfileMutationResult> for ConnectorProfileMutationResponse {
    fn from(result: ConnectorProfileMutationResult) -> Self {
        Self {
            record: result.record.into(),
            replayed: result.replayed,
        }
    }
}
