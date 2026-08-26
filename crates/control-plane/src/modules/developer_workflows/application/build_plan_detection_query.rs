use super::BuildPlanDetectionService;
use crate::modules::developer_workflows::domain::{BuildPlanDetection, SourceLayoutSnapshot};
use crate::modules::shared_kernel::application::{ApplicationError, ApplicationResult};
use a3s_boot::{CqrsContext, Query, QueryHandler};
use std::sync::Arc;

/// Detect reviewable BuildPlan proposals from one exact canonical source
/// layout without accepting a plan or starting owner lifecycle work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectBuildPlanProposals {
    pub layout: SourceLayoutSnapshot,
}

impl Query for DetectBuildPlanProposals {
    type Output = ApplicationResult<BuildPlanDetection>;
}

/// The single Application entry point for the built-in BuildPlan detector
/// set. Concrete detector ownership remains in Infrastructure and composition.
pub struct DetectBuildPlanProposalsHandler {
    detection: Arc<BuildPlanDetectionService>,
}

impl DetectBuildPlanProposalsHandler {
    pub fn new(detection: Arc<BuildPlanDetectionService>) -> Self {
        Self { detection }
    }
}

impl QueryHandler<DetectBuildPlanProposals> for DetectBuildPlanProposalsHandler {
    fn execute(
        &self,
        query: DetectBuildPlanProposals,
        _context: CqrsContext,
    ) -> a3s_boot::BoxFuture<'static, a3s_boot::Result<ApplicationResult<BuildPlanDetection>>> {
        let detection = Arc::clone(&self.detection);
        Box::pin(async move {
            Ok(detection
                .detect(&query.layout)
                .map_err(ApplicationError::Invalid))
        })
    }
}
