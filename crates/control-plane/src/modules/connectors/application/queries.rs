use super::resource_access::{environment, profile_not_found, revision_not_found};
use crate::modules::connectors::domain::{
    ConnectorProfile, ConnectorRecord, ConnectorRevision, IConnectorProfileRepository,
};
use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    ConnectorProfileId, ConnectorRevisionId, EnvironmentId, OrganizationId, ProjectId,
    RepositoryError,
};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

pub const DEFAULT_CONNECTOR_PROFILE_LIST_LIMIT: usize = 50;
pub const MAXIMUM_CONNECTOR_PROFILE_LIST_LIMIT: usize = 200;

#[derive(Debug, Clone)]
pub struct GetConnectorProfile {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub profile_id: ConnectorProfileId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetConnectorProfile {
    type Output = ApplicationResult<ConnectorRecord>;
}

pub struct GetConnectorProfileHandler {
    connectors: Arc<dyn IConnectorProfileRepository>,
}

impl GetConnectorProfileHandler {
    pub fn new(connectors: Arc<dyn IConnectorProfileRepository>) -> Self {
        Self { connectors }
    }
}

impl QueryHandler<GetConnectorProfile> for GetConnectorProfileHandler {
    fn execute(
        &self,
        query: GetConnectorProfile,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<ConnectorRecord>>> {
        let connectors = Arc::clone(&self.connectors);
        Box::pin(async move {
            if let Err(error) = environment(
                query.project_id,
                query.environment_id,
                &query.resource_access,
            ) {
                return Ok(Err(error));
            }
            Ok(load_record(
                connectors.as_ref(),
                query.organization_id,
                query.project_id,
                query.environment_id,
                query.profile_id,
            )
            .await)
        })
    }
}

#[derive(Debug, Clone)]
pub struct ListConnectorProfiles {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub limit: usize,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for ListConnectorProfiles {
    type Output = ApplicationResult<Vec<ConnectorProfile>>;
}

pub struct ListConnectorProfilesHandler {
    connectors: Arc<dyn IConnectorProfileRepository>,
}

impl ListConnectorProfilesHandler {
    pub fn new(connectors: Arc<dyn IConnectorProfileRepository>) -> Self {
        Self { connectors }
    }
}

impl QueryHandler<ListConnectorProfiles> for ListConnectorProfilesHandler {
    fn execute(
        &self,
        query: ListConnectorProfiles,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<ConnectorProfile>>>>
    {
        let connectors = Arc::clone(&self.connectors);
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
            Ok(connectors
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
pub struct GetConnectorRevision {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub profile_id: ConnectorProfileId,
    pub revision_id: ConnectorRevisionId,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetConnectorRevision {
    type Output = ApplicationResult<ConnectorRevision>;
}

pub struct GetConnectorRevisionHandler {
    connectors: Arc<dyn IConnectorProfileRepository>,
}

impl GetConnectorRevisionHandler {
    pub fn new(connectors: Arc<dyn IConnectorProfileRepository>) -> Self {
        Self { connectors }
    }
}

impl QueryHandler<GetConnectorRevision> for GetConnectorRevisionHandler {
    fn execute(
        &self,
        query: GetConnectorRevision,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<ConnectorRevision>>> {
        let connectors = Arc::clone(&self.connectors);
        Box::pin(async move {
            if let Err(error) = environment(
                query.project_id,
                query.environment_id,
                &query.resource_access,
            ) {
                return Ok(Err(error));
            }
            Ok(
                match connectors
                    .find_revision(
                        query.organization_id,
                        query.project_id,
                        query.environment_id,
                        query.profile_id,
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
pub struct ListConnectorRevisions {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub profile_id: ConnectorProfileId,
    pub limit: usize,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for ListConnectorRevisions {
    type Output = ApplicationResult<Vec<ConnectorRevision>>;
}

pub struct ListConnectorRevisionsHandler {
    connectors: Arc<dyn IConnectorProfileRepository>,
}

impl ListConnectorRevisionsHandler {
    pub fn new(connectors: Arc<dyn IConnectorProfileRepository>) -> Self {
        Self { connectors }
    }
}

impl QueryHandler<ListConnectorRevisions> for ListConnectorRevisionsHandler {
    fn execute(
        &self,
        query: ListConnectorRevisions,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<ConnectorRevision>>>>
    {
        let connectors = Arc::clone(&self.connectors);
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
            match connectors
                .find(
                    query.organization_id,
                    query.project_id,
                    query.environment_id,
                    query.profile_id,
                )
                .await
            {
                Ok(Some(_)) => {}
                Ok(None) | Err(RepositoryError::NotFound) => return Ok(Err(profile_not_found())),
                Err(error) => return Ok(Err(error.into())),
            }
            Ok(connectors
                .list_revisions(
                    query.organization_id,
                    query.project_id,
                    query.environment_id,
                    query.profile_id,
                    query.limit,
                )
                .await
                .map_err(Into::into))
        })
    }
}

fn validate_list_limit(limit: usize) -> ApplicationResult<()> {
    if !(1..=MAXIMUM_CONNECTOR_PROFILE_LIST_LIMIT).contains(&limit) {
        return Err(ApplicationError::Invalid(format!(
            "Connector profile list limit must be between 1 and {MAXIMUM_CONNECTOR_PROFILE_LIST_LIMIT}"
        )));
    }
    Ok(())
}

async fn load_record(
    connectors: &dyn IConnectorProfileRepository,
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    profile_id: ConnectorProfileId,
) -> ApplicationResult<ConnectorRecord> {
    let profile = match connectors
        .find(organization_id, project_id, environment_id, profile_id)
        .await
    {
        Ok(Some(value)) => value,
        Ok(None) | Err(RepositoryError::NotFound) => return Err(profile_not_found()),
        Err(error) => return Err(error.into()),
    };
    let revision = match connectors
        .find_revision(
            organization_id,
            project_id,
            environment_id,
            profile_id,
            profile.current_revision_id,
        )
        .await
    {
        Ok(Some(value)) => value,
        Ok(None) | Err(RepositoryError::NotFound) => {
            return Err(ApplicationError::Internal(
                "Connector profile current revision is missing".into(),
            ))
        }
        Err(error) => return Err(error.into()),
    };
    ConnectorRecord::new(profile, revision).map_err(ApplicationError::Internal)
}
