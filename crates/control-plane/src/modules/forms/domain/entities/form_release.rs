use super::FormDraft;
use crate::modules::forms::domain::FormReleaseContent;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, FormId, FormReleaseId, OrganizationId, PrincipalId, ProjectId,
};
use a3s_form_core::{FormReleaseMode, FormReleaseRef, FORM_RELEASE_REF_API_VERSION};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FormRelease {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub form_id: FormId,
    pub id: FormReleaseId,
    pub revision: u64,
    pub source_draft_version: u64,
    pub name: String,
    pub description: String,
    pub content: FormReleaseContent,
    pub published_by: PrincipalId,
    pub published_at: DateTime<Utc>,
}

impl FormRelease {
    pub fn publish(
        draft: &FormDraft,
        id: FormReleaseId,
        content: FormReleaseContent,
        published_by: PrincipalId,
        published_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        draft.validate()?;
        let revision = match &draft.latest_release {
            Some(latest) => latest
                .revision
                .checked_add(1)
                .ok_or_else(|| "Form release revision is exhausted".to_owned())?,
            None => 1,
        };
        let published_at = canonical_timestamp(published_at);
        if published_at < draft.updated_at {
            return Err("Form release publication time precedes the source draft version".into());
        }
        let value = Self {
            organization_id: draft.organization_id,
            project_id: draft.project_id,
            form_id: draft.id,
            id,
            revision,
            source_draft_version: draft.aggregate_version,
            name: draft.name.clone(),
            description: draft.description.clone(),
            content,
            published_by,
            published_at,
        };
        value.validate()?;
        Ok(value)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        organization_id: OrganizationId,
        project_id: ProjectId,
        form_id: FormId,
        id: FormReleaseId,
        revision: u64,
        source_draft_version: u64,
        name: String,
        description: String,
        content: FormReleaseContent,
        published_by: PrincipalId,
        published_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let value = Self {
            organization_id,
            project_id,
            form_id,
            id,
            revision,
            source_draft_version,
            name,
            description,
            content,
            published_by,
            published_at: canonical_timestamp(published_at),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.form_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.published_by.as_uuid().is_nil()
            || self.revision == 0
            || self.source_draft_version == 0
        {
            return Err("stored Form release identity or revision is invalid".into());
        }
        super::super::validation::validate_form_identity_text(&self.name, &self.description)?;
        self.content.validate()
    }

    pub fn release_ref(&self) -> Result<FormReleaseRef, String> {
        self.validate()?;
        let value = FormReleaseRef {
            api_version: FORM_RELEASE_REF_API_VERSION.into(),
            organization_id: self.organization_id.to_string(),
            project_id: self.project_id.to_string(),
            form_id: self.form_id.to_string(),
            release_id: self.id.to_string(),
            uri: format!("a3s://forms/{}/releases/{}", self.form_id, self.id),
            revision: self.revision,
            digest: self.content.digest().to_string(),
            compiler_revision: self.content.compiler_revision().to_owned(),
            schema_profile: self.content.schema_profile().to_owned(),
            mode: FormReleaseMode::Interaction,
        };
        value
            .validate()
            .map_err(|error| format!("Form release reference is invalid: {error}"))?;
        Ok(value)
    }
}
