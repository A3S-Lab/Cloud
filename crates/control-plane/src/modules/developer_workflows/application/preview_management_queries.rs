use super::resource_access::authorize_environment;
use super::{
    DeveloperWorkflowAccess, DeveloperWorkflowEnvironmentScope, IDeveloperWorkflowEnvironmentPort,
};
use crate::modules::developer_workflows::domain::{
    AcceptedPullRequestPreviewPolicyRevision, IPullRequestPreviewPolicyRepository,
    IPullRequestPreviewProjectionRepository, PullRequestPreview,
    MAX_DEVELOPER_WORKFLOW_SAFE_INTEGER, MAX_PULL_REQUEST_PREVIEW_POLICY_REVISIONS_PAGE,
};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, PullRequestPreviewPolicyRevisionId,
    SourceSubscriptionId,
};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

pub const DEFAULT_PREVIEW_POLICY_REVISION_LIST_LIMIT: usize = 50;
pub const MAXIMUM_PREVIEW_POLICY_REVISION_LIST_LIMIT: usize =
    MAX_PULL_REQUEST_PREVIEW_POLICY_REVISIONS_PAGE;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetCurrentAcceptedPullRequestPreviewPolicyRevision {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub source_environment_id: EnvironmentId,
    pub source_subscription_id: SourceSubscriptionId,
    pub access: DeveloperWorkflowAccess,
}

impl Query for GetCurrentAcceptedPullRequestPreviewPolicyRevision {
    type Output = ApplicationResult<AcceptedPullRequestPreviewPolicyRevision>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetAcceptedPullRequestPreviewPolicyRevision {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub source_environment_id: EnvironmentId,
    pub source_subscription_id: SourceSubscriptionId,
    pub preview_policy_revision_id: PullRequestPreviewPolicyRevisionId,
    pub access: DeveloperWorkflowAccess,
}

impl Query for GetAcceptedPullRequestPreviewPolicyRevision {
    type Output = ApplicationResult<AcceptedPullRequestPreviewPolicyRevision>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListAcceptedPullRequestPreviewPolicyRevisions {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub source_environment_id: EnvironmentId,
    pub source_subscription_id: SourceSubscriptionId,
    pub limit: usize,
    pub access: DeveloperWorkflowAccess,
}

impl Query for ListAcceptedPullRequestPreviewPolicyRevisions {
    type Output = ApplicationResult<Vec<AcceptedPullRequestPreviewPolicyRevision>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetPullRequestPreview {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub source_environment_id: EnvironmentId,
    pub source_subscription_id: SourceSubscriptionId,
    pub pull_request_id: u64,
    pub access: DeveloperWorkflowAccess,
}

impl Query for GetPullRequestPreview {
    type Output = ApplicationResult<PullRequestPreview>;
}

#[derive(Debug, Clone, Copy)]
struct PreviewReadScope {
    organization_id: OrganizationId,
    project_id: ProjectId,
    source_environment_id: EnvironmentId,
}

/// The sole Application read authority for accepted Preview Policy revisions.
///
/// Public adapters dispatch typed queries through this service. They never
/// parse policy ACL, read the repository, or repeat visibility/owner policy.
pub struct PreviewPolicyQueryService {
    policies: Arc<dyn IPullRequestPreviewPolicyRepository>,
    environments: Arc<dyn IDeveloperWorkflowEnvironmentPort>,
}

impl PreviewPolicyQueryService {
    pub fn new(
        policies: Arc<dyn IPullRequestPreviewPolicyRepository>,
        environments: Arc<dyn IDeveloperWorkflowEnvironmentPort>,
    ) -> Self {
        Self {
            policies,
            environments,
        }
    }

