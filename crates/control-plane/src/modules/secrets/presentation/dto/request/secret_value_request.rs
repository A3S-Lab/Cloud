use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretValueRequest {
    pub value: String,
}

impl std::fmt::Debug for SecretValueRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SecretValueRequest")
            .field("value", &"<redacted-secret-plaintext>")
            .finish()
    }
}
