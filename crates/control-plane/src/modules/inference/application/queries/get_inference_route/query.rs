use crate::modules::inference::domain::entities::InferenceRoute;
use crate::modules::inference::InferenceAccess;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    EnvironmentId, InferenceRouteId, OrganizationId, ProjectId,
};
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct GetInferenceRoute {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
    pub route_id: InferenceRouteId,
    pub access: InferenceAccess,
}

impl Query for GetInferenceRoute {
    type Output = ApplicationResult<InferenceRoute>;
}
