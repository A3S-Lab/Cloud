use crate::modules::identity::application::queries::resolve_partner_subject::PartnerSubjectLinkView;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId};
use a3s_boot::Query;

#[derive(Debug, Clone)]
pub struct ListPartnerSubjectLinks {
    pub organization_id: OrganizationId,
    pub principal_id: PrincipalId,
    pub provider_key: Option<String>,
}

impl Query for ListPartnerSubjectLinks {
    type Output = ApplicationResult<Vec<PartnerSubjectLinkView>>;
}
