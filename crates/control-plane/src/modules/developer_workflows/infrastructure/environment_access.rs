use crate::modules::developer_workflows::application::{
    DeveloperWorkflowEnvironmentScope, IDeveloperWorkflowEnvironmentPort,
};
use crate::modules::projects::domain::repositories::IEnvironmentRepository;
use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;
use std::sync::Arc;

/// Read-only anti-corruption adapter for the Projects environment authority.
///
/// Identity visibility is already narrowed at the process boundary. This
/// adapter therefore has one responsibility: verify that Projects owns the
/// exact environment scope and fail on inconsistent owner evidence.
#[derive(Clone)]
pub struct ProjectsDeveloperWorkflowEnvironmentAdapter {
    environments: Arc<dyn IEnvironmentRepository>,
}

impl ProjectsDeveloperWorkflowEnvironmentAdapter {
    pub fn new(environments: Arc<dyn IEnvironmentRepository>) -> Self {
        Self { environments }
    }
}

#[async_trait]
impl IDeveloperWorkflowEnvironmentPort for ProjectsDeveloperWorkflowEnvironmentAdapter {
    async fn environment_exists(
        &self,
        scope: DeveloperWorkflowEnvironmentScope,
    ) -> Result<bool, RepositoryError> {
        scope.validate().map_err(RepositoryError::Forbidden)?;
        match self
            .environments
            .find(
                scope.organization_id,
                scope.project_id,
                scope.environment_id,
            )
            .await?
        {
            Some(environment)
                if environment.organization_id == scope.organization_id
                    && environment.project_id == scope.project_id
                    && environment.id == scope.environment_id
                    && environment.aggregate_version > 0 =>
            {
                Ok(true)
            }
            Some(_) => Err(RepositoryError::Storage(
                "Projects returned inconsistent Developer Workflow environment evidence".into(),
            )),
            None => Ok(false),
        }
    }
}
