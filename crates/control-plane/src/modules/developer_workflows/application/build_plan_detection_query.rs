use super::resource_access::authorize_environment;
use super::{
    BuildPlanDetectionService, BuildPlanSourceLayoutError, BuildPlanSourceLayoutRequest,
    DeveloperWorkflowAccess, DeveloperWorkflowEnvironmentScope, IBuildPlanSourceLayoutPort,
    IDeveloperWorkflowEnvironmentPort,
};
use crate::modules::developer_workflows::domain::BuildPlanDetection;
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use crate::modules::shared_kernel::domain::{
    EnvironmentId, OrganizationId, ProjectId, SourceRevisionId,
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
    pub access: DeveloperWorkflowAccess,
}

impl Query for DetectBuildPlanProposals {
    type Output = ApplicationResult<BuildPlanDetection>;
}

/// The single Application entry point for the built-in BuildPlan detector
/// set. Concrete detector ownership remains in Infrastructure and composition.
pub struct DetectBuildPlanProposalsHandler {
    detection: Arc<BuildPlanDetectionService>,
    source_layouts: Arc<dyn IBuildPlanSourceLayoutPort>,
    environments: Arc<dyn IDeveloperWorkflowEnvironmentPort>,
}

impl DetectBuildPlanProposalsHandler {
    pub fn new(
        detection: Arc<BuildPlanDetectionService>,
        source_layouts: Arc<dyn IBuildPlanSourceLayoutPort>,
        environments: Arc<dyn IDeveloperWorkflowEnvironmentPort>,
    ) -> Self {
        Self {
            detection,
            source_layouts,
            environments,
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
        let environments = Arc::clone(&self.environments);
        Box::pin(async move {
            let result = async move {
                authorize_environment(
                    environments.as_ref(),
                    DeveloperWorkflowEnvironmentScope {
                        organization_id: query.organization_id,
                        project_id: query.project_id,
                        environment_id: query.environment_id,
                    },
                    &query.access,
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
