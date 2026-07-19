use super::{SecretListItemResponse, SecretVersionResponse};
use crate::modules::secrets::application::SecretDetails;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretDetailsResponse {
    #[serde(flatten)]
    pub secret: SecretListItemResponse,
    pub versions: Vec<SecretVersionResponse>,
}

impl From<SecretDetails> for SecretDetailsResponse {
    fn from(details: SecretDetails) -> Self {
        Self {
            secret: details.secret.into(),
            versions: details
                .versions
                .into_iter()
                .map(SecretVersionResponse::from)
                .collect(),
        }
    }
}
