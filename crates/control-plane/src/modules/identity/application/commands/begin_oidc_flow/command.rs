use crate::modules::identity::domain::entities::OidcFlowPurpose;
use crate::modules::identity::domain::value_objects::OidcProviderKey;
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::{OrganizationId, PrincipalId};
use a3s_boot::Command;
use chrono::{DateTime, Utc};
use zeroize::Zeroizing;

pub struct BeginOidcFlow {
    pub organization_id: OrganizationId,
    pub provider_key: OidcProviderKey,
    pub purpose: OidcFlowPurpose,
    pub principal_id: Option<PrincipalId>,
}

impl Command for BeginOidcFlow {
    type Output = ApplicationResult<BeginOidcFlowResult>;
}

pub struct BeginOidcFlowResult {
    pub authorization_url: String,
    pub nonce: Zeroizing<String>,
    pub pkce_verifier: Zeroizing<String>,
    pub expires_at: DateTime<Utc>,
}
