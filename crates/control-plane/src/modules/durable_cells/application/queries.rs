use super::resource_access::{application_not_found, environment, revision_not_found};
use crate::modules::durable_cells::domain::{
    DurableCellApplication, DurableCellApplicationRecord, DurableCellApplicationRevision,
    IDurableCellApplicationRepository,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    DurableCellApplicationId, DurableCellApplicationRevisionId, EnvironmentId, OrganizationId,
    ProjectId, RepositoryError,
};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

pub const DEFAULT_DURABLE_CELL_APPLICATION_LIST_LIMIT: usize = 50;
pub const MAXIMUM_DURABLE_CELL_APPLICATION_LIST_LIMIT: usize = 200;

#[derive(Debug, Clone)]
pub struct GetDurableCellApplication {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub application_id: DurableCellApplicationId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetDurableCellApplication {
    type Output = ApplicationResult<DurableCellApplicationRecord>;
}

pub struct GetDurableCellApplicationHandler {
    applications: Arc<dyn IDurableCellApplicationRepository>,
}

impl GetDurableCellApplicationHandler {
    pub fn new(applications: Arc<dyn IDurableCellApplicationRepository>) -> Self {
        Self { applications }
    }
}

impl QueryHandler<GetDurableCellApplication> for GetDurableCellApplicationHandler {
    fn execute(
        &self,
        query: GetDurableCellApplication,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<DurableCellApplicationRecord>>,
    > {
        let applications = Arc::clone(&self.applications);
        Box::pin(async move {
            if let Err(error) = environment(
                query.project_id,
                query.environment_id,
                &query.resource_access,
            ) {
                return Ok(Err(error));
            }
            Ok(load_record(
                applications.as_ref(),
                query.organization_id,
                query.project_id,
                query.environment_id,
                query.application_id,
            )
            .await)
        })
    }
}

#[derive(Debug, Clone)]
pub struct ListDurableCellApplications {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub limit: usize,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for ListDurableCellApplications {
    type Output = ApplicationResult<Vec<DurableCellApplication>>;
}

pub struct ListDurableCellApplicationsHandler {
    applications: Arc<dyn IDurableCellApplicationRepository>,
}

impl ListDurableCellApplicationsHandler {
    pub fn new(applications: Arc<dyn IDurableCellApplicationRepository>) -> Self {
        Self { applications }
    }
}

impl QueryHandler<ListDurableCellApplications> for ListDurableCellApplicationsHandler {
    fn execute(
        &self,
        query: ListDurableCellApplications,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<Vec<DurableCellApplication>>>,
    > {
        let applications = Arc::clone(&self.applications);
        Box::pin(async move {
            if let Err(error) = validate_list_limit(query.limit) {
                return Ok(Err(error));
            }
            if let Err(error) = environment(
                query.project_id,
                query.environment_id,
                &query.resource_access,
            ) {
                return Ok(Err(error));
            }
            Ok(applications
                .list(
                    query.organization_id,
                    query.project_id,
                    query.environment_id,
                    query.limit,
                )
                .await
                .map_err(Into::into))
        })
    }
}

#[derive(Debug, Clone)]
pub struct GetDurableCellApplicationRevision {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub application_id: DurableCellApplicationId,
    pub revision_id: DurableCellApplicationRevisionId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetDurableCellApplicationRevision {
    type Output = ApplicationResult<DurableCellApplicationRevision>;
}

pub struct GetDurableCellApplicationRevisionHandler {
    applications: Arc<dyn IDurableCellApplicationRepository>,
}

impl GetDurableCellApplicationRevisionHandler {
    pub fn new(applications: Arc<dyn IDurableCellApplicationRepository>) -> Self {
        Self { applications }
    }
}

impl QueryHandler<GetDurableCellApplicationRevision> for GetDurableCellApplicationRevisionHandler {
    fn execute(
        &self,
        query: GetDurableCellApplicationRevision,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<DurableCellApplicationRevision>>,
    > {
        let applications = Arc::clone(&self.applications);
        Box::pin(async move {
            if let Err(error) = environment(
                query.project_id,
                query.environment_id,
                &query.resource_access,
            ) {
                return Ok(Err(error));
            }
            Ok(
                match applications
                    .find_revision(
                        query.organization_id,
                        query.project_id,
                        query.environment_id,
                        query.application_id,
                        query.revision_id,
                    )
                    .await
                {
                    Ok(Some(value)) => Ok(value),
                    Ok(None) | Err(RepositoryError::NotFound) => Err(revision_not_found()),
                    Err(error) => Err(error.into()),
                },
            )
        })
    }
}

#[derive(Debug, Clone)]
pub struct ListDurableCellApplicationRevisions {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub application_id: DurableCellApplicationId,
    pub limit: usize,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for ListDurableCellApplicationRevisions {
    type Output = ApplicationResult<Vec<DurableCellApplicationRevision>>;
}

pub struct ListDurableCellApplicationRevisionsHandler {
    applications: Arc<dyn IDurableCellApplicationRepository>,
}

impl ListDurableCellApplicationRevisionsHandler {
    pub fn new(applications: Arc<dyn IDurableCellApplicationRepository>) -> Self {
        Self { applications }
    }
}

impl QueryHandler<ListDurableCellApplicationRevisions>
    for ListDurableCellApplicationRevisionsHandler
{
    fn execute(
        &self,
        query: ListDurableCellApplicationRevisions,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<Vec<DurableCellApplicationRevision>>>,
    > {
        let applications = Arc::clone(&self.applications);
        Box::pin(async move {
            if let Err(error) = validate_list_limit(query.limit) {
                return Ok(Err(error));
            }
            if let Err(error) = environment(
                query.project_id,
                query.environment_id,
                &query.resource_access,
            ) {
                return Ok(Err(error));
            }
            match applications
                .find(
                    query.organization_id,
                    query.project_id,
                    query.environment_id,
                    query.application_id,
                )
                .await
            {
                Ok(Some(_)) => {}
                Ok(None) | Err(RepositoryError::NotFound) => {
                    return Ok(Err(application_not_found()))
                }
                Err(error) => return Ok(Err(error.into())),
            }
            Ok(applications
                .list_revisions(
                    query.organization_id,
                    query.project_id,
                    query.environment_id,
                    query.application_id,
                    query.limit,
                )
                .await
                .map_err(Into::into))
        })
    }
}

fn validate_list_limit(limit: usize) -> ApplicationResult<()> {
    if !(1..=MAXIMUM_DURABLE_CELL_APPLICATION_LIST_LIMIT).contains(&limit) {
        return Err(ApplicationError::Invalid(format!(
            "Durable Cell application list limit must be between 1 and {MAXIMUM_DURABLE_CELL_APPLICATION_LIST_LIMIT}"
        )));
    }
    Ok(())
}

async fn load_record(
    applications: &dyn IDurableCellApplicationRepository,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    application_id: DurableCellApplicationId,
) -> ApplicationResult<DurableCellApplicationRecord> {
    let application = match applications
        .find(organization_id, project_id, environment_id, application_id)
        .await
    {
        Ok(Some(value)) => value,
        Ok(None) | Err(RepositoryError::NotFound) => return Err(application_not_found()),
        Err(error) => return Err(error.into()),
    };
    let revision = match applications
        .find_revision(
            organization_id,
            project_id,
            environment_id,
            application_id,
            application.current_revision_id,
        )
        .await
    {
        Ok(Some(value)) => value,
        Ok(None) | Err(RepositoryError::NotFound) => {
            return Err(ApplicationError::Internal(
                "Durable Cell application current revision is missing".into(),
            ))
        }
        Err(error) => return Err(error.into()),
    };
    DurableCellApplicationRecord::new(application, revision).map_err(ApplicationError::Internal)
}
