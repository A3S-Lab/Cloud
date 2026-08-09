use crate::modules::forms::application::{FormDraftMutationResult, FormPublicationMutationResult};
use crate::modules::forms::domain::{FormDraft, FormRelease, FormReleaseSummary};
use a3s_form_core::FormReleaseRef;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormReleaseSummaryResponse {
    pub id: Uuid,
    pub revision: u64,
    pub source_draft_version: u64,
    pub digest: String,
    pub published_at: DateTime<Utc>,
}

impl From<FormReleaseSummary> for FormReleaseSummaryResponse {
    fn from(value: FormReleaseSummary) -> Self {
        Self {
            id: value.id.as_uuid(),
            revision: value.revision,
            source_draft_version: value.source_draft_version,
            digest: value.digest.to_string(),
            published_at: value.published_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormDraftResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub document: serde_json::Value,
    pub draft_digest: String,
    pub aggregate_version: u64,
    pub latest_release: Option<FormReleaseSummaryResponse>,
    pub created_by: Uuid,
    pub updated_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl TryFrom<FormDraft> for FormDraftResponse {
    type Error = String;

    fn try_from(value: FormDraft) -> Result<Self, Self::Error> {
        let document = serde_json::from_str(value.document.canonical_json())
            .map_err(|error| format!("stored Form draft response is invalid: {error}"))?;
        Ok(Self {
            organization_id: value.organization_id.as_uuid(),
            project_id: value.project_id.as_uuid(),
            id: value.id.as_uuid(),
            name: value.name,
            description: value.description,
            document,
            draft_digest: value.document.digest().to_string(),
            aggregate_version: value.aggregate_version,
            latest_release: value.latest_release.map(Into::into),
            created_by: value.created_by.as_uuid(),
            updated_by: value.updated_by.as_uuid(),
            created_at: value.created_at,
            updated_at: value.updated_at,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormReleaseResponse {
    pub organization_id: Uuid,
    pub project_id: Uuid,
    pub form_id: Uuid,
    pub id: Uuid,
    pub revision: u64,
    pub source_draft_version: u64,
    pub name: String,
    pub description: String,
    pub normalized_document: serde_json::Value,
    pub form_plan: serde_json::Value,
    pub compiler_revision: String,
    pub schema_profile: String,
    pub content_digest: String,
    pub release_ref: FormReleaseRef,
    pub published_by: Uuid,
    pub published_at: DateTime<Utc>,
}

impl TryFrom<FormRelease> for FormReleaseResponse {
    type Error = String;

    fn try_from(value: FormRelease) -> Result<Self, Self::Error> {
        let normalized_document = serde_json::from_str(value.content.normalized_document_json())
            .map_err(|error| format!("stored normalized Form response is invalid: {error}"))?;
        let form_plan = serde_json::from_str(value.content.form_plan_json())
            .map_err(|error| format!("stored Form plan response is invalid: {error}"))?;
        let release_ref = value.release_ref()?;
        Ok(Self {
            organization_id: value.organization_id.as_uuid(),
            project_id: value.project_id.as_uuid(),
            form_id: value.form_id.as_uuid(),
            id: value.id.as_uuid(),
            revision: value.revision,
            source_draft_version: value.source_draft_version,
            name: value.name,
            description: value.description,
            normalized_document,
            form_plan,
            compiler_revision: value.content.compiler_revision().to_owned(),
            schema_profile: value.content.schema_profile().to_owned(),
            content_digest: value.content.digest().to_string(),
            release_ref,
            published_by: value.published_by.as_uuid(),
            published_at: value.published_at,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormDraftMutationResponse {
    pub form: FormDraftResponse,
    pub replayed: bool,
}

impl TryFrom<FormDraftMutationResult> for FormDraftMutationResponse {
    type Error = String;

    fn try_from(value: FormDraftMutationResult) -> Result<Self, Self::Error> {
        Ok(Self {
            form: FormDraftResponse::try_from(value.draft)?,
            replayed: value.replayed,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormPublicationMutationResponse {
    pub form: FormDraftResponse,
    pub release: FormReleaseResponse,
    pub replayed: bool,
}

impl TryFrom<FormPublicationMutationResult> for FormPublicationMutationResponse {
    type Error = String;

    fn try_from(value: FormPublicationMutationResult) -> Result<Self, Self::Error> {
        Ok(Self {
            form: FormDraftResponse::try_from(value.publication.draft)?,
            release: FormReleaseResponse::try_from(value.publication.release)?,
            replayed: value.replayed,
        })
    }
}
