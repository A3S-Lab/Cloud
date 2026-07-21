use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::OrganizationId;
use crate::modules::sources::domain::GithubConnection;
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct GetGithubConnection {
    pub organization_id: OrganizationId,
}

impl Query for GetGithubConnection {
    type Output = ApplicationResult<GithubConnection>;
}
