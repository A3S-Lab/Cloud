use super::{ApplicationAudience, ApplicationRelease};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, ApplicationEndUserId, ApplicationId, OrganizationId, PrincipalId,
    ProjectId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// One Applications-owned delivery identity scoped to a single Application.
///
/// The optional Principal link is explicit and never creates a Membership,
/// role, Resource Grant, or credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationEndUser {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub id: ApplicationEndUserId,
    pub audience: ApplicationAudience,
    pub linked_principal_id: Option<PrincipalId>,
    pub created_by: PrincipalId,
    pub created_at: DateTime<Utc>,
}

impl ApplicationEndUser {
    pub fn create(
        id: ApplicationEndUserId,
        release: &ApplicationRelease,
        linked_principal_id: Option<PrincipalId>,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        release.validate()?;
        let value = Self {
            organization_id: release.organization_id,
            project_id: release.project_id,
            application_id: release.application_id,
            id,
            audience: release.contract.spec().audience,
            linked_principal_id,
            created_by,
            created_at: canonical_timestamp(created_at),
        };
        value.validate()?;
        if value.created_at < release.created_at {
            return Err("Application end user cannot predate its release".into());
        }
        Ok(value)
    }

    pub fn restore(mut self) -> Result<Self, String> {
        self.created_at = canonical_timestamp(self.created_at);
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.application_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.created_by.as_uuid().is_nil()
            || self
                .linked_principal_id
                .is_some_and(|principal_id| principal_id.as_uuid().is_nil())
            || self.created_at != canonical_timestamp(self.created_at)
        {
            return Err("stored Application end user identity is invalid".into());
        }
        match (self.audience, self.linked_principal_id) {
            (ApplicationAudience::ProjectMembers, None) => Err(
                "project-member Application end users require an explicit Principal link".into(),
            ),
            (ApplicationAudience::Anonymous, Some(_)) => Err(
                "anonymous Application end users cannot imply workspace Principal authority".into(),
            ),
            _ => Ok(()),
        }
    }

    pub fn validate_release(&self, release: &ApplicationRelease) -> Result<(), String> {
        self.validate()?;
        release.validate()?;
        if self.organization_id != release.organization_id
            || self.project_id != release.project_id
            || self.application_id != release.application_id
            || self.audience != release.contract.spec().audience
        {
            return Err("Application end user is outside the exact release audience".into());
        }
        Ok(())
    }
}
