use super::resource_access::authorize_environment;
use super::{
    DeveloperWorkflowAccess, DeveloperWorkflowEnvironmentScope, IDeveloperWorkflowEnvironmentPort,
};
use crate::modules::developer_workflows::domain::{AcceptedBuildPlan, IBuildPlanRepository};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    BuildPlanId, EnvironmentId, OrganizationId, ProjectId, SourceRevisionId,
};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

pub const DEFAULT_BUILD_PLAN_LIST_LIMIT: usize = 50;
pub const MAXIMUM_BUILD_PLAN_LIST_LIMIT: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetAcceptedBuildPlan {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub build_plan_id: BuildPlanId,
    pub access: DeveloperWorkflowAccess,
}

impl Query for GetAcceptedBuildPlan {
    type Output = ApplicationResult<AcceptedBuildPlan>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListAcceptedBuildPlans {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub source_revision_id: SourceRevisionId,
    pub limit: usize,
    pub access: DeveloperWorkflowAccess,
}

impl Query for ListAcceptedBuildPlans {
    type Output = ApplicationResult<Vec<AcceptedBuildPlan>>;
}

#[derive(Debug, Clone, Copy)]
struct BuildPlanReadScope {
    organization_id: OrganizationId,
    project_id: ProjectId,
    environment_id: EnvironmentId,
}

/// The single Application read authority for accepted BuildPlans.
///
/// Public adapters dispatch typed queries through this service instead of
/// reading the repository or repeating resource-visibility and Projects-owner checks.
pub struct BuildPlanQueryService {
    plans: Arc<dyn IBuildPlanRepository>,
    environments: Arc<dyn IDeveloperWorkflowEnvironmentPort>,
}

impl BuildPlanQueryService {
    pub fn new(
        plans: Arc<dyn IBuildPlanRepository>,
        environments: Arc<dyn IDeveloperWorkflowEnvironmentPort>,
    ) -> Self {
        Self {
            plans,
            environments,
        }
    }

    async fn get(
        &self,
        scope: BuildPlanReadScope,
        access: &DeveloperWorkflowAccess,
        build_plan_id: BuildPlanId,
    ) -> ApplicationResult<AcceptedBuildPlan> {
        self.authorize(scope, access).await?;
        if build_plan_id.as_uuid().is_nil() {
            return Err(ApplicationError::Invalid(
                "BuildPlan identity is invalid".into(),
            ));
        }
        let plan = self
            .plans
            .find(
                scope.organization_id,
                scope.project_id,
                scope.environment_id,
                build_plan_id,
            )
            .await?
            .ok_or_else(|| ApplicationError::NotFound("BuildPlan not found".into()))?;
        validate_plan_scope(&plan, scope, Some(build_plan_id), None)?;
        Ok(plan)
    }

    async fn list(
        &self,
        scope: BuildPlanReadScope,
        access: &DeveloperWorkflowAccess,
        source_revision_id: SourceRevisionId,
        limit: usize,
    ) -> ApplicationResult<Vec<AcceptedBuildPlan>> {
        self.authorize(scope, access).await?;
        if source_revision_id.as_uuid().is_nil() {
            return Err(ApplicationError::Invalid(
                "BuildPlan Source revision identity is invalid".into(),
            ));
        }
        if limit == 0 || limit > MAXIMUM_BUILD_PLAN_LIST_LIMIT {
            return Err(ApplicationError::Invalid(format!(
                "BuildPlan list limit must be between 1 and {MAXIMUM_BUILD_PLAN_LIST_LIMIT}"
            )));
        }
        let plans = self
            .plans
            .list_for_source(
                scope.organization_id,
                scope.project_id,
                scope.environment_id,
                source_revision_id,
                limit,
            )
            .await?;
        if plans.len() > limit {
            return Err(ApplicationError::Internal(
                "BuildPlan repository exceeded the requested page bound".into(),
            ));
        }
        for plan in &plans {
            validate_plan_scope(plan, scope, None, Some(source_revision_id))?;
        }
        if plans
            .windows(2)
            .any(|pair| !pair[0].canonical_cmp(&pair[1]).is_lt())
        {
            return Err(ApplicationError::Internal(
                "BuildPlan repository returned a non-canonical page".into(),
            ));
        }
        Ok(plans)
    }

    async fn authorize(
        &self,
        scope: BuildPlanReadScope,
        access: &DeveloperWorkflowAccess,
    ) -> ApplicationResult<()> {
        authorize_environment(
            self.environments.as_ref(),
            DeveloperWorkflowEnvironmentScope {
                organization_id: scope.organization_id,
                project_id: scope.project_id,
                environment_id: scope.environment_id,
            },
            access,
        )
        .await
    }
}

pub struct GetAcceptedBuildPlanHandler {
    queries: Arc<BuildPlanQueryService>,
}

impl GetAcceptedBuildPlanHandler {
    pub fn new(queries: Arc<BuildPlanQueryService>) -> Self {
        Self { queries }
    }
}

impl QueryHandler<GetAcceptedBuildPlan> for GetAcceptedBuildPlanHandler {
    fn execute(
        &self,
        query: GetAcceptedBuildPlan,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<AcceptedBuildPlan>>> {
        let queries = Arc::clone(&self.queries);
        Box::pin(async move {
            Ok(queries
                .get(
                    BuildPlanReadScope {
                        organization_id: query.organization_id,
                        project_id: query.project_id,
                        environment_id: query.environment_id,
                    },
                    &query.access,
                    query.build_plan_id,
                )
                .await)
        })
    }
}

pub struct ListAcceptedBuildPlansHandler {
    queries: Arc<BuildPlanQueryService>,
}

impl ListAcceptedBuildPlansHandler {
    pub fn new(queries: Arc<BuildPlanQueryService>) -> Self {
        Self { queries }
    }
}

impl QueryHandler<ListAcceptedBuildPlans> for ListAcceptedBuildPlansHandler {
    fn execute(
        &self,
        query: ListAcceptedBuildPlans,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<Vec<AcceptedBuildPlan>>>>
    {
        let queries = Arc::clone(&self.queries);
        Box::pin(async move {
            Ok(queries
                .list(
                    BuildPlanReadScope {
                        organization_id: query.organization_id,
                        project_id: query.project_id,
                        environment_id: query.environment_id,
                    },
                    &query.access,
                    query.source_revision_id,
                    query.limit,
                )
                .await)
        })
    }
}

fn validate_plan_scope(
    plan: &AcceptedBuildPlan,
    scope: BuildPlanReadScope,
    build_plan_id: Option<BuildPlanId>,
    source_revision_id: Option<SourceRevisionId>,
) -> ApplicationResult<()> {
    plan.validate().map_err(|error| {
        ApplicationError::Internal(format!(
            "BuildPlan repository returned invalid state: {error}"
        ))
    })?;
    if plan.organization_id != scope.organization_id
        || plan.project_id != scope.project_id
        || plan.environment_id != scope.environment_id
        || build_plan_id.is_some_and(|value| plan.id != value)
        || source_revision_id.is_some_and(|value| plan.source_revision_id != value)
    {
        return Err(ApplicationError::Internal(
            "BuildPlan repository returned state outside the requested scope".into(),
        ));
    }
    Ok(())
}
