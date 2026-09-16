//! Gateway-owned bound rate-shaping profile ACL for Cloud → Gateway snapshots.
//!
//! Separate from declare-only `application_publication_route_intents`: intents keep
//! opaque profile id + digest only. This block carries bounded token-bucket or
//! GCRA scalars admitted from the Gateway catalog at snapshot compile.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

const MAX_PROFILE_ID_LEN: usize = 128;
const MAX_SAFE_ACL_INTEGER: u64 = 9_007_199_254_740_991;

/// Token-bucket scalars for Gateway rate shaping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayRateShapingTokenBucketAcl {
    pub capacity: u64,
    pub refill_tokens_per_second: u64,
}

/// GCRA scalars for Gateway rate shaping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayRateShapingGcraAcl {
    pub emission_interval_nanos: u64,
    pub burst_tolerance: u64,
}

/// Bound algorithm parameters emitted after catalog admission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "algorithm", rename_all = "snake_case")]
pub enum GatewayRateShapingBoundParametersAcl {
    TokenBucket(GatewayRateShapingTokenBucketAcl),
    Gcra(GatewayRateShapingGcraAcl),
}

/// One Gateway catalog profile bound into snapshot ACL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayRateShapingBoundProfileAclProjection {
    pub profile_id: String,
    pub policy_revision_digest: String,
    pub shaping: GatewayRateShapingBoundParametersAcl,
}

impl GatewayRateShapingBoundProfileAclProjection {
    pub fn validate(&self) -> Result<(), String> {
        validate_token(&self.profile_id, "profile_id", MAX_PROFILE_ID_LEN)?;
        if !valid_sha256(&self.policy_revision_digest) {
            return Err("gateway rate shaping bound profile digest is invalid".into());
        }
        match &self.shaping {
            GatewayRateShapingBoundParametersAcl::TokenBucket(bucket) => {
                require_positive_u64(bucket.capacity, "token_bucket.capacity")?;
                require_positive_u64(
                    bucket.refill_tokens_per_second,
                    "token_bucket.refill_tokens_per_second",
                )?;
            }
            GatewayRateShapingBoundParametersAcl::Gcra(gcra) => {
                require_positive_u64(gcra.emission_interval_nanos, "gcra.emission_interval_nanos")?;
                require_positive_u64(gcra.burst_tolerance, "gcra.burst_tolerance")?;
            }
        }
        Ok(())
    }
}

/// Render bound rate-shaping profile ACL blocks for one Gateway snapshot.
///
/// Empty input renders nothing. Non-empty profiles are sorted by `profile_id`
/// then digest for digest stability.
pub fn render_gateway_rate_shaping_bound_profile_acl_blocks(
    profiles: &[GatewayRateShapingBoundProfileAclProjection],
) -> Result<String, String> {
    if profiles.is_empty() {
        return Ok(String::new());
    }
    let mut ordered = profiles.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        left.profile_id
            .cmp(&right.profile_id)
            .then(left.policy_revision_digest.cmp(&right.policy_revision_digest))
    });
    let mut seen = HashSet::new();
    let mut out = String::from("application_publication_rate_shaping_bound_profiles {\n");
    for profile in ordered {
        profile.validate()?;
        let key = (
            profile.profile_id.as_str(),
            profile.policy_revision_digest.as_str(),
        );
        if !seen.insert(key) {
            return Err(
                "gateway rate shaping bound profile ids+digests must be unique within one snapshot"
                    .into(),
            );
        }
        out.push_str(&render_one_profile(profile)?);
    }
    out.push_str("}\n");
    Ok(out)
}

