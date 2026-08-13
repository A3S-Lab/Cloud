use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::projects::domain::entities::ProjectAttributionProfile;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    OrganizationId, ProjectAttributionProfileId, ProjectId,
};
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct GetProjectAttribution {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub attribution_profile_id: Option<ProjectAttributionProfileId>,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for GetProjectAttribution {
    type Output = ApplicationResult<ProjectAttributionProfile>;
}
