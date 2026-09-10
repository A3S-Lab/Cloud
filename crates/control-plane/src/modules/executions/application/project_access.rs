use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId, RepositoryError};
use async_trait::async_trait;

/// Exact Projects-owned project identity required by Executions template
/// commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionsProjectScope {
    organization_id: OrganizationId,
    project_id: ProjectId,
}

impl ExecutionsProjectScope {
    pub fn new(organization_id: OrganizationId, project_id: ProjectId) -> Result<Self, String> {
        let scope = Self {
            organization_id,
            project_id,
        };
        scope.validate()?;
        Ok(scope)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil() || self.project_id.as_uuid().is_nil() {
            return Err("Executions project scope requires non-nil identities".into());
        }
        Ok(())
    }

    pub const fn organization_id(self) -> OrganizationId {
        self.organization_id
    }

    pub const fn project_id(self) -> ProjectId {
        self.project_id
    }
}

/// Identity-owned read port for the Projects project authority.
#[async_trait]
pub trait IExecutionsProjectAccess: Send + Sync {
    async fn project_exists(&self, scope: ExecutionsProjectScope) -> Result<bool, RepositoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn scope_rejects_nil_owner_identities() {
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        assert!(ExecutionsProjectScope::new(organization_id, project_id).is_ok());
        assert!(
            ExecutionsProjectScope::new(OrganizationId::from_uuid(Uuid::nil()), project_id,)
                .is_err()
        );
    }
}
