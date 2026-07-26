use super::ApiTokenReadResponse;
use crate::modules::identity::application::commands::create_api_token::CreateApiTokenResult;
use crate::modules::identity::application::commands::revoke_api_token::RevokeApiTokenResult;
use crate::modules::identity::domain::entities::ApiToken;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiTokenResponse {
    #[serde(flatten)]
    pub token: ApiTokenReadResponse,
    pub replayed: bool,
}

impl ApiTokenResponse {
    pub fn new(token: ApiToken, replayed: bool) -> Self {
        Self {
            token: token.into(),
            replayed,
        }
    }
}

impl From<CreateApiTokenResult> for ApiTokenResponse {
    fn from(result: CreateApiTokenResult) -> Self {
        Self::new(result.api_token, result.replayed)
    }
}

impl From<RevokeApiTokenResult> for ApiTokenResponse {
    fn from(result: RevokeApiTokenResult) -> Self {
        Self::new(result.api_token, result.replayed)
    }
}
