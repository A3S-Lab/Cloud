use crate::modules::shared_kernel::domain::{
    ApplicationId, OrganizationId, ProjectId, RepositoryError,
};
use async_trait::async_trait;

/// Exact Applications-owned application identity required by Identity Resource
/// Grant commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdentityApplicationScope {
    organization_id: OrganizationId,
    project_id: ProjectId,
    application_id: ApplicationId,
}

impl IdentityApplicationScope {
    pub fn new(
        organization_id: OrganizationId,
        project_id: ProjectId,
        application_id: ApplicationId,
    ) -> Result<Self, String> {
        let scope = Self {
            organization_id,
            project_id,
            application_id,
        };
        scope.validate()?;
        Ok(scope)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.application_id.as_uuid().is_nil()
        {
            return Err("Identity application scope requires non-nil identities".into());
        }
        Ok(())
    }

    pub const fn organization_id(self) -> OrganizationId {
        self.organization_id
    }

    pub const fn project_id(self) -> ProjectId {
        self.project_id
    }

    pub const fn application_id(self) -> ApplicationId {
        self.application_id
    }
}

/// Identity-owned read port for the Applications application authority.
///
/// Only exact existence evidence crosses the boundary; Applications aggregates
/// and repositories remain outside Identity Application.
#[async_trait]
pub trait IIdentityApplicationAccess: Send + Sync {
    async fn application_exists(
        &self,
        scope: IdentityApplicationScope,
    ) -> Result<bool, RepositoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn scope_rejects_nil_owner_identities() {
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        let application_id = ApplicationId::new();

        assert!(
            IdentityApplicationScope::new(organization_id, project_id, application_id).is_ok()
        );
        assert!(IdentityApplicationScope::new(
            OrganizationId::from_uuid(Uuid::nil()),
            project_id,
            application_id,
        )
        .is_err());
    }
}
