use crate::modules::shared_kernel::domain::{OrganizationId, ProjectId, RepositoryError};
use async_trait::async_trait;

/// Exact Projects-owned project identity required by Workflow project-scoped
/// commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkflowProjectScope {
    organization_id: OrganizationId,
    project_id: ProjectId,
}

impl WorkflowProjectScope {
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
            return Err("Workflow project scope requires non-nil identities".into());
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

/// Workflow-owned read port for the Projects project authority.
#[async_trait]
pub trait IWorkflowProjectAccess: Send + Sync {
    async fn project_exists(&self, scope: WorkflowProjectScope) -> Result<bool, RepositoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn scope_rejects_nil_owner_identities() {
        let organization_id = OrganizationId::new();
        let project_id = ProjectId::new();
        assert!(WorkflowProjectScope::new(organization_id, project_id).is_ok());
        assert!(
            WorkflowProjectScope::new(OrganizationId::from_uuid(Uuid::nil()), project_id,).is_err()
        );
    }
}
