use super::authorization::authorize_environment_action;
use super::{
    DeveloperWorkflowAction, DeveloperWorkflowEnvironmentAccess,
    IDeveloperWorkflowAuthorizationPort,
};
use crate::modules::developer_workflows::domain::{
    AcceptedWorkloadProfileRevision, IWorkloadProfileRepository,
    MAX_WORKLOAD_PROFILE_REVISIONS_PAGE,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, PrincipalId, ProjectId, WorkloadProfileId,
    WorkloadProfileRevisionId,
};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

pub const DEFAULT_WORKLOAD_PROFILE_REVISION_LIST_LIMIT: usize = 50;
pub const MAXIMUM_WORKLOAD_PROFILE_REVISION_LIST_LIMIT: usize = MAX_WORKLOAD_PROFILE_REVISIONS_PAGE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GetCurrentAcceptedWorkloadProfileRevision {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub workload_profile_id: WorkloadProfileId,
    pub principal_id: PrincipalId,
}

impl Query for GetCurrentAcceptedWorkloadProfileRevision {
    type Output = ApplicationResult<AcceptedWorkloadProfileRevision>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GetAcceptedWorkloadProfileRevision {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub workload_profile_id: WorkloadProfileId,
    pub workload_profile_revision_id: WorkloadProfileRevisionId,
    pub principal_id: PrincipalId,
}

impl Query for GetAcceptedWorkloadProfileRevision {
    type Output = ApplicationResult<AcceptedWorkloadProfileRevision>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListAcceptedWorkloadProfileRevisions {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub workload_profile_id: WorkloadProfileId,
    pub limit: usize,
    pub principal_id: PrincipalId,
}

impl Query for ListAcceptedWorkloadProfileRevisions {
    type Output = ApplicationResult<Vec<AcceptedWorkloadProfileRevision>>;
}

#[derive(Debug, Clone, Copy)]
struct WorkloadProfileReadScope {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
    principal_id: PrincipalId,
}

/// The single Application read authority for accepted WorkloadProfile revisions.
///
/// Public adapters dispatch typed queries through this service instead of
/// reading the revision repository or repeating Identity/Projects authorization.
pub struct WorkloadProfileQueryService {
    profiles: Arc<dyn IWorkloadProfileRepository>,
    authorization: Arc<dyn IDeveloperWorkflowAuthorizationPort>,
}

impl WorkloadProfileQueryService {
    pub fn new(
        profiles: Arc<dyn IWorkloadProfileRepository>,
        authorization: Arc<dyn IDeveloperWorkflowAuthorizationPort>,
    ) -> Self {
        Self {
            profiles,
            authorization,
        }
    }

    async fn get_current(
        &self,
        scope: WorkloadProfileReadScope,
        workload_profile_id: WorkloadProfileId,
    ) -> ApplicationResult<AcceptedWorkloadProfileRevision> {
        self.authorize(scope).await?;
        validate_profile_id(workload_profile_id)?;
        let revision = self
            .profiles
            .find_current(
                scope.organization_id,
                scope.project_id,
                scope.environment_id,
                workload_profile_id,
            )
            .await?
            .ok_or_else(|| ApplicationError::NotFound("WorkloadProfile not found".into()))?;
        validate_revision_scope(&revision, scope, workload_profile_id, None)?;
        Ok(revision)
    }

    async fn get_revision(
        &self,
        scope: WorkloadProfileReadScope,
        workload_profile_id: WorkloadProfileId,
        workload_profile_revision_id: WorkloadProfileRevisionId,
    ) -> ApplicationResult<AcceptedWorkloadProfileRevision> {
        self.authorize(scope).await?;
        validate_profile_id(workload_profile_id)?;
        if workload_profile_revision_id.as_uuid().is_nil() {
            return Err(ApplicationError::Invalid(
                "WorkloadProfile revision identity is invalid".into(),
            ));
        }
        let revision = self
            .profiles
            .find_revision(
                scope.organization_id,
                scope.project_id,
                scope.environment_id,
                workload_profile_id,
                workload_profile_revision_id,
            )
            .await?
            .ok_or_else(|| {
                ApplicationError::NotFound("WorkloadProfile revision not found".into())
            })?;
        validate_revision_scope(
            &revision,
            scope,
            workload_profile_id,
            Some(workload_profile_revision_id),
        )?;
        Ok(revision)
    }

    async fn list_revisions(
        &self,
        scope: WorkloadProfileReadScope,
        workload_profile_id: WorkloadProfileId,
        limit: usize,
    ) -> ApplicationResult<Vec<AcceptedWorkloadProfileRevision>> {
        self.authorize(scope).await?;
        validate_profile_id(workload_profile_id)?;
        if limit == 0 || limit > MAXIMUM_WORKLOAD_PROFILE_REVISION_LIST_LIMIT {
            return Err(ApplicationError::Invalid(format!(
                "WorkloadProfile revision list limit must be between 1 and {MAXIMUM_WORKLOAD_PROFILE_REVISION_LIST_LIMIT}"
            )));
        }
        let revisions = self
            .profiles
            .list_revisions(
                scope.organization_id,
                scope.project_id,
                scope.environment_id,
                workload_profile_id,
                limit,
            )
            .await?;
        if revisions.len() > limit {
            return Err(ApplicationError::Internal(
                "WorkloadProfile repository exceeded the requested page bound".into(),
            ));
        }
        if revisions.is_empty() {
            return Err(ApplicationError::NotFound(
                "WorkloadProfile not found".into(),
            ));
        }
        for revision in &revisions {
            validate_revision_scope(revision, scope, workload_profile_id, None)?;
        }
        if revisions
            .first()
            .is_some_and(|revision| revision.revision_number != 1)
            || revisions
                .windows(2)
                .any(|pair| pair[0].revision_number.checked_add(1) != Some(pair[1].revision_number))
        {
            return Err(ApplicationError::Internal(
                "WorkloadProfile repository returned a non-canonical revision page".into(),
            ));
        }
        Ok(revisions)
    }

