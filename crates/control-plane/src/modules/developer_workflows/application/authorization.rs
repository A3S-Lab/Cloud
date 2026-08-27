use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, PrincipalId, ProjectId, RepositoryError,
};
use async_trait::async_trait;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeveloperWorkflowAction {
    DetectBuildPlan,
    ReadBuildPlan,
    AcceptBuildPlan,
    AcceptWorkloadProfile,
    AcceptPullRequestPreviewPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeveloperWorkflowEnvironmentAccess {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub principal_id: PrincipalId,
    pub action: DeveloperWorkflowAction,
}

impl DeveloperWorkflowEnvironmentAccess {
    pub fn validate(&self) -> Result<(), String> {
        if self.organization_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.environment_id.as_uuid().is_nil()
            || self.principal_id.as_uuid().is_nil()
        {
            return Err("Developer Workflow environment access scope is invalid".into());
        }
        Ok(())
    }
}

/// Consumer-owned authorization port.
///
/// A Developer Workflows infrastructure adapter bridges Identity and Projects
/// owner interfaces without exposing their policy vocabulary to Application.
/// A concealed or absent environment returns `false`; provider/storage failures
/// remain typed repository errors.
#[async_trait]
pub trait IDeveloperWorkflowAuthorizationPort: Send + Sync {
    async fn is_environment_action_allowed(
        &self,
        access: DeveloperWorkflowEnvironmentAccess,
    ) -> Result<bool, RepositoryError>;
}

pub(super) async fn authorize_environment_action(
    authorization: &dyn IDeveloperWorkflowAuthorizationPort,
    access: DeveloperWorkflowEnvironmentAccess,
) -> ApplicationResult<()> {
    access.validate().map_err(|_| concealed_environment())?;
    match authorization.is_environment_action_allowed(access).await {
        Ok(true) => Ok(()),
        Ok(false) | Err(RepositoryError::NotFound | RepositoryError::Forbidden(_)) => {
            Err(concealed_environment())
        }
        Err(error) => Err(error.into()),
    }
}

fn concealed_environment() -> ApplicationError {
    ApplicationError::NotFound("Developer Workflow environment not found".into())
}
