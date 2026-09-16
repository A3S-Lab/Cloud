//! Fail-closed Gateway rate-shaping profile binding admission for publication
//! route intent snapshot compile.
//!
//! Mirrors Inference Edge binding admission: consumer-facing port, Edge adapter
//! against Edge/Gateway authority. Applications declare opaque profile refs only;
//! numeric token-bucket/GCRA policy stays in the Gateway catalog.

use crate::modules::edge::domain::{
    GatewayRateShapingAlgorithm, GatewayRateShapingGcra, GatewayRateShapingProfile,
    GatewayRateShapingTokenBucket,
};
use crate::modules::shared_kernel::domain::Sha256Digest;
use a3s_cloud_contracts::{
    GatewayRateShapingBoundParametersAcl, GatewayRateShapingBoundProfileAclProjection,
    GatewayRateShapingGcraAcl, GatewayRateShapingTokenBucketAcl,
};

/// Stable compile error prefix for rejected rate-shaping profile bindings.
pub const RATE_SHAPING_BINDING_INVALID: &str = "RATE_SHAPING_BINDING_INVALID";

/// Declare-only rate-shaping reference admitted against the Gateway catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationPublicationRateShapingBindingAdmissionRequest {
    pub profile_id: String,
    pub policy_revision_digest: Sha256Digest,
}

impl ApplicationPublicationRateShapingBindingAdmissionRequest {
    pub fn new(profile_id: impl Into<String>, policy_revision_digest: Sha256Digest) -> Self {
        Self {
            profile_id: profile_id.into(),
            policy_revision_digest,
        }
    }
}

/// Bound catalog material ready for Gateway snapshot ACL emit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationPublicationRateShapingBoundProfile {
    pub profile: GatewayRateShapingProfile,
}

impl ApplicationPublicationRateShapingBoundProfile {
    pub fn into_acl_projection(self) -> GatewayRateShapingBoundProfileAclProjection {
        let shaping = match self.profile.algorithm {
            GatewayRateShapingAlgorithm::TokenBucket(GatewayRateShapingTokenBucket {
                capacity,
                refill_tokens_per_second,
            }) => GatewayRateShapingBoundParametersAcl::TokenBucket(GatewayRateShapingTokenBucketAcl {
                capacity,
                refill_tokens_per_second,
            }),
            GatewayRateShapingAlgorithm::Gcra(GatewayRateShapingGcra {
                emission_interval_nanos,
                burst_tolerance,
            }) => GatewayRateShapingBoundParametersAcl::Gcra(GatewayRateShapingGcraAcl {
                emission_interval_nanos,
                burst_tolerance,
            }),
        };
        GatewayRateShapingBoundProfileAclProjection {
            profile_id: self.profile.profile_id,
            policy_revision_digest: self.profile.policy_revision_digest.as_str().to_owned(),
            shaping,
        }
    }
}

/// Edge/Gateway-owned admission for publication route intent rate-shaping refs.
pub trait IApplicationPublicationRateShapingBindingAdmissionPort: Send + Sync {
    fn admit(
        &self,
        request: ApplicationPublicationRateShapingBindingAdmissionRequest,
    ) -> Result<ApplicationPublicationRateShapingBoundProfile, String>;
}
