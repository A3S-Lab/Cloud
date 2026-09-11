use crate::modules::inference::domain::entities::InferenceRoute;
use crate::modules::inference::InferenceAccess;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use a3s_boot::Query;

pub const DEFAULT_INFERENCE_ROUTE_LIST_LIMIT: usize = 50;
pub const MAXIMUM_INFERENCE_ROUTE_LIST_LIMIT: usize = 100;

/// Bounded page of non-retired Inference route catalog heads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceRoutePage {
    pub routes: Vec<InferenceRoute>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ListInferenceRoutes {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub cursor: Option<String>,
    pub limit: usize,
    pub access: InferenceAccess,
}

impl Query for ListInferenceRoutes {
    type Output = ApplicationResult<InferenceRoutePage>;
}
