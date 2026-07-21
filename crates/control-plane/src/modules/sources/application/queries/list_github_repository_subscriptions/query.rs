use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{EnvironmentId, OrganizationId, ProjectId};
use crate::modules::sources::domain::GithubRepositorySubscription;
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct ListGithubRepositorySubscriptions {
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub environment_id: EnvironmentId,
}

impl Query for ListGithubRepositorySubscriptions {
    type Output = ApplicationResult<Vec<GithubRepositorySubscription>>;
}
