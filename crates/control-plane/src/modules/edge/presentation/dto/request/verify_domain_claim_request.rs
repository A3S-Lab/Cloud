use serde::Deserialize;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VerifyDomainClaimRequest {
    pub proof: String,
}

impl std::fmt::Debug for VerifyDomainClaimRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VerifyDomainClaimRequest")
            .field("proof", &"<redacted-proof>")
            .finish()
    }
}
