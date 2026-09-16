use crate::modules::applications::IApplicationRepository;
use crate::modules::identity::application::{
    IIdentityApplicationAccess, IdentityApplicationScope,
};
use crate::modules::shared_kernel::domain::RepositoryError;
use async_trait::async_trait;
use std::sync::Arc;

/// Read-only anti-corruption adapter for the Applications application authority.
#[derive(Clone)]
pub struct ApplicationsIdentityApplicationAccessAdapter {
    applications: Arc<dyn IApplicationRepository>,
}

impl ApplicationsIdentityApplicationAccessAdapter {
    pub fn new(applications: Arc<dyn IApplicationRepository>) -> Self {
        Self { applications }
    }
}

#[async_trait]
impl IIdentityApplicationAccess for ApplicationsIdentityApplicationAccessAdapter {
    async fn application_exists(
        &self,
        scope: IdentityApplicationScope,
    ) -> Result<bool, RepositoryError> {
        scope.validate().map_err(RepositoryError::Forbidden)?;
        match self
            .applications
            .find(
                scope.organization_id(),
                scope.project_id(),
                scope.application_id(),
            )
            .await?
        {
            Some(application)
                if application.organization_id == scope.organization_id()
                    && application.project_id == scope.project_id()
                    && application.id == scope.application_id()
                    && application.aggregate_version > 0 =>
            {
                Ok(true)
            }
            Some(_) => Err(RepositoryError::Storage(
                "Applications returned inconsistent Identity application evidence".into(),
            )),
            None => Ok(false),
        }
    }
}
