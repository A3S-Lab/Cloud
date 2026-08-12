use crate::modules::identity::domain::services::ResourceAccessEvaluator;
use crate::modules::search::domain::SearchResult;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::OrganizationId;
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct SearchResources {
    pub organization_id: OrganizationId,
    pub query: String,
    pub limit: u16,
    pub resource_access: ResourceAccessEvaluator,
}

impl Query for SearchResources {
    type Output = ApplicationResult<Vec<SearchResult>>;
}
