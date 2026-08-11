use super::FormRelease;
use crate::modules::forms::domain::FormDocument;
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, FormId, FormReleaseId, OrganizationId, PrincipalId, ProjectId,
    Sha256Digest,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FormReleaseSummary {
    pub id: FormReleaseId,
    pub revision: u64,
    pub source_draft_version: u64,
    pub digest: Sha256Digest,
    pub published_at: DateTime<Utc>,
}

impl FormReleaseSummary {
    fn from_release(release: &FormRelease) -> Self {
        Self {
            id: release.id,
            revision: release.revision,
            source_draft_version: release.source_draft_version,
            digest: release.content.digest().clone(),
            published_at: release.published_at,
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.id.as_uuid().is_nil() || self.revision == 0 || self.source_draft_version == 0 {
            return Err("stored latest Form release summary is invalid".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FormDraft {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub id: FormId,
    pub name: String,
    pub description: String,
    pub document: FormDocument,
    pub aggregate_version: u64,
    pub latest_release: Option<FormReleaseSummary>,
    pub created_by: PrincipalId,
    pub updated_by: PrincipalId,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl FormDraft {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        organization_id: OrganizationId,
        project_id: ProjectId,
        id: FormId,
        name: String,
        description: String,
        document: FormDocument,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let created_at = canonical_timestamp(created_at);
        let value = Self {
            organization_id,
            project_id,
            id,
            name,
            description,
            document,
            aggregate_version: 1,
            latest_release: None,
            created_by,
            updated_by: created_by,
            created_at,
            updated_at: created_at,
        };
        value.validate()?;
        Ok(value)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        organization_id: OrganizationId,
        project_id: ProjectId,
        id: FormId,
        name: String,
        description: String,
        document: FormDocument,
        aggregate_version: u64,
        latest_release: Option<FormReleaseSummary>,
        created_by: PrincipalId,
        updated_by: PrincipalId,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let value = Self {
            organization_id,
            project_id,
            id,
            name,
            description,
            document,
            aggregate_version,
            latest_release,
            created_by,
            updated_by,
            created_at: canonical_timestamp(created_at),
            updated_at: canonical_timestamp(updated_at),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn revise(
        &self,
        expected_version: u64,
        name: String,
        description: String,
        document: FormDocument,
        updated_by: PrincipalId,
        updated_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        self.require_version(expected_version)?;
        if self.name == name && self.description == description && self.document == document {
            return Err("Form draft revision must change its content or metadata".into());
        }
        let updated_at = canonical_timestamp(updated_at);
        if updated_at < self.updated_at {
            return Err("Form draft update time precedes its current version".into());
        }
        let aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Form draft aggregate version is exhausted".to_owned())?;
        let value = Self {
            organization_id: self.organization_id,
            project_id: self.project_id,
            id: self.id,
            name,
            description,
            document,
            aggregate_version,
            latest_release: self.latest_release.clone(),
            created_by: self.created_by,
            updated_by,
            created_at: self.created_at,
            updated_at,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn record_release(
        &self,
        expected_version: u64,
        release: &FormRelease,
    ) -> Result<Self, String> {
        self.require_version(expected_version)?;
        release.validate()?;
        let expected_revision = match &self.latest_release {
            Some(latest) => latest
                .revision
                .checked_add(1)
                .ok_or_else(|| "Form release revision is exhausted".to_owned())?,
            None => 1,
        };
        if release.organization_id != self.organization_id
            || release.project_id != self.project_id
            || release.form_id != self.id
            || release.revision != expected_revision
            || release.source_draft_version != self.aggregate_version
            || release.name != self.name
            || release.description != self.description
        {
            return Err("Form release does not match the current draft version".into());
        }
        if release.published_at < self.updated_at {
            return Err("Form release publication time precedes the current draft version".into());
        }
        let aggregate_version = self
            .aggregate_version
            .checked_add(1)
            .ok_or_else(|| "Form draft aggregate version is exhausted".to_owned())?;
        let value = Self {
            organization_id: self.organization_id,
            project_id: self.project_id,
            id: self.id,
            name: self.name.clone(),
            description: self.description.clone(),
            document: self.document.clone(),
            aggregate_version,
            latest_release: Some(FormReleaseSummary::from_release(release)),
            created_by: self.created_by,
            updated_by: release.published_by,
            created_at: self.created_at,
            updated_at: release.published_at,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.created_by.as_uuid().is_nil()
            || self.updated_by.as_uuid().is_nil()
            || self.aggregate_version == 0
            || self.updated_at < self.created_at
        {
            return Err(
                "stored Form draft identity, version, actor, or timestamp is invalid".into(),
            );
        }
        super::super::validation::validate_form_identity_text(&self.name, &self.description)?;
        self.document.validate()?;
        if let Some(latest) = &self.latest_release {
            latest.validate()?;
            if latest.source_draft_version >= self.aggregate_version
                || latest.published_at < self.created_at
                || latest.published_at > self.updated_at
            {
                return Err("stored latest Form release does not match its draft head".into());
            }
        }
        Ok(())
    }

    fn require_version(&self, expected_version: u64) -> Result<(), String> {
        if expected_version == 0 || expected_version != self.aggregate_version {
            Err("Form draft aggregate version does not match the expected version".into())
        } else {
            Ok(())
        }
    }
}
