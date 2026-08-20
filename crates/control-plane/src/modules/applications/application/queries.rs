use super::resource_access::{application_not_found, project, release_not_found};
use crate::modules::applications::domain::{
    Application, ApplicationRelease, IApplicationRepository,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ApplicationId, ApplicationReleaseId, OrganizationId, ProjectId, RepositoryError,
};
use a3s_boot::{Query, QueryHandler};
use std::sync::Arc;

pub const DEFAULT_APPLICATION_LIST_LIMIT: usize = 50;
pub const MAXIMUM_APPLICATION_LIST_LIMIT: usize = 200;

#[derive(Debug, Clone)]
pub struct GetApplication {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetApplication {
    type Output = ApplicationResult<Application>;
}

pub struct GetApplicationHandler {
    applications: Arc<dyn IApplicationRepository>,
}

impl GetApplicationHandler {
    pub fn new(applications: Arc<dyn IApplicationRepository>) -> Self {
        Self { applications }
    }
}

impl QueryHandler<GetApplication> for GetApplicationHandler {
    fn execute(
        &self,
        query: GetApplication,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Application>>> {
        let applications = Arc::clone(&self.applications);
        Box::pin(async move {
            if let Err(error) = project(query.project_id, &query.resource_access) {
                return Ok(Err(error));
            }
            match applications
                .find(
                    query.organization_id,
                    query.project_id,
                    query.application_id,
                )
                .await
            {
                Ok(Some(value)) => Ok(Ok(value)),
                Ok(None) | Err(RepositoryError::NotFound) => Ok(Err(application_not_found())),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

#[derive(Debug, Clone)]
pub struct ListApplications {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub limit: Option<usize>,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for ListApplications {
    type Output = ApplicationResult<Vec<Application>>;
}

pub struct ListApplicationsHandler {
    applications: Arc<dyn IApplicationRepository>,
}

impl ListApplicationsHandler {
    pub fn new(applications: Arc<dyn IApplicationRepository>) -> Self {
        Self { applications }
    }
}

impl QueryHandler<ListApplications> for ListApplicationsHandler {
    fn execute(
        &self,
        query: ListApplications,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<Application>>>> {
        let applications = Arc::clone(&self.applications);
        Box::pin(async move {
            if let Err(error) = project(query.project_id, &query.resource_access) {
                return Ok(Err(error));
            }
            let limit = match list_limit(query.limit) {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            Ok(applications
                .list(query.organization_id, query.project_id, limit)
                .await
                .map_err(Into::into))
        })
    }
}

#[derive(Debug, Clone)]
pub struct GetApplicationRelease {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub release_id: ApplicationReleaseId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetApplicationRelease {
    type Output = ApplicationResult<ApplicationRelease>;
}

pub struct GetApplicationReleaseHandler {
    applications: Arc<dyn IApplicationRepository>,
}

impl GetApplicationReleaseHandler {
    pub fn new(applications: Arc<dyn IApplicationRepository>) -> Self {
        Self { applications }
    }
}

impl QueryHandler<GetApplicationRelease> for GetApplicationReleaseHandler {
    fn execute(
        &self,
        query: GetApplicationRelease,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<ApplicationRelease>>> {
        let applications = Arc::clone(&self.applications);
        Box::pin(async move {
            if let Err(error) = project(query.project_id, &query.resource_access) {
                return Ok(Err(error));
            }
            match applications
                .find_release(
                    query.organization_id,
                    query.project_id,
                    query.application_id,
                    query.release_id,
                )
                .await
            {
                Ok(Some(value)) => Ok(Ok(value)),
                Ok(None) | Err(RepositoryError::NotFound) => Ok(Err(release_not_found())),
                Err(error) => Ok(Err(error.into())),
            }
        })
    }
}

#[derive(Debug, Clone)]
pub struct ListApplicationReleases {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub application_id: ApplicationId,
    pub limit: Option<usize>,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for ListApplicationReleases {
    type Output = ApplicationResult<Vec<ApplicationRelease>>;
}

pub struct ListApplicationReleasesHandler {
    applications: Arc<dyn IApplicationRepository>,
}

impl ListApplicationReleasesHandler {
    pub fn new(applications: Arc<dyn IApplicationRepository>) -> Self {
        Self { applications }
    }
}

impl QueryHandler<ListApplicationReleases> for ListApplicationReleasesHandler {
    fn execute(
        &self,
        query: ListApplicationReleases,
        _context: a3s_boot::CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<ApplicationRelease>>>>
    {
        let applications = Arc::clone(&self.applications);
        Box::pin(async move {
            if let Err(error) = project(query.project_id, &query.resource_access) {
                return Ok(Err(error));
            }
            let limit = match list_limit(query.limit) {
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            let exists = match applications
                .find(
                    query.organization_id,
                    query.project_id,
                    query.application_id,
                )
                .await
            {
                Ok(Some(_)) => true,
                Ok(None) | Err(RepositoryError::NotFound) => false,
                Err(error) => return Ok(Err(error.into())),
            };
            if !exists {
                return Ok(Err(application_not_found()));
            }
            Ok(applications
                .list_releases(
                    query.organization_id,
                    query.project_id,
                    query.application_id,
                    limit,
                )
                .await
                .map_err(Into::into))
        })
    }
}

fn list_limit(limit: Option<usize>) -> ApplicationResult<usize> {
    let limit = limit.unwrap_or(DEFAULT_APPLICATION_LIST_LIMIT);
    if limit == 0 || limit > MAXIMUM_APPLICATION_LIST_LIMIT {
        return Err(ApplicationError::Invalid(format!(
            "Application list limit must be between 1 and {MAXIMUM_APPLICATION_LIST_LIMIT}"
        )));
    }
    Ok(limit)
}
