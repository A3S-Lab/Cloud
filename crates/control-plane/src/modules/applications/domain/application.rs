use super::{ApplicationExperience, ApplicationReleaseContract};
use crate::modules::shared_kernel::domain::{
    canonical_timestamp, ApplicationId, ApplicationReleaseId, OrganizationId, PrincipalId,
    ProjectId, ResourceName, Sha256Digest,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const APPLICATION_DESCRIPTION_MAX_CHARS: usize = 4_096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationRelease {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub id: ApplicationReleaseId,
    pub release_number: u64,
    pub parent_release_id: Option<ApplicationReleaseId>,
    pub parent_digest: Option<Sha256Digest>,
    pub contract: ApplicationReleaseContract,
    pub created_by: PrincipalId,
    pub created_at: DateTime<Utc>,
}

impl ApplicationRelease {
    #[allow(clippy::too_many_arguments)]
    pub fn initial(
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        id: ApplicationReleaseId,
        contract: ApplicationReleaseContract,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let value = Self {
            organization_id,
            project_id,
            application_id,
            id,
            release_number: 1,
            parent_release_id: None,
            parent_digest: None,
            contract,
            created_by,
            created_at: canonical_timestamp(created_at),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn successor(
        parent: &Self,
        id: ApplicationReleaseId,
        contract: ApplicationReleaseContract,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        parent.validate()?;
        contract.validate()?;
        if contract.spec().experience != parent.contract.spec().experience {
            return Err(
                "Application experience is immutable; create another Application identity".into(),
            );
        }
        if contract.digest() == parent.contract.digest() {
            return Err("successor Application release must change its contract digest".into());
        }
        let created_at = canonical_timestamp(created_at);
        if created_at < parent.created_at {
            return Err("Application release time must not precede its parent".into());
        }
        let value = Self {
            organization_id: parent.organization_id,
            project_id: parent.project_id,
            application_id: parent.application_id,
            id,
            release_number: parent
                .release_number
                .checked_add(1)
                .ok_or_else(|| "Application release number is exhausted".to_owned())?,
            parent_release_id: Some(parent.id),
            parent_digest: Some(parent.contract.digest().clone()),
            contract,
            created_by,
            created_at,
        };
        value.validate()?;
        Ok(value)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
        id: ApplicationReleaseId,
        release_number: u64,
        parent_release_id: Option<ApplicationReleaseId>,
        parent_digest: Option<Sha256Digest>,
        canonical_acl: &str,
        stored_digest: &str,
        created_by: PrincipalId,
        created_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let value = Self {
            organization_id,
            project_id,
            application_id,
            id,
            release_number,
            parent_release_id,
            parent_digest,
            contract: ApplicationReleaseContract::restore(canonical_acl, stored_digest)?,
            created_by,
            created_at: canonical_timestamp(created_at),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if let Some(parent_digest) = &self.parent_digest {
            if Sha256Digest::parse(parent_digest.as_str())? != *parent_digest {
                return Err("Application release parent digest is not canonical".into());
            }
        }
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.application_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.created_by.as_uuid().is_nil()
            || self.release_number == 0
            || self.created_at != canonical_timestamp(self.created_at)
        {
            return Err("stored Application release identity or timestamp is invalid".into());
        }
        self.contract.validate()?;
        match (&self.parent_release_id, &self.parent_digest) {
            (None, None) if self.release_number == 1 => Ok(()),
            (Some(parent_id), Some(parent_digest))
                if self.release_number > 1
                    && !parent_id.as_uuid().is_nil()
                    && parent_digest != self.contract.digest() =>
            {
                Ok(())
            }
            _ => Err("Application release lineage is invalid".into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Application {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub id: ApplicationId,
    pub name: ResourceName,
    pub description: String,
    pub experience: ApplicationExperience,
    pub current_release_id: ApplicationReleaseId,
    pub current_release_number: u64,
    pub current_release_digest: Sha256Digest,
    pub aggregate_version: u64,
    pub created_by: PrincipalId,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Application {
    pub fn create(
        id: ApplicationId,
        name: ResourceName,
        description: String,
        release: &ApplicationRelease,
    ) -> Result<Self, String> {
        release.validate()?;
        validate_description(&description)?;
        if id != release.application_id || release.release_number != 1 {
            return Err("initial Application release does not belong to the Application".into());
        }
        let value = Self {
            organization_id: release.organization_id,
            project_id: release.project_id,
            id,
            name,
            description,
            experience: release.contract.spec().experience,
            current_release_id: release.id,
            current_release_number: release.release_number,
            current_release_digest: release.contract.digest().clone(),
            aggregate_version: 1,
            created_by: release.created_by,
            created_at: release.created_at,
            updated_at: release.created_at,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn advance(
        &self,
        expected_version: u64,
        release: &ApplicationRelease,
    ) -> Result<Self, String> {
        self.validate()?;
        release.validate()?;
        if expected_version == 0
            || self.aggregate_version != expected_version
            || release.organization_id != self.organization_id
            || release.project_id != self.project_id
            || release.application_id != self.id
            || release.release_number != expected_version.saturating_add(1)
            || release.parent_release_id != Some(self.current_release_id)
            || release.parent_digest.as_ref() != Some(&self.current_release_digest)
            || release.contract.spec().experience != self.experience
            || release.created_at < self.updated_at
        {
            return Err("Application was revised from a stale or foreign release".into());
        }
        let aggregate_version = expected_version
            .checked_add(1)
            .ok_or_else(|| "Application aggregate version is exhausted".to_owned())?;
        let value = Self {
            organization_id: self.organization_id,
            project_id: self.project_id,
            id: self.id,
            name: self.name.clone(),
            description: self.description.clone(),
            experience: self.experience,
            current_release_id: release.id,
            current_release_number: release.release_number,
            current_release_digest: release.contract.digest().clone(),
            aggregate_version,
            created_by: self.created_by,
            created_at: self.created_at,
            updated_at: release.created_at,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn at_release(&self, release: &ApplicationRelease) -> Result<Self, String> {
        self.validate()?;
        release.validate()?;
        if release.organization_id != self.organization_id
            || release.project_id != self.project_id
            || release.application_id != self.id
            || release.contract.spec().experience != self.experience
            || release.created_at < self.created_at
            || release.release_number > self.current_release_number
        {
            return Err("Application release does not belong to this Application".into());
        }
        let value = Self {
            organization_id: self.organization_id,
            project_id: self.project_id,
            id: self.id,
            name: self.name.clone(),
            description: self.description.clone(),
            experience: self.experience,
            current_release_id: release.id,
            current_release_number: release.release_number,
            current_release_digest: release.contract.digest().clone(),
            aggregate_version: release.release_number,
            created_by: self.created_by,
            created_at: self.created_at,
            updated_at: release.created_at,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        let canonical_name = ResourceName::parse(self.name.as_str().to_owned())?;
        validate_description(&self.description)?;
        let canonical_release_digest = Sha256Digest::parse(self.current_release_digest.as_str())?;
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.id.as_uuid().is_nil()
            || self.current_release_id.as_uuid().is_nil()
            || self.created_by.as_uuid().is_nil()
            || self.current_release_number == 0
            || self.aggregate_version == 0
            || self.current_release_number != self.aggregate_version
            || self.created_at != canonical_timestamp(self.created_at)
            || self.updated_at != canonical_timestamp(self.updated_at)
            || self.updated_at < self.created_at
            || canonical_name != self.name
            || canonical_release_digest != self.current_release_digest
        {
            return Err("stored Application aggregate is invalid".into());
        }
        Ok(())
    }
}

fn validate_description(value: &str) -> Result<(), String> {
    if value.chars().count() > APPLICATION_DESCRIPTION_MAX_CHARS || value.contains(['\0', '\r']) {
        return Err("Application description exceeds its text bound".into());
    }
    Ok(())
}