    async fn get_current(
        &self,
        scope: PreviewReadScope,
        access: &DeveloperWorkflowAccess,
        source_subscription_id: SourceSubscriptionId,
    ) -> ApplicationResult<AcceptedPullRequestPreviewPolicyRevision> {
        self.authorize(scope, access).await?;
        validate_source_subscription_id(source_subscription_id)?;
        let revision = self
            .policies
            .find_current(
                scope.organization_id,
                scope.project_id,
                scope.source_environment_id,
                source_subscription_id,
            )
            .await?
            .ok_or_else(|| ApplicationError::NotFound("Preview Policy not found".into()))?;
        validate_policy_revision_scope(&revision, scope, source_subscription_id, None)?;
        Ok(revision)
    }

    async fn get_revision(
        &self,
        scope: PreviewReadScope,
        access: &DeveloperWorkflowAccess,
        source_subscription_id: SourceSubscriptionId,
        preview_policy_revision_id: PullRequestPreviewPolicyRevisionId,
    ) -> ApplicationResult<AcceptedPullRequestPreviewPolicyRevision> {
        self.authorize(scope, access).await?;
        validate_source_subscription_id(source_subscription_id)?;
        if preview_policy_revision_id.as_uuid().is_nil() {
            return Err(ApplicationError::Invalid(
                "Preview Policy revision identity is invalid".into(),
            ));
        }
        let revision = self
            .policies
            .find_revision(
                scope.organization_id,
                scope.project_id,
                scope.source_environment_id,
                source_subscription_id,
                preview_policy_revision_id,
            )
            .await?
            .ok_or_else(|| {
                ApplicationError::NotFound("Preview Policy revision not found".into())
            })?;
        validate_policy_revision_scope(
            &revision,
            scope,
            source_subscription_id,
            Some(preview_policy_revision_id),
        )?;
        Ok(revision)
    }

    async fn list_revisions(
        &self,
        scope: PreviewReadScope,
        access: &DeveloperWorkflowAccess,
        source_subscription_id: SourceSubscriptionId,
        limit: usize,
    ) -> ApplicationResult<Vec<AcceptedPullRequestPreviewPolicyRevision>> {
        self.authorize(scope, access).await?;
        validate_source_subscription_id(source_subscription_id)?;
        if limit == 0 || limit > MAXIMUM_PREVIEW_POLICY_REVISION_LIST_LIMIT {
            return Err(ApplicationError::Invalid(format!(
                "Preview Policy revision list limit must be between 1 and {MAXIMUM_PREVIEW_POLICY_REVISION_LIST_LIMIT}"
            )));
        }
        let revisions = self
            .policies
            .list_revisions(
                scope.organization_id,
                scope.project_id,
                scope.source_environment_id,
                source_subscription_id,
                limit,
            )
            .await?;
        if revisions.len() > limit {
            return Err(ApplicationError::Internal(
                "Preview Policy repository exceeded the requested page bound".into(),
            ));
        }
        if revisions.is_empty() {
            return Err(ApplicationError::NotFound(
                "Preview Policy not found".into(),
            ));
        }
        for revision in &revisions {
            validate_policy_revision_scope(revision, scope, source_subscription_id, None)?;
        }
        if revisions
            .first()
            .is_some_and(|revision| revision.revision_number != 1)
            || revisions
                .windows(2)
                .any(|pair| pair[0].revision_number.checked_add(1) != Some(pair[1].revision_number))
        {
            return Err(ApplicationError::Internal(
                "Preview Policy repository returned a non-canonical revision page".into(),
            ));
        }
        Ok(revisions)
    }

    async fn authorize(
        &self,
        scope: PreviewReadScope,
        access: &DeveloperWorkflowAccess,
    ) -> ApplicationResult<()> {
        authorize_environment(
            self.environments.as_ref(),
            DeveloperWorkflowEnvironmentScope {
                organization_id: scope.organization_id,
                project_id: scope.project_id,
                environment_id: scope.source_environment_id,
            },
            access,
        )
        .await
    }
}

/// The sole Application read authority for the current pull-request Preview.
///
/// The service revalidates restored aggregate state and exact scope before a
/// public projection is allowed to observe it.
pub struct PullRequestPreviewQueryService {
    previews: Arc<dyn IPullRequestPreviewProjectionRepository>,
    environments: Arc<dyn IDeveloperWorkflowEnvironmentPort>,
}

impl PullRequestPreviewQueryService {
    pub fn new(
        previews: Arc<dyn IPullRequestPreviewProjectionRepository>,
        environments: Arc<dyn IDeveloperWorkflowEnvironmentPort>,
    ) -> Self {
        Self {
            previews,
            environments,
        }
    }

