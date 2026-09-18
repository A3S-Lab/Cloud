use crate::modules::identity::domain::entities::DirectoryMembershipProjectionBinding;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId};
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub enum DirectoryMembershipProjectionListFilter {
    SubjectRef(String),
    PrincipalId(PrincipalId),
}

#[derive(Debug, Clone)]
pub struct ListDirectoryMembershipProjections {
    pub organization_id: OrganizationId,
    pub filter: DirectoryMembershipProjectionListFilter,
}

impl Query for ListDirectoryMembershipProjections {
    type Output = ApplicationResult<Vec<DirectoryMembershipProjectionBinding>>;
}
