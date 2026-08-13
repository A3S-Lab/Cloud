use crate::modules::identity::domain::entities::{ApiToken, ExternalIdentityLink};
use crate::modules::identity::domain::value_objects::OidcProviderKey;
use crate::modules::shared_kernel::application::ApplicationResult;
use a3s_boot::Command;
use uuid::Uuid;
use zeroize::Zeroizing;

pub struct CompleteOidcFlow {
    pub provider_key: OidcProviderKey,
    pub code: Zeroizing<String>,
    pub state: Zeroizing<String>,
    pub nonce: Zeroizing<String>,
    pub pkce_verifier: Zeroizing<String>,
    pub request_id: Uuid,
}

impl Command for CompleteOidcFlow {
    type Output = ApplicationResult<CompleteOidcFlowResult>;
}

pub enum CompleteOidcFlowResult {
    Linked(ExternalIdentityLink),
    LoggedIn {
        api_token: ApiToken,
        credential: Zeroizing<String>,
    },
}
