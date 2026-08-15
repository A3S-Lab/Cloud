use super::resource_access::{attempt_not_found, environment, revision_not_found};
use crate::modules::connectors::domain::{
    ConnectorExecutionAttemptCursor, ConnectorExecutionAttemptPage,
    ConnectorExecutionAttemptRecord, IConnectorExecutionAttemptRepository,
    IConnectorProfileRepository, MAXIMUM_CONNECTOR_EXECUTION_ATTEMPT_PAGE_SIZE,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ConnectorProfileId, ConnectorRevisionId, EnvironmentId, OrganizationId, ProjectId,
    RepositoryError,
};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct GetConnectorExecutionAttempt {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub profile_id: ConnectorProfileId,
    pub revision_id: ConnectorRevisionId,
    pub attempt_id: Uuid,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetConnectorExecutionAttempt {
    type Output = ApplicationResult<ConnectorExecutionAttemptRecord>;
}

pub struct GetConnectorExecutionAttemptHandler {
    attempts: Arc<dyn IConnectorExecutionAttemptRepository>,
}

impl GetConnectorExecutionAttemptHandler {
    pub fn new(attempts: Arc<dyn IConnectorExecutionAttemptRepository>) -> Self {
        Self { attempts }
    }
}

impl QueryHandler<GetConnectorExecutionAttempt> for GetConnectorExecutionAttemptHandler {
    fn execute(
        &self,
        query: GetConnectorExecutionAttempt,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ConnectorExecutionAttemptRecord>>,
    > {
        let attempts = Arc::clone(&self.attempts);
        Box::pin(async move {
            if let Err(error) = authorize_and_validate(
                query.organization_id,
                query.project_id,
                query.environment_id,
                query.profile_id,
                query.revision_id,
                Some(query.attempt_id),
                &query.resource_access,
            ) {
                return Ok(Err(error));
            }
            Ok(
                match attempts
                    .find(
                        query.organization_id,
                        query.project_id,
                        query.environment_id,
                        query.profile_id,
                        query.revision_id,
                        query.attempt_id,
                    )
                    .await
                {
                    Ok(Some(value)) => Ok(value),
                    Ok(None) | Err(RepositoryError::NotFound) => Err(attempt_not_found()),
                    Err(error) => Err(error.into()),
                },
            )
        })
    }
}

#[derive(Debug, Clone)]
pub struct ListUnresolvedConnectorExecutionAttempts {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub profile_id: ConnectorProfileId,
    pub revision_id: ConnectorRevisionId,
    pub after: Option<ConnectorExecutionAttemptCursor>,
    pub limit: usize,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for ListUnresolvedConnectorExecutionAttempts {
    type Output = ApplicationResult<ConnectorExecutionAttemptPage>;
}

pub struct ListUnresolvedConnectorExecutionAttemptsHandler {
    profiles: Arc<dyn IConnectorProfileRepository>,
    attempts: Arc<dyn IConnectorExecutionAttemptRepository>,
}

impl ListUnresolvedConnectorExecutionAttemptsHandler {
    pub fn new(
        profiles: Arc<dyn IConnectorProfileRepository>,
        attempts: Arc<dyn IConnectorExecutionAttemptRepository>,
    ) -> Self {
        Self { profiles, attempts }
    }
}

impl QueryHandler<ListUnresolvedConnectorExecutionAttempts>
    for ListUnresolvedConnectorExecutionAttemptsHandler
{
    fn execute(
        &self,
        query: ListUnresolvedConnectorExecutionAttempts,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<ConnectorExecutionAttemptPage>>,
    > {
        let profiles = Arc::clone(&self.profiles);
        let attempts = Arc::clone(&self.attempts);
        Box::pin(async move {
            if let Err(error) = authorize_and_validate(
                query.organization_id,
                query.project_id,
                query.environment_id,
                query.profile_id,
                query.revision_id,
                None,
                &query.resource_access,
            ) {
                return Ok(Err(error));
            }
            if query.limit == 0 || query.limit > MAXIMUM_CONNECTOR_EXECUTION_ATTEMPT_PAGE_SIZE {
                return Ok(Err(ApplicationError::Invalid(format!(
                    "Connector execution attempt limit must be between 1 and {MAXIMUM_CONNECTOR_EXECUTION_ATTEMPT_PAGE_SIZE}"
                ))));
            }
            let after = match query
                .after
                .map(ConnectorExecutionAttemptCursor::validate)
                .transpose()
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(ApplicationError::Invalid(error))),
            };
            match profiles
                .find_revision(
                    query.organization_id,
                    query.project_id,
                    query.environment_id,
                    query.profile_id,
                    query.revision_id,
                )
                .await
            {
                Ok(Some(_)) => {}
                Ok(None) | Err(RepositoryError::NotFound) => return Ok(Err(revision_not_found())),
                Err(error) => return Ok(Err(error.into())),
            }
            let mut page = match attempts
                .list_unresolved_page(
                    query.organization_id,
                    query.project_id,
                    query.environment_id,
                    query.profile_id,
                    query.revision_id,
                    after,
                    query.limit + 1,
                )
                .await
            {
                Ok(value) => value,
                Err(error) => return Ok(Err(error.into())),
            };
            let next_cursor = (page.len() > query.limit)
                .then(|| ConnectorExecutionAttemptCursor::after(&page[query.limit - 1].attempt));
            page.truncate(query.limit);
            Ok(Ok(ConnectorExecutionAttemptPage {
                attempts: page,
                next_cursor,
            }))
        })
    }
}

fn authorize_and_validate(
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    profile_id: ConnectorProfileId,
    revision_id: ConnectorRevisionId,
    attempt_id: Option<Uuid>,
    evaluator: &ResourceAccessEvaluator,
) -> ApplicationResult<()> {
    environment(project_id, environment_id, evaluator)?;
    if organization_id.as_uuid().is_nil()
        || project_id.as_uuid().is_nil()
        || environment_id.as_uuid().is_nil()
        || profile_id.as_uuid().is_nil()
        || revision_id.as_uuid().is_nil()
        || attempt_id.is_some_and(|value| value.is_nil())
    {
        return Err(ApplicationError::Invalid(
            "Connector execution attempt identity is invalid".into(),
        ));
    }
    Ok(())
}
