use crate::modules::projects::domain::entities::ProjectAttributionProfile;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{
    OrganizationId, ProjectAttributionProfileId, ProjectId,
};
use a3s_boot::Query;
use crate::modules::projects::application::ProjectAccess;

#[derive(Debug, Clone)]
pub struct GetProjectAttribution {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub attribution_profile_id: Option<ProjectAttributionProfileId>,
    pub access: ProjectAccess,
}

impl Query for GetProjectAttribution {
    type Output = ApplicationResult<ProjectAttributionProfile>;
}
