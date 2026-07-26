use super::DomainClaimResponse;
use crate::modules::edge::domain::DomainClaim;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainClaimMutationResponse {
    #[serde(flatten)]
    pub claim: DomainClaimResponse,
    pub replayed: bool,
}

impl DomainClaimMutationResponse {
    pub fn new(claim: DomainClaim, replayed: bool) -> Self {
        Self {
            claim: claim.into(),
            replayed,
        }
    }
}
