use super::authorization::authorize_environment_action;
use super::{
    BuildPlanDetectionService, BuildPlanSourceLayoutError, BuildPlanSourceLayoutRequest,
    DeveloperWorkflowAction, DeveloperWorkflowEnvironmentAccess, IBuildPlanSourceLayoutPort,
    IDeveloperWorkflowAuthorizationPort,
};
use crate::modules::developer_workflows::domain::BuildPlanDetection;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, PrincipalId, ProjectId, SourceRevisionId,
};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

/// Detect reviewable BuildPlan proposals from one exact canonical source
/// layout without accepting a plan or starting owner lifecycle work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectBuildPlanProposals {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub source_revision_id: SourceRevisionId,
    pub principal_id: PrincipalId,
}

impl Query for DetectBuildPlanProposals {
    type Output = ApplicationResult<BuildPlanDetection>;
}

/// The single Application entry point for the built-in BuildPlan detector
/// set. Concrete detector ownership remains in Infrastructure and composition.
pub struct DetectBuildPlanProposalsHandler {
    detection: Arc<BuildPlanDetectionService>,
    source_layouts: Arc<dyn IBuildPlanSourceLayoutPort>,
    authorization: Arc<dyn IDeveloperWorkflowAuthorizationPort>,
}

impl DetectBuildPlanProposalsHandler {
    pub fn new(
        detection: Arc<BuildPlanDetectionService>,
        source_layouts: Arc<dyn IBuildPlanSourceLayoutPort>,
        authorization: Arc<dyn IDeveloperWorkflowAuthorizationPort>,
    ) -> Self {
        Self {
            detection,
            source_layouts,
            authorization,
        }
    }
}

impl QueryHandler<DetectBuildPlanProposals> for DetectBuildPlanProposalsHandler {
    fn execute(
        &self,
        query: DetectBuildPlanProposals,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<BuildPlanDetection>>> {
        let detection = Arc::clone(&self.detection);
        let source_layouts = Arc::clone(&self.source_layouts);
        let authorization = Arc::clone(&self.authorization);
        Box::pin(async move {
            let result = async move {
                authorize_environment_action(
                    authorization.as_ref(),
                    DeveloperWorkflowEnvironmentAccess {
                        organization_id: query.organization_id,
                        project_id: query.project_id,
                        environment_id: query.environment_id,
                        principal_id: query.principal_id,
                        action: DeveloperWorkflowAction::DetectBuildPlan,
                    },
                )
                .await?;
                let request = BuildPlanSourceLayoutRequest {
                    organization_id: query.organization_id,
                    project_id: query.project_id,
                    environment_id: query.environment_id,
                    source_revision_id: query.source_revision_id,
                };
                request.validate().map_err(ApplicationError::Invalid)?;
                let layout = source_layouts
                    .acquire(request)
                    .await
                    .map_err(map_source_layout_error)?
                    .ok_or_else(|| {
                        ApplicationError::NotFound(
                            "Developer Workflow source revision not found".into(),
                        )
                    })?;
                detection.detect(&layout).map_err(ApplicationError::Invalid)
            }
            .await;
            Ok(result)
        })
    }
}

fn map_source_layout_error(error: BuildPlanSourceLayoutError) -> ApplicationError {
    match error {
        BuildPlanSourceLayoutError::Invalid(message) => ApplicationError::Invalid(message),
        BuildPlanSourceLayoutError::Conflict => ApplicationError::Conflict(
            "Developer Workflow source layout conflicts with Sources authority".into(),
        ),
        BuildPlanSourceLayoutError::Unavailable(message) => ApplicationError::Unavailable(message),
        BuildPlanSourceLayoutError::Integrity(message) => ApplicationError::Conflict(message),
        BuildPlanSourceLayoutError::Storage(message) => ApplicationError::Internal(message),
    }
}
