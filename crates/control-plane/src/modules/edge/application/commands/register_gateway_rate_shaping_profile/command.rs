use crate::modules::edge::domain::{
    GatewayRateShapingAlgorithm, GatewayRateShapingProfile,
};
use crate::modules::shared_kernel::application::ApplicationResult;
use crate::modules::shared_kernel::domain::Sha256Digest;
use a3s_boot::Command;

/// Register (or replace) one Gateway-owned rate-shaping profile revision in the
/// process-local Edge catalog used by snapshot compile admission.
#[derive(Debug, Clone)]
pub struct RegisterGatewayRateShapingProfile {
    pub profile_id: String,
    pub policy_revision_digest: Sha256Digest,
    pub algorithm: GatewayRateShapingAlgorithm,
}

impl Command for RegisterGatewayRateShapingProfile {
    type Output = ApplicationResult<RegisterGatewayRateShapingProfileResult>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterGatewayRateShapingProfileResult {
    pub profile: GatewayRateShapingProfile,
}
