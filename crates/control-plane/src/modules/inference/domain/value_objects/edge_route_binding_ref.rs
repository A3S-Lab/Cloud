//! Immutable Edge binding reference owned by Inference route revisions.
//!
//! Edge validates and owns applied Gateway state. Inference only stores the
//! same-environment binding identity needed for publication acknowledgement.

use crate::modules::shared_kernel::domain::{DomainClaimId, GatewayScopeId};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

const MAX_SAFE_ACL_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_HOSTNAME_LEN: usize = 253;
const MAX_PATH_PREFIX_LEN: usize = 2048;

/// Same-environment Edge binding identity carried by every InferenceRoute head.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EdgeRouteBindingRef {
    pub domain_claim_id: DomainClaimId,
    pub gateway_scope_id: GatewayScopeId,
    pub hostname: String,
    pub path_prefix: String,
    pub binding_generation: u64,
}

impl EdgeRouteBindingRef {
    pub fn new(
        domain_claim_id: DomainClaimId,
        gateway_scope_id: GatewayScopeId,
        hostname: impl Into<String>,
        path_prefix: impl Into<String>,
        binding_generation: u64,
    ) -> Result<Self, String> {
        let hostname = hostname.into().trim().to_ascii_lowercase();
        validate_hostname(&hostname)?;
        let path_prefix = path_prefix.into();
        validate_path_prefix(&path_prefix)?;
        let binding = Self {
            domain_claim_id,
            gateway_scope_id,
            hostname,
            path_prefix,
            binding_generation,
        };
        binding.validate()?;
        Ok(binding)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.domain_claim_id.as_uuid().is_nil() || self.gateway_scope_id.as_uuid().is_nil() {
            return Err("inference edge binding requires non-nil domain claim and gateway scope".into());
        }
        if self.binding_generation == 0 || self.binding_generation > MAX_SAFE_ACL_INTEGER {
            return Err(format!(
                "inference edge binding_generation must be in 1..={MAX_SAFE_ACL_INTEGER}"
            ));
        }
        validate_hostname(&self.hostname)?;
        validate_path_prefix(&self.path_prefix)?;
        Ok(())
    }
}

fn validate_hostname(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > MAX_HOSTNAME_LEN
        || value.ends_with('.')
        || value.parse::<IpAddr>().is_ok()
        || value.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label.bytes().all(|byte| {
                    byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
                })
        })
    {
        return Err("inference edge binding hostname must be a canonical DNS name".into());
    }
    Ok(())
}

fn validate_path_prefix(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > MAX_PATH_PREFIX_LEN
        || !value.starts_with('/')
        || value.contains(['\0', '\r', '\n', '`', '?', '#'])
        || value.contains("//")
        || value
            .split('/')
            .any(|segment| matches!(segment, "." | ".."))
    {
        return Err(
            "inference edge binding path_prefix must be a canonical absolute URL path".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_nil_identities_and_invalid_hostname() {
        assert!(EdgeRouteBindingRef::new(
            DomainClaimId::new(),
            GatewayScopeId::new(),
            "api.example.com",
            "/v1",
            1,
        )
        .is_ok());
        assert!(EdgeRouteBindingRef::new(
            DomainClaimId::from_uuid(uuid::Uuid::nil()),
            GatewayScopeId::new(),
            "api.example.com",
            "/v1",
            1,
        )
        .is_err());
        assert!(EdgeRouteBindingRef::new(
            DomainClaimId::new(),
            GatewayScopeId::new(),
            "127.0.0.1",
            "/v1",
            1,
        )
        .is_err());
        assert!(EdgeRouteBindingRef::new(
            DomainClaimId::new(),
            GatewayScopeId::new(),
            "api.example.com",
            "v1",
            1,
        )
        .is_err());
    }
}
