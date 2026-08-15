use crate::modules::connectors::application::ConnectorProfileMutationResult;
use crate::modules::connectors::domain::{ConnectorProfile, ConnectorRecord, ConnectorRevision};
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
