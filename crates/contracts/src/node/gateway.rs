use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::validate_sha256;

const MAX_GATEWAY_ACL_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewaySnapshot {
    pub schema: String,
    pub revision: u64,
    pub expected_revision: Option<u64>,
    pub snapshot_digest: String,
    pub acl: String,
}

impl GatewaySnapshot {
    pub const SCHEMA: &'static str = "a3s.cloud.gateway-snapshot.v1";

    pub fn new(
        revision: u64,
        expected_revision: Option<u64>,
        acl: impl Into<String>,
    ) -> Result<Self, String> {
        let acl = acl.into();
        let snapshot = Self {
            schema: Self::SCHEMA.into(),
            revision,
            expected_revision,
            snapshot_digest: digest_acl(&acl),
            acl,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA {
            return Err(format!(
                "unsupported Gateway snapshot schema {:?}",
                self.schema
            ));
        }
        if self.revision == 0 {
            return Err("Gateway snapshot revision must be positive".into());
        }
        if self
            .expected_revision
            .is_some_and(|expected| expected == 0 || expected >= self.revision)
        {
            return Err(
                "Gateway snapshot expected revision must be positive and precede its revision"
                    .into(),
            );
        }
        if self.acl.trim().is_empty()
            || self.acl.len() > MAX_GATEWAY_ACL_BYTES
            || self.acl.contains('\0')
        {
            return Err("Gateway snapshot ACL must contain 1 byte to 1 MiB without NUL".into());
        }
        validate_sha256("Gateway snapshot digest", &self.snapshot_digest)?;
        if self.snapshot_digest != digest_acl(&self.acl) {
            return Err("Gateway snapshot digest does not match its ACL".into());
        }
        Ok(())
    }
}

fn digest_acl(acl: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(acl.as_bytes()))
}