    async fn get(
        &self,
        scope: PreviewReadScope,
        access: &DeveloperWorkflowAccess,
        source_subscription_id: SourceSubscriptionId,
        pull_request_id: u64,
    ) -> ApplicationResult<PullRequestPreview> {
        authorize_environment(
            self.environments.as_ref(),
            DeveloperWorkflowEnvironmentScope {
                organization_id: scope.organization_id,
                project_id: scope.project_id,
                environment_id: scope.source_environment_id,
            },
            access,
        )
        .await?;
        validate_source_subscription_id(source_subscription_id)?;
        if pull_request_id == 0 || pull_request_id > MAX_DEVELOPER_WORKFLOW_SAFE_INTEGER {
            return Err(ApplicationError::Invalid(
                "pull-request identity must be a portable positive integer".into(),
            ));
        }
        let preview = self
            .previews
            .find_preview(
                scope.organization_id,
                scope.project_id,
                scope.source_environment_id,
                source_subscription_id,
                pull_request_id,
            )
            .await?
            .ok_or_else(|| ApplicationError::NotFound("Pull-request Preview not found".into()))?;
        validate_preview_scope(&preview, scope, source_subscription_id, pull_request_id)?;
        Ok(preview)
    }
}

pub struct GetCurrentAcceptedPullRequestPreviewPolicyRevisionHandler {
    queries: Arc<PreviewPolicyQueryService>,
}

impl GetCurrentAcceptedPullRequestPreviewPolicyRevisionHandler {
    pub fn new(queries: Arc<PreviewPolicyQueryService>) -> Self {
        Self { queries }
    }
}

impl QueryHandler<GetCurrentAcceptedPullRequestPreviewPolicyRevision>
    for GetCurrentAcceptedPullRequestPreviewPolicyRevisionHandler
{
    fn execute(
        &self,
        query: GetCurrentAcceptedPullRequestPreviewPolicyRevision,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<AcceptedPullRequestPreviewPolicyRevision>>,
    > {
        let queries = Arc::clone(&self.queries);
        Box::pin(async move {
            Ok(queries
                .get_current(
                    scope_from_policy_query(&query),
                    &query.access,
                    query.source_subscription_id,
                )
                .await)
        })
    }
}

pub struct GetAcceptedPullRequestPreviewPolicyRevisionHandler {
    queries: Arc<PreviewPolicyQueryService>,
}

impl GetAcceptedPullRequestPreviewPolicyRevisionHandler {
    pub fn new(queries: Arc<PreviewPolicyQueryService>) -> Self {
        Self { queries }
    }
}

impl QueryHandler<GetAcceptedPullRequestPreviewPolicyRevision>
    for GetAcceptedPullRequestPreviewPolicyRevisionHandler
{
    fn execute(
        &self,
        query: GetAcceptedPullRequestPreviewPolicyRevision,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<AcceptedPullRequestPreviewPolicyRevision>>,
    > {
        let queries = Arc::clone(&self.queries);
        Box::pin(async move {
            Ok(queries
                .get_revision(
                    PreviewReadScope {
                        organization_id: query.organization_id,
                        project_id: query.project_id,
                        source_environment_id: query.source_environment_id,
                    },
                    &query.access,
                    query.source_subscription_id,
                    query.preview_policy_revision_id,
                )
                .await)
        })
    }
}

pub struct ListAcceptedPullRequestPreviewPolicyRevisionsHandler {
    queries: Arc<PreviewPolicyQueryService>,
}

impl ListAcceptedPullRequestPreviewPolicyRevisionsHandler {
    pub fn new(queries: Arc<PreviewPolicyQueryService>) -> Self {
        Self { queries }
    }
}

impl QueryHandler<ListAcceptedPullRequestPreviewPolicyRevisions>
    for ListAcceptedPullRequestPreviewPolicyRevisionsHandler
{
    fn execute(
        &self,
        query: ListAcceptedPullRequestPreviewPolicyRevisions,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<
        'static,
        a3s_boot::Result<ApplicationResult<Vec<AcceptedPullRequestPreviewPolicyRevision>>>,
    > {
        let queries = Arc::clone(&self.queries);
        Box::pin(async move {
            Ok(queries
                .list_revisions(
                    PreviewReadScope {
                        organization_id: query.organization_id,
                        project_id: query.project_id,
                        source_environment_id: query.source_environment_id,
                    },
                    &query.access,
                    query.source_subscription_id,
                    query.limit,
                )
                .await)
        })
    }
}

pub struct GetPullRequestPreviewHandler {
    queries: Arc<PullRequestPreviewQueryService>,
}

impl GetPullRequestPreviewHandler {
    pub fn new(queries: Arc<PullRequestPreviewQueryService>) -> Self {
        Self { queries }
    }
}

impl QueryHandler<GetPullRequestPreview> for GetPullRequestPreviewHandler {
    fn execute(
        &self,
        query: GetPullRequestPreview,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<PullRequestPreview>>> {
        let queries = Arc::clone(&self.queries);
        Box::pin(async move {
            Ok(queries
                .get(
                    PreviewReadScope {
                        organization_id: query.organization_id,
                        project_id: query.project_id,
                        source_environment_id: query.source_environment_id,
                    },
                    &query.access,
                    query.source_subscription_id,
                    query.pull_request_id,
                )
                .await)
        })
    }
}

fn scope_from_policy_query(
    query: &GetCurrentAcceptedPullRequestPreviewPolicyRevision,
) -> PreviewReadScope {
    PreviewReadScope {
        organization_id: query.organization_id,
        project_id: query.project_id,
        source_environment_id: query.source_environment_id,
    }
}

fn validate_source_subscription_id(
    source_subscription_id: SourceSubscriptionId,
) -> ApplicationResult<()> {
    if source_subscription_id.as_uuid().is_nil() {
        return Err(ApplicationError::Invalid(
            "Preview source subscription identity is invalid".into(),
        ));
    }
    Ok(())
}

fn validate_policy_revision_scope(
    revision: &AcceptedPullRequestPreviewPolicyRevision,
    scope: PreviewReadScope,
    source_subscription_id: SourceSubscriptionId,
    preview_policy_revision_id: Option<PullRequestPreviewPolicyRevisionId>,
) -> ApplicationResult<()> {
    revision.validate().map_err(|error| {
        ApplicationError::Internal(format!(
            "Preview Policy repository returned invalid state: {error}"
        ))
    })?;
    if revision.organization_id != scope.organization_id
        || revision.project_id != scope.project_id
        || revision.source_environment_id != scope.source_environment_id
        || revision.source_subscription_id != source_subscription_id
        || preview_policy_revision_id.is_some_and(|value| revision.id != value)
    {
        return Err(ApplicationError::Internal(
            "Preview Policy repository returned state outside the requested scope".into(),
        ));
    }
    Ok(())
}

fn validate_preview_scope(
    preview: &PullRequestPreview,
    scope: PreviewReadScope,
    source_subscription_id: SourceSubscriptionId,
    pull_request_id: u64,
) -> ApplicationResult<()> {
    preview.validate().map_err(|error| {
        ApplicationError::Internal(format!(
            "Preview repository returned invalid state: {error}"
        ))
    })?;
    let authority = &preview.policy_authority;
    if authority.policy.organization_id != scope.organization_id
        || authority.policy.project_id != scope.project_id
        || authority.source_environment_id != scope.source_environment_id
        || authority.policy.source_subscription_id != source_subscription_id
        || preview.pull_request_id != pull_request_id
    {
        return Err(ApplicationError::Internal(
            "Preview repository returned state outside the requested scope".into(),
        ));
    }
    Ok(())
}