fn render_one_profile(
    profile: &GatewayRateShapingBoundProfileAclProjection,
) -> Result<String, String> {
    let shaping = match &profile.shaping {
        GatewayRateShapingBoundParametersAcl::TokenBucket(bucket) => format!(
            "    token_bucket {{\n      capacity = {}\n      refill_tokens_per_second = {}\n    }}\n",
            bucket.capacity, bucket.refill_tokens_per_second
        ),
        GatewayRateShapingBoundParametersAcl::Gcra(gcra) => format!(
            "    gcra {{\n      emission_interval_nanos = {}\n      burst_tolerance = {}\n    }}\n",
            gcra.emission_interval_nanos, gcra.burst_tolerance
        ),
    };
    Ok(format!(
        "  profiles \"{}\" {{\n    policy_revision_digest = \"{}\"\n{shaping}  }}\n",
        escape_acl_string(&profile.profile_id),
        escape_acl_string(&profile.policy_revision_digest),
    ))
}

fn require_positive_u64(value: u64, field: &str) -> Result<(), String> {
    if value == 0 || value > MAX_SAFE_ACL_INTEGER {
        return Err(format!(
            "gateway rate shaping bound profile {field} must be a positive integer up to {MAX_SAFE_ACL_INTEGER}"
        ));
    }
    Ok(())
}

fn validate_token(value: &str, field: &str, max_len: usize) -> Result<(), String> {
    if value.is_empty() || value.len() > max_len || value.trim() != value {
        return Err(format!(
            "gateway rate shaping bound profile {field} must be a non-empty canonical token up to {max_len} characters"
        ));
    }
    if value.contains('\n') || value.contains('\r') || value.contains('\0') {
        return Err(format!(
            "gateway rate shaping bound profile {field} must not contain control characters"
        ));
    }
    Ok(())
}

fn valid_sha256(value: &str) -> bool {
    let Some(hexadecimal) = value.strip_prefix("sha256:") else {
        return false;
    };
    hexadecimal.len() == 64
        && hexadecimal
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn escape_acl_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(byte: u8) -> String {
        format!("sha256:{}", format!("{:02x}", byte).repeat(32))
    }

    fn token_bucket_profile() -> GatewayRateShapingBoundProfileAclProjection {
        GatewayRateShapingBoundProfileAclProjection {
            profile_id: "public-api-default".into(),
            policy_revision_digest: digest(0xbb),
            shaping: GatewayRateShapingBoundParametersAcl::TokenBucket(
                GatewayRateShapingTokenBucketAcl {
                    capacity: 120,
                    refill_tokens_per_second: 60,
                },
            ),
        }
    }

    #[test]
    fn empty_profiles_render_nothing() {
        assert_eq!(
            render_gateway_rate_shaping_bound_profile_acl_blocks(&[]).unwrap(),
            ""
        );
    }

    #[test]
    fn renders_token_bucket_bound_profile() {
        let rendered =
            render_gateway_rate_shaping_bound_profile_acl_blocks(&[token_bucket_profile()]).unwrap();
        assert!(rendered.starts_with("application_publication_rate_shaping_bound_profiles {"));
        assert!(rendered.contains("profiles \"public-api-default\""));
        assert!(rendered.contains("capacity = 120"));
        assert!(rendered.contains("refill_tokens_per_second = 60"));
        assert!(rendered.contains("token_bucket {"));
    }

    #[test]
    fn renders_gcra_bound_profile() {
        let profile = GatewayRateShapingBoundProfileAclProjection {
            profile_id: "strict-gcra".into(),
            policy_revision_digest: digest(0xcc),
            shaping: GatewayRateShapingBoundParametersAcl::Gcra(GatewayRateShapingGcraAcl {
                emission_interval_nanos: 1_000_000,
                burst_tolerance: 5,
            }),
        };
        let rendered = render_gateway_rate_shaping_bound_profile_acl_blocks(&[profile]).unwrap();
        assert!(rendered.contains("gcra {"));
        assert!(rendered.contains("emission_interval_nanos = 1000000"));
        assert!(rendered.contains("burst_tolerance = 5"));
    }

    #[test]
    fn rejects_duplicate_profile_digest_pairs() {
        let left = token_bucket_profile();
        let right = token_bucket_profile();
        assert!(render_gateway_rate_shaping_bound_profile_acl_blocks(&[left, right]).is_err());
    }
}
