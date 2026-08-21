use super::{ApplicationAudience, ApplicationRelease};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, ApplicationEndUserId, ApplicationId, OrganizationId, PrincipalId,
    ProjectId,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const PROJECT_MEMBER_END_USER_IDENTITY: &[u8] = b"cloud.application.end-user.project-member.v1";

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
    /// Derive the one Application-scoped delivery identity for a project
    /// Principal. Stable identity lets independently retried session commands
    /// adopt the same end user without creating a second Identity authority.
    pub fn project_member_id(
        application_id: ApplicationId,
        principal_id: PrincipalId,
    ) -> Result<ApplicationEndUserId, String> {
        if application_id.as_uuid().is_nil() || principal_id.as_uuid().is_nil() {
            return Err("Application project-member end user identity is invalid".into());
        }
        let mut identity = Vec::with_capacity(PROJECT_MEMBER_END_USER_IDENTITY.len() + 17);
        identity.extend_from_slice(PROJECT_MEMBER_END_USER_IDENTITY);
        identity.push(0);
        identity.extend_from_slice(principal_id.as_uuid().as_bytes());
        Ok(ApplicationEndUserId::from_uuid(Uuid::new_v5(
            &application_id.as_uuid(),
            &identity,
        )))
    }

    pub fn project_member(
        release: &ApplicationRelease,
        principal_id: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        release.validate()?;
        if release.contract.spec().audience != ApplicationAudience::ProjectMembers {
            return Err(
                "project-authorized Application delivery requires a project-member release".into(),
            );
        }
        Self::create(
            Self::project_member_id(release.application_id, principal_id)?,
            release,
            Some(principal_id),
            principal_id,
            created_at,
        )
    }

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

    pub fn validate_project_member(
        &self,
        release: &ApplicationRelease,
        principal_id: PrincipalId,
    ) -> Result<(), String> {
        self.validate_release(release)?;
        if self.audience != ApplicationAudience::ProjectMembers
            || self.linked_principal_id != Some(principal_id)
            || self.id != Self::project_member_id(self.application_id, principal_id)?
        {
            return Err("Application session is not owned by the requesting project member".into());
        }
        Ok(())
    }
}
