//! Gateway-owned rate-shaping profile catalog facts.
//!
//! Applications declare opaque `(profile_id, policy_revision_digest)` only.
//! Edge/Gateway owns bounded token-bucket or GCRA scalars and admits them at
//! snapshot compile.

use crate::modules::shared_kernel::domain::Sha256Digest;

const MAX_PROFILE_ID_LEN: usize = 128;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// Token-bucket shaping parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayRateShapingTokenBucket {
    pub capacity: u64,
    pub refill_tokens_per_second: u64,
}

/// GCRA shaping parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayRateShapingGcra {
    pub emission_interval_nanos: u64,
    pub burst_tolerance: u64,
}

/// Boring algorithm choice for one catalog revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GatewayRateShapingAlgorithm {
    TokenBucket(GatewayRateShapingTokenBucket),
    Gcra(GatewayRateShapingGcra),
}

/// One Gateway catalog rate-shaping profile revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayRateShapingProfile {
    pub profile_id: String,
    pub policy_revision_digest: Sha256Digest,
    pub algorithm: GatewayRateShapingAlgorithm,
}

impl GatewayRateShapingProfile {
    pub fn new(
        profile_id: impl Into<String>,
        policy_revision_digest: Sha256Digest,
        algorithm: GatewayRateShapingAlgorithm,
    ) -> Result<Self, String> {
        let profile = Self {
            profile_id: profile_id.into(),
            policy_revision_digest,
            algorithm,
        };
        profile.validate()?;
        Ok(profile)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.profile_id.is_empty()
            || self.profile_id.len() > MAX_PROFILE_ID_LEN
            || self.profile_id.trim() != self.profile_id
        {
            return Err(format!(
                "gateway rate shaping profile_id must be a non-empty canonical token up to {MAX_PROFILE_ID_LEN} characters"
            ));
        }
        if self.profile_id.contains('\n')
            || self.profile_id.contains('\r')
            || self.profile_id.contains('\0')
        {
            return Err(
                "gateway rate shaping profile_id must not contain control characters".into(),
            );
        }
        Sha256Digest::parse(self.policy_revision_digest.as_str())?;
        match &self.algorithm {
            GatewayRateShapingAlgorithm::TokenBucket(bucket) => {
                require_positive(bucket.capacity, "token_bucket.capacity")?;
                require_positive(
                    bucket.refill_tokens_per_second,
                    "token_bucket.refill_tokens_per_second",
                )?;
            }
            GatewayRateShapingAlgorithm::Gcra(gcra) => {
                require_positive(gcra.emission_interval_nanos, "gcra.emission_interval_nanos")?;
                require_positive(gcra.burst_tolerance, "gcra.burst_tolerance")?;
            }
        }
        Ok(())
    }
}

fn require_positive(value: u64, field: &str) -> Result<(), String> {
    if value == 0 || value > MAX_SAFE_INTEGER {
        return Err(format!(
            "gateway rate shaping {field} must be a positive integer up to {MAX_SAFE_INTEGER}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(byte: u8) -> Sha256Digest {
        Sha256Digest::parse(format!("sha256:{}", format!("{:02x}", byte).repeat(32)))
            .expect("digest")
    }

    #[test]
    fn accepts_token_bucket_profile() {
        let profile = GatewayRateShapingProfile::new(
            "public-api-default",
            digest(0xaa),
            GatewayRateShapingAlgorithm::TokenBucket(GatewayRateShapingTokenBucket {
                capacity: 100,
                refill_tokens_per_second: 50,
            }),
        )
        .expect("profile");
        assert_eq!(profile.profile_id, "public-api-default");
    }

    #[test]
    fn rejects_empty_profile_id_and_zero_capacity() {
        assert!(GatewayRateShapingProfile::new(
            "",
            digest(0xaa),
            GatewayRateShapingAlgorithm::TokenBucket(GatewayRateShapingTokenBucket {
                capacity: 1,
                refill_tokens_per_second: 1,
            }),
        )
        .is_err());
        assert!(GatewayRateShapingProfile::new(
            "ok",
            digest(0xaa),
            GatewayRateShapingAlgorithm::TokenBucket(GatewayRateShapingTokenBucket {
                capacity: 0,
                refill_tokens_per_second: 1,
            }),
        )
        .is_err());
    }
}
