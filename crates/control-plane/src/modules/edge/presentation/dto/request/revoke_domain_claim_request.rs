use serde::Deserialize;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevokeDomainClaimRequest {
    pub reason: String,
}

impl std::fmt::Debug for RevokeDomainClaimRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RevokeDomainClaimRequest")
            .field("reason", &"<redacted-reason>")
            .finish()
    }
}