    async fn authorize(&self, scope: WorkloadProfileReadScope) -> ApplicationResult<()> {
        authorize_environment_action(
            self.authorization.as_ref(),
            DeveloperWorkflowEnvironmentAccess {
                organization_id: scope.organization_id,
                project_id: scope.project_id,
                environment_id: scope.environment_id,
                principal_id: scope.principal_id,
                action: DeveloperWorkflowAction::ReadWorkloadProfile,
            },
        )
        .await
    }
}

pub struct GetCurrentAcceptedWorkloadProfileRevisionHandler {
    queries: Arc<WorkloadProfileQueryService>,
}

impl GetCurrentAcceptedWorkloadProfileRevisionHandler {
    pub fn new(queries: Arc<WorkloadProfileQueryService>) -> Self {
        Self { queries }
    }
}

impl QueryHandler<GetCurrentAcceptedWorkloadProfileRevision>
    for GetCurrentAcceptedWorkloadProfileRevisionHandler
{
    fn execute(
        &self,
        query: GetCurrentAcceptedWorkloadProfileRevision,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<AcceptedWorkloadProfileRevision>>,
    > {
        let queries = Arc::clone(&self.queries);
        Box::pin(async move {
            Ok(queries
                .get_current(
                    WorkloadProfileReadScope {
                        organization_id: query.organization_id,
                        project_id: query.project_id,
                        environment_id: query.environment_id,
                        principal_id: query.principal_id,
                    },
                    query.workload_profile_id,
                )
                .await)
        })
    }
}

pub struct GetAcceptedWorkloadProfileRevisionHandler {
    queries: Arc<WorkloadProfileQueryService>,
}

impl GetAcceptedWorkloadProfileRevisionHandler {
    pub fn new(queries: Arc<WorkloadProfileQueryService>) -> Self {
        Self { queries }
    }
}

impl QueryHandler<GetAcceptedWorkloadProfileRevision>
    for GetAcceptedWorkloadProfileRevisionHandler
{
    fn execute(
        &self,
        query: GetAcceptedWorkloadProfileRevision,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<AcceptedWorkloadProfileRevision>>,
    > {
        let queries = Arc::clone(&self.queries);
        Box::pin(async move {
            Ok(queries
                .get_revision(
                    WorkloadProfileReadScope {
                        organization_id: query.organization_id,
                        project_id: query.project_id,
                        environment_id: query.environment_id,
                        principal_id: query.principal_id,
                    },
                    query.workload_profile_id,
                    query.workload_profile_revision_id,
                )
                .await)
        })
    }
}

pub struct ListAcceptedWorkloadProfileRevisionsHandler {
    queries: Arc<WorkloadProfileQueryService>,
}

impl ListAcceptedWorkloadProfileRevisionsHandler {
    pub fn new(queries: Arc<WorkloadProfileQueryService>) -> Self {
        Self { queries }
    }
}

impl QueryHandler<ListAcceptedWorkloadProfileRevisions>
    for ListAcceptedWorkloadProfileRevisionsHandler
{
    fn execute(
        &self,
        query: ListAcceptedWorkloadProfileRevisions,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<Vec<AcceptedWorkloadProfileRevision>>>,
    > {
        let queries = Arc::clone(&self.queries);
        Box::pin(async move {
            Ok(queries
                .list_revisions(
                    WorkloadProfileReadScope {
                        organization_id: query.organization_id,
                        project_id: query.project_id,
                        environment_id: query.environment_id,
                        principal_id: query.principal_id,
                    },
                    query.workload_profile_id,
                    query.limit,
                )
                .await)
        })
    }
}

fn validate_profile_id(workload_profile_id: WorkloadProfileId) -> ApplicationResult<()> {
    if workload_profile_id.as_uuid().is_nil() {
        return Err(ApplicationError::Invalid(
            "WorkloadProfile identity is invalid".into(),
        ));
    }
    Ok(())
}

fn validate_revision_scope(
    revision: &AcceptedWorkloadProfileRevision,
    scope: WorkloadProfileReadScope,
    workload_profile_id: WorkloadProfileId,
    workload_profile_revision_id: Option<WorkloadProfileRevisionId>,
) -> ApplicationResult<()> {
    revision.validate().map_err(|error| {
        ApplicationError::Internal(format!(
            "WorkloadProfile repository returned invalid state: {error}"
        ))
    })?;
    if revision.organization_id != scope.organization_id
        || revision.project_id != scope.project_id
        || revision.environment_id != scope.environment_id
        || revision.profile_id != workload_profile_id
        || workload_profile_revision_id.is_some_and(|value| revision.id != value)
    {
        return Err(ApplicationError::Internal(
            "WorkloadProfile repository returned state outside the requested scope".into(),
        ));
    }
    Ok(())
}
