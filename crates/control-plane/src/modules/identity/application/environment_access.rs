use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, RepositoryError,
};
use async_trait::async_trait;

/// Exact Projects-owned environment identity required by Identity inference-key
/// commands and queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdentityEnvironmentScope {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
}

impl IdentityEnvironmentScope {
    pub fn new(
        organization_id: OrganizationId,
        project_id: ProjectId,
        environment_id: EnvironmentId,
    ) -> Result<Self, String> {
        let scope = Self {
            organization_id,
            project_id,
            environment_id,
        };
        scope.validate()?;
        Ok(scope)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
        {
            return Err("Identity environment scope requires non-nil identities".into());
        }
        Ok(())
    }

    pub const fn organization_id(self) -> OrganizationId {
        self.organization_id
    }

    pub const fn project_id(self) -> ProjectId {
        self.project_id
    }

    pub const fn environment_id(self) -> EnvironmentId {
        self.environment_id
    }
}

/// Identity-owned read port for the Projects environment authority.
///
/// Only exact existence evidence crosses the boundary; Projects aggregates and
/// repositories remain outside Identity Application.
#[async_trait]
pub trait IIdentityEnvironmentAccess: Send + Sync {
    async fn environment_exists(
        &self,
        scope: IdentityEnvironmentScope,
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
        let environment_id = EnvironmentId::new();

        assert!(IdentityEnvironmentScope::new(organization_id, project_id, environment_id).is_ok());
        assert!(IdentityEnvironmentScope::new(
            OrganizationId::from_uuid(Uuid::nil()),
            project_id,
            environment_id,
        )
        .is_err());
    }
}
