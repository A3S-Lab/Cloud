use super::GatewayScopeResponse;
use crate::modules::edge::domain::GatewayScope;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayScopeMutationResponse {
    #[serde(flatten)]
    pub scope: GatewayScopeResponse,
    pub replayed: bool,
}

impl GatewayScopeMutationResponse {
    pub fn new(scope: GatewayScope, replayed: bool) -> Self {
        Self {
            scope: scope.into(),
            replayed,
        }
    }
}
