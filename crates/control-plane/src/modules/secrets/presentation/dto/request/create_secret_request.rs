use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateSecretRequest {
    pub name: String,
    pub value: String,
}

impl std::fmt::Debug for CreateSecretRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CreateSecretRequest")
            .field("name", &self.name)
            .field("value", &"<redacted-secret-plaintext>")
            .finish()
    }
}
