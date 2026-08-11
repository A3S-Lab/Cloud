use crate::modules::forms::domain::{FormDraft, FormRelease};
use a3s_cloud_contracts::DomainEventEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormDraftChanged {
    pub project_id: Uuid,
    pub form_id: Uuid,
    pub draft_digest: String,
    pub latest_release_id: Option<Uuid>,
}

impl FormDraftChanged {
    pub fn created(
        draft: &FormDraft,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope("form.draft.created", draft, correlation_id)
    }

    pub fn revised(
        draft: &FormDraft,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        Self::envelope("form.draft.revised", draft, correlation_id)
    }

    fn envelope(
        event_key: &'static str,
        draft: &FormDraft,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        let payload = Self {
            project_id: draft.project_id.as_uuid(),
            form_id: draft.id.as_uuid(),
            draft_digest: draft.document.digest().as_str().to_owned(),
            latest_release_id: draft
                .latest_release
                .as_ref()
                .map(|release| release.id.as_uuid()),
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: event_key.into(),
            schema_version: 1,
            organization_id: draft.organization_id.as_uuid(),
            aggregate_id: draft.id.as_uuid(),
            aggregate_version: draft.aggregate_version,
            occurred_at: draft.updated_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(payload)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormReleasePublished {
    pub project_id: Uuid,
    pub form_id: Uuid,
    pub release_id: Uuid,
    pub revision: u64,
    pub source_draft_version: u64,
    pub content_digest: String,
    pub compiler_revision: String,
    pub schema_profile: String,
}

impl FormReleasePublished {
    pub fn envelope(
        draft: &FormDraft,
        release: &FormRelease,
        correlation_id: Uuid,
    ) -> Result<DomainEventEnvelope, serde_json::Error> {
        let payload = Self {
            project_id: release.project_id.as_uuid(),
            form_id: release.form_id.as_uuid(),
            release_id: release.id.as_uuid(),
            revision: release.revision,
            source_draft_version: release.source_draft_version,
            content_digest: release.content.digest().as_str().to_owned(),
            compiler_revision: release.content.compiler_revision().to_owned(),
            schema_profile: release.content.schema_profile().to_owned(),
        };
        Ok(DomainEventEnvelope {
            event_id: Uuid::now_v7(),
            event_key: "form.release.published".into(),
            schema_version: 1,
            organization_id: release.organization_id.as_uuid(),
            aggregate_id: release.form_id.as_uuid(),
            aggregate_version: draft.aggregate_version,
            occurred_at: release.published_at,
            correlation_id,
            causation_id: None,
            payload: serde_json::to_value(payload)?,
        })
    }
}
